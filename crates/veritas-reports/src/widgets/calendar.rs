//! The calendar card (`CalendarCard.tsx`, `spreadsheets/calendar-spreadsheet.ts`):
//! each month as a grid of weeks, with the day's money in and money out and
//! each as a share of the month's total.

use std::collections::BTreeMap;

use chrono::{Datelike, Days, Months, NaiveDate};
use veritas_domain::{BudgetData, CoreError, Money};

use super::filtered_rows_between;
use super::meta_filter;
use super::summary::card_range;
use crate::json::Json;
use crate::report_query::{parse_date, week_start};

#[derive(Clone, Debug, PartialEq)]
pub struct CalendarDay {
	pub date: NaiveDate,
	/// Money in and out that day, both positive minor units. Days of the
	/// neighbouring months that pad the grid are zero.
	pub income: Money,
	pub expense: Money,
	/// Each as a percentage of the month's total, for the bar lengths.
	pub income_size: f64,
	pub expense_size: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CalendarMonth {
	pub start: NaiveDate,
	pub end: NaiveDate,
	/// Whole weeks, from the week holding the 1st through the week holding
	/// the last day.
	pub days: Vec<CalendarDay>,
	pub total_income: Money,
	pub total_expense: Money,
}

fn month_end(date: NaiveDate) -> NaiveDate {
	date.with_day(1)
		.and_then(|first| first.checked_add_months(Months::new(1)))
		.and_then(|next| next.pred_opt())
		.unwrap_or(date)
}

fn bad_date(text: &str) -> CoreError {
	CoreError::Invalid(format!("the calendar card's date {text:?} isn't a date"))
}

#[allow(clippy::cast_precision_loss)]
fn share(value: Money, total: Money) -> f64 {
	if total.minor() <= 0 {
		return 0.0;
	}
	let result = value.minor().abs() as f64 / total.minor() as f64 * 100.0;
	if result.is_finite() { result } else { 0.0 }
}

/// `first_day_of_week` is the `firstDayOfWeekIdx` pref, 0 for Sunday.
pub fn calendar(
	budget: &BudgetData,
	meta: Option<&Json>,
	today: NaiveDate,
	first_day_of_week: u32,
) -> Result<Vec<CalendarMonth>, CoreError> {
	let range = card_range(meta, budget, today);
	let start = parse_date(&range.start)
		.and_then(|date| date.with_day(1))
		.ok_or_else(|| bad_date(&range.start))?;
	let end = month_end(parse_date(&range.end).ok_or_else(|| bad_date(&range.end))?);
	let filter = meta_filter(meta)?;
	let mut income: BTreeMap<NaiveDate, Money> = BTreeMap::new();
	let mut expense: BTreeMap<NaiveDate, Money> = BTreeMap::new();
	for row in filtered_rows_between(budget, &filter, start, end) {
		let row = row?;
		let date = row.date.naive();
		if row.amount == Money::ZERO {
			continue;
		}
		let side = if row.amount > Money::ZERO {
			&mut income
		} else {
			&mut expense
		};
		let sum = side.entry(date).or_insert(Money::ZERO);
		*sum = sum.checked_add(row.amount)?;
	}
	let first_day_of_week = if first_day_of_week <= 6 {
		first_day_of_week
	} else {
		0
	};
	let mut months = Vec::new();
	let mut first = start;
	loop {
		let last = month_end(first);
		let in_month = |map: &BTreeMap<NaiveDate, Money>| -> Result<Option<Money>, CoreError> {
			let mut total: Option<Money> = None;
			for amount in map.range(first..=last).map(|(_, amount)| *amount) {
				let absolute = Money::from_minor(amount.minor().abs());
				total = Some(total.unwrap_or(Money::ZERO).checked_add(absolute)?);
			}
			Ok(total)
		};
		let total_income = in_month(&income)?.unwrap_or(Money::ZERO);
		let total_expense = in_month(&expense)?.unwrap_or(Money::ZERO);
		let begin = week_start(first, first_day_of_week);
		let mut count = (first - begin).num_days() + i64::from(last.day());
		if count % 7 != 0 {
			count += 7 - count % 7;
		}
		let mut days = Vec::new();
		for offset in 0..count.unsigned_abs() {
			let date = begin.checked_add_days(Days::new(offset)).unwrap_or(begin);
			let inside = date.month() == first.month() && date.year() == first.year();
			let day_income = income.get(&date).copied().filter(|_| inside);
			let day_expense = expense.get(&date).copied().filter(|_| inside);
			let day_income = day_income.unwrap_or(Money::ZERO);
			let day_expense = day_expense.unwrap_or(Money::ZERO);
			days.push(CalendarDay {
				date,
				income: Money::from_minor(day_income.minor().abs()),
				expense: Money::from_minor(day_expense.minor().abs()),
				income_size: share(day_income, total_income),
				expense_size: share(day_expense, total_expense),
			});
		}
		months.push(CalendarMonth {
			start: first,
			end: last,
			days,
			total_income,
			total_expense,
		});
		if last >= end {
			break;
		}
		first = last.succ_opt().unwrap_or(last);
	}
	Ok(months)
}
