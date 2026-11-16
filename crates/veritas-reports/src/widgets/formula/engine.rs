//! The formula engine's interface: what `useFormulaExecution`'s three
//! evaluations ask of the spreadsheet engine Actual runs a formula on.
//!
//! Veritas ships no engine yet. Actual's is HyperFormula, which is GPL, so a
//! clean-room engine for D75's functions takes its place; until then every
//! formula evaluates to `NotImplemented`.

use std::collections::HashMap;

use chrono::NaiveDate;
use veritas_domain::CoreError;

use super::{FormulaError, FormulaErrorKind, FormulaValue};

/// One `BUDGET_QUERY` call, as `createBudgetQueryPrefetchKey` tells calls
/// apart.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct BudgetRequest {
	pub dimension: String,
	pub categories: Vec<String>,
	pub start: String,
	pub end: String,
}

/// `FormulaQueryContext`: what an evaluation asked for, which the engine
/// records, and what was fetched for it, which the engine reads on the next
/// evaluation. JavaScript's `Set` and `Map` keep insertion order, so the
/// names are lists.
#[derive(Default)]
pub(super) struct Names {
	pub query_names: Vec<String>,
	pub category_names: Vec<String>,
	pub start_names: Vec<String>,
	pub end_names: Vec<String>,
	pub budget_requests: Vec<BudgetRequest>,
	pub query_sums: HashMap<String, f64>,
	pub categories: HashMap<String, Vec<String>>,
	pub starts: HashMap<String, String>,
	pub ends: HashMap<String, String>,
	pub budget_values: HashMap<BudgetRequest, f64>,
	pub budget_errors: HashMap<BudgetRequest, String>,
}

/// One evaluation of `formula` (which starts with `=`) as of `today`. A
/// number comes back as Actual exports it, rounded to 10 significant
/// digits.
#[allow(clippy::unnecessary_wraps, reason = "the engine to come can fail")]
pub(super) fn evaluate(
	_formula: &str,
	_names: &mut Names,
	_today: NaiveDate,
) -> Result<FormulaValue, CoreError> {
	Ok(FormulaValue::Error(FormulaError::new(
		FormulaErrorKind::NotImplemented,
	)))
}
