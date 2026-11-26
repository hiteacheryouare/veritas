//! The assistant's period resolution (R-AI-1, docs/finance/assistant.md
//! AI-6): the user's words and today's date to the exact dates a tool reads
//! and the label that states them, so neither the model nor the app does date
//! arithmetic.

use std::ops::RangeInclusive;

use veritas_domain::{CivilDate, CoreError, YearMonth};

use crate::budget::civil_date;
use crate::names::fold;
use crate::{FfiError, guard};

pub(crate) const PERIOD_HINT: &str = "Ask again with a period such as this month, so far this month, last month, this year, October 2026, 2026, or 2026-10-01 to 2026-10-15.";

/// The years a period may name. The words come from the model, so anything
/// outside these is refused rather than read.
const YEARS: RangeInclusive<i32> = 1900..=2200;

const MONTH_NAMES: [&str; 12] = [
	"January",
	"February",
	"March",
	"April",
	"May",
	"June",
	"July",
	"August",
	"September",
	"October",
	"November",
	"December",
];

const TO_DATE: &[&str] = &[
	"so far",
	"to date",
	"ytd",
	"mtd",
	"until now",
	"up to now",
	"until today",
	"through today",
];
const LAST_YEAR: &[&str] = &["last year", "previous year", "past year"];
const THIS_YEAR: &[&str] = &["this year", "current year", "year to date", "ytd"];
const THIS_MONTH: &[&str] = &["this month", "same month", "current month"];
const LAST_MONTH: &[&str] = &["last month", "previous month", "past month"];
const MONTH_TO_DATE: &[&str] = &["month to date", "mtd"];

/// The dates a tool reads, inclusive, and the label that states them (AI-6.1).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AssistantPeriod {
	/// `YYYY-MM-DD`.
	pub first: String,
	/// `YYYY-MM-DD`, on or after `first`.
	pub last: String,
	pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum PeriodResolution {
	Range {
		period: AssistantPeriod,
	},
	/// The words name no period this reads; the tool answers with `hint`
	/// and no figure.
	Refused {
		hint: String,
	},
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Span {
	first: CivilDate,
	last: CivilDate,
}

impl Span {
	fn months(first: YearMonth, last: YearMonth) -> Result<Span, CoreError> {
		Ok(Span {
			first: first.first_day(),
			last: last_day(last)?,
		})
	}

	fn month(month: YearMonth) -> Result<Span, CoreError> {
		Span::months(month, month)
	}

	fn years(first: i32, last: i32) -> Result<Span, CoreError> {
		Span::months(YearMonth::new(first, 1)?, YearMonth::new(last, 12)?)
	}

	/// "So far" and "to date" stop a period that holds today at today (AI-6.3).
	fn ending_today(self, to_date: bool, today: CivilDate) -> Span {
		if to_date && self.first <= today && today <= self.last {
			Span {
				last: today,
				..self
			}
		} else {
			self
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token {
	Month(u32),
	Year(i32),
	YearMonth(YearMonth),
	Date(CivilDate),
}

/// A month name as written, with the year written beside it, if any.
#[derive(Debug, Clone, Copy)]
struct Named {
	month: u32,
	year: Option<i32>,
}

/// v23, R-AI-1 (AI-6): the period the user's words name, against today.
#[uniffi::export]
pub fn resolve_period(text: String, today: String) -> Result<PeriodResolution, FfiError> {
	guard(|| resolution(&text, &today))
}

fn resolution(text: &str, today: &str) -> Result<PeriodResolution, CoreError> {
	let today = civil_date(today)?;
	if !YEARS.contains(&today.year()) {
		return Err(CoreError::Invalid(format!(
			"{today} is outside the years the assistant reads."
		)));
	}
	Ok(match span(text, today)? {
		Some(span) => PeriodResolution::Range {
			period: AssistantPeriod {
				first: span.first.to_string(),
				last: span.last.to_string(),
				label: label(span)?,
			},
		},
		None => PeriodResolution::Refused {
			hint: PERIOD_HINT.to_owned(),
		},
	})
}

/// `None` when the words name no period this reads.
fn span(text: &str, today: CivilDate) -> Result<Option<Span>, CoreError> {
	let text = normalized(text);
	let Some(tokens) = tokens(&text) else {
		return Ok(None);
	};
	let has = |phrases: &[&str]| phrases.iter().any(|phrase| text.contains(phrase));
	let to_date = has(TO_DATE);
	let this_month = today.year_month();

	let dates: Vec<CivilDate> = tokens
		.iter()
		.filter_map(|token| match token {
			Token::Date(date) => Some(*date),
			_ => None,
		})
		.collect();
	if !dates.is_empty() {
		let months = tokens
			.iter()
			.any(|token| matches!(token, Token::YearMonth(_)));
		return Ok(match dates[..] {
			_ if months => None,
			[day] => Some(Span {
				first: day,
				last: day,
			}),
			[first, last] if first <= last => Some(Span { first, last }),
			_ => None,
		});
	}

	let relative = if has(LAST_YEAR) {
		Some(this_month.year() - 1)
	} else if has(THIS_YEAR) {
		Some(this_month.year())
	} else {
		None
	};
	if tokens
		.iter()
		.any(|token| matches!(token, Token::Month(_) | Token::YearMonth(_)))
	{
		let Some(named) = named_months(&tokens) else {
			return Ok(None);
		};
		let Some((first, last)) = month_range(&named, relative, this_month)? else {
			return Ok(None);
		};
		return Ok(Some(
			Span::months(first, last)?.ending_today(to_date, today),
		));
	}

	let years: Vec<i32> = tokens
		.iter()
		.filter_map(|token| match token {
			Token::Year(year) => Some(*year),
			_ => None,
		})
		.collect();
	let year = match years[..] {
		[] => relative,
		[year] => Some(year),
		[first, last] if first <= last => {
			return Ok(Some(Span::years(first, last)?.ending_today(to_date, today)));
		}
		_ => return Ok(None),
	};
	let named_this_month = has(THIS_MONTH);
	if let Some(year) = year.filter(|_| named_this_month) {
		let month = YearMonth::new(year, this_month.month())?;
		return Ok(Some(Span::month(month)?.ending_today(to_date, today)));
	}
	if has(LAST_MONTH) {
		return Ok(Some(Span::month(this_month.offset(-1)?)?));
	}
	if let Some(year) = year {
		return Ok(Some(Span::years(year, year)?.ending_today(to_date, today)));
	}
	if text == "today" {
		return Ok(Some(Span {
			first: today,
			last: today,
		}));
	}
	if text.is_empty() || to_date || named_this_month || has(MONTH_TO_DATE) {
		return Ok(Some(Span::month(this_month)?.ending_today(to_date, today)));
	}
	Ok(None)
}

/// Case and accents folded, curly apostrophes made straight, runs of
/// whitespace made one space.
fn normalized(text: &str) -> String {
	fold(text)
		.replace('\u{2019}', "'")
		.split_whitespace()
		.collect::<Vec<_>>()
		.join(" ")
}

/// The month names, years, months and dates written, in order. `None` when
/// a run of digits and hyphens is no date, month or number this reads, or a
/// year is outside `YEARS`.
fn tokens(text: &str) -> Option<Vec<Token>> {
	let mut tokens = Vec::new();
	let mut rest = text;
	while let Some(start) = rest.find(|c: char| c.is_alphabetic() || c.is_ascii_digit()) {
		rest = &rest[start..];
		let letters = rest.starts_with(char::is_alphabetic);
		let end = rest
			.find(|c: char| {
				if letters {
					!c.is_alphabetic()
				} else {
					!(c.is_ascii_digit() || c == '-')
				}
			})
			.unwrap_or(rest.len());
		let run = &rest[..end];
		rest = &rest[end..];
		let token = if letters {
			month_number(run).map(Token::Month)
		} else {
			number(run.trim_end_matches('-'))?
		};
		tokens.extend(token);
	}
	Some(tokens)
}

/// A month by its English name or abbreviation.
fn month_number(word: &str) -> Option<u32> {
	let index = MONTH_NAMES.iter().position(|name| {
		let name = name.to_lowercase();
		word == name || word == &name[..3] || (name == "september" && word == "sept")
	})?;
	u32::try_from(index + 1).ok()
}

/// `Some(None)` for a number that is neither a year, a month nor a date,
/// which the words may hold ("last 3 months") and is left to the phrases.
fn number(run: &str) -> Option<Option<Token>> {
	let parts: Vec<&str> = run.split('-').collect();
	let digits = |part: &str, lengths: RangeInclusive<usize>| {
		lengths
			.contains(&part.len())
			.then(|| part.parse::<u32>().ok())
			.flatten()
	};
	let year = |part: &str| {
		digits(part, 4..=4)
			.and_then(|year| i32::try_from(year).ok())
			.filter(|year| YEARS.contains(year))
	};
	match parts[..] {
		[single] if single.len() == 4 => year(single).map(|year| Some(Token::Year(year))),
		[_] => Some(None),
		[y, m] => {
			let month = YearMonth::new(year(y)?, digits(m, 1..=2)?).ok()?;
			Some(Some(Token::YearMonth(month)))
		}
		[y, m, d] => {
			let date = CivilDate::new(year(y)?, digits(m, 1..=2)?, digits(d, 1..=2)?).ok()?;
			Some(Some(Token::Date(date)))
		}
		_ => None,
	}
}

/// The months written, each with its year. A year belongs to the month name
/// just before it ("November 2025 to January 2026"), or to the next one when
/// no month name comes before it. `None` when a year has no month to belong
/// to or a month would get two.
fn named_months(tokens: &[Token]) -> Option<Vec<Named>> {
	let mut named: Vec<Named> = Vec::new();
	let mut leading = None;
	for token in tokens {
		match *token {
			Token::Month(month) => {
				let year = if named.is_empty() {
					leading.take()
				} else {
					None
				};
				named.push(Named { month, year });
			}
			Token::YearMonth(month) if leading.is_none() => named.push(Named {
				month: month.month(),
				year: Some(month.year()),
			}),
			Token::Year(year) if leading.is_none() => match named.last_mut() {
				Some(last) if last.year.is_none() => last.year = Some(year),
				Some(_) => return None,
				None => leading = Some(year),
			},
			Token::Date(_) => {}
			Token::YearMonth(_) | Token::Year(_) => return None,
		}
	}
	Some(named)
}

/// The first and last months the named months span (AI-6.2, AI-6.3).
/// Written years always win. A month with no year is the latest of its name
/// not after the end, and the end with no year is the latest not after
/// today's month, or the earliest not before a start whose year is written.
/// `None` when the start falls after the end.
fn month_range(
	named: &[Named],
	relative: Option<i32>,
	this_month: YearMonth,
) -> Result<Option<(YearMonth, YearMonth)>, CoreError> {
	let (Some(start), Some(end)) = (named.first(), named.last()) else {
		return Ok(None);
	};
	let end_alone = |month: u32| match relative {
		Some(year) => YearMonth::new(year, month),
		None => latest(month, this_month),
	};
	let (first, last) = if named.len() == 1 {
		let month = match end.year {
			Some(year) => YearMonth::new(year, end.month)?,
			None => end_alone(end.month)?,
		};
		(month, month)
	} else {
		match (start.year, end.year) {
			(Some(first), Some(last)) => (
				YearMonth::new(first, start.month)?,
				YearMonth::new(last, end.month)?,
			),
			(Some(year), None) => {
				let first = YearMonth::new(year, start.month)?;
				(first, earliest(end.month, first)?)
			}
			(None, year) => {
				let last = match year {
					Some(year) => YearMonth::new(year, end.month)?,
					None => end_alone(end.month)?,
				};
				(latest(start.month, last)?, last)
			}
		}
	};
	Ok((first <= last).then_some((first, last)))
}

/// The latest month with this number that isn't after `limit` (AI-6.3).
fn latest(month: u32, limit: YearMonth) -> Result<YearMonth, CoreError> {
	let candidate = YearMonth::new(limit.year(), month)?;
	if candidate > limit {
		candidate.offset(-12)
	} else {
		Ok(candidate)
	}
}

/// The earliest month with this number that isn't before `limit`.
fn earliest(month: u32, limit: YearMonth) -> Result<YearMonth, CoreError> {
	let candidate = YearMonth::new(limit.year(), month)?;
	if candidate < limit {
		candidate.offset(12)
	} else {
		Ok(candidate)
	}
}

fn last_day(month: YearMonth) -> Result<CivilDate, CoreError> {
	month
		.offset(1)?
		.first_day()
		.naive()
		.pred_opt()
		.map(CivilDate::from)
		.ok_or(CoreError::Overflow)
}

/// AI-6.1: "November 2026", "2026", "May to October 2026", "November 2025 to
/// February 2026", "November 1 to 22, 2026", "January 1 to November 22,
/// 2026", "December 15, 2025 to January 3, 2026".
fn label(span: Span) -> Result<String, CoreError> {
	let (first, last) = (span.first, span.last);
	let (start, end) = (first.year_month(), last.year_month());
	if first == start.first_day() && last == last_day(end)? {
		return Ok(if start == end {
			month_year(start)
		} else if start.year() == end.year() && start.month() == 1 && end.month() == 12 {
			start.year().to_string()
		} else if start.year() == end.year() {
			format!("{} to {}", month_name(start), month_year(end))
		} else {
			format!("{} to {}", month_year(start), month_year(end))
		});
	}
	let day = |date: CivilDate| format!("{} {}", month_name(date.year_month()), date.day());
	Ok(if first == last {
		format!("{}, {}", day(first), first.year())
	} else if start == end {
		format!("{} to {}, {}", day(first), last.day(), last.year())
	} else if start.year() == end.year() {
		format!("{} to {}, {}", day(first), day(last), last.year())
	} else {
		format!(
			"{}, {} to {}, {}",
			day(first),
			first.year(),
			day(last),
			last.year()
		)
	})
}

fn month_name(month: YearMonth) -> &'static str {
	MONTH_NAMES
		.get(month.month() as usize - 1)
		.copied()
		.unwrap_or_default()
}

fn month_year(month: YearMonth) -> String {
	format!("{} {}", month_name(month), month.year())
}

#[cfg(test)]
mod tests {
	use super::*;

	const TODAY: &str = "2026-11-22";

	fn resolved(words: &str, today: &str) -> Option<(String, String, String)> {
		match resolve_period(words.to_owned(), today.to_owned()).expect(words) {
			PeriodResolution::Range { period } => Some((period.first, period.last, period.label)),
			PeriodResolution::Refused { hint } => {
				assert_eq!(hint, PERIOD_HINT);
				None
			}
		}
	}

	fn check(rows: &[(&str, &str, &str, &str)], today: &str) {
		for (words, first, last, label) in rows {
			let expected = ((*first).to_owned(), (*last).to_owned(), (*label).to_owned());
			assert_eq!(
				resolved(words, today),
				Some(expected),
				"{words:?} on {today}"
			);
		}
	}

	#[test]
	fn the_ai_6_2_table() {
		check(
			&[
				("", "2026-11-01", "2026-11-30", "November 2026"),
				("this month", "2026-11-01", "2026-11-30", "November 2026"),
				(
					"so far this month",
					"2026-11-01",
					"2026-11-22",
					"November 1 to 22, 2026",
				),
				(
					"this month so far",
					"2026-11-01",
					"2026-11-22",
					"November 1 to 22, 2026",
				),
				(
					"month to date",
					"2026-11-01",
					"2026-11-22",
					"November 1 to 22, 2026",
				),
				("last month", "2026-10-01", "2026-10-31", "October 2026"),
				("this year", "2026-01-01", "2026-12-31", "2026"),
				(
					"so far this year",
					"2026-01-01",
					"2026-11-22",
					"January 1 to November 22, 2026",
				),
				(
					"year to date",
					"2026-01-01",
					"2026-11-22",
					"January 1 to November 22, 2026",
				),
				("last year", "2025-01-01", "2025-12-31", "2025"),
				(
					"this month last year",
					"2025-11-01",
					"2025-11-30",
					"November 2025",
				),
				(
					"same month last year",
					"2025-11-01",
					"2025-11-30",
					"November 2025",
				),
				("October", "2026-10-01", "2026-10-31", "October 2026"),
				("October 2026", "2026-10-01", "2026-10-31", "October 2026"),
				("2026-10", "2026-10-01", "2026-10-31", "October 2026"),
				("February 2024", "2024-02-01", "2024-02-29", "February 2024"),
				("2026", "2026-01-01", "2026-12-31", "2026"),
				(
					"2026-10-01 to 2026-10-31",
					"2026-10-01",
					"2026-10-31",
					"October 2026",
				),
				(
					"2025-12-15 to 2026-01-03",
					"2025-12-15",
					"2026-01-03",
					"December 15, 2025 to January 3, 2026",
				),
			],
			TODAY,
		);
		check(
			&[("last month", "2025-12-01", "2025-12-31", "December 2025")],
			"2026-01-15",
		);
		assert_eq!(resolved("2026-11-10 to 2026-11-05", TODAY), None);
	}

	#[test]
	fn a_month_named_alone_is_the_latest_not_after_todays_month() {
		check(
			&[("October", "2026-10-01", "2026-10-31", "October 2026")],
			TODAY,
		);
		check(
			&[("October", "2025-10-01", "2025-10-31", "October 2025")],
			"2026-09-05",
		);
		check(
			&[("October", "2026-10-01", "2026-10-31", "October 2026")],
			"2026-10-05",
		);
		check(
			&[(
				"october so far",
				"2026-10-01",
				"2026-10-05",
				"October 1 to 5, 2026",
			)],
			"2026-10-05",
		);
	}

	#[test]
	fn the_words_the_app_resolved_before() {
		check(
			&[
				(
					"Month to date",
					"2026-11-01",
					"2026-11-22",
					"November 1 to 22, 2026",
				),
				(
					"so far",
					"2026-11-01",
					"2026-11-22",
					"November 1 to 22, 2026",
				),
				(
					"this month this year",
					"2026-11-01",
					"2026-11-30",
					"November 2026",
				),
				("december", "2025-12-01", "2025-12-31", "December 2025"),
				("Oct 2026", "2026-10-01", "2026-10-31", "October 2026"),
				(
					"in October of last year",
					"2025-10-01",
					"2025-10-31",
					"October 2025",
				),
				("2025", "2025-01-01", "2025-12-31", "2025"),
				(
					"2026-10-01 to 2026-10-15",
					"2026-10-01",
					"2026-10-15",
					"October 1 to 15, 2026",
				),
				(
					"May to October 2026",
					"2026-05-01",
					"2026-10-31",
					"May to October 2026",
				),
				(
					"November to February",
					"2025-11-01",
					"2026-02-28",
					"November 2025 to February 2026",
				),
				("today", "2026-11-22", "2026-11-22", "November 22, 2026"),
				(
					"  OCTOBER\t 2026 ",
					"2026-10-01",
					"2026-10-31",
					"October 2026",
				),
				("sept 2026", "2026-09-01", "2026-09-30", "September 2026"),
			],
			TODAY,
		);
	}

	#[test]
	fn written_years_win_on_either_end_of_a_range() {
		let span = ("2025-11-01", "2026-01-31", "November 2025 to January 2026");
		for words in [
			"November 2025 to January 2026",
			"from Nov 2025 through Jan 2026",
			"2025-11 to 2026-01",
			"November 2025 to January",
			"November to January 2026",
			"November to January",
			"2025 November to January",
		] {
			check(&[(words, span.0, span.1, span.2)], TODAY);
		}
		check(
			&[(
				"November 2024 to January 2026",
				"2024-11-01",
				"2026-01-31",
				"November 2024 to January 2026",
			)],
			TODAY,
		);
		check(
			&[(
				"March 2026 to May 2026",
				"2026-03-01",
				"2026-05-31",
				"March to May 2026",
			)],
			TODAY,
		);
	}

	#[test]
	fn a_range_with_no_years_is_the_latest_span_ending_by_todays_month() {
		let winter =
			|today| resolved("December to February", today).map(|(first, last, _)| (first, last));
		let span = |first: &str, last: &str| Some((first.to_owned(), last.to_owned()));
		assert_eq!(winter(TODAY), span("2025-12-01", "2026-02-28"));
		assert_eq!(winter("2026-02-10"), span("2025-12-01", "2026-02-28"));
		assert_eq!(winter("2026-01-10"), span("2024-12-01", "2025-02-28"));
		check(
			&[(
				"December to February so far",
				"2025-12-01",
				"2026-02-10",
				"December 1, 2025 to February 10, 2026",
			)],
			"2026-02-10",
		);
	}

	#[test]
	fn a_start_with_a_year_and_an_end_without_runs_forward_from_the_start() {
		check(
			&[(
				"November 2026 to January",
				"2026-11-01",
				"2027-01-31",
				"November 2026 to January 2027",
			)],
			TODAY,
		);
		check(
			&[(
				"October 2025 to October",
				"2025-10-01",
				"2025-10-31",
				"October 2025",
			)],
			TODAY,
		);
	}

	#[test]
	fn years_written_alone() {
		check(
			&[
				(
					"2024 to 2025",
					"2024-01-01",
					"2025-12-31",
					"January 2024 to December 2025",
				),
				(
					"2026 so far",
					"2026-01-01",
					"2026-11-22",
					"January 1 to November 22, 2026",
				),
			],
			TODAY,
		);
	}

	#[test]
	fn words_it_cant_read_get_the_hint() {
		let long_nines = "9".repeat(4_000);
		let long_months = "2026-".repeat(1_000);
		for words in [
			"2026-13",
			"2026-02-30",
			"1899",
			"next fortnight",
			"last 3 months",
			"x-y",
			"2026-11-10 to 2026-11-05",
			"January 2026 to November 2025",
			"2026-01 to 2025-11",
			"2025 to 2024",
			"October 1899",
			"November 2025 2026",
			"2024 2025 November",
			"2026-10-01 to 2026-11",
			"2026-10-01 to 2026-10-15 to 2026-10-20",
			"-999-01",
			"0000-01",
			"99999-01",
			"2026-00",
			"1899-12",
			"2201-01",
			"-999-01-01",
			"2026-10-99999999999999999999",
			long_nines.as_str(),
			long_months.as_str(),
		] {
			assert_eq!(
				resolved(words, TODAY),
				None,
				"{:?}",
				&words[..words.len().min(40)]
			);
		}
	}

	#[test]
	fn a_today_that_isnt_a_date_is_an_error_not_a_hint() {
		for today in ["", "2026-02-30", "1899-12-31", "today"] {
			assert!(
				resolve_period("this month".to_owned(), today.to_owned()).is_err(),
				"{today:?}"
			);
		}
	}
}
