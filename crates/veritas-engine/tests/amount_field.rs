//! R-BUD-3 and PLAN §9 edge case 12: every amount field shows its value as
//! Actual's edit text, appends on a leading operator, and keeps the old value
//! on input that isn't a storable amount (D37).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::Value;
use veritas_domain::Money;
use veritas_engine::{
	AmountEdit, NumberFormat, Rejection, edit_amount, edit_text, field_expression, js_round,
};

use NumberFormat::{ApostropheDot, CommaDot, CommaDotIn, DotComma, SpaceComma};

fn cents(minor: i64) -> Money {
	Money::from_minor(minor)
}

fn type_into(current: i64, typed: &str) -> AmountEdit {
	edit_amount(cents(current), typed, CommaDot, false)
}

fn accepted(minor: i64) -> AmountEdit {
	AmountEdit {
		amount: cents(minor),
		rejection: None,
	}
}

fn kept(current: i64, rejection: Rejection) -> AmountEdit {
	AmountEdit {
		amount: cents(current),
		rejection: Some(rejection),
	}
}

#[test]
fn typing_a_leading_operator_appends_to_the_current_value() {
	assert_eq!(field_expression("100.00", "+12.34"), "100.00+12.34");
	assert_eq!(type_into(10_000, "+12.34"), accepted(11_234));
	assert_eq!(type_into(10_000, "-12.34"), accepted(8_766));
	assert_eq!(type_into(10_000, "*2"), accepted(20_000));
	assert_eq!(type_into(10_000, "/4"), accepted(2_500));
	assert_eq!(type_into(10_000, "^2"), accepted(1_000_000));
	assert_eq!(type_into(0, "+12.34"), accepted(1_234));
	assert_eq!(type_into(10_000, "-0"), accepted(10_000));
	assert_eq!(type_into(-5_000, "+12.34"), accepted(-3_766));
}

/// A leading `-` is an operator too, so typing `-5` over 100.00 gives 95.00,
/// exactly as typing it into Actual's selected field does.
#[test]
fn a_leading_minus_subtracts_from_the_current_value() {
	assert_eq!(type_into(10_000, "-5"), accepted(9_500));
	assert_eq!(type_into(10_000, "-2^2"), accepted(9_600));
	assert_eq!(type_into(0, "-5"), accepted(-500));
}

#[test]
fn typing_a_number_replaces_the_current_value() {
	assert_eq!(type_into(10_000, "12.34"), accepted(1_234));
	assert_eq!(type_into(10_000, "(5 + 3) * 10"), accepted(8_000));
	assert_eq!(type_into(10_000, "0"), accepted(0));
}

#[test]
fn edge_case_12_grouped_values_and_expressions() {
	assert_eq!(type_into(10_000, "1,234.56+10"), accepted(124_456));
	assert_eq!(type_into(123_456, "+10"), accepted(124_456));
	assert_eq!(type_into(10_000, "2^3^2"), accepted(51_200));
	assert_eq!(type_into(10_000, "(2^3)^2"), accepted(6_400));
	assert_eq!(type_into(10_000, "10 - -4"), accepted(1_400));
}

#[test]
fn edge_case_12_negative_results() {
	assert_eq!(type_into(10_000, "-150"), accepted(-5_000));
	assert_eq!(type_into(10_000, "10-20"), accepted(-1_000));
	assert_eq!(type_into(10_000, "1-1.01"), accepted(-1));
}

#[test]
fn edge_case_12_pasted_currency_symbols() {
	assert_eq!(type_into(0, "$1,234.56"), accepted(123_456));
	assert_eq!(type_into(0, "\u{20AC}12,50"), accepted(1_250));
	assert_eq!(type_into(0, "\u{A3}5 + \u{A5}100"), accepted(10_500));
	assert_eq!(type_into(0, "-$5"), accepted(-500));
	assert_eq!(type_into(0, "12.34 USD"), accepted(1_234));
	assert_eq!(
		edit_amount(cents(0), "\u{20AC}1.234,56", DotComma, false),
		accepted(123_456)
	);
	assert_eq!(type_into(700, "$-5"), kept(700, Rejection::NotAnAmount));
	assert_eq!(type_into(700, "USD 5"), kept(700, Rejection::NotAnAmount));
}

#[test]
fn edge_case_12_divide_by_zero_and_infinity_keep_the_old_value() {
	assert_eq!(type_into(10_000, "1/0"), kept(10_000, Rejection::NotFinite));
	assert_eq!(type_into(10_000, "/0"), kept(10_000, Rejection::NotFinite));
	assert_eq!(
		type_into(10_000, "-1/0"),
		kept(10_000, Rejection::NotFinite)
	);
	assert_eq!(
		type_into(10_000, "2^1024"),
		kept(10_000, Rejection::NotFinite)
	);
	assert_eq!(
		type_into(10_000, "0/0"),
		kept(10_000, Rejection::NotAnAmount)
	);
	assert_eq!(
		type_into(10_000, "1^(1/0)"),
		kept(10_000, Rejection::NotAnAmount)
	);
}

/// D37: Veritas keeps the old value wherever Actual's transaction fields do,
/// in every field, instead of saving 0 the way Actual's budget cells do.
#[test]
fn d37_invalid_input_keeps_the_old_value() {
	assert_eq!(type_into(4_200, "(5"), kept(4_200, Rejection::NotAnAmount));
	assert_eq!(type_into(4_200, "abc"), kept(4_200, Rejection::NotAnAmount));
	assert_eq!(type_into(4_200, "+"), kept(4_200, Rejection::NotAnAmount));
	assert_eq!(type_into(4_200, "*"), kept(4_200, Rejection::NotAnAmount));
	assert_eq!(type_into(4_200, "."), kept(4_200, Rejection::NotAnAmount));
	assert_eq!(
		type_into(4_200, "\u{2212}5"),
		kept(4_200, Rejection::NotAnAmount)
	);
	assert_eq!(type_into(4_200, ""), kept(4_200, Rejection::Empty));
	assert_eq!(type_into(4_200, "  "), kept(4_200, Rejection::Empty));
}

#[test]
fn trailing_junk_is_ignored_like_actual() {
	assert_eq!(type_into(0, "12.34abc"), accepted(1_234));
	assert_eq!(type_into(0, "5%"), accepted(500));
	assert_eq!(type_into(0, "1e5"), accepted(100));
	assert_eq!(type_into(0, "1+2)"), accepted(300));
}

#[test]
fn amounts_beyond_actuals_safe_range_keep_the_old_value() {
	assert_eq!(
		type_into(1, "22517998136852.47"),
		accepted(2_251_799_813_685_247)
	);
	assert_eq!(
		type_into(0, "-22517998136852.47"),
		accepted(-2_251_799_813_685_247)
	);
	assert_eq!(
		type_into(1, "22517998136852.48"),
		kept(1, Rejection::TooLarge)
	);
	assert_eq!(
		type_into(1, "99999999999999999999"),
		kept(1, Rejection::TooLarge)
	);
}

#[test]
fn rounding_to_cents_follows_math_round() {
	assert_eq!(type_into(0, "1.005"), accepted(100));
	assert_eq!(type_into(0, "0.125"), accepted(13));
	assert_eq!(type_into(0, "-0.125"), accepted(-12));
	assert_eq!(type_into(0, "100/3"), accepted(3_333));
	assert_eq!(type_into(0, "0.1+0.2"), accepted(30));
}

#[test]
fn appending_works_in_every_number_format() {
	let append = |current, format| edit_amount(cents(current), "+10", format, false);
	assert_eq!(edit_text(cents(123_456), DotComma, false), "1.234,56");
	assert_eq!(append(123_456, DotComma), accepted(124_456));
	assert_eq!(
		edit_text(cents(123_456), SpaceComma, false),
		"1\u{202F}234,56"
	);
	assert_eq!(append(123_456, SpaceComma), accepted(124_456));
	assert_eq!(
		edit_text(cents(123_456), ApostropheDot, false),
		"1\u{2019}234.56"
	);
	assert_eq!(append(123_456, ApostropheDot), accepted(124_456));
	assert_eq!(
		edit_text(cents(123_456_789), CommaDotIn, false),
		"12,34,567.89"
	);
	assert_eq!(append(123_456_789, CommaDotIn), accepted(123_457_789));
	assert_eq!(
		edit_amount(cents(0), "12,50+1", DotComma, false),
		accepted(1_350)
	);
}

#[test]
fn hidden_fractions_drop_the_cents_when_appending_like_actual() {
	assert_eq!(edit_text(cents(10_040), CommaDot, true), "100");
	assert_eq!(
		edit_amount(cents(10_040), "+5", CommaDot, true),
		accepted(10_500)
	);
}

mod vectors_from_actuals_code {
	use super::*;

	fn vectors() -> Value {
		serde_json::from_str(include_str!("fixtures/actual-arithmetic-vectors.json"))
			.expect("the vector file is JSON")
	}

	fn number(value: &Value) -> f64 {
		value
			.as_str()
			.and_then(|s| s.parse().ok())
			.expect("a JavaScript number")
	}

	#[test]
	fn edit_text_matches_actuals_for_edit() {
		let vectors = vectors();
		let cases = vectors["forEdit"].as_array().expect("cases");
		assert!(cases.len() > 200);
		let failures: Vec<String> = cases
			.iter()
			.filter_map(|case| {
				let format =
					NumberFormat::from_preference(case["format"].as_str().expect("format"));
				let hide_fraction = case["hideFraction"].as_bool().expect("hideFraction");
				let minor = case["cents"].as_i64().expect("cents");
				let want = case["text"].as_str().expect("text");
				let got = edit_text(cents(minor), format, hide_fraction);
				(got != want).then(|| {
					format!(
						"{format:?} {hide_fraction} {minor}: got {got:?}, Actual shows {want:?}"
					)
				})
			})
			.collect();
		assert!(failures.is_empty(), "{}", failures.join("\n"));
	}

	#[test]
	fn js_round_matches_math_round_and_amount_to_integer() {
		for case in vectors()["amountToInteger"].as_array().expect("cases") {
			let input = number(&case["input"]);
			assert_eq!(
				js_round(input).to_bits(),
				number(&case["result"]).to_bits(),
				"Math.round({input})"
			);
			assert_eq!(
				js_round(input * 100.0).to_bits(),
				number(&case["cents"]).to_bits(),
				"amountToInteger({input})"
			);
		}
	}
}
