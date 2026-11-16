//! Editing an amount field the way Actual's `InputValue` works: focus shows
//! the value as edit text, each keystroke goes through the single-operator
//! rule (a lone operator typed over the selection appends to the value), and
//! commit evaluates the text as it stands. Anything that doesn't evaluate to
//! a storable amount keeps the old value (D37) with a reason the UI can show.

use veritas_domain::{FormatPrefs, Money};

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

/// Actual's `forEdit` at two places, as the register shows amounts:
/// grouping separators, no currency symbol, two decimals unless the fraction
/// is hidden, and `-0` shown as `0`.
pub fn edit_text(amount: Money, format: NumberFormat, hide_fraction: bool) -> String {
	edit_text_at(amount, format, hide_fraction, 2)
}

/// The register's edit text (`serializeTransaction` with
/// `integerToCurrencyWithDecimal`): when the budget hides fractions, only a
/// whole amount drops them. Cents that aren't zero always show, so opening
/// and committing a field never rounds the stored amount.
pub fn register_edit_text(amount: Money, format: NumberFormat, hide_fraction: bool) -> String {
	edit_text(amount, format, hide_fraction && amount.minor() % 100 == 0)
}

/// `forEdit` at the currency's `places`, as budget cells show amounts
/// (`useFormat`, CLS-12, D63).
pub fn edit_text_at(
	amount: Money,
	format: NumberFormat,
	hide_fraction: bool,
	places: u8,
) -> String {
	// No currency has more than 3 places; 10^18 still fits a u64.
	let places = places.min(18);
	let scale = 10u64.pow(u32::from(places));
	let minor = amount.minor();
	let magnitude = minor.unsigned_abs();
	let (whole, fraction) = if hide_fraction || places == 0 {
		// Intl rounds half away from zero.
		((magnitude + scale / 2) / scale, None)
	} else {
		(magnitude / scale, Some(magnitude % scale))
	};
	let mut text = String::new();
	if minor < 0 && (whole > 0 || fraction.is_some_and(|f| f > 0)) {
		text.push('-');
	}
	text.push_str(&group_digits(whole, format));
	if let Some(fraction) = fraction {
		text.push(format.decimal_separator());
		text.push_str(&format!("{fraction:0width$}", width = usize::from(places)));
	}
	text
}

/// A register-scale amount as copy shows it (CLS-12.3): the budget's
/// currency places (rounded half away from zero when fewer than 2, or when
/// the budget hides fractions), its number style, a true minus, and the
/// symbol placed and spaced by the budget's preferences, unset reading as
/// Actual's default (before, no space).
pub fn register_currency_text(amount: Money, prefs: &FormatPrefs) -> String {
	let format = NumberFormat::from_preference(prefs.number_format.as_deref().unwrap_or(""));
	let whole = prefs.hide_fraction || prefs.decimal_places() == 0;
	let magnitude = Money::from_minor(amount.minor().saturating_abs());
	let digits = edit_text_at(magnitude, format, whole, 2);
	let shown_zero = digits.chars().all(|c| !c.is_ascii_digit() || c == '0');
	let sign = if amount.is_negative() && !shown_zero {
		"\u{2212}"
	} else {
		""
	};
	let symbol = prefs
		.currency_code
		.as_deref()
		.and_then(veritas_domain::currency)
		.map_or("", |currency| currency.symbol);
	if symbol.is_empty() {
		return format!("{sign}{digits}");
	}
	let space = if prefs.symbol_space.as_deref() == Some("true") {
		"\u{202F}"
	} else {
		""
	};
	if prefs.symbol_position.as_deref() == Some("after") {
		format!("{sign}{digits}{space}{symbol}")
	} else {
		format!("{sign}{symbol}{space}{digits}")
	}
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

/// Commits the field's text as it stands, as Actual's `InputValue` does on
/// Return, Tab or blur. The single-operator rule has already shaped the text
/// keystroke by keystroke, so it isn't applied again here.
pub fn commit_amount(current: Money, text: &str, format: NumberFormat) -> AmountEdit {
	let kept = |rejection| AmountEdit {
		amount: current,
		rejection: Some(rejection),
	};
	if text.trim().is_empty() {
		return kept(Rejection::Empty);
	}
	match eval_arithmetic(text, format) {
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
	fn copy_shows_a_register_amount_in_the_budgets_currency() {
		let prefs =
			|code: Option<&str>, style: &str, hide: bool, after: bool, space: bool| FormatPrefs {
				number_format: Some(style.to_owned()),
				hide_fraction: hide,
				currency_code: code.map(str::to_owned),
				symbol_position: Some(if after { "after" } else { "before" }.to_owned()),
				symbol_space: Some(space.to_string()),
			};
		let text =
			|minor, prefs: &FormatPrefs| register_currency_text(Money::from_minor(minor), prefs);
		let usd = prefs(Some("USD"), "comma-dot", false, false, false);
		assert_eq!(text(2_000, &usd), "$20.00", "DEBT-2.2's example");
		assert_eq!(text(-123_456, &usd), "\u{2212}$1,234.56");
		let yen = prefs(Some("JPY"), "comma-dot", true, false, false);
		assert_eq!(text(2_000, &yen), "¥20", "CLS-12.3: R / 100 at 0 places");
		assert_eq!(text(50_050, &yen), "¥501", "half away from zero");
		let eur = prefs(Some("EUR"), "dot-comma", false, true, true);
		assert_eq!(text(123_456, &eur), "1.234,56\u{202F}€");
		assert_eq!(
			text(2_000, &FormatPrefs::default()),
			"20.00",
			"no currency, no symbol"
		);
		assert_eq!(text(-40, &yen), "¥0", "no minus on a figure shown as zero");
	}

	#[test]
	fn edit_text_follows_the_currencys_places() {
		let text = |minor, hide, places| {
			edit_text_at(
				Money::from_minor(minor),
				NumberFormat::CommaDot,
				hide,
				places,
			)
		};
		assert_eq!(text(5_000, false, 0), "5,000");
		assert_eq!(text(-5_000, false, 0), "-5,000");
		assert_eq!(text(1_234_567, false, 3), "1,234.567");
		assert_eq!(text(-5, false, 3), "-0.005");
		assert_eq!(text(1_234_500, true, 3), "1,235");
		assert_eq!(
			text(123_456, false, 2),
			edit_text(Money::from_minor(123_456), NumberFormat::CommaDot, false)
		);
	}

	#[test]
	fn the_register_hides_only_a_zero_fraction() {
		let text = |minor, hide| {
			register_edit_text(Money::from_minor(minor), NumberFormat::CommaDot, hide)
		};
		assert_eq!(text(10_000, true), "100");
		assert_eq!(text(-123_400, true), "-1,234");
		assert_eq!(text(0, true), "0");
		assert_eq!(
			text(10_040, true),
			"100.40",
			"cents stay, as Actual's register keeps them"
		);
		assert_eq!(text(-5, true), "-0.05");
		assert_eq!(text(10_000, false), "100.00");
	}

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
