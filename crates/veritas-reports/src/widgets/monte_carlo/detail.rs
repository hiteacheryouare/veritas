//! The year-by-year rows of one captured run, which the report page's
//! drill-in and cashflow chart read (`monteCarloSimulation.ts:1013-1161`).
//! Amounts are rounded minor units, deflated like the result's balances.

use super::js::{js_round, stable_sort};
use super::simulation::to_safe_amount;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloorCeilingBound {
	Floor,
	Ceiling,
	Rate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FactorRule {
	Guardrails,
	Ratcheting,
	Boundaries,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleAction {
	Cut,
	Raise,
	None,
}

/// How the withdrawal rule set a year's amount (`MonteCarloRuleExplanation`).
#[derive(Clone, Debug, PartialEq)]
pub enum RuleExplanation {
	/// Floor & ceiling's first spending year, which sets its rate.
	Anchor { rate: f64 },
	FloorCeiling {
		rate: f64,
		unclamped: f64,
		floor: f64,
		ceiling: f64,
		applied: FloorCeilingBound,
	},
	Factor {
		rule: FactorRule,
		factor: f64,
		previous_factor: f64,
		planned: f64,
		adjusted: f64,
		action: RuleAction,
		/// Guardrails and boundaries.
		current_rate: Option<f64>,
		/// Guardrails.
		reference_rate: Option<f64>,
		/// Ratcheting, in a year it didn't raise: the streak so far.
		ratchet_streak: Option<u32>,
	},
}

/// One simulated year of the captured run (`MonteCarloRunDetailRow`). The
/// per-pot lists follow the configured pot order.
#[derive(Clone, Debug, PartialEq)]
pub struct RunDetailRow {
	pub year: usize,
	pub start_balance: f64,
	pub withdrawal: f64,
	pub planned_spending: f64,
	pub spent: f64,
	pub growth: f64,
	pub end_balance: f64,
	pub pot_balances: Vec<f64>,
	pub pot_start_balances: Vec<f64>,
	/// The year's inflation rate; `None` with inflation off, and after
	/// depletion.
	pub inflation: Option<f64>,
	pub income: f64,
	pub income_amounts: Vec<f64>,
	pub income_tax: f64,
	pub unspent_income: f64,
	pub surplus_saved: f64,
	pub contributions: f64,
	pub pot_contributions: Vec<f64>,
	pub contribution_amounts: Vec<f64>,
	pub pot_withdrawals: Vec<f64>,
	pub pot_taxes: Vec<f64>,
	pub pot_taxables: Vec<f64>,
	pub tax_paid: f64,
	pub fees_paid: f64,
	pub pot_fees: Vec<f64>,
	/// Each pot's return that year, `None` when it held nothing.
	pub pot_returns: Vec<Option<f64>>,
	pub rule_explanation: Option<RuleExplanation>,
	pub minimum_applied: bool,
	/// Money left in locked pots in the year the plan failed.
	pub inaccessible_balance: Option<f64>,
	/// A year after the plan ran out, kept so the cashflow chart can show
	/// what the plan still called for.
	pub after_depletion: bool,
}

pub(crate) fn emit(value: f64, deflator: f64) -> f64 {
	to_safe_amount(js_round(value * deflator))
}

/// `emitParts` (`monteCarloSimulation.ts:2534-2565`): floors each part, then
/// hands the cents left over to the parts that lost the most, so the parts
/// add up to the row's rounded `target`.
pub(crate) fn emit_parts(values: Option<&[f64]>, deflator: f64, target: f64) -> Vec<f64> {
	let Some(values) = values else {
		return Vec::new();
	};
	let scaled: Vec<f64> = values.iter().map(|value| value * deflator).collect();
	let mut parts: Vec<f64> = scaled.iter().map(|value| value.floor()).collect();
	let shortfall = target - parts.iter().fold(0.0, |sum, part| sum + part);
	if shortfall < 0.0 || shortfall > parts.len() as f64 {
		return values.iter().map(|value| emit(*value, deflator)).collect();
	}
	let mut order: Vec<usize> = (0..parts.len()).collect();
	stable_sort(&mut order, |&a, &b| {
		let by_remainder = scaled[b] - parts[b] - (scaled[a] - parts[a]);
		if by_remainder != 0.0 && !by_remainder.is_nan() {
			by_remainder
		} else {
			a as f64 - b as f64
		}
	});
	let mut cent = 0;
	while (cent as f64) < shortfall {
		parts[order[cent]] += 1.0;
		cent += 1;
	}
	parts.into_iter().map(to_safe_amount).collect()
}
