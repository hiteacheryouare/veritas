//! The budget analysis card (`BudgetAnalysisCard.tsx`,
//! `spreadsheets/budget-analysis-spreadsheet.ts`): per month, what the chosen
//! expense categories were budgeted, spent and held, read from the envelope
//! sheet, with overspending in categories without rollover shown as the next
//! month's adjustment.

use chrono::{Datelike, NaiveDate};
use serde_json::json;
use veritas_domain::{BudgetData, BudgetType, Category, CoreError, Money, YearMonth};
use veritas_engine::EnvelopeSheet;
use veritas_rules::{Condition, ConditionValue};

use super::calculate_time_range;
use crate::json::Json;
use crate::report_query::{parse_date, unsupported};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BudgetAnalysisMonth {
	pub month: YearMonth,
	pub budgeted: Money,
	pub spent: Money,
	pub balance: Money,
	/// Last month's overspending that this month's balance didn't carry,
	/// as a positive amount.
	pub overspending_adjustment: Money,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BudgetAnalysis {
	pub start: NaiveDate,
	pub end: NaiveDate,
	pub months: Vec<BudgetAnalysisMonth>,
	pub total_budgeted: Money,
	pub total_spent: Money,
	pub total_overspending_adjustment: Money,
	/// The last month's overspending, negative, still to land next month.
	pub final_overspending_adjustment: Money,
}

#[derive(Default)]
struct Totals {
	budgeted: i64,
	spent: i64,
	carryover: i64,
	overspending: i64,
	has_data: bool,
}

/// `summarizeMonthCategories`. A category outside the month's sheet, or in
/// a deleted group, reads as zeros, as Actual's empty cells do.
fn month_totals(
	budget: &BudgetData,
	sheet: &EnvelopeSheet,
	month: YearMonth,
	categories: &[&Category],
) -> Totals {
	let mut totals = Totals::default();
	for category in categories {
		let group_live = budget
			.category_groups
			.get(&category.group)
			.is_some_and(|group| !group.tombstone);
		let Some(cell) = sheet.category(month, &category.id).filter(|_| group_live) else {
			continue;
		};
		let (budgeted, spent, balance) = (
			cell.budgeted.minor(),
			cell.spent.minor(),
			cell.balance.minor(),
		);
		if budgeted != 0 || spent != 0 || balance != 0 {
			totals.has_data = true;
		}
		totals.budgeted += budgeted;
		totals.spent += spent;
		if balance > 0 || (balance < 0 && cell.carryover) {
			totals.carryover += balance;
		} else if balance < 0 {
			totals.overspending += balance;
		}
	}
	totals
}

enum Test {
	Is(Option<String>),
	IsNot(Option<String>),
	OneOf(Option<Vec<Option<String>>>),
	NotOneOf(Option<Vec<Option<String>>>),
	Contains(Option<String>, bool),
	Matches(Option<ConditionValue>),
	Never,
}

struct CategoryCondition {
	group: bool,
	test: Test,
}

/// The card's own reading of category and category-group conditions, which
/// compares ids and names in memory rather than through a query.
fn category_condition(condition: &Json) -> Result<CategoryCondition, CoreError> {
	let group = condition.get("field").and_then(Json::as_str) == Some("category_group");
	let value = condition.get("value").unwrap_or(&Json::Null);
	let id = |value: &Json| value.as_str().map(str::to_owned);
	let list = || {
		value
			.as_array()
			.map(|values| values.iter().map(id).collect())
	};
	let test = match condition
		.get("op")
		.and_then(Json::as_str)
		.unwrap_or_default()
	{
		"is" => Test::Is(id(value)),
		"isNot" => Test::IsNot(id(value)),
		"oneOf" => Test::OneOf(list()),
		"notOneOf" => Test::NotOneOf(list()),
		"contains" => Test::Contains(id(value), true),
		"doesNotContain" => Test::Contains(id(value), false),
		"matches" => match value.as_str() {
			Some(source) if source.encode_utf16().count() <= 256 => {
				// `new RegExp(value, 'i')`. Lowercasing both sides gives the
				// same answer only while the pattern has no escapes.
				if source.contains('\\') {
					return Err(unsupported(
						"a case-insensitive regex with escapes in a budget analysis filter"
							.to_owned(),
					));
				}
				let parsed = Condition::parse(
					&json!({"field": "category", "op": "matches", "value": source.to_lowercase()}),
				)
				.map_err(|_| unsupported("a budget analysis filter regex".to_owned()))?;
				Test::Matches(Some(parsed.value))
			}
			_ => Test::Matches(None),
		},
		_ => Test::Never,
	};
	Ok(CategoryCondition { group, test })
}

fn category_passes(
	condition: &CategoryCondition,
	budget: &BudgetData,
	category: &Category,
) -> Result<bool, CoreError> {
	let key = if condition.group {
		category.group.to_string()
	} else {
		category.id.to_string()
	};
	let text = if condition.group {
		budget
			.category_groups
			.get(&category.group)
			.filter(|group| !group.tombstone)
			.map_or_else(|| key.clone(), |group| group.name.clone())
	} else {
		category.name.clone()
	};
	let is = |value: &Option<String>| value.as_deref() == Some(key.as_str());
	Ok(match &condition.test {
		Test::Is(value) => is(value),
		Test::IsNot(value) => !is(value),
		Test::OneOf(values) => values.as_ref().is_some_and(|values| values.iter().any(is)),
		Test::NotOneOf(values) => values.as_ref().is_some_and(|values| !values.iter().any(is)),
		Test::Contains(needle, wanted) => needle
			.as_ref()
			.is_some_and(|needle| text.to_lowercase().contains(&needle.to_lowercase()) == *wanted),
		Test::Matches(Some(ConditionValue::Pattern(pattern))) => pattern
			.test(&text.to_lowercase())
			.map_err(|_| unsupported("a budget analysis filter regex".to_owned()))?,
		Test::Matches(_) | Test::Never => false,
	})
}

fn month_end(date: NaiveDate) -> NaiveDate {
	date.with_day(1)
		.and_then(|first| first.checked_add_months(chrono::Months::new(1)))
		.and_then(|next| next.pred_opt())
		.unwrap_or(date)
}

/// `sheet` is the envelope sheet for `budget` as of `today`.
pub fn budget_analysis(
	budget: &BudgetData,
	sheet: &EnvelopeSheet,
	meta: Option<&Json>,
	today: NaiveDate,
) -> Result<BudgetAnalysis, CoreError> {
	if budget.budget_type == BudgetType::Tracking {
		return Err(unsupported(
			"budget analysis for a tracking budget".to_owned(),
		));
	}
	let range = calculate_time_range(
		meta.and_then(|meta| meta.get("timeFrame")),
		None,
		None,
		today,
	);
	let bad =
		|text: &str| CoreError::Invalid(format!("the budget analysis date {text:?} isn't a date"));
	let start = parse_date(&range.start)
		.and_then(|date| date.with_day(1))
		.ok_or_else(|| bad(&range.start))?;
	let end = month_end(parse_date(&range.end).ok_or_else(|| bad(&range.end))?);
	let show_hidden = meta
		.and_then(|meta| meta.get("showHiddenCategories"))
		.and_then(Json::as_bool)
		.unwrap_or(false);
	let mut base: Vec<&Category> = budget
		.categories
		.values()
		.filter(|category| !category.tombstone && !category.is_income)
		.filter(|category| show_hidden || !category.hidden)
		.collect();
	base.sort_by(|a, b| {
		a.sort_order
			.total_cmp(&b.sort_order)
			.then_with(|| a.id.cmp(&b.id))
	});
	let conditions: Vec<CategoryCondition> = meta
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
		.map(category_condition)
		.collect::<Result<_, _>>()?;
	let any = meta
		.and_then(|meta| meta.get("conditionsOp"))
		.and_then(Json::as_str)
		== Some("or");
	let mut included = Vec::new();
	for category in base {
		let keep = if conditions.is_empty() {
			true
		} else {
			let mut results = Vec::with_capacity(conditions.len());
			for condition in &conditions {
				results.push(category_passes(condition, budget, category)?);
			}
			if any {
				results.iter().any(|passed| *passed)
			} else {
				results.iter().all(|passed| *passed)
			}
		};
		if keep {
			included.push(category);
		}
	}

	let first = YearMonth::new(start.year(), start.month())?;
	let last = YearMonth::new(end.year(), end.month())?;
	let mut running = month_totals(budget, sheet, first.offset(-1)?, &included).carryover;
	let mut overspending_before = 0_i64;
	let (mut total_budgeted, mut total_spent, mut total_adjustment) = (0_i64, 0_i64, 0_i64);
	let mut months = Vec::new();
	let mut month = first;
	while month <= last {
		let totals = month_totals(budget, sheet, month, &included);
		let balance = totals.budgeted + totals.spent + running;
		total_budgeted += totals.budgeted;
		total_spent += totals.spent;
		total_adjustment += overspending_before.abs();
		months.push(BudgetAnalysisMonth {
			month,
			budgeted: Money::from_minor(totals.budgeted),
			spent: Money::from_minor(totals.spent),
			balance: Money::from_minor(balance),
			overspending_adjustment: Money::from_minor(overspending_before.abs()),
		});
		if totals.has_data {
			running = totals.carryover;
		}
		overspending_before = totals.overspending;
		month = month.offset(1)?;
	}
	Ok(BudgetAnalysis {
		start,
		end,
		months,
		total_budgeted: Money::from_minor(total_budgeted),
		total_spent: Money::from_minor(total_spent),
		total_overspending_adjustment: Money::from_minor(total_adjustment),
		final_overspending_adjustment: Money::from_minor(overspending_before),
	})
}
