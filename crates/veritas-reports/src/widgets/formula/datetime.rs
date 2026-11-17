//! Written clean-room from HyperFormula's public docs and black-box tests
//! against Actual's formula card. No HyperFormula source was read.
//!
//! Date serials (days since `nullDate`, 1899-12-30, with no 1900 leap day)
//! and the date and time text Actual's card accepts: its `dateFormats`
//! `DD/MM/YYYY`, `YYYY-MM-DD` and `YYYY/MM/DD`, and the default
//! `timeFormats`.

use chrono::{Datelike, NaiveDate};

use crate::filter::is_js_space;

/// 9999-12-31, the last date DATE and EOMONTH return.
pub(super) const MAX_SERIAL: i128 = 2_958_465;

/// Days from 1970-01-01 to a proleptic Gregorian date; `month` is 1 to 12.
fn days_from_civil(year: i128, month: i128, day: i128) -> i128 {
	let year = if month <= 2 { year - 1 } else { year };
	let era = year.div_euclid(400);
	let year_of_era = year - era * 400;
	let month_index = (month + 9) % 12;
	let day_of_year = (153 * month_index + 2) / 5 + day - 1;
	let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
	era * 146_097 + day_of_era - 719_468
}

/// The serial of a date, where `day` may run past the month's end.
pub(super) fn serial(year: i128, month: i128, day: i128) -> i128 {
	days_from_civil(year, month, 1) + day - 1 - days_from_civil(1899, 12, 30)
}

/// The date a serial falls on, as (year, month, day).
pub(super) fn civil(serial: i128) -> (i128, i128, i128) {
	let days = serial + days_from_civil(1899, 12, 30) + 719_468;
	let era = days.div_euclid(146_097);
	let day_of_era = days - era * 146_097;
	let year_of_era =
		(day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
	let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
	let month_index = (5 * day_of_year + 2) / 153;
	let day = day_of_year - (153 * month_index + 2) / 5 + 1;
	let month = if month_index < 10 {
		month_index + 3
	} else {
		month_index - 9
	};
	let year = year_of_era + era * 400 + i128::from(month <= 2);
	(year, month, day)
}

pub(super) fn days_in_month(year: i128, month: i128) -> i128 {
	let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
	match month {
		2 if leap => 29,
		2 => 28,
		4 | 6 | 9 | 11 => 30,
		_ => 31,
	}
}

pub(super) fn today_serial(today: NaiveDate) -> f64 {
	serial(
		i128::from(today.year()),
		i128::from(today.month()),
		i128::from(today.day()),
	) as f64
}

/// Text that reads as a date, a time, or a date then a time, as its serial
/// number; `None` for anything else.
///
/// Only the first run of whitespace may be longer than one space, as if it
/// were collapsed and every later one taken as written: `15  01 2026` is a
/// date, `15 01  2026` isn't.
pub(super) fn parse_date_time(text: &str) -> Option<f64> {
	let text = text.trim_matches(is_js_space);
	if text.is_empty() {
		return None;
	}
	let text = collapse_first_space_run(text);
	if let Some(serial) = parse_date(&text) {
		return Some(serial as f64);
	}
	if let Some(time) = parse_time(&text) {
		return Some(time);
	}
	text.match_indices(' ').find_map(|(at, _)| {
		let date = parse_date(&text[..at])?;
		let time = parse_time(&text[at + 1..])?;
		Some(date as f64 + time)
	})
}

fn collapse_first_space_run(text: &str) -> String {
	let Some(start) = text.find(is_js_space) else {
		return text.to_owned();
	};
	let rest = text[start..].trim_start_matches(is_js_space);
	format!("{} {rest}", &text[..start])
}

/// Three runs of digits split by single `/`, `-`, `.` or spaces, read day
/// first and then year first. An empty year reads as 2000.
fn parse_date(text: &str) -> Option<i128> {
	let parts: Vec<&str> = text.split(['/', '-', '.', ' ']).collect();
	let [first, second, third] = parts.as_slice() else {
		return None;
	};
	if !parts
		.iter()
		.all(|part| part.bytes().all(|b| b.is_ascii_digit()))
	{
		return None;
	}
	date_parts(first, second, third).or_else(|| date_parts(third, second, first))
}

fn date_parts(day: &str, month: &str, year: &str) -> Option<i128> {
	let number = |digits: &str| -> Option<i128> {
		if digits.is_empty() {
			None
		} else {
			digits.parse().ok()
		}
	};
	let year = if year.is_empty() {
		2000
	} else {
		number(year).filter(|year| (1582..=9999).contains(year))?
	};
	let month = number(month).filter(|month| (1..=12).contains(month))?;
	let day = number(day).filter(|day| (1..=days_in_month(year, month)).contains(day))?;
	Some(serial(year, month, day))
}

/// `hh:mm` or `hh:mm:ss.sss` with an optional am/pm, as a fraction of a
/// day. Any part may be empty, and only am/pm limits the hour.
fn parse_time(text: &str) -> Option<f64> {
	let lower = text.to_ascii_lowercase();
	let (clock, meridiem) = ["am", "pm", "a", "p"]
		.iter()
		.find_map(|suffix| {
			lower
				.strip_suffix(suffix)
				.map(|rest| (rest.trim_end_matches(is_js_space), suffix.starts_with('p')))
		})
		.map_or((lower.as_str(), None), |(rest, pm)| (rest, Some(pm)));
	let mut fields = clock.split(':');
	let (hours, minutes, seconds) = (fields.next()?, fields.next()?, fields.next().unwrap_or(""));
	if fields.next().is_some() {
		return None;
	}
	let whole = |digits: &str| -> Option<f64> {
		if digits.bytes().all(|b| b.is_ascii_digit()) {
			Some(digits.parse().unwrap_or(0.0))
		} else {
			None
		}
	};
	// A minus sign is allowed before minutes and seconds that are zero.
	let unsigned = |field: &str| -> Option<(String, bool)> {
		match field.strip_prefix('-') {
			Some(rest) if !rest.is_empty() => Some((rest.to_owned(), true)),
			Some(_) => None,
			None => Some((field.to_owned(), false)),
		}
	};
	let (minutes, minutes_signed) = unsigned(minutes)?;
	let (seconds, seconds_signed) = unsigned(seconds)?;
	let mut hours = whole(hours)?;
	let minutes = whole(&minutes)?;
	let seconds = match seconds.split_once('.') {
		Some((int, fraction)) if !(int.is_empty() && fraction.is_empty()) => {
			whole(int)?;
			whole(fraction)?;
			format!("0{int}.{fraction}0").parse().ok()?
		}
		Some(_) => return None,
		None => whole(&seconds)?,
	};
	if (minutes_signed && minutes != 0.0) || (seconds_signed && seconds != 0.0) {
		return None;
	}
	if let Some(pm) = meridiem {
		if hours > 12.0 {
			return None;
		}
		hours = hours % 12.0 + if pm { 12.0 } else { 0.0 };
	}
	Some((hours * 3600.0 + minutes * 60.0 + seconds) / 86_400.0)
}
