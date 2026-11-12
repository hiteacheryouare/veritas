//! The rows Actual's report query returns (`makeQuery` over AQL's
//! `transactions` table, with conditions compiled by `conditionsToAQL`), and
//! the live date ranges the report page resolves (`getLiveRange`).

use std::collections::BTreeMap;

use chrono::{Datelike, Days, Months, NaiveDate};
use veritas_domain::{
	AccountId, BudgetData, CategoryGroupId, CategoryId, CoreError, Money, PayeeId, SplitRole,
	Transaction,
};

use crate::custom_report::{CustomReport, Interval};
use crate::json::Json;

/// One grouped row: a sum of same-signed amounts for an interval, account,
/// payee, category and transfer account, as Actual's query returns it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryRow {
	/// `YYYY-MM` for Monthly, `YYYY` for Yearly, otherwise `YYYY-MM-DD`.
	pub date: String,
	pub category: Option<CategoryId>,
	pub category_hidden: bool,
	pub category_income: bool,
	pub category_group: Option<CategoryGroupId>,
	pub category_group_hidden: bool,
	pub account: Option<AccountId>,
	pub account_off_budget: bool,
	pub payee: Option<PayeeId>,
	pub transfer_account: Option<AccountId>,
	pub amount: Money,
}

pub(crate) fn unsupported(what: String) -> CoreError {
	CoreError::Invalid(format!("Veritas can't evaluate this report yet: {what}"))
}

pub(crate) fn parse_date(text: &str) -> Option<NaiveDate> {
	let parts: Vec<&str> = text.split('-').collect();
	let number = |index: usize, default: u32| {
		parts
			.get(index)
			.map_or(Some(default), |part| part.parse::<u32>().ok())
	};
	let year = parts.first()?.parse::<i32>().ok()?;
	NaiveDate::from_ymd_opt(year, number(1, 1)?, number(2, 1)?)
}

pub(crate) fn day(date: NaiveDate) -> String {
	date.format("%Y-%m-%d").to_string()
}

fn month_end(date: NaiveDate) -> NaiveDate {
	date.with_day(1)
		.and_then(|first| first.checked_add_months(Months::new(1)))
		.and_then(|next| next.pred_opt())
		.unwrap_or(date)
}

pub(crate) fn week_start(date: NaiveDate, first_day_of_week: u32) -> NaiveDate {
	let offset = (date.weekday().num_days_from_sunday() + 7 - first_day_of_week % 7) % 7;
	date - Days::new(u64::from(offset))
}

/// The first and last transaction dates, split parents counted, children not:
/// `q('transactions').options({splits: 'none'})`. Both are `today` when the
/// budget has no transactions.
pub fn transaction_date_bounds(budget: &BudgetData, today: NaiveDate) -> (NaiveDate, NaiveDate) {
	let dates = budget
		.transactions
		.values()
		.filter(|row| !row.tombstone && !matches!(row.split, SplitRole::Child { .. }))
		.map(|row| row.date.naive());
	let mut bounds: Option<(NaiveDate, NaiveDate)> = None;
	for date in dates {
		bounds = Some(match bounds {
			None => (date, date),
			Some((first, last)) => (first.min(date), last.max(date)),
		});
	}
	bounds.unwrap_or((today, today))
}

/// The report's start and end dates: stored ones for a static report, else
/// Actual's `getLiveRange` for its named range as of `today`.
pub fn report_range(
	report: &CustomReport,
	budget: &BudgetData,
	today: NaiveDate,
	first_day_of_week: u32,
) -> Result<(String, String), CoreError> {
	if report.date_static {
		return Ok((report.start_date.clone(), report.end_date.clone()));
	}
	let Some(name) = report.date_range.as_deref() else {
		return Ok((report.start_date.clone(), report.end_date.clone()));
	};
	let (earliest, latest) = transaction_date_bounds(budget, today);
	let clamp = |start: NaiveDate| day(start.max(earliest));
	let quarter = |month: NaiveDate| {
		let first = NaiveDate::from_ymd_opt(month.year(), (month.month0() / 3) * 3 + 1, 1);
		first.map(|first| (first, month_end(first + Months::new(2))))
	};
	let this_month = today.with_day(1).unwrap_or(today);
	let range =
		|start: Option<NaiveDate>, end: Option<NaiveDate>| -> Result<(String, String), CoreError> {
			match (start, end) {
				(Some(start), Some(end)) => Ok((clamp(start), day(end))),
				_ => Err(CoreError::Invalid(format!(
					"the date range {name:?} overflows"
				))),
			}
		};
	let months = |offset: u32, add: u32| {
		let start = this_month.checked_sub_months(Months::new(offset));
		let end = start
			.and_then(|start| start.checked_add_months(Months::new(add)))
			.map(month_end);
		start.zip(end).map(|(start, end)| (day(start), day(end)))
	};
	let weeks = |offset: u64, add: u64| {
		let start = week_start(today, first_day_of_week).checked_sub_days(Days::new(offset * 7));
		let end = start.and_then(|start| start.checked_add_days(Days::new(add * 7 + 6)));
		start.zip(end).map(|(start, end)| (day(start), day(end)))
	};
	let lag = |offset: u32| offset - u32::from(!report.include_current);
	let numeric = match name {
		"This month" => Some(months(0, 0)),
		"Last month" => Some(months(1, lag(1))),
		"Last 3 months" => Some(months(3, lag(3))),
		"Last 6 months" => Some(months(6, lag(6))),
		"Last 12 months" => Some(months(12, lag(12))),
		"This week" => Some(weeks(0, 0)),
		"Last week" => Some(weeks(1, u64::from(lag(1)))),
		_ => None,
	};
	if let Some(found) = numeric {
		return found
			.ok_or_else(|| CoreError::Invalid(format!("the date range {name:?} overflows")));
	}
	let year_start = |year: i32| NaiveDate::from_ymd_opt(year, 1, 1);
	match name {
		"Year to date" => range(year_start(today.year()), Some(today)),
		"Last year" => range(
			year_start(today.year() - 1),
			NaiveDate::from_ymd_opt(today.year() - 1, 12, 31),
		),
		"Prior year to date" => range(
			year_start(today.year() - 1),
			today.checked_sub_months(Months::new(12)),
		),
		"Current quarter" => {
			let (start, end) = quarter(this_month).unzip();
			range(start, end)
		}
		"Previous quarter" => {
			let (start, end) = this_month
				.checked_sub_months(Months::new(3))
				.and_then(quarter)
				.unzip();
			range(start, end)
		}
		"Last 30 days" => range(today.checked_sub_days(Days::new(29)), Some(today)),
		"All time" => Ok((day(earliest), day(latest))),
		other => Err(unsupported(format!("the date range {other:?}"))),
	}
}

/// What the report's queries can see of one live, non-parent row.
struct Fields<'a> {
	transaction: &'a Transaction,
	category: Option<&'a CategoryId>,
	category_group: Option<&'a CategoryGroupId>,
	payee: Option<&'a PayeeId>,
	transfer_account: Option<&'a AccountId>,
	account: Option<&'a AccountId>,
}

fn is_row(budget: &BudgetData, transaction: &Transaction) -> bool {
	!transaction.tombstone
		&& match &transaction.split {
			SplitRole::None => true,
			SplitRole::Parent => false,
			SplitRole::Child { parent } => budget
				.transactions
				.get(parent)
				.is_some_and(|parent| !parent.tombstone),
		}
}

fn fields<'a>(budget: &'a BudgetData, transaction: &'a Transaction) -> Fields<'a> {
	let live_account = |id: &AccountId| {
		budget
			.accounts
			.get(id)
			.filter(|account| !account.tombstone)
			.map(|account| &account.id)
	};
	let category = transaction
		.category
		.as_ref()
		.and_then(|id| budget.resolve_category(id))
		.and_then(|id| budget.categories.get(id))
		.filter(|category| !category.tombstone);
	let category_group = category
		.and_then(|category| budget.category_groups.get(&category.group))
		.filter(|group| !group.tombstone);
	let payee = transaction
		.payee
		.as_ref()
		.and_then(|id| budget.resolve_payee(id))
		.and_then(|id| budget.payees.get(id))
		.filter(|payee| !payee.tombstone)
		.filter(|payee| {
			payee
				.transfer_account
				.as_ref()
				.is_none_or(|account| live_account(account).is_some())
		});
	Fields {
		transaction,
		category: category.map(|category| &category.id),
		category_group: category_group.map(|group| &group.id),
		payee: payee.map(|payee| &payee.id),
		transfer_account: payee
			.and_then(|payee| payee.transfer_account.as_ref())
			.and_then(live_account),
		account: live_account(&transaction.account),
	}
}

fn interval_key(date: NaiveDate, interval: Interval) -> String {
	match interval {
		Interval::Monthly => date.format("%Y-%m").to_string(),
		Interval::Yearly => date.format("%Y").to_string(),
		Interval::Daily | Interval::Weekly => day(date),
	}
}

/// The date bound in the unit the query compares: whole months for Monthly,
/// whole years for Yearly, exact days otherwise (which need full dates).
fn bound(text: &str, interval: Interval) -> Result<i64, CoreError> {
	let date =
		parse_date(text).ok_or_else(|| CoreError::Invalid(format!("bad report date {text:?}")))?;
	Ok(match interval {
		Interval::Monthly => i64::from(date.year()) * 100 + i64::from(date.month()),
		Interval::Yearly => i64::from(date.year()),
		Interval::Daily | Interval::Weekly => {
			if text.len() != 10 {
				return Err(CoreError::Invalid(format!("bad date format {text:?}")));
			}
			i64::from(date.year()) * 10_000 + i64::from(date.month()) * 100 + i64::from(date.day())
		}
	})
}

fn date_in_unit(date: NaiveDate, interval: Interval) -> i64 {
	let (y, m, d) = (
		i64::from(date.year()),
		i64::from(date.month()),
		i64::from(date.day()),
	);
	match interval {
		Interval::Monthly => y * 100 + m,
		Interval::Yearly => y,
		Interval::Daily | Interval::Weekly => y * 10_000 + m * 100 + d,
	}
}

type GroupKey<'a> = (
	String,
	Option<&'a AccountId>,
	Option<&'a PayeeId>,
	Option<&'a CategoryId>,
	Option<&'a AccountId>,
);

/// Actual's two report queries: positive rows (`assets`) and negative rows
/// (`debts`), each grouped and summed.
pub fn report_rows(
	budget: &BudgetData,
	report: &CustomReport,
	start: &str,
	end: &str,
) -> Result<(Vec<QueryRow>, Vec<QueryRow>), CoreError> {
	let conditions = report
		.conditions
		.iter()
		.filter(|condition| condition.get("customName").is_none())
		.map(Condition::parse)
		.collect::<Result<Vec<_>, _>>()?
		.into_iter()
		.flatten()
		.collect::<Vec<_>>();
	let (low, high) = (bound(start, report.interval)?, bound(end, report.interval)?);
	let mut assets: BTreeMap<GroupKey, (Fields, Money)> = BTreeMap::new();
	let mut debts: BTreeMap<GroupKey, (Fields, Money)> = BTreeMap::new();
	for transaction in budget.transactions.values() {
		if !is_row(budget, transaction) {
			continue;
		}
		let date = transaction.date.naive();
		let unit = date_in_unit(date, report.interval);
		if unit < low || unit > high || transaction.amount == Money::ZERO {
			continue;
		}
		let row = fields(budget, transaction);
		let passes = |condition: &Condition| condition.matches(&row);
		let keep = if conditions.is_empty() {
			true
		} else if report.conditions_or {
			conditions.iter().any(passes)
		} else {
			conditions.iter().all(passes)
		};
		if !keep {
			continue;
		}
		let key = (
			interval_key(date, report.interval),
			row.account,
			row.payee,
			row.category,
			row.transfer_account,
		);
		let target = if transaction.amount > Money::ZERO {
			&mut assets
		} else {
			&mut debts
		};
		match target.get_mut(&key) {
			Some((_, sum)) => *sum = sum.checked_add(transaction.amount)?,
			None => {
				target.insert(key, (row, transaction.amount));
			}
		}
	}
	let finish = |groups: BTreeMap<GroupKey, (Fields, Money)>| {
		groups
			.into_iter()
			.map(|((date, ..), (row, amount))| {
				let category = row.category.and_then(|id| budget.categories.get(id));
				let group = row
					.category_group
					.and_then(|id| budget.category_groups.get(id));
				QueryRow {
					date,
					category: row.category.cloned(),
					category_hidden: category.is_some_and(|category| category.hidden),
					category_income: category.is_some_and(|category| category.is_income),
					category_group: row.category_group.cloned(),
					category_group_hidden: group.is_some_and(|group| group.hidden),
					account: row.account.cloned(),
					account_off_budget: row
						.account
						.and_then(|id| budget.accounts.get(id))
						.is_some_and(|account| account.off_budget),
					payee: row.payee.cloned(),
					transfer_account: row.transfer_account.cloned(),
					amount,
				}
			})
			.collect::<Vec<_>>()
	};
	Ok((finish(assets), finish(debts)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FieldName {
	Category,
	CategoryGroup,
	Payee,
	Account,
	Notes,
	ImportedPayee,
	Amount,
	Date,
	Cleared,
	Reconciled,
	Transfer,
	Parent,
}

#[derive(Clone, Debug)]
enum Test {
	Equals(Option<String>),
	NotEquals(Option<String>),
	OneOf(Vec<String>),
	NotOneOf(Vec<String>),
	Contains(String, bool),
	Number(&'static str, i64, Sign),
	Between(i64, i64),
	Day(&'static str, NaiveDate),
	Within(NaiveDate, NaiveDate),
	Flag(bool),
	All(Vec<Condition>),
}

#[derive(Clone, Copy, Debug)]
enum Sign {
	Any,
	Inflow,
	Outflow,
}

#[derive(Clone, Debug)]
struct Condition {
	field: FieldName,
	test: Test,
}

impl Condition {
	/// `Ok(None)` drops a condition Actual drops too (an invalid one, or
	/// `saved`, which has no condition type).
	fn parse(json: &Json) -> Result<Option<Condition>, CoreError> {
		let field = json.get("field").and_then(Json::as_str).unwrap_or_default();
		let op = json.get("op").and_then(Json::as_str).unwrap_or_default();
		let value = json.get("value").unwrap_or(&Json::Null);
		let options = json.get("options");
		let option =
			|name: &str| options.and_then(|o| o.get(name)).and_then(Json::as_bool) == Some(true);
		let name = match field {
			"category" => FieldName::Category,
			"category_group" => FieldName::CategoryGroup,
			"payee" => FieldName::Payee,
			"account" => FieldName::Account,
			"notes" => FieldName::Notes,
			"imported_payee" => FieldName::ImportedPayee,
			"amount" => FieldName::Amount,
			"date" => FieldName::Date,
			"cleared" => FieldName::Cleared,
			"reconciled" => FieldName::Reconciled,
			"transfer" => FieldName::Transfer,
			"parent" => FieldName::Parent,
			"saved" => return Ok(None),
			other => return Err(unsupported(format!("the condition field {other:?}"))),
		};
		let cant = || unsupported(format!("the condition {field} {op}"));
		let text = |value: &Json| match value {
			Json::Null => None,
			Json::String(text) => Some(text.clone()),
			other => Some(other.stringify()),
		};
		let lower = matches!(name, FieldName::Notes | FieldName::ImportedPayee);
		let prepare = |value: &Json| text(value).map(|t| if lower { t.to_lowercase() } else { t });
		let list = || {
			value
				.as_array()
				.unwrap_or_default()
				.iter()
				.filter_map(&prepare)
				.filter(|item| !item.is_empty())
				.collect::<Vec<_>>()
		};
		let make = |test| Ok(Some(Condition { field: name, test }));
		match name {
			FieldName::Category
			| FieldName::CategoryGroup
			| FieldName::Payee
			| FieldName::Account
			| FieldName::Notes
			| FieldName::ImportedPayee => match op {
				"is" if name == FieldName::Category && matches!(value, Json::Null) => {
					make(Test::All(vec![
						Condition {
							field: FieldName::Category,
							test: Test::Equals(None),
						},
						Condition {
							field: FieldName::Transfer,
							test: Test::Flag(false),
						},
						Condition {
							field: FieldName::Parent,
							test: Test::Flag(false),
						},
					]))
				}
				"isNot" if name == FieldName::Category && matches!(value, Json::Null) => {
					make(Test::All(vec![
						Condition {
							field: FieldName::Category,
							test: Test::NotEquals(None),
						},
						Condition {
							field: FieldName::Parent,
							test: Test::Flag(false),
						},
					]))
				}
				"is" => make(Test::Equals(prepare(value))),
				"isNot" => make(Test::NotEquals(prepare(value))),
				"oneOf" => make(Test::OneOf(list())),
				"notOneOf" => make(Test::NotOneOf(list())),
				"contains" | "doesNotContain" if lower => make(Test::Contains(
					prepare(value).unwrap_or_default(),
					op == "contains",
				)),
				_ => Err(cant()),
			},
			FieldName::Amount => {
				let sign = if option("inflow") {
					Sign::Inflow
				} else if option("outflow") {
					Sign::Outflow
				} else {
					Sign::Any
				};
				let number = || value.as_i64().ok_or_else(cant);
				match op {
					"is" => make(Test::Number("=", number()?, sign)),
					"gt" => make(Test::Number(">", number()?, sign)),
					"gte" => make(Test::Number(">=", number()?, sign)),
					"lt" => make(Test::Number("<", number()?, sign)),
					"lte" => make(Test::Number("<=", number()?, sign)),
					"isbetween" => {
						let a = value.get("num1").and_then(Json::as_i64).ok_or_else(cant)?;
						let b = value.get("num2").and_then(Json::as_i64).ok_or_else(cant)?;
						make(Test::Between(a.min(b), a.max(b)))
					}
					_ => Err(cant()),
				}
			}
			FieldName::Date => {
				let raw = value.as_str().ok_or_else(cant)?;
				let date = parse_date(raw).ok_or_else(cant)?;
				match (op, raw.len()) {
					("is", 10) => make(Test::Within(date, date)),
					("is", 7) => make(Test::Within(date, month_end(date))),
					("is", 4) => make(Test::Within(
						date,
						NaiveDate::from_ymd_opt(date.year(), 12, 31).ok_or_else(cant)?,
					)),
					("gt", 10) => make(Test::Day(">", date)),
					("gte", 10) => make(Test::Day(">=", date)),
					("lt", 10) => make(Test::Day("<", date)),
					("lte", 10) => make(Test::Day("<=", date)),
					_ => Err(cant()),
				}
			}
			FieldName::Cleared
			| FieldName::Reconciled
			| FieldName::Transfer
			| FieldName::Parent => match (op, value) {
				("is", Json::Bool(flag)) => make(Test::Flag(*flag)),
				_ => Err(cant()),
			},
		}
	}

	fn matches(&self, row: &Fields) -> bool {
		let transaction = row.transaction;
		let id = |field: FieldName| -> Option<String> {
			match field {
				FieldName::Category => row.category.map(ToString::to_string),
				FieldName::CategoryGroup => row.category_group.map(ToString::to_string),
				FieldName::Payee => row.payee.map(ToString::to_string),
				FieldName::Account => row.account.map(ToString::to_string),
				FieldName::Notes => transaction.notes.as_ref().map(|t| t.to_lowercase()),
				FieldName::ImportedPayee => transaction
					.imported_payee
					.as_ref()
					.map(|t| t.to_lowercase()),
				_ => None,
			}
		};
		match &self.test {
			Test::All(parts) => parts.iter().all(|part| part.matches(row)),
			Test::Equals(None) => id(self.field).is_none(),
			Test::Equals(Some(value)) if value.is_empty() => {
				id(self.field).is_none_or(|actual| actual.is_empty())
			}
			Test::Equals(Some(value)) => id(self.field).as_ref() == Some(value),
			Test::NotEquals(None) => id(self.field).is_some(),
			Test::NotEquals(Some(value)) => id(self.field).as_ref() != Some(value),
			Test::OneOf(values) => id(self.field).is_some_and(|actual| values.contains(&actual)),
			Test::NotOneOf(values) => {
				!values.is_empty() && id(self.field).is_none_or(|actual| !values.contains(&actual))
			}
			Test::Contains(needle, wanted) => {
				let found = id(self.field)
					.filter(|text| !text.is_empty())
					.is_some_and(|text| {
						like(&normalise(&format!("%{needle}%")), &normalise(&text))
					});
				found == *wanted
			}
			Test::Number(op, value, sign) => {
				let amount = transaction.amount.minor();
				let (ok, actual) = match sign {
					Sign::Any => (true, amount),
					Sign::Inflow => (amount > 0, amount),
					Sign::Outflow => (amount < 0, -amount),
				};
				ok && compare(op, actual, *value)
			}
			Test::Between(low, high) => (*low..=*high).contains(&transaction.amount.minor()),
			Test::Day(op, date) => compare(
				op,
				transaction.date.naive().num_days_from_ce(),
				date.num_days_from_ce(),
			),
			Test::Within(first, last) => (*first..=*last).contains(&transaction.date.naive()),
			Test::Flag(flag) => {
				let actual = match self.field {
					FieldName::Cleared => transaction.cleared,
					FieldName::Reconciled => transaction.reconciled,
					FieldName::Transfer => transaction.transfer.is_some(),
					_ => false,
				};
				actual == *flag
			}
		}
	}
}

fn compare<T: Ord>(op: &str, a: T, b: T) -> bool {
	match op {
		"=" => a == b,
		">" => a > b,
		">=" => a >= b,
		"<" => a < b,
		_ => a <= b,
	}
}

/// Actual's `getNormalisedString` for the letters it can meet in Latin text:
/// lowercase, a few letters that have no decomposition, then accents removed.
pub(crate) fn normalise(text: &str) -> String {
	const PLAIN: &str = "aaaaaaæceeeeiiiiðnooooo÷øuuuuyþy";
	let mut out = String::new();
	for c in text.to_lowercase().chars() {
		match c {
			'ł' => out.push('l'),
			'ø' => out.push('o'),
			'ß' => out.push_str("ss"),
			'œ' => out.push_str("oe"),
			'\u{0300}'..='\u{036F}' => {}
			'\u{E0}'..='\u{FF}' => out.push(PLAIN.chars().nth(c as usize - 0xE0).unwrap_or(c)),
			c => out.push(c),
		}
	}
	out
}

/// Actual's `UNICODE_LIKE`: `%` is any run, `?` one character, and the match
/// may start anywhere.
fn like(pattern: &str, text: &str) -> bool {
	let pattern: Vec<char> = pattern.chars().collect();
	let text: Vec<char> = text.chars().collect();
	(0..=text.len()).any(|start| glob(&pattern, text.get(start..).unwrap_or_default()))
}

fn glob(pattern: &[char], text: &[char]) -> bool {
	match pattern.split_first() {
		None => true,
		Some(('%', rest)) => {
			(0..=text.len()).any(|skip| glob(rest, text.get(skip..).unwrap_or_default()))
		}
		Some(('?', rest)) => !text.is_empty() && glob(rest, text.get(1..).unwrap_or_default()),
		Some(('\\', rest)) if rest.first().is_some_and(|c| matches!(c, '%' | '?' | '\\')) => {
			text.first() == rest.first()
				&& glob(
					rest.get(1..).unwrap_or_default(),
					text.get(1..).unwrap_or_default(),
				)
		}
		Some((c, rest)) => text.first() == Some(c) && glob(rest, text.get(1..).unwrap_or_default()),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn like_matches_actuals_wildcards_anywhere() {
		assert!(like("%cafe%", "le cafe noir"));
		assert!(like("%c?fe%", "cafe"));
		assert!(!like("%tea%", "coffee"));
		assert!(like("%100\\%%", "a 100% match"));
		assert_eq!(normalise("Café Ñandú"), "cafe nandu");
		assert_eq!(normalise("Straße Łódz"), "strasse lodz");
	}
}
