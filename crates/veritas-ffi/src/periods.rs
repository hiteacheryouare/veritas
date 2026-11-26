//! The assistant's period resolution (R-AI-1, docs/finance/assistant.md
//! AI-6): the user's words and today's date to the exact dates a tool reads
//! and the label that states them, so neither the model nor the app does date
//! arithmetic.
//!
//! The words become tokens (`raw_tokens`), the AI-6.11 phrases become period
//! tokens (`phrases`), and `interpret` reads those as one period, a range
//! with a connector, or an open end. Anything else is refused.

use std::ops::RangeInclusive;

use veritas_domain::{CivilDate, CoreError, YearMonth};

use crate::budget::civil_date;
use crate::names::fold;
use crate::{FfiError, guard};

pub(crate) const PERIOD_HINT: &str = "Ask again with a period such as this month, so far this month, last month, this year, October 2026, 2026, or 2026-10-01 to 2026-10-15.";
pub(crate) const BALANCE_DATE_HINT: &str =
	"Ask again with today or an earlier date, such as today or 2026-10-31.";
pub(crate) const TAX_YEAR_HINT: &str = "Ask again with a calendar year such as 2026.";

/// The years a period may name or reach (AI-6.7).
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

const CONNECTORS: &[&str] = &["to", "through", "thru", "until", "till", "-"];
const NOW: &[&str] = &["now", "today", "present"];
const INTRODUCERS: &[&str] = &["in", "for", "during"];
/// Punctuation that only separates words. Any other symbol is unread.
const SEPARATORS: &[char] = &['.', ',', ';', ':', '!', '?', '\'', '"', '(', ')'];

/// The AI-6.11 phrases, longest first where one starts another.
const PHRASES: &[(&[&str], Tok)] = &[
	(&["month", "to", "date"], Tok::MonthToDate),
	(&["year", "to", "date"], Tok::YearToDate),
	(&["this", "month"], Tok::ThisMonth),
	(&["current", "month"], Tok::ThisMonth),
	(&["same", "month"], Tok::ThisMonth),
	(&["last", "month"], Tok::LastMonth),
	(&["previous", "month"], Tok::LastMonth),
	(&["this", "year"], Tok::ThisYear),
	(&["current", "year"], Tok::ThisYear),
	(&["last", "year"], Tok::LastYear),
	(&["previous", "year"], Tok::LastYear),
	(&["so", "far"], Tok::SoFar),
	(&["to", "date"], Tok::SoFar),
	(&["first", "quarter"], Tok::Quarter(1)),
	(&["second", "quarter"], Tok::Quarter(2)),
	(&["third", "quarter"], Tok::Quarter(3)),
	(&["fourth", "quarter"], Tok::Quarter(4)),
	(&["first", "half"], Tok::Half(1)),
	(&["second", "half"], Tok::Half(2)),
	(&["q1"], Tok::Quarter(1)),
	(&["q2"], Tok::Quarter(2)),
	(&["q3"], Tok::Quarter(3)),
	(&["q4"], Tok::Quarter(4)),
	(&["h1"], Tok::Half(1)),
	(&["h2"], Tok::Half(2)),
	(&["mtd"], Tok::MonthToDate),
	(&["ytd"], Tok::YearToDate),
	(&["since"], Tok::Since),
	(&["today"], Tok::Today),
	(&["from"], Tok::From),
	(&["of"], Tok::Of),
	(&["the"], Tok::The),
];

/// The dates a tool reads, inclusive, and the label that states them (AI-6.1).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AssistantPeriod {
	/// `YYYY-MM-DD`.
	pub first: String,
	/// `YYYY-MM-DD`, on or after `first`.
	pub last: String,
	pub label: String,
	/// `first` is after today, so a tool that reads recorded rows adds
	/// AI-6.16's line.
	pub not_started: bool,
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

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum TaxYearResolution {
	Year { year: i32 },
	Refused { hint: String },
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

	fn day(day: CivilDate) -> Span {
		Span {
			first: day,
			last: day,
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tok {
	Month(u32),
	Year(i32),
	YearMonth(YearMonth),
	Date(CivilDate),
	Quarter(u32),
	Half(u32),
	ThisMonth,
	LastMonth,
	ThisYear,
	LastYear,
	MonthToDate,
	YearToDate,
	SoFar,
	/// A connector and `now`, `today` or `present`: the range runs to today
	/// (AI-6.12).
	OpenEnd,
	Since,
	Today,
	Connector,
	From,
	Of,
	The,
}

/// One end of a range, or the whole period when there is no connector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum End {
	Name {
		month: u32,
		year: Option<i32>,
	},
	Month(YearMonth),
	Year(i32),
	Day(CivilDate),
	/// The first month of a quarter or half, and its length in months.
	Part {
		first: u32,
		months: u32,
		year: Option<i32>,
	},
	ThisMonth,
	LastMonth,
}

enum Raw {
	Word(String),
	Value(Tok),
}

/// v23, R-AI-1 (AI-6): the period the user's words name, against today.
#[uniffi::export]
pub fn resolve_period(text: String, today: String) -> Result<PeriodResolution, FfiError> {
	guard(|| {
		let today = checked_today(&today)?;
		match span(&text, today)? {
			Some(span) => range(span, today),
			None => Ok(refused(PERIOD_HINT)),
		}
	})
}

/// v23 (AI-6.14): the one day the balance sheet reads. A single date after
/// today is refused; a period ending after today is read as of today.
#[uniffi::export]
pub fn resolve_balance_sheet_date(
	text: String,
	today: String,
) -> Result<PeriodResolution, FfiError> {
	guard(|| {
		let today = checked_today(&today)?;
		let day = span(&text, today)?.and_then(|span| {
			if span.first == span.last {
				(span.last <= today).then_some(span.last)
			} else {
				Some(span.last.min(today))
			}
		});
		match day {
			Some(day) => range(Span::day(day), today),
			None => Ok(refused(BALANCE_DATE_HINT)),
		}
	})
}

/// v23 (AI-6.15): the one whole calendar year the tax summary reads, never
/// one that has not started.
#[uniffi::export]
pub fn resolve_tax_year(text: String, today: String) -> Result<TaxYearResolution, FfiError> {
	guard(|| {
		let today = checked_today(&today)?;
		let year = match span(&text, today)? {
			Some(span) if span.first <= today => {
				let year = span.first.year();
				(Span::years(year, year)? == span).then_some(year)
			}
			_ => None,
		};
		Ok(match year {
			Some(year) => TaxYearResolution::Year { year },
			None => TaxYearResolution::Refused {
				hint: TAX_YEAR_HINT.to_owned(),
			},
		})
	})
}

/// AI-6.9: a `today` that isn't a date in `YEARS` is the app's bug.
fn checked_today(today: &str) -> Result<CivilDate, CoreError> {
	let today = civil_date(today)?;
	if YEARS.contains(&today.year()) {
		Ok(today)
	} else {
		Err(CoreError::Invalid(format!(
			"{today} is outside the years the assistant reads."
		)))
	}
}

fn range(span: Span, today: CivilDate) -> Result<PeriodResolution, CoreError> {
	Ok(PeriodResolution::Range {
		period: AssistantPeriod {
			first: span.first.to_string(),
			last: span.last.to_string(),
			label: label(span)?,
			not_started: span.first > today,
		},
	})
}

fn refused(hint: &str) -> PeriodResolution {
	PeriodResolution::Refused {
		hint: hint.to_owned(),
	}
}

/// `None` when the words name no period this reads, or one outside `YEARS`
/// (AI-6.7).
fn span(text: &str, today: CivilDate) -> Result<Option<Span>, CoreError> {
	let Some(tokens) = raw_tokens(&normalized(text)).and_then(|raw| phrases(&raw)) else {
		return Ok(None);
	};
	let within =
		|span: &Span| YEARS.contains(&span.first.year()) && YEARS.contains(&span.last.year());
	Ok(interpret(&tokens, today)?.filter(within))
}

/// Case and accents folded, curly apostrophes made straight, runs of
/// whitespace made one space (AI-6.5).
fn normalized(text: &str) -> String {
	fold(text)
		.replace('\u{2019}', "'")
		.split_whitespace()
		.collect::<Vec<_>>()
		.join(" ")
}

/// Words, the numbers and dates written, and hyphens and dashes as `-`.
/// `None` for a symbol, or a run of digits that is no year, month or date in
/// `YEARS`, which includes a day number (AI-6.8).
fn raw_tokens(text: &str) -> Option<Vec<Raw>> {
	let mut raw = Vec::new();
	let mut rest = text;
	while let Some(c) = rest.chars().next() {
		let end = if c.is_whitespace() || SEPARATORS.contains(&c) {
			c.len_utf8()
		} else if matches!(c, '-' | '\u{2013}' | '\u{2014}') {
			raw.push(Raw::Word("-".to_owned()));
			c.len_utf8()
		} else if c.is_alphabetic() {
			let end = rest
				.find(|c: char| !c.is_alphanumeric())
				.unwrap_or(rest.len());
			raw.push(Raw::Word(rest[..end].to_owned()));
			end
		} else if c.is_ascii_digit() {
			let end = digit_run_end(rest);
			raw.extend(numbers(&rest[..end])?.into_iter().map(Raw::Value));
			end
		} else {
			return None;
		};
		rest = &rest[end..];
	}
	Some(raw)
}

/// Digits, and hyphens with a digit after them.
fn digit_run_end(text: &str) -> usize {
	let bytes = text.as_bytes();
	let mut end = 0;
	while let Some(&byte) = bytes.get(end) {
		let joins = byte == b'-' && bytes.get(end + 1).is_some_and(u8::is_ascii_digit);
		if !(byte.is_ascii_digit() || joins) {
			break;
		}
		end += 1;
	}
	end
}

/// A run such as `2026-10-01`, `2025-11` or `2024-2025`: dates, months and
/// years, with a connector at each hyphen between two of them.
fn numbers(run: &str) -> Option<Vec<Tok>> {
	let groups: Vec<&str> = run.split('-').collect();
	let year = |part: &str| {
		(part.len() == 4)
			.then(|| part.parse::<i32>().ok())
			.flatten()
			.filter(|year| YEARS.contains(year))
	};
	let short = |part: &str| (1..=2).contains(&part.len());
	let number = |part: &str| part.parse::<u32>().ok();
	let mut tokens = Vec::new();
	let mut index = 0;
	while index < groups.len() {
		if index > 0 {
			tokens.push(Tok::Connector);
		}
		match groups[index..] {
			[y, m, d, ..] if y.len() == 4 && short(m) && short(d) => {
				let date = CivilDate::new(year(y)?, number(m)?, number(d)?).ok()?;
				tokens.push(Tok::Date(date));
				index += 3;
			}
			[y, m, ..] if y.len() == 4 && short(m) => {
				tokens.push(Tok::YearMonth(YearMonth::new(year(y)?, number(m)?).ok()?));
				index += 2;
			}
			[y, ..] => {
				tokens.push(Tok::Year(year(y)?));
				index += 1;
			}
			[] => return None,
		}
	}
	Some(tokens)
}

/// The AI-6.11 words as period tokens. `None` for a word outside it.
fn phrases(raw: &[Raw]) -> Option<Vec<Tok>> {
	let mut tokens = Vec::new();
	let mut index = 0;
	while let Some(item) = raw.get(index) {
		let (token, used) = match item {
			Raw::Value(token) => (Some(*token), 1),
			Raw::Word(_) => phrase(&raw[index..])?,
		};
		tokens.extend(token);
		index += used;
	}
	Some(tokens)
}

/// The token the words at the start of `raw` make and how many items they
/// take. An introducer makes no token.
fn phrase(raw: &[Raw]) -> Option<(Option<Tok>, usize)> {
	let word = |index: usize| match raw.get(index) {
		Some(Raw::Word(word)) => word.as_str(),
		_ => "",
	};
	let open_end = |start: usize| {
		let mut index = start;
		if !CONNECTORS.contains(&word(index)) {
			return None;
		}
		index += 1;
		if word(index) == "the" && word(index + 1) == "present" {
			index += 1;
		}
		NOW.contains(&word(index)).then_some(index + 1)
	};
	if word(0) == "up" {
		return (word(1) == "to")
			.then(|| open_end(1))
			.flatten()
			.map(|used| (Some(Tok::OpenEnd), used));
	}
	if let Some(used) = open_end(0) {
		return Some((Some(Tok::OpenEnd), used));
	}
	for (words, token) in PHRASES {
		if let Some(used) = matched(raw, words) {
			return Some((Some(*token), used));
		}
	}
	if CONNECTORS.contains(&word(0)) {
		return Some((Some(Tok::Connector), 1));
	}
	if INTRODUCERS.contains(&word(0)) {
		return Some((None, 1));
	}
	month_number(word(0)).map(|month| (Some(Tok::Month(month)), 1))
}

/// How many items `words` takes at the start of `raw`. A hyphen between two
/// of its words reads as a space ("year-to-date", AI-6.11).
fn matched(raw: &[Raw], words: &[&str]) -> Option<usize> {
	let mut index = 0;
	for (position, wanted) in words.iter().enumerate() {
		if position > 0 && matches!(raw.get(index), Some(Raw::Word(word)) if word == "-") {
			index += 1;
		}
		match raw.get(index) {
			Some(Raw::Word(word)) if word == wanted => index += 1,
			_ => return None,
		}
	}
	Some(index)
}

/// A month by its English name or abbreviation (AI-6.5).
fn month_number(word: &str) -> Option<u32> {
	let index = MONTH_NAMES.iter().position(|name| {
		let name = name.to_lowercase();
		word == name || *word == name[..3] || (name == "september" && word == "sept")
	})?;
	u32::try_from(index + 1).ok()
}

/// The period the tokens name (AI-6.2, AI-6.6, AI-6.7, AI-6.12, AI-6.13).
fn interpret(tokens: &[Tok], today: CivilDate) -> Result<Option<Span>, CoreError> {
	let Some(tokens) = placed(tokens) else {
		return Ok(None);
	};
	let this_month = today.year_month();
	let mut so_far = false;
	let mut open = false;
	let mut since = false;
	let mut relative = None;
	let mut rest = Vec::new();
	for (index, token) in tokens.iter().enumerate() {
		let year = match token {
			Tok::ThisYear | Tok::YearToDate => Some(this_month.year()),
			Tok::LastYear => Some(this_month.year() - 1),
			_ => None,
		};
		if year.is_some() {
			if relative.is_some() {
				return Ok(None);
			}
			relative = year;
		}
		match token {
			Tok::SoFar | Tok::YearToDate => so_far = true,
			Tok::MonthToDate => {
				so_far = true;
				rest.push(Tok::ThisMonth);
			}
			Tok::ThisYear | Tok::LastYear => {}
			Tok::Since if index == 0 => since = true,
			Tok::OpenEnd if index + 1 == tokens.len() => open = true,
			Tok::Since | Tok::OpenEnd => return Ok(None),
			other => rest.push(*other),
		}
	}
	let parts: Vec<&[Tok]> = rest.split(|token| *token == Tok::Connector).collect();
	let span = match parts[..] {
		[[]] if since => None,
		[[]] => match relative {
			Some(year) => Some(Span::years(year, year)?),
			None => Some(Span::month(this_month)?),
		},
		[one] => match end(one, true, today) {
			Some(end) => single(end, relative, this_month)?,
			None => None,
		},
		[first, last] if !(since || open) => {
			match (end(first, true, today), end(last, false, today)) {
				(Some(first), Some(last)) => between(first, last, relative, this_month)?,
				_ => None,
			}
		}
		_ => None,
	};
	let Some(span) = span else {
		return Ok(None);
	};
	if since || open {
		return Ok((span.first <= today).then_some(Span {
			first: span.first,
			last: today,
		}));
	}
	if so_far {
		if span.first > today {
			return Ok(None);
		}
		return Ok(Some(Span {
			last: span.last.min(today),
			..span
		}));
	}
	Ok(Some(span))
}

/// The tokens with `from`, `of` and `the` checked against their places
/// (AI-6.11) and taken out. `of` joins a month name, quarter or half to the
/// year after it, so the two stay beside each other.
fn placed(tokens: &[Tok]) -> Option<Vec<Tok>> {
	let mut kept = Vec::new();
	for (index, token) in tokens.iter().enumerate() {
		let before = index.checked_sub(1).and_then(|before| tokens.get(before));
		let after = tokens.get(index + 1);
		let fits = match token {
			Tok::From => {
				index == 0
					&& tokens
						.iter()
						.any(|token| matches!(token, Tok::Connector | Tok::OpenEnd))
			}
			Tok::Of => {
				matches!(before, Some(Tok::Month(_) | Tok::Quarter(_) | Tok::Half(_)))
					&& matches!(after, Some(Tok::Year(_) | Tok::ThisYear | Tok::LastYear))
			}
			Tok::The => matches!(after, Some(Tok::Quarter(_) | Tok::Half(_))),
			_ => {
				kept.push(*token);
				continue;
			}
		};
		if !fits {
			return None;
		}
	}
	Some(kept)
}

/// One end, or the whole period. A year belongs to the month name, quarter
/// or half directly beside it, before it only on the first end (AI-6.6).
fn end(tokens: &[Tok], first: bool, today: CivilDate) -> Option<End> {
	let part = |token: Tok| match token {
		Tok::Quarter(number) => Some((3 * number - 2, 3)),
		Tok::Half(number) => Some((6 * number - 5, 6)),
		_ => None,
	};
	Some(match *tokens {
		[Tok::Month(month)] => End::Name { month, year: None },
		[Tok::Month(month), Tok::Year(year)] => End::Name {
			month,
			year: Some(year),
		},
		[Tok::Year(year), Tok::Month(month)] if first => End::Name {
			month,
			year: Some(year),
		},
		[Tok::YearMonth(month)] => End::Month(month),
		[Tok::Year(year)] => End::Year(year),
		[Tok::Date(day)] => End::Day(day),
		[Tok::Today] => End::Day(today),
		[Tok::ThisMonth] => End::ThisMonth,
		[Tok::LastMonth] => End::LastMonth,
		[token] => {
			let (first, months) = part(token)?;
			End::Part {
				first,
				months,
				year: None,
			}
		}
		[token, Tok::Year(year)] => {
			let (first, months) = part(token)?;
			End::Part {
				first,
				months,
				year: Some(year),
			}
		}
		[Tok::Year(year), token] if first => {
			let (first, months) = part(token)?;
			End::Part {
				first,
				months,
				year: Some(year),
			}
		}
		_ => return None,
	})
}

/// A period with no connector. `relative` is "this year" or "last year",
/// which only a month name, a quarter, a half or this month take (AI-6.6).
fn single(
	end: End,
	relative: Option<i32>,
	this_month: YearMonth,
) -> Result<Option<Span>, CoreError> {
	let span = match (end, relative) {
		(End::Name { month, year }, relative) => {
			let month = match (year, relative) {
				(Some(_), Some(_)) => return Ok(None),
				(Some(year), None) | (None, Some(year)) => YearMonth::new(year, month)?,
				(None, None) => latest(month, this_month)?,
			};
			Span::month(month)?
		}
		(
			End::Part {
				first,
				months,
				year,
			},
			relative,
		) => {
			let start = match (year, relative) {
				(Some(_), Some(_)) => return Ok(None),
				(Some(year), None) | (None, Some(year)) => YearMonth::new(year, first)?,
				(None, None) => latest(first, this_month)?,
			};
			let months = i32::try_from(months).map_err(|_| CoreError::Overflow)?;
			Span::months(start, start.offset(months - 1)?)?
		}
		(End::ThisMonth, Some(year)) => Span::month(YearMonth::new(year, this_month.month())?)?,
		(End::ThisMonth, None) => Span::month(this_month)?,
		(_, Some(_)) => return Ok(None),
		(End::LastMonth, None) => Span::month(this_month.offset(-1)?)?,
		(End::Month(month), None) => Span::month(month)?,
		(End::Year(year), None) => Span::years(year, year)?,
		(End::Day(day), None) => Span::day(day),
	};
	Ok(Some(span))
}

/// A range with a connector: two dates, two years, or two months, each a
/// month name or a `YYYY-MM` (AI-6.6, AI-6.7).
fn between(
	first: End,
	last: End,
	relative: Option<i32>,
	this_month: YearMonth,
) -> Result<Option<Span>, CoreError> {
	let month = |end: End| match end {
		End::Name { month, year } => Some((month, year)),
		End::Month(month) => Some((month.month(), Some(month.year()))),
		_ => None,
	};
	let span = match (first, last) {
		(End::Day(first), End::Day(last)) if relative.is_none() => Span { first, last },
		(End::Year(first), End::Year(last)) if relative.is_none() && first <= last => {
			Span::years(first, last)?
		}
		_ => match (month(first), month(last)) {
			(Some(start), Some(end)) => match month_range(start, end, relative, this_month)? {
				Some((first, last)) => Span::months(first, last)?,
				None => return Ok(None),
			},
			_ => return Ok(None),
		},
	};
	Ok((span.first <= span.last).then_some(span))
}

/// The first and last months of a range of two months (AI-6.6). Written
/// years always win. An end with no year is the latest of its name not after
/// today's month, or, after a start with a written year, the first on or
/// after it; a start with no year is the latest not after the end.
fn month_range(
	(start, start_year): (u32, Option<i32>),
	(end, end_year): (u32, Option<i32>),
	relative: Option<i32>,
	this_month: YearMonth,
) -> Result<Option<(YearMonth, YearMonth)>, CoreError> {
	if relative.is_some() && (start_year.is_some() || end_year.is_some()) {
		return Ok(None);
	}
	let (first, last) = match (start_year, end_year) {
		(Some(first), Some(last)) => (YearMonth::new(first, start)?, YearMonth::new(last, end)?),
		(Some(year), None) => {
			let first = YearMonth::new(year, start)?;
			(first, earliest(end, first)?)
		}
		(None, year) => {
			let last = match year.or(relative) {
				Some(year) => YearMonth::new(year, end)?,
				None => latest(end, this_month)?,
			};
			(latest(start, last)?, last)
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

	fn reads(rows: &[(&str, &str, &str, &str)], today: &str) {
		for (words, first, last, label) in rows {
			let expected = ((*first).to_owned(), (*last).to_owned(), (*label).to_owned());
			assert_eq!(
				resolved(words, today),
				Some(expected),
				"{words:?} on {today}"
			);
		}
	}

	fn refuses(words: &[&str], today: &str) {
		for words in words {
			let shown = &words[..words.len().min(40)];
			assert_eq!(resolved(words, today), None, "{shown:?} on {today}");
		}
	}

	#[test]
	fn the_hint_is_pinned() {
		assert_eq!(
			PERIOD_HINT,
			"Ask again with a period such as this month, so far this month, last month, this year, October 2026, 2026, or 2026-10-01 to 2026-10-15."
		);
		assert_eq!(
			BALANCE_DATE_HINT,
			"Ask again with today or an earlier date, such as today or 2026-10-31."
		);
		assert_eq!(
			TAX_YEAR_HINT,
			"Ask again with a calendar year such as 2026."
		);
	}

	#[test]
	fn ai_6_2() {
		reads(
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
		reads(
			&[("last month", "2025-12-01", "2025-12-31", "December 2025")],
			"2026-01-15",
		);
		refuses(&["2026-11-10 to 2026-11-05"], TODAY);
	}

	#[test]
	fn ai_6_3() {
		reads(
			&[("October", "2025-10-01", "2025-10-31", "October 2025")],
			"2026-09-05",
		);
		reads(
			&[("October", "2026-10-01", "2026-10-31", "October 2026")],
			"2026-10-05",
		);
		reads(
			&[(
				"October so far",
				"2026-10-01",
				"2026-10-05",
				"October 1 to 5, 2026",
			)],
			"2026-10-05",
		);
	}

	#[test]
	fn ai_6_10() {
		reads(
			&[
				(
					"November 2025 to January 2026",
					"2025-11-01",
					"2026-01-31",
					"November 2025 to January 2026",
				),
				(
					"November 2025 to January",
					"2025-11-01",
					"2026-01-31",
					"November 2025 to January 2026",
				),
				(
					"November to January 2026",
					"2025-11-01",
					"2026-01-31",
					"November 2025 to January 2026",
				),
				(
					"November to January",
					"2025-11-01",
					"2026-01-31",
					"November 2025 to January 2026",
				),
				(
					"November 2024 to January 2026",
					"2024-11-01",
					"2026-01-31",
					"November 2024 to January 2026",
				),
				(
					"November 2026 to January",
					"2026-11-01",
					"2027-01-31",
					"November 2026 to January 2027",
				),
				(
					"November 2026 to January so far",
					"2026-11-01",
					"2026-11-22",
					"November 1 to 22, 2026",
				),
				(
					"October 2025 to October",
					"2025-10-01",
					"2025-10-31",
					"October 2025",
				),
				(
					"September to November",
					"2026-09-01",
					"2026-11-30",
					"September to November 2026",
				),
				(
					"December to February",
					"2025-12-01",
					"2026-02-28",
					"December 2025 to February 2026",
				),
				(
					"March to May this year",
					"2026-03-01",
					"2026-05-31",
					"March to May 2026",
				),
				(
					"2024 to 2025",
					"2024-01-01",
					"2025-12-31",
					"January 2024 to December 2025",
				),
				("2025 November", "2025-11-01", "2025-11-30", "November 2025"),
				(
					"2025 November to January",
					"2025-11-01",
					"2026-01-31",
					"November 2025 to January 2026",
				),
			],
			TODAY,
		);
		reads(
			&[(
				"December to February",
				"2024-12-01",
				"2025-02-28",
				"December 2024 to February 2025",
			)],
			"2026-01-10",
		);
		refuses(
			&[
				"2025 to 2024",
				"November 2025 2026",
				"October 1899",
				"2201-01",
				"2026-10-01 to 2026-10-15 to 2026-10-20",
				"2026-10 to 2026-10-15",
				"October 2026-10-15",
				"January, March and June",
				"October 15",
				"October 1 to 15",
				"mars",
				"Mai",
			],
			TODAY,
		);
		assert!(resolve_period("this month".to_owned(), "2026-02-30".to_owned()).is_err());
	}

	#[test]
	fn ai_6_11_to_6_13_examples() {
		reads(
			&[
				(
					"since October",
					"2026-10-01",
					"2026-11-22",
					"October 1 to November 22, 2026",
				),
				(
					"October to now",
					"2026-10-01",
					"2026-11-22",
					"October 1 to November 22, 2026",
				),
				(
					"October through today",
					"2026-10-01",
					"2026-11-22",
					"October 1 to November 22, 2026",
				),
				(
					"since October 2025",
					"2025-10-01",
					"2026-11-22",
					"October 1, 2025 to November 22, 2026",
				),
				(
					"since 2026-10-15",
					"2026-10-15",
					"2026-11-22",
					"October 15 to November 22, 2026",
				),
				(
					"since December",
					"2025-12-01",
					"2026-11-22",
					"December 1, 2025 to November 22, 2026",
				),
				("October so far", "2026-10-01", "2026-10-31", "October 2026"),
				(
					"October to date",
					"2026-10-01",
					"2026-10-31",
					"October 2026",
				),
				(
					"Q4 2025",
					"2025-10-01",
					"2025-12-31",
					"October to December 2025",
				),
				("Q4", "2026-10-01", "2026-12-31", "October to December 2026"),
				(
					"first half of 2026",
					"2026-01-01",
					"2026-06-30",
					"January to June 2026",
				),
				(
					"second half",
					"2026-07-01",
					"2026-12-31",
					"July to December 2026",
				),
				("may", "2026-05-01", "2026-05-31", "May 2026"),
				(
					"October - December",
					"2025-10-01",
					"2025-12-31",
					"October to December 2025",
				),
				(
					"October through December",
					"2025-10-01",
					"2025-12-31",
					"October to December 2025",
				),
				(
					"October to December this year",
					"2026-10-01",
					"2026-12-31",
					"October to December 2026",
				),
				(
					"year-to-date",
					"2026-01-01",
					"2026-11-22",
					"January 1 to November 22, 2026",
				),
				(
					"month-to-date",
					"2026-11-01",
					"2026-11-22",
					"November 1 to 22, 2026",
				),
				(
					"since Q3",
					"2026-07-01",
					"2026-11-22",
					"July 1 to November 22, 2026",
				),
				(
					"since 2025 November",
					"2025-11-01",
					"2026-11-22",
					"November 1, 2025 to November 22, 2026",
				),
				("in October", "2026-10-01", "2026-10-31", "October 2026"),
				("during October", "2026-10-01", "2026-10-31", "October 2026"),
				("for 2025", "2025-01-01", "2025-12-31", "2025"),
				("previous month", "2026-10-01", "2026-10-31", "October 2026"),
				("current year", "2026-01-01", "2026-12-31", "2026"),
				("current month", "2026-11-01", "2026-11-30", "November 2026"),
				(
					"October of last year",
					"2025-10-01",
					"2025-10-31",
					"October 2025",
				),
				(
					"the first half of 2026",
					"2026-01-01",
					"2026-06-30",
					"January to June 2026",
				),
				(
					"up to now",
					"2026-11-01",
					"2026-11-22",
					"November 1 to 22, 2026",
				),
				(
					"2025-11 to 2026-01",
					"2025-11-01",
					"2026-01-31",
					"November 2025 to January 2026",
				),
				("March 2027", "2027-03-01", "2027-03-31", "March 2027"),
			],
			TODAY,
		);
		reads(
			&[("Q4", "2025-10-01", "2025-12-31", "October to December 2025")],
			"2026-03-10",
		);
		refuses(
			&[
				"since 2027",
				"Q1 to Q3 2026",
				"Q4 and March",
				"Novembre 2025",
				"may I see this year",
				"January and March",
				"2026-10-01 to October 15",
				"October and November",
				"October versus November",
				"October November",
				"2024 2025",
				"past month",
				"past year",
				"the past year",
				"from October",
				"November to 2026",
				"2025 to November",
				"November 2025 to 2026",
				"March 2027 so far",
				"Q1 2027 so far",
				"October this month",
				"last month in October",
				"this month last month",
				"this year last year",
			],
			TODAY,
		);
	}

	#[test]
	fn a_period_that_has_not_started_says_so() {
		let started =
			|words: &str| match resolve_period(words.to_owned(), TODAY.to_owned()).expect(words) {
				PeriodResolution::Range { period } => !period.not_started,
				PeriodResolution::Refused { .. } => panic!("{words:?} was refused"),
			};
		assert!(!started("March 2027"));
		assert!(started("this month"));
		assert!(started("this year"));
		assert!(started("November 2026 to January"));
	}

	#[test]
	fn the_balance_sheet_reads_one_day_never_after_today() {
		let day = |words: &str| match resolve_balance_sheet_date(words.to_owned(), TODAY.to_owned())
			.expect(words)
		{
			PeriodResolution::Range { period } => {
				assert_eq!(period.first, period.last, "{words:?}");
				Some((period.last, period.label))
			}
			PeriodResolution::Refused { hint } => {
				assert_eq!(hint, BALANCE_DATE_HINT);
				None
			}
		};
		let on = |date: &str, label: &str| Some((date.to_owned(), label.to_owned()));
		assert_eq!(day("today"), on("2026-11-22", "November 22, 2026"));
		assert_eq!(day("2026-10-31"), on("2026-10-31", "October 31, 2026"));
		assert_eq!(day("this month"), on("2026-11-22", "November 22, 2026"));
		assert_eq!(day("last month"), on("2026-10-31", "October 31, 2026"));
		assert_eq!(day("2026-12-31"), None);
		assert_eq!(day("next fortnight"), None);
	}

	#[test]
	fn the_tax_summary_reads_one_whole_year_that_has_started() {
		let year = |words: &str| match resolve_tax_year(words.to_owned(), TODAY.to_owned())
			.expect(words)
		{
			TaxYearResolution::Year { year } => Some(year),
			TaxYearResolution::Refused { hint } => {
				assert_eq!(hint, TAX_YEAR_HINT);
				None
			}
		};
		assert_eq!(year("last year"), Some(2025));
		assert_eq!(year("2025"), Some(2025));
		assert_eq!(year("this year"), Some(2026));
		for words in [
			"2024 to 2025",
			"so far this year",
			"year to date",
			"October 2026",
			"Q4 2025",
			"2026-10-01",
			"2027",
			"",
		] {
			assert_eq!(year(words), None, "{words:?}");
		}
	}

	#[test]
	fn the_words_the_app_resolved_before() {
		reads(
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
				(
					"from Nov 2025 through Jan 2026",
					"2025-11-01",
					"2026-01-31",
					"November 2025 to January 2026",
				),
				(
					"2024-2025",
					"2024-01-01",
					"2025-12-31",
					"January 2024 to December 2025",
				),
			],
			TODAY,
		);
		reads(
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
	fn hostile_words_get_the_hint() {
		let nines = "9".repeat(4_000);
		let months = "2026-".repeat(1_000);
		refuses(
			&[
				"2026-13",
				"2026-02-30",
				"1899",
				"next fortnight",
				"last 3 months",
				"x-y",
				"-999-01",
				"0000-01",
				"99999-01",
				"2026-00",
				"1899-12",
				"-999-01-01",
				"2026-10-99999999999999999999",
				"October & November",
				"up",
				"since",
				nines.as_str(),
				months.as_str(),
			],
			TODAY,
		);
		refuses(&["last year"], "1900-06-01");
		refuses(&["last month", "December"], "1900-01-15");
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
