//! The months a dashboard card covers, read from its `timeFrame` setting the
//! way Actual's `calculateTimeRange` reads it (`reports/reportRanges.ts`).

use veritas_domain::{CivilDate, CoreError, YearMonth};

use crate::json::Json;

/// What a card shows when it has no `timeFrame` of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultFrame {
	/// The last six months, this one included. Net worth, Sankey and most cards.
	SixMonths,
	/// This month only. The cash flow card.
	ThisMonth,
}

/// The first and last month of a card's range. `today` is the local date and
/// `latest` the newest transaction's, which a `full` range runs to when it is
/// later than this month.
pub fn widget_months(
	meta: Option<&Json>,
	default: DefaultFrame,
	today: CivilDate,
	latest: Option<CivilDate>,
) -> Result<(YearMonth, YearMonth), CoreError> {
	let current = today.year_month();
	let frame = meta.and_then(|meta| meta.get("timeFrame"));
	let text = |key: &str| {
		frame
			.and_then(|frame| frame.get(key))
			.and_then(Json::as_str)
	};
	let month = |key: &str| {
		text(key)
			.and_then(|value| value.get(..7))
			.map(YearMonth::parse)
	};
	let start = match month("start") {
		Some(parsed) => parsed?,
		None => match default {
			DefaultFrame::SixMonths => current.offset(-5)?,
			DefaultFrame::ThisMonth => current,
		},
	};
	let end = match month("end") {
		Some(parsed) => parsed?,
		None => current,
	};
	let year_start = |year: i32| YearMonth::new(year, 1);
	let quarter_start =
		|month: YearMonth| YearMonth::new(month.year(), (month.month() - 1) / 3 * 3 + 1);
	Ok(match text("mode").unwrap_or("sliding-window") {
		"full" => (
			start,
			latest.map_or(current, |date| date.year_month().max(current)),
		),
		"static" => (start, end),
		"lastMonth" => {
			let last = current.offset(-1)?;
			(last, last)
		}
		"lastYear" => (
			year_start(current.year() - 1)?,
			YearMonth::new(current.year() - 1, 12)?,
		),
		"yearToDate" => (year_start(current.year())?, current),
		"priorYearToDate" => (year_start(current.year() - 1)?, current.offset(-12)?),
		"currentQuarter" => {
			let first = quarter_start(current)?;
			(first, first.offset(2)?)
		}
		"previousQuarter" => {
			let first = quarter_start(current.offset(-3)?)?;
			(first, first.offset(2)?)
		}
		_ => {
			let width = months_between(start, end);
			if width < 0 {
				(current.offset(width)?, current)
			} else {
				(current.offset(-width)?, current)
			}
		}
	})
}

/// The two months a spending card compares, as Actual's
/// `calculateSpendingReportTimeRange` picks them: by default this month
/// against the one before. `None` for the budget and average modes, which
/// Veritas doesn't draw yet.
pub fn spending_months(
	meta: Option<&Json>,
	today: CivilDate,
) -> Result<Option<(YearMonth, YearMonth)>, CoreError> {
	let field = |key: &str| meta.and_then(|meta| meta.get(key));
	let month = |key: &str| {
		field(key)
			.and_then(Json::as_str)
			.and_then(|value| value.get(..7))
			.map(YearMonth::parse)
			.transpose()
	};
	if field("mode")
		.and_then(Json::as_str)
		.is_some_and(|mode| mode != "single-month")
	{
		return Ok(None);
	}
	let current = today.year_month();
	let compare = month("compare")?.unwrap_or(current);
	let compare_to = match month("compareTo")? {
		Some(month) => month,
		None => compare.offset(-1)?,
	};
	if field("isLive").and_then(Json::as_bool).unwrap_or(true) {
		Ok(Some((
			current,
			current.offset(months_between(compare, compare_to))?,
		)))
	} else {
		Ok(Some((compare, compare_to)))
	}
}

fn months_between(start: YearMonth, end: YearMonth) -> i32 {
	(end.year() - start.year()) * 12 + end.month() as i32 - start.month() as i32
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
