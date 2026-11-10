//! R-BUD-3: Actual's arithmetic, proven with its own unit tests and with
//! vectors produced by running Actual's code.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use veritas_engine::{NumberFormat, currency_to_amount, eval_arithmetic};

use NumberFormat::{ApostropheDot, CommaDot, SpaceComma};

fn eval(expression: &str, format: NumberFormat) -> Option<f64> {
	eval_arithmetic(expression, format)
}

// Actual's `lc/shared/arithmetic.test.ts`, verbatim in order. Its number
// format is module state, so each test passes the format that Actual's test
// runs under at that point in the file.
mod actual_arithmetic_test {
	use super::*;

	#[test]
	fn handles_negative_numbers() {
		assert_eq!(eval("-4", CommaDot), Some(-4.0));
		assert_eq!(eval("10 + -4", CommaDot), Some(6.0));
	}

	#[test]
	fn handles_simple_addition() {
		assert_eq!(eval("10 + 10", CommaDot), Some(20.0));
		assert_eq!(eval("1.5 + 1.5", CommaDot), Some(3.0));
		assert_eq!(eval("(12 + 3) + (10)", CommaDot), Some(25.0));
		assert_eq!(eval("10 + 20 + 30 + 40", CommaDot), Some(100.0));
	}

	#[test]
	fn handles_simple_subtraction() {
		assert_eq!(eval("10 - 10", CommaDot), Some(0.0));
		assert_eq!(eval("4.5 - 1.5", CommaDot), Some(3.0));
		assert_eq!(eval("(12 - 3) - (10)", CommaDot), Some(-1.0));
		assert_eq!(eval("10 - 20 - 30 - 40", CommaDot), Some(-80.0));
	}

	#[test]
	fn handles_multiplication() {
		assert_eq!(eval("10 * 10", CommaDot), Some(100.0));
		assert_eq!(eval("1.5 * 1.5", CommaDot), Some(2.25));
		assert_eq!(eval("10 * 20 * 30 * 40", CommaDot), Some(240000.0));
	}

	#[test]
	fn handles_division() {
		assert_eq!(eval("10 / 10", CommaDot), Some(1.0));
		assert_eq!(eval("1.5 / .5", CommaDot), Some(3.0));
		assert_eq!(eval("2400 / 2 / 5", CommaDot), Some(240.0));
	}

	#[test]
	fn handles_order_of_operations() {
		assert_eq!(eval("(5 + 3) * 10", CommaDot), Some(80.0));
		assert_eq!(eval("5 + 3 * 10", CommaDot), Some(35.0));
		assert_eq!(eval("20^3 - 5 * (10 / 2)", CommaDot), Some(7975.0));
	}

	#[test]
	fn handles_exponent_as_right_associative() {
		assert_eq!(eval("2^3^2", CommaDot), Some(512.0));
	}

	#[test]
	fn handles_same_precedence_operators_left_to_right() {
		assert_eq!(eval("24 / 3 * 2", CommaDot), Some(16.0));
		assert_eq!(eval("24 * 3 / 2", CommaDot), Some(36.0));
		assert_eq!(eval("10 - 2 + 1", CommaDot), Some(9.0));
		assert_eq!(eval("10 + 2 - 1", CommaDot), Some(11.0));
	}

	#[test]
	fn respects_current_number_format() {
		assert_eq!(eval("1,222.45", CommaDot), Some(1222.45));
		assert_eq!(eval("1\u{202F}222,45", SpaceComma), Some(1222.45));
		assert_eq!(eval("1\u{2019}222.45", ApostropheDot), Some(1222.45));
	}

	#[test]
	fn ignores_leftover_characters() {
		assert_eq!(eval("1+2)", ApostropheDot), Some(3.0));
		assert_eq!(eval("1+2)foo", ApostropheDot), Some(3.0));
		assert_eq!(eval("(1+2)x", ApostropheDot), Some(3.0));
		assert_eq!(eval("10+20 trailing", ApostropheDot), Some(30.0));
		assert_eq!(eval("1+2(3", ApostropheDot), Some(3.0));
	}

	#[test]
	fn handles_apostrophe_dot_format_with_keyboard_apostrophe() {
		let keyboard_apostrophe = "12\u{0027}345.67";
		assert_eq!(keyboard_apostrophe.chars().nth(2), Some('\u{0027}'));
		assert_eq!(eval(keyboard_apostrophe, ApostropheDot), Some(12345.67));
		assert_eq!(eval("1\u{0027}234.56", ApostropheDot), Some(1234.56));
		assert_eq!(eval("1\u{0027}000.33", ApostropheDot), Some(1000.33));
		assert_eq!(eval("100\u{0027}000.99", ApostropheDot), Some(100000.99));
		assert_eq!(
			eval("1\u{0027}000\u{0027}000.50", ApostropheDot),
			Some(1000000.5)
		);
	}

	#[test]
	fn handles_apostrophe_dot_format_with_typographic_apostrophe() {
		let intl_apostrophe = "12\u{2019}345.67";
		assert_eq!(intl_apostrophe.chars().nth(2), Some('\u{2019}'));
		assert_eq!(eval(intl_apostrophe, ApostropheDot), Some(12345.67));
		assert_eq!(eval("1\u{2019}234.56", ApostropheDot), Some(1234.56));
		assert_eq!(eval("1\u{2019}000.33", ApostropheDot), Some(1000.33));
	}

	#[test]
	fn handles_apostrophe_dot_format_in_arithmetic_expressions() {
		assert_eq!(
			eval("1\u{0027}000 + 2\u{0027}000", ApostropheDot),
			Some(3000.0)
		);
		assert_eq!(
			eval("10\u{0027}000 - 2\u{0027}500", ApostropheDot),
			Some(7500.0)
		);
		assert_eq!(eval("1\u{0027}000 * 2", ApostropheDot), Some(2000.0));
		assert_eq!(eval("4\u{0027}000 / 2", ApostropheDot), Some(2000.0));
		assert_eq!(
			eval("1\u{2019}000 + 2\u{2019}000", ApostropheDot),
			Some(3000.0)
		);
		assert_eq!(
			eval("10\u{2019}000 - 2\u{2019}500", ApostropheDot),
			Some(7500.0)
		);
	}
}

mod vectors_from_actuals_code {
	use super::*;
	use serde_json::Value;

	fn vectors() -> Value {
		let text = include_str!("fixtures/actual-arithmetic-vectors.json");
		serde_json::from_str(text).expect("the vector file is JSON")
	}

	fn format(value: &Value) -> NumberFormat {
		NumberFormat::from_preference(value.as_str().expect("format is a string"))
	}

	/// Null, or the number as JavaScript prints it, parsed by Rust's exact
	/// float parser rather than serde_json's.
	fn expected(value: &Value) -> Option<f64> {
		let text = value.as_str()?;
		Some(match text {
			"Infinity" => f64::INFINITY,
			"-Infinity" => f64::NEG_INFINITY,
			_ => text.parse().expect("a JavaScript number"),
		})
	}

	fn same(a: Option<f64>, b: Option<f64>) -> bool {
		match (a, b) {
			(Some(a), Some(b)) => a.to_bits() == b.to_bits(),
			(None, None) => true,
			_ => false,
		}
	}

	#[test]
	fn eval_arithmetic_matches_actual_bit_for_bit() {
		let vectors = vectors();
		let cases = vectors["evalArithmetic"].as_array().expect("cases");
		assert!(cases.len() > 500);
		let failures: Vec<String> = cases
			.iter()
			.filter_map(|case| {
				let expression = case["expression"].as_str().expect("expression");
				let actual = eval(expression, format(&case["format"]));
				let want = expected(&case["result"]);
				(!same(actual, want)).then(|| {
					format!(
						"{} {expression:?}: got {actual:?}, Actual gives {want:?}",
						case["format"]
					)
				})
			})
			.collect();
		assert!(failures.is_empty(), "{}", failures.join("\n"));
	}

	#[test]
	fn currency_to_amount_matches_actual_bit_for_bit() {
		let vectors = vectors();
		let failures: Vec<String> = vectors["currencyToAmount"]
			.as_array()
			.expect("cases")
			.iter()
			.filter_map(|case| {
				let input = case["input"].as_str().expect("input");
				let actual = currency_to_amount(input, format(&case["format"]));
				let want = expected(&case["result"]);
				(!same(actual, want)).then(|| {
					format!(
						"{} {input:?}: got {actual:?}, Actual gives {want:?}",
						case["format"]
					)
				})
			})
			.collect();
		assert!(failures.is_empty(), "{}", failures.join("\n"));
	}
}
