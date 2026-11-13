//! The crossover card (`CrossoverCard.tsx`, `spreadsheets/crossover-spreadsheet.ts`):
//! the chosen accounts' balance times a safe withdrawal rate against monthly
//! spending in the chosen categories, then projected forward until the
//! income covers the spending.

use std::collections::BTreeMap;

use chrono::{Datelike, NaiveDate};
use veritas_domain::{BudgetData, CoreError, YearMonth};

use super::calculate_time_range;
use crate::json::Json;
use crate::report_query::{is_row, parse_date, transaction_date_bounds};

const MAX_PROJECTION_MONTHS: i32 = 600;

#[derive(Clone, Debug, PartialEq)]
pub struct CrossoverPoint {
	pub month: YearMonth,
	pub investment_income: f64,
	pub expenses: f64,
	pub nest_egg: f64,
	/// Only on projected months: expenses times the adjustment factor.
	pub adjusted_expenses: Option<f64>,
	pub projection: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Crossover {
	pub start: YearMonth,
	pub end: YearMonth,
	pub points: Vec<CrossoverPoint>,
	pub crossover: Option<YearMonth>,
	pub last_known_balance: f64,
	pub last_known_monthly_income: f64,
	pub last_known_monthly_expenses: f64,
	/// The yearly growth the history implies.
	pub historical_return: Option<f64>,
	pub years_to_retire: Option<f64>,
	pub target_monthly_income: Option<f64>,
	pub target_nest_egg: Option<f64>,
}

fn js_round(x: f64) -> f64 {
	if !x.is_finite() {
		return x;
	}
	let floor = x.floor();
	if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

fn median(values: &[f64]) -> f64 {
	match values {
		[] => 0.0,
		[only] => *only,
		_ => {
			let mut sorted = values.to_vec();
			sorted.sort_by(f64::total_cmp);
			let mid = sorted.len() / 2;
			if sorted.len().is_multiple_of(2) {
				(sorted[mid - 1] + sorted[mid]) / 2.0
			} else {
				sorted[mid]
			}
		}
	}
}

#[allow(clippy::cast_precision_loss)]
fn mean(values: &[f64]) -> f64 {
	if values.is_empty() {
		return 0.0;
	}
	values.iter().fold(0.0, |sum, value| sum + value) / values.len() as f64
}

fn hampel_median(values: &[f64]) -> f64 {
	match values {
		[] => 0.0,
		[only] => *only,
		_ => {
			let middle = median(values);
			let deviations: Vec<f64> = values.iter().map(|value| (value - middle).abs()).collect();
			let mad = median(&deviations);
			let lower = middle - 1.4826 * mad * 3.0;
			let upper = middle + 1.4826 * mad * 3.0;
			let kept: Vec<f64> = values
				.iter()
				.copied()
				.filter(|value| *value >= lower && *value <= upper)
				.collect();
			median(&kept)
		}
	}
}

fn month_of(date: NaiveDate) -> Result<YearMonth, CoreError> {
	YearMonth::new(date.year(), date.month())
}

fn month_text(text: &str) -> Result<YearMonth, CoreError> {
	let date = parse_date(text)
		.ok_or_else(|| CoreError::Invalid(format!("the crossover date {text:?} isn't a date")))?;
	month_of(date)
}

fn months_between(start: YearMonth, end: YearMonth) -> Result<Vec<YearMonth>, CoreError> {
	let mut months = Vec::new();
	let mut month = start;
	while month <= end {
		months.push(month);
		month = month.offset(1)?;
	}
	Ok(months)
}

fn ids(meta: Option<&Json>, key: &str) -> Option<Vec<String>> {
	meta.and_then(|meta| meta.get(key))
		.and_then(Json::as_array)
		.map(|values| {
			values
				.iter()
				.filter_map(Json::as_str)
				.map(str::to_owned)
				.collect()
		})
}

fn number(meta: Option<&Json>, key: &str) -> Option<f64> {
	meta.and_then(|meta| meta.get(key)).and_then(Json::as_f64)
}

/// The months the card covers: its time frame as of last month, moved back a
/// month for a sliding window, kept between the first transaction's month
/// and last month.
fn card_months(
	budget: &BudgetData,
	meta: Option<&Json>,
	today: NaiveDate,
) -> Result<(YearMonth, YearMonth), CoreError> {
	let this_month = month_of(today)?;
	let previous = this_month.offset(-1)?;
	let has_rows = budget.transactions.values().any(|row| !row.tombstone);
	let earliest = if has_rows {
		month_of(transaction_date_bounds(budget, today).0)?
	} else {
		previous
	};
	if earliest > previous {
		return Err(CoreError::Invalid(
			"the crossover card has no whole month of history yet".to_owned(),
		));
	}
	let default_start = this_month.offset(-120)?.to_string();
	let default_end = previous.to_string();
	let range = calculate_time_range(
		meta.and_then(|meta| meta.get("timeFrame")),
		Some((&default_start, &default_end, "full")),
		Some(previous.first_day().naive()),
		today,
	);
	let clamp = |month: YearMonth| month.max(earliest).min(previous);
	let (start, end) = match range.mode.as_str() {
		"sliding-window" => (
			clamp(month_text(&range.start)?.offset(-1)?),
			clamp(month_text(&range.end)?.offset(-1)?),
		),
		"full" => (earliest, previous),
		_ => (
			clamp(month_text(&range.start)?),
			clamp(month_text(&range.end)?),
		),
	};
	Ok((start, end.max(start)))
}

pub fn crossover(
	budget: &BudgetData,
	meta: Option<&Json>,
	today: NaiveDate,
) -> Result<Crossover, CoreError> {
	let show_hidden = meta
		.and_then(|meta| meta.get("showHiddenCategories"))
		.and_then(Json::as_bool)
		.unwrap_or(false);
	let stored_categories = ids(meta, "expenseCategoryIds");
	let categories: Vec<String> = budget
		.categories
		.values()
		.filter(|category| !category.tombstone)
		.filter(|category| match &stored_categories {
			Some(stored) => stored.iter().any(|id| *id == category.id.as_str()),
			None => !category.is_income,
		})
		.filter(|category| show_hidden || !category.hidden)
		.map(|category| category.id.to_string())
		.collect();
	let accounts: Vec<String> = ids(meta, "incomeAccountIds").unwrap_or_else(|| {
		budget
			.accounts
			.values()
			.filter(|account| !account.tombstone)
			.map(|account| account.id.to_string())
			.collect()
	});
	let swr = number(meta, "safeWithdrawalRate").unwrap_or(0.04);
	let estimated_return = number(meta, "estimatedReturn");
	let contribution = number(meta, "expectedContribution").unwrap_or(0.0);
	let adjustment = number(meta, "expenseAdjustmentFactor").unwrap_or(1.0);
	let projection = meta
		.and_then(|meta| meta.get("projectionType"))
		.and_then(Json::as_str)
		.unwrap_or("hampel")
		.to_owned();

	let (start, end) = card_months(budget, meta, today)?;
	if accounts.is_empty() {
		return Ok(Crossover {
			start,
			end,
			points: Vec::new(),
			crossover: None,
			last_known_balance: 0.0,
			last_known_monthly_income: 0.0,
			last_known_monthly_expenses: 0.0,
			historical_return: None,
			years_to_retire: None,
			target_monthly_income: None,
			target_nest_egg: None,
		});
	}
	let months = months_between(start, end)?;
	let first_day = start.first_day().naive();
	let last_day = end
		.offset(1)?
		.first_day()
		.naive()
		.pred_opt()
		.unwrap_or(first_day);
	let start_end = start
		.offset(1)?
		.first_day()
		.naive()
		.pred_opt()
		.unwrap_or(first_day);

	let mut spend: BTreeMap<YearMonth, i64> = BTreeMap::new();
	let mut starting = 0_i64;
	let mut deltas: BTreeMap<YearMonth, i64> = BTreeMap::new();
	for row in budget
		.transactions
		.values()
		.filter(|row| is_row(budget, row))
	{
		let date = row.date.naive();
		let amount = row.amount.minor();
		let category = row
			.category
			.as_ref()
			.and_then(|id| budget.resolve_category(id))
			.map(ToString::to_string);
		if date >= first_day
			&& date <= last_day
			&& category.is_some_and(|category| categories.contains(&category))
		{
			*spend.entry(month_of(date)?).or_default() -= amount;
		}
		let account = row.account.as_str();
		let count = accounts.iter().filter(|id| id.as_str() == account).count();
		if count == 0 {
			continue;
		}
		let weighted = amount * i64::try_from(count).unwrap_or(1);
		if date <= start_end {
			starting += weighted;
		}
		if date > start_end && date <= last_day {
			*deltas.entry(month_of(date)?).or_default() += weighted;
		}
	}
	#[allow(clippy::cast_precision_loss)]
	let as_f64 = |value: i64| value as f64;
	let mut balances = Vec::with_capacity(months.len());
	let mut running = starting;
	for month in &months {
		running += deltas.get(month).copied().unwrap_or(0);
		balances.push(as_f64(running));
	}
	let monthly_swr = swr / 12.0;
	let mut points = Vec::new();
	let mut last_balance = 0.0;
	let mut last_expense = 0.0;
	let expenses: Vec<f64> = months
		.iter()
		.map(|month| as_f64(spend.get(month).copied().unwrap_or(0)))
		.collect();
	for ((month, balance), expense) in months.iter().zip(&balances).zip(&expenses) {
		points.push(CrossoverPoint {
			month: *month,
			investment_income: js_round(balance * monthly_swr),
			expenses: *expense,
			nest_egg: *balance,
			adjusted_expenses: None,
			projection: false,
		});
		last_balance = *balance;
		last_expense = *expense;
	}

	let mut monthly_return = estimated_return.map(|annual| (1.0 + annual).powf(1.0 / 12.0) - 1.0);
	let mut default_return = None;
	if balances.len() >= 2 {
		let mut first = balances[0];
		let last = balances[balances.len() - 1];
		#[allow(clippy::cast_precision_loss)]
		let n = (balances.len() - 1) as f64;
		if first == 0.0
			&& let Some(found) = balances.iter().skip(1).find(|balance| **balance != 0.0)
		{
			first = *found;
		}
		default_return = Some(if first > 0.0 && last > 0.0 {
			let cagr = (last / first).powf(1.0 / n) - 1.0;
			if cagr.is_finite() { cagr } else { 0.0 }
		} else {
			0.0
		});
	}
	let mut crossover_index = None;
	if !months.is_empty() {
		if monthly_return.is_none() {
			monthly_return = default_return;
		}
		let flat = match projection.as_str() {
			"hampel" => hampel_median(&expenses),
			"median" => median(&expenses),
			"mean" => mean(&expenses),
			_ => 0.0,
		};
		let mut balance = last_balance;
		let mut month = end;
		for step in 1..=MAX_PROJECTION_MONTHS {
			month = month.offset(1)?;
			balance += contribution;
			if let Some(rate) = monthly_return {
				balance *= 1.0 + rate;
			}
			let income = balance * monthly_swr;
			let projected = flat.max(0.0);
			let adjusted = projected * adjustment;
			points.push(CrossoverPoint {
				month,
				investment_income: js_round(income),
				expenses: js_round(projected),
				nest_egg: js_round(balance),
				adjusted_expenses: Some(js_round(adjusted)),
				projection: true,
			});
			if js_round(income) >= js_round(adjusted) {
				crossover_index = Some(months.len() + usize::try_from(step - 1).unwrap_or(0));
				break;
			}
		}
	}
	let crossover_point = crossover_index.and_then(|index| points.get(index));
	let (mut years, mut target_income, mut target_nest_egg) = (None, None, None);
	if let Some(point) = crossover_point {
		let this_month = month_of(today)?;
		let calendar = (i64::from(point.month.year()) - i64::from(this_month.year())) * 12
			+ i64::from(point.month.month())
			- i64::from(this_month.month());
		// date-fns `differenceInMonths` from today to the 1st of that month:
		// the last month is never whole.
		let whole = if calendar >= 1 { calendar - 1 } else { 0 };
		#[allow(clippy::cast_precision_loss)]
		let value = if whole > 0 { whole as f64 / 12.0 } else { 0.0 };
		years = Some(value);
		target_income = point.adjusted_expenses;
		target_nest_egg = target_income.map(|income| js_round(income / monthly_swr));
	}
	let last_known = balances.last().copied().unwrap_or(0.0);
	Ok(Crossover {
		start,
		end,
		crossover: crossover_point.map(|point| point.month),
		points,
		last_known_balance: last_known,
		last_known_monthly_income: js_round(last_known * monthly_swr),
		last_known_monthly_expenses: last_expense,
		historical_return: default_return.map(|monthly| (1.0 + monthly).powf(12.0) - 1.0),
		years_to_retire: years,
		target_monthly_income: target_income,
		target_nest_egg,
	})
}
