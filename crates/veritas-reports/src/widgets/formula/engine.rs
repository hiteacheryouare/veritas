//! Written clean-room from HyperFormula's public docs and black-box tests
//! against Actual's formula card. No HyperFormula source was read.
//!
//! The formula engine's interface: what `useFormulaExecution`'s three
//! evaluations ask of the spreadsheet engine Actual runs a formula on.
//! Actual's is HyperFormula, which is GPL, so Veritas evaluates D75's
//! functions with its own engine: `parser`, `evaluator`, `functions`,
//! `value` and `datetime`.

use std::collections::HashMap;

use chrono::NaiveDate;
use veritas_domain::CoreError;

use super::evaluator::{self, Evaluator};
use super::parser::{self, ParseError};
use super::value::{self, Stop, Value};
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
/// number comes back as Actual exports it, rounded to 11 significant
/// digits. `Err` is a formula Veritas refuses to evaluate.
pub(super) fn evaluate(
	formula: &str,
	names: &mut Names,
	today: NaiveDate,
) -> Result<FormulaValue, CoreError> {
	let body = formula.strip_prefix('=').unwrap_or(formula);
	let tree = match parser::parse(body) {
		Ok(tree) => tree,
		Err(ParseError::Syntax) => return Ok(error(FormulaErrorKind::Error)),
		Err(ParseError::Refuse(refusal)) => return Err(refusal),
	};
	let mut evaluator = Evaluator { names, today };
	let outcome = evaluator::check(&tree).and_then(|()| evaluator.evaluate(&tree));
	match outcome {
		Ok(Value::Number(number)) => Ok(FormulaValue::Number(value::export_number(number))),
		Ok(Value::Text(text)) => Ok(FormulaValue::Text(text)),
		Ok(Value::Bool(flag)) => Ok(FormulaValue::Boolean(flag)),
		Ok(Value::Empty) => Ok(FormulaValue::Empty),
		Err(Stop::Fault(kind)) => Ok(error(kind)),
		Err(Stop::Refuse(refusal)) => Err(refusal),
	}
}

fn error(kind: FormulaErrorKind) -> FormulaValue {
	FormulaValue::Error(FormulaError::new(kind))
}
