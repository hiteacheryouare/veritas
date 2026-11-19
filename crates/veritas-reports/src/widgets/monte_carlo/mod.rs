//! The Monte Carlo card (`reports/monte-carlo/MonteCarloCard.tsx`): thousands
//! of seeded runs of a retirement drawdown, summarized as the share that
//! lasted and a fan chart of balances in today's money. The engine is the
//! one the full report page runs, so the whole simulation result is here.

mod config;
mod detail;
mod fdlibm;
mod history;
mod investment;
mod js;
mod simulation;

pub use config::{
	AllocationPreset, DEFAULT_HISTORICAL_BLOCK_LENGTH, InvestmentTransition, MonteCarloConfig,
	MonteCarloContribution, MonteCarloIncomeStream, MonteCarloInvestmentFields,
	MonteCarloInvestmentPhase, MonteCarloPot, MonteCarloSpendingPhase, MonteCarloTaxBand,
	MonteCarloWithdrawalRule, ReturnModel, TaxModel, WITHDRAWAL_RULE_DEFAULTS, WithdrawalRuleType,
	WithdrawalStrategy, monte_carlo_config_from_meta,
};
pub use detail::{FactorRule, FloorCeilingBound, RuleAction, RuleExplanation, RunDetailRow};
pub use history::{HISTORICAL_ANNUAL_RETURNS, HistoricalYear};
pub use investment::{AssetWeights, PotInvestment, pot_asset_weights, pot_investment_at_age};
pub use simulation::{
	DEFAULT_SIMULATION_SEED, DepletionCount, MAX_AMOUNT, MAX_ANNUAL_FEE_RATE,
	MAX_HISTORICAL_BLOCK_LENGTH, MAX_HORIZON_YEARS, MAX_SIMULATION_COUNT, MAX_TAX_BAND_RATE,
	MAX_WITHDRAWAL_TAX_RATE, MIN_HISTORICAL_BLOCK_LENGTH, MIN_HORIZON_YEARS, MIN_SIMULATION_COUNT,
	MonteCarloParams, MonteCarloResult, PercentileBand, monte_carlo_horizon_years,
	run_monte_carlo_simulation,
};

use veritas_domain::{AccountId, BudgetData, CoreError, Money, SplitRole};

use crate::json::Json;
use js::{js_max, js_round};

/// What the card shows, plus the run it summarizes.
#[derive(Clone, Debug, PartialEq)]
pub struct MonteCarloCard {
	pub result: MonteCarloResult,
	/// `Math.round(successRate * 1000) / 10`: the headline, to one decimal.
	pub success_percent: f64,
	/// The configured current age, where the chart starts.
	pub start_age: f64,
	/// The age the success rate runs to.
	pub end_age: f64,
	/// The fan chart, one point per year from the start.
	pub chart: Vec<MonteCarloChartPoint>,
}

/// One year of the fan chart (`MonteCarloGraph.tsx:53-67`). The card shades
/// p10–p90 and p25–p75 and draws p50; its tooltip lists p90, p75, p50, p25
/// and p10.
#[derive(Clone, Debug, PartialEq)]
pub struct MonteCarloChartPoint {
	/// 0 is the start.
	pub year: usize,
	/// The current age plus `year`; ages needn't be whole.
	pub age: f64,
	/// Whole minor units, or NaN or infinite when a run's balance is, which
	/// Actual draws as it is.
	pub p5: f64,
	pub p10: f64,
	pub p25: f64,
	pub p30: f64,
	pub p50: f64,
	pub p70: f64,
	pub p75: f64,
	pub p90: f64,
}

/// `accountBalance` (`desktop-client/src/spreadsheet/bindings.ts:23-31`), the
/// balance `useResolvedMonteCarloConfig` reads for a linked pot: every live
/// row of the account that isn't a split's child, whatever its date. A
/// missing or deleted account sums to 0, which AQL's `calculate` returns
/// for no rows.
fn account_balance(budget: &BudgetData, account: &str) -> Result<Money, CoreError> {
	let id = AccountId::new(account);
	if budget
		.accounts
		.get(&id)
		.is_none_or(|account| account.tombstone)
	{
		return Ok(Money::ZERO);
	}
	Money::checked_sum(
		budget
			.transactions
			.values()
			.filter(|row| {
				row.account == id && !row.tombstone && !matches!(row.split, SplitRole::Child { .. })
			})
			.map(|row| row.amount),
	)
}

/// The settings' amounts, which Actual's editor saves at the currency's
/// places, moved to the register's scale so they add up with a linked
/// account's balance (D74). Rates, ages and counts don't change.
fn to_register_scale(config: &mut MonteCarloConfig, factor: f64) {
	if factor == 1.0 {
		return;
	}
	for pot in &mut config.pots {
		pot.starting_balance *= factor;
		pot.investment.annual_fee_fixed *= factor;
		for phase in &mut pot.investment_phases {
			phase.investment.annual_fee_fixed *= factor;
		}
	}
	config.minimum_spending *= factor;
	for phase in &mut config.spending_phases {
		phase.annual_withdrawal *= factor;
	}
	for contribution in &mut config.contributions {
		contribution.annual_amount *= factor;
	}
	for stream in &mut config.income_streams {
		stream.annual_amount *= factor;
	}
	for band in &mut config.tax_bands {
		band.from *= factor;
	}
}

/// The card's headline: `Math.round(successRate * 1000) / 10`, a percent
/// to one decimal (`MonteCarloCard.tsx:61`).
pub fn success_percent(success_rate: f64) -> f64 {
	js_round(success_rate * 1000.0) / 10.0
}

/// The Monte Carlo card for its meta. A pot linked to an account starts at
/// that account's balance, or 0 when it's negative
/// (`useResolvedMonteCarloConfig.ts`). The card always deflates to today's
/// money and uses the default seed.
pub fn monte_carlo_card(
	budget: &BudgetData,
	meta: Option<&Json>,
) -> Result<MonteCarloCard, CoreError> {
	let mut config = monte_carlo_config_from_meta(meta)?;
	to_register_scale(&mut config, super::register_factor(budget));
	for pot in &mut config.pots {
		if let Some(account) = &pot.account_id {
			pot.starting_balance = js_max(0.0, account_balance(budget, account)?.minor() as f64);
		}
	}
	let current_age = config.current_age;
	let horizon_years = monte_carlo_horizon_years(&config);
	let result = run_monte_carlo_simulation(&MonteCarloParams {
		config,
		horizon_years,
		seed: DEFAULT_SIMULATION_SEED,
		capture_run_detail: None,
		deflate_to_todays_money: true,
	})?;
	let chart = result
		.percentile_bands
		.iter()
		.map(|band| MonteCarloChartPoint {
			year: band.year,
			age: current_age + band.year as f64,
			p5: band.p5,
			p10: band.p10,
			p25: band.p25,
			p30: band.p30,
			p50: band.p50,
			p70: band.p70,
			p75: band.p75,
			p90: band.p90,
		})
		.collect();
	Ok(MonteCarloCard {
		success_percent: success_percent(result.success_rate),
		start_age: current_age,
		end_age: current_age + result.horizon_years as f64,
		chart,
		result,
	})
}
