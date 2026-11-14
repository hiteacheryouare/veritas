//! `calculateTimeRange` (`components/reports/reportRanges.ts:250-345`): the
//! start and end a card reads from its `timeFrame`, as `yyyy-MM` months or
//! `yyyy-MM-dd` days.

use chrono::{Datelike, Months, NaiveDate};

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
