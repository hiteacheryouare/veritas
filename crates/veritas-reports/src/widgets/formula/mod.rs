//! The formula card (`components/reports/reports/FormulaCard.tsx`,
//! `FormulaResult.tsx`, `hooks/useFormulaExecution.ts`): an Excel-style
//! formula reading its named queries' transaction sums and the envelope
//! budget. This is Actual's plumbing around the formula; the formula itself
//! goes to `engine` (D75).
//!
//! `colorFormula` is left out: its `theme_*` variables are the web app's CSS
//! colors.

mod engine;
mod months;

use std::collections::HashMap;

use chrono::{Datelike, NaiveDate};
use veritas_domain::{
	BudgetData, BudgetType, Category, CategoryGroupId, CategoryId, CoreError, Money, YearMonth,
};
use veritas_engine::EnvelopeSheet;

use self::FormulaErrorKind as ErrorKind;
use self::engine::{BudgetRequest, Names};
use super::budget_analysis::{category_condition, category_passes};
use super::{TimeRange, calculate_time_range, filtered_rows, filtered_rows_between};
use crate::filter::TransactionFilter;
use crate::json::Json;
use crate::report_query::{parse_date, unsupported};

/// The formula a new card shows (`FormulaCard.tsx:44`).
pub const DEFAULT_FORMULA: &str = "=SUM(1, 2, 3)";

/// The error types Actual's card names in `Formula error: <type>`, the one
/// it raises itself for text that isn't a formula, and `NotImplemented`
/// while Veritas has no formula engine (D75).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FormulaErrorKind {
	NotAFormula,
	NotImplemented,
	Cycle,
	DivByZero,
	Error,
	Na,
	Name,
	Num,
	Ref,
	Spill,
	Value,
}

impl FormulaErrorKind {
	/// The type Actual's card names, `None` for the two it doesn't.
	pub fn error_type(self) -> Option<&'static str> {
		Some(match self {
			FormulaErrorKind::NotAFormula | FormulaErrorKind::NotImplemented => return None,
			FormulaErrorKind::Cycle => "CYCLE",
			FormulaErrorKind::DivByZero => "DIV_BY_ZERO",
			FormulaErrorKind::Error => "ERROR",
			FormulaErrorKind::Na => "NA",
			FormulaErrorKind::Name => "NAME",
			FormulaErrorKind::Num => "NUM",
			FormulaErrorKind::Ref => "REF",
			FormulaErrorKind::Spill => "SPILL",
			FormulaErrorKind::Value => "VALUE",
		})
	}
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormulaError {
	pub kind: FormulaErrorKind,
	/// The text Actual's card shows: `Formula error: <type>` or
	/// `Formula must start with =` (`useFormulaExecution.ts:127,174`).
	pub message: String,
}

impl FormulaError {
	fn new(kind: FormulaErrorKind) -> Self {
		let message = match (kind, kind.error_type()) {
			(_, Some(name)) => format!("Formula error: {name}"),
			(FormulaErrorKind::NotImplemented, None) => {
				"Veritas can't compute formulas yet.".to_owned()
			}
			(_, None) => "Formula must start with =".to_owned(),
		};
		FormulaError { kind, message }
	}
}

/// The formula's value: a number already rounded to 10 significant digits,
/// as Actual exports it, not yet formatted as money.
#[derive(Clone, Debug, PartialEq)]
pub enum FormulaValue {
	Number(f64),
	Text(String),
	Boolean(bool),
	Empty,
	Error(FormulaError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontSizeMode {
	Dynamic,
	Static,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FormulaCard {
	pub formula: String,
	pub value: FormulaValue,
	pub show_title: bool,
	pub font_size_mode: FontSizeMode,
	pub static_font_size: f64,
}

/// JavaScript truthiness of a stored value.
fn truthy(value: Option<&Json>) -> bool {
	match value {
		None | Some(Json::Null) => false,
		Some(Json::Bool(flag)) => *flag,
		Some(Json::Number(text)) => text.parse::<f64>().is_ok_and(|n| n != 0.0 && !n.is_nan()),
		Some(Json::String(text)) => !text.is_empty(),
		Some(Json::Array(_) | Json::Object(_)) => true,
	}
}

fn text_field(value: Option<&Json>) -> Option<&str> {
	value.and_then(Json::as_str)
}

/// `FormulaCard.tsx:44-47`'s reading of the meta, then `executeFormula`
/// (`useFormulaExecution.ts:171-250`): three evaluations, fetching the
/// queries the first asked for, then the budget cells the second asked for.
pub fn formula_card(
	budget: &BudgetData,
	sheet: &EnvelopeSheet,
	meta: Option<&Json>,
	today: NaiveDate,
) -> Result<FormulaCard, CoreError> {
	let field = |key: &str| meta.and_then(|meta| meta.get(key));
	let formula = match field("formula") {
		Some(Json::String(text)) if !text.is_empty() => text.clone(),
		value if !truthy(value) => DEFAULT_FORMULA.to_owned(),
		_ => return Err(unsupported("a formula that isn't text".to_owned())),
	};
	let static_font_size = match field("staticFontSize").and_then(Json::as_f64) {
		Some(size) if size != 0.0 && !size.is_nan() => size,
		_ => 32.0,
	};
	let font_size_mode = if text_field(field("fontSizeMode")) == Some("static") {
		FontSizeMode::Static
	} else {
		FontSizeMode::Dynamic
	};
	let show_title = match field("showTitle") {
		None | Some(Json::Null) => true,
		value => truthy(value),
	};
	let value = evaluate(budget, sheet, field("queries"), &formula, today)?;
	Ok(FormulaCard {
		formula,
		value,
		show_title,
		font_size_mode,
		static_font_size,
	})
}

fn evaluate(
	budget: &BudgetData,
	sheet: &EnvelopeSheet,
	queries: Option<&Json>,
	formula: &str,
	today: NaiveDate,
) -> Result<FormulaValue, CoreError> {
	if !formula.starts_with('=') {
		return Ok(FormulaValue::Error(FormulaError::new(
			ErrorKind::NotAFormula,
		)));
	}
	let mut data = Data {
		budget,
		sheet,
		queries,
		today,
		cells: None,
		cell_index: HashMap::new(),
	};
	let mut session = Names::default();
	let run = |session: &mut Names| engine::evaluate(formula, session, today);
	run(&mut session)?;
	for name in session.query_names.clone() {
		let sum = data.query_sum(&name)?;
		session.query_sums.insert(name, sum);
	}
	for name in session.category_names.clone() {
		let ids = data.query_categories(&name)?;
		session.categories.insert(name, ids);
	}
	for name in session.start_names.clone() {
		let start = data.query_months(&name)?.start;
		session.starts.insert(name, start);
	}
	for name in session.end_names.clone() {
		let end = data.query_months(&name)?.end;
		session.ends.insert(name, end);
	}
	session.budget_requests.clear();
	run(&mut session)?;
	for request in session.budget_requests.clone() {
		match data.budget_value(&request)? {
			Ok(value) => {
				session.budget_errors.remove(&request);
				session.budget_values.insert(request, value);
			}
			Err(message) => {
				session.budget_values.remove(&request);
				session.budget_errors.insert(request, message);
			}
		}
	}
	run(&mut session)
}

/// A cell `envelope-budget-month` lists (`server/budget/app.ts:209-261`).
#[derive(Clone, Copy, Debug)]
enum Cell<'a> {
	GroupBudget(&'a CategoryGroupId),
	GroupSpent(&'a CategoryGroupId),
	GroupLeftover(&'a CategoryGroupId),
	Budget(&'a CategoryId),
	Spent(&'a CategoryId),
	Leftover(&'a CategoryId),
	Carryover(&'a CategoryId),
	Goal(&'a CategoryId),
	LongGoal(&'a CategoryId),
}

/// A cell's value; a missing or NULL cell reads as 0.
#[derive(Clone, Copy, Debug, PartialEq)]
enum CellValue {
	Amount(i64),
	Flag(bool),
	Null,
}

impl CellValue {
	fn money(self) -> Money {
		Money::from_minor(self.number())
	}

	fn number(self) -> i64 {
		match self {
			CellValue::Amount(amount) => amount,
			CellValue::Flag(flag) => i64::from(flag),
			CellValue::Null => 0,
		}
	}
}

/// `Object.prototype`'s keys, which `queries[name]` finds on any object.
const PROTOTYPE_KEYS: [&str; 12] = [
	"__proto__",
	"__defineGetter__",
	"__defineSetter__",
	"__lookupGetter__",
	"__lookupSetter__",
	"constructor",
	"hasOwnProperty",
	"isPrototypeOf",
	"propertyIsEnumerable",
	"toLocaleString",
	"toString",
	"valueOf",
];

struct Data<'a> {
	budget: &'a BudgetData,
	sheet: &'a EnvelopeSheet,
	queries: Option<&'a Json>,
	today: NaiveDate,
	cells: Option<Vec<(String, Cell<'a>)>>,
	cell_index: HashMap<String, Option<usize>>,
}

impl<'a> Data<'a> {
	/// `queries[name]`, `None` when it is missing or falsy.
	fn query(&self, name: &str) -> Result<Option<&'a Json>, CoreError> {
		let entries = match self.queries {
			None | Some(Json::Null) => return Ok(None),
			Some(Json::Object(entries)) => entries,
			Some(_) => {
				return Err(unsupported(
					"formula queries that aren't an object".to_owned(),
				));
			}
		};
		let Some((_, config)) = entries.iter().rev().find(|(key, _)| key == name) else {
			if PROTOTYPE_KEYS.contains(&name) {
				return Err(unsupported(format!("a query named {name:?}")));
			}
			return Ok(None);
		};
		match config {
			Json::Object(_) => Ok(Some(config)),
			config if !truthy(Some(config)) => Ok(None),
			_ => Err(unsupported(format!(
				"the query {name:?}, which isn't an object"
			))),
		}
	}

	/// `calculateTimeRange(asMonthSlidingTimeFrame(timeFrame))`.
	fn time_range(&self, frame: &Json) -> Result<TimeRange, CoreError> {
		let Json::Object(entries) = frame else {
			return Ok(calculate_time_range(Some(frame), None, None, self.today));
		};
		// Only a sliding window reads its months; other modes pass them through.
		let sliding = matches!(frame.get("mode"), None | Some(Json::Null))
			|| text_field(frame.get("mode")) == Some("sliding-window");
		for key in ["start", "end", "mode"] {
			match frame.get(key) {
				None | Some(Json::Null) => {}
				Some(Json::String(text))
					if key == "mode"
						|| !sliding || text.is_empty()
						|| parse_date(text).is_some() => {}
				Some(value) => {
					return Err(unsupported(format!(
						"a query time frame {key} of {}",
						value.stringify()
					)));
				}
			}
		}
		let start = text_field(frame.get("start")).unwrap_or_default();
		let end = text_field(frame.get("end")).unwrap_or_default();
		if text_field(frame.get("mode")) != Some("sliding-window")
			|| start.is_empty()
			|| end.is_empty()
		{
			return Ok(calculate_time_range(Some(frame), None, None, self.today));
		}
		let month = |text: &str| Json::String(text.chars().take(7).collect());
		let entries = entries
			.iter()
			.map(|(key, value)| match key.as_str() {
				"start" => (key.clone(), month(start)),
				"end" => (key.clone(), month(end)),
				_ => (key.clone(), value.clone()),
			})
			.collect();
		Ok(calculate_time_range(
			Some(&Json::Object(entries)),
			None,
			None,
			self.today,
		))
	}

	/// `QUERY_EXTRACT_TIMEFRAME_START` and `_END`: this month when the
	/// query has no time frame.
	fn query_months(&self, name: &str) -> Result<TimeRange, CoreError> {
		let frame = self.query(name)?.and_then(|config| config.get("timeFrame"));
		match frame {
			Some(frame) if truthy(Some(frame)) => self.time_range(frame),
			_ => {
				let month = self.today.format("%Y-%m").to_string();
				Ok(TimeRange {
					start: month.clone(),
					end: month,
					mode: String::new(),
				})
			}
		}
	}

	/// `fetchQuerySum` through `integerToAmount`; a missing query is 0
	/// (`prefetchFormulaQueries`).
	fn query_sum(&self, name: &str) -> Result<f64, CoreError> {
		let Some(config) = self.query(name)? else {
			return Ok(0.0);
		};
		let conditions = match config.get("conditions") {
			Some(Json::Array(conditions)) => conditions.as_slice(),
			value if !truthy(value) => &[],
			// `conditionsToAQL` throws on anything else, and `fetchQuerySum`
			// reads a throw as 0.
			_ => return Ok(0.0),
		};
		let mut stripped = Vec::with_capacity(conditions.len());
		for condition in conditions {
			let Json::Object(entries) = condition else {
				return Err(unsupported(
					"a query condition that isn't an object".to_owned(),
				));
			};
			let entries = entries
				.iter()
				.filter(|(key, _)| key != "customName")
				.cloned()
				.collect();
			stripped.push(Json::Object(entries));
		}
		let any = text_field(config.get("conditionsOp")) == Some("or");
		let filter = TransactionFilter::new(&stripped, any)?;
		let frame = config
			.get("timeFrame")
			.filter(|frame| matches!(frame, Json::Object(_)));
		let mode = frame.and_then(|frame| frame.get("mode"));
		let mut total = Money::ZERO;
		if truthy(mode) {
			if !matches!(mode, Some(Json::String(_))) {
				return Err(unsupported(
					"a query time frame mode that isn't text".to_owned(),
				));
			}
			let range = self.time_range(frame.unwrap_or(&Json::Null))?;
			let month_like = |text: &str| text.split('-').count() == 2;
			let first = months::parse_month_date(&range.start).map(|date| {
				if month_like(&range.start) {
					date.with_day(1).unwrap_or(date)
				} else {
					date
				}
			});
			let last = months::parse_month_date(&range.end).map(|date| {
				if month_like(&range.end) {
					month_end(date)
				} else {
					date
				}
			});
			let (Some(first), Some(last)) = (first, last) else {
				return Ok(0.0);
			};
			check_year(first)?;
			check_year(last)?;
			for row in filtered_rows_between(self.budget, &filter, first, last) {
				total = total.checked_add(row?.amount)?;
			}
		} else {
			for row in filtered_rows(self.budget, &filter) {
				total = total.checked_add(row?.amount)?;
			}
		}
		Ok(total.minor() as f64 / 100.0)
	}

	/// `extractQueryCategories` and `getCategoriesFromConditions`
	/// (`useFormulaExecution.ts:451-590`).
	fn query_categories(&self, name: &str) -> Result<Vec<String>, CoreError> {
		let Some(config) = self.query(name)? else {
			return Ok(Vec::new());
		};
		let conditions = match config.get("conditions") {
			Some(Json::Array(conditions)) => conditions.as_slice(),
			value if !truthy(value) => &[],
			_ => {
				return Err(unsupported(
					"query conditions that aren't a list".to_owned(),
				));
			}
		};
		let mut tests = Vec::new();
		for condition in conditions {
			if !matches!(condition, Json::Object(_)) {
				return Err(unsupported(
					"a query condition that isn't an object".to_owned(),
				));
			}
			let field = text_field(condition.get("field"));
			if truthy(condition.get("customName"))
				|| !matches!(field, Some("category" | "category_group"))
			{
				continue;
			}
			let value = condition.get("value");
			let valid = match text_field(condition.get("op")) {
				Some("contains" | "doesNotContain") => matches!(value, Some(Json::String(_))),
				Some("oneOf" | "notOneOf") => matches!(value, Some(Json::Array(_))),
				Some("matches") => {
					text_field(value).is_some_and(|text| text.encode_utf16().count() <= 256)
				}
				_ => true,
			};
			if !valid {
				return Err(unsupported(format!(
					"the category condition {} in a formula query",
					condition.stringify()
				)));
			}
			tests.push(category_condition(condition)?);
		}
		let any = text_field(config.get("conditionsOp")) == Some("or");
		let mut categories: Vec<&Category> = self
			.budget
			.categories
			.values()
			.filter(|category| !category.tombstone)
			.collect();
		categories.sort_by(|a, b| {
			a.sort_order
				.total_cmp(&b.sort_order)
				.then_with(|| a.id.cmp(&b.id))
		});
		let mut ids = Vec::new();
		for category in categories {
			let keep = if tests.is_empty() {
				!category.is_income && !category.hidden
			} else {
				let mut results = Vec::with_capacity(tests.len());
				for test in &tests {
					results.push(category_passes(test, self.budget, category)?);
				}
				if any {
					results.contains(&true)
				} else {
					!results.contains(&false)
				}
			};
			if keep {
				ids.push(category.id.to_string());
			}
		}
		Ok(ids)
	}

	/// The cells `envelope-budget-month` lists, in its order.
	fn cells(&mut self) -> &[(String, Cell<'a>)] {
		let (budget, sheet) = (self.budget, self.sheet);
		self.cells.get_or_insert_with(|| {
			let mut cells = Vec::new();
			for (group, categories) in sheet.group_order() {
				let income = budget
					.category_groups
					.get(group)
					.is_some_and(|g| g.is_income);
				if income {
					for category in categories {
						cells.push((format!("sum-amount-{category}"), Cell::Spent(category)));
					}
					continue;
				}
				cells.push((format!("group-budget-{group}"), Cell::GroupBudget(group)));
				cells.push((format!("group-sum-amount-{group}"), Cell::GroupSpent(group)));
				cells.push((
					format!("group-leftover-{group}"),
					Cell::GroupLeftover(group),
				));
				for category in categories {
					cells.push((format!("budget-{category}"), Cell::Budget(category)));
					cells.push((format!("sum-amount-{category}"), Cell::Spent(category)));
					cells.push((format!("leftover-{category}"), Cell::Leftover(category)));
					cells.push((format!("carryover-{category}"), Cell::Carryover(category)));
					cells.push((format!("goal-{category}"), Cell::Goal(category)));
					cells.push((format!("long-goal-{category}"), Cell::LongGoal(category)));
				}
			}
			cells
		})
	}

	/// `getMonthDataValue`: the first cell whose name ends with the field.
	fn cell(&mut self, pattern: &str, id: &str) -> Option<Cell<'a>> {
		let field = format!("{pattern}{id}");
		if let Some(&found) = self.cell_index.get(&field) {
			return found.map(|index| self.cells()[index].1);
		}
		let found = self
			.cells()
			.iter()
			.position(|(name, _)| name.ends_with(&field));
		self.cell_index.insert(field, found);
		found.map(|index| self.cells()[index].1)
	}

	fn read(&mut self, month: Option<YearMonth>, pattern: &str, id: &str) -> CellValue {
		let (Some(month), Some(cell)) = (month, self.cell(pattern, id)) else {
			return CellValue::Null;
		};
		let sheet = self.sheet;
		let row = |category: &CategoryId| self.budget.budget_cells.get(&(month, category.clone()));
		let amount = |money: veritas_domain::Money| CellValue::Amount(money.minor());
		let in_sheet = sheet.contains(month);
		match cell {
			Cell::GroupBudget(group) => sheet
				.group(month, group)
				.map_or(CellValue::Null, |g| amount(g.budgeted)),
			Cell::GroupSpent(group) => sheet
				.group(month, group)
				.map_or(CellValue::Null, |g| amount(g.spent)),
			Cell::GroupLeftover(group) => sheet
				.group(month, group)
				.map_or(CellValue::Null, |g| amount(g.balance)),
			Cell::Budget(category) if in_sheet => sheet
				.category(month, category)
				.map_or(CellValue::Null, |c| amount(c.budgeted)),
			Cell::Budget(category) => {
				row(category).map_or(CellValue::Null, |row| amount(row.budgeted))
			}
			Cell::Spent(category) => sheet
				.category(month, category)
				.map_or(CellValue::Null, |c| amount(c.spent)),
			Cell::Leftover(category) => sheet
				.category(month, category)
				.map_or(CellValue::Null, |c| amount(c.balance)),
			Cell::Carryover(category) if in_sheet => sheet
				.category(month, category)
				.map_or(CellValue::Null, |c| CellValue::Flag(c.carryover)),
			Cell::Carryover(category) => {
				row(category).map_or(CellValue::Null, |row| CellValue::Flag(row.carryover))
			}
			Cell::Goal(category) => row(category)
				.and_then(|row| row.goal)
				.map_or(CellValue::Null, amount),
			Cell::LongGoal(category) => row(category).map_or(CellValue::Null, |row| {
				CellValue::Amount(i64::from(row.long_goal))
			}),
		}
	}

	/// `fetchBudgetDimensionValueDirect` (`useFormulaExecution.ts:631-740`).
	/// `Err` is the message of the error Actual throws, which BUDGET_QUERY
	/// shows as `#VALUE!`.
	fn budget_value(&mut self, request: &BudgetRequest) -> Result<Result<f64, String>, CoreError> {
		if self.budget.budget_type == BudgetType::Tracking {
			return Err(unsupported("BUDGET_QUERY on a tracking budget".to_owned()));
		}
		let dimension = request.dimension.to_lowercase();
		if !["budgeted", "spent", "balance_start", "balance_end", "goal"]
			.contains(&dimension.as_str())
		{
			return Ok(Err(format!(
				"Invalid BUDGET_QUERY dimension: {}",
				request.dimension
			)));
		}
		let invalid = || Ok(Err("Invalid time value".to_owned()));
		let (Some(start), Some(end)) = (
			months::parse_month_date(&request.start),
			months::parse_month_date(&request.end),
		) else {
			return invalid();
		};
		check_year(start)?;
		check_year(end)?;
		let index = |date: NaiveDate| i64::from(date.year()) * 12 + i64::from(date.month0());
		let (first, last) = (index(start), index(end));
		let months: Vec<i64> = if first < last {
			(first..=last).collect()
		} else {
			vec![first]
		};
		let month = |index: i64| {
			i32::try_from(index.div_euclid(12)).ok().and_then(|year| {
				YearMonth::new(year, u32::try_from(index.rem_euclid(12) + 1).ok()?).ok()
			})
		};
		let ids = &request.categories;
		let pattern = match dimension.as_str() {
			"budgeted" => Some("budget-"),
			"spent" => Some("sum-amount-"),
			"goal" => Some("goal-"),
			_ => None,
		};
		if let Some(pattern) = pattern {
			let mut total = Money::ZERO;
			for &index in &months {
				for id in ids {
					total = total.checked_add(self.read(month(index), pattern, id).money())?;
				}
			}
			return Ok(Ok(total.minor() as f64 / 100.0));
		}
		let carried = |balance: i64, carryover: CellValue| {
			balance > 0 || (balance < 0 && carryover.number() != 0)
		};
		let mut running = Money::ZERO;
		for id in ids {
			let before = month(first - 1);
			let balance = self.read(before, "leftover-", id).number();
			if carried(balance, self.read(before, "carryover-", id)) {
				running = running.checked_add(Money::from_minor(balance))?;
			}
		}
		let mut balances = Vec::with_capacity(months.len());
		for &index in &months {
			let (mut budgeted, mut spent, mut next) = (Money::ZERO, Money::ZERO, Money::ZERO);
			for id in ids {
				budgeted = budgeted.checked_add(self.read(month(index), "budget-", id).money())?;
				spent = spent.checked_add(self.read(month(index), "sum-amount-", id).money())?;
				let balance = self.read(month(index), "leftover-", id).number();
				if carried(balance, self.read(month(index), "carryover-", id)) {
					next = next.checked_add(Money::from_minor(balance))?;
				}
			}
			let end = budgeted.checked_add(spent)?.checked_add(running)?;
			balances.push((running, end));
			running = next;
		}
		let value = if dimension == "balance_start" {
			balances.first().map_or(Money::ZERO, |(start, _)| *start)
		} else {
			balances.last().map_or(Money::ZERO, |(_, end)| *end)
		};
		Ok(Ok(value.minor() as f64 / 100.0))
	}
}

fn month_end(date: NaiveDate) -> NaiveDate {
	date.with_day(1)
		.and_then(|first| first.checked_add_months(chrono::Months::new(1)))
		.and_then(|next| next.pred_opt())
		.unwrap_or(date)
}

/// Dates past year 9999 or before 1000 print differently in date-fns.
fn check_year(date: NaiveDate) -> Result<(), CoreError> {
	if (1000..=9999).contains(&date.year()) {
		Ok(())
	} else {
		Err(unsupported(format!("the query date {date}")))
	}
}
