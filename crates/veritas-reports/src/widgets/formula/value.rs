//! Written clean-room from HyperFormula's public docs and black-box tests
//! against Actual's formula card. No HyperFormula source was read.
//!
//! A formula's values and how they turn into one another: the coercions,
//! comparisons and rounding HyperFormula's docs describe (`precisionEpsilon`,
//! `precisionRounding`, `caseSensitive`, `accentSensitive`) as Actual's card
//! shows them.

use std::cmp::Ordering;

use veritas_domain::CoreError;

use super::FormulaErrorKind as Kind;
use super::datetime;
use crate::filter::is_js_space;
use crate::report_query::unsupported;

/// HyperFormula's default `precisionEpsilon`.
const EPSILON: f64 = 1e-13;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Value {
	Number(f64),
	Text(String),
	Bool(bool),
	Empty,
}

/// Why an evaluation stopped: an error value, which the formula shows, or a
/// formula Veritas refuses to evaluate.
#[derive(Debug)]
pub(super) enum Stop {
	Fault(Kind),
	Refuse(CoreError),
}

impl From<Kind> for Stop {
	fn from(kind: Kind) -> Self {
		Stop::Fault(kind)
	}
}

impl From<CoreError> for Stop {
	fn from(error: CoreError) -> Self {
		Stop::Refuse(error)
	}
}

pub(super) type Outcome<T> = Result<T, Stop>;

/// A finite number, or `#NUM!` for the infinities and NaN that arithmetic
/// can produce.
pub(super) fn finite(number: f64) -> Outcome<f64> {
	if number.is_finite() {
		Ok(number)
	} else {
		Err(Kind::Num.into())
	}
}

/// `a + b`, snapped to 0 when it is within `precisionEpsilon` of cancelling
/// out (the docs' rule for `+` and `-`).
pub(super) fn add(a: f64, b: f64) -> f64 {
	snap(a, a + b)
}

pub(super) fn subtract(a: f64, b: f64) -> f64 {
	snap(a, a - b)
}

fn snap(a: f64, sum: f64) -> f64 {
	if sum.is_finite() && sum.abs() <= EPSILON * a.abs() {
		0.0
	} else {
		sum
	}
}

/// The docs' equality: same sign, and each no more than `1 + epsilon` times
/// the other.
pub(super) fn approximately_equal(a: f64, b: f64) -> bool {
	if a == b {
		return true;
	}
	(a < 0.0) == (b < 0.0)
		&& a.abs() <= (1.0 + EPSILON) * b.abs()
		&& b.abs() <= (1.0 + EPSILON) * a.abs()
}

/// JavaScript's `Math.round`: halves go toward +∞, and nothing is lost to
/// adding 0.5 first.
pub(super) fn js_round(number: f64) -> f64 {
	let floor = number.floor();
	if number - floor >= 0.5 {
		floor + 1.0
	} else {
		floor
	}
}

/// The number a cell exports: HyperFormula's `precisionRounding` of 10,
/// which keeps 11 significant digits and is computed in floating point, so
/// `1e15 + 0.3` exports as 999999999999999.9 and a tiny number as NaN.
pub(super) fn export_number(number: f64) -> f64 {
	if number == 0.0 {
		return 0.0;
	}
	let places = 10.0 - number.abs().log10().floor();
	let scale = 10f64.powf(places);
	let rounded = (js_round(number.abs() * scale) / scale).copysign(number);
	if rounded.is_nan() { f64::NAN } else { rounded }
}

/// JavaScript's `Number.prototype.toString`.
pub(super) fn js_number_text(number: f64) -> String {
	if number.is_nan() {
		return "NaN".to_owned();
	}
	if number == 0.0 {
		return "0".to_owned();
	}
	if number.is_infinite() {
		return if number > 0.0 {
			"Infinity"
		} else {
			"-Infinity"
		}
		.to_owned();
	}
	let scientific = format!("{:e}", number.abs());
	let (mantissa, exponent) = scientific
		.split_once('e')
		.unwrap_or((scientific.as_str(), "0"));
	let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
	let count = digits.len() as i32;
	let point = exponent.parse::<i32>().unwrap_or(0) + 1;
	let body = if count <= point && point <= 21 {
		format!("{digits}{}", "0".repeat((point - count) as usize))
	} else if 0 < point && point <= 21 {
		let (whole, fraction) = digits.split_at(point as usize);
		format!("{whole}.{fraction}")
	} else if -6 < point && point <= 0 {
		format!("0.{}{digits}", "0".repeat((-point) as usize))
	} else {
		let (first, rest) = digits.split_at(1);
		let fraction = if rest.is_empty() {
			String::new()
		} else {
			format!(".{rest}")
		};
		let sign = if point > 0 { '+' } else { '-' };
		format!("{first}{fraction}e{sign}{}", (point - 1).abs())
	};
	if number < 0.0 {
		format!("-{body}")
	} else {
		body
	}
}

pub(super) fn to_number(value: &Value) -> Outcome<f64> {
	match value {
		Value::Number(number) => Ok(*number),
		Value::Bool(flag) => Ok(f64::from(u8::from(*flag))),
		Value::Empty => Ok(0.0),
		Value::Text(text) => text_to_number(text),
	}
}

pub(super) fn to_text(value: &Value) -> String {
	match value {
		Value::Number(number) => js_number_text(*number),
		Value::Text(text) => text.clone(),
		Value::Bool(true) => "TRUE".to_owned(),
		Value::Bool(false) => "FALSE".to_owned(),
		Value::Empty => String::new(),
	}
}

pub(super) fn to_bool(value: &Value) -> Outcome<bool> {
	match value {
		Value::Number(number) => Ok(*number != 0.0),
		Value::Bool(flag) => Ok(*flag),
		Value::Empty => Ok(false),
		Value::Text(text) if text.is_empty() => Ok(false),
		Value::Text(text) if text.eq_ignore_ascii_case("true") => Ok(true),
		Value::Text(text) if text.eq_ignore_ascii_case("false") => Ok(false),
		Value::Text(_) => Err(Kind::Value.into()),
	}
}

/// Text read as a number: plain, percent or `$` currency, then a date or
/// time. Only the empty string is 0; blank text is `#VALUE!`.
fn text_to_number(text: &str) -> Outcome<f64> {
	if text.is_empty() {
		return Ok(0.0);
	}
	let trimmed = text.trim_matches(is_js_space);
	let number = if let Some(rest) = trimmed.strip_suffix('%') {
		plain_number(rest.trim_matches(is_js_space)).map(|n| n / 100.0)
	} else if let Some(rest) = trimmed.strip_prefix('$') {
		plain_number(rest.trim_matches(is_js_space))
	} else if let Some(rest) = trimmed.strip_suffix('$') {
		plain_number(rest.trim_matches(is_js_space))
	} else {
		plain_number(trimmed)
	};
	match number.or_else(|| datetime::parse_date_time(text)) {
		Some(number) => finite(number),
		None => Err(Kind::Value.into()),
	}
}

/// A decimal number with an optional sign and exponent, the shape
/// HyperFormula reads from text. Too large is +∞, which callers reject.
fn plain_number(text: &str) -> Option<f64> {
	let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
	let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
		Some(at) => (&unsigned[..at], Some(&unsigned[at + 1..])),
		None => (unsigned, None),
	};
	let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
	let digits = |part: &str| part.bytes().all(|b| b.is_ascii_digit());
	let mantissa_ok =
		digits(whole) && digits(fraction) && !(whole.is_empty() && fraction.is_empty());
	let exponent_ok = exponent.is_none_or(|e| {
		let e = e.strip_prefix(['+', '-']).unwrap_or(e);
		!e.is_empty() && digits(e)
	});
	if !(mantissa_ok && exponent_ok) {
		return None;
	}
	let normalized = format!(
		"{}{}.{}{}",
		if text.starts_with('-') { "-" } else { "" },
		if whole.is_empty() { "0" } else { whole },
		if fraction.is_empty() { "0" } else { fraction },
		exponent.map(|e| format!("e{e}")).unwrap_or_default()
	);
	normalized.parse().ok()
}

/// The order of `a` and `b` for the comparison operators: numbers by value
/// within `precisionEpsilon`, then text, then booleans, with an empty value
/// standing for the other side's zero. Text that reads as a date or time
/// compares as that number against a number or against other such text.
pub(super) fn compare(a: &Value, b: &Value) -> Outcome<Ordering> {
	let date = |value: &Value| match value {
		Value::Text(text) => datetime::parse_date_time(text),
		_ => None,
	};
	let (a, b) = match (a, b, date(a), date(b)) {
		(Value::Text(_), Value::Text(_), Some(x), Some(y)) => (Value::Number(x), Value::Number(y)),
		(Value::Text(_), Value::Number(_), Some(x), _) => (Value::Number(x), b.clone()),
		(Value::Number(_), Value::Text(_), _, Some(y)) => (a.clone(), Value::Number(y)),
		_ => (a.clone(), b.clone()),
	};
	let zero_like = |other: &Value| match other {
		Value::Number(_) => Value::Number(0.0),
		Value::Text(_) => Value::Text(String::new()),
		Value::Bool(_) => Value::Bool(false),
		Value::Empty => Value::Empty,
	};
	let (a, b) = match (&a, &b) {
		(Value::Empty, _) => (zero_like(&b), b),
		(_, Value::Empty) => {
			let zero = zero_like(&a);
			(a, zero)
		}
		_ => (a, b),
	};
	let rank = |value: &Value| match value {
		Value::Number(_) | Value::Empty => 0,
		Value::Text(_) => 1,
		Value::Bool(_) => 2,
	};
	Ok(match (&a, &b) {
		(Value::Number(x), Value::Number(y)) => {
			if approximately_equal(*x, *y) {
				Ordering::Equal
			} else {
				x.total_cmp(y)
			}
		}
		(Value::Text(x), Value::Text(y)) => compare_text(x, y)?,
		(Value::Bool(x), Value::Bool(y)) => x.cmp(y),
		_ => rank(&a).cmp(&rank(&b)),
	})
}

/// The en-US collation order, ignoring case and accents, of the ASCII
/// characters it gives a weight. Letters stand for both cases.
const COLLATION: &str =
	"\t\n\u{0B}\u{0C}\r _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$0123456789abcdefghijklmnopqrstuvwxyz";

/// HyperFormula compares text with the locale's collator, which ignores
/// case and accents. Veritas has only the ASCII part of that table, so text
/// with any other character compares only when it is identical.
fn compare_text(a: &str, b: &str) -> Outcome<Ordering> {
	if a == b {
		return Ok(Ordering::Equal);
	}
	let weights = |text: &str| -> Option<Vec<usize>> {
		text.chars()
			.map(|c| COLLATION.find(c.to_ascii_lowercase()))
			.collect()
	};
	match (weights(a), weights(b)) {
		(Some(x), Some(y)) => Ok(x.cmp(&y)),
		_ => Err(unsupported(format!(
			"comparing {a:?} with {b:?}, which needs the full Unicode collation table"
		))
		.into()),
	}
}
