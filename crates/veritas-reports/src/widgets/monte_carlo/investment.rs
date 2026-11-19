//! What a pot is invested in at each age (`monteCarloSimulation.ts:62-331`).

use super::config::{
	AllocationPreset, InvestmentTransition, MonteCarloInvestmentFields, MonteCarloInvestmentPhase,
	MonteCarloPot, preset_weights,
};
use super::js::{clamp, js_max, stable_sort};
use super::simulation::{MAX_AMOUNT, MAX_ANNUAL_FEE_RATE};

/// Relative shares of each asset class (`MonteCarloAssetWeights`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AssetWeights {
	pub stocks: f64,
	pub bonds: f64,
	pub cash: f64,
}

/// What a pot is invested in, and what it costs, in one year
/// (`MonteCarloPotInvestment`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PotInvestment {
	/// The mix the historical models blend; `None` draws normally.
	pub weights: Option<AssetWeights>,
	pub mean: f64,
	pub std_dev: f64,
	pub fee_rate: f64,
	pub fee_fixed: f64,
	pub fee_adjusts: bool,
}

/// `getPotAssetWeights` (`monteCarloSimulation.ts:62-90`): `None` for a
/// custom pot or a custom mix that doesn't total 100%.
pub fn pot_asset_weights(fields: &MonteCarloInvestmentFields) -> Option<AssetWeights> {
	match fields.allocation_preset {
		AllocationPreset::Custom => None,
		AllocationPreset::CustomMix => {
			let sanitize = |share: f64| {
				if share.is_finite() && share > 0.0 {
					share
				} else {
					0.0
				}
			};
			let stocks = sanitize(fields.allocation_stocks);
			let bonds = sanitize(fields.allocation_bonds);
			let cash = sanitize(fields.allocation_cash);
			let total = stocks + bonds + cash;
			((total - 1.0).abs() < 1e-9).then(|| AssetWeights {
				stocks: stocks / total,
				bonds: bonds / total,
				cash: cash / total,
			})
		}
		preset => preset_weights(preset).map(|[stocks, bonds, cash]| AssetWeights {
			stocks,
			bonds,
			cash,
		}),
	}
}

fn investment(fields: &MonteCarloInvestmentFields) -> PotInvestment {
	PotInvestment {
		weights: pot_asset_weights(fields),
		mean: fields.expected_return_mean,
		std_dev: js_max(0.0, fields.return_std_dev),
		fee_rate: clamp(fields.annual_fee_rate, 0.0, MAX_ANNUAL_FEE_RATE),
		fee_fixed: clamp(fields.annual_fee_fixed, 0.0, MAX_AMOUNT),
		fee_adjusts: fields.fee_adjusts_with_inflation,
	}
}

fn lerp(from: f64, to: f64, fraction: f64) -> f64 {
	from + (to - from) * fraction
}

fn lerp_investment(from: &PotInvestment, to: &PotInvestment, fraction: f64) -> PotInvestment {
	PotInvestment {
		weights: match (from.weights, to.weights) {
			(Some(from), Some(to)) => Some(AssetWeights {
				stocks: lerp(from.stocks, to.stocks, fraction),
				bonds: lerp(from.bonds, to.bonds, fraction),
				cash: lerp(from.cash, to.cash, fraction),
			}),
			_ => None,
		},
		mean: lerp(from.mean, to.mean, fraction),
		std_dev: lerp(from.std_dev, to.std_dev, fraction),
		fee_rate: lerp(from.fee_rate, to.fee_rate, fraction),
		fee_fixed: lerp(from.fee_fixed, to.fee_fixed, fraction),
		fee_adjusts: from.fee_adjusts,
	}
}

fn phases_by_age(
	pot: &MonteCarloPot,
	keep: impl Fn(f64) -> bool,
) -> Vec<&MonteCarloInvestmentPhase> {
	let mut phases: Vec<_> = pot
		.investment_phases
		.iter()
		.filter(|phase| phase.from_age.is_finite() && keep(phase.from_age))
		.collect();
	stable_sort(&mut phases, |a, b| a.from_age - b.from_age);
	phases
}

/// `getPotInvestmentAtAge` (`monteCarloSimulation.ts:292-331`): the pot's own
/// fields (or the latest phase already reached) until a later phase takes
/// over, gliding toward a gradual phase from the previous phase's start.
pub fn pot_investment_at_age(pot: &MonteCarloPot, current_age: f64, age: f64) -> PotInvestment {
	let reached = phases_by_age(pot, |from| from <= current_age);
	let base = reached
		.last()
		.map_or(&pot.investment, |phase| &phase.investment);
	let phases = phases_by_age(pot, |from| from > current_age);
	let active_index = phases
		.iter()
		.take_while(|phase| phase.from_age <= age)
		.count();
	let (active, active_start) = match active_index.checked_sub(1) {
		None => (investment(base), current_age),
		Some(index) => (
			investment(&phases[index].investment),
			phases[index].from_age,
		),
	};
	match phases.get(active_index) {
		Some(next)
			if next.transition == InvestmentTransition::Gradual && next.from_age > active_start =>
		{
			let fraction = clamp(
				(age - active_start) / (next.from_age - active_start),
				0.0,
				1.0,
			);
			lerp_investment(&active, &investment(&next.investment), fraction)
		}
		_ => active,
	}
}
