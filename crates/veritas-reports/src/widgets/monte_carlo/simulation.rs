//! `runMonteCarloSimulation` (`monte-carlo/monteCarloSimulation.ts:1304-2853`),
//! kept to its order of operations, since float sums depend on it, and to its
//! order of random draws, since every later draw depends on that.

use veritas_domain::CoreError;

use super::config::{
	MonteCarloConfig, MonteCarloPot, MonteCarloSpendingPhase, ReturnModel, TaxModel,
	WithdrawalRuleType, WithdrawalStrategy, default_pots, surplus_pot,
};
use super::detail::{
	FactorRule, FloorCeilingBound, RuleAction, RuleExplanation, RunDetailRow, emit, emit_parts,
};
use super::history::HISTORICAL_ANNUAL_RETURNS;
use super::investment::{AssetWeights, pot_investment_at_age};
use super::js::{Mulberry32, clamp, js_max, js_min, js_round, sort_typed, stable_sort};
use crate::report_query::unsupported;

/// Money inputs are clamped to this many minor units.
pub const MAX_AMOUNT: f64 = 100_000_000_000_000.0;
pub const MIN_SIMULATION_COUNT: f64 = 1000.0;
pub const MAX_SIMULATION_COUNT: f64 = 10_000.0;
pub const MIN_HISTORICAL_BLOCK_LENGTH: f64 = 1.0;
pub const MAX_HISTORICAL_BLOCK_LENGTH: f64 = 30.0;
pub const MIN_HORIZON_YEARS: f64 = 1.0;
pub const MAX_HORIZON_YEARS: f64 = 100.0;
pub const MAX_WITHDRAWAL_TAX_RATE: f64 = 0.75;
pub const MAX_TAX_BAND_RATE: f64 = 0.99;
pub const MAX_ANNUAL_FEE_RATE: f64 = 0.1;
/// The seed every run uses unless its caller passes another; the card
/// never does.
pub const DEFAULT_SIMULATION_SEED: u32 = 1234;
/// Every emitted amount stays within ±2⁵⁰.
const MAX_EMITTED: f64 = 1_125_899_906_842_624.0;

pub(crate) fn to_safe_amount(value: f64) -> f64 {
	clamp(value, -MAX_EMITTED, MAX_EMITTED)
}

/// `getMonteCarloHorizonYears`: the simulated years between the two ages.
pub fn monte_carlo_horizon_years(config: &MonteCarloConfig) -> f64 {
	clamp(
		js_round(config.target_age - config.current_age),
		MIN_HORIZON_YEARS,
		MAX_HORIZON_YEARS,
	)
}

/// `MonteCarloParams`, minus the test-only `historicalReturns`.
#[derive(Clone, Debug, PartialEq)]
pub struct MonteCarloParams {
	pub config: MonteCarloConfig,
	pub horizon_years: f64,
	pub seed: u32,
	/// The run to record year by year in `run_detail`.
	pub capture_run_detail: Option<usize>,
	/// Discount every amount by the run's own inflation path.
	pub deflate_to_todays_money: bool,
}

/// Balances across the runs at one point (`MonteCarloPercentileBand`),
/// rounded to minor units. Year 0 is the start.
#[derive(Clone, Debug, PartialEq)]
pub struct PercentileBand {
	pub year: usize,
	pub p5: f64,
	pub p10: f64,
	pub p25: f64,
	pub p30: f64,
	pub p50: f64,
	pub p70: f64,
	pub p75: f64,
	pub p90: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DepletionCount {
	pub year: usize,
	pub count: u32,
}

/// `MonteCarloResult`. Amounts are minor units.
#[derive(Clone, Debug, PartialEq)]
pub struct MonteCarloResult {
	pub success_rate: f64,
	pub percentile_bands: Vec<PercentileBand>,
	/// Runs that ran out in each year 1..=horizon.
	pub depletion_histogram: Vec<DepletionCount>,
	/// The share of runs out of money by each year 0..=horizon.
	pub depletion_probability_by_year: Vec<f64>,
	pub median_ending_balance: f64,
	pub median_total_withdrawn: f64,
	pub median_depletion_year: Option<usize>,
	pub earliest_depletion_year: Option<usize>,
	pub latest_depletion_year: Option<usize>,
	/// The balance path of the run that ran out first, or ended lowest.
	pub worst_run_path: Vec<f64>,
	pub ending_balances: Vec<f64>,
	/// −1 for a run that lasted.
	pub depletion_year_by_simulation: Vec<i32>,
	pub total_withdrawn_by_simulation: Vec<f64>,
	pub run_detail: Option<Vec<RunDetailRow>>,
	/// The runs actually simulated: one per historical start year when
	/// replaying sequences, otherwise the clamped count asked for.
	pub simulation_count: usize,
	pub horizon_years: usize,
}

#[derive(Clone, Copy)]
struct TaxBand {
	from: f64,
	rate: f64,
}

/// A year's amounts in today's money, split so the year's realized amount
/// is `flat + adjusted × cumulative inflation`. Indexed `[year][item]`.
struct Schedule {
	flat: Vec<Vec<f64>>,
	adjusted: Vec<Vec<f64>>,
}

impl Schedule {
	fn amount(&self, year: usize, item: usize, inflation: f64) -> f64 {
		self.flat[year][item] + self.adjusted[year][item] * inflation
	}
}

/// One pot's investment in each year 1..=horizon.
struct PotYears {
	weights: Vec<Option<AssetWeights>>,
	mean: Vec<f64>,
	std_dev: Vec<f64>,
	fee_rate: Vec<f64>,
	fee_fixed: Vec<f64>,
	fee_adjusts: Vec<bool>,
}

/// Everything fixed before the first run.
struct Plan {
	pot_start_balances: Vec<f64>,
	strategy: WithdrawalStrategy,
	/// The first year each pot can fund withdrawals.
	access_from_year: Vec<f64>,
	surplus: Option<usize>,
	tax_model: TaxModel,
	pot_tax_rates: Vec<f64>,
	pot_taxable_fractions: Vec<f64>,
	tax_bands: Vec<TaxBand>,
	has_tax: bool,
	income_tax_rates: Vec<f64>,
	income_taxable_fractions: Vec<f64>,
	has_income_tax: bool,
	contribution_pot: Vec<Option<usize>>,
	/// `None` for money from outside the plan, and for a stream that no
	/// longer exists, whose contribution is scheduled at 0.
	contribution_source: Vec<Option<usize>>,
	contribution_before_tax: Vec<bool>,
	contributions: Schedule,
	income: Schedule,
	deflate: bool,
	rule: WithdrawalRuleType,
	minimum_spending: f64,
}

/// Balances and scratch space for one run, reused across runs.
struct Pots {
	balances: Vec<f64>,
	takes: Vec<f64>,
	previous_returns: Vec<f64>,
	drain_order: Vec<usize>,
	ideal: Vec<f64>,
}

/// One year's income and contributions (`settleYearFlows`' scratch).
struct Flows {
	contribution_amounts: Vec<f64>,
	income_gross: Vec<f64>,
	income_remaining: Vec<f64>,
}

struct Settled {
	gross_income: f64,
	income_tax: f64,
	taxable_income_base: f64,
	spendable_income: f64,
}

impl Plan {
	fn can_drain(&self, pot: usize, year: usize) -> bool {
		year as f64 >= self.access_from_year[pot] && Some(pot) != self.surplus
	}

	/// `bandTax`: progressive tax on a taxable income in today's money.
	fn band_tax(&self, taxable_income: f64) -> f64 {
		let mut tax = 0.0;
		for (index, band) in self.tax_bands.iter().enumerate() {
			if taxable_income <= band.from {
				break;
			}
			let upper = self
				.tax_bands
				.get(index + 1)
				.map_or(f64::INFINITY, |next| next.from);
			tax += band.rate * (js_min(taxable_income, upper) - band.from);
		}
		tax
	}

	/// `taxForTakes`: tax on the takes, above the income that already used
	/// up the lower bands.
	fn tax_for_takes(&self, takes: &[f64], inflation: f64, taxable_income_base: f64) -> f64 {
		if !self.has_tax {
			return 0.0;
		}
		if self.tax_model == TaxModel::Flat {
			let mut tax = 0.0;
			for (take, rate) in takes.iter().zip(&self.pot_tax_rates) {
				tax += take * rate;
			}
			return tax;
		}
		let mut taxable = 0.0;
		for (take, fraction) in takes.iter().zip(&self.pot_taxable_fractions) {
			taxable += take * fraction;
		}
		let base_today = taxable_income_base / inflation;
		(self.band_tax(base_today + taxable / inflation) - self.band_tax(base_today)) * inflation
	}

	fn capture_taxables(&self, takes: &[f64], into: &mut [f64]) {
		for (pot, taxable) in into.iter_mut().enumerate() {
			*taxable = match self.tax_model {
				TaxModel::Bands => takes[pot] * self.pot_taxable_fractions[pot],
				TaxModel::Flat if self.pot_tax_rates[pot] > 0.0 => takes[pot],
				TaxModel::Flat => 0.0,
			};
		}
	}

	fn attribute_tax_to_pots(&self, total_tax: f64, takes: &[f64], into: &mut [f64]) {
		if total_tax <= 0.0 {
			return;
		}
		if self.tax_model == TaxModel::Flat {
			for (pot, tax) in into.iter_mut().enumerate() {
				*tax = takes[pot] * self.pot_tax_rates[pot];
			}
			return;
		}
		let mut taxable_total = 0.0;
		for (take, fraction) in takes.iter().zip(&self.pot_taxable_fractions) {
			taxable_total += take * fraction;
		}
		if taxable_total <= 0.0 {
			return;
		}
		for (pot, tax) in into.iter_mut().enumerate() {
			*tax = total_tax * ((takes[pot] * self.pot_taxable_fractions[pot]) / taxable_total);
		}
	}

	/// `computeTakes`: the surplus pot first, then the strategy's split.
	fn compute_takes(&self, pots: &mut Pots, gross_total: f64, year: usize, accessible: f64) {
		pots.takes.fill(0.0);
		if gross_total <= 0.0 || accessible <= 0.0 {
			return;
		}
		let mut remaining_gross = gross_total;
		let mut remaining_accessible = accessible;
		if let Some(surplus) = self.surplus
			&& year as f64 >= self.access_from_year[surplus]
		{
			let take = js_min(pots.balances[surplus], remaining_gross);
			pots.takes[surplus] = take;
			remaining_gross -= take;
			remaining_accessible -= pots.balances[surplus];
		}
		if remaining_gross > 0.0 && remaining_accessible > 0.0 {
			self.split_across_pots(pots, remaining_gross, year, remaining_accessible);
		}
	}

	/// `splitAcrossPots` (`monteCarloSimulation.ts:1524-1635`).
	fn split_across_pots(&self, pots: &mut Pots, gross_total: f64, year: usize, accessible: f64) {
		let count = pots.balances.len();
		match self.strategy {
			WithdrawalStrategy::Sequential | WithdrawalStrategy::BestPerformer => {
				let best = self.strategy == WithdrawalStrategy::BestPerformer;
				if best {
					for (index, slot) in pots.drain_order.iter_mut().enumerate() {
						*slot = index;
					}
					let returns = &pots.previous_returns;
					stable_sort(&mut pots.drain_order, |&a, &b| returns[b] - returns[a]);
				}
				let mut remaining = gross_total;
				for order in 0..count {
					if remaining.is_nan() || remaining <= 0.0 {
						break;
					}
					let pot = if best { pots.drain_order[order] } else { order };
					if !self.can_drain(pot, year) {
						continue;
					}
					let take = js_min(pots.balances[pot], remaining);
					pots.takes[pot] = take;
					remaining -= take;
				}
			}
			WithdrawalStrategy::TargetMix => {
				let mut target_accessible = 0.0;
				for pot in 0..count {
					if self.can_drain(pot, year) {
						target_accessible += self.pot_start_balances[pot];
					}
				}
				let remaining_total = accessible - gross_total;
				for pot in 0..count {
					pots.drain_order[pot] = pot;
					pots.ideal[pot] = if self.can_drain(pot, year) && target_accessible > 0.0 {
						(self.pot_start_balances[pot] / target_accessible) * remaining_total
					} else {
						0.0
					};
				}
				let (balances, ideal) = (&pots.balances, &pots.ideal);
				stable_sort(&mut pots.drain_order, |&a, &b| {
					balances[b] - ideal[b] - (balances[a] - ideal[a])
				});
				let mut remaining = gross_total;
				for order in 0..count {
					if remaining.is_nan() || remaining <= 0.0 {
						break;
					}
					let pot = pots.drain_order[order];
					if !self.can_drain(pot, year) {
						continue;
					}
					let excess = pots.balances[pot] - pots.ideal[pot];
					if excess <= 0.0 {
						continue;
					}
					let take = js_min(js_min(remaining, pots.balances[pot]), excess);
					pots.takes[pot] = take;
					remaining -= take;
				}
				for pot in 0..count {
					if remaining.is_nan() || remaining <= 0.0 {
						break;
					}
					if !self.can_drain(pot, year) {
						continue;
					}
					let take = js_min(pots.balances[pot] - pots.takes[pot], remaining);
					pots.takes[pot] += take;
					remaining -= take;
				}
			}
			WithdrawalStrategy::Proportional => {
				let last = (0..count).rev().find(|&pot| self.can_drain(pot, year));
				let mut remaining = gross_total;
				for pot in 0..last.unwrap_or(0) {
					if !self.can_drain(pot, year) {
						continue;
					}
					let take = gross_total * (pots.balances[pot] / accessible);
					pots.takes[pot] = take;
					remaining -= take;
				}
				if let Some(last) = last {
					pots.takes[last] = remaining;
				}
			}
		}
	}

	/// `settleYearFlows` (`monteCarloSimulation.ts:1808-1904`): income
	/// arrives, before-tax contributions come out of it, it's taxed, then
	/// after-tax contributions come out of what's left.
	fn settle_year_flows(
		&self,
		flows: &mut Flows,
		year: usize,
		inflation: f64,
		with_contributions: bool,
	) -> Settled {
		let mut gross_income = 0.0;
		for stream in 0..flows.income_gross.len() {
			let gross = self.income.amount(year, stream, inflation);
			flows.income_gross[stream] = gross;
			flows.income_remaining[stream] = gross;
			gross_income += gross;
		}
		flows.contribution_amounts.fill(0.0);
		if with_contributions {
			for item in 0..flows.contribution_amounts.len() {
				let scheduled = self.contributions.amount(year, item, inflation);
				match self.contribution_source[item] {
					None => flows.contribution_amounts[item] = scheduled,
					Some(source) if self.contribution_before_tax[item] => {
						let amount = js_min(scheduled, flows.income_remaining[source]);
						flows.income_remaining[source] -= amount;
						flows.contribution_amounts[item] = amount;
					}
					Some(_) => {}
				}
			}
		}
		let mut income_tax = 0.0;
		let mut taxable_income_base = 0.0;
		if self.has_income_tax && self.tax_model == TaxModel::Flat {
			for (remaining, rate) in flows
				.income_remaining
				.iter_mut()
				.zip(&self.income_tax_rates)
			{
				let tax = *remaining * rate;
				*remaining -= tax;
				income_tax += tax;
			}
		} else if self.tax_model == TaxModel::Bands {
			for (remaining, fraction) in flows
				.income_remaining
				.iter()
				.zip(&self.income_taxable_fractions)
			{
				taxable_income_base += remaining * fraction;
			}
			if self.has_income_tax && taxable_income_base > 0.0 {
				income_tax = self.band_tax(taxable_income_base / inflation) * inflation;
				for (remaining, fraction) in flows
					.income_remaining
					.iter_mut()
					.zip(&self.income_taxable_fractions)
				{
					*remaining -= income_tax * ((*remaining * fraction) / taxable_income_base);
				}
			}
		}
		if with_contributions {
			for item in 0..flows.contribution_amounts.len() {
				let Some(source) = self.contribution_source[item] else {
					continue;
				};
				if self.contribution_before_tax[item] {
					continue;
				}
				let scheduled = self.contributions.amount(year, item, inflation);
				let amount = js_min(scheduled, flows.income_remaining[source]);
				flows.income_remaining[source] -= amount;
				flows.contribution_amounts[item] = amount;
			}
		}
		let mut spendable_income = 0.0;
		for remaining in &flows.income_remaining {
			spendable_income += remaining;
		}
		Settled {
			gross_income,
			income_tax,
			taxable_income_base,
			spendable_income,
		}
	}
}

/// `scheduleByYear`: each item's amount in the years its ages cover.
fn schedule(
	horizon: usize,
	current_age: f64,
	items: impl Iterator<Item = (Option<f64>, Option<f64>, f64, bool)>,
	count: usize,
) -> Schedule {
	let mut schedule = Schedule {
		flat: vec![vec![0.0; count]; horizon + 1],
		adjusted: vec![vec![0.0; count]; horizon + 1],
	};
	for (index, (from_age, to_age, annual_amount, adjusts)) in items.enumerate() {
		let amount = clamp(annual_amount, 0.0, MAX_AMOUNT);
		for year in 1..=horizon {
			let age = current_age + year as f64 - 1.0;
			if from_age.is_some_and(|from| age < from) || to_age.is_some_and(|to| age > to) {
				continue;
			}
			let target = if adjusts {
				&mut schedule.adjusted
			} else {
				&mut schedule.flat
			};
			target[year][index] = amount;
		}
	}
	schedule
}

/// `resolveSpendingPhases` then `getActiveSpendingPhase`'s amount for each
/// year, clamped.
fn planned_today_by_year(
	phases: &[MonteCarloSpendingPhase],
	horizon: usize,
	current_age: f64,
) -> Vec<f64> {
	let mut resolved: Vec<&MonteCarloSpendingPhase> = phases.iter().collect();
	stable_sort(&mut resolved, |a, b| {
		a.from_age.unwrap_or(f64::NEG_INFINITY) - b.from_age.unwrap_or(f64::NEG_INFINITY)
	});
	let fallback = MonteCarloSpendingPhase {
		id: "phase-1".into(),
		name: String::new(),
		from_age: None,
		annual_withdrawal: 2_000_000.0,
	};
	if resolved.is_empty() {
		resolved.push(&fallback);
	}
	let mut planned = vec![0.0; horizon + 1];
	for (year, amount) in planned.iter_mut().enumerate().skip(1) {
		let age = current_age + year as f64 - 1.0;
		let mut active = resolved[0];
		for phase in &resolved {
			if phase.from_age.is_none_or(|from| from <= age) {
				active = phase;
			} else {
				break;
			}
		}
		*amount = clamp(active.annual_withdrawal, 0.0, MAX_AMOUNT);
	}
	planned
}

fn percentile_of_sorted(sorted: &[f64], percentile: f64) -> f64 {
	let position = (sorted.len() - 1) as f64 * percentile;
	let lower = position.floor();
	let upper = position.ceil();
	if lower == upper {
		return sorted[lower as usize];
	}
	let weight = position - lower;
	sorted[lower as usize] * (1.0 - weight) + sorted[upper as usize] * weight
}

fn band(year: usize, sorted: &[f64]) -> PercentileBand {
	let at = |percentile| js_round(percentile_of_sorted(sorted, percentile));
	PercentileBand {
		year,
		p5: at(0.05),
		p10: at(0.1),
		p25: at(0.25),
		p30: at(0.3),
		p50: at(0.5),
		p70: at(0.7),
		p75: at(0.75),
		p90: at(0.9),
	}
}

/// What one captured year carries into its row.
struct Captured {
	pot_contributions: Vec<f64>,
	contribution_amounts: Vec<f64>,
	pot_returns: Vec<Option<f64>>,
	pot_withdrawals: Vec<f64>,
	pot_taxes: Vec<f64>,
	pot_taxables: Vec<f64>,
	pot_fees: Vec<f64>,
	pot_start_balances: Vec<f64>,
	failure_snapshot: Option<Vec<f64>>,
	rule_explanation: Option<RuleExplanation>,
	minimum_applied: bool,
}

/// `runMonteCarloSimulation`. Fails only when an age or the run count isn't
/// a number JavaScript could size an array with.
pub fn run_monte_carlo_simulation(
	params: &MonteCarloParams,
) -> Result<MonteCarloResult, CoreError> {
	let config = &params.config;
	let pots_config: Vec<MonteCarloPot> = if config.pots.is_empty() {
		default_pots()
	} else {
		config.pots.clone()
	}
	.into_iter()
	.map(|pot| {
		if pot.is_surplus {
			MonteCarloPot {
				name: pot.name,
				..surplus_pot(pot.id)
			}
		} else {
			pot
		}
	})
	.collect();
	let pot_count = pots_config.len();
	let current_age = config.current_age;

	let mut tax_bands: Vec<TaxBand> = config
		.tax_bands
		.iter()
		.map(|band| TaxBand {
			from: clamp(band.from, 0.0, MAX_AMOUNT),
			rate: clamp(band.rate, 0.0, MAX_TAX_BAND_RATE),
		})
		.collect();
	stable_sort(&mut tax_bands, |a, b| a.from - b.from);
	if tax_bands.first().is_none_or(|band| band.from > 0.0) {
		tax_bands.insert(
			0,
			TaxBand {
				from: 0.0,
				rate: 0.0,
			},
		);
	}
	let tax_model = config.tax_model;
	let pot_tax_rates: Vec<f64> = pots_config
		.iter()
		.map(|pot| clamp(pot.withdrawal_tax_rate, 0.0, MAX_WITHDRAWAL_TAX_RATE))
		.collect();
	let pot_taxable_fractions: Vec<f64> = pots_config
		.iter()
		.map(|pot| clamp(pot.taxable_fraction, 0.0, 1.0))
		.collect();
	let any_band_rate = tax_bands.iter().any(|band| band.rate > 0.0);
	let has_tax = match tax_model {
		TaxModel::Bands => any_band_rate && pot_taxable_fractions.iter().any(|f| *f > 0.0),
		TaxModel::Flat => pot_tax_rates.iter().any(|rate| *rate > 0.0),
	};
	let streams = &config.income_streams;
	let income_tax_rates: Vec<f64> = streams
		.iter()
		.map(|stream| clamp(stream.tax_rate, 0.0, MAX_WITHDRAWAL_TAX_RATE))
		.collect();
	let income_taxable_fractions: Vec<f64> = streams
		.iter()
		.map(|stream| clamp(stream.taxable_fraction, 0.0, 1.0))
		.collect();
	let has_income_tax = match tax_model {
		TaxModel::Bands => any_band_rate && income_taxable_fractions.iter().any(|f| *f > 0.0),
		TaxModel::Flat => income_tax_rates.iter().any(|rate| *rate > 0.0),
	};
	let max_tax_rate = match tax_model {
		TaxModel::Bands => tax_bands
			.iter()
			.fold(0.0, |max, band| js_max(max, band.rate)),
		TaxModel::Flat => pot_tax_rates
			.iter()
			.fold(0.0, |max, rate| js_max(max, *rate)),
	};
	let min_net_factor = 1.0 - max_tax_rate;

	let horizon_years = clamp(
		js_round(params.horizon_years),
		MIN_HORIZON_YEARS,
		MAX_HORIZON_YEARS,
	);
	if horizon_years.is_nan() {
		return Err(unsupported(format!(
			"a Monte Carlo horizon of {}",
			params.horizon_years
		)));
	}
	let horizon = horizon_years as usize;

	let mut pot_years = Vec::with_capacity(pot_count);
	let mut has_normal_draw = vec![false; horizon + 1];
	let mut has_fees = false;
	for pot in &pots_config {
		let mut years = PotYears {
			weights: vec![None; horizon + 1],
			mean: vec![0.0; horizon + 1],
			std_dev: vec![0.0; horizon + 1],
			fee_rate: vec![0.0; horizon + 1],
			fee_fixed: vec![0.0; horizon + 1],
			fee_adjusts: vec![false; horizon + 1],
		};
		for (year, normal_draw) in has_normal_draw.iter_mut().enumerate().skip(1) {
			let investment =
				pot_investment_at_age(pot, current_age, current_age + year as f64 - 1.0);
			years.weights[year] = match config.return_model {
				ReturnModel::Normal => None,
				_ => investment.weights,
			};
			if years.weights[year].is_none() {
				*normal_draw = true;
			}
			years.mean[year] = investment.mean;
			years.std_dev[year] = investment.std_dev;
			years.fee_rate[year] = investment.fee_rate;
			years.fee_fixed[year] = investment.fee_fixed;
			years.fee_adjusts[year] = investment.fee_adjusts;
			if investment.fee_rate > 0.0 || investment.fee_fixed > 0.0 {
				has_fees = true;
			}
		}
		pot_years.push(years);
	}

	let planned_today = planned_today_by_year(&config.spending_phases, horizon, current_age);

	let contribution_count = config.contributions.len();
	let contribution_pot: Vec<Option<usize>> = config
		.contributions
		.iter()
		.map(|item| pots_config.iter().position(|pot| pot.id == item.pot_id))
		.collect();
	let contribution_source: Vec<Option<usize>> = config
		.contributions
		.iter()
		.map(|item| {
			item.source_income_stream_id
				.as_ref()
				.and_then(|source| streams.iter().position(|stream| stream.id == *source))
		})
		.collect();
	let contributions = schedule(
		horizon,
		current_age,
		config
			.contributions
			.iter()
			.enumerate()
			.map(|(index, item)| {
				let orphaned = contribution_pot[index].is_none()
					|| (item.source_income_stream_id.is_some()
						&& contribution_source[index].is_none());
				let amount = if orphaned { 0.0 } else { item.annual_amount };
				(
					item.from_age,
					item.to_age,
					amount,
					item.adjusts_with_inflation,
				)
			}),
		contribution_count,
	);
	let has_contributions = contributions
		.flat
		.iter()
		.zip(&contributions.adjusted)
		.any(|(flat, adjusted)| flat.iter().chain(adjusted).any(|amount| *amount > 0.0));
	let income = schedule(
		horizon,
		current_age,
		streams.iter().map(|stream| {
			(
				stream.from_age,
				stream.to_age,
				stream.annual_amount,
				stream.adjusts_with_inflation,
			)
		}),
		streams.len(),
	);

	let pot_start_balances: Vec<f64> = pots_config
		.iter()
		.map(|pot| clamp(pot.starting_balance, 0.0, MAX_AMOUNT))
		.collect();
	let access_from_year: Vec<f64> = pots_config
		.iter()
		.map(|pot| {
			pot.access_age
				.map_or(1.0, |age| js_max(1.0, js_round(age - current_age) + 1.0))
		})
		.collect();
	let plan = Plan {
		pot_start_balances,
		strategy: config.withdrawal_strategy,
		access_from_year,
		surplus: pots_config.iter().position(|pot| pot.is_surplus),
		tax_model,
		pot_tax_rates,
		pot_taxable_fractions,
		tax_bands,
		has_tax,
		income_tax_rates,
		income_taxable_fractions,
		has_income_tax,
		contribution_pot,
		contribution_source,
		contribution_before_tax: config
			.contributions
			.iter()
			.map(|item| item.before_tax)
			.collect(),
		contributions,
		income,
		deflate: params.deflate_to_todays_money && config.inflation_mean.is_some(),
		rule: config.withdrawal_rule.kind,
		minimum_spending: clamp(config.minimum_spending, 0.0, MAX_AMOUNT),
	};
	let mut flows = Flows {
		contribution_amounts: vec![0.0; contribution_count],
		income_gross: vec![0.0; streams.len()],
		income_remaining: vec![0.0; streams.len()],
	};

	let mut pot_funded_planned_today = vec![0.0; horizon + 1];
	for year in 1..=horizon {
		let settled = plan.settle_year_flows(&mut flows, year, 1.0, true);
		pot_funded_planned_today[year] =
			js_max(0.0, planned_today[year] - settled.spendable_income);
	}
	let first_spending_year = (1..=horizon).find(|&year| pot_funded_planned_today[year] > 0.0);

	let history = &HISTORICAL_ANNUAL_RETURNS;
	let history_count = history.len();
	let inflation_mean = config.inflation_mean;
	let inflation_std_dev = if inflation_mean.is_some() {
		js_max(0.0, config.inflation_std_dev)
	} else {
		0.0
	};
	let deflate = plan.deflate;
	let simulation_count = match config.return_model {
		ReturnModel::HistoricalSequence => history_count,
		_ => {
			let count = clamp(
				js_round(config.simulation_count),
				MIN_SIMULATION_COUNT,
				MAX_SIMULATION_COUNT,
			);
			if count.is_nan() {
				return Err(unsupported(format!(
					"a Monte Carlo simulation count of {}",
					config.simulation_count
				)));
			}
			count as usize
		}
	};
	let block_continue_probability = 1.0
		- 1.0
			/ clamp(
				config.historical_block_length,
				MIN_HISTORICAL_BLOCK_LENGTH,
				MAX_HISTORICAL_BLOCK_LENGTH,
			);
	let mut random = Mulberry32::new(params.seed);

	let mut balances_by_year = vec![vec![0.0; simulation_count]; horizon + 1];
	let mut depletion_counts = vec![0_u32; horizon + 1];
	let mut survived_count = 0_usize;
	let mut worst_sim_index = 0;
	let mut worst_depletion_year = f64::INFINITY;
	let mut worst_final_balance = f64::INFINITY;

	let starting_total = plan
		.pot_start_balances
		.iter()
		.fold(0.0, |sum, balance| sum + balance);
	let mut pots = Pots {
		balances: vec![0.0; pot_count],
		takes: vec![0.0; pot_count],
		previous_returns: vec![0.0; pot_count],
		drain_order: (0..pot_count).collect(),
		ideal: vec![0.0; pot_count],
	};
	let rule = &config.withdrawal_rule;
	let minimum_spending = plan.minimum_spending;

	let mut accessible_start_by_year = vec![0.0; horizon + 1];
	for (year, accessible_start) in accessible_start_by_year.iter_mut().enumerate().skip(1) {
		for pot in 0..pot_count {
			if year as f64 >= plan.access_from_year[pot] {
				*accessible_start += plan.pot_start_balances[pot];
			}
		}
	}

	let mut withdrawn_totals = vec![0.0; simulation_count];
	let mut depletion_year_by_simulation = vec![-1_i32; simulation_count];
	let capture = params
		.capture_run_detail
		.filter(|index| *index < simulation_count);
	let mut run_detail: Option<Vec<RunDetailRow>> = capture.map(|_| Vec::new());

	for simulation_index in 0..simulation_count {
		pots.balances.copy_from_slice(&plan.pot_start_balances);
		pots.previous_returns.fill(0.0);
		let mut total = starting_total;
		let mut adjustment_factor = 1.0;
		let mut floor_ceiling_anchor_rate = 0.0;
		let mut cumulative_inflation = 1.0;
		let mut previous_history_index: Option<usize> = None;
		let mut ratchet_streak = 0_u32;
		let mut withdrawn_sum = 0.0;
		let mut depleted = false;
		let mut simulation_depletion_year: Option<usize> = None;
		let captured_run = capture == Some(simulation_index);

		balances_by_year[0][simulation_index] = to_safe_amount(total);

		for year in 1..=horizon {
			if !depleted {
				let start_deflator = if deflate {
					1.0 / cumulative_inflation
				} else {
					1.0
				};
				let settled = plan.settle_year_flows(&mut flows, year, cumulative_inflation, true);
				let mut contributions_this_year = 0.0;
				let mut pre_contribution_balances = None;
				let mut captured_pot_contributions = captured_run.then(|| vec![0.0; pot_count]);
				if has_contributions {
					if captured_pot_contributions.is_some() {
						pre_contribution_balances = Some(pots.balances.clone());
					}
					for (item, deposit) in flows.contribution_amounts.iter().enumerate() {
						if *deposit > 0.0
							&& let Some(pot) = plan.contribution_pot[item]
						{
							pots.balances[pot] += deposit;
							total += deposit;
							contributions_this_year += deposit;
							if let Some(captured) = captured_pot_contributions.as_mut() {
								captured[pot] += deposit;
							}
						}
					}
				}
				let captured_contribution_amounts = captured_pot_contributions
					.as_ref()
					.map(|_| flows.contribution_amounts.clone());

				let total_planned = planned_today[year] * cumulative_inflation;
				let income_towards_spending = js_min(settled.spendable_income, total_planned);
				let planned = total_planned - income_towards_spending;
				let unspent_income = settled.spendable_income - income_towards_spending;
				let history_index = match config.return_model {
					ReturnModel::Normal => None,
					ReturnModel::HistoricalBootstrap => {
						Some((random.next() * history_count as f64).floor() as usize)
					}
					ReturnModel::HistoricalBlockBootstrap => {
						let index = match previous_history_index {
							Some(previous) if random.next() < block_continue_probability => {
								(previous + 1) % history_count
							}
							_ => (random.next() * history_count as f64).floor() as usize,
						};
						previous_history_index = Some(index);
						Some(index)
					}
					ReturnModel::HistoricalSequence => {
						Some((simulation_index + year - 1) % history_count)
					}
				};

				let mut accessible_total = 0.0;
				for pot in 0..pot_count {
					if year as f64 >= plan.access_from_year[pot] {
						accessible_total += pots.balances[pot];
					}
				}

				let past_first = first_spending_year.is_some_and(|first| year > first);
				let mut rule_explanation = None;
				let mut minimum_applied = false;
				let mut withdrawal;
				if rule.kind == WithdrawalRuleType::FloorCeiling {
					if Some(year) == first_spending_year {
						floor_ceiling_anchor_rate = if accessible_total > 0.0 {
							planned / accessible_total
						} else {
							0.0
						};
						if captured_run {
							rule_explanation = Some(RuleExplanation::Anchor {
								rate: floor_ceiling_anchor_rate,
							});
						}
					}
					if planned > 0.0 && past_first {
						let unclamped = floor_ceiling_anchor_rate * accessible_total;
						let floor_amount = planned * (1.0 - rule.floor_pct);
						let ceiling_amount = planned * (1.0 + rule.ceiling_pct);
						withdrawal = clamp(unclamped, floor_amount, ceiling_amount);
						if captured_run {
							rule_explanation = Some(RuleExplanation::FloorCeiling {
								rate: floor_ceiling_anchor_rate,
								unclamped: emit(unclamped, start_deflator),
								floor: emit(floor_amount, start_deflator),
								ceiling: emit(ceiling_amount, start_deflator),
								applied: if unclamped < floor_amount {
									FloorCeilingBound::Floor
								} else if unclamped > ceiling_amount {
									FloorCeilingBound::Ceiling
								} else {
									FloorCeilingBound::Rate
								},
							});
						}
					} else {
						withdrawal = planned;
					}
				} else {
					if planned > 0.0
						&& past_first && rule.kind != WithdrawalRuleType::None
						&& accessible_total > 0.0
						&& accessible_start_by_year[year] > 0.0
					{
						let current_rate = (planned * adjustment_factor) / accessible_total;
						let previous_factor = adjustment_factor;
						let reference_rate =
							pot_funded_planned_today[year] / accessible_start_by_year[year];
						let mut action = RuleAction::None;
						match rule.kind {
							WithdrawalRuleType::Guardrails => {
								if current_rate
									> reference_rate * (1.0 + rule.preservation_trigger_pct)
								{
									adjustment_factor *= 1.0 - rule.preservation_cut_pct;
									action = RuleAction::Cut;
								} else if current_rate
									< reference_rate * (1.0 - rule.prosperity_trigger_pct)
								{
									adjustment_factor *= 1.0 + rule.prosperity_increase_pct;
									action = RuleAction::Raise;
								}
							}
							WithdrawalRuleType::Ratcheting => {
								if accessible_total
									> accessible_start_by_year[year]
										* rule.balance_threshold_multiple
								{
									ratchet_streak += 1;
									if f64::from(ratchet_streak) >= rule.consecutive_years {
										adjustment_factor *= 1.0 + rule.ratchet_increase_pct;
										ratchet_streak = 0;
										action = RuleAction::Raise;
									}
								} else {
									ratchet_streak = 0;
								}
							}
							WithdrawalRuleType::Boundaries => {
								if current_rate > rule.upper_rate_threshold {
									adjustment_factor *= 1.0 - rule.upper_cut_pct;
									action = RuleAction::Cut;
								} else if current_rate < rule.lower_rate_threshold {
									adjustment_factor *= 1.0 + rule.lower_increase_pct;
									action = RuleAction::Raise;
								}
							}
							WithdrawalRuleType::None | WithdrawalRuleType::FloorCeiling => {}
						}
						let factor_rule = match rule.kind {
							WithdrawalRuleType::Guardrails => Some(FactorRule::Guardrails),
							WithdrawalRuleType::Ratcheting => Some(FactorRule::Ratcheting),
							WithdrawalRuleType::Boundaries => Some(FactorRule::Boundaries),
							_ => None,
						};
						if let Some(factor_rule) = factor_rule.filter(|_| captured_run) {
							rule_explanation = Some(RuleExplanation::Factor {
								rule: factor_rule,
								factor: adjustment_factor,
								previous_factor,
								planned: emit(planned, start_deflator),
								adjusted: emit(planned * adjustment_factor, start_deflator),
								action,
								current_rate: (factor_rule != FactorRule::Ratcheting)
									.then_some(current_rate),
								reference_rate: (factor_rule == FactorRule::Guardrails)
									.then_some(reference_rate),
								ratchet_streak: (factor_rule == FactorRule::Ratcheting
									&& action == RuleAction::None)
									.then_some(ratchet_streak),
							});
						}
					}
					withdrawal = planned * adjustment_factor;
				}
				let minimum_this_year = minimum_spending * cumulative_inflation;
				let minimum_from_pots = js_max(0.0, minimum_this_year - income_towards_spending);
				if rule.kind != WithdrawalRuleType::None
					&& minimum_spending > 0.0
					&& planned > 0.0
					&& withdrawal < minimum_from_pots
				{
					withdrawal = minimum_from_pots;
					minimum_applied = captured_run;
				}

				let planned_spending_this_year = income_towards_spending + withdrawal;
				let year_start_total = total;
				let net_required = withdrawal;
				let withdrawal_taken;
				let net_delivered;
				let mut funding_shortfall = false;
				let mut surplus_saved_this_year = 0.0;
				let mut captured = captured_run.then(|| Captured {
					pot_contributions: captured_pot_contributions.take().unwrap_or_default(),
					contribution_amounts: captured_contribution_amounts.unwrap_or_default(),
					pot_returns: vec![None; pot_count],
					pot_withdrawals: vec![0.0; pot_count],
					pot_taxes: vec![0.0; pot_count],
					pot_taxables: vec![0.0; pot_count],
					pot_fees: vec![0.0; pot_count],
					pot_start_balances: pre_contribution_balances
						.take()
						.unwrap_or_else(|| pots.balances.clone()),
					failure_snapshot: None,
					rule_explanation: rule_explanation.take(),
					minimum_applied,
				});

				let mut accessible_net_capacity = accessible_total;
				if plan.has_tax && accessible_total * min_net_factor <= net_required {
					plan.compute_takes(&mut pots, accessible_total, year, accessible_total);
					accessible_net_capacity = accessible_total
						- plan.tax_for_takes(
							&pots.takes,
							cumulative_inflation,
							settled.taxable_income_base,
						);
				}

				if net_required > 0.0 && accessible_net_capacity < net_required {
					funding_shortfall = true;
					withdrawn_sum =
						to_safe_amount(withdrawn_sum + accessible_total * start_deflator);
					withdrawal_taken = accessible_total;
					net_delivered = js_max(0.0, accessible_net_capacity);
					if let Some(captured) = captured.as_mut() {
						let accessible = |pot: usize| year as f64 >= plan.access_from_year[pot];
						captured.failure_snapshot = Some(
							(0..pot_count)
								.map(|pot| {
									if accessible(pot) {
										0.0
									} else {
										js_round(pots.balances[pot])
									}
								})
								.collect(),
						);
						for pot in 0..pot_count {
							captured.pot_withdrawals[pot] = if accessible(pot) {
								pots.balances[pot]
							} else {
								0.0
							};
						}
						plan.attribute_tax_to_pots(
							accessible_total - net_delivered,
							&pots.takes,
							&mut captured.pot_taxes,
						);
						if plan.has_tax {
							plan.capture_taxables(&pots.takes, &mut captured.pot_taxables);
						}
					}
					pots.balances.fill(0.0);
					total = 0.0;
					depleted = true;
					simulation_depletion_year = Some(year);
					depletion_counts[year] += 1;
				} else {
					let mut gross_total = net_required;
					if plan.has_tax {
						for _ in 0..40 {
							plan.compute_takes(&mut pots, gross_total, year, accessible_total);
							let next = net_required
								+ plan.tax_for_takes(
									&pots.takes,
									cumulative_inflation,
									settled.taxable_income_base,
								);
							if (next - gross_total).abs() <= 1e-7 * js_max(1.0, next) {
								gross_total = next;
								break;
							}
							gross_total = next;
						}
						gross_total = js_min(gross_total, accessible_total);
					}
					plan.compute_takes(&mut pots, gross_total, year, accessible_total);
					for (balance, take) in pots.balances.iter_mut().zip(&pots.takes) {
						*balance -= take;
					}
					withdrawn_sum = to_safe_amount(withdrawn_sum + gross_total * start_deflator);
					withdrawal_taken = gross_total;
					net_delivered = net_required;
					if let Some(captured) = captured.as_mut() {
						captured.pot_withdrawals.copy_from_slice(&pots.takes);
						plan.attribute_tax_to_pots(
							gross_total - net_required,
							&pots.takes,
							&mut captured.pot_taxes,
						);
						if plan.has_tax {
							plan.capture_taxables(&pots.takes, &mut captured.pot_taxables);
						}
					}
					if let Some(surplus) = plan.surplus {
						surplus_saved_this_year = unspent_income;
						pots.balances[surplus] += surplus_saved_this_year;
					}

					let market_shock = if has_normal_draw[year] {
						random.next_normal()
					} else {
						0.0
					};
					total = 0.0;
					for (pot, years) in pot_years.iter().enumerate() {
						if pots.balances[pot] > 0.0 {
							let year_return = match (years.weights[year], history_index) {
								(Some(weights), Some(index)) => {
									let market = &history[index];
									weights.stocks * market.stocks
										+ weights.bonds * market.bonds + weights.cash * market.cash
								}
								_ => years.mean[year] + years.std_dev[year] * market_shock,
							};
							if let Some(captured) = captured.as_mut() {
								captured.pot_returns[pot] = Some(year_return);
							}
							pots.previous_returns[pot] = year_return;
							pots.balances[pot] *= 1.0 + year_return;
							if pots.balances[pot] <= 0.0 {
								pots.balances[pot] = 0.0;
							}
						}
						total += pots.balances[pot];
					}
				}

				let mut year_inflation_rate = None;
				if let Some(mean) = inflation_mean {
					let rate = match history_index {
						Some(index) => history[index].inflation,
						None if inflation_std_dev > 0.0 => {
							js_max(-0.9, mean + inflation_std_dev * random.next_normal())
						}
						None => mean,
					};
					cumulative_inflation *= 1.0 + rate;
					year_inflation_rate = Some(rate);
				}

				let mut fees_this_year = 0.0;
				if has_fees && !funding_shortfall && total > 0.0 {
					for (pot, years) in pot_years.iter().enumerate() {
						if pots.balances[pot] <= 0.0 {
							continue;
						}
						let fee = pots.balances[pot] * years.fee_rate[year]
							+ years.fee_fixed[year]
								* if years.fee_adjusts[year] {
									cumulative_inflation
								} else {
									1.0
								};
						let charged = js_min(pots.balances[pot], fee);
						pots.balances[pot] -= charged;
						fees_this_year += charged;
						if let Some(captured) = captured.as_mut() {
							captured.pot_fees[pot] = charged;
						}
					}
					total = 0.0;
					for balance in &pots.balances {
						total += balance;
					}
				}
				let end_deflator = if deflate {
					1.0 / cumulative_inflation
				} else {
					1.0
				};

				if let (Some(captured), Some(rows)) = (captured, run_detail.as_mut()) {
					let start_balance =
						emit(year_start_total - contributions_this_year, start_deflator);
					let contributions = emit(contributions_this_year, start_deflator);
					let withdrawal = emit(withdrawal_taken, start_deflator);
					let tax_paid = emit(withdrawal_taken - net_delivered, start_deflator);
					let income = emit(settled.gross_income, start_deflator);
					let mut row = RunDetailRow {
						year,
						start_balance,
						withdrawal,
						planned_spending: emit(planned_spending_this_year, start_deflator),
						spent: emit(income_towards_spending + net_delivered, start_deflator),
						growth: 0.0,
						end_balance: 0.0,
						pot_balances: Vec::new(),
						pot_start_balances: emit_parts(
							Some(&captured.pot_start_balances),
							start_deflator,
							start_balance,
						),
						inflation: year_inflation_rate,
						income,
						income_amounts: emit_parts(
							Some(&flows.income_gross),
							start_deflator,
							income,
						),
						income_tax: emit(settled.income_tax, start_deflator),
						unspent_income: emit(unspent_income, start_deflator),
						surplus_saved: emit(surplus_saved_this_year, start_deflator),
						contributions,
						pot_contributions: emit_parts(
							Some(&captured.pot_contributions),
							start_deflator,
							contributions,
						),
						contribution_amounts: emit_parts(
							Some(&captured.contribution_amounts),
							start_deflator,
							contributions,
						),
						pot_withdrawals: emit_parts(
							Some(&captured.pot_withdrawals),
							start_deflator,
							withdrawal,
						),
						pot_taxes: emit_parts(Some(&captured.pot_taxes), start_deflator, tax_paid),
						pot_taxables: captured
							.pot_taxables
							.iter()
							.map(|taxable| emit(*taxable, start_deflator))
							.collect(),
						tax_paid,
						fees_paid: 0.0,
						pot_fees: vec![0.0; pot_count],
						pot_returns: captured.pot_returns.clone(),
						rule_explanation: captured.rule_explanation,
						minimum_applied: captured.minimum_applied,
						inaccessible_balance: None,
						after_depletion: false,
					};
					if funding_shortfall {
						let locked = emit(year_start_total - withdrawal_taken, start_deflator);
						row.pot_balances = emit_parts(
							captured.failure_snapshot.as_deref(),
							start_deflator,
							locked,
						);
						row.inaccessible_balance = (locked > 0.0).then_some(locked);
					} else {
						row.fees_paid = emit(fees_this_year, end_deflator);
						row.end_balance = emit(total, end_deflator);
						row.growth = to_safe_amount(js_round(
							(total + fees_this_year) * end_deflator
								- (year_start_total - withdrawal_taken + surplus_saved_this_year)
									* start_deflator,
						));
						row.pot_fees =
							emit_parts(Some(&captured.pot_fees), end_deflator, row.fees_paid);
						row.pot_balances =
							emit_parts(Some(&pots.balances), end_deflator, row.end_balance);
						row.pot_returns = captured
							.pot_returns
							.iter()
							.map(|value| {
								value.map(|value| {
									(1.0 + value) * (end_deflator / start_deflator) - 1.0
								})
							})
							.collect();
					}
					rows.push(row);
				}

				balances_by_year[year][simulation_index] = to_safe_amount(total * end_deflator);
			} else if let Some(rows) = run_detail.as_mut().filter(|_| captured_run) {
				rows.push(plan.after_depletion_row(
					&mut flows,
					year,
					cumulative_inflation,
					planned_today[year],
					adjustment_factor,
				));
			}
		}

		withdrawn_totals[simulation_index] = withdrawn_sum;
		if let Some(year) = simulation_depletion_year {
			depletion_year_by_simulation[simulation_index] = year as i32;
		} else {
			survived_count += 1;
		}
		let depletion_year = simulation_depletion_year.map_or(f64::INFINITY, |year| year as f64);
		if depletion_year < worst_depletion_year
			|| (depletion_year == worst_depletion_year && total < worst_final_balance)
		{
			worst_sim_index = simulation_index;
			worst_depletion_year = depletion_year;
			worst_final_balance = total;
		}
	}

	let worst_run_path: Vec<f64> = balances_by_year
		.iter()
		.map(|year| js_round(year[worst_sim_index]))
		.collect();
	let ending_balances = balances_by_year[horizon].clone();
	let mut percentile_bands = Vec::with_capacity(horizon + 1);
	for (year, balances) in balances_by_year.iter_mut().enumerate() {
		sort_typed(balances);
		percentile_bands.push(band(year, balances));
	}
	let median_ending_balance = js_round(percentile_of_sorted(&balances_by_year[horizon], 0.5));

	let depletion_histogram = (1..=horizon)
		.map(|year| DepletionCount {
			year,
			count: depletion_counts[year],
		})
		.collect();
	let mut depletion_probability_by_year = vec![0.0];
	let mut cumulative_depleted = 0_u32;
	for count in &depletion_counts[1..] {
		cumulative_depleted += count;
		depletion_probability_by_year
			.push(f64::from(cumulative_depleted) / simulation_count as f64);
	}

	let total_depleted = (simulation_count - survived_count) as i64;
	let median_target_index = (total_depleted - 1).div_euclid(2);
	let mut earliest_depletion_year = None;
	let mut latest_depletion_year = None;
	let mut median_depletion_year = None;
	let mut seen_depleted = 0_i64;
	for (year, &count) in depletion_counts.iter().enumerate().skip(1) {
		if count == 0 {
			continue;
		}
		earliest_depletion_year.get_or_insert(year);
		latest_depletion_year = Some(year);
		if median_depletion_year.is_none() {
			seen_depleted += i64::from(count);
			if seen_depleted > median_target_index {
				median_depletion_year = Some(year);
			}
		}
	}

	let mut withdrawn_sorted = withdrawn_totals.clone();
	sort_typed(&mut withdrawn_sorted);

	Ok(MonteCarloResult {
		success_rate: survived_count as f64 / simulation_count as f64,
		percentile_bands,
		depletion_histogram,
		depletion_probability_by_year,
		median_ending_balance,
		median_total_withdrawn: js_round(percentile_of_sorted(&withdrawn_sorted, 0.5)),
		median_depletion_year,
		earliest_depletion_year,
		latest_depletion_year,
		worst_run_path,
		ending_balances,
		depletion_year_by_simulation,
		total_withdrawn_by_simulation: withdrawn_totals,
		run_detail,
		simulation_count,
		horizon_years: horizon,
	})
}

/// A captured year after the plan ran out (`monteCarloSimulation.ts:2675-2748`):
/// income still arrives at the failure year's prices and covers what it
/// can, and the rule's running adjustment still shapes the plan. It draws
/// nothing from the random stream.
impl Plan {
	fn after_depletion_row(
		&self,
		flows: &mut Flows,
		year: usize,
		cumulative_inflation: f64,
		planned_today: f64,
		adjustment_factor: f64,
	) -> RunDetailRow {
		let frozen_deflator = if self.deflate {
			1.0 / cumulative_inflation
		} else {
			1.0
		};
		let emit_frozen = |value: f64| emit(value, frozen_deflator);
		let settled = self.settle_year_flows(flows, year, cumulative_inflation, false);
		let total_planned = planned_today * cumulative_inflation;
		let income_towards_spending = js_min(settled.spendable_income, total_planned);
		let pot_funded_plan = total_planned - income_towards_spending;
		let mut adjusted_plan = pot_funded_plan * adjustment_factor;
		if self.rule != WithdrawalRuleType::None
			&& self.minimum_spending > 0.0
			&& pot_funded_plan > 0.0
		{
			adjusted_plan = js_max(
				adjusted_plan,
				self.minimum_spending * cumulative_inflation - income_towards_spending,
			);
		}
		let pot_count = self.pot_start_balances.len();
		let zeros = vec![0.0; pot_count];
		RunDetailRow {
			year,
			start_balance: 0.0,
			withdrawal: 0.0,
			planned_spending: emit_frozen(income_towards_spending + js_max(0.0, adjusted_plan)),
			spent: emit_frozen(income_towards_spending),
			growth: 0.0,
			end_balance: 0.0,
			pot_balances: zeros.clone(),
			pot_start_balances: zeros.clone(),
			inflation: None,
			income: emit_frozen(settled.gross_income),
			income_amounts: flows
				.income_gross
				.iter()
				.map(|gross| emit_frozen(*gross))
				.collect(),
			income_tax: emit_frozen(settled.income_tax),
			unspent_income: emit_frozen(settled.spendable_income - income_towards_spending),
			surplus_saved: 0.0,
			contributions: 0.0,
			pot_contributions: zeros.clone(),
			contribution_amounts: vec![0.0; flows.contribution_amounts.len()],
			pot_withdrawals: zeros.clone(),
			pot_taxes: zeros.clone(),
			pot_taxables: zeros.clone(),
			tax_paid: 0.0,
			fees_paid: 0.0,
			pot_fees: zeros,
			pot_returns: vec![None; pot_count],
			rule_explanation: None,
			minimum_applied: false,
			inaccessible_balance: None,
			after_depletion: true,
		}
	}
}
