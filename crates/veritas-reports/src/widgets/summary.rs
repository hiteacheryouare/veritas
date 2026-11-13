//! The summary card (`SummaryCard.tsx`, `spreadsheets/summary-spreadsheet.ts`):
//! one number over the card's filtered transactions, as a sum, an average
//! per month, year or transaction, or a percentage of a second filter.

use chrono::{Datelike, NaiveDate};
use veritas_domain::{BudgetData, CoreError, Money};

use super::{TimeRange, calculate_time_range, conditions_filter, filtered_rows, meta_filter};
use crate::json::Json;
use crate::report_query::{day, parse_date, transaction_date_bounds};

#[derive(Clone, Debug, PartialEq)]
pub struct SummaryData {
	/// The first and last day the card covers.
	pub start: NaiveDate,
	pub end: NaiveDate,
	/// The number the card shows: minor units, or a percentage for
	/// `percentage`. It can be fractional, infinite or NaN, as in Actual.
	pub total: f64,
	pub dividend: f64,
	pub divisor: f64,
	pub content: SummaryContent,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SummaryContent {
	Sum,
	AveragePerMonth,
	AveragePerYear,
	AveragePerTransaction,
	Percentage,
}

fn last_day_of_month(date: NaiveDate) -> NaiveDate {
	date.with_day(1)
		.and_then(|first| first.checked_add_months(chrono::Months::new(1)))
		.and_then(|next| next.pred_opt())
		.unwrap_or(date)
}

fn bad_date(text: &str) -> CoreError {
	CoreError::Invalid(format!("the summary card's date {text:?} isn't a date"))
}

/// The card's first and last day: the month `start` falls in from its first
/// day, through the last day of `end`'s month, or through today when that
/// month is this month.
pub(crate) fn summary_days(
	range: &TimeRange,
	today: NaiveDate,
) -> Result<(NaiveDate, NaiveDate), CoreError> {
	let start = parse_date(&range.start)
		.and_then(|date| date.with_day(1))
		.ok_or_else(|| bad_date(&range.start))?;
	let end = parse_date(&range.end).ok_or_else(|| bad_date(&range.end))?;
	let end = if (end.year(), end.month()) == (today.year(), today.month()) {
		today
	} else {
		last_day_of_month(end)
	};
	if start > end {
		return Err(CoreError::Invalid(
			"the summary card starts after it ends".to_owned(),
		));
	}
	Ok((start, end))
}

/// `get-latest-transaction`: the newest date, split parents counted.
pub(crate) fn latest_transaction(budget: &BudgetData) -> Option<NaiveDate> {
	let any = budget.transactions.values().any(|row| !row.tombstone);
	any.then(|| transaction_date_bounds(budget, NaiveDate::MIN).1)
}

/// The summary card's range when it has no `timeFrame`: this month through
/// the newest transaction's month, in `full` mode.
pub(crate) fn card_range(meta: Option<&Json>, budget: &BudgetData, today: NaiveDate) -> TimeRange {
	let first = day(today.with_day(1).unwrap_or(today));
	let today_text = day(today);
	calculate_time_range(
		meta.and_then(|meta| meta.get("timeFrame")),
		Some((&first, &today_text, "full")),
		Some(latest_transaction(budget).unwrap_or(today)),
		today,
	)
}

fn content(meta: Option<&Json>) -> Json {
	let sum = || Json::Object(vec![("type".to_owned(), Json::String("sum".to_owned()))]);
	match meta
		.and_then(|meta| meta.get("content"))
		.and_then(Json::as_str)
	{
		Some(text) if !text.is_empty() => Json::parse(text).unwrap_or_else(|_| sum()),
		_ => sum(),
	}
}

#[allow(clippy::cast_precision_loss)]
fn as_f64(money: Money) -> f64 {
	money.minor() as f64
}

pub fn summary(
	budget: &BudgetData,
	meta: Option<&Json>,
	today: NaiveDate,
) -> Result<SummaryData, CoreError> {
	let range = card_range(meta, budget, today);
	let (start, end) = summary_days(&range, today)?;
	let filter = meta_filter(meta)?;
	let content = content(meta);
	let kind = match content.get("type").and_then(Json::as_str) {
		Some("sum") => SummaryContent::Sum,
		Some("avgPerMonth") => SummaryContent::AveragePerMonth,
		Some("avgPerYear") => SummaryContent::AveragePerYear,
		Some("avgPerTransact") => SummaryContent::AveragePerTransaction,
		Some("percentage") => SummaryContent::Percentage,
		_ => {
			return Err(CoreError::Invalid("Unsupported summary type".to_owned()));
		}
	};
	let mut amount = Money::ZERO;
	let mut count = 0_u64;
	for row in filtered_rows(budget, &filter) {
		let row = row?;
		let date = row.date.naive();
		if date < start || date > end {
			continue;
		}
		amount = amount.checked_add(row.amount)?;
		count += 1;
	}
	let total_amount = as_f64(amount);
	let data = |total: f64, dividend: f64, divisor: f64| SummaryData {
		start,
		end,
		total,
		dividend,
		divisor,
		content: kind.clone(),
	};
	#[allow(clippy::cast_precision_loss)]
	Ok(match kind {
		SummaryContent::Sum => data(total_amount, total_amount, 0.0),
		SummaryContent::AveragePerTransaction => {
			let total = if count == 0 {
				0.0
			} else {
				total_amount / count as f64
			};
			data(total, total_amount, count as f64)
		}
		SummaryContent::AveragePerMonth if count == 0 => data(0.0, 0.0, 0.0),
		SummaryContent::AveragePerMonth => {
			let whole = calendar_months(end, start);
			let days_in_month = f64::from(last_day_of_month(end).day());
			let months = whole as f64 + f64::from(end.day()) / days_in_month;
			data(total_amount / months, total_amount, months)
		}
		SummaryContent::AveragePerYear if count == 0 => data(0.0, 0.0, 0.0),
		SummaryContent::AveragePerYear => {
			let years = ((end - start).num_days() + 1) as f64 / 365.25;
			data(total_amount / years, total_amount, years)
		}
		SummaryContent::Percentage => {
			let Some(conditions) = content.get("divisorConditions").and_then(Json::as_array) else {
				return Ok(data(0.0, 0.0, 0.0));
			};
			let divisor_filter = conditions_filter(
				Some(&Json::Object(vec![
					("conditions".to_owned(), Json::Array(conditions.to_vec())),
					(
						"op".to_owned(),
						content
							.get("divisorConditionsOp")
							.cloned()
							.unwrap_or(Json::Null),
					),
				])),
				"conditions",
				"op",
			)?;
			let all_time = content
				.get("divisorAllTimeDateRange")
				.and_then(Json::as_bool)
				.unwrap_or(false);
			let mut divisor = Money::ZERO;
			for row in filtered_rows(budget, &divisor_filter) {
				let row = row?;
				let date = row.date.naive();
				if all_time || (date >= start && date <= end) {
					divisor = divisor.checked_add(row.amount)?;
				}
			}
			let divisor = as_f64(divisor);
			let total = js_round(total_amount / divisor * 10000.0) / 100.0;
			data(total, total_amount, divisor)
		}
	})
}

fn calendar_months(later: NaiveDate, earlier: NaiveDate) -> i64 {
	(i64::from(later.year()) - i64::from(earlier.year())) * 12 + i64::from(later.month())
		- i64::from(earlier.month())
}

/// JS `Math.round`, which keeps NaN and infinities.
fn js_round(x: f64) -> f64 {
	if !x.is_finite() {
		return x;
	}
	let floor = x.floor();
	if x - floor >= 0.5 { floor + 1.0 } else { floor }
}
