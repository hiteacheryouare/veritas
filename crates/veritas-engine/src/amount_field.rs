//! Editing an amount field. The field shows the current value as Actual's
//! edit text, typing a lone operator appends to it (Actual's `InputValue`
//! rule), and anything that doesn't evaluate to a storable amount keeps the
//! old value (D37) with a reason the UI can show as a hint.

use veritas_domain::Money;

use crate::arithmetic::{NumberFormat, eval_arithmetic};

/// The largest magnitude Actual's `safeNumber` accepts, 2^51 − 1.
pub const MAX_SAFE_MINOR: i64 = (1 << 51) - 1;

const OPERATORS: [char; 5] = ['+', '-', '*', '/', '^'];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
	Empty,
	NotAnAmount,
	/// Division by zero or an overflow to infinity.
	NotFinite,
	TooLarge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AmountEdit {
	pub amount: Money,
	pub rejection: Option<Rejection>,
}

/// Actual's `forEdit`: grouping separators, no currency symbol, two decimals
/// unless the fraction is hidden, and `-0` shown as `0`.
pub fn edit_text(amount: Money, format: NumberFormat, hide_fraction: bool) -> String {
	let minor = amount.minor();
	let magnitude = minor.unsigned_abs();
	let (whole, cents) = if hide_fraction {
		// Intl rounds half away from zero.
		((magnitude + 50) / 100, None)
	} else {
		(magnitude / 100, Some(magnitude % 100))
	};
	let mut text = String::new();
	if minor < 0 && (whole > 0 || cents.is_some_and(|c| c > 0)) {
		text.push('-');
	}
	text.push_str(&group_digits(whole, format));
	if let Some(cents) = cents {
		text.push(format.decimal_separator());
		text.push_str(&format!("{cents:02}"));
	}
	text
}

fn group_digits(whole: u64, format: NumberFormat) -> String {
	let digits = whole.to_string();
	let separator = format.thousands_separator();
	let len = digits.len();
	let mut grouped = String::with_capacity(len + len / 2);
	for (index, digit) in digits.chars().enumerate() {
		let remaining = len - index;
		let boundary = match format {
			// Indian grouping: the last three digits, then pairs.
			NumberFormat::CommaDotIn => remaining >= 3 && (remaining - 3).is_multiple_of(2),
			_ => remaining.is_multiple_of(3),
		};
		if index > 0 && boundary {
			grouped.push(separator);
		}
		grouped.push(digit);
	}
	grouped
}

/// Actual's single-operator rule, applied on every keystroke: when the field's
/// whole text becomes one operator (the user typed it over the selected
/// value), the text becomes the original value followed by that operator.
pub fn single_operator_rule(original_text: &str, new_text: &str) -> String {
	let mut chars = new_text.chars();
	match (chars.next(), chars.next()) {
		(Some(op), None) if OPERATORS.contains(&op) => format!("{original_text}{op}"),
		_ => new_text.to_owned(),
	}
}

/// The expression a field ends up evaluating when the user selects the value
/// and types `typed`: a leading operator appends to the edit text.
pub fn field_expression(edit_text: &str, typed: &str) -> String {
	match typed.chars().next() {
		Some(op) if OPERATORS.contains(&op) => format!("{edit_text}{typed}"),
		_ => typed.to_owned(),
	}
}

/// Commits typed text into a field that held `current`.
pub fn edit_amount(
	current: Money,
	typed: &str,
	format: NumberFormat,
	hide_fraction: bool,
) -> AmountEdit {
	let expression = field_expression(&edit_text(current, format, hide_fraction), typed);
	let kept = |rejection| AmountEdit {
		amount: current,
		rejection: Some(rejection),
	};
	if expression.trim().is_empty() {
		return kept(Rejection::Empty);
	}
	match eval_arithmetic(&expression, format) {
		None => kept(Rejection::NotAnAmount),
		Some(value) => match amount_to_minor(value) {
			Ok(amount) => AmountEdit {
				amount,
				rejection: None,
			},
			Err(rejection) => kept(rejection),
		},
	}
}

/// Actual's `amountToInteger` with two decimal places: `Math.round(v * 100)`.
pub fn amount_to_minor(value: f64) -> Result<Money, Rejection> {
	if !value.is_finite() {
		return Err(Rejection::NotFinite);
	}
	let minor = js_round(value * 100.0);
	if minor.abs() > MAX_SAFE_MINOR as f64 {
		return Err(Rejection::TooLarge);
	}
	// In range and integral, so the cast is exact; `-0.0` becomes 0.
	Ok(Money::from_minor(minor as i64))
}

/// JavaScript's `Math.round`: halves go toward +∞, so −2.5 rounds to −2.
/// Neither `f64::round` nor `(x + 0.5).floor()` matches it.
pub fn js_round(value: f64) -> f64 {
	let floor = value.floor();
	let rounded = if value - floor >= 0.5 {
		floor + 1.0
	} else {
		floor
	};
	// JavaScript keeps the sign of zero: Math.round(-0.4) is -0.
	if rounded == 0.0 {
		rounded.copysign(value)
	} else {
		rounded
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_single_operator_rule_fires_only_on_a_lone_operator() {
		assert_eq!(single_operator_rule("100.00", "+"), "100.00+");
		assert_eq!(single_operator_rule("100.00", "^"), "100.00^");
		assert_eq!(single_operator_rule("100.00", "+1"), "+1");
		assert_eq!(single_operator_rule("100.00", "5"), "5");
		assert_eq!(single_operator_rule("100.00", ""), "");
	}

	#[test]
	fn rounds_like_math_round() {
		assert_eq!(js_round(-2.5), -2.0);
		assert_eq!(js_round(2.5), 3.0);
		assert_eq!(js_round(0.499_999_999_999_999_94), 0.0);
		assert_eq!(js_round(4_503_599_627_370_497.0), 4_503_599_627_370_497.0);
	}

	#[test]
	fn indian_grouping_uses_pairs_above_the_thousands() {
		let text = |minor| edit_text(Money::from_minor(minor), NumberFormat::CommaDotIn, false);
		assert_eq!(text(12_345_678_900), "12,34,56,789.00");
		assert_eq!(text(99_900), "999.00");
		assert_eq!(text(100_000), "1,000.00");
	}
}
