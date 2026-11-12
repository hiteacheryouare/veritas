use veritas_domain::{CoreError, Money, TransactionKind, YearMonth};
use veritas_reports::{Decision, Period};

use crate::band::{MIN_MONTHS_BEFORE, actual_flow, band, baselines_at, first_month, forecast_flow};
use crate::baseline::months_between;
use crate::history::{Series, bill_linked};
use crate::math::{round_half_away, round_half_up};
use crate::projection::Forecaster;

pub const BACKTEST_HORIZONS: [u32; 5] = [1, 3, 6, 10, 12];
const MIN_MONTHS_NET_WORTH: i32 = 6;
const SEASONAL_NAIVE_MIN: i32 = 13;

/// FC-7's metrics for one target at one horizon.
#[derive(Clone, Debug, PartialEq)]
pub struct Metrics {
	pub horizon: u32,
	/// Origins scored.
	pub n: usize,
	pub mae: Money,
	/// Basis points; `None` when every actual was 0.
	pub wape: Option<i64>,
	pub bias: Money,
	/// Whole percent of actuals inside the band.
	pub coverage: Option<i64>,
	/// Basis points against "last complete month repeated".
	pub skill_vs_last_month: Option<i64>,
	/// Basis points against "same month last year", over the origins that have one.
	pub skill_vs_last_year: Option<i64>,
	pub last_year_n: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Backtest {
	/// Target (a): the uncertain net flow of month `o + h`.
	pub uncertain_flow: Vec<Metrics>,
	/// Target (b): net-worth change over `o + 1 ..= o + h` without non-cash changes.
	pub net_worth: Vec<Metrics>,
}

#[derive(Default)]
struct Scores {
	errors: Vec<i128>,
	actuals: Vec<i128>,
	inside: usize,
	banded: usize,
	naive: Vec<i128>,
	seasonal: Vec<(i128, i128)>,
}

impl Scores {
	fn add(
		&mut self,
		actual: i64,
		forecast: i64,
		band: Option<(Money, Money)>,
		naive: i64,
		seasonal: Option<i64>,
	) {
		let error = i128::from(actual) - i128::from(forecast);
		self.errors.push(error);
		self.actuals.push(i128::from(actual));
		if let Some((low, high)) = band {
			self.banded += 1;
			let actual = i128::from(actual);
			let forecast = i128::from(forecast);
			if forecast + i128::from(low.minor()) <= actual
				&& actual <= forecast + i128::from(high.minor())
			{
				self.inside += 1;
			}
		}
		self.naive.push(i128::from(actual) - i128::from(naive));
		if let Some(seasonal) = seasonal {
			self.seasonal
				.push((error, i128::from(actual) - i128::from(seasonal)));
		}
	}

	fn metrics(&self, horizon: u32) -> Result<Metrics, CoreError> {
		let n = i128::try_from(self.errors.len()).map_err(|_| CoreError::Overflow)?;
		let mae = |errors: &mut dyn Iterator<Item = i128>, n: i128| {
			round_half_up(errors.map(i128::abs).sum::<i128>(), n)
		};
		let model = mae(&mut self.errors.iter().copied(), n);
		let skill = |model: i128, naive: i128| {
			(naive > 0).then(|| 10_000 - round_half_up(model * 10_000, naive))
		};
		let actual_total: i128 = self.actuals.iter().map(|actual| actual.abs()).sum();
		let error_total: i128 = self.errors.iter().map(|error| error.abs()).sum();
		let seasonal_n = i128::try_from(self.seasonal.len()).map_err(|_| CoreError::Overflow)?;
		let narrow = |value: i128| i64::try_from(value).map_err(|_| CoreError::Overflow);
		Ok(Metrics {
			horizon,
			n: self.errors.len(),
			mae: Money::from_minor(narrow(model)?),
			wape: (actual_total > 0)
				.then(|| narrow(round_half_up(error_total * 10_000, actual_total)))
				.transpose()?,
			bias: Money::from_minor(narrow(round_half_away(self.errors.iter().sum(), n))?),
			coverage: (self.banded > 0)
				.then(|| {
					narrow(round_half_up(
						100 * i128::try_from(self.inside).unwrap_or(0),
						i128::try_from(self.banded).unwrap_or(1),
					))
				})
				.transpose()?,
			skill_vs_last_month: skill(model, mae(&mut self.naive.iter().copied(), n))
				.map(narrow)
				.transpose()?,
			skill_vs_last_year: skill(
				mae(
					&mut self.seasonal.iter().map(|(model, _)| *model),
					seasonal_n,
				),
				mae(
					&mut self.seasonal.iter().map(|(_, naive)| *naive),
					seasonal_n,
				),
			)
			.map(narrow)
			.transpose()?,
			last_year_n: self.seasonal.len(),
		})
	}
}

fn add(a: i64, b: i64) -> Result<i64, CoreError> {
	a.checked_add(b).ok_or(CoreError::Overflow)
}

/// The latest month before `origin` in the same calendar month as `target`.
fn same_month_before(origin: YearMonth, target: YearMonth, first: YearMonth) -> Option<YearMonth> {
	(1..=10)
		.filter_map(|years| target.offset(-12 * years).ok())
		.find(|month| *month < origin)
		.filter(|month| *month >= first)
}

impl Forecaster<'_> {
	/// Net income without non-cash changes, E1 + E2 (statements.md ST-4),
	/// and the bill-linked part of it, which the backtest treats as known.
	fn cash_net_income(&self, month: YearMonth) -> Result<(i64, i64), CoreError> {
		let mut total = 0i64;
		let mut bills = 0i64;
		for row in self.classification.within(Period::month(month)) {
			let counted = row.is_primary()
				&& matches!(
					row.kind(),
					TransactionKind::Income | TransactionKind::Expense
				) && !matches!(
				row.decision,
				Decision::CapitalizedExpense | Decision::CapitalizedIncome
			);
			if counted {
				total = add(total, row.amount.minor())?;
				if bill_linked(self.budget, row) {
					bills = add(bills, row.amount.minor())?;
				}
			}
		}
		Ok((total, bills))
	}

	/// FC-7's rolling-origin backtest over every complete month before
	/// `current`. Origin `o` is a "today" on the 1st of `o`: it trains on the
	/// months before `o` and is scored on `o + 1 ..= o + h`. Bill-linked rows
	/// of the scored months count as known flows, so bills are perfect foresight.
	pub fn backtest(&self, current: YearMonth) -> Result<Backtest, CoreError> {
		let series: Vec<&Series> = self.baselined_series(&[]);
		let Some(first) = first_month(&series) else {
			return Ok(Backtest {
				uncertain_flow: Vec::new(),
				net_worth: Vec::new(),
			});
		};
		let last = current.offset(-1)?;
		let mut flow_scores: Vec<Scores> = BACKTEST_HORIZONS
			.iter()
			.map(|_| Scores::default())
			.collect();
		let mut worth_scores: Vec<Scores> = BACKTEST_HORIZONS
			.iter()
			.map(|_| Scores::default())
			.collect();

		let min_before = i32::try_from(MIN_MONTHS_BEFORE).unwrap_or(3);
		let mut origin = first.offset(min_before)?;
		while origin < last {
			let history = months_between(first, origin)?;
			let baselines = baselines_at(self.budget, &series, origin, self.season_min)?;
			let band = band(self.budget, &series, origin, self.season_min)?;
			let previous = origin.offset(-1)?;
			for (slot, horizon) in BACKTEST_HORIZONS.iter().enumerate() {
				let steps = i32::try_from(*horizon).map_err(|_| CoreError::Overflow)?;
				let target = origin.offset(steps)?;
				if target > last {
					break;
				}
				let seasonal_ok = history >= SEASONAL_NAIVE_MIN;
				let seasonal = match same_month_before(origin, target, first) {
					Some(month) if seasonal_ok => Some(actual_flow(&series, month)?),
					_ => None,
				};
				if let Some(scores) = flow_scores.get_mut(slot) {
					scores.add(
						actual_flow(&series, target)?,
						forecast_flow(&baselines, target)?,
						band.offsets(1)?,
						actual_flow(&series, previous)?,
						seasonal,
					);
				}
				if history < MIN_MONTHS_NET_WORTH {
					continue;
				}
				let (mut actual, mut forecast, mut seasonal) = (0i64, 0i64, Some(0i64));
				for step in 1..=steps {
					let month = origin.offset(step)?;
					let (cash, bills) = self.cash_net_income(month)?;
					actual = add(actual, cash)?;
					forecast = add(add(forecast, bills)?, forecast_flow(&baselines, month)?)?;
					seasonal = match (seasonal, same_month_before(origin, month, first)) {
						(Some(total), Some(before)) if seasonal_ok => {
							Some(add(total, self.cash_net_income(before)?.0)?)
						}
						_ => None,
					};
				}
				let naive = self
					.cash_net_income(previous)?
					.0
					.checked_mul(i64::from(*horizon))
					.ok_or(CoreError::Overflow)?;
				if let Some(scores) = worth_scores.get_mut(slot) {
					scores.add(actual, forecast, band.offsets(*horizon)?, naive, seasonal);
				}
			}
			origin = origin.offset(1)?;
		}

		let collect = |scores: Vec<Scores>| -> Result<Vec<Metrics>, CoreError> {
			scores
				.iter()
				.zip(BACKTEST_HORIZONS)
				.filter(|(scores, _)| !scores.errors.is_empty())
				.map(|(scores, horizon)| scores.metrics(horizon))
				.collect()
		};
		Ok(Backtest {
			uncertain_flow: collect(flow_scores)?,
			net_worth: collect(worth_scores)?,
		})
	}
}
