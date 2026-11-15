//! The JavaScript and HyperFormula 3.5.0 primitives the formula card leans
//! on, written so each one returns the same bits V8 does.

use std::cmp::Ordering;

use veritas_domain::CoreError;

use crate::filter::is_js_space;
use crate::report_query::unsupported;

/// `Math.round`: halves go up, toward +∞.
pub(super) fn round(x: f64) -> f64 {
	if !x.is_finite() {
		return x;
	}
	let floor = x.floor();
	if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

/// `Math.pow` as V8 runs it with its default `--use-std-math-pow`: the
/// platform's `pow`, which `powf` also calls, after V8's own shortcuts for
/// squares and square roots (`src/numbers/ieee754.cc`).
pub(super) fn pow(x: f64, y: f64) -> f64 {
	if y == 2.0 {
		x * x
	} else if y == 0.5 {
		if x.is_infinite() {
			f64::INFINITY
		} else {
			(x + 0.0).sqrt()
		}
	} else {
		x.powf(y)
	}
}

/// `String.prototype.trim`.
pub(super) fn trim(text: &str) -> &str {
	text.trim_matches(is_js_space)
}

/// `Number.prototype.toString()`: the shortest digits that round-trip, laid
/// out by ECMA-262's Number::toString.
pub(super) fn number_text(x: f64) -> String {
	if x == 0.0 {
		return "0".to_owned();
	}
	if x.is_nan() {
		return "NaN".to_owned();
	}
	if x.is_infinite() {
		return if x > 0.0 { "Infinity" } else { "-Infinity" }.to_owned();
	}
	let scientific = format!("{:e}", x.abs());
	let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
	let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
	let n = exponent.parse::<i32>().unwrap_or(0) + 1;
	let k = i32::try_from(digits.len()).unwrap_or(i32::MAX);
	let zeros = |count: i32| "0".repeat(usize::try_from(count).unwrap_or(0));
	let body = if k <= n && n <= 21 {
		format!("{digits}{}", zeros(n - k))
	} else if 0 < n && n <= 21 {
		let (whole, fraction) = digits.split_at(usize::try_from(n).unwrap_or(0));
		format!("{whole}.{fraction}")
	} else if -6 < n && n <= 0 {
		format!("0.{}{digits}", zeros(-n))
	} else {
		let (first, rest) = digits.split_at(1);
		let point = if rest.is_empty() {
			String::new()
		} else {
			format!(".{rest}")
		};
		let sign = if n > 0 { '+' } else { '-' };
		format!("{first}{point}e{sign}{}", (n - 1).abs())
	};
	if x < 0.0 { format!("-{body}") } else { body }
}

/// `Number(text)` for text HyperFormula has already cut down to digits and
/// at most one point: empty text is 0.
pub(super) fn plain_number(text: &str) -> Option<f64> {
	if text.is_empty() {
		return Some(0.0);
	}
	let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
	let digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
	if !digits(whole) || !digits(fraction) || (whole.is_empty() && fraction.is_empty()) {
		return None;
	}
	format!(
		"{}.{}",
		if whole.is_empty() { "0" } else { whole },
		if fraction.is_empty() { "0" } else { fraction }
	)
	.parse()
	.ok()
}

/// `NumberLiteralHelper.numericStringToMaybeNumber` with the default empty
/// thousands separator: `^[+-]?(\.\d+|\d+(\.\d*)?)([eE][+-]?\d+)?$`.
pub(super) fn numeric_text(text: &str) -> Option<f64> {
	let (sign, rest) = match text.as_bytes().first() {
		Some(b'+') => ("", &text[1..]),
		Some(b'-') => ("-", &text[1..]),
		_ => ("", text),
	};
	let (mantissa, exponent) = match rest.find(['e', 'E']) {
		Some(at) => (&rest[..at], Some(&rest[at + 1..])),
		None => (rest, None),
	};
	let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
	let digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
	let has_point = mantissa.contains('.');
	if !digits(whole) || !digits(fraction) {
		return None;
	}
	if whole.is_empty() && (!has_point || fraction.is_empty()) {
		return None;
	}
	let exponent = match exponent {
		None => "0",
		Some(exponent) => {
			let unsigned = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
			if unsigned.is_empty() || !digits(unsigned) {
				return None;
			}
			exponent
		}
	};
	format!(
		"{sign}{}.{}e{exponent}",
		if whole.is_empty() { "0" } else { whole },
		if fraction.is_empty() { "0" } else { fraction }
	)
	.parse()
	.ok()
}

/// `Exporter.cellValueRounding` with `precisionRounding` 10: rounded to
/// 11 significant digits, the magnitude read with `Math.log10`.
pub(super) fn export_rounding(value: f64) -> f64 {
	if value == 0.0 {
		return value;
	}
	let places = pow(10.0, 10.0 - log10(value.abs()).floor());
	if value < 0.0 {
		-round(-value * places) / places
	} else {
		round(value * places) / places
	}
}

fn high_word(x: f64) -> i32 {
	(x.to_bits() >> 32) as u32 as i32
}

fn low_word(x: f64) -> u32 {
	x.to_bits() as u32
}

fn from_words(high: i32, low: u32) -> f64 {
	f64::from_bits((u64::from(high as u32) << 32) | u64::from(low))
}

/// `Math.log10`: fdlibm's `log10` and `log` as V8 has them
/// (`src/base/ieee754.cc`), constants as fdlibm's hex, with each `a * b + c` fused the way clang
/// contracts them when it builds Node for arm64. Matches Node 24 bit for bit
/// on 400,000 sampled values, where Rust's own `log10` differs on 2%.
#[allow(clippy::many_single_char_names)]
fn log10(mut x: f64) -> f64 {
	const TWO54: f64 = f64::from_bits(0x4350_0000_0000_0000);
	const IVLN10: f64 = f64::from_bits(0x3fdb_cb7b_1526_e50e);
	const LOG10_2HI: f64 = f64::from_bits(0x3fd3_4413_509f_6000);
	const LOG10_2LO: f64 = f64::from_bits(0x3d59_fef3_11f1_2b36);
	let (mut hx, mut lx) = (high_word(x), low_word(x));
	let mut k: i32 = 0;
	if hx < 0x0010_0000 {
		if ((hx & 0x7fff_ffff) as u32 | lx) == 0 {
			return f64::NEG_INFINITY;
		}
		if hx < 0 {
			return f64::NAN;
		}
		k -= 54;
		x *= TWO54;
		(hx, lx) = (high_word(x), low_word(x));
	}
	if hx >= 0x7ff0_0000 {
		return x + x;
	}
	if hx == 0x3ff0_0000 && lx == 0 {
		return 0.0;
	}
	k += (hx >> 20) - 1023;
	let i = ((k as u32 & 0x8000_0000) >> 31) as i32;
	hx = (hx & 0x000f_ffff) | ((0x3ff - i) << 20);
	let y = f64::from(k + i);
	let z = y.mul_add(LOG10_2LO, IVLN10 * log(from_words(hx, lx)));
	y.mul_add(LOG10_2HI, z)
}

#[allow(clippy::many_single_char_names)]
fn log(mut x: f64) -> f64 {
	const LN2_HI: f64 = f64::from_bits(0x3fe6_2e42_fee0_0000);
	const LN2_LO: f64 = f64::from_bits(0x3dea_39ef_3579_3c76);
	const LG1: f64 = f64::from_bits(0x3fe5_5555_5555_5593);
	const LG2: f64 = f64::from_bits(0x3fd9_9999_9997_fa04);
	const LG3: f64 = f64::from_bits(0x3fd2_4924_9422_9359);
	const LG4: f64 = f64::from_bits(0x3fcc_71c5_1d8e_78af);
	const LG5: f64 = f64::from_bits(0x3fc7_4664_96cb_03de);
	const LG6: f64 = f64::from_bits(0x3fc3_9a09_d078_c69f);
	const LG7: f64 = f64::from_bits(0x3fc2_f112_df3e_5244);
	let mut hx = high_word(x);
	let mut k: i32 = 0;
	if hx < 0x0010_0000 {
		if ((hx & 0x7fff_ffff) as u32 | low_word(x)) == 0 {
			return f64::NEG_INFINITY;
		}
		if hx < 0 {
			return f64::NAN;
		}
		k -= 54;
		x *= f64::from_bits(0x4350_0000_0000_0000);
		hx = high_word(x);
	}
	if hx >= 0x7ff0_0000 {
		return x + x;
	}
	k += (hx >> 20) - 1023;
	hx &= 0x000f_ffff;
	let i = (hx + 0x95f64) & 0x10_0000;
	x = from_words(hx | (i ^ 0x3ff0_0000), low_word(x));
	k += i >> 20;
	let f = x - 1.0;
	let dk = f64::from(k);
	if (0x000f_ffff & (2 + hx)) < 3 {
		if f == 0.0 {
			return if k == 0 {
				0.0
			} else {
				dk.mul_add(LN2_HI, dk * LN2_LO)
			};
		}
		let r = f * f * (-f64::from_bits(0x3fd5_5555_5555_5555)).mul_add(f, 0.5);
		if k == 0 {
			return f - r;
		}
		return dk.mul_add(LN2_HI, -((-dk).mul_add(LN2_LO, r) - f));
	}
	let s = f / (2.0 + f);
	let z = s * s;
	let w = z * z;
	let t1 = w * w.mul_add(w.mul_add(LG6, LG4), LG2);
	let t2 = z * w.mul_add(w.mul_add(w.mul_add(LG7, LG5), LG3), LG1);
	let r = t2 + t1;
	if ((hx - 0x6147a) | (0x6b851 - hx)) > 0 {
		let hfsq = 0.5 * f * f;
		if k == 0 {
			f - (-s).mul_add(hfsq + r, hfsq)
		} else {
			dk.mul_add(LN2_HI, -((hfsq - s.mul_add(hfsq + r, dk * LN2_LO)) - f))
		}
	} else if k == 0 {
		(-s).mul_add(f - r, f)
	} else {
		dk.mul_add(LN2_HI, -(s.mul_add(f - r, -(dk * LN2_LO)) - f))
	}
}

/// The order of printable ASCII under `Intl.Collator('en-US', {sensitivity:
/// 'base'})`, which HyperFormula compares text with. Letters share a rank
/// with their capitals. Checked against Node's ICU on 600,000 random pairs:
/// within printable ASCII the collator compares these ranks one character
/// at a time.
const COLLATION: &str = " _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$0123456789AaBbCcDdEeFfGgHhIiJjKkLlMmNnOoPpQqRrSsTtUuVvWwXxYyZz";

fn collation_rank(c: char) -> Option<usize> {
	if c.is_ascii_alphabetic() {
		let letters = COLLATION.find('A')?;
		return Some(letters + (c.to_ascii_uppercase() as usize - 'A' as usize));
	}
	COLLATION
		.find(c)
		.filter(|_| c.is_ascii() && !c.is_ascii_control())
}

/// `ArithmeticHelper.stringCmp`. Text outside printable ASCII can't be
/// ordered without ICU, so it is refused unless the two are identical.
pub(super) fn collate(left: &str, right: &str) -> Result<Ordering, CoreError> {
	let ranks = |text: &str| text.chars().map(collation_rank).collect::<Option<Vec<_>>>();
	match (ranks(left), ranks(right)) {
		(Some(left), Some(right)) => Ok(left.cmp(&right)),
		_ if left == right => Ok(Ordering::Equal),
		_ => Err(unsupported(
			"comparing text outside printable ASCII, which needs ICU's collation".to_owned(),
		)),
	}
}

/// `parseInt(text, 10)` as `_parse` in `shared/months.ts` uses it, `None`
/// for NaN. Results too large for any date are also `None`.
fn parse_int(text: &str) -> Option<i64> {
	let text = text.trim_start_matches(is_js_space);
	let (negative, rest) = match text.as_bytes().first() {
		Some(b'-') => (true, &text[1..]),
		Some(b'+') => (false, &text[1..]),
		_ => (false, text),
	};
	let (radix, rest) = match rest.get(..2) {
		Some("0x" | "0X") => (16, &rest[2..]),
		_ => (10, rest),
	};
	let digits: String = rest.chars().take_while(|c| c.is_digit(radix)).collect();
	if digits.is_empty() {
		return None;
	}
	let value = i64::from_str_radix(&digits, radix).ok()?;
	(value < 1_000_000_000).then_some(if negative { -value } else { value })
}

/// `_parse` from `shared/months.ts`: `yyyy-MM` or `yyyy-MM-dd` read as a
/// local `new Date(year, month - 1, day)`, which rolls over out-of-range
/// months and days and reads years 0 to 99 as 1900 to 1999. `None` is an
/// Invalid Date, which date-fns refuses to format.
pub(super) fn parse_month_date(text: &str) -> Option<chrono::NaiveDate> {
	let mut parts = text.split('-');
	let year = parse_int(parts.next().unwrap_or_default())?;
	let month = parts.next();
	let day = parts.next();
	let month = match month {
		Some(month) => parse_int(month)? - 1,
		None => 0,
	};
	let day = match day {
		Some(day) => parse_int(day)?,
		None => 1,
	};
	let year = if (0..=99).contains(&year) {
		year + 1900
	} else {
		year
	};
	let total = year.checked_mul(12)?.checked_add(month)?;
	let first = chrono::NaiveDate::from_ymd_opt(
		i32::try_from(total.div_euclid(12)).ok()?,
		u32::try_from(total.rem_euclid(12) + 1).ok()?,
		1,
	)?;
	let shifted = if day >= 1 {
		first.checked_add_days(chrono::Days::new(u64::try_from(day - 1).ok()?))
	} else {
		first.checked_sub_days(chrono::Days::new(u64::try_from(1 - day).ok()?))
	}?;
	Some(shifted)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn numbers_print_the_way_javascript_prints_them() {
		for (value, text) in [
			(1.0 / 3.0, "0.3333333333333333"),
			(1e21, "1e+21"),
			(123_456_789_012_345_680_000.0, "123456789012345680000"),
			(1e-7, "1e-7"),
			(0.000_001, "0.000001"),
			(-2.5, "-2.5"),
			(46037.0, "46037"),
			(1.5e300, "1.5e+300"),
		] {
			assert_eq!(number_text(value), text);
		}
	}

	#[test]
	fn log10_lands_where_v8s_does() {
		assert_eq!(log10(9.999_999_999_999_96e18), 19.0);
		assert_eq!(log10(1000.0), 3.0);
		assert_eq!(export_rounding(1.0 / 3.0), 0.333_333_333_33);
	}

	#[test]
	fn numeric_text_follows_hyperformulas_pattern() {
		assert_eq!(numeric_text("1e3"), Some(1000.0));
		assert_eq!(numeric_text("+.5"), Some(0.5));
		assert_eq!(numeric_text("5."), Some(5.0));
		assert_eq!(numeric_text("."), None);
		assert_eq!(numeric_text("1e"), None);
		assert_eq!(numeric_text("1,000"), None);
	}

	#[test]
	fn months_roll_over_like_javascript_dates() {
		let day = |text: &str| parse_month_date(text).map(|date| date.to_string());
		assert_eq!(day("2026-13"), Some("2027-01-01".to_owned()));
		assert_eq!(day("2026-00"), Some("2025-12-01".to_owned()));
		assert_eq!(day("2026-02-30"), Some("2026-03-02".to_owned()));
		assert_eq!(day("26-01"), Some("1926-01-01".to_owned()));
		assert_eq!(day("abc"), None);
	}
}
