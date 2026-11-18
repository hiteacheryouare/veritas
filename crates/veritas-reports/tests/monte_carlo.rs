//! D73: the Monte Carlo card, compared with Actual's own
//! `runMonteCarloSimulation`, run by the oracle (`widgets`) on the same
//! fixture and meta. On arm64 every value matches bit for bit, since the
//! engine runs V8's own fdlibm; raw values may still differ by 1e-9
//! relative (an absolute 1e-9 below magnitude 1) for an oracle on another
//! V8 build (`docs/oracle.md`). What the card and graph display, and every
//! count and depletion year, must be identical.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use serde_json::{Map, Value, json};
use support::actual::{fixture, oracle};
use veritas_domain::BudgetData;
use veritas_reports::monte_carlo::{
	DEFAULT_SIMULATION_SEED, MonteCarloParams, MonteCarloResult, RuleExplanation, RunDetailRow,
	monte_carlo_config_from_meta, monte_carlo_horizon_years, run_monte_carlo_simulation,
};
use veritas_reports::{Json, MonteCarloCard, monte_carlo_card};

const ORACLE_TODAY: &str = "2026-11-09";
const TOLERANCE: f64 = 1e-9;

fn meta(value: &Value) -> Option<Json> {
	(!value.is_null()).then(|| Json::parse(&value.to_string()).unwrap())
}

/// A whole number as JSON writes an integer, so it compares equal to the
/// oracle's.
fn num(value: f64) -> Value {
	if value.is_finite() && value.fract() == 0.0 && value.abs() < 9e15 {
		json!(value as i64)
	} else {
		json!(value)
	}
}

fn nums(values: &[f64]) -> Value {
	Value::Array(values.iter().copied().map(num).collect())
}

fn opt(value: Option<f64>) -> Value {
	value.map_or(Value::Null, num)
}

fn explanation_json(explanation: &RuleExplanation) -> Value {
	match explanation {
		RuleExplanation::Anchor { rate } => json!({"kind": "anchor", "rate": num(*rate)}),
		RuleExplanation::FloorCeiling {
			rate,
			unclamped,
			floor,
			ceiling,
			applied,
		} => json!({
			"kind": "floor-ceiling",
			"rate": num(*rate),
			"unclamped": num(*unclamped),
			"floor": num(*floor),
			"ceiling": num(*ceiling),
			"applied": format!("{applied:?}").to_lowercase(),
		}),
		RuleExplanation::Factor {
			rule,
			factor,
			previous_factor,
			planned,
			adjusted,
			action,
			current_rate,
			reference_rate,
			ratchet_streak,
		} => {
			let mut object = json!({
				"kind": "factor",
				"rule": format!("{rule:?}").to_lowercase(),
				"factor": num(*factor),
				"previousFactor": num(*previous_factor),
				"planned": num(*planned),
				"adjusted": num(*adjusted),
				"action": format!("{action:?}").to_lowercase(),
			});
			if let Some(rate) = current_rate {
				object["currentRate"] = num(*rate);
			}
			if let Some(rate) = reference_rate {
				object["referenceRate"] = num(*rate);
			}
			if let Some(streak) = ratchet_streak {
				object["ratchetStreak"] = json!(streak);
			}
			object
		}
	}
}

fn row_json(row: &RunDetailRow) -> Value {
	let mut object = json!({
		"year": row.year,
		"startBalance": num(row.start_balance),
		"withdrawal": num(row.withdrawal),
		"plannedSpending": num(row.planned_spending),
		"spent": num(row.spent),
		"growth": num(row.growth),
		"endBalance": num(row.end_balance),
		"potBalances": nums(&row.pot_balances),
		"potStartBalances": nums(&row.pot_start_balances),
		"inflation": opt(row.inflation),
		"income": num(row.income),
		"incomeAmounts": nums(&row.income_amounts),
		"incomeTax": num(row.income_tax),
		"unspentIncome": num(row.unspent_income),
		"surplusSaved": num(row.surplus_saved),
		"contributions": num(row.contributions),
		"potContributions": nums(&row.pot_contributions),
		"contributionAmounts": nums(&row.contribution_amounts),
		"potWithdrawals": nums(&row.pot_withdrawals),
		"potTaxes": nums(&row.pot_taxes),
		"potTaxables": nums(&row.pot_taxables),
		"taxPaid": num(row.tax_paid),
		"feesPaid": num(row.fees_paid),
		"potFees": nums(&row.pot_fees),
		"potReturns": Value::Array(row.pot_returns.iter().copied().map(opt).collect()),
	});
	if let Some(explanation) = &row.rule_explanation {
		object["ruleExplanation"] = explanation_json(explanation);
	}
	if row.minimum_applied {
		object["minimumApplied"] = json!(true);
	}
	if let Some(locked) = row.inaccessible_balance {
		object["inaccessibleBalance"] = num(locked);
	}
	if row.after_depletion {
		object["afterDepletion"] = json!(true);
	}
	object
}

/// `MonteCarloResult` as Actual's JSON names it.
fn result_json(result: &MonteCarloResult) -> Value {
	let year = |year: Option<usize>| year.map_or(Value::Null, |year| json!(year));
	json!({
		"successRate": num(result.success_rate),
		"percentileBands": result.percentile_bands.iter().map(|band| json!({
			"year": band.year,
			"p5": num(band.p5),
			"p10": num(band.p10),
			"p25": num(band.p25),
			"p30": num(band.p30),
			"p50": num(band.p50),
			"p70": num(band.p70),
			"p75": num(band.p75),
			"p90": num(band.p90),
		})).collect::<Vec<_>>(),
		"depletionHistogram": result.depletion_histogram.iter()
			.map(|entry| json!({"year": entry.year, "count": entry.count}))
			.collect::<Vec<_>>(),
		"depletionProbabilityByYear": nums(&result.depletion_probability_by_year),
		"medianEndingBalance": num(result.median_ending_balance),
		"medianTotalWithdrawn": num(result.median_total_withdrawn),
		"medianDepletionYear": year(result.median_depletion_year),
		"earliestDepletionYear": year(result.earliest_depletion_year),
		"latestDepletionYear": year(result.latest_depletion_year),
		"worstRunPath": nums(&result.worst_run_path),
		"endingBalances": nums(&result.ending_balances),
		"depletionYearBySimulation": result.depletion_year_by_simulation,
		"totalWithdrawnBySimulation": nums(&result.total_withdrawn_by_simulation),
		"runDetail": result.run_detail.as_ref().map_or(Value::Null, |rows| {
			Value::Array(rows.iter().map(row_json).collect())
		}),
		"simulationCount": result.simulation_count,
		"horizonYears": result.horizon_years,
	})
}

/// The oracle writes non-integers as their JavaScript text.
fn number(value: &Value) -> Option<f64> {
	match value {
		Value::Number(number) => number.as_f64(),
		Value::String(text) => text.parse().ok(),
		_ => None,
	}
}

/// The largest relative difference seen, and where.
#[derive(Default)]
struct Worst {
	difference: f64,
	at: String,
}

/// Compares two JSON trees, numbers within `tolerance` relative (absolute
/// below magnitude 1). Strings that hold numbers compare as numbers.
fn compare(path: &str, ours: &Value, theirs: &Value, tolerance: f64, worst: &mut Worst) {
	match (ours, theirs) {
		(Value::Object(ours), Value::Object(theirs)) => {
			let keys = |map: &Map<String, Value>| {
				let mut keys: Vec<String> = map.keys().cloned().collect();
				keys.sort();
				keys
			};
			assert_eq!(keys(ours), keys(theirs), "{path}: keys");
			for (key, value) in ours {
				compare(
					&format!("{path}.{key}"),
					value,
					&theirs[key],
					tolerance,
					worst,
				);
			}
		}
		(Value::Array(ours), Value::Array(theirs)) => {
			assert_eq!(ours.len(), theirs.len(), "{path}: length");
			for (index, (ours, theirs)) in ours.iter().zip(theirs).enumerate() {
				compare(&format!("{path}[{index}]"), ours, theirs, tolerance, worst);
			}
		}
		(Value::Number(_), _) | (Value::Null, Value::String(_)) => {
			let a = number(ours).unwrap_or(f64::NAN);
			let b = number(theirs)
				.unwrap_or_else(|| panic!("{path}: we have {ours}, Actual has {theirs}"));
			if a.is_nan() && b.is_nan() {
				return;
			}
			let difference = (a - b).abs() / 1f64.max(a.abs()).max(b.abs());
			assert!(
				difference <= tolerance,
				"{path}: we have {a}, Actual has {b} (difference {difference:e})"
			);
			if difference > worst.difference {
				worst.difference = difference;
				worst.at = path.to_owned();
			}
		}
		_ => assert_eq!(ours, theirs, "{path}"),
	}
}

/// Everything the card and graph display, and every count, exactly.
fn check_exact(label: &str, card: &MonteCarloCard, ours: &Value, theirs: &Value) {
	let mut ignored = Worst::default();
	for key in [
		"successRate",
		"percentileBands",
		"depletionHistogram",
		"depletionProbabilityByYear",
		"medianEndingBalance",
		"medianTotalWithdrawn",
		"medianDepletionYear",
		"earliestDepletionYear",
		"latestDepletionYear",
		"worstRunPath",
		"depletionYearBySimulation",
		"simulationCount",
		"horizonYears",
	] {
		compare(
			&format!("{label} {key}"),
			&ours[key],
			&theirs["result"][key],
			0.0,
			&mut ignored,
		);
	}
	assert_eq!(
		number(&theirs["successPercent"]),
		Some(card.success_percent),
		"{label} success percent"
	);
	assert_eq!(
		number(&theirs["endAge"]),
		Some(card.end_age),
		"{label} end age"
	);
	let start_age = number(&theirs["startAge"]).unwrap();
	let bands = theirs["result"]["percentileBands"].as_array().unwrap();
	assert_eq!(card.chart.len(), bands.len(), "{label} chart");
	for (point, band) in card.chart.iter().zip(bands) {
		assert_eq!(
			point.age,
			start_age + point.year as f64,
			"{label} age {}",
			point.year
		);
		let shown = [
			("p5", point.p5),
			("p10", point.p10),
			("p25", point.p25),
			("p30", point.p30),
			("p50", point.p50),
			("p70", point.p70),
			("p75", point.p75),
			("p90", point.p90),
		];
		for (key, value) in shown {
			let theirs = number(&band[key]).unwrap();
			assert!(
				theirs == value || (theirs.is_nan() && value.is_nan()),
				"{label} year {} {key}: {theirs} against {value}",
				point.year
			);
		}
	}
}

struct Case {
	id: String,
	meta: Value,
	capture: Option<usize>,
}

fn case(id: impl Into<String>, meta: Value) -> Case {
	Case {
		id: id.into(),
		meta,
		capture: None,
	}
}

/// What we compute for a case: the card, and the engine's result with any
/// captured run.
fn ours(budget: &BudgetData, case: &Case) -> (MonteCarloCard, MonteCarloResult) {
	let meta = meta(&case.meta);
	let card = monte_carlo_card(budget, meta.as_ref()).unwrap();
	let Some(capture) = case.capture else {
		let result = card.result.clone();
		return (card, result);
	};
	let config = monte_carlo_config_from_meta(meta.as_ref()).unwrap();
	let captured = run_monte_carlo_simulation(&MonteCarloParams {
		horizon_years: monte_carlo_horizon_years(&config),
		config,
		seed: DEFAULT_SIMULATION_SEED,
		capture_run_detail: Some(capture),
		deflate_to_todays_money: true,
	})
	.unwrap();
	assert_eq!(
		MonteCarloResult {
			run_detail: None,
			..captured.clone()
		},
		card.result,
		"{}: capturing a run changed the others",
		case.id
	);
	(card, captured)
}

/// Runs every case through both apps and compares them, returning the
/// worst relative difference.
fn compare_cases(budget: &BudgetData, cases: &[Case]) -> Worst {
	let computed: Vec<_> = cases.iter().map(|case| ours(budget, case)).collect();
	let requests: Vec<Value> = cases
		.iter()
		.map(|case| {
			let mut request = json!({"id": case.id, "type": "monte-carlo-card", "meta": case.meta});
			if let Some(capture) = case.capture {
				request["captureRunDetail"] = json!(capture);
			}
			request
		})
		.collect();
	let mut worst = Worst::default();
	let Some(theirs) = oracle(
		"widgets",
		"all-features",
		&Value::Array(requests),
		ORACLE_TODAY,
	) else {
		return worst;
	};
	for (case, (card, result)) in cases.iter().zip(&computed) {
		let theirs = &theirs[case.id.as_str()];
		let label = format!("{} {}", case.id, case.meta);
		assert!(
			theirs.get("error").is_none(),
			"{label}: Actual failed: {theirs}"
		);
		let ours = result_json(result);
		compare(&label, &ours, &theirs["result"], TOLERANCE, &mut worst);
		check_exact(&label, card, &ours, theirs);
	}
	worst
}

fn with(base: &Value, extra: Value) -> Value {
	let mut meta = base.clone();
	for (key, value) in extra.as_object().unwrap() {
		meta[key] = value.clone();
	}
	meta
}

fn report(name: &str, cases: usize, worst: &Worst) {
	eprintln!(
		"{name}: {cases} cases, worst relative difference {:e} at {}",
		worst.difference, worst.at
	);
}

fn quick() -> Value {
	json!({"simulationCount": 1000})
}

fn three_pots() -> Value {
	json!([
		{"id": "isa", "name": "ISA", "startingBalance": 30_000_000, "allocationPreset": "equity-80", "expectedReturnMean": 0.065, "returnStdDev": 0.12},
		{"id": "pension", "startingBalance": 60_000_000, "accessAge": 63, "allocationPreset": "equity-60", "expectedReturnMean": 0.06, "returnStdDev": 0.1, "withdrawalTaxRate": 0.2, "taxableFraction": 0.75},
		{"id": "cash", "startingBalance": 5_000_000, "allocationPreset": "cash", "expectedReturnMean": 0.03, "returnStdDev": 0.015},
	])
}

#[test]
fn d73_monte_carlo_card_matches_actual_across_models() {
	let budget = fixture("all-features");
	let base = quick();
	let mut cases = vec![
		case("default-null", json!(null)),
		case("default-empty", json!({})),
	];
	for model in [
		"normal",
		"historical-bootstrap",
		"historical-block-bootstrap",
		"historical-sequence",
	] {
		for preset in [
			"equity-100",
			"equity-80",
			"equity-60",
			"equity-40",
			"cash",
			"custom",
		] {
			cases.push(case(
				format!("model-{model}-{preset}"),
				with(
					&base,
					json!({"returnModel": model, "pots": [{"id": "only", "allocationPreset": preset, "expectedReturnMean": 0.055, "returnStdDev": 0.13}]}),
				),
			));
		}
		for (name, shares) in [
			("mix", json!([0.7, 0.2, 0.1])),
			("dust", json!([0.7, 0.3, 0.0])),
			("incomplete", json!([0.5, 0.3, 0.0])),
			("negative", json!([1.2, -0.2, 0.0])),
		] {
			cases.push(case(
				format!("model-{model}-custom-{name}"),
				with(
					&base,
					json!({"returnModel": model, "pots": [{"id": "only", "allocationPreset": "custom-mix", "allocationStocks": shares[0], "allocationBonds": shares[1], "allocationCash": shares[2]}]}),
				),
			));
		}
		cases.push(case(
			format!("model-{model}-mixed-pots"),
			with(&base, json!({"returnModel": model, "pots": three_pots()})),
		));
	}
	for length in [0.0, 1.0, 2.5, 5.0, 10.0, 30.0, 45.0] {
		cases.push(case(
			format!("block-{length}"),
			with(
				&base,
				json!({"returnModel": "historical-block-bootstrap", "historicalBlockLength": length, "pots": three_pots()}),
			),
		));
	}
	for (name, extra) in [
		(
			"glide",
			json!({"pots": [{"id": "glide", "allocationPreset": "equity-100", "investmentPhases": [
				{"id": "a", "fromAge": 70, "transition": "gradual", "allocationPreset": "equity-40", "annualFeeRate": 0.004},
				{"id": "b", "fromAge": 80, "allocationPreset": "cash"},
				{"id": "dropped", "allocationPreset": "custom"},
			]}]}),
		),
		(
			"reached-phase",
			json!({"pots": [{"id": "old", "allocationPreset": "custom", "expectedReturnMean": 0.09, "returnStdDev": 0.2, "investmentPhases": [
				{"id": "past", "fromAge": 55, "allocationPreset": "equity-80"},
				{"id": "later", "fromAge": 75, "transition": "gradual", "allocationPreset": "custom", "expectedReturnMean": 0.02, "returnStdDev": 0.05},
			]}]}),
		),
	] {
		for model in ["normal", "historical-bootstrap"] {
			cases.push(case(
				format!("phases-{name}-{model}"),
				with(&with(&base, extra.clone()), json!({"returnModel": model})),
			));
		}
	}
	let worst = compare_cases(&budget, &cases);
	report("models", cases.len(), &worst);
}

#[test]
fn d73_monte_carlo_card_matches_actual_across_withdrawals() {
	let budget = fixture("all-features");
	let base = with(
		&quick(),
		json!({"pots": three_pots(), "spendingPhases": [{"id": "s", "annualWithdrawal": 4_200_000}]}),
	);
	let mut cases = Vec::new();
	for strategy in ["proportional", "sequential", "best-performer", "target-mix"] {
		for model in ["normal", "historical-bootstrap"] {
			cases.push(case(
				format!("strategy-{strategy}-{model}"),
				with(
					&base,
					json!({"withdrawalStrategy": strategy, "returnModel": model}),
				),
			));
		}
		let mut surplus = three_pots();
		surplus
			.as_array_mut()
			.unwrap()
			.insert(1, json!({"id": "spare", "name": "Spare", "isSurplus": true, "startingBalance": 999, "accountId": "ignored"}));
		cases.push(case(
			format!("strategy-{strategy}-surplus"),
			with(
				&base,
				json!({"withdrawalStrategy": strategy, "pots": surplus, "incomeStreams": [{"id": "pension", "fromAge": 67, "annualAmount": 6_000_000}]}),
			),
		));
	}
	let rules = [
		json!({"type": "none"}),
		json!({"type": "guardrails"}),
		json!({"type": "guardrails", "preservationTriggerPct": 0.1, "preservationCutPct": 0.2, "prosperityTriggerPct": 0.1, "prosperityIncreasePct": 0.15}),
		json!({"type": "ratcheting"}),
		json!({"type": "ratcheting", "consecutiveYears": 1, "balanceThresholdMultiple": 1.1}),
		json!({"type": "floor-ceiling"}),
		json!({"type": "floor-ceiling", "floorPct": 0.05, "ceilingPct": 0.5}),
		json!({"type": "boundaries"}),
		json!({"type": "boundaries", "upperRateThreshold": 0.05, "lowerRateThreshold": null}),
	];
	for (index, rule) in rules.iter().enumerate() {
		cases.push(case(
			format!("rule-{index}"),
			with(&base, json!({"withdrawalRule": rule})),
		));
		cases.push(case(
			format!("rule-{index}-minimum"),
			with(
				&base,
				json!({"withdrawalRule": rule, "minimumSpending": 3_800_000, "spendingPhases": [{"id": "s", "annualWithdrawal": 4_200_000}, {"id": "later", "fromAge": 75, "annualWithdrawal": 3_000_000}]}),
			),
		));
	}
	let worst = compare_cases(&budget, &cases);
	report("withdrawals", cases.len(), &worst);
}

#[test]
fn d73_monte_carlo_card_matches_actual_across_flows_and_taxes() {
	let budget = fixture("all-features");
	let base = with(&quick(), json!({"pots": three_pots()}));
	let streams = json!([
		{"id": "salary", "toAge": 64, "annualAmount": 7_000_000, "adjustsWithInflation": true, "taxRate": 0.25, "taxableFraction": 1},
		{"id": "state", "fromAge": 67, "annualAmount": 1_100_000, "taxRate": 0.1, "taxableFraction": 0.85},
		{"id": "annuity", "fromAge": 70, "toAge": 80, "annualAmount": 500_000, "adjustsWithInflation": false},
	]);
	let contributions = json!([
		{"id": "outside", "potId": "isa", "toAge": 64, "annualAmount": 1_200_000},
		{"id": "sacrifice", "potId": "pension", "sourceIncomeStreamId": "salary", "beforeTax": true, "annualAmount": 900_000, "adjustsWithInflation": false},
		{"id": "after-tax", "potId": "isa", "sourceIncomeStreamId": "salary", "annualAmount": 10_000_000},
		{"id": "lost-pot", "potId": "gone", "annualAmount": 500_000},
		{"id": "lost-stream", "potId": "isa", "sourceIncomeStreamId": "gone", "annualAmount": 500_000},
	]);
	let phases = json!([
		{"id": "work", "annualWithdrawal": 0},
		{"id": "go-go", "fromAge": 65, "annualWithdrawal": 5_500_000},
		{"id": "slow", "fromAge": 75, "annualWithdrawal": 4_000_000},
		{"id": "no-go", "fromAge": 85, "annualWithdrawal": 3_500_000},
	]);
	let bands = json!([
		{"id": "basic", "from": 1_257_000, "rate": 0.2},
		{"id": "higher", "from": 5_027_000, "rate": 0.4},
		{"id": "additional", "from": 12_514_000, "rate": 0.45},
	]);
	let mut cases = Vec::new();
	for tax_model in ["flat", "bands"] {
		for (name, extra) in [
			("phases", json!({"spendingPhases": phases})),
			(
				"income",
				json!({"incomeStreams": streams, "spendingPhases": phases}),
			),
			(
				"contributions",
				json!({"incomeStreams": streams, "contributions": contributions, "spendingPhases": phases, "currentAge": 55}),
			),
			(
				"rule",
				json!({"incomeStreams": streams, "contributions": contributions, "spendingPhases": phases, "currentAge": 55, "withdrawalRule": {"type": "guardrails"}, "minimumSpending": 4_000_000}),
			),
		] {
			cases.push(case(
				format!("{tax_model}-{name}"),
				with(
					&with(&base, extra),
					json!({"taxModel": tax_model, "taxBands": bands}),
				),
			));
		}
	}
	cases.push(case(
		"bands-unsorted",
		with(
			&base,
			json!({"taxModel": "bands", "taxBands": [{"id": "b", "from": 6_000_000, "rate": 0.4}, {"id": "a", "from": 0, "rate": 0.1}, {"id": "c", "from": 6_000_000, "rate": 0.5}]}),
		),
	));
	cases.push(case(
		"bands-zero-rates",
		with(
			&base,
			json!({"taxModel": "bands", "taxBands": [{"id": "z"}]}),
		),
	));
	for (name, fees) in [
		("rate", json!({"annualFeeRate": 0.0075})),
		("fixed", json!({"annualFeeFixed": 150_000})),
		(
			"fixed-inflating",
			json!({"annualFeeFixed": 150_000, "feeAdjustsWithInflation": true, "annualFeeRate": 0.2}),
		),
	] {
		let mut pots = three_pots();
		for pot in pots.as_array_mut().unwrap() {
			for (key, value) in fees.as_object().unwrap() {
				pot[key] = value.clone();
			}
		}
		cases.push(case(
			format!("fees-{name}"),
			with(&base, json!({"pots": pots})),
		));
	}
	for (name, inflation) in [
		("off", json!({"inflationMean": null})),
		(
			"fixed",
			json!({"inflationMean": 0.03, "inflationStdDev": 0}),
		),
		(
			"volatile",
			json!({"inflationMean": 0.03, "inflationStdDev": 0.05}),
		),
		(
			"wild",
			json!({"inflationMean": -0.2, "inflationStdDev": 0.6}),
		),
	] {
		for model in ["normal", "historical-block-bootstrap"] {
			cases.push(case(
				format!("inflation-{name}-{model}"),
				with(
					&with(&base, inflation.clone()),
					json!({"returnModel": model, "incomeStreams": streams}),
				),
			));
		}
	}
	let worst = compare_cases(&budget, &cases);
	report("flows and taxes", cases.len(), &worst);
}

#[test]
fn d73_monte_carlo_card_resolves_linked_pots_and_extremes() {
	let budget = fixture("all-features");
	let mut cases = Vec::new();
	let mut accounts: Vec<_> = budget.accounts.values().collect();
	accounts.sort_by_key(|account| account.id.to_string());
	for account in &accounts {
		cases.push(case(
			format!("linked-{}", account.id),
			json!({"simulationCount": 1000, "targetAge": 61, "pots": [{"id": "linked", "startingBalance": 12_345, "accountId": account.id.to_string()}]}),
		));
	}
	cases.push(case(
		"linked-missing",
		json!({"simulationCount": 1000, "targetAge": 61, "pots": [{"id": "linked", "startingBalance": 12_345, "accountId": "no-such-account"}]}),
	));
	let mut linked_pots = three_pots();
	for (pot, account) in linked_pots
		.as_array_mut()
		.unwrap()
		.iter_mut()
		.zip(&accounts)
	{
		pot["accountId"] = json!(account.id.to_string());
	}
	cases.push(case(
		"linked-portfolio",
		json!({"simulationCount": 1000, "pots": linked_pots, "spendingPhases": [{"id": "s", "annualWithdrawal": 300_000}]}),
	));
	for (name, ages) in [
		("one-year", json!({"currentAge": 60, "targetAge": 61})),
		("backwards", json!({"currentAge": 60, "targetAge": 40})),
		("century", json!({"currentAge": 20, "targetAge": 140})),
		("fractional", json!({"currentAge": 60.4, "targetAge": 90.7})),
		("half", json!({"currentAge": 60, "targetAge": 72.5})),
	] {
		cases.push(case(
			format!("ages-{name}"),
			with(&quick(), with(&ages, json!({"pots": three_pots()}))),
		));
	}
	for count in [1.0, 999.4, 1000.0, 1500.5, 5000.0, 10_000.0, 25_000.0] {
		cases.push(case(
			format!("count-{count}"),
			json!({"simulationCount": count}),
		));
	}
	cases.push(case(
		"sequence-ignores-count",
		json!({"simulationCount": 10_000, "returnModel": "historical-sequence"}),
	));
	let worst = compare_cases(&budget, &cases);
	report("linked pots and extremes", cases.len(), &worst);
	let Some(theirs) = oracle(
		"widgets",
		"all-features",
		&json!([{"id": "all", "type": "monte-carlo-card", "meta": {"simulationCount": 1000, "targetAge": 61, "pots": accounts.iter().map(|account| json!({"id": account.id.to_string(), "accountId": account.id.to_string()})).collect::<Vec<_>>()}}]),
		ORACLE_TODAY,
	) else {
		return;
	};
	let balances = theirs["all"]["balances"].as_object().unwrap();
	assert!(
		balances
			.values()
			.any(|balance| balance.as_i64().is_some_and(|balance| balance < 0)),
		"a linked pot covers an overdrawn account: {balances:?}"
	);
}

/// The run the report page drills into, for runs that last and runs that
/// run out, under each rule, with flows, taxes and fees.
#[test]
fn d73_monte_carlo_card_matches_actual_run_detail() {
	let budget = fixture("all-features");
	let rich = json!({
		"simulationCount": 1000,
		"currentAge": 55,
		"pots": [
			{"id": "isa", "startingBalance": 20_000_000, "allocationPreset": "equity-80", "annualFeeRate": 0.003},
			{"id": "spare", "isSurplus": true},
			{"id": "pension", "startingBalance": 40_000_000, "accessAge": 58, "withdrawalTaxRate": 0.2, "taxableFraction": 0.75, "annualFeeFixed": 20_000, "feeAdjustsWithInflation": true},
		],
		"incomeStreams": [
			{"id": "salary", "toAge": 59, "annualAmount": 6_000_000, "taxRate": 0.3, "taxableFraction": 1},
			{"id": "state", "fromAge": 67, "annualAmount": 1_100_000, "taxRate": 0.1},
		],
		"contributions": [
			{"id": "sacrifice", "potId": "pension", "sourceIncomeStreamId": "salary", "beforeTax": true, "annualAmount": 800_000},
			{"id": "top-up", "potId": "isa", "toAge": 59, "annualAmount": 300_000, "adjustsWithInflation": false},
		],
		"spendingPhases": [
			{"id": "work", "annualWithdrawal": 3_000_000},
			{"id": "retired", "fromAge": 60, "annualWithdrawal": 5_000_000},
		],
		"minimumSpending": 3_500_000,
	});
	let mut cases = Vec::new();
	for rule in [
		"none",
		"guardrails",
		"ratcheting",
		"floor-ceiling",
		"boundaries",
	] {
		for tax_model in ["flat", "bands"] {
			let value = with(
				&rich,
				json!({"withdrawalRule": {"type": rule, "consecutiveYears": 2}, "taxModel": tax_model, "taxBands": [{"id": "a", "from": 1_000_000, "rate": 0.2}, {"id": "b", "from": 5_000_000, "rate": 0.4}]}),
			);
			let card = monte_carlo_card(&budget, meta(&value).as_ref()).unwrap();
			let depletions = &card.result.depletion_year_by_simulation;
			let failed = depletions.iter().position(|year| *year > 0);
			let lasted = depletions.iter().position(|year| *year < 0);
			for (name, capture) in [
				("first", Some(0)),
				("failed", failed),
				("lasted", lasted),
				("last", Some(999)),
			] {
				if let Some(capture) = capture {
					cases.push(Case {
						id: format!("detail-{rule}-{tax_model}-{name}"),
						meta: value.clone(),
						capture: Some(capture),
					});
				}
			}
		}
	}
	cases.push(Case {
		id: "detail-out-of-range".into(),
		meta: rich.clone(),
		capture: Some(1000),
	});
	cases.push(Case {
		id: "detail-flat-money".into(),
		meta: with(
			&rich,
			json!({"inflationMean": null, "returnModel": "historical-sequence"}),
		),
		capture: Some(40),
	});
	let after_depletion = cases.iter().any(|case| {
		ours(&budget, case)
			.1
			.run_detail
			.is_some_and(|rows| rows.iter().any(|row| row.after_depletion))
	});
	assert!(after_depletion, "some captured run runs out");
	let worst = compare_cases(&budget, &cases);
	report("run detail", cases.len(), &worst);
}

#[test]
fn monte_carlo_meta_actual_never_writes_is_unsupported() {
	let budget = fixture("all-features");
	for value in [
		json!({"returnModel": "lottery"}),
		json!({"withdrawalRule": {"type": null}}),
		json!({"pots": [{"id": "a", "startingBalance": "lots"}]}),
		json!({"pots": "all of them"}),
	] {
		assert!(
			monte_carlo_card(&budget, meta(&value).as_ref()).is_err(),
			"{value} is unsupported"
		);
	}
	let infinite = Json::parse(r#"{"currentAge": 1e400, "targetAge": 1e400}"#).unwrap();
	assert!(monte_carlo_card(&budget, Some(&infinite)).is_err());
}
