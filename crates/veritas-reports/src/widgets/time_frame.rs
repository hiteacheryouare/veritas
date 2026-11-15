//! `calculateTimeRange` (`components/reports/reportRanges.ts:250-345`): the
//! start and end a card reads from its `timeFrame`, as `yyyy-MM` months or
//! `yyyy-MM-dd` days.

use chrono::{Datelike, Months, NaiveDate};
use veritas_domain::{CivilDate, CoreError, YearMonth};

use super::spending::{SpendingMode, compared_months};
use crate::json::Json;
use crate::report_query::{day, parse_date};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimeRange {
	pub start: String,
	pub end: String,
	pub mode: String,
}

fn month(date: NaiveDate) -> String {
	date.format("%Y-%m").to_string()
}

fn sub_months(date: NaiveDate, n: i64) -> NaiveDate {
	let shifted = if n >= 0 {
		date.checked_sub_months(Months::new(u32::try_from(n).unwrap_or(u32::MAX)))
	} else {
		date.checked_add_months(Months::new(u32::try_from(-n).unwrap_or(u32::MAX)))
	};
	shifted.unwrap_or(date)
}

fn calendar_months(later: NaiveDate, earlier: NaiveDate) -> i64 {
	(i64::from(later.year()) - i64::from(earlier.year())) * 12 + i64::from(later.month())
		- i64::from(earlier.month())
}

fn is_day(text: &str) -> bool {
	text.len() == 10 && chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").is_ok()
}

fn quarter_start(date: NaiveDate) -> NaiveDate {
	NaiveDate::from_ymd_opt(date.year(), (date.month0() / 3) * 3 + 1, 1).unwrap_or(date)
}

/// `latest` is the newest transaction's date (`get-latest-transaction`).
/// `default` is the card's fallback `{start, end, mode}`.
pub fn calculate_time_range(
	frame: Option<&Json>,
	default: Option<(&str, &str, &str)>,
	latest: Option<NaiveDate>,
	today: NaiveDate,
) -> TimeRange {
	let pick = |key: &str, fallback: Option<&str>| {
		frame
			.and_then(|frame| frame.get(key))
			.and_then(Json::as_str)
			.or(fallback)
			.map(str::to_owned)
	};
	let current = month(today);
	let start = pick("start", default.map(|d| d.0)).unwrap_or_else(|| month(sub_months(today, 5)));
	let end = pick("end", default.map(|d| d.1)).unwrap_or_else(|| current.clone());
	let mode = pick("mode", default.map(|d| d.2)).unwrap_or_else(|| "sliding-window".to_owned());
	let range = |start: String, end: String, mode: &str| TimeRange {
		start,
		end,
		mode: mode.to_owned(),
	};
	let this_month = today.with_day(1).unwrap_or(today);
	match mode.as_str() {
		"full" => {
			let end = match latest {
				Some(latest) if latest.with_day(1).unwrap_or(latest) > this_month => month(latest),
				_ => current,
			};
			range(start, end, "full")
		}
		"sliding-window" => {
			if is_day(&start) && is_day(&end) {
				let (Some(first), Some(last)) = (parse_date(&start), parse_date(&end)) else {
					return range(start, end, "sliding-window");
				};
				let offset = (last - first).num_days().max(0);
				let from = today
					.checked_sub_days(chrono::Days::new(offset.unsigned_abs()))
					.unwrap_or(today);
				return range(day(from), day(today), "sliding-window");
			}
			let (Some(first), Some(last)) = (parse_date(&start), parse_date(&end)) else {
				return range(start, end, "sliding-window");
			};
			let offset = calendar_months(last, first);
			if start > end {
				return range(current, month(sub_months(today, -offset)), "sliding-window");
			}
			range(month(sub_months(today, offset)), current, "sliding-window")
		}
		"lastMonth" => {
			let last = month(sub_months(today, 1));
			range(last.clone(), last, "lastMonth")
		}
		"lastYear" => {
			let year = today.year() - 1;
			range(format!("{year:04}-01"), format!("{year:04}-12"), "lastYear")
		}
		"yearToDate" => range(format!("{:04}-01", today.year()), current, "yearToDate"),
		"priorYearToDate" => {
			let year_ago = sub_months(today, 12);
			range(
				format!("{:04}-01", year_ago.year()),
				day(year_ago),
				"priorYearToDate",
			)
		}
		"currentQuarter" => {
			let first = quarter_start(today);
			range(month(first), month(sub_months(first, -2)), "currentQuarter")
		}
		"previousQuarter" => {
			let first = quarter_start(sub_months(today, 3));
			range(
				month(first),
				month(sub_months(first, -2)),
				"previousQuarter",
			)
		}
		_ => range(start, end, "static"),
	}
}

/// What a card shows when it has no `timeFrame` of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultFrame {
	/// The last six months, this one included. Net worth, Sankey and most cards.
	SixMonths,
	/// This month only. The cash flow card.
	ThisMonth,
}

fn month_of(text: &str) -> Result<YearMonth, CoreError> {
	text.get(..7)
		.map(YearMonth::parse)
		.unwrap_or_else(|| Err(CoreError::Invalid(format!("a card month {text:?}"))))
}

/// `calculate_time_range` as whole months. `latest` is the newest
/// transaction's date, which a `full` range runs to when it is later than
/// this month.
pub fn widget_months(
	meta: Option<&Json>,
	default: DefaultFrame,
	today: CivilDate,
	latest: Option<CivilDate>,
) -> Result<(YearMonth, YearMonth), CoreError> {
	let current = today.year_month().to_string();
	let fallback = match default {
		DefaultFrame::SixMonths => None,
		DefaultFrame::ThisMonth => Some((current.as_str(), current.as_str(), "sliding-window")),
	};
	let range = calculate_time_range(
		meta.and_then(|meta| meta.get("timeFrame")),
		fallback,
		latest.map(CivilDate::naive),
		today.naive(),
	);
	Ok((month_of(&range.start)?, month_of(&range.end)?))
}

/// The two months a single-month spending card compares
/// (`calculateSpendingReportTimeRange`). `None` for the budget and average
/// modes.
pub fn spending_months(
	meta: Option<&Json>,
	today: CivilDate,
) -> Result<Option<(YearMonth, YearMonth)>, CoreError> {
	if meta
		.and_then(|meta| meta.get("mode"))
		.and_then(Json::as_str)
		.is_some_and(|mode| mode != "single-month")
	{
		return Ok(None);
	}
	compared_months(meta, SpendingMode::SingleMonth, today.naive()).map(Some)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn today() -> CivilDate {
		CivilDate::new(2026, 11, 13).unwrap()
	}

	fn month(text: &str) -> YearMonth {
		YearMonth::parse(text).unwrap()
	}

	fn months(meta: &str, default: DefaultFrame) -> (String, String) {
		let meta = Json::parse(meta).unwrap();
		let (first, last) = widget_months(Some(&meta), default, today(), None).unwrap();
		(first.to_string(), last.to_string())
	}

	#[test]
	fn a_card_without_a_time_frame_uses_its_default() {
		assert_eq!(
			widget_months(None, DefaultFrame::SixMonths, today(), None).unwrap(),
			(month("2026-06"), month("2026-11"))
		);
		assert_eq!(
			widget_months(None, DefaultFrame::ThisMonth, today(), None).unwrap(),
			(month("2026-11"), month("2026-11"))
		);
	}

	#[test]
	fn a_sliding_window_keeps_its_width_and_ends_this_month() {
		let meta = r#"{"timeFrame":{"start":"2025-01","end":"2025-03","mode":"sliding-window"}}"#;
		assert_eq!(
			months(meta, DefaultFrame::SixMonths),
			("2026-09".to_owned(), "2026-11".to_owned())
		);
	}

	#[test]
	fn static_and_full_ranges_keep_their_start() {
		let fixed = r#"{"timeFrame":{"start":"2025-01","end":"2025-03","mode":"static"}}"#;
		assert_eq!(
			months(fixed, DefaultFrame::SixMonths),
			("2025-01".to_owned(), "2025-03".to_owned())
		);
		let meta =
			Json::parse(r#"{"timeFrame":{"start":"2024-05","end":"2024-06","mode":"full"}}"#)
				.unwrap();
		let later = CivilDate::new(2027, 2, 1).unwrap();
		assert_eq!(
			widget_months(Some(&meta), DefaultFrame::SixMonths, today(), Some(later)).unwrap(),
			(month("2024-05"), month("2027-02"))
		);
	}

	#[test]
	fn a_live_spending_card_compares_this_month_with_the_one_before() {
		let spending = |meta: &str| {
			let meta = Json::parse(meta).unwrap();
			spending_months(Some(&meta), today())
				.unwrap()
				.map(|(a, b)| (a.to_string(), b.to_string()))
		};
		let both = |a: &str, b: &str| Some((a.to_owned(), b.to_owned()));
		assert_eq!(spending("{}"), both("2026-11", "2026-10"));
		assert_eq!(
			spending(r#"{"compare":"2025-04","compareTo":"2024-04"}"#),
			both("2026-11", "2025-11")
		);
		assert_eq!(
			spending(r#"{"compare":"2025-04","compareTo":"2024-04","isLive":false}"#),
			both("2025-04", "2024-04")
		);
		assert_eq!(spending(r#"{"mode":"average"}"#), None);
	}

	#[test]
	fn named_ranges_follow_the_calendar() {
		let named = |mode: &str| {
			months(
				&format!(r#"{{"timeFrame":{{"mode":"{mode}"}}}}"#),
				DefaultFrame::SixMonths,
			)
		};
		assert_eq!(
			named("lastMonth"),
			("2026-10".to_owned(), "2026-10".to_owned())
		);
		assert_eq!(
			named("lastYear"),
			("2025-01".to_owned(), "2025-12".to_owned())
		);
		assert_eq!(
			named("yearToDate"),
			("2026-01".to_owned(), "2026-11".to_owned())
		);
		assert_eq!(
			named("priorYearToDate"),
			("2025-01".to_owned(), "2025-11".to_owned())
		);
		assert_eq!(
			named("currentQuarter"),
			("2026-10".to_owned(), "2026-12".to_owned())
		);
		assert_eq!(
			named("previousQuarter"),
			("2026-07".to_owned(), "2026-09".to_owned())
		);
	}
}
