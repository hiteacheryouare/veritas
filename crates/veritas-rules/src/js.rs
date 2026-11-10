//! The few JavaScript semantics Actual's rule code leans on, so the ports read
//! like the original: truthiness, `Math.round`, `new Date(y, m, d)` overflow
//! and the `\s` class.

use chrono::{Days, NaiveDate};
use serde_json::Value;

pub(crate) fn truthy(value: Option<&Value>) -> bool {
	match value {
		None | Some(Value::Null) => false,
		Some(Value::Bool(flag)) => *flag,
		Some(Value::Number(number)) => number.as_f64().is_some_and(|n| n != 0.0),
		Some(Value::String(text)) => !text.is_empty(),
		Some(Value::Array(_) | Value::Object(_)) => true,
	}
}

/// `Math.round`: halves go toward +∞, so −2.5 becomes −2 (Rust's `round` gives −3).
pub(crate) fn math_round(x: f64) -> f64 {
	let floor = x.floor();
	if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

/// A JSON number that `Number.isInteger` accepts.
pub(crate) fn integer(value: &Value) -> Option<i64> {
	let number = value.as_f64()?;
	(number.fract() == 0.0 && number.abs() < 9.007_199_254_740_992e15).then_some(number as i64)
}

/// `parseInt(text)` for the decimal case: leading whitespace, an optional sign,
/// then digits up to the first non-digit. `None` is `NaN`.
fn parse_int(text: &str) -> Option<i64> {
	let text = text.trim_start_matches(is_js_whitespace);
	let (negative, digits) = match text.as_bytes().first() {
		Some(b'-') => (true, &text[1..]),
		Some(b'+') => (false, &text[1..]),
		_ => (false, text),
	};
	if digits.starts_with("0x") || digits.starts_with("0X") {
		return None;
	}
	let end = digits
		.find(|c: char| !c.is_ascii_digit())
		.unwrap_or(digits.len());
	let magnitude: i64 = digits[..end].parse().ok()?;
	Some(if negative { -magnitude } else { magnitude })
}

/// Actual's `parseDate` (`shared/months.ts` `_parse`): split on `-` and hand
/// the parts to `new Date(year, month - 1, day, 12)`, which rolls overflow into
/// the next month, so `2026-02-30` is March 2. `None` is an Invalid Date.
pub(crate) fn parse_date(text: &str) -> Option<NaiveDate> {
	let mut parts = text.split('-');
	let year = parse_int(parts.next()?)?;
	let month = match parts.next() {
		Some(part) => parse_int(part)?,
		None => 1,
	};
	let day = match parts.next() {
		Some(part) => parse_int(part)?,
		None => 1,
	};
	let year = if (0..=99).contains(&year) {
		year + 1900
	} else {
		year
	};
	let month_index = year.checked_mul(12)?.checked_add(month - 1)?;
	let first = NaiveDate::from_ymd_opt(
		i32::try_from(month_index.div_euclid(12)).ok()?,
		u32::try_from(month_index.rem_euclid(12) + 1).ok()?,
		1,
	)?;
	let offset = day - 1;
	if offset >= 0 {
		first.checked_add_days(Days::new(offset.unsigned_abs()))
	} else {
		first.checked_sub_days(Days::new(offset.unsigned_abs()))
	}
}

/// A strict `YYYY-MM-DD` that names a real day, the only shape Veritas treats as a date.
pub(crate) fn strict_date(text: &str) -> Option<NaiveDate> {
	let bytes = text.as_bytes();
	let shaped = bytes.len() == 10
		&& bytes[4] == b'-'
		&& bytes[7] == b'-'
		&& bytes
			.iter()
			.enumerate()
			.all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit());
	if !shaped {
		return None;
	}
	NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()
}

/// The characters JavaScript's `\s` matches.
pub(crate) fn is_js_whitespace(c: char) -> bool {
	matches!(
		c,
		'\t' | '\n' | '\u{0b}' | '\u{0c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
			..='\u{200a}'
				| '\u{2028}' | '\u{2029}'
				| '\u{202f}' | '\u{205f}'
				| '\u{3000}' | '\u{feff}'
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn date(text: &str) -> NaiveDate {
		NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
	}

	#[test]
	fn math_round_sends_halves_up_like_javascript() {
		assert_eq!(math_round(2.5), 3.0);
		assert_eq!(math_round(-2.5), -2.0);
		assert_eq!(math_round(7.5), 8.0);
		assert_eq!(math_round(-0.4), 0.0);
		assert_eq!(math_round(115.125), 115.0);
	}

	#[test]
	fn parse_date_rolls_overflow_like_new_date() {
		assert_eq!(parse_date("2026-02-29"), Some(date("2026-03-01")));
		assert_eq!(parse_date("2026-02-30"), Some(date("2026-03-02")));
		assert_eq!(parse_date("2026-13-01"), Some(date("2027-01-01")));
		assert_eq!(parse_date("2026-03-00"), Some(date("2026-02-28")));
		assert_eq!(parse_date("2026-03"), Some(date("2026-03-01")));
		assert_eq!(parse_date("2026"), Some(date("2026-01-01")));
		assert_eq!(parse_date("2026-1-5"), Some(date("2026-01-05")));
		assert_eq!(parse_date(""), None);
		assert_eq!(parse_date("someday"), None);
	}

	#[test]
	fn strict_date_rejects_impossible_days_and_other_shapes() {
		assert_eq!(strict_date("2028-02-29"), Some(date("2028-02-29")));
		assert_eq!(strict_date("2026-02-29"), None);
		assert_eq!(strict_date("2026-1-05"), None);
		assert_eq!(strict_date("2026-W01-1"), None);
	}
}
