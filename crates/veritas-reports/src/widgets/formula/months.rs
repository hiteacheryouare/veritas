//! Actual's month parsing (`packages/loot-core/src/shared/months.ts`
//! `_parse`), which the formula card's query time frames go through.

use crate::filter::is_js_space;

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
	fn months_roll_over_like_javascript_dates() {
		let day = |text: &str| parse_month_date(text).map(|date| date.to_string());
		assert_eq!(day("2026-13"), Some("2027-01-01".to_owned()));
		assert_eq!(day("2026-00"), Some("2025-12-01".to_owned()));
		assert_eq!(day("2026-02-30"), Some("2026-03-02".to_owned()));
		assert_eq!(day("26-01"), Some("1926-01-01".to_owned()));
		assert_eq!(day("abc"), None);
	}
}
