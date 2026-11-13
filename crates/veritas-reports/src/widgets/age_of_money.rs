//! The age of money card (`AgeOfMoneyCard.tsx`,
//! `spreadsheets/age-of-money-spreadsheet.ts`): money in fills dated buckets,
//! money out drains the oldest first, and an outflow's age is the days since
//! the last bucket it drew on. The card shows the rounded mean of the last
//! ten ages.
//!
//! Actual sorts by amount sign, not by category: a refund is money in. That
//! is Actual's own number for Actual's card (D73), not a CLS-* reading.

use chrono::{Datelike, Days, NaiveDate};
use veritas_domain::{Account, BudgetData, CoreError, Money, Transaction};

use super::{TimeRange, calculate_time_range, filtered_rows, meta_filter};
use crate::filter::contains;
use crate::json::Json;
use crate::report_query::parse_date;
use crate::widgets::summary::latest_transaction;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Granularity {
	Daily,
	Weekly,
	Monthly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trend {
	Up,
	Down,
	Stable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgePoint {
	/// The period's first day: a day, a Monday, or the 1st of a month.
	pub period: NaiveDate,
	/// The rounded mean of the last ten ages up to this period, in days.
	pub age: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgeOfMoney {
	pub range: TimeRange,
	pub granularity: Granularity,
	pub points: Vec<AgePoint>,
	pub current_age: Option<i64>,
	pub trend: Trend,
	/// Some outflow found no money left to draw on.
	pub insufficient_data: bool,
}

/// What a transaction's transfer payee points at, joined the way AQL joins
/// `payee.transfer_acct`.
fn transfer_account<'a>(budget: &'a BudgetData, row: &Transaction) -> Option<&'a Account> {
	let payee = row
		.payee
		.as_ref()
		.and_then(|id| budget.resolve_payee(id))
		.and_then(|id| budget.payees.get(id))
		.filter(|payee| !payee.tombstone)?;
	budget
		.accounts
		.get(payee.transfer_account.as_ref()?)
		.filter(|account| !account.tombstone)
}

/// One account condition, inverted, over the counterpart account
/// (`accountConditionToTransferAcctAql`). `None` is an op Actual can't invert,
/// which drops back to the plain transfer rule.
#[derive(Clone, Debug)]
enum Outside {
	Is(Option<String>),
	IsNot(Option<String>),
	OneOf(Vec<Option<String>>),
	NotOneOf(Vec<Option<String>>),
	OffBudget(bool),
	/// The needle, and whether the name must not contain it.
	Contains(String, bool),
}

impl Outside {
	fn parse(condition: &Json) -> Option<Outside> {
		let value = condition.get("value").unwrap_or(&Json::Null);
		let id = |value: &Json| value.as_str().map(str::to_owned);
		let list = || {
			value
				.as_array()
				.map(|values| values.iter().map(id).collect::<Vec<_>>())
		};
		Some(match condition.get("op").and_then(Json::as_str)? {
			"is" => Outside::IsNot(id(value)),
			"isNot" => Outside::Is(id(value)),
			"oneOf" => Outside::NotOneOf(list()?),
			"notOneOf" => Outside::OneOf(list()?),
			"onBudget" => Outside::OffBudget(true),
			"offBudget" => Outside::OffBudget(false),
			"contains" => Outside::Contains(value.as_str()?.to_owned(), true),
			"doesNotContain" => Outside::Contains(value.as_str()?.to_owned(), false),
			_ => return None,
		})
	}

	fn matches(&self, account: &Account) -> bool {
		let id = account.id.as_str();
		let eq = |value: &Option<String>| value.as_deref() == Some(id);
		match self {
			Outside::Is(value) => eq(value),
			Outside::IsNot(value) => value.as_deref().is_none_or(|value| value != id),
			Outside::OneOf(values) => values.iter().any(eq),
			Outside::NotOneOf(values) => values
				.iter()
				.all(|value| value.as_deref().is_none_or(|value| value != id)),
			Outside::OffBudget(flag) => account.off_budget == *flag,
			Outside::Contains(needle, negate) => {
				contains(needle, Some(&account.name.to_lowercase())) != *negate
			}
		}
	}
}

/// `buildTransferInclusionFilter`: plain rows and transfers with off-budget
/// accounts always count; transfers between on-budget accounts count only
/// when an account filter leaves the other side out.
struct TransferRule {
	outside: Option<(Vec<Outside>, bool)>,
}

impl TransferRule {
	fn new(conditions: &[Json], any: bool) -> TransferRule {
		let accounts: Vec<&Json> = conditions
			.iter()
			.filter(|condition| {
				condition
					.get("customName")
					.is_none_or(|name| matches!(name, Json::Null))
			})
			.filter(|condition| condition.get("field").and_then(Json::as_str) == Some("account"))
			.collect();
		if accounts.is_empty() {
			return TransferRule { outside: None };
		}
		let parsed: Option<Vec<Outside>> = accounts.into_iter().map(Outside::parse).collect();
		TransferRule {
			outside: parsed.map(|negations| (negations, !any)),
		}
	}

	fn counts(&self, budget: &BudgetData, row: &Transaction) -> bool {
		let Some(account) = transfer_account(budget, row) else {
			return true;
		};
		if account.off_budget {
			return true;
		}
		match &self.outside {
			None => false,
			Some((negations, either)) if *either => negations.iter().any(|n| n.matches(account)),
			Some((negations, _)) => negations.iter().all(|n| n.matches(account)),
		}
	}
}

/// JS `Math.round` of a mean of whole days.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn rounded_mean(ages: &[i64]) -> i64 {
	let sum: i64 = ages.iter().sum();
	let mean = sum as f64 / ages.len() as f64;
	let floor = mean.floor();
	(if mean - floor >= 0.5 {
		floor + 1.0
	} else {
		floor
	}) as i64
}

fn last_ten(ages: &[i64]) -> &[i64] {
	&ages[ages.len().saturating_sub(10)..]
}

fn monday(date: NaiveDate) -> NaiveDate {
	date - Days::new(u64::from(date.weekday().num_days_from_monday()))
}

fn month_end(date: NaiveDate) -> NaiveDate {
	date.with_day(1)
		.and_then(|first| first.checked_add_months(chrono::Months::new(1)))
		.and_then(|next| next.pred_opt())
		.unwrap_or(date)
}

fn period_of(date: NaiveDate, granularity: Granularity) -> NaiveDate {
	match granularity {
		Granularity::Daily => date,
		Granularity::Weekly => monday(date),
		Granularity::Monthly => date.with_day(1).unwrap_or(date),
	}
}

fn bad_date(text: &str) -> CoreError {
	CoreError::Invalid(format!(
		"the age of money card's date {text:?} isn't a date"
	))
}

pub fn age_of_money(
	budget: &BudgetData,
	meta: Option<&Json>,
	today: NaiveDate,
) -> Result<AgeOfMoney, CoreError> {
	let range = calculate_time_range(
		meta.and_then(|meta| meta.get("timeFrame")),
		None,
		Some(latest_transaction(budget).unwrap_or(today)),
		today,
	);
	let granularity = match meta
		.and_then(|meta| meta.get("granularity"))
		.and_then(Json::as_str)
	{
		Some("daily") => Granularity::Daily,
		Some("weekly") => Granularity::Weekly,
		_ => Granularity::Monthly,
	};
	let start = parse_date(&range.start)
		.and_then(|date| date.with_day(1))
		.ok_or_else(|| bad_date(&range.start))?;
	let end = month_end(parse_date(&range.end).ok_or_else(|| bad_date(&range.end))?);
	let fixed_end = end.min(today);
	let conditions = meta
		.and_then(|meta| meta.get("conditions"))
		.and_then(Json::as_array)
		.unwrap_or_default();
	let any = meta
		.and_then(|meta| meta.get("conditionsOp"))
		.and_then(Json::as_str)
		== Some("or");
	let filter = meta_filter(meta)?;
	let transfers = TransferRule::new(conditions, any);

	let mut income: Vec<&Transaction> = Vec::new();
	let mut expenses: Vec<&Transaction> = Vec::new();
	for row in filtered_rows(budget, &filter) {
		let row = row?;
		let on_budget = budget
			.accounts
			.get(&row.account)
			.is_some_and(|account| !account.tombstone && !account.off_budget);
		if row.date.naive() > fixed_end || !on_budget || !transfers.counts(budget, row) {
			continue;
		}
		if row.amount > Money::ZERO {
			income.push(row);
		} else if row.amount < Money::ZERO {
			expenses.push(row);
		}
	}
	// AQL's default order for transactions breaks date ties, and the stable
	// sort by date keeps it.
	let order = |a: &&Transaction, b: &&Transaction| {
		a.date
			.cmp(&b.date)
			.then(a.starting_balance.cmp(&b.starting_balance))
			.then(b.sort_order.total_cmp(&a.sort_order))
			.then_with(|| a.id.cmp(&b.id))
	};
	income.sort_by(order);
	expenses.sort_by(order);

	let mut buckets: Vec<(NaiveDate, i64)> = income
		.iter()
		.map(|row| (row.date.naive(), row.amount.minor()))
		.collect();
	let mut ages: Vec<(NaiveDate, i64)> = Vec::new();
	let mut current = 0;
	let mut insufficient_data = false;
	for expense in &expenses {
		let mut remaining = expense.amount.minor().abs();
		let mut last_bucket: Option<NaiveDate> = None;
		while remaining > 0 && current < buckets.len() {
			let bucket = &mut buckets[current];
			if bucket.1 > 0 {
				let deduction = bucket.1.min(remaining);
				bucket.1 -= deduction;
				remaining -= deduction;
				last_bucket = Some(bucket.0);
			}
			if bucket.1 <= 0 {
				current += 1;
			}
		}
		if remaining > 0 {
			insufficient_data = true;
		}
		if let Some(bucket) = last_bucket {
			let date = expense.date.naive();
			ages.push((date, (date - bucket).num_days().max(0)));
		}
	}

	let shown: Vec<(NaiveDate, i64)> = ages
		.into_iter()
		.filter(|(date, _)| *date >= start)
		.collect();
	let mut graph_end = end;
	if granularity != Granularity::Monthly && graph_end > today {
		graph_end = today;
	}
	let mut periods = Vec::new();
	let mut period = period_of(start, granularity);
	while period <= graph_end {
		periods.push(period);
		let next = match granularity {
			Granularity::Daily => period.succ_opt(),
			Granularity::Weekly => period.checked_add_days(Days::new(7)),
			Granularity::Monthly => period.checked_add_months(chrono::Months::new(1)),
		};
		let Some(next) = next else {
			break;
		};
		period = next;
	}
	let mut points = Vec::new();
	let mut so_far: Vec<i64> = Vec::new();
	let mut next_age = 0;
	for period in periods {
		while next_age < shown.len() && period_of(shown[next_age].0, granularity) <= period {
			so_far.push(shown[next_age].1);
			next_age += 1;
		}
		if !so_far.is_empty() {
			points.push(AgePoint {
				period,
				age: rounded_mean(last_ten(&so_far)),
			});
		}
	}
	let all_ages: Vec<i64> = shown.iter().map(|(_, age)| *age).collect();
	let current_age = (!all_ages.is_empty()).then(|| rounded_mean(last_ten(&all_ages)));
	let trend = match points.as_slice() {
		[.., before, last] if last.age - before.age > 2 => Trend::Up,
		[.., before, last] if last.age - before.age < -2 => Trend::Down,
		_ => Trend::Stable,
	};
	Ok(AgeOfMoney {
		range,
		granularity,
		points,
		current_age,
		trend,
		insufficient_data,
	})
}
