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

/// Results pinned from Actual's card, so the engine is checked where the
/// oracle isn't installed. `tests/formula.rs` compares many more live.
#[cfg(test)]
mod tests {
	use super::*;

	fn run(formula: &str) -> Result<FormulaValue, CoreError> {
		let today = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap_or_default();
		evaluate(formula, &mut Names::default(), today)
	}

	fn number(formula: &str) -> f64 {
		match run(formula) {
			Ok(FormulaValue::Number(number)) => number,
			other => panic!("{formula}: {other:?}"),
		}
	}

	fn fault(formula: &str) -> FormulaErrorKind {
		match run(formula) {
			Ok(FormulaValue::Error(error)) => error.kind,
			other => panic!("{formula}: {other:?}"),
		}
	}

	#[test]
	fn numbers_export_as_actual_rounds_them() {
		assert_eq!(number("=1/3"), 0.33333333333);
		assert_eq!(number("=0.1+0.2-0.3"), 0.0);
		assert_eq!(number("=1e15+0.3"), 999_999_999_999_999.9);
		assert_eq!(number("=123456789012*1000"), 123_456_789_010_000.0);
		assert!(number("=1e-300").is_nan());
		assert_eq!(number("=ROUND(2.675,2)"), 2.68);
		assert_eq!(number("=ROUND(1.005,2)"), 1.0);
		assert_eq!(number("=-2^2"), 4.0);
		assert_eq!(number("=2^3^2"), 64.0);
		assert_eq!(number("=7^26"), 9.387480337600001e21);
		assert_eq!(number("=1e300"), 9.999999999999999e299);
	}

	#[test]
	fn text_reads_as_numbers_dates_and_times() {
		assert_eq!(number("=\"$-5\"+1"), -4.0);
		assert_eq!(number("=\"5 %\"+0"), 0.05);
		assert_eq!(number("=\"15/01/2026\"+0"), 46037.0);
		assert_eq!(number("=\"15/01/\"+0"), 36540.0);
		assert_eq!(number("=\"2026-01-15 10:30\"+0"), 46037.4375);
		assert_eq!(fault("=\"-$5\"+1"), FormulaErrorKind::Value);
		assert_eq!(fault("=\"15 01  2026\"+0"), FormulaErrorKind::Value);
	}

	#[test]
	fn dates_follow_the_1900_system() {
		assert_eq!(number("=TODAY()"), 46335.0);
		assert_eq!(number("=DATE(1900,2,29)"), 61.0);
		assert_eq!(number("=DATE(1899,12,31)"), 693_962.0);
		assert_eq!(number("=EOMONTH(DATE(2024,1,31),1)"), 45351.0);
		assert_eq!(number("=YEAR(1e15)"), 2_737_907_008_900.0);
		assert_eq!(fault("=DATE(-1899,1,1)"), FormulaErrorKind::Value);
		assert_eq!(fault("=DATE(10000,1,1)"), FormulaErrorKind::Num);
		assert_eq!(fault("=EOMONTH(46037,1.5)"), FormulaErrorKind::Num);
	}

	#[test]
	fn errors_parse_failures_and_refusals() {
		assert_eq!(fault("=1E3"), FormulaErrorKind::Error);
		assert_eq!(fault("=SUM (1)"), FormulaErrorKind::Error);
		assert_eq!(fault("=#N/A/1"), FormulaErrorKind::Error);
		assert_eq!(fault("=\"x\"+1/0"), FormulaErrorKind::DivByZero);
		assert_eq!(fault("=SUM(\"x\",1/0)"), FormulaErrorKind::Value);
		assert_eq!(fault("=SQRT(4)"), FormulaErrorKind::Name);
		assert_eq!(fault("=IF(1)"), FormulaErrorKind::Na);
		assert!(matches!(run("=IF(1,)"), Ok(FormulaValue::Empty)));
		assert!(matches!(
			run("=\"Ab\"=\"aB\""),
			Ok(FormulaValue::Boolean(true))
		));
		assert!(run("=A1").is_err());
		assert!(run("={1,2}").is_err());
		assert!(run("=\"é\"=\"e\"").is_err());
	}

	#[test]
	fn queries_record_what_to_fetch_from_every_branch() {
		let mut names = Names::default();
		let today = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap_or_default();
		let formula = "=IF(FALSE(), QUERY(\"a\"), BUDGET_QUERY(\"Spent\", QUERY_EXTRACT_CATEGORIES(\"b\"), QUERY_EXTRACT_TIMEFRAME_START(\"c\"), \"2026-02\"))";
		assert!(matches!(
			evaluate(formula, &mut names, today),
			Ok(FormulaValue::Number(0.0))
		));
		assert_eq!(names.query_names, ["a"]);
		assert_eq!(names.category_names, ["b"]);
		assert_eq!(names.start_names, ["c"]);
		assert_eq!(names.budget_requests[0].dimension, "spent");
	}
}
