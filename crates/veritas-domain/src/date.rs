use std::fmt;

use chrono::{Datelike, Months, NaiveDate};

use crate::CoreError;

/// A calendar date with no time zone, so a transaction's date never shifts
/// when the Mac changes zones (PLAN §9 #14).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CivilDate(NaiveDate);

impl CivilDate {
	pub fn new(year: i32, month: u32, day: u32) -> Result<Self, CoreError> {
		NaiveDate::from_ymd_opt(year, month, day)
			.map(CivilDate)
			.ok_or_else(|| {
				CoreError::Invalid(format!("{year:04}-{month:02}-{day:02} is not a date"))
			})
	}

	pub fn year(self) -> i32 {
		self.0.year()
	}

	pub fn month(self) -> u32 {
		self.0.month()
	}

	pub fn day(self) -> u32 {
		self.0.day()
	}

	pub fn year_month(self) -> YearMonth {
		YearMonth {
			year: self.0.year(),
			month: self.0.month(),
		}
	}

	pub fn naive(self) -> NaiveDate {
		self.0
	}
}

impl From<NaiveDate> for CivilDate {
	fn from(date: NaiveDate) -> Self {
		CivilDate(date)
	}
}

impl fmt::Display for CivilDate {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}", self.0.format("%Y-%m-%d"))
	}
}

/// A budget month.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct YearMonth {
	year: i32,
	month: u32,
}

impl YearMonth {
	pub fn new(year: i32, month: u32) -> Result<Self, CoreError> {
		CivilDate::new(year, month, 1).map(CivilDate::year_month)
	}

	pub fn parse(text: &str) -> Result<Self, CoreError> {
		let invalid = || CoreError::Invalid(format!("{text:?} is not a month like 2027-03"));
		let (year, month) = text.split_once('-').ok_or_else(invalid)?;
		if year.len() != 4 || month.len() != 2 {
			return Err(invalid());
		}
		let year = year.parse().map_err(|_| invalid())?;
		let month = month.parse().map_err(|_| invalid())?;
		YearMonth::new(year, month).map_err(|_| invalid())
	}

	pub fn year(self) -> i32 {
		self.year
	}

	pub fn month(self) -> u32 {
		self.month
	}

	pub fn first_day(self) -> CivilDate {
		CivilDate(NaiveDate::from_ymd_opt(self.year, self.month, 1).unwrap_or_default())
	}

	pub fn offset(self, months: i32) -> Result<Self, CoreError> {
		let first = self.first_day().0;
		let shifted = if months >= 0 {
			first.checked_add_months(Months::new(months.unsigned_abs()))
		} else {
			first.checked_sub_months(Months::new(months.unsigned_abs()))
		};
		shifted
			.map(|date| CivilDate(date).year_month())
			.ok_or(CoreError::Overflow)
	}
}

impl fmt::Display for YearMonth {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{:04}-{:02}", self.year, self.month)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn rejects_dates_that_do_not_exist() {
		assert!(CivilDate::new(2027, 2, 29).is_err());
		assert!(CivilDate::new(2028, 2, 29).is_ok());
		assert!(CivilDate::new(2027, 4, 31).is_err());
	}

	#[test]
	fn parses_and_prints_budget_months() {
		let march = YearMonth::parse("2027-03").unwrap();
		assert_eq!((march.year(), march.month()), (2027, 3));
		assert_eq!(march.to_string(), "2027-03");
		for bad in ["2027-3", "2027-13", "27-03", "2027/03", "", "2027-00"] {
			assert!(YearMonth::parse(bad).is_err(), "{bad:?} should be rejected");
		}
	}

	#[test]
	fn steps_across_year_boundaries() {
		let november = YearMonth::parse("2026-11").unwrap();
		assert_eq!(november.offset(2).unwrap().to_string(), "2027-01");
		assert_eq!(november.offset(-11).unwrap().to_string(), "2025-12");
		assert_eq!(november.offset(12).unwrap().to_string(), "2027-11");
	}
}
