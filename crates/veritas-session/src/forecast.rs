//! The Insights forecast (R-FC-1 to R-FC-4, docs/finance/forecasting.md): the
//! baseline and up to three what-ifs over the budget's known flows, next
//! month's suggestions, goal ETAs, and "Apply suggestions". Reads only; the
//! one command plans budget cells.

use veritas_domain::{BudgetData, CivilDate, CoreError, Row, Scenario, ScenarioId};
use veritas_engine::{BudgetEditor, BudgetWrites, EnvelopeSheet};
use veritas_reports::NetWorthPoint;
use veritas_forecast::{
	Comparison, Forecaster, GoalEta, MAX_COMPARED, ScheduledFlows, Suggestions, goal_etas,
	next_month_envelopes,
};

use crate::CancelToken;

/// One forecast screen: the projections, the known flows they used, and
/// each projection's goal ETAs, baseline first, then each scenario in the
/// order asked.
pub struct ForecastView {
	pub comparison: Comparison,
	pub scenarios: Vec<Scenario>,
	pub flows: ScheduledFlows,
	pub goal_etas: Vec<Vec<GoalEta>>,
	/// FC-3's net worth history before today's month (ST-2).
	pub history: Vec<NetWorthPoint>,
}

pub fn forecast(
	data: &BudgetData,
	sheet: &EnvelopeSheet,
	today: CivilDate,
	scenario_ids: &[ScenarioId],
	cancel: &CancelToken,
) -> Result<ForecastView, CoreError> {
	if scenario_ids.len() > MAX_COMPARED {
		return Err(CoreError::Invalid(format!(
			"Compare up to {MAX_COMPARED} what-ifs at a time."
		)));
	}
	let scenarios: Vec<Scenario> = scenario_ids
		.iter()
		.map(|id| {
			data.scenarios
				.get(id)
				.cloned()
				.ok_or_else(|| CoreError::NotFound {
					entity: "scenario",
					id: id.to_string(),
				})
		})
		.collect::<Result<_, _>>()?;
	cancel.check()?;
	let forecaster = Forecaster::new(data)?;
	cancel.check()?;
	let flows = forecaster.scheduled_flows(today, Forecaster::horizon_end(today)?)?;
	cancel.check()?;
	let baseline = forecaster.project(today, &flows.flows, None)?;
	let mut projections = Vec::new();
	for scenario in &scenarios {
		cancel.check()?;
		projections.push(forecaster.project(today, &flows.flows, Some(scenario))?);
	}
	let classification = forecaster.classification();
	let mut etas = vec![goal_etas(data, sheet, classification, today, None)?];
	for scenario in &scenarios {
		etas.push(goal_etas(
			data,
			sheet,
			classification,
			today,
			Some(scenario),
		)?);
	}
	let history = forecaster.net_worth_history(today)?;
	cancel.check()?;
	Ok(ForecastView {
		comparison: Comparison {
			baseline,
			scenarios: projections,
		},
		scenarios,
		flows,
		goal_etas: etas,
		history,
	})
}

/// FC-5's suggestions for the month after `today`.
pub fn suggestions(
	data: &BudgetData,
	sheet: &EnvelopeSheet,
	today: CivilDate,
) -> Result<Suggestions, CoreError> {
	let forecaster = Forecaster::new(data)?;
	let flows = forecaster.scheduled_flows(today, Forecaster::horizon_end(today)?)?;
	let own;
	let next = today.year_month().offset(1)?;
	let sheet = if sheet.month(next).is_some() {
		sheet
	} else {
		own = EnvelopeSheet::compute(data, next.first_day())?;
		&own
	};
	forecaster.suggestions(today, &flows.flows, &next_month_envelopes(sheet, today)?)
}

/// "Apply suggestions" (R-FC-3): next month's budget set to every suggested
/// amount, already on the budget scale (CLS-12.11), as one command. A
/// "keeping" line writes nothing.
pub fn apply_suggestions(
	data: &BudgetData,
	sheet: &EnvelopeSheet,
	today: CivilDate,
) -> Result<Vec<Row>, CoreError> {
	let suggested = suggestions(data, sheet, today)?;
	let own;
	let sheet = if sheet.months().contains(&suggested.month) {
		sheet
	} else {
		own = EnvelopeSheet::compute(data, suggested.month.first_day())?;
		&own
	};
	let editor = BudgetEditor::new(data, sheet);
	let mut writes = BudgetWrites::default();
	for item in &suggested.items {
		let Some(amount) = item.amount else {
			continue;
		};
		let cell = editor.set_budget(suggested.month, &item.category, amount)?;
		writes.cells.extend(cell.cells);
	}
	Ok(crate::writes::rows(data, &writes))
}
