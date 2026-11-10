use veritas_domain::{BudgetData, CoreError, Money, YearMonth};

use crate::baseline::{Baseline, complete_months, months_between};
use crate::history::{Series, Side};
use crate::math::{isqrt, median, percentile, round_half_away, to_money};

pub const BAND_MIN_RESIDUALS: usize = 6;
pub(crate) const MIN_MONTHS_BEFORE: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BandMethod {
	/// P10 and P90 of the rolling-origin residuals (FC-4.2).
	Residuals,
	/// Range of the monthly uncertain net flow around its median (FC-4.3).
	Range,
	NoHistory,
}

/// FC-4's 80% band on the uncertain net flow, as one-month offsets.
#[derive(Clone, Debug, PartialEq)]
pub struct Band {
	pub method: BandMethod,
	/// Residuals for `Residuals`, months for `Range`.
	pub months: usize,
	pub low: Money,
	pub high: Money,
}

impl Band {
	/// Offsets for an `h`-month horizon, scaled by `sqrt(h)`. `None` with no history.
	pub fn offsets(&self, horizon: u32) -> Result<Option<(Money, Money)>, CoreError> {
		if self.method == BandMethod::NoHistory {
			return Ok(None);
		}
		let scale = isqrt(i128::from(horizon) * 100_000_000);
		let scaled =
			|offset: Money| to_money(round_half_away(i128::from(offset.minor()) * scale, 10_000));
		Ok(Some((scaled(self.low)?, scaled(self.high)?)))
	}

	pub fn label(&self) -> String {
		let n = self.months;
		match self.method {
			BandMethod::Residuals => format!("80% band from {n} past months"),
			BandMethod::Range => {
				format!("Based on {n} months of history, not a statistical range")
			}
			BandMethod::NoHistory => "No history yet".to_owned(),
		}
	}

	pub(crate) fn from_residuals(mut residuals: Vec<i64>, actuals: &[i64]) -> Band {
		residuals.sort_unstable();
		if residuals.len() >= BAND_MIN_RESIDUALS {
			return Band {
				method: BandMethod::Residuals,
				months: residuals.len(),
				low: Money::from_minor(percentile(&residuals, 1).unwrap_or(0)),
				high: Money::from_minor(percentile(&residuals, 9).unwrap_or(0)),
			};
		}
		let (Some(mid), Some(min), Some(max)) =
			(median(actuals), actuals.iter().min(), actuals.iter().max())
		else {
			return Band {
				method: BandMethod::NoHistory,
				months: 0,
				low: Money::ZERO,
				high: Money::ZERO,
			};
		};
		Band {
			method: BandMethod::Range,
			months: actuals.len(),
			low: Money::from_minor(min.saturating_sub(mid)),
			high: Money::from_minor(max.saturating_sub(mid)),
		}
	}
}

fn signed(side: Side, value: i64) -> i64 {
	match side {
		Side::Income => value,
		Side::Spending => value.saturating_neg(),
	}
}

/// The actual uncertain net flow `F_t` of a month: baselined income minus spending.
pub(crate) fn actual_flow(series: &[&Series], month: YearMonth) -> i64 {
	series.iter().fold(0i64, |total, series| {
		total.saturating_add(signed(series.side, series.month(month)))
	})
}

/// Every series' baseline as of `origin`, with only the months before it.
pub(crate) fn baselines_at(
	budget: &BudgetData,
	series: &[&Series],
	origin: YearMonth,
) -> Result<Vec<Baseline>, CoreError> {
	series
		.iter()
		.map(|series| Baseline::new(budget, series, origin, None))
		.collect()
}

/// FC-1's forecast of `F_t` for `month` from baselines made earlier.
pub(crate) fn forecast_flow(baselines: &[Baseline], month: YearMonth) -> Result<i64, CoreError> {
	let mut total = 0i64;
	for baseline in baselines {
		total = total
			.checked_add(signed(baseline.side, baseline.forecast(month)?.minor()))
			.ok_or(CoreError::Overflow)?;
	}
	Ok(total)
}

/// The first month any baselined series has a row.
pub(crate) fn first_month(series: &[&Series]) -> Option<YearMonth> {
	series
		.iter()
		.filter_map(|series| series.first_month())
		.min()
}

/// FC-4.2 step 1 and FC-4.3, from the complete months before `current`.
pub(crate) fn band(
	budget: &BudgetData,
	series: &[&Series],
	current: YearMonth,
) -> Result<Band, CoreError> {
	let Some(first) = first_month(series) else {
		return Ok(Band::from_residuals(Vec::new(), &[]));
	};
	let months = complete_months(current, months_between(first, current)?)?;
	let actuals: Vec<i64> = months
		.iter()
		.map(|month| actual_flow(series, *month))
		.collect();
	let mut residuals = Vec::new();
	for (month, actual) in months.iter().zip(&actuals).skip(MIN_MONTHS_BEFORE) {
		let forecast = forecast_flow(&baselines_at(budget, series, *month)?, *month)?;
		residuals.push(actual.saturating_sub(forecast));
	}
	Ok(Band::from_residuals(residuals, &actuals))
}

#[cfg(test)]
mod tests {
	use super::*;

	const RESIDUALS: [i64; 10] = [9000, -8000, -5000, -3000, -2000, -500, 0, 1000, 2500, 4000];

	fn offsets(band: &Band, horizon: u32) -> (i64, i64) {
		let (low, high) = band.offsets(horizon).unwrap().unwrap();
		(low.minor(), high.minor())
	}

	#[test]
	fn fc_4_worked_example_percentiles_and_scaling() {
		let band = Band::from_residuals(RESIDUALS.to_vec(), &[]);
		assert_eq!(band.method, BandMethod::Residuals);
		assert_eq!((band.low.minor(), band.high.minor()), (-8000, 4000));
		assert_eq!(offsets(&band, 1), (-8000, 4000));
		assert_eq!(offsets(&band, 4), (-16000, 8000));
		assert_eq!(offsets(&band, 3), (-13856, 6928));
		assert_eq!(band.label(), "80% band from 10 past months");
	}

	#[test]
	fn fc_4_3_fallback_with_few_residuals() {
		let band = Band::from_residuals(vec![1, 2], &[-1000, 0, 2000]);
		assert_eq!(band.method, BandMethod::Range);
		assert_eq!(offsets(&band, 1), (-1000, 2000));
		assert_eq!(
			band.label(),
			"Based on 3 months of history, not a statistical range"
		);
	}

	#[test]
	fn fc_4_3_no_history_means_no_band() {
		let band = Band::from_residuals(Vec::new(), &[]);
		assert_eq!(band.method, BandMethod::NoHistory);
		assert_eq!(band.offsets(3).unwrap(), None);
		assert_eq!(band.label(), "No history yet");
	}
}
