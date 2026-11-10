use veritas_domain::{AccountId, BudgetData, CategoryId, CivilDate, CoreError, Money, YearMonth};

use crate::history::{Series, Side};
use crate::math::{median, round_half_away, round_half_up, to_money};

pub const WINDOW: i32 = 6;
pub const SEASON_MIN: i32 = 13;
pub const INDEX_MIN: i64 = 5_000;
pub const INDEX_MAX: i64 = 20_000;
pub const INDEX_ACTIVE_MIN: usize = 6;
const TYPICAL: i64 = 10_000;

/// A category's FC-1 baseline as of one current month.
#[derive(Clone, Debug, PartialEq)]
pub struct Baseline {
	pub category: CategoryId,
	pub side: Side,
	/// `L`, the trailing median.
	pub level: Money,
	/// The complete months in the window; 0 means no history yet.
	pub window_months: usize,
	/// Complete months since the category's first row.
	pub history_months: usize,
	/// Basis points per calendar month (January first), when seasonality is on.
	pub seasonal_index: Option<[i64; 12]>,
	/// `L'`, the median of the de-seasonalized window.
	pub deseasonalized_level: Option<Money>,
	/// The current month's `x` through today.
	pub month_to_date: Money,
	/// Where the forecast posts (FC-3).
	pub account: Option<AccountId>,
}

impl Baseline {
	pub(crate) fn new(
		budget: &BudgetData,
		series: &Series,
		current: YearMonth,
		today: Option<CivilDate>,
	) -> Result<Baseline, CoreError> {
		let history_months = series
			.first_month()
			.map(|first| months_between(first, current))
			.transpose()?
			.unwrap_or(0)
			.max(0);
		let window = complete_months(current, history_months.min(WINDOW))?;
		let values = window
			.iter()
			.map(|month| series.month(*month))
			.collect::<Result<Vec<i64>, _>>()?;
		let level = median(&values).unwrap_or(0);
		let seasonal_index = if history_months >= SEASON_MIN {
			seasonal_index(series, current, history_months)?
		} else {
			None
		};
		let deseasonalized_level = match &seasonal_index {
			Some(index) => {
				let adjusted = window
					.iter()
					.zip(&values)
					.map(|(month, x)| {
						i64::try_from(round_half_away(
							i128::from(*x) * i128::from(TYPICAL),
							i128::from(index_for(index, *month)),
						))
						.map_err(|_| CoreError::Overflow)
					})
					.collect::<Result<Vec<_>, _>>()?;
				median(&adjusted).map(Money::from_minor)
			}
			None => None,
		};
		let month_to_date = match today {
			Some(today) => series.sum(current.first_day(), today)?,
			None => 0,
		};
		Ok(Baseline {
			category: series.category.clone(),
			side: series.side,
			level: Money::from_minor(level),
			window_months: window.len(),
			history_months: usize::try_from(history_months).unwrap_or(0),
			seasonal_index,
			deseasonalized_level,
			month_to_date: Money::from_minor(month_to_date),
			account: match series.account(budget, &window)? {
				Some(account) => Some(account),
				None => {
					let mut months = complete_months(current, history_months)?;
					months.push(current);
					series.account(budget, &months)?
				}
			},
		})
	}

	pub fn has_history(&self) -> bool {
		self.window_months > 0
	}

	/// The seasonal index for `month`, 10000 without seasonality.
	pub fn index(&self, month: YearMonth) -> i64 {
		self.seasonal_index
			.as_ref()
			.map_or(TYPICAL, |index| index_for(index, month))
	}

	/// FC-1 step 5: the forecast for a whole target month, a positive magnitude.
	pub fn forecast(&self, month: YearMonth) -> Result<Money, CoreError> {
		match self.deseasonalized_level {
			Some(level) => to_money(round_half_away(
				i128::from(level.minor()) * i128::from(self.index(month)),
				i128::from(TYPICAL),
			)),
			None => Ok(self.level),
		}
	}

	/// The current month's uncertain rest: `max(0, forecast - month_to_date)`.
	pub fn remainder(&self, month: YearMonth) -> Result<Money, CoreError> {
		let rest = self.forecast(month)?.checked_sub(self.month_to_date)?;
		Ok(rest.max(Money::ZERO))
	}

	pub fn label(&self) -> String {
		match self.window_months {
			0 => "No history yet".to_owned(),
			n => format!("{n}-month median"),
		}
	}
}

/// Months from `first` up to but not including `current`.
pub(crate) fn months_between(first: YearMonth, current: YearMonth) -> Result<i32, CoreError> {
	let years = current
		.year()
		.checked_sub(first.year())
		.ok_or(CoreError::Overflow)?;
	let months = i64::from(years) * 12 + i64::from(current.month()) - i64::from(first.month());
	i32::try_from(months).map_err(|_| CoreError::Overflow)
}

/// The `count` complete months before `current`, oldest first.
pub(crate) fn complete_months(current: YearMonth, count: i32) -> Result<Vec<YearMonth>, CoreError> {
	(1..=count.max(0))
		.rev()
		.map(|back| current.offset(-back))
		.collect()
}

fn index_for(index: &[i64; 12], month: YearMonth) -> i64 {
	let slot = usize::try_from(month.month())
		.unwrap_or(1)
		.saturating_sub(1);
	index.get(slot).copied().unwrap_or(TYPICAL)
}

/// Raw indices of one 12-month window, or `None` when the window fails
/// `sum12 > 0` or `INDEX_ACTIVE_MIN` (FC-1 step 1).
fn raw_window(series: &Series, months: &[YearMonth]) -> Result<Option<[i64; 12]>, CoreError> {
	let values = months
		.iter()
		.map(|month| Ok((*month, series.month(*month)?)))
		.collect::<Result<Vec<(YearMonth, i64)>, CoreError>>()?;
	let sum12: i128 = values.iter().map(|(_, x)| i128::from(*x)).sum();
	let active = values.iter().filter(|(_, x)| *x != 0).count();
	if sum12 <= 0 || active < INDEX_ACTIVE_MIN {
		return Ok(None);
	}
	let mut raw = [TYPICAL; 12];
	for (month, x) in values {
		let value = round_half_up(i128::from(x.max(0)) * 120_000, sum12);
		let slot = usize::try_from(month.month())
			.ok()
			.and_then(|month| month.checked_sub(1))
			.and_then(|slot| raw.get_mut(slot))
			.ok_or(CoreError::Overflow)?;
		*slot = i64::try_from(value).map_err(|_| CoreError::Overflow)?;
	}
	Ok(Some(raw))
}

fn seasonal_index(
	series: &Series,
	current: YearMonth,
	history_months: i32,
) -> Result<Option<[i64; 12]>, CoreError> {
	let latest = complete_months(current, 12)?;
	let Some(raw) = raw_window(series, &latest)? else {
		return Ok(None);
	};
	let previous = if history_months >= 25 {
		let months = complete_months(current, 24)?;
		raw_window(series, months.get(..12).unwrap_or_default())?
	} else {
		None
	};
	let mut index = [TYPICAL; 12];
	for (slot, value) in index.iter_mut().enumerate() {
		let latest = i128::from(raw.get(slot).copied().unwrap_or(TYPICAL));
		let combined = match &previous {
			Some(previous) => round_half_up(
				latest + i128::from(previous.get(slot).copied().unwrap_or(TYPICAL)),
				2,
			),
			None => i128::from(TYPICAL) + round_half_away(latest - i128::from(TYPICAL), 2),
		};
		*value = i64::try_from(combined)
			.map_err(|_| CoreError::Overflow)?
			.clamp(INDEX_MIN, INDEX_MAX);
	}
	Ok(Some(index))
}
