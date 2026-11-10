use rusqlite::types::ValueRef;
use veritas_domain::{CivilDate, Money, YearMonth};

pub(crate) type Cell<T> = Result<T, &'static str>;

pub(crate) fn text(value: ValueRef<'_>) -> Cell<Option<String>> {
	match value {
		ValueRef::Null => Ok(None),
		ValueRef::Text(bytes) => Ok(Some(String::from_utf8_lossy(bytes).into_owned())),
		_ => Err("a text column holds a number or blob"),
	}
}

pub(crate) fn required_text(value: ValueRef<'_>, missing: &'static str) -> Cell<String> {
	text(value)?.ok_or(missing)
}

pub(crate) fn integer(value: ValueRef<'_>) -> Cell<Option<i64>> {
	match value {
		ValueRef::Null => Ok(None),
		ValueRef::Integer(n) => Ok(Some(n)),
		// JS reads 1200.0 and 1200 as the same number.
		ValueRef::Real(x) if x.fract() == 0.0 && x.abs() < 9.2e18 => Ok(Some(x as i64)),
		_ => Err("an integer column holds a fraction, text or blob"),
	}
}

pub(crate) fn money(value: ValueRef<'_>) -> Cell<Option<Money>> {
	integer(value).map(|n| n.map(Money::from_minor))
}

fn number(value: ValueRef<'_>) -> Option<f64> {
	match value {
		ValueRef::Integer(n) => Some(n as f64),
		ValueRef::Real(x) => Some(x),
		_ => None,
	}
}

/// Actual's AQL reads a boolean as `value === 1`, so NULL, 2 and '1' are false.
pub(crate) fn flag(value: ValueRef<'_>) -> bool {
	number(value) == Some(1.0)
}

/// The views treat a row as alive when `IFNULL(tombstone, 0) = 0`.
pub(crate) fn tombstone(value: ValueRef<'_>) -> bool {
	!matches!(value, ValueRef::Null) && number(value) != Some(0.0)
}

pub(crate) fn sort_order(value: ValueRef<'_>) -> Cell<f64> {
	match value {
		ValueRef::Null => Ok(0.0),
		_ => number(value).ok_or("sort_order isn't a number"),
	}
}

pub(crate) fn date_from_int(n: i64) -> Cell<CivilDate> {
	if !(10_000_101..=99_991_231).contains(&n) {
		return Err("a date isn't YYYYMMDD");
	}
	let (year, month, day) = (n / 10_000, n / 100 % 100, n % 100);
	CivilDate::new(year as i32, month as u32, day as u32).map_err(|_| "a date isn't YYYYMMDD")
}

pub(crate) fn date(value: ValueRef<'_>) -> Cell<Option<CivilDate>> {
	integer(value)?.map(date_from_int).transpose()
}

pub(crate) fn db_month(value: ValueRef<'_>) -> Cell<Option<YearMonth>> {
	integer(value)?
		.map(|n| {
			if !(100_001..=999_912).contains(&n) {
				return Err("a budget month isn't YYYYMM");
			}
			YearMonth::new((n / 100) as i32, (n % 100) as u32)
				.map_err(|_| "a budget month isn't YYYYMM")
		})
		.transpose()
}

pub(crate) fn text_month(value: ValueRef<'_>) -> Cell<Option<YearMonth>> {
	text(value)?
		.map(|text| YearMonth::parse(&text).map_err(|_| "a month isn't YYYY-MM"))
		.transpose()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn booleans_follow_actuals_strict_equality() {
		assert!(flag(ValueRef::Integer(1)));
		assert!(flag(ValueRef::Real(1.0)));
		for falsy in [
			ValueRef::Null,
			ValueRef::Integer(0),
			ValueRef::Integer(2),
			ValueRef::Text(b"1"),
		] {
			assert!(!flag(falsy));
		}
	}

	#[test]
	fn tombstones_are_anything_but_null_or_zero() {
		assert!(!tombstone(ValueRef::Null));
		assert!(!tombstone(ValueRef::Integer(0)));
		assert!(tombstone(ValueRef::Integer(1)));
		assert!(tombstone(ValueRef::Integer(2)));
		assert!(tombstone(ValueRef::Text(b"yes")));
	}

	#[test]
	fn dates_and_months_decode_or_report() {
		assert_eq!(
			date(ValueRef::Integer(20261109))
				.unwrap()
				.unwrap()
				.to_string(),
			"2026-11-09"
		);
		assert!(date(ValueRef::Integer(20270229)).is_err());
		assert!(date(ValueRef::Integer(2026110)).is_err());
		assert!(date(ValueRef::Text(b"2026-11-09")).is_err());
		assert_eq!(
			db_month(ValueRef::Integer(202611))
				.unwrap()
				.unwrap()
				.to_string(),
			"2026-11"
		);
		assert!(db_month(ValueRef::Integer(202613)).is_err());
		assert_eq!(
			text_month(ValueRef::Text(b"2026-07"))
				.unwrap()
				.unwrap()
				.to_string(),
			"2026-07"
		);
	}

	#[test]
	fn integral_reals_read_as_amounts_but_fractions_do_not() {
		assert_eq!(
			money(ValueRef::Real(1200.0)),
			Ok(Some(Money::from_minor(1200)))
		);
		assert!(money(ValueRef::Real(12.5)).is_err());
		assert_eq!(money(ValueRef::Null), Ok(None));
	}
}
