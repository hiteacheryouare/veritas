//! The spending card (`SpendingCard.tsx`, `spreadsheets/spending-spreadsheet.ts`):
//! a month's spending day by day, cumulative, against another month, the
//! average of earlier months, or the month's budget. Days 28 to 31 share the
//! 28th.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{Datelike, Days, NaiveDate};
use veritas_domain::{BudgetData, CategoryId, CoreError, Money, YearMonth};

use super::budget_analysis::{category_condition, category_passes};
use super::summary::earliest_transaction;
use super::{calculate_time_range, meta_filter};
use crate::custom_report::Interval;
use crate::json::Json;
use crate::report_query::{day, query_rows};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpendingMode {
	SingleMonth,
	Budget,
	Average,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpendingDay {
	/// 1 to 28.
	pub day: u32,
	/// Spending so far in the compared month, `None` for days after today.
	pub compare: Option<Money>,
	pub compare_to: Option<Money>,
	/// The average month's spending so far, rounded.
	pub average: Money,
	/// The month's budget spread evenly over its days, so far: negative and
	/// fractional minor units, as Actual accumulates it.
	pub budget: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Spending {
	pub mode: SpendingMode,
	pub compare: YearMonth,
	pub compare_to: YearMonth,
	/// The months averaged, `None` when there are none.
	pub average_months: Option<(YearMonth, YearMonth)>,
	pub days: Vec<SpendingDay>,
	pub total_assets: Money,
	pub total_debts: Money,
	/// The day the headline compares both running totals through: today's
	/// day in the current month (at most 28), else the 28th.
	pub headline_day: u32,
	/// The chosen comparison's running total less the compared month's,
	/// through `headline_day`, rounded.
	pub difference: i64,
}

fn month_of(date: NaiveDate) -> Result<YearMonth, CoreError> {
	YearMonth::new(date.year(), date.month())
}

fn month_text(text: &str) -> Result<YearMonth, CoreError> {
	YearMonth::parse(text)
		.map_err(|_| CoreError::Invalid(format!("the spending card's month {text:?}")))
}

fn last_day(month: YearMonth) -> Result<NaiveDate, CoreError> {
	month
		.offset(1)?
		.first_day()
		.naive()
		.pred_opt()
		.ok_or(CoreError::Overflow)
}

/// `calculateSpendingReportTimeRange`, reading `isLive` as JS does: missing
/// is live, `null` is live for the slide but not for the budget and average
/// shortcut.
pub(super) fn compared_months(
	meta: Option<&Json>,
	mode: SpendingMode,
	today: NaiveDate,
) -> Result<(YearMonth, YearMonth), CoreError> {
	let field = |key: &str| meta.and_then(|meta| meta.get(key));
	let current = month_of(today)?;
	let (live, slides) = match field("isLive") {
		None => (true, true),
		Some(Json::Null) => (false, true),
		Some(Json::Bool(live)) => (*live, *live),
		Some(_) => (true, true),
	};
	if mode != SpendingMode::SingleMonth && live {
		return Ok((current, current));
	}
	let compare = field("compare").and_then(Json::as_str);
	let compare_to = match (field("compareTo").and_then(Json::as_str), compare) {
		(Some(text), _) => Some(text.to_owned()),
		(None, Some(text)) => Some(month_text(text)?.offset(-1)?.to_string()),
		(None, None) => None,
	};
	let text = |value: &str| Json::String(value.to_owned());
	let mut frame = vec![(
		"mode".to_owned(),
		text(if slides { "sliding-window" } else { "static" }),
	)];
	if let Some(compare) = compare {
		frame.push(("start".to_owned(), text(compare)));
	}
	if let Some(compare_to) = &compare_to {
		frame.push(("end".to_owned(), text(compare_to)));
	}
	let (now, before) = (current.to_string(), current.offset(-1)?.to_string());
	let range = calculate_time_range(
		Some(&Json::Object(frame)),
		Some((&now, &before, "sliding-window")),
		None,
		today,
	);
	Ok((month_text(&range.start)?, month_text(&range.end)?))
}

/// `resolveSpendingAverageRange` after `normalizeSpendingAverageRange`: the
/// months before `compare` that the average covers.
fn average_months(
	budget: &BudgetData,
	meta: Option<&Json>,
	compare: YearMonth,
) -> Result<Option<(YearMonth, YearMonth)>, CoreError> {
	let range = meta.and_then(|meta| meta.get("averageRange"));
	let field = |key: &str| range.and_then(|range| range.get(key));
	let last = compare.offset(-1)?;
	let first = match field("mode").and_then(Json::as_str) {
		Some("last-n-months") => {
			let count = match field("months").and_then(Json::as_i64) {
				Some(6) => 6,
				Some(12) => 12,
				_ => 3,
			};
			Some(compare.offset(-count)?)
		}
		Some("year-to-date") => Some(YearMonth::new(compare.year(), 1)?),
		Some("all-time") => earliest_transaction(budget).map(month_of).transpose()?,
		_ => Some(compare.offset(-3)?),
	};
	Ok(first
		.filter(|first| *first <= last)
		.map(|first| (first, last)))
}

fn is_supported_category_condition(condition: &Json) -> bool {
	let text = |value: Option<&Json>| matches!(value, Some(Json::String(_)));
	let value = condition.get("value");
	match condition.get("op").and_then(Json::as_str) {
		Some("is" | "isNot" | "contains" | "doesNotContain" | "matches") => text(value),
		Some("oneOf" | "notOneOf") => value
			.and_then(Json::as_array)
			.is_some_and(|values| values.iter().all(|value| text(Some(value)))),
		_ => false,
	}
}

/// `new RegExp` is skipped for a pattern over 256 UTF-16 units, and the
/// condition then passes every category.
fn passes_everything(condition: &Json) -> bool {
	condition.get("op").and_then(Json::as_str) == Some("matches")
		&& condition
			.get("value")
			.and_then(Json::as_str)
			.is_some_and(|source| source.encode_utf16().count() > 256)
}

/// `getSpendingBudgetFilters` with `filterCategoriesByConditions`: the
/// expense categories whose budgets make the budget line. Conditions on
/// anything but categories don't narrow it, and one it can't read in memory
/// widens it to every expense category.
fn budget_categories(
	budget: &BudgetData,
	meta: Option<&Json>,
) -> Result<BTreeSet<CategoryId>, CoreError> {
	let expense = budget
		.categories
		.values()
		.filter(|category| !category.tombstone && !category.is_income);
	let conditions: Vec<&Json> = meta
		.and_then(|meta| meta.get("conditions"))
		.and_then(Json::as_array)
		.unwrap_or_default()
		.iter()
		.filter(|condition| {
			condition
				.get("customName")
				.is_none_or(|name| matches!(name, Json::Null))
		})
		.filter(|condition| {
			matches!(
				condition.get("field").and_then(Json::as_str),
				Some("category" | "category_group")
			)
		})
		.collect();
	let any = meta
		.and_then(|meta| meta.get("conditionsOp"))
		.and_then(Json::as_str)
		== Some("or");
	let everything = conditions.is_empty()
		|| !conditions
			.iter()
			.all(|condition| is_supported_category_condition(condition))
		|| (any
			&& conditions
				.iter()
				.any(|condition| passes_everything(condition)));
	if everything {
		return Ok(expense.map(|category| category.id.clone()).collect());
	}
	let tests = conditions
		.into_iter()
		.filter(|condition| !passes_everything(condition))
		.map(category_condition)
		.collect::<Result<Vec<_>, _>>()?;
	let mut chosen = BTreeSet::new();
	for category in expense {
		let mut results = Vec::with_capacity(tests.len());
		for test in &tests {
			results.push(category_passes(test, budget, category)?);
		}
		let keep = if any {
			results.iter().any(|passed| *passed)
		} else {
			results.iter().all(|passed| *passed)
		};
		if keep {
			chosen.insert(category.id.clone());
		}
	}
	Ok(chosen)
}

/// JS `Math.round`.
fn js_round(x: f64) -> f64 {
	let floor = x.floor();
	if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

#[allow(clippy::cast_precision_loss)]
fn as_f64(money: Money) -> f64 {
	money.minor() as f64
}

/// What one month spent on each day, from its first.
struct MonthSpending {
	days: Vec<(NaiveDate, Money)>,
}

impl MonthSpending {
	/// The running total through `day` (the 28th takes the rest of the
	/// month), and what the card shows for that day: the total through its
	/// last date up to today, or `None` when all of its dates are later.
	fn through(&self, day: u32, today: NaiveDate) -> Result<(Money, Option<Money>), CoreError> {
		let mut total = Money::ZERO;
		let mut shown = None;
		for (date, amount) in &self.days {
			if day < 28 && date.day() > day {
				break;
			}
			total = total.checked_add(*amount)?;
			if date.day() >= day && *date <= today {
				shown = Some(total);
			}
		}
		Ok((total, shown))
	}
}

pub fn spending(
	budget: &BudgetData,
	meta: Option<&Json>,
	today: NaiveDate,
) -> Result<Spending, CoreError> {
	let mode = match meta.and_then(|meta| meta.get("mode")) {
		None | Some(Json::Null) => SpendingMode::SingleMonth,
		Some(mode) => match mode.as_str() {
			Some("single-month") => SpendingMode::SingleMonth,
			Some("budget") => SpendingMode::Budget,
			Some("average") => SpendingMode::Average,
			_ => {
				return Err(CoreError::Invalid(format!(
					"the spending card's mode {}",
					mode.stringify()
				)));
			}
		},
	};
	let (compare, compare_to) = compared_months(meta, mode, today)?;
	let average = average_months(budget, meta, compare)?;
	let start = average
		.map_or(compare, |(first, _)| first)
		.first_day()
		.naive();
	let end = last_day(compare)?;
	let (start_to, end_to) = (compare_to.first_day().naive(), last_day(compare_to)?);
	let apart = end_to < start || start_to > end;

	let filter = meta_filter(meta)?;
	let mut by_date: BTreeMap<NaiveDate, Money> = BTreeMap::new();
	let (mut total_assets, mut total_debts) = (Money::ZERO, Money::ZERO);
	let mut ranges = vec![(start, end)];
	if apart {
		ranges.push((start_to, end_to));
	}
	for (from, to) in ranges {
		let (assets, debts) = query_rows(budget, &filter, Interval::Daily, &day(from), &day(to))?;
		for (row, positive) in assets
			.iter()
			.map(|row| (row, true))
			.chain(debts.iter().map(|row| (row, false)))
		{
			if row.category_income || row.account_off_budget {
				continue;
			}
			let date = NaiveDate::parse_from_str(&row.date, "%Y-%m-%d")
				.map_err(|_| CoreError::Internal(format!("a daily row dated {}", row.date)))?;
			let total = if positive {
				&mut total_assets
			} else {
				&mut total_debts
			};
			*total = total.checked_add(row.amount)?;
			let sum = by_date.entry(date).or_insert(Money::ZERO);
			*sum = sum.checked_add(row.amount)?;
		}
	}

	let month_spending = |month: YearMonth| -> Result<MonthSpending, CoreError> {
		let mut days = Vec::new();
		let mut date = month.first_day().naive();
		let last = last_day(month)?;
		while date <= last {
			days.push((date, by_date.get(&date).copied().unwrap_or(Money::ZERO)));
			date = date + Days::new(1);
		}
		Ok(MonthSpending { days })
	};
	let compared = month_spending(compare)?;
	let compared_to = month_spending(compare_to)?;
	let averaged = match average {
		Some((first, last)) => {
			let mut months = Vec::new();
			let mut month = first;
			while month <= last {
				months.push(month_spending(month)?);
				month = month.offset(1)?;
			}
			months
		}
		None => Vec::new(),
	};

	let categories = budget_categories(budget, meta)?;
	let budgeted = Money::checked_sum(
		budget
			.budget_cells
			.values()
			.filter(|cell| cell.month == compare && categories.contains(&cell.category))
			.map(|cell| cell.budgeted),
	)?;
	let daily_budget = as_f64(budgeted) / compared.days.len() as f64;

	let mut running_budget = 0.0_f64;
	let mut spent_days = 0;
	let mut days = Vec::with_capacity(28);
	for day in 1..=28_u32 {
		let through = if day < 28 {
			day as usize
		} else {
			compared.days.len()
		};
		while spent_days < through {
			running_budget -= daily_budget;
			spent_days += 1;
		}
		let mut sum = 0.0_f64;
		for month in &averaged {
			sum += as_f64(month.through(day, NaiveDate::MAX)?.0);
		}
		let average = if averaged.is_empty() {
			0.0
		} else {
			js_round(sum / averaged.len() as f64)
		};
		#[allow(clippy::cast_possible_truncation)]
		days.push(SpendingDay {
			day,
			compare: compared.through(day, today)?.1,
			compare_to: compared_to.through(day, today)?.1,
			average: Money::from_minor(average as i64),
			budget: running_budget,
		});
	}

	let headline_day = if compare == month_of(today)? {
		today.day().min(28)
	} else {
		28
	};
	let headline = &days[headline_day as usize - 1];
	let shown = |value: Option<Money>| value.map_or(0.0, as_f64);
	let chosen = match mode {
		SpendingMode::SingleMonth => shown(headline.compare_to),
		SpendingMode::Average => as_f64(headline.average),
		SpendingMode::Budget => headline.budget,
	};
	#[allow(clippy::cast_possible_truncation)]
	let difference = js_round(chosen - shown(headline.compare)) as i64;
	Ok(Spending {
		mode,
		compare,
		compare_to,
		average_months: average,
		days,
		total_assets,
		total_debts,
		headline_day,
		difference,
	})
}
