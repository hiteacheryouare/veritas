//! Schedule dates the way Actual computes them: next date, the dates after a
//! given day, the upcoming list, status and the window in which a posted
//! transaction counts as paid (reference/actual/packages/loot-core/src/shared/schedules.ts).

use chrono::{Datelike, Days, Months, NaiveDate, Weekday};
use serde_json::Value;
use veritas_domain::{CivilDate, CoreError};

use crate::condition::DateValue;
use crate::js;
use crate::recur::{RecurError, WeekendSolveMode};

fn previous_day(date: NaiveDate) -> NaiveDate {
	date.pred_opt().unwrap_or(date)
}

fn next_day(date: NaiveDate) -> NaiveDate {
	date.succ_opt().unwrap_or(date)
}

/// `getDateWithSkippedWeekend`: Saturday and Sunday go to the next Monday
/// (`after`) or the previous Friday (`before`).
fn skip_weekend(date: NaiveDate, mode: &WeekendSolveMode) -> Result<NaiveDate, RecurError> {
	let (to_monday, to_friday) = match date.weekday() {
		Weekday::Sat => (2, 1),
		Weekday::Sun => (1, 2),
		_ => return Ok(date),
	};
	let moved = match mode {
		WeekendSolveMode::After => date.checked_add_days(Days::new(to_monday)),
		WeekendSolveMode::Before => date.checked_sub_days(Days::new(to_friday)),
		_ => return Err(RecurError::WeekendSolveMode),
	};
	Ok(moved.unwrap_or(date))
}

/// `getNextDate(cond, start, noSkipWeekend)`: an exact date is returned as it
/// is; a recurrence gives its first date on or after `start`, or its last date
/// once it has ended, then moves off the weekend.
fn next_date_from_start(
	date: &DateValue,
	start: NaiveDate,
	no_skip_weekend: bool,
) -> Result<Option<NaiveDate>, RecurError> {
	match date {
		DateValue::Day(day) => Ok(Some(*day)),
		DateValue::Month(_) | DateValue::Year(_) => Ok(None),
		DateValue::Recur(recurrence) => {
			let found = match recurrence.first_on_or_after(start)? {
				Some(date) => Some(date),
				None => recurrence.last()?,
			};
			match found {
				Some(date) if recurrence.skip_weekend && !no_skip_weekend => {
					skip_weekend(date, &recurrence.weekend_solve_mode).map(Some)
				}
				other => Ok(other),
			}
		}
	}
}

pub fn next_date(date: &DateValue, start: CivilDate) -> Result<Option<CivilDate>, RecurError> {
	Ok(next_date_from_start(date, start.naive(), false)?.map(CivilDate::from))
}

/// `getNextDateAfter`: the first weekend-adjusted date strictly after `after`.
pub fn next_date_after(
	date: &DateValue,
	after: CivilDate,
) -> Result<Option<CivilDate>, RecurError> {
	let after = after.naive();
	let mut start = previous_day(after);
	for _ in 0..7 {
		let occurrence = match next_date_from_start(date, start, true)? {
			Some(occurrence) if occurrence >= start => occurrence,
			_ => return Ok(None),
		};
		if let Some(next) = next_date_from_start(date, start, false)?
			&& next > after
		{
			return Ok(Some(next.into()));
		}
		start = next_day(occurrence);
	}
	Ok(None)
}

/// `getNextDateFrom` (`server/schedules/app.ts:216-224`), what Actual stores
/// as `next_date` when a schedule is created or reset: the next date from
/// today, unless an earlier occurrence was moved after the weekend onto today
/// or later.
pub fn next_date_from(date: &DateValue, today: CivilDate) -> Result<Option<CivilDate>, RecurError> {
	let next = next_date(date, today)?;
	let moved_onto_today = next_date_after(date, previous_day(today.naive()).into())?;
	Ok(match (moved_onto_today, next) {
		(Some(moved), None) => Some(moved),
		(Some(moved), Some(next)) if moved < next => Some(moved),
		(_, next) => next,
	})
}

/// `getScheduleDates` (`server/schedules/occurrences.ts`): the dates from
/// `start` on, each found with `next_date_after` so weekend moves and repeats
/// come out exactly as the app shows them.
pub fn schedule_dates(
	date: &DateValue,
	start: CivilDate,
	end: Option<CivilDate>,
	count: Option<usize>,
) -> Result<Vec<CivilDate>, RecurError> {
	if count == Some(0) {
		return Ok(Vec::new());
	}
	let within = |day: CivilDate| day >= start && end.is_none_or(|end| day <= end);
	let recurrence = match date {
		DateValue::Day(day) => {
			let day = CivilDate::from(*day);
			return Ok(if within(day) { vec![day] } else { Vec::new() });
		}
		DateValue::Month(_) | DateValue::Year(_) => return Ok(Vec::new()),
		DateValue::Recur(recurrence) => recurrence,
	};
	if end.is_none() && count.is_none() && recurrence.open_ended {
		return Err(RecurError::Unbounded);
	}
	if recurrence.skip_weekend && recurrence.weekend_solve_mode == WeekendSolveMode::Missing {
		return Err(RecurError::WeekendSolveMode);
	}
	let mut dates = Vec::new();
	let mut next = next_date_after(date, previous_day(start.naive()).into())?;
	while let Some(day) = next.filter(|day| end.is_none_or(|end| *day <= end)) {
		dates.push(day);
		if count.is_some_and(|count| dates.len() >= count) {
			break;
		}
		next = next_date_after(date, day)?;
	}
	Ok(dates)
}

/// The checks Actual's API makes before it expands a schedule date
/// (`validateScheduleDate`, `server/api.ts:132-159`). The app itself is more
/// lenient, but a bill Veritas saves should pass these so it can't hold a
/// date Actual would read differently (R-BILL-4).
pub fn validate_schedule_date(value: &Value) -> Result<(), String> {
	let day = |name: &str, value: Option<&Value>| match value.and_then(Value::as_str) {
		Some(text) if js::strict_date(text).is_some() => Ok(()),
		_ => Err(format!("invalid {name}, use YYYY-MM-DD")),
	};
	let config = match value {
		Value::String(_) => return day("schedule date", Some(value)),
		Value::Object(config) => config,
		_ => return Err("date must be a YYYY-MM-DD string or a RecurConfig".into()),
	};
	day("recurrence start date", config.get("start"))?;
	if let Some(interval) = config.get("interval").filter(|value| !value.is_null())
		&& !js::integer(interval).is_some_and(|n| n > 0)
	{
		return Err("interval must be a positive integer".into());
	}
	match config.get("endMode").and_then(Value::as_str) {
		Some("on_date") => day("recurrence end date", config.get("endDate"))?,
		Some("after_n_occurrences") => {
			let count = config.get("endOccurrences").and_then(js::integer);
			if !count.is_some_and(|n| n > 0) {
				return Err("endOccurrences must be a positive integer".into());
			}
		}
		_ => {}
	}
	Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleStatus {
	Completed,
	Paid,
	Due,
	Upcoming,
	Missed,
	Scheduled,
}

/// `getStatus`. A schedule without a next date is only ever scheduled, paid
/// or completed, since JS compares `null` with a date string as false.
pub fn status(
	next_date: Option<CivilDate>,
	completed: bool,
	has_transaction: bool,
	upcoming_days: i64,
	today: CivilDate,
) -> ScheduleStatus {
	if completed {
		return ScheduleStatus::Completed;
	}
	if has_transaction {
		return ScheduleStatus::Paid;
	}
	let Some(next) = next_date else {
		return ScheduleStatus::Scheduled;
	};
	let horizon = today
		.naive()
		.checked_add_days(Days::new(upcoming_days.max(0).unsigned_abs()))
		.unwrap_or(today.naive());
	if next == today {
		ScheduleStatus::Due
	} else if next > today && next.naive() <= horizon {
		ScheduleStatus::Upcoming
	} else if next < today {
		ScheduleStatus::Missed
	} else {
		ScheduleStatus::Scheduled
	}
}

/// `getUpcomingDays`: how many days ahead count as upcoming for a preset
/// (`1`, `7`, `14`, `oneMonth`, `currentMonth`) or a custom `N-day|week|month|year`
/// (`custom_upcoming_length`). Text JS would turn into `NaN` is `Invalid`, and a
/// length too large to count is `Overflow` (R-REL-7).
pub fn upcoming_days(length: &str, today: CivilDate) -> Result<i64, CoreError> {
	let invalid = || CoreError::Invalid(format!("{length:?} isn't an upcoming length"));
	let first_of_month = today.naive().with_day(1).ok_or_else(invalid)?;
	let days_until = |months: i64| {
		let months = u32::try_from(months).map_err(|_| CoreError::Overflow)?;
		first_of_month
			.checked_add_months(Months::new(months))
			.map(|future| (future - first_of_month).num_days())
			.ok_or(CoreError::Overflow)
	};
	match length {
		"currentMonth" => {
			let next_month = first_of_month
				.checked_add_months(Months::new(1))
				.ok_or(CoreError::Overflow)?;
			let last_day = next_month.pred_opt().ok_or(CoreError::Overflow)?.day();
			Ok(i64::from(last_day) - i64::from(today.day()))
		}
		"oneMonth" => days_until(1),
		_ if length.contains('-') => {
			// `const [num, unit] = length.split('-')`: only the first two parts count.
			let mut parts = length.split('-');
			let number = parts.next().unwrap_or_default();
			let unit = parts.next().unwrap_or_default();
			let value = || Ok::<i64, CoreError>(parse_int(number)?.ok_or_else(invalid)?.max(1));
			match unit {
				"day" => value(),
				"week" => value()?.checked_mul(7).ok_or(CoreError::Overflow),
				"month" => days_until(value()?)?
					.checked_add(1)
					.ok_or(CoreError::Overflow),
				"year" => days_until(value()?.checked_mul(12).ok_or(CoreError::Overflow)?)?
					.checked_add(1)
					.ok_or(CoreError::Overflow),
				_ => Ok(7),
			}
		}
		_ => parse_int(length)?.ok_or_else(invalid),
	}
}

/// `parseInt(text, 10)`: an optional sign and the leading digits. `None` is
/// `NaN`; digits past `i64` are `Overflow` rather than JS's lost precision.
fn parse_int(text: &str) -> Result<Option<i64>, CoreError> {
	let text = text.trim_start_matches(js::is_js_whitespace);
	let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
	let end = unsigned
		.find(|c: char| !c.is_ascii_digit())
		.unwrap_or(unsigned.len());
	if end == 0 {
		return Ok(None);
	}
	let sign = text.len() - unsigned.len();
	text[..sign + end]
		.parse()
		.map(Some)
		.map_err(|_| CoreError::Overflow)
}

/// `getScheduleOccurrenceMatchStartDate`: a transaction linked to the
/// schedule and dated on or after this day marks the occurrence paid. There
/// is no upper bound. `date_op_is` is whether the first date condition's op is `is`.
pub fn paid_window_start(
	date_op_is: bool,
	posts_transaction: bool,
	occurrence: CivilDate,
) -> CivilDate {
	if date_op_is || posts_transaction {
		return occurrence;
	}
	occurrence
		.naive()
		.checked_sub_days(Days::new(2))
		.map_or(occurrence, CivilDate::from)
}

#[cfg(test)]
mod tests {
	use serde_json::json;

	use super::*;
	use crate::condition::{Op, parse_date_value};

	fn day(text: &str) -> CivilDate {
		CivilDate::from(NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap())
	}

	fn recur(config: Value) -> DateValue {
		parse_date_value(Op::IsApprox, Some(&config)).unwrap()
	}

	fn strings(dates: Vec<CivilDate>) -> Vec<String> {
		dates.into_iter().map(|d| d.to_string()).collect()
	}

	// The next two tests port reference/actual/packages/loot-core/src/shared/schedules.test.ts:467-612.

	#[test]
	fn get_next_date_returns_the_last_date_once_ended() {
		let ended = recur(
			json!({"start": "2016-01-25", "frequency": "monthly", "endMode": "on_date", "endDate": "2016-08-25"}),
		);
		assert_eq!(
			next_date(&ended, day("2017-01-01")).unwrap(),
			Some(day("2016-08-25"))
		);
		let backwards = recur(
			json!({"start": "2016-03-25", "frequency": "monthly", "endMode": "on_date", "endDate": "2016-01-25"}),
		);
		assert_eq!(next_date(&backwards, day("2017-01-01")).unwrap(), None);
	}

	#[test]
	fn get_next_date_after() {
		let saturday = |extra: Value| {
			let mut config = json!({"start": "2020-12-05", "frequency": "weekly", "patterns": []});
			config
				.as_object_mut()
				.unwrap()
				.extend(extra.as_object().unwrap().clone());
			recur(config)
		};
		let after = |date: &DateValue, text: &str| {
			next_date_after(date, day(text))
				.unwrap()
				.map(|d| d.to_string())
		};
		assert_eq!(
			after(&saturday(json!({})), "2020-12-05").as_deref(),
			Some("2020-12-12")
		);
		let moved_after = saturday(json!({"skipWeekend": true, "weekendSolveMode": "after"}));
		assert_eq!(
			after(&moved_after, "2020-12-07").as_deref(),
			Some("2020-12-14")
		);
		assert_eq!(
			after(&moved_after, "2020-12-13").as_deref(),
			Some("2020-12-14")
		);
		let sunday = recur(
			json!({"start": "2020-12-06", "frequency": "weekly", "patterns": [], "skipWeekend": true, "weekendSolveMode": "after"}),
		);
		assert_eq!(after(&sunday, "2020-12-07").as_deref(), Some("2020-12-14"));
		let moved_before = saturday(json!({"skipWeekend": true, "weekendSolveMode": "before"}));
		assert_eq!(
			after(&moved_before, "2020-12-04").as_deref(),
			Some("2020-12-11")
		);
		let daily = recur(
			json!({"start": "2020-12-04", "frequency": "daily", "patterns": [], "skipWeekend": true, "weekendSolveMode": "before"}),
		);
		assert_eq!(after(&daily, "2020-12-04").as_deref(), Some("2020-12-07"));
		let walk = recur(
			json!({"start": "2026-08-24", "frequency": "daily", "patterns": [], "skipWeekend": true, "weekendSolveMode": "before"}),
		);
		assert_eq!(after(&walk, "2026-08-28").as_deref(), Some("2026-08-31"));
		let twice = saturday(json!({"endMode": "after_n_occurrences", "endOccurrences": 2}));
		assert_eq!(after(&twice, "2020-12-05").as_deref(), Some("2020-12-12"));
		assert_eq!(after(&twice, "2020-12-12"), None);

		let once = parse_date_value(Op::Is, Some(&json!("2024-08-10"))).unwrap();
		assert_eq!(after(&once, "2024-08-01").as_deref(), Some("2024-08-10"));
		assert_eq!(after(&once, "2024-08-10"), None);
	}

	#[test]
	fn an_ended_schedule_has_no_upcoming_dates() {
		let date = recur(
			json!({"start": "2026-01-31", "frequency": "monthly", "endMode": "on_date", "endDate": "2026-10-31"}),
		);
		assert_eq!(
			next_date_from(&date, day("2026-11-09")).unwrap(),
			Some(day("2026-10-31"))
		);
		assert!(
			schedule_dates(&date, day("2026-11-09"), None, Some(12))
				.unwrap()
				.is_empty()
		);
	}

	#[test]
	fn weekend_moves_before_and_after() {
		// Saturday 2026-11-07.
		let after = recur(
			json!({"start": "2026-11-07", "frequency": "daily", "skipWeekend": true, "weekendSolveMode": "after"}),
		);
		assert_eq!(
			next_date_from(&after, day("2026-11-09")).unwrap(),
			Some(day("2026-11-09"))
		);
		assert_eq!(
			strings(schedule_dates(&after, day("2026-11-09"), None, Some(6)).unwrap()),
			[
				"2026-11-09",
				"2026-11-10",
				"2026-11-11",
				"2026-11-12",
				"2026-11-13",
				"2026-11-16"
			]
		);
		let before = recur(
			json!({"start": "2026-11-07", "frequency": "weekly", "interval": 2, "skipWeekend": true, "weekendSolveMode": "before"}),
		);
		assert_eq!(
			next_date_from(&before, day("2026-11-09")).unwrap(),
			Some(day("2026-11-20"))
		);
	}

	#[test]
	fn next_date_from_catches_a_date_moved_onto_today() {
		// The 2024-06-01 occurrence is a Saturday moved after the weekend to Monday the 3rd.
		let date = recur(
			json!({"start": "2024-01-01", "frequency": "monthly", "skipWeekend": true, "weekendSolveMode": "after"}),
		);
		assert_eq!(
			next_date(&date, day("2024-06-03")).unwrap(),
			Some(day("2024-07-01"))
		);
		assert_eq!(
			next_date_from(&date, day("2024-06-03")).unwrap(),
			Some(day("2024-06-03"))
		);
	}

	#[test]
	fn open_ended_lists_need_a_bound_and_weekend_moves_need_a_mode() {
		let open = recur(json!({"start": "2024-01-01", "frequency": "daily"}));
		assert_eq!(
			schedule_dates(&open, day("2024-01-01"), None, None),
			Err(RecurError::Unbounded)
		);
		let modeless =
			recur(json!({"start": "2024-01-01", "frequency": "daily", "skipWeekend": true}));
		assert_eq!(
			schedule_dates(&modeless, day("2024-01-01"), None, Some(3)),
			Err(RecurError::WeekendSolveMode)
		);
	}

	#[test]
	fn status_follows_get_status() {
		let today = day("2026-11-09");
		assert_eq!(
			status(Some(today), true, true, 7, today),
			ScheduleStatus::Completed
		);
		assert_eq!(
			status(Some(today), false, true, 7, today),
			ScheduleStatus::Paid
		);
		assert_eq!(
			status(Some(today), false, false, 7, today),
			ScheduleStatus::Due
		);
		assert_eq!(
			status(Some(day("2026-11-16")), false, false, 7, today),
			ScheduleStatus::Upcoming
		);
		assert_eq!(
			status(Some(day("2026-11-17")), false, false, 7, today),
			ScheduleStatus::Scheduled
		);
		assert_eq!(
			status(Some(day("2026-11-08")), false, false, 7, today),
			ScheduleStatus::Missed
		);
		assert_eq!(
			status(None, false, false, 7, today),
			ScheduleStatus::Scheduled
		);
	}

	#[test]
	fn upcoming_lengths() {
		let today = day("2026-11-09");
		assert_eq!(upcoming_days("7", today).unwrap(), 7);
		assert_eq!(upcoming_days("currentMonth", today).unwrap(), 21);
		assert_eq!(upcoming_days("oneMonth", today).unwrap(), 30);
		assert_eq!(upcoming_days("2-week", today).unwrap(), 14);
		assert_eq!(upcoming_days("0-day", today).unwrap(), 1);
		// 2027-01-01 minus 2026-11-01, plus one.
		assert_eq!(upcoming_days("2-month", today).unwrap(), 62);
		assert_eq!(upcoming_days("1-fortnight", today).unwrap(), 7);
		assert_eq!(
			upcoming_days("3x-day", today).unwrap(),
			3,
			"parseInt reads the leading digits"
		);
		assert_eq!(
			upcoming_days("-3-day", today).unwrap(),
			7,
			"split('-') makes the unit \"3\""
		);
		assert!(matches!(
			upcoming_days("soon", today),
			Err(CoreError::Invalid(_))
		));
		// A huge custom length is a typed error, never a panic or a wrap.
		for huge in [
			"9223372036854775807-week",
			"9223372036854775807-year",
			"4294967296-month",
			"99999999999999999999-day",
			"99999999999999999999",
		] {
			assert!(
				matches!(upcoming_days(huge, today), Err(CoreError::Overflow)),
				"{huge}"
			);
		}
	}

	#[test]
	fn paid_window_looks_back_two_days_only_for_approximate_manual_bills() {
		let occurrence = day("2026-12-03");
		assert_eq!(paid_window_start(true, false, occurrence), occurrence);
		assert_eq!(paid_window_start(false, true, occurrence), occurrence);
		assert_eq!(
			paid_window_start(false, false, occurrence),
			day("2026-12-01")
		);
	}

	#[test]
	fn api_validation_matches_validate_schedule_date() {
		assert!(validate_schedule_date(&json!("2026-12-05")).is_ok());
		assert!(validate_schedule_date(&json!("2026-02-29")).is_err());
		assert!(
			validate_schedule_date(&json!({"start": "2026-02-29", "frequency": "monthly"}))
				.is_err()
		);
		assert!(
			validate_schedule_date(
				&json!({"start": "2026-02-28", "frequency": "monthly", "interval": 0})
			)
			.is_err()
		);
		assert!(validate_schedule_date(&json!({"start": "2026-02-28", "frequency": "monthly", "endMode": "after_n_occurrences", "endOccurrences": 0})).is_err());
		assert!(
			validate_schedule_date(
				&json!({"start": "2026-02-28", "frequency": "monthly", "endMode": "on_date"})
			)
			.is_err()
		);
		assert!(
			validate_schedule_date(
				&json!({"start": "2026-02-28", "frequency": "monthly", "endMode": "never"})
			)
			.is_ok()
		);
	}
}
