use std::collections::BTreeSet;

use veritas_domain::{
	BudgetData, CategoryId, CivilDate, CoreError, Money, TransactionId, TransactionKind,
};

use crate::net_profit::labels;
use crate::{CategoryLabels, Classification, ClassifiedRow, Period};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Bucket {
	W2Income,
	InterestIncome,
	BusinessIncome,
	BusinessExpense,
	Charitable,
	Medical,
	Education,
	Hsa,
	RetirementContribution,
	TaxesWithheld,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
	Inflow,
	Outflow,
}

const BUCKETS: [Bucket; 10] = [
	Bucket::W2Income,
	Bucket::InterestIncome,
	Bucket::BusinessIncome,
	Bucket::BusinessExpense,
	Bucket::Charitable,
	Bucket::Medical,
	Bucket::Education,
	Bucket::Hsa,
	Bucket::RetirementContribution,
	Bucket::TaxesWithheld,
];

/// The bucket id that removes a row from its category's bucket.
const EXCLUDE: &str = "none";

impl Bucket {
	pub fn id(self) -> &'static str {
		match self {
			Bucket::W2Income => "w2_income",
			Bucket::InterestIncome => "interest_income",
			Bucket::BusinessIncome => "business_income",
			Bucket::BusinessExpense => "business_expense",
			Bucket::Charitable => "charitable",
			Bucket::Medical => "medical",
			Bucket::Education => "education",
			Bucket::Hsa => "hsa",
			Bucket::RetirementContribution => "retirement_contribution",
			Bucket::TaxesWithheld => "taxes_withheld",
		}
	}

	pub fn parse(id: &str) -> Option<Bucket> {
		BUCKETS.into_iter().find(|bucket| bucket.id() == id)
	}

	pub fn direction(self) -> Direction {
		match self {
			Bucket::W2Income | Bucket::InterestIncome | Bucket::BusinessIncome => Direction::Inflow,
			_ => Direction::Outflow,
		}
	}

	/// A row's signed contribution: positive adds to the bucket, so a refund
	/// in an outflow bucket reduces it.
	pub fn counted(self, amount: Money) -> Result<Money, CoreError> {
		match self.direction() {
			Direction::Inflow => Ok(amount),
			Direction::Outflow => amount.checked_neg(),
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
	Tag,
	Category,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TaxRow {
	pub row: TransactionId,
	pub date: CivilDate,
	/// `None` is a conflict: the row's tags name two or more buckets, so it
	/// is in no total and waits for the user to decide.
	pub bucket: Option<Bucket>,
	pub source: Source,
	pub amount: Money,
	pub counted: Option<Money>,
	/// The classified row's category, after `category_mapping`.
	pub category: Option<CategoryId>,
	pub parent: Option<TransactionId>,
}

impl TaxRow {
	pub fn bucket_id(&self) -> &'static str {
		self.bucket.map_or("conflict", Bucket::id)
	}
}

#[derive(Clone, Debug, PartialEq)]
pub struct BucketTotal {
	pub bucket: Bucket,
	/// May be negative when refunds exceed the year's amounts. Never clamped.
	pub total: Money,
	pub rows: Vec<TransactionId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TaxSummary {
	pub year: i32,
	/// Buckets with at least one counted row, by bucket id.
	pub buckets: Vec<BucketTotal>,
	/// Counted and conflicting rows, by bucket id, then date, then id.
	pub detail: Vec<TaxRow>,
}

impl TaxSummary {
	pub fn bucket(&self, bucket: Bucket) -> Option<&BucketTotal> {
		self.buckets.iter().find(|total| total.bucket == bucket)
	}

	pub fn conflicts(&self) -> impl Iterator<Item = &TaxRow> {
		self.detail.iter().filter(|row| row.bucket.is_none())
	}
}

/// The tags in a row's notes, parsed exactly as Actual's `extractTagsFromText`:
/// every `#` not preceded by `#`, then the characters up to whitespace or the
/// next `#`. `##x` is an escape. Duplicates are dropped, order is kept.
pub fn extract_tags(notes: &str) -> Vec<String> {
	let chars: Vec<char> = notes.chars().collect();
	let mut tags = Vec::new();
	let mut index = 0;
	while index < chars.len() {
		let after_hash = index > 0 && chars.get(index - 1) == Some(&'#');
		if chars.get(index) != Some(&'#') || after_hash {
			index += 1;
			continue;
		}
		let start = index + 1;
		let end = chars
			.iter()
			.skip(start)
			.position(|&c| c == '#' || is_js_whitespace(c))
			.map_or(chars.len(), |offset| start + offset);
		if end > start {
			let tag: String = chars.get(start..end).unwrap_or_default().iter().collect();
			if !tags.contains(&tag) {
				tags.push(tag);
			}
			index = end;
		} else {
			index += 1;
		}
	}
	tags
}

/// JavaScript's `\s`, which Actual's tag regex uses. It differs from Unicode
/// White_Space: it includes U+FEFF and excludes U+0085.
fn is_js_whitespace(c: char) -> bool {
	matches!(
		c,
		'\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
			..='\u{200A}'
				| '\u{2028}' | '\u{2029}'
				| '\u{202F}' | '\u{205F}'
				| '\u{3000}' | '\u{FEFF}'
	)
}

enum Assignment {
	Counted(Bucket, Source),
	Conflict,
	NotCounted,
}

fn assign(budget: &BudgetData, row: &ClassifiedRow) -> Assignment {
	let notes = budget
		.transactions
		.get(&row.id)
		.and_then(|transaction| transaction.notes.as_deref())
		.unwrap_or_default();
	let mapped: Vec<&str> = extract_tags(notes)
		.iter()
		.filter_map(|tag| budget.tax_tags.get(tag))
		.map(String::as_str)
		.collect();
	if mapped.contains(&EXCLUDE) {
		return Assignment::NotCounted;
	}
	let tagged: BTreeSet<&str> = mapped
		.into_iter()
		.filter(|id| Bucket::parse(id).is_some())
		.collect();
	match tagged.len() {
		0 => {}
		1 => {
			return tagged
				.first()
				.and_then(|id| Bucket::parse(id))
				.map_or(Assignment::NotCounted, |bucket| {
					Assignment::Counted(bucket, Source::Tag)
				});
		}
		_ => return Assignment::Conflict,
	}
	row.category
		.as_ref()
		.and_then(|category| budget.category_meta.get(category))
		.and_then(|meta| meta.tax_bucket.as_deref())
		.and_then(Bucket::parse)
		.map_or(Assignment::NotCounted, |bucket| {
			Assignment::Counted(bucket, Source::Category)
		})
}

fn eligible(row: &ClassifiedRow) -> bool {
	row.is_primary()
		&& matches!(
			row.kind(),
			TransactionKind::Income
				| TransactionKind::Expense
				| TransactionKind::DebtPrincipal
				| TransactionKind::SavingsContribution
		)
}

/// The US tax summary for one calendar year. It totals what the user tagged
/// and says nothing about what is deductible or taxable.
pub fn tax_summary(
	budget: &BudgetData,
	classification: &Classification,
	year: i32,
) -> Result<TaxSummary, CoreError> {
	let period = Period::new(CivilDate::new(year, 1, 1)?, CivilDate::new(year, 12, 31)?)?;
	let mut detail = Vec::new();
	for row in classification
		.within(period)
		.iter()
		.filter(|row| eligible(row))
	{
		let (bucket, source) = match assign(budget, row) {
			Assignment::Counted(bucket, source) => (Some(bucket), source),
			Assignment::Conflict => (None, Source::Tag),
			Assignment::NotCounted => continue,
		};
		detail.push(TaxRow {
			row: row.id.clone(),
			date: row.date,
			bucket,
			source,
			amount: row.amount,
			counted: bucket
				.map(|bucket| bucket.counted(row.amount))
				.transpose()?,
			category: row.category.clone(),
			parent: row.parent.clone(),
		});
	}
	detail.sort_by(|a, b| {
		a.bucket_id()
			.cmp(b.bucket_id())
			.then_with(|| a.date.cmp(&b.date))
			.then_with(|| a.row.cmp(&b.row))
	});

	let mut buckets: Vec<BucketTotal> = Vec::new();
	for row in &detail {
		let (Some(bucket), Some(counted)) = (row.bucket, row.counted) else {
			continue;
		};
		match buckets.iter_mut().find(|total| total.bucket == bucket) {
			Some(total) => {
				total.total = total.total.checked_add(counted)?;
				total.rows.push(row.row.clone());
			}
			None => buckets.push(BucketTotal {
				bucket,
				total: counted,
				rows: vec![row.row.clone()],
			}),
		}
	}
	Ok(TaxSummary {
		year,
		buckets,
		detail,
	})
}

/// `veritas-tax-summary-<year>.csv`.
pub fn summary_csv(summary: &TaxSummary) -> String {
	let mut csv = Csv::new(&[
		"tax_year",
		"bucket",
		"direction",
		"total_cents",
		"total",
		"transaction_count",
	]);
	for total in &summary.buckets {
		csv.row([
			Cell::Number(summary.year.to_string()),
			Cell::Text(total.bucket.id().to_owned()),
			Cell::Text(direction_id(total.bucket.direction()).to_owned()),
			Cell::Number(total.total.minor().to_string()),
			Cell::Number(total.total.to_string()),
			Cell::Number(total.rows.len().to_string()),
		]);
	}
	csv.finish()
}

/// `veritas-tax-detail-<year>.csv`.
pub fn detail_csv(budget: &BudgetData, summary: &TaxSummary) -> String {
	let mut csv = Csv::new(&[
		"tax_year",
		"bucket",
		"source",
		"date",
		"account",
		"payee",
		"category",
		"category_status",
		"amount_cents",
		"counted_cents",
		"notes",
		"transaction_id",
		"parent_id",
	]);
	for row in &summary.detail {
		let transaction = budget.transactions.get(&row.row);
		let account = transaction
			.and_then(|transaction| budget.accounts.get(&transaction.account))
			.map(|account| account.name.clone());
		let payee = transaction
			.and_then(|transaction| transaction.payee.as_ref())
			.and_then(|payee| budget.resolve_payee(payee))
			.and_then(|payee| budget.payees.get(payee))
			.map(|payee| {
				payee
					.transfer_account
					.as_ref()
					.and_then(|account| budget.accounts.get(account))
					.map_or_else(|| payee.name.clone(), |account| account.name.clone())
			});
		let category = row
			.category
			.as_ref()
			.and_then(|category| budget.categories.get(category))
			.map(|category| category.name.clone());
		let status = row
			.category
			.as_ref()
			.map_or("", |category| category_status(labels(budget, category)));
		let parent = row.parent.as_ref().map(ToString::to_string);
		csv.row([
			Cell::Number(summary.year.to_string()),
			Cell::Text(row.bucket_id().to_owned()),
			Cell::Text(
				match row.source {
					Source::Tag => "tag",
					Source::Category => "category",
				}
				.to_owned(),
			),
			Cell::Text(row.date.to_string()),
			Cell::Text(account.unwrap_or_default()),
			Cell::Text(payee.unwrap_or_default()),
			Cell::Text(category.unwrap_or_default()),
			Cell::Text(status.to_owned()),
			Cell::Number(row.amount.minor().to_string()),
			Cell::Number(
				row.counted
					.map(|counted| counted.minor().to_string())
					.unwrap_or_default(),
			),
			Cell::Text(
				transaction
					.and_then(|transaction| transaction.notes.clone())
					.unwrap_or_default(),
			),
			Cell::Text(row.row.to_string()),
			Cell::Text(parent.unwrap_or_default()),
		]);
	}
	csv.finish()
}

/// One word per category, the most serious first: a deleted category may
/// also be hidden, and an archived one usually is.
fn category_status(labels: CategoryLabels) -> &'static str {
	if labels.deleted {
		"deleted"
	} else if labels.archived {
		"archived"
	} else if labels.hidden {
		"hidden"
	} else {
		""
	}
}

fn direction_id(direction: Direction) -> &'static str {
	match direction {
		Direction::Inflow => "inflow",
		Direction::Outflow => "outflow",
	}
}

enum Cell {
	Text(String),
	Number(String),
}

/// UTF-8 without a BOM, LF line endings, RFC 4180 quoting, and text cells
/// that a spreadsheet would run as a formula prefixed with `'`.
struct Csv(String);

impl Csv {
	fn new(header: &[&str]) -> Self {
		let mut csv = Csv(String::new());
		csv.row(header.iter().map(|name| Cell::Number((*name).to_owned())));
		csv
	}

	fn row(&mut self, cells: impl IntoIterator<Item = Cell>) {
		for (index, cell) in cells.into_iter().enumerate() {
			if index > 0 {
				self.0.push(',');
			}
			let text = match cell {
				Cell::Number(text) => text,
				Cell::Text(text) if text.starts_with(['=', '+', '-', '@']) => format!("'{text}"),
				Cell::Text(text) => text,
			};
			if text.contains([',', '"', '\n', '\r']) {
				self.0.push('"');
				self.0.push_str(&text.replace('"', "\"\""));
				self.0.push('"');
			} else {
				self.0.push_str(&text);
			}
		}
		self.0.push('\n');
	}

	fn finish(self) -> String {
		self.0
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn tags_follow_actuals_grammar() {
		assert_eq!(extract_tags("#medical#rx"), ["medical", "rx"]);
		assert_eq!(extract_tags("##medical"), Vec::<String>::new());
		assert_eq!(extract_tags("###a #b"), ["b"]);
		assert_eq!(extract_tags("pharmacy #medical and #medical"), ["medical"]);
		assert_eq!(extract_tags("a#b c# #"), ["b"]);
		assert_eq!(extract_tags("#Medical #medical"), ["Medical", "medical"]);
		assert_eq!(extract_tags("#hsa\u{A0}#rx\u{FEFF}x"), ["hsa", "rx"]);
		assert_eq!(extract_tags("#a\u{85}b"), ["a\u{85}b"]);
		assert_eq!(extract_tags("#café!"), ["café!"]);
	}

	#[test]
	fn every_bucket_id_round_trips() {
		for bucket in BUCKETS {
			assert_eq!(Bucket::parse(bucket.id()), Some(bucket));
		}
		assert_eq!(Bucket::parse(EXCLUDE), None);
		assert_eq!(Bucket::parse("deductible"), None);
	}
}
