//! The card's settings, read from its meta the way `monteCarloConfigFromMeta`
//! reads them (`monte-carlo/monteCarloSimulation.ts:368-990`). Amounts are
//! minor units in today's money and rates are decimal fractions, as Actual
//! stores them. A value of a type Actual never writes is unsupported, since
//! JavaScript would coerce it in ways not worth copying.

use veritas_domain::CoreError;

use crate::json::Json;
use crate::report_query::unsupported;

pub const DEFAULT_HISTORICAL_BLOCK_LENGTH: f64 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AllocationPreset {
	Equity100,
	Equity80,
	Equity60,
	Equity40,
	Cash,
	CustomMix,
	Custom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WithdrawalStrategy {
	Proportional,
	Sequential,
	BestPerformer,
	TargetMix,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReturnModel {
	Normal,
	HistoricalBootstrap,
	HistoricalBlockBootstrap,
	HistoricalSequence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WithdrawalRuleType {
	None,
	Guardrails,
	Ratcheting,
	FloorCeiling,
	Boundaries,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaxModel {
	Flat,
	Bands,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvestmentTransition {
	AtOnce,
	Gradual,
}

/// What a pot is invested in and what it costs (`MonteCarloInvestmentFields`).
#[derive(Clone, Debug, PartialEq)]
pub struct MonteCarloInvestmentFields {
	pub allocation_preset: AllocationPreset,
	pub allocation_stocks: f64,
	pub allocation_bonds: f64,
	pub allocation_cash: f64,
	pub expected_return_mean: f64,
	pub return_std_dev: f64,
	pub annual_fee_fixed: f64,
	pub fee_adjusts_with_inflation: bool,
	pub annual_fee_rate: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MonteCarloInvestmentPhase {
	pub id: String,
	pub from_age: f64,
	pub transition: InvestmentTransition,
	pub investment: MonteCarloInvestmentFields,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MonteCarloPot {
	pub id: String,
	pub name: String,
	pub starting_balance: f64,
	pub investment: MonteCarloInvestmentFields,
	pub access_age: Option<f64>,
	/// The account whose balance replaces `starting_balance` when the card
	/// runs.
	pub account_id: Option<String>,
	pub withdrawal_tax_rate: f64,
	pub taxable_fraction: f64,
	pub is_surplus: bool,
	pub investment_phases: Vec<MonteCarloInvestmentPhase>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MonteCarloSpendingPhase {
	pub id: String,
	pub name: String,
	/// `None` starts immediately.
	pub from_age: Option<f64>,
	pub annual_withdrawal: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MonteCarloContribution {
	pub id: String,
	pub name: String,
	pub pot_id: String,
	pub from_age: Option<f64>,
	pub to_age: Option<f64>,
	pub annual_amount: f64,
	pub adjusts_with_inflation: bool,
	pub source_income_stream_id: Option<String>,
	pub before_tax: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MonteCarloIncomeStream {
	pub id: String,
	pub name: String,
	pub from_age: Option<f64>,
	pub to_age: Option<f64>,
	pub annual_amount: f64,
	pub adjusts_with_inflation: bool,
	pub tax_rate: f64,
	pub taxable_fraction: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MonteCarloTaxBand {
	pub id: String,
	pub from: f64,
	pub rate: f64,
}

/// Every rule's settings side by side; `kind` picks the active one
/// (`MonteCarloWithdrawalRuleConfig`).
#[derive(Clone, Debug, PartialEq)]
pub struct MonteCarloWithdrawalRule {
	pub kind: WithdrawalRuleType,
	pub prosperity_trigger_pct: f64,
	pub prosperity_increase_pct: f64,
	pub preservation_trigger_pct: f64,
	pub preservation_cut_pct: f64,
	pub balance_threshold_multiple: f64,
	pub consecutive_years: f64,
	pub ratchet_increase_pct: f64,
	pub floor_pct: f64,
	pub ceiling_pct: f64,
	pub upper_rate_threshold: f64,
	pub upper_cut_pct: f64,
	pub lower_rate_threshold: f64,
	pub lower_increase_pct: f64,
}

/// `WITHDRAWAL_RULE_DEFAULTS` (`monteCarloSimulation.ts:723-742`).
pub const WITHDRAWAL_RULE_DEFAULTS: MonteCarloWithdrawalRule = MonteCarloWithdrawalRule {
	kind: WithdrawalRuleType::None,
	prosperity_trigger_pct: 0.2,
	prosperity_increase_pct: 0.1,
	preservation_trigger_pct: 0.2,
	preservation_cut_pct: 0.1,
	balance_threshold_multiple: 1.5,
	consecutive_years: 3.0,
	ratchet_increase_pct: 0.05,
	floor_pct: 0.15,
	ceiling_pct: 0.2,
	upper_rate_threshold: 0.06,
	upper_cut_pct: 0.1,
	lower_rate_threshold: 0.04,
	lower_increase_pct: 0.05,
};

/// `MonteCarloConfig`.
#[derive(Clone, Debug, PartialEq)]
pub struct MonteCarloConfig {
	pub pots: Vec<MonteCarloPot>,
	pub withdrawal_strategy: WithdrawalStrategy,
	pub return_model: ReturnModel,
	pub historical_block_length: f64,
	pub withdrawal_rule: MonteCarloWithdrawalRule,
	pub minimum_spending: f64,
	pub spending_phases: Vec<MonteCarloSpendingPhase>,
	pub contributions: Vec<MonteCarloContribution>,
	pub income_streams: Vec<MonteCarloIncomeStream>,
	/// `None` keeps spending flat.
	pub inflation_mean: Option<f64>,
	pub inflation_std_dev: f64,
	pub tax_model: TaxModel,
	pub tax_bands: Vec<MonteCarloTaxBand>,
	pub current_age: f64,
	pub target_age: f64,
	pub simulation_count: f64,
}

impl Default for MonteCarloConfig {
	/// `MONTE_CARLO_DEFAULTS` (`monteCarloSimulation.ts:783-803`).
	fn default() -> Self {
		MonteCarloConfig {
			pots: default_pots(),
			withdrawal_strategy: WithdrawalStrategy::Proportional,
			return_model: ReturnModel::Normal,
			historical_block_length: DEFAULT_HISTORICAL_BLOCK_LENGTH,
			withdrawal_rule: WITHDRAWAL_RULE_DEFAULTS,
			minimum_spending: 0.0,
			spending_phases: vec![spending_phase("phase-1".into())],
			contributions: Vec::new(),
			income_streams: Vec::new(),
			inflation_mean: Some(0.025),
			inflation_std_dev: 0.02,
			tax_model: TaxModel::Flat,
			tax_bands: vec![tax_band("band-1".into())],
			current_age: 60.0,
			target_age: 90.0,
			simulation_count: 5000.0,
		}
	}
}

pub(crate) fn default_pots() -> Vec<MonteCarloPot> {
	vec![surplus_pot("surplus-pot".into()), pot("pot-1".into())]
}

/// `PRESET_ASSET_WEIGHTS` as stocks, bonds and cash, for the five fixed mixes.
pub(crate) fn preset_weights(preset: AllocationPreset) -> Option<[f64; 3]> {
	match preset {
		AllocationPreset::Equity100 => Some([1.0, 0.0, 0.0]),
		AllocationPreset::Equity80 => Some([0.8, 0.2, 0.0]),
		AllocationPreset::Equity60 => Some([0.6, 0.4, 0.0]),
		AllocationPreset::Equity40 => Some([0.4, 0.6, 0.0]),
		AllocationPreset::Cash => Some([0.0, 0.0, 1.0]),
		AllocationPreset::CustomMix | AllocationPreset::Custom => None,
	}
}

/// `createMonteCarloPot`.
pub(crate) fn pot(id: String) -> MonteCarloPot {
	MonteCarloPot {
		id,
		name: String::new(),
		starting_balance: 50_000_000.0,
		investment: MonteCarloInvestmentFields {
			allocation_preset: AllocationPreset::Equity60,
			allocation_stocks: 0.6,
			allocation_bonds: 0.4,
			allocation_cash: 0.0,
			expected_return_mean: 0.06,
			return_std_dev: 0.1,
			annual_fee_fixed: 0.0,
			fee_adjusts_with_inflation: false,
			annual_fee_rate: 0.0,
		},
		access_age: None,
		account_id: None,
		withdrawal_tax_rate: 0.0,
		taxable_fraction: 1.0,
		is_surplus: false,
		investment_phases: Vec::new(),
	}
}

/// `createMonteCarloSurplusPot`: empty cash, untaxed and fee-free.
pub(crate) fn surplus_pot(id: String) -> MonteCarloPot {
	let mut pot = pot(id);
	pot.starting_balance = 0.0;
	pot.investment.allocation_preset = AllocationPreset::Cash;
	pot.investment.allocation_stocks = 0.0;
	pot.investment.allocation_bonds = 0.0;
	pot.investment.allocation_cash = 1.0;
	pot.investment.expected_return_mean = 0.03;
	pot.investment.return_std_dev = 0.015;
	pot.taxable_fraction = 0.0;
	pot.is_surplus = true;
	pot
}

fn spending_phase(id: String) -> MonteCarloSpendingPhase {
	MonteCarloSpendingPhase {
		id,
		name: String::new(),
		from_age: None,
		annual_withdrawal: 2_000_000.0,
	}
}

fn tax_band(id: String) -> MonteCarloTaxBand {
	MonteCarloTaxBand {
		id,
		from: 0.0,
		rate: 0.0,
	}
}

fn invalid(what: &str, value: &Json) -> CoreError {
	unsupported(format!("a Monte Carlo {what} of {}", value.stringify()))
}

/// The value at `key`, with `null` read as missing, as `??` reads it.
fn present<'a>(object: &'a Json, key: &str) -> Option<&'a Json> {
	object.get(key).filter(|value| **value != Json::Null)
}

fn number(object: &Json, key: &str) -> Result<Option<f64>, CoreError> {
	present(object, key)
		.map(|value| value.as_f64().ok_or_else(|| invalid(key, value)))
		.transpose()
}

fn boolean(object: &Json, key: &str) -> Result<Option<bool>, CoreError> {
	present(object, key)
		.map(|value| value.as_bool().ok_or_else(|| invalid(key, value)))
		.transpose()
}

fn text(object: &Json, key: &str) -> Result<Option<String>, CoreError> {
	present(object, key)
		.map(|value| {
			value
				.as_str()
				.map(str::to_owned)
				.ok_or_else(|| invalid(key, value))
		})
		.transpose()
}

/// `value.id || fallback`.
fn id(object: &Json, fallback: impl FnOnce() -> String) -> Result<String, CoreError> {
	Ok(text(object, "id")?
		.filter(|id| !id.is_empty())
		.unwrap_or_else(fallback))
}

/// The list at `key`, or `None` when it's missing or empty, which the
/// card reads as "use the default".
fn list<'a>(object: &'a Json, key: &str) -> Result<Option<&'a [Json]>, CoreError> {
	match present(object, key) {
		None => Ok(None),
		Some(value) => {
			let items = value.as_array().ok_or_else(|| invalid(key, value))?;
			Ok((!items.is_empty()).then_some(items))
		}
	}
}

fn object<'a>(value: &'a Json, what: &str) -> Result<&'a Json, CoreError> {
	match value {
		Json::Object(_) => Ok(value),
		_ => Err(invalid(what, value)),
	}
}

fn choice<T: Copy>(
	object: &Json,
	key: &str,
	options: &[(&str, T)],
	default: T,
) -> Result<T, CoreError> {
	match present(object, key) {
		None => Ok(default),
		Some(value) => value
			.as_str()
			.and_then(|name| options.iter().find(|(option, _)| *option == name))
			.map(|(_, option)| *option)
			.ok_or_else(|| invalid(key, value)),
	}
}

const PRESETS: [(&str, AllocationPreset); 7] = [
	("equity-100", AllocationPreset::Equity100),
	("equity-80", AllocationPreset::Equity80),
	("equity-60", AllocationPreset::Equity60),
	("equity-40", AllocationPreset::Equity40),
	("cash", AllocationPreset::Cash),
	("custom-mix", AllocationPreset::CustomMix),
	("custom", AllocationPreset::Custom),
];

/// Each investment field from `meta`, or `base`'s.
fn investment_fields(
	meta: &Json,
	base: &MonteCarloInvestmentFields,
) -> Result<MonteCarloInvestmentFields, CoreError> {
	Ok(MonteCarloInvestmentFields {
		allocation_preset: choice(meta, "allocationPreset", &PRESETS, base.allocation_preset)?,
		allocation_stocks: number(meta, "allocationStocks")?.unwrap_or(base.allocation_stocks),
		allocation_bonds: number(meta, "allocationBonds")?.unwrap_or(base.allocation_bonds),
		allocation_cash: number(meta, "allocationCash")?.unwrap_or(base.allocation_cash),
		expected_return_mean: number(meta, "expectedReturnMean")?
			.unwrap_or(base.expected_return_mean),
		return_std_dev: number(meta, "returnStdDev")?.unwrap_or(base.return_std_dev),
		annual_fee_fixed: number(meta, "annualFeeFixed")?.unwrap_or(base.annual_fee_fixed),
		fee_adjusts_with_inflation: boolean(meta, "feeAdjustsWithInflation")?
			.unwrap_or(base.fee_adjusts_with_inflation),
		annual_fee_rate: number(meta, "annualFeeRate")?.unwrap_or(base.annual_fee_rate),
	})
}

/// `potFromMeta` (`monteCarloSimulation.ts:846-880`). A phase without a
/// numeric start age is dropped, as `hasStartAge` drops it.
fn pot_from_meta(meta: &Json, index: usize) -> Result<MonteCarloPot, CoreError> {
	let meta = object(meta, "pot")?;
	let defaults = pot(id(meta, || format!("pot-{}", index + 1))?);
	let investment = investment_fields(meta, &defaults.investment)?;
	let mut phases = Vec::new();
	for phase in list(meta, "investmentPhases")?.unwrap_or_default() {
		let phase = object(phase, "investment phase")?;
		let Some(from_age) = phase
			.get("fromAge")
			.and_then(Json::as_f64)
			.filter(|age| age.is_finite())
		else {
			continue;
		};
		let index = phases.len();
		phases.push(MonteCarloInvestmentPhase {
			id: id(phase, || format!("phase-{}", index + 1))?,
			from_age,
			transition: choice(
				phase,
				"transition",
				&[
					("at-once", InvestmentTransition::AtOnce),
					("gradual", InvestmentTransition::Gradual),
				],
				InvestmentTransition::AtOnce,
			)?,
			investment: investment_fields(phase, &investment)?,
		});
	}
	Ok(MonteCarloPot {
		name: text(meta, "name")?.unwrap_or_default(),
		starting_balance: number(meta, "startingBalance")?.unwrap_or(defaults.starting_balance),
		investment,
		access_age: number(meta, "accessAge")?,
		account_id: text(meta, "accountId")?,
		withdrawal_tax_rate: number(meta, "withdrawalTaxRate")?
			.unwrap_or(defaults.withdrawal_tax_rate),
		taxable_fraction: number(meta, "taxableFraction")?.unwrap_or(defaults.taxable_fraction),
		is_surplus: boolean(meta, "isSurplus")?.unwrap_or(defaults.is_surplus),
		investment_phases: phases,
		..defaults
	})
}

/// `{ ...WITHDRAWAL_RULE_DEFAULTS, ...meta.withdrawalRule }`: the spread
/// copies an explicit `null` too, which the rule's arithmetic reads as 0.
fn withdrawal_rule(meta: Option<&Json>) -> Result<MonteCarloWithdrawalRule, CoreError> {
	let defaults = WITHDRAWAL_RULE_DEFAULTS;
	let Some(meta) = meta.filter(|meta| **meta != Json::Null) else {
		return Ok(defaults);
	};
	let meta = object(meta, "withdrawal rule")?;
	let field = |key: &str, default: f64| match meta.get(key) {
		None => Ok(default),
		Some(Json::Null) => Ok(0.0),
		Some(value) => value.as_f64().ok_or_else(|| invalid(key, value)),
	};
	let kind = match meta.get("type") {
		None => defaults.kind,
		Some(value) => match value.as_str() {
			Some("none") => WithdrawalRuleType::None,
			Some("guardrails") => WithdrawalRuleType::Guardrails,
			Some("ratcheting") => WithdrawalRuleType::Ratcheting,
			Some("floor-ceiling") => WithdrawalRuleType::FloorCeiling,
			Some("boundaries") => WithdrawalRuleType::Boundaries,
			_ => return Err(invalid("withdrawal rule type", value)),
		},
	};
	Ok(MonteCarloWithdrawalRule {
		kind,
		prosperity_trigger_pct: field("prosperityTriggerPct", defaults.prosperity_trigger_pct)?,
		prosperity_increase_pct: field("prosperityIncreasePct", defaults.prosperity_increase_pct)?,
		preservation_trigger_pct: field(
			"preservationTriggerPct",
			defaults.preservation_trigger_pct,
		)?,
		preservation_cut_pct: field("preservationCutPct", defaults.preservation_cut_pct)?,
		balance_threshold_multiple: field(
			"balanceThresholdMultiple",
			defaults.balance_threshold_multiple,
		)?,
		consecutive_years: field("consecutiveYears", defaults.consecutive_years)?,
		ratchet_increase_pct: field("ratchetIncreasePct", defaults.ratchet_increase_pct)?,
		floor_pct: field("floorPct", defaults.floor_pct)?,
		ceiling_pct: field("ceilingPct", defaults.ceiling_pct)?,
		upper_rate_threshold: field("upperRateThreshold", defaults.upper_rate_threshold)?,
		upper_cut_pct: field("upperCutPct", defaults.upper_cut_pct)?,
		lower_rate_threshold: field("lowerRateThreshold", defaults.lower_rate_threshold)?,
		lower_increase_pct: field("lowerIncreasePct", defaults.lower_increase_pct)?,
	})
}

fn each<T>(
	items: Option<&[Json]>,
	what: &str,
	read: impl Fn(&Json, usize) -> Result<T, CoreError>,
) -> Result<Option<Vec<T>>, CoreError> {
	items
		.map(|items| {
			items
				.iter()
				.enumerate()
				.map(|(index, item)| read(object(item, what)?, index))
				.collect()
		})
		.transpose()
}

/// `monteCarloConfigFromMeta` (`monteCarloSimulation.ts:951-990`).
pub fn monte_carlo_config_from_meta(meta: Option<&Json>) -> Result<MonteCarloConfig, CoreError> {
	let defaults = MonteCarloConfig::default();
	let Some(meta) = meta.filter(|meta| **meta != Json::Null) else {
		return Ok(defaults);
	};
	let meta = object(meta, "meta")?;
	let spending_phases = each(
		list(meta, "spendingPhases")?,
		"spending phase",
		|phase, index| {
			let defaults = spending_phase(id(phase, || format!("phase-{}", index + 1))?);
			Ok(MonteCarloSpendingPhase {
				name: text(phase, "name")?.unwrap_or_default(),
				from_age: number(phase, "fromAge")?,
				annual_withdrawal: number(phase, "annualWithdrawal")?
					.unwrap_or(defaults.annual_withdrawal),
				..defaults
			})
		},
	)?;
	let contributions = each(
		list(meta, "contributions")?,
		"contribution",
		|item, index| {
			Ok(MonteCarloContribution {
				id: id(item, || format!("contribution-{}", index + 1))?,
				name: text(item, "name")?.unwrap_or_default(),
				pot_id: text(item, "potId")?.unwrap_or_default(),
				from_age: number(item, "fromAge")?,
				to_age: number(item, "toAge")?,
				annual_amount: number(item, "annualAmount")?.unwrap_or(1_000_000.0),
				adjusts_with_inflation: boolean(item, "adjustsWithInflation")?.unwrap_or(true),
				source_income_stream_id: text(item, "sourceIncomeStreamId")?,
				before_tax: boolean(item, "beforeTax")?.unwrap_or(false),
			})
		},
	)?;
	let income_streams = each(
		list(meta, "incomeStreams")?,
		"income stream",
		|item, index| {
			Ok(MonteCarloIncomeStream {
				id: id(item, || format!("income-{}", index + 1))?,
				name: text(item, "name")?.unwrap_or_default(),
				from_age: number(item, "fromAge")?,
				to_age: number(item, "toAge")?,
				annual_amount: number(item, "annualAmount")?.unwrap_or(1_000_000.0),
				adjusts_with_inflation: boolean(item, "adjustsWithInflation")?.unwrap_or(true),
				tax_rate: number(item, "taxRate")?.unwrap_or(0.0),
				taxable_fraction: number(item, "taxableFraction")?.unwrap_or(1.0),
			})
		},
	)?;
	let tax_bands = each(list(meta, "taxBands")?, "tax band", |band, index| {
		let defaults = tax_band(id(band, || format!("band-{}", index + 1))?);
		Ok(MonteCarloTaxBand {
			from: number(band, "from")?.unwrap_or(defaults.from),
			rate: number(band, "rate")?.unwrap_or(defaults.rate),
			..defaults
		})
	})?;
	let inflation_mean = match meta.get("inflationMean") {
		None => defaults.inflation_mean,
		Some(Json::Null) => None,
		Some(value) => Some(
			value
				.as_f64()
				.ok_or_else(|| invalid("inflationMean", value))?,
		),
	};
	Ok(MonteCarloConfig {
		pots: each(list(meta, "pots")?, "pot", pot_from_meta)?.unwrap_or(defaults.pots),
		withdrawal_strategy: choice(
			meta,
			"withdrawalStrategy",
			&[
				("proportional", WithdrawalStrategy::Proportional),
				("sequential", WithdrawalStrategy::Sequential),
				("best-performer", WithdrawalStrategy::BestPerformer),
				("target-mix", WithdrawalStrategy::TargetMix),
			],
			defaults.withdrawal_strategy,
		)?,
		return_model: choice(
			meta,
			"returnModel",
			&[
				("normal", ReturnModel::Normal),
				("historical-bootstrap", ReturnModel::HistoricalBootstrap),
				(
					"historical-block-bootstrap",
					ReturnModel::HistoricalBlockBootstrap,
				),
				("historical-sequence", ReturnModel::HistoricalSequence),
			],
			defaults.return_model,
		)?,
		historical_block_length: number(meta, "historicalBlockLength")?
			.unwrap_or(defaults.historical_block_length),
		withdrawal_rule: withdrawal_rule(meta.get("withdrawalRule"))?,
		minimum_spending: number(meta, "minimumSpending")?.unwrap_or(defaults.minimum_spending),
		spending_phases: spending_phases.unwrap_or(defaults.spending_phases),
		contributions: contributions.unwrap_or_default(),
		income_streams: income_streams.unwrap_or_default(),
		inflation_mean,
		inflation_std_dev: number(meta, "inflationStdDev")?.unwrap_or(defaults.inflation_std_dev),
		tax_model: choice(
			meta,
			"taxModel",
			&[("flat", TaxModel::Flat), ("bands", TaxModel::Bands)],
			defaults.tax_model,
		)?,
		tax_bands: tax_bands.unwrap_or(defaults.tax_bands),
		current_age: number(meta, "currentAge")?.unwrap_or(defaults.current_age),
		target_age: number(meta, "targetAge")?.unwrap_or(defaults.target_age),
		simulation_count: number(meta, "simulationCount")?.unwrap_or(defaults.simulation_count),
	})
}
