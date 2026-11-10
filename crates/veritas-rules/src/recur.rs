//! Actual's `RecurConfig` and the occurrences `@rschedule/core` 1.5 gives for
//! it, at day granularity: `recurConfigToRSchedule` builds up to two rrules,
//! and the schedule is their deduplicated union.

use chrono::{Datelike, Days, Months, NaiveDate, Weekday};
use serde_json::{Map, Value};

use crate::js;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RecurError {
	#[error("Actual rejects this recurrence: {0}")]
	Rejected(String),
	#[error("Veritas can't evaluate this recurrence: {0}")]
	Unsupported(String),
	#[error("the recurrence never lands on a date")]
	NoOccurrence,
	#[error("a recurrence without an end can't be walked backwards")]
	Unbounded,
	#[error("a weekend date needs a weekend solve mode of before or after")]
	WeekendSolveMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frequency {
	Daily,
	Weekly,
	Monthly,
	Yearly,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WeekendSolveMode {
	Before,
	After,
	Missing,
	Other(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Pattern {
	EveryPeriod,
	MonthDays(Vec<i64>),
	MonthWeekdays(Vec<(Weekday, i64)>),
	YearDay { month: u32, day: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RRule {
	frequency: Frequency,
	interval: i64,
	start: NaiveDate,
	count: Option<u64>,
	end: Option<NaiveDate>,
	pattern: Pattern,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recurrence {
	rules: Vec<RRule>,
	pub skip_weekend: bool,
	pub weekend_solve_mode: WeekendSolveMode,
	pub open_ended: bool,
}

/// Periods with no candidate date in a row before a pattern counts as
/// impossible. rschedule gives up sooner; both refuse to answer.
const MAX_EMPTY_PERIODS: u32 = 400;

impl Recurrence {
	/// Parses a date condition's object value the way `parseRecurDate` does.
	pub fn parse(value: &Value) -> Result<Recurrence, RecurError> {
		let config = value
			.as_object()
			.ok_or_else(|| RecurError::Rejected("a recurrence must be an object".into()))?;
		let frequency = match config.get("frequency").and_then(Value::as_str) {
			Some("daily") => Frequency::Daily,
			Some("weekly") => Frequency::Weekly,
			Some("monthly") => Frequency::Monthly,
			Some("yearly") => Frequency::Yearly,
			_ => return Err(RecurError::Rejected("invalid recurring date config".into())),
		};
		let start = js_date_option(config.get("start"), "start")?
			.ok_or_else(|| RecurError::Rejected("\"start\" must be a date".into()))?;

		let interval = match config.get("interval") {
			Some(value) if js::truthy(Some(value)) => {
				js::integer(value).filter(|n| *n >= 1).ok_or_else(|| {
					RecurError::Rejected("\"interval\" expects a whole number".into())
				})?
			}
			_ => 1,
		};

		let (count, end) = match config.get("endMode").and_then(Value::as_str) {
			Some("after_n_occurrences") => match config.get("endOccurrences") {
				None => (None, None),
				Some(value) => {
					let count = js::integer(value)
						.and_then(|n| u64::try_from(n).ok())
						.ok_or_else(|| {
							RecurError::Rejected("\"count\" must be a whole number".into())
						})?;
					(Some(count), None)
				}
			},
			Some("on_date") => (None, js_date_option(config.get("endDate"), "endDate")?),
			_ => (None, None),
		};

		let base = |pattern| RRule {
			frequency,
			interval,
			start,
			count,
			end,
			pattern,
		};
		let rules = match frequency {
			Frequency::Daily | Frequency::Weekly => vec![base(Pattern::EveryPeriod)],
			Frequency::Yearly => vec![base(Pattern::YearDay {
				month: start.month(),
				day: start.day(),
			})],
			Frequency::Monthly => {
				let (days, weekdays) = monthly_patterns(config)?;
				if days.is_empty() && weekdays.is_empty() {
					vec![base(Pattern::MonthDays(vec![i64::from(start.day())]))]
				} else {
					let mut rules = Vec::new();
					if !days.is_empty() {
						rules.push(base(Pattern::MonthDays(days)));
					}
					if !weekdays.is_empty() {
						rules.push(base(Pattern::MonthWeekdays(weekdays)));
					}
					rules
				}
			}
		};

		let weekend_solve_mode = match config.get("weekendSolveMode") {
			None | Some(Value::Null) => WeekendSolveMode::Missing,
			Some(Value::String(mode)) if mode == "before" => WeekendSolveMode::Before,
			Some(Value::String(mode)) if mode == "after" => WeekendSolveMode::After,
			Some(other) => WeekendSolveMode::Other(other.to_string()),
		};
		let open_ended = match config.get("endMode") {
			None | Some(Value::Null) => true,
			Some(mode) => mode == "never",
		};

		Ok(Recurrence {
			rules,
			skip_weekend: js::truthy(config.get("skipWeekend")),
			weekend_solve_mode,
			open_ended,
		})
	}

	/// The first occurrence on or after `from`, before any weekend move.
	pub fn first_on_or_after(&self, from: NaiveDate) -> Result<Option<NaiveDate>, RecurError> {
		let mut first: Option<NaiveDate> = None;
		for rule in &self.rules {
			if let Some(date) = Occurrences::new(rule, Some(from)).next_date()? {
				first = Some(first.map_or(date, |current| current.min(date)));
			}
		}
		Ok(first)
	}

	/// The last occurrence of a recurrence that ends.
	pub fn last(&self) -> Result<Option<NaiveDate>, RecurError> {
		let mut last: Option<NaiveDate> = None;
		for rule in &self.rules {
			if rule.count.is_none() && rule.end.is_none() {
				return Err(RecurError::Unbounded);
			}
			let found = match (rule.count, rule.end) {
				(None, Some(end)) => rule.last_on_or_before(end),
				_ => {
					let mut occurrences = Occurrences::new(rule, None);
					let mut found = None;
					while let Some(date) = occurrences.next_date()? {
						found = Some(date);
					}
					found
				}
			};
			if let Some(date) = found {
				last = Some(last.map_or(date, |current| current.max(date)));
			}
		}
		Ok(last)
	}

	/// rschedule's `occursBetween`, inclusive at both ends, ignoring weekends.
	pub fn occurs_between(&self, low: NaiveDate, high: NaiveDate) -> Result<bool, RecurError> {
		Ok(self
			.first_on_or_after(low)?
			.is_some_and(|date| date <= high))
	}

	pub fn occurs_on(&self, date: NaiveDate) -> Result<bool, RecurError> {
		self.occurs_between(date, date)
	}
}

/// `start` and `endDate` go through `parseDate`. A missing value is no date,
/// as rschedule sees `undefined`; a number would be an epoch time in JS.
fn js_date_option(value: Option<&Value>, name: &str) -> Result<Option<NaiveDate>, RecurError> {
	match value {
		None | Some(Value::Null) | Some(Value::Bool(false)) => Ok(None),
		Some(Value::String(text)) => js::parse_date(text)
			.map(Some)
			.ok_or_else(|| RecurError::Rejected(format!("\"{name}\" is an invalid date"))),
		Some(Value::Number(_)) => Err(RecurError::Unsupported(format!(
			"\"{name}\" is a timestamp"
		))),
		Some(_) => Err(RecurError::Rejected(format!("\"{name}\" must be a date"))),
	}
}

type MonthlyPatterns = (Vec<i64>, Vec<(Weekday, i64)>);

fn monthly_patterns(config: &Map<String, Value>) -> Result<MonthlyPatterns, RecurError> {
	let patterns = match config.get("patterns") {
		Some(Value::Array(patterns)) => patterns,
		value if !js::truthy(value) => return Ok((Vec::new(), Vec::new())),
		Some(Value::Object(_)) => return Ok((Vec::new(), Vec::new())),
		_ => return Err(RecurError::Unsupported("\"patterns\" isn't a list".into())),
	};
	let mut days = Vec::new();
	let mut weekdays = Vec::new();
	for pattern in patterns {
		let kind = pattern
			.get("type")
			.and_then(Value::as_str)
			.ok_or_else(|| RecurError::Rejected("a pattern needs a type".into()))?;
		let value = pattern.get("value");
		let number = value.and_then(js::integer);
		if kind == "day" {
			let day = number.ok_or_else(|| {
				RecurError::Unsupported("a day pattern needs a whole number".into())
			})?;
			if day == 0 || !(-31..=31).contains(&day) {
				return Err(RecurError::Rejected(
					"\"byDayOfMonth\" values must be `num !== 0 && num <= 31 && num >= -31`".into(),
				));
			}
			days.push(day);
		} else {
			let abbreviation: String = kind.chars().take(2).collect::<String>().to_uppercase();
			let weekday = match abbreviation.as_str() {
				"SU" => Weekday::Sun,
				"MO" => Weekday::Mon,
				"TU" => Weekday::Tue,
				"WE" => Weekday::Wed,
				"TH" => Weekday::Thu,
				"FR" => Weekday::Fri,
				"SA" => Weekday::Sat,
				_ => {
					return Err(RecurError::Rejected(format!(
						"\"byDayOfWeek\" doesn't know the weekday {kind:?}"
					)));
				}
			};
			let nth = number.ok_or_else(|| {
				RecurError::Unsupported("a weekday pattern needs a whole number".into())
			})?;
			if nth == 0 || !(-31..=31).contains(&nth) {
				return Err(RecurError::Rejected(
					"a monthly \"byDayOfWeek\" number must be within ±31 and not 0".into(),
				));
			}
			weekdays.push((weekday, nth));
		}
	}
	Ok((days, weekdays))
}

fn add_months(date: NaiveDate, months: i64) -> Option<NaiveDate> {
	let shift = Months::new(u32::try_from(months.unsigned_abs()).ok()?);
	if months >= 0 {
		date.checked_add_months(shift)
	} else {
		date.checked_sub_months(shift)
	}
}

fn add_days(date: NaiveDate, days: i64) -> Option<NaiveDate> {
	let shift = Days::new(days.unsigned_abs());
	if days >= 0 {
		date.checked_add_days(shift)
	} else {
		date.checked_sub_days(shift)
	}
}

fn days_in_month(first: NaiveDate) -> i64 {
	add_months(first, 1)
		.map(|next| (next - first).num_days())
		.unwrap_or(31)
}

impl RRule {
	fn months_since_start(&self, date: NaiveDate) -> i64 {
		i64::from(date.year() - self.start.year()) * 12 + i64::from(date.month())
			- i64::from(self.start.month())
	}

	/// Which interval-sized period `date` falls in, counted from the start's.
	fn period_of(&self, date: NaiveDate) -> i64 {
		let elapsed = match self.frequency {
			Frequency::Daily => (date - self.start).num_days(),
			Frequency::Weekly => (date - self.start).num_days().div_euclid(7),
			Frequency::Monthly => self.months_since_start(date),
			Frequency::Yearly => self.months_since_start(date).div_euclid(12),
		};
		elapsed.div_euclid(self.interval)
	}

	/// Every candidate date of period `k` in ascending order, or `None` once
	/// the calendar runs out.
	fn period_dates(&self, k: i64) -> Option<Vec<NaiveDate>> {
		let steps = k.checked_mul(self.interval)?;
		match (&self.pattern, self.frequency) {
			(Pattern::EveryPeriod, Frequency::Weekly) => {
				Some(vec![add_days(self.start, steps.checked_mul(7)?)?])
			}
			(Pattern::EveryPeriod, _) => Some(vec![add_days(self.start, steps)?]),
			(Pattern::YearDay { month, day }, _) => {
				let year = i64::from(self.start.year()).checked_add(steps)?;
				let year = i32::try_from(year).ok()?;
				NaiveDate::from_ymd_opt(year, 1, 1)?;
				Some(
					NaiveDate::from_ymd_opt(year, *month, *day)
						.into_iter()
						.collect(),
				)
			}
			(Pattern::MonthDays(days), _) => {
				let first = add_months(self.start.with_day(1)?, steps)?;
				let length = days_in_month(first);
				let mut dates: Vec<NaiveDate> = days
					.iter()
					.filter(|day| length >= day.abs())
					.map(|day| if *day > 0 { *day } else { length + day + 1 })
					.filter_map(|day| add_days(first, day - 1))
					.collect();
				dates.sort();
				dates.dedup();
				Some(dates)
			}
			(Pattern::MonthWeekdays(weekdays), _) => {
				let first = add_months(self.start.with_day(1)?, steps)?;
				let mut dates: Vec<NaiveDate> = weekdays
					.iter()
					.filter_map(|(weekday, nth)| nth_weekday(first, *weekday, *nth))
					.filter(|date| date.year() == first.year() && date.month() == first.month())
					.collect();
				dates.sort();
				dates.dedup();
				Some(dates)
			}
		}
	}
	/// The rule's first date on or after `cursor`, found the way rschedule's
	/// pipeline finds it when it starts from that point.
	fn step(&self, cursor: NaiveDate) -> Result<Option<NaiveDate>, RecurError> {
		let cursor = cursor.max(self.start);
		if let Pattern::MonthWeekdays(weekdays) = &self.pattern {
			return self.walk_weekdays(weekdays, cursor);
		}
		let mut empty_periods = 0;
		let mut period = self.period_of(cursor).max(0);
		loop {
			let Some(dates) = self.period_dates(period) else {
				return Ok(None);
			};
			if dates.is_empty() {
				empty_periods += 1;
				if empty_periods > MAX_EMPTY_PERIODS {
					return Err(RecurError::NoOccurrence);
				}
			} else if let Some(date) = dates.into_iter().find(|date| *date >= cursor) {
				return Ok(Some(date));
			}
			period += 1;
		}
	}

	/// rschedule's monthly `ByDayOfWeek` pipe followed by its interval check.
	/// An nth weekday the month doesn't have (a fifth Friday in April) spills
	/// into the next month, and rschedule jumps to that spilled date before
	/// looking again, so dates early in the next month are skipped (April's
	/// spilled Friday is May 5, which passes over Tuesday May 2). The pipe
	/// runs before the interval check, so a month outside the interval spills
	/// too. The result depends on where the walk starts, exactly as in Actual.
	fn walk_weekdays(
		&self,
		weekdays: &[(Weekday, i64)],
		mut cursor: NaiveDate,
	) -> Result<Option<NaiveDate>, RecurError> {
		for _ in 0..MAX_EMPTY_PERIODS * 4 {
			let Some(first) = cursor.with_day(1) else {
				return Ok(None);
			};
			let candidate = weekdays
				.iter()
				.filter_map(|(weekday, nth)| nth_weekday(first, *weekday, *nth))
				.filter(|date| *date >= cursor)
				.min();
			let Some(candidate) = candidate else {
				match add_months(first, 1) {
					Some(next) => cursor = next,
					None => return Ok(None),
				}
				continue;
			};
			let offset = self
				.months_since_start(candidate.with_day(1).unwrap_or(candidate))
				.rem_euclid(self.interval);
			if offset == 0 && candidate == cursor {
				return Ok(Some(candidate));
			}
			cursor = if offset == 0 {
				candidate
			} else {
				match candidate
					.with_day(1)
					.and_then(|month| add_months(month, self.interval - offset))
				{
					Some(next) => next,
					None => return Ok(None),
				}
			};
		}
		Err(RecurError::NoOccurrence)
	}

	/// The last date on or before `end` from the plain set of dates, which is
	/// what rschedule's reverse pipes walk (they never see spilled weekdays).
	fn last_on_or_before(&self, end: NaiveDate) -> Option<NaiveDate> {
		if end < self.start {
			return None;
		}
		(0..=self.period_of(end)).rev().find_map(|period| {
			self.period_dates(period)?
				.into_iter()
				.filter(|date| *date >= self.start && *date <= end)
				.max()
		})
	}
}

/// rschedule's `getNthWeekdayOfMonth`, which doesn't check the date stays in
/// the month.
fn nth_weekday(first: NaiveDate, weekday: Weekday, nth: i64) -> Option<NaiveDate> {
	let base = if nth < 0 {
		add_months(first, 1)?
	} else {
		first
	};
	let ahead = (i64::from(weekday.num_days_from_monday())
		- i64::from(base.weekday().num_days_from_monday()))
	.rem_euclid(7);
	let base = add_days(base, ahead)?;
	let weeks = if nth < 0 { nth } else { nth - 1 };
	add_days(base, weeks.checked_mul(7)?)
}

/// One rrule's occurrences in order. A rule without a count starts at the
/// window; one with a count walks from its own start, because the count
/// always runs from there (rschedule's `iterateWithCount`).
struct Occurrences<'a> {
	rule: &'a RRule,
	from: Option<NaiveDate>,
	cursor: NaiveDate,
	counted: u64,
	done: bool,
}

impl<'a> Occurrences<'a> {
	fn new(rule: &'a RRule, from: Option<NaiveDate>) -> Self {
		let cursor = match (rule.count, from) {
			(None, Some(from)) => from.max(rule.start),
			_ => rule.start,
		};
		Occurrences {
			rule,
			from,
			cursor,
			counted: 0,
			done: rule.count == Some(0),
		}
	}

	fn next_date(&mut self) -> Result<Option<NaiveDate>, RecurError> {
		while !self.done {
			let Some(date) = self.rule.step(self.cursor)? else {
				break;
			};
			if self.rule.end.is_some_and(|end| date > end) {
				break;
			}
			if let Some(count) = self.rule.count {
				if self.counted >= count {
					break;
				}
				self.counted += 1;
			}
			match date.succ_opt() {
				Some(next) => self.cursor = next,
				None => self.done = true,
			}
			if self.from.is_some_and(|from| date < from) {
				continue;
			}
			return Ok(Some(date));
		}
		self.done = true;
		Ok(None)
	}
}

#[cfg(test)]
mod tests {
	use serde_json::json;

	use super::*;

	fn date(text: &str) -> NaiveDate {
		NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
	}

	fn dates(config: Value, from: &str, n: usize) -> Vec<String> {
		let recurrence = Recurrence::parse(&config).unwrap();
		let mut out = Vec::new();
		let mut cursor = date(from);
		while out.len() < n {
			let Some(next) = recurrence.first_on_or_after(cursor).unwrap() else {
				break;
			};
			out.push(next.to_string());
			cursor = next.succ_opt().unwrap();
		}
		out
	}

	#[test]
	fn monthly_on_the_31st_skips_short_months() {
		// The oracle finding in docs/oracle.md: Jan 31, Mar 31, May 31, Jul 31, Aug 31.
		let config = json!({"frequency": "monthly", "start": "2026-01-31"});
		assert_eq!(
			dates(config, "2026-01-01", 5),
			[
				"2026-01-31",
				"2026-03-31",
				"2026-05-31",
				"2026-07-31",
				"2026-08-31"
			]
		);
	}

	#[test]
	fn last_day_pattern_follows_month_length_through_leap_years() {
		// reference/actual/packages/loot-core/src/server/schedules/occurrences.test.ts:77-87
		let config = json!({"frequency": "monthly", "start": "2024-01-01", "patterns": [{"type": "day", "value": -1}]});
		assert_eq!(
			dates(config, "2024-01-01", 4),
			["2024-01-31", "2024-02-29", "2024-03-31", "2024-04-30"]
		);
	}

	#[test]
	fn weekday_patterns_take_the_nth_and_last_weekday() {
		// occurrences.test.ts:90-103
		let config = json!({"frequency": "monthly", "start": "2024-01-01",
			"patterns": [{"type": "FR", "value": -1}, {"type": "MO", "value": 2}]});
		assert_eq!(
			dates(config, "2024-01-01", 4),
			["2024-01-08", "2024-01-26", "2024-02-12", "2024-02-23"]
		);
	}

	#[test]
	fn a_fifth_weekday_occurs_only_in_months_that_have_one() {
		let config = json!({"frequency": "monthly", "start": "2026-11-01", "patterns": [{"type": "MO", "value": 5}]});
		assert_eq!(
			dates(config, "2026-11-09", 3),
			["2026-11-30", "2027-03-29", "2027-05-31"]
		);
	}

	#[test]
	fn a_spilled_fifth_weekday_skips_early_dates_of_the_next_month() {
		// Found by the oracle test. April 2028 has no fifth Friday; rschedule
		// jumps to the spilled May 5 and never yields Tuesday May 2.
		let config = json!({"frequency": "monthly", "start": "2028-03-29",
			"patterns": [{"type": "FR", "value": 5}, {"type": "TU", "value": 1}]});
		assert_eq!(
			dates(config.clone(), "2028-04-05", 2),
			["2028-06-06", "2028-06-30"]
		);
		// Asked from May 1 instead, the walk never passes through April.
		assert_eq!(dates(config, "2028-05-01", 1), ["2028-05-02"]);
	}

	#[test]
	fn a_month_outside_the_interval_still_spills() {
		// Found by the oracle test. Every other month from March, so December
		// is skipped, but its fifth Friday spills to January 2 and passes over
		// Thursday January 1.
		let config = json!({"frequency": "monthly", "interval": 2, "start": "2024-03-09",
			"patterns": [{"type": "TH", "value": 1}, {"type": "FR", "value": 5}]});
		assert_eq!(dates(config, "2025-12-30", 1), ["2026-01-30"]);
	}

	#[test]
	fn yearly_feb_29_lands_only_in_leap_years() {
		let config = json!({"frequency": "yearly", "start": "2024-02-29"});
		assert_eq!(
			dates(config, "2024-01-01", 3),
			["2024-02-29", "2028-02-29", "2032-02-29"]
		);
	}

	#[test]
	fn mixed_patterns_count_per_rule_and_share_dates_once() {
		// Each rrule carries the count, and the union drops the shared Nov 2.
		let config = json!({"frequency": "monthly", "start": "2026-11-01",
			"patterns": [{"type": "day", "value": 2}, {"type": "MO", "value": 1}],
			"endMode": "after_n_occurrences", "endOccurrences": 3});
		assert_eq!(
			dates(config, "2026-11-01", 10),
			[
				"2026-11-02",
				"2026-12-02",
				"2026-12-07",
				"2027-01-02",
				"2027-01-04"
			]
		);
	}

	#[test]
	fn intervals_count_from_the_start_period() {
		let weekly = json!({"frequency": "weekly", "interval": 2, "start": "2024-01-01"});
		assert_eq!(dates(weekly, "2024-01-02", 2), ["2024-01-15", "2024-01-29"]);
		let daily = json!({"frequency": "daily", "interval": 3, "start": "2024-01-01"});
		assert_eq!(
			dates(daily, "2024-01-01", 4),
			["2024-01-01", "2024-01-04", "2024-01-07", "2024-01-10"]
		);
	}

	#[test]
	fn end_dates_are_inclusive_and_counts_are_exact() {
		// reference/actual/packages/loot-core/src/shared/schedules.test.ts:468-485
		let on_date = json!({"frequency": "monthly", "start": "2024-01-10", "endMode": "on_date", "endDate": "2024-03-10"});
		let recurrence = Recurrence::parse(&on_date).unwrap();
		assert_eq!(recurrence.last().unwrap(), Some(date("2024-03-10")));
		let counted = json!({"frequency": "monthly", "start": "2024-01-10", "endMode": "after_n_occurrences", "endOccurrences": 3});
		let recurrence = Recurrence::parse(&counted).unwrap();
		assert_eq!(recurrence.last().unwrap(), Some(date("2024-03-10")));
		assert_eq!(
			recurrence.first_on_or_after(date("2024-03-11")).unwrap(),
			None
		);
	}

	#[test]
	fn an_impossible_pattern_is_an_error_not_a_hang() {
		let config = json!({"frequency": "monthly", "interval": 12, "start": "2026-04-30",
			"patterns": [{"type": "day", "value": 31}]});
		let recurrence = Recurrence::parse(&config).unwrap();
		assert_eq!(
			recurrence.first_on_or_after(date("2026-04-01")),
			Err(RecurError::NoOccurrence)
		);
	}

	#[test]
	fn rejects_what_rschedule_rejects() {
		for config in [
			json!({"frequency": "Monthly", "start": "2026-01-01"}),
			json!({"frequency": "monthly", "start": ""}),
			json!({"frequency": "monthly"}),
			json!({"frequency": "monthly", "start": "2026-01-01", "interval": 1.5}),
			json!({"frequency": "monthly", "start": "2026-01-01", "patterns": [{"type": "day", "value": 32}]}),
			json!({"frequency": "monthly", "start": "2026-01-01", "patterns": [{"type": "XX", "value": 1}]}),
			json!({"frequency": "monthly", "start": "2026-01-01", "endMode": "after_n_occurrences", "endOccurrences": null}),
		] {
			assert!(
				matches!(Recurrence::parse(&config), Err(RecurError::Rejected(_))),
				"{config} should be rejected"
			);
		}
	}
}
