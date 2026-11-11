use veritas_domain::{CivilDate, CoreError, YearMonth};

/// An inclusive range of civil dates. A row belongs to a period when its date
/// falls inside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Period {
	first: CivilDate,
	last: CivilDate,
}

impl Period {
	pub fn new(first: CivilDate, last: CivilDate) -> Result<Self, CoreError> {
		if first > last {
			return Err(CoreError::Invalid(format!(
				"the period starts on {first}, after it ends on {last}"
			)));
		}
		Ok(Period { first, last })
	}

	pub fn month(month: YearMonth) -> Self {
		let first = month.first_day();
		let last = (28..=31)
			.rev()
			.find_map(|day| CivilDate::new(month.year(), month.month(), day).ok())
			.unwrap_or(first);
		Period { first, last }
	}

	pub fn first(self) -> CivilDate {
		self.first
	}

	pub fn last(self) -> CivilDate {
		self.last
	}

	pub fn contains(self, date: CivilDate) -> bool {
		self.first <= date && date <= self.last
	}
}

/// Every month from `first` through `last`, in order.
pub fn months(first: YearMonth, last: YearMonth) -> Result<Vec<YearMonth>, CoreError> {
	if first > last {
		return Err(CoreError::Invalid(format!(
			"the range starts in {first}, after it ends in {last}"
		)));
	}
	let mut months = vec![first];
	let mut month = first;
	while month < last {
		month = month.offset(1)?;
		months.push(month);
	}
	Ok(months)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn date(year: i32, month: u32, day: u32) -> CivilDate {
		CivilDate::new(year, month, day).unwrap()
	}

	#[test]
	fn a_month_runs_from_the_first_to_its_last_day() {
		let feb = Period::month(YearMonth::new(2028, 2).unwrap());
		assert_eq!(
			(feb.first(), feb.last()),
			(date(2028, 2, 1), date(2028, 2, 29))
		);
		let apr = Period::month(YearMonth::new(2027, 4).unwrap());
		assert_eq!(apr.last(), date(2027, 4, 30));
		assert!(apr.contains(date(2027, 4, 30)));
		assert!(!apr.contains(date(2027, 5, 1)));
	}

	#[test]
	fn months_span_a_year_boundary() {
		let nov = YearMonth::new(2026, 11).unwrap();
		let feb = YearMonth::new(2027, 2).unwrap();
		let names: Vec<_> = months(nov, feb)
			.unwrap()
			.iter()
			.map(ToString::to_string)
			.collect();
		assert_eq!(names, ["2026-11", "2026-12", "2027-01", "2027-02"]);
		assert!(months(feb, nov).is_err());
	}

	#[test]
	fn a_backwards_range_is_rejected() {
		assert!(Period::new(date(2027, 2, 1), date(2027, 1, 31)).is_err());
		assert!(Period::new(date(2027, 2, 1), date(2027, 2, 1)).is_ok());
	}
}
