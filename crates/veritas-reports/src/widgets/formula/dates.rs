//! HyperFormula's `DateTimeHelper` and `defaultParseToDateTime`
//! (`es/DateTimeHelper.mjs`, `es/DateTimeDefault.mjs`) with Actual's
//! `dateFormats` and the default `timeFormats`, `nullDate` 1899-12-30,
//! `nullYear` 30 and `leapYear1900` false. Arithmetic stays in `f64`, as in
//! JavaScript, so out-of-range input fails the same way.

use super::js;

const MONTH_DAYS: [f64; 12] = [
	31.0, 28.0, 31.0, 30.0, 31.0, 30.0, 31.0, 31.0, 30.0, 31.0, 30.0, 31.0,
];
const DAYS_BEFORE: [f64; 12] = [
	0.0, 31.0, 59.0, 90.0, 120.0, 151.0, 181.0, 212.0, 243.0, 273.0, 304.0, 334.0,
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct SimpleDate {
	pub year: f64,
	pub month: f64,
	pub day: f64,
}

#[derive(Clone, Copy, Debug)]
struct SimpleTime {
	hours: f64,
	minutes: f64,
	seconds: f64,
}

fn leap_years_before(year: f64) -> f64 {
	(year / 4.0).floor() - (year / 100.0).floor() + (year / 400.0).floor()
}

pub(super) fn is_leap_year(year: f64) -> bool {
	if year % 4.0 != 0.0 {
		false
	} else if year % 100.0 != 0.0 {
		true
	} else {
		year % 400.0 == 0.0
	}
}

fn month_index(month: f64) -> Option<usize> {
	(month.fract() == 0.0 && (1.0..=12.0).contains(&month)).then(|| month as usize - 1)
}

fn days_from_zero(date: SimpleDate) -> f64 {
	let Some(index) = month_index(date.month) else {
		return f64::NAN;
	};
	let leap_years = if date.month <= 2.0 {
		leap_years_before(date.year - 1.0)
	} else {
		leap_years_before(date.year)
	};
	365.0 * date.year + DAYS_BEFORE[index] + date.day - 1.0 + leap_years
}

fn null_date() -> f64 {
	days_from_zero(SimpleDate {
		year: 1899.0,
		month: 12.0,
		day: 30.0,
	})
}

/// `dateToNumber`: the serial HyperFormula counts from 1899-12-30.
pub(super) fn to_serial(date: SimpleDate) -> f64 {
	days_from_zero(date) - null_date()
}

fn max_serial() -> f64 {
	to_serial(SimpleDate {
		year: 9999.0,
		month: 12.0,
		day: 31.0,
	})
}

/// `getWithinBounds`.
pub(super) fn within_bounds(serial: f64) -> Option<f64> {
	(serial <= max_serial() && serial >= 0.0).then_some(serial)
}

/// `isValidDate`.
pub(super) fn is_valid(date: SimpleDate) -> bool {
	let SimpleDate { year, month, day } = date;
	if year.is_nan() || month.is_nan() || day.is_nan() {
		return false;
	}
	if day != js::round(day) || month != js::round(month) || year != js::round(year) {
		return false;
	}
	if year < 1582.0 || !(1.0..=12.0).contains(&month) || day < 1.0 {
		return false;
	}
	day <= days_in_month(year, month)
}

/// `daysInMonth`, NaN for a month that isn't 1 to 12.
pub(super) fn days_in_month(year: f64, month: f64) -> f64 {
	if is_leap_year(year) && month == 2.0 {
		return 29.0;
	}
	month_index(month).map_or(f64::NAN, |index| MONTH_DAYS[index])
}

fn day_to_month(day_of_year: f64) -> usize {
	let mut month = 0;
	if DAYS_BEFORE[month + 6] <= day_of_year {
		month += 6;
	}
	if DAYS_BEFORE[month + 3] <= day_of_year {
		month += 3;
	}
	if DAYS_BEFORE[month + 2] <= day_of_year {
		month += 2;
	} else if DAYS_BEFORE[month + 1] <= day_of_year {
		month += 1;
	}
	month
}

/// `numberToSimpleDate`.
pub(super) fn from_serial(serial: f64) -> SimpleDate {
	let number = serial.floor() + null_date();
	let first_of = |year: f64| {
		days_from_zero(SimpleDate {
			year,
			month: 1.0,
			day: 1.0,
		})
	};
	let mut year = (number / 365.2425).floor();
	if first_of(year + 1.0) <= number {
		year += 1.0;
	} else if first_of(year - 1.0) > number {
		year -= 1.0;
	}
	let day_of_year = number - first_of(year);
	let leap = is_leap_year(year);
	let month = day_to_month(
		day_of_year
			- if leap && day_of_year >= 59.0 {
				1.0
			} else {
				0.0
			},
	);
	let day = day_of_year - DAYS_BEFORE[month] - if leap && month > 1 { 1.0 } else { 0.0 };
	SimpleDate {
		year,
		month: month as f64 + 1.0,
		day: day + 1.0,
	}
}

fn time_to_number(time: SimpleTime) -> f64 {
	((time.seconds / 60.0 + time.minutes) / 60.0 + time.hours) / 24.0
}

#[derive(Clone, Copy)]
struct DateFormat {
	items: usize,
	day: usize,
	month: usize,
	year: usize,
}

/// `parseDateFormat` of 'DD/MM/YYYY', 'YYYY-MM-DD' and 'YYYY/MM/DD'.
const DATE_FORMATS: [DateFormat; 3] = [
	DateFormat {
		items: 3,
		day: 0,
		month: 1,
		year: 2,
	},
	DateFormat {
		items: 3,
		day: 2,
		month: 1,
		year: 0,
	},
	DateFormat {
		items: 3,
		day: 2,
		month: 1,
		year: 0,
	},
];

#[derive(Clone, Copy)]
struct TimeFormat {
	items: usize,
	seconds: Option<usize>,
}

/// `parseTimeFormat` of 'hh:mm' and 'hh:mm:ss.sss'; hours come first and
/// minutes second in both.
const TIME_FORMATS: [TimeFormat; 2] = [
	TimeFormat {
		items: 2,
		seconds: None,
	},
	TimeFormat {
		items: 3,
		seconds: Some(2),
	},
];

fn is_integer(value: f64) -> bool {
	value.is_finite() && value.fract() == 0.0
}

fn parse_time(mut items: Vec<String>, format: TimeFormat) -> Option<SimpleTime> {
	let meridiem = match items.last().map(String::as_str) {
		Some("am" | "a") => Some(false),
		Some("pm" | "p") => Some(true),
		_ => None,
	};
	if meridiem.is_some() {
		items.pop();
	}
	if items.len() != format.items {
		return None;
	}
	let item = |index: Option<usize>| index.and_then(|i| items.get(i)).map_or("0", String::as_str);
	let seconds = js::plain_number(item(format.seconds)).filter(|s| s.is_finite())?;
	let seconds = js::round(seconds * 1000.0) / 1000.0;
	let minutes = js::plain_number(item(Some(1))).filter(|m| is_integer(*m))?;
	let hours = js::plain_number(item(Some(0))).filter(|h| is_integer(*h))?;
	let hours = match meridiem {
		Some(_) if !(0.0..=12.0).contains(&hours) => return None,
		Some(pm) => hours % 12.0 + if pm { 12.0 } else { 0.0 },
		None => hours,
	};
	Some(SimpleTime {
		hours,
		minutes,
		seconds,
	})
}

fn parse_date(items: &[&str], format: DateFormat) -> Option<SimpleDate> {
	if items.len() != format.items {
		return None;
	}
	let day = js::plain_number(items[format.day]).filter(|d| is_integer(*d))?;
	let month = js::plain_number(items[format.month]).filter(|m| is_integer(*m))?;
	let year_text = items[format.year];
	let year = js::plain_number(year_text).filter(|y| is_integer(*y))?;
	if !year_text.is_empty() && !(1000.0..=9999.0).contains(&year) {
		return None;
	}
	Some(SimpleDate { year, month, day })
}

enum Parsed {
	Date(SimpleDate),
	Time(SimpleTime),
	DateTime(SimpleDate, SimpleTime),
}

fn is_date_separator(c: char) -> bool {
	matches!(c, ' ' | '/' | '.' | '-')
}

/// `defaultParseToDateTime` for one date format and one time format.
fn parse_with(text: &str, date_format: DateFormat, time_format: TimeFormat) -> Option<Parsed> {
	let mut text = text.to_owned();
	let body = text.trim_end_matches(['a', 'p', 'm']);
	let suffix = &text[body.len()..];
	let looks_like_a_date = !body.is_empty()
		&& body
			.bytes()
			.all(|b| b.is_ascii_digit() || b"/.-: ".contains(&b))
		&& matches!(suffix, "" | "a" | "p" | "m" | "am" | "pm");
	if !looks_like_a_date {
		return None;
	}
	let mut meridiem = None;
	for token in ["am", "pm", "a", "p"] {
		if text.ends_with(token) {
			meridiem = Some(token.to_owned());
			text = text[..text.len() - token.len()]
				.trim_matches(is_space)
				.to_owned();
			break;
		}
	}
	let mut date_items: Vec<String> = text.split(is_date_separator).map(str::to_owned).collect();
	let count = date_items.len();
	if count >= 2 && date_items[count - 2].contains(':') {
		let last = date_items.pop().unwrap_or_default();
		date_items[count - 2] = format!("{}.{last}", date_items[count - 2]);
	}
	let mut time_items: Vec<String> = date_items
		.last()
		.map(|last| last.split(':').map(str::to_owned).collect())
		.unwrap_or_default();
	if let Some(meridiem) = meridiem {
		time_items.push(meridiem);
	}
	if date_items.len() == 1 {
		return parse_time(time_items, time_format).map(Parsed::Time);
	}
	let all: Vec<&str> = date_items.iter().map(String::as_str).collect();
	if time_items.len() == 1 {
		return parse_date(&all, date_format).map(Parsed::Date);
	}
	let date = parse_date(&all[..all.len() - 1], date_format)?;
	let time = parse_time(time_items, time_format)?;
	Some(Parsed::DateTime(date, time))
}

fn is_space(c: char) -> bool {
	crate::filter::is_js_space(c)
}

/// The text `defaultParseToDateTime` reads: the first run of whitespace made
/// one space, then trimmed and lowercased.
fn normalized(text: &str) -> String {
	let mut out = String::with_capacity(text.len());
	let mut chars = text.chars().peekable();
	let mut replaced = false;
	while let Some(c) = chars.next() {
		if !replaced && is_space(c) {
			while chars.peek().is_some_and(|next| is_space(*next)) {
				chars.next();
			}
			out.push(' ');
			replaced = true;
		} else {
			out.push(c);
		}
	}
	js::trim(&out).to_ascii_lowercase()
}

/// `parseSingleFormat`: two-digit years and date validity.
fn finish(parsed: Parsed) -> Option<Parsed> {
	let fix = |mut date: SimpleDate| {
		if (0.0..100.0).contains(&date.year) {
			date.year += if date.year < 30.0 { 2000.0 } else { 1900.0 };
		}
		is_valid(date).then_some(date)
	};
	Some(match parsed {
		Parsed::Date(date) => Parsed::Date(fix(date)?),
		Parsed::DateTime(date, time) => Parsed::DateTime(fix(date)?, time),
		Parsed::Time(time) => Parsed::Time(time),
	})
}

/// `dateStringToDateNumber`: the serial a date, time or date-time string
/// reads as, or `None`.
pub(super) fn text_to_serial(text: &str) -> Option<f64> {
	let text = normalized(text);
	if text.is_empty() {
		return None;
	}
	for date_format in DATE_FORMATS {
		for time_format in TIME_FORMATS {
			let Some(parsed) = parse_with(&text, date_format, time_format).and_then(finish) else {
				continue;
			};
			return Some(match parsed {
				Parsed::Date(date) => to_serial(date),
				Parsed::Time(time) => time_to_number(time),
				Parsed::DateTime(date, time) => time_to_number(time) + to_serial(date),
			});
		}
	}
	None
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn date_strings_read_as_hyperformula_reads_them() {
		assert_eq!(text_to_serial("2026-01-15"), Some(46037.0));
		assert_eq!(text_to_serial("15/01/"), Some(36540.0));
		assert_eq!(text_to_serial("10:"), Some(10.0 / 24.0));
		assert_eq!(text_to_serial("5"), None);
		assert_eq!(text_to_serial("2026-02-30"), None);
	}

	#[test]
	fn serials_round_trip_through_leap_days() {
		for (year, month, day) in [(2024.0, 2.0, 29.0), (2000.0, 3.0, 1.0), (1900.0, 3.0, 1.0)] {
			let date = SimpleDate { year, month, day };
			assert_eq!(from_serial(to_serial(date)), date);
		}
	}
}
