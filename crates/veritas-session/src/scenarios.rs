//! What-if scenarios as the app edits them (R-FC-2, docs/finance/forecasting.md
//! FC-3, D50, D67). Each command plans one `Row::Scenario`, so it writes only
//! `veritas_scenario` and `veritas_scenario_event` and is one undo step.

use veritas_domain::{
	AccountId, BudgetData, CategoryId, CategoryKind, CivilDate, CoreError, Row, Scenario,
	ScenarioChange, ScenarioEvent, ScenarioEventId, ScenarioId, ScenarioRow,
};

/// FC-2's 2^51 − 1 limit on any amount a what-if carries, as for Actual's
/// own amounts, so a projection never starts from an absurd number.
const MAX_AMOUNT: i64 = (1 << 51) - 1;

/// A new what-if, with its first event when one is given (v17). The event is
/// checked as `add_scenario_event` checks one, and a refused event refuses
/// the whole command, so "Add what-if" is one undo step that leaves nothing
/// behind when it fails.
pub fn create_scenario(
	data: &BudgetData,
	id: ScenarioId,
	name: &str,
	created_at: i64,
	first_event: Option<(ScenarioEventId, ScenarioChange, CivilDate)>,
) -> Result<Vec<Row>, CoreError> {
	if data.scenarios.contains_key(&id) {
		return Err(CoreError::Invalid(format!("what-if {id} already exists")));
	}
	let name = scenario_name(data, None, name)?;
	let events = match first_event {
		Some((event_id, change, today)) => {
			check_change(data, &change, today)?;
			vec![ScenarioEvent {
				id: event_id,
				change,
			}]
		}
		None => Vec::new(),
	};
	let sort_order = data
		.scenarios
		.values()
		.map(|scenario| scenario.sort_order)
		.fold(0.0, f64::max)
		+ 1.0;
	Ok(vec![put(Scenario {
		id,
		name,
		sort_order,
		created_at,
		events,
	})])
}

pub fn rename_scenario(
	data: &BudgetData,
	id: &ScenarioId,
	name: &str,
) -> Result<Vec<Row>, CoreError> {
	let scenario = scenario(data, id)?;
	Ok(vec![put(Scenario {
		name: scenario_name(data, Some(id), name)?,
		..scenario.clone()
	})])
}

/// Removes the scenario and its events. Real data is never touched.
pub fn delete_scenario(data: &BudgetData, id: &ScenarioId) -> Result<Vec<Row>, CoreError> {
	scenario(data, id)?;
	Ok(vec![Row::Scenario(ScenarioRow {
		id: id.clone(),
		scenario: None,
	})])
}

/// A new event. Dated in the past is refused (D67), and so is a one-time
/// transfer, which schema v1 can't store (D50).
pub fn add_scenario_event(
	data: &BudgetData,
	scenario_id: &ScenarioId,
	event_id: ScenarioEventId,
	change: ScenarioChange,
	today: CivilDate,
) -> Result<Vec<Row>, CoreError> {
	let mut scenario = scenario(data, scenario_id)?.clone();
	let taken = data
		.scenarios
		.values()
		.flat_map(|scenario| &scenario.events)
		.any(|event| event.id == event_id);
	if taken {
		return Err(CoreError::Invalid(format!(
			"what-if event {event_id} already exists"
		)));
	}
	check_change(data, &change, today)?;
	scenario.events.push(ScenarioEvent {
		id: event_id,
		change,
	});
	scenario.events.sort_by(|a, b| a.id.cmp(&b.id));
	Ok(vec![put(scenario)])
}

/// Replaces an event's change. The new change is checked as a new entry, so
/// re-dating an event "in the past" must move it to today or later.
pub fn edit_scenario_event(
	data: &BudgetData,
	scenario_id: &ScenarioId,
	event_id: &ScenarioEventId,
	change: ScenarioChange,
	today: CivilDate,
) -> Result<Vec<Row>, CoreError> {
	let mut scenario = scenario(data, scenario_id)?.clone();
	check_change(data, &change, today)?;
	let event = scenario
		.events
		.iter_mut()
		.find(|event| &event.id == event_id)
		.ok_or_else(|| missing_event(event_id))?;
	event.change = change;
	Ok(vec![put(scenario)])
}

pub fn remove_scenario_event(
	data: &BudgetData,
	scenario_id: &ScenarioId,
	event_id: &ScenarioEventId,
) -> Result<Vec<Row>, CoreError> {
	let mut scenario = scenario(data, scenario_id)?.clone();
	let before = scenario.events.len();
	scenario.events.retain(|event| &event.id != event_id);
	if scenario.events.len() == before {
		return Err(missing_event(event_id));
	}
	Ok(vec![put(scenario)])
}

fn put(scenario: Scenario) -> Row {
	Row::Scenario(ScenarioRow {
		id: scenario.id.clone(),
		scenario: Some(scenario),
	})
}

fn scenario<'a>(data: &'a BudgetData, id: &ScenarioId) -> Result<&'a Scenario, CoreError> {
	data.scenarios.get(id).ok_or_else(|| CoreError::NotFound {
		entity: "scenario",
		id: id.to_string(),
	})
}

fn missing_event(id: &ScenarioEventId) -> CoreError {
	CoreError::NotFound {
		entity: "scenario event",
		id: id.to_string(),
	}
}

/// Trimmed, not blank, and not another what-if's name, case-insensitively.
fn scenario_name(
	data: &BudgetData,
	id: Option<&ScenarioId>,
	name: &str,
) -> Result<String, CoreError> {
	let name = name.trim();
	if name.is_empty() {
		return Err(CoreError::Invalid(
			"Enter a name for the what-if.".to_owned(),
		));
	}
	let taken = data
		.scenarios
		.values()
		.any(|other| Some(&other.id) != id && other.name.to_lowercase() == name.to_lowercase());
	if taken {
		return Err(CoreError::Invalid(
			"Another what-if already has that name. Choose a different one.".to_owned(),
		));
	}
	Ok(name.to_owned())
}

fn check_change(
	data: &BudgetData,
	change: &ScenarioChange,
	today: CivilDate,
) -> Result<(), CoreError> {
	veritas_forecast::validate_new(change, today)?;
	let amount = |amount: veritas_domain::Money| {
		if amount
			.minor()
			.checked_abs()
			.is_none_or(|size| size > MAX_AMOUNT)
		{
			Err(CoreError::Overflow)
		} else {
			Ok(())
		}
	};
	match change {
		ScenarioChange::OneTime {
			account,
			category,
			amount: value,
			..
		}
		| ScenarioChange::Recurring {
			account,
			category,
			amount: value,
			..
		} => {
			live_account(data, account)?;
			if let Some(category) = category {
				live_category(data, category)?;
			}
			amount(*value)
		}
		ScenarioChange::OneTimeTransfer { .. } => Err(CoreError::Invalid(
			"A one-time transfer can't be saved in a what-if yet.".to_owned(),
		)),
		ScenarioChange::CategoryPercent {
			category,
			percent_bp,
			..
		} => {
			live_category(data, category)?;
			if percent_bp.checked_abs().is_none_or(|size| size > 1_000_000) {
				return Err(CoreError::Invalid(
					"A percent change is between -10000% and 10000%.".to_owned(),
				));
			}
			Ok(())
		}
		ScenarioChange::IncomeChange {
			account,
			category,
			change,
			..
		} => {
			live_account(data, account)?;
			if let Some(category) = category {
				live_category(data, category)?;
				if data.category_kind(category) != CategoryKind::Income {
					return Err(CoreError::Invalid(
						"An income change needs an income category.".to_owned(),
					));
				}
			}
			match change {
				veritas_domain::IncomeDelta::Amount(value) => amount(*value),
				veritas_domain::IncomeDelta::Percent(bp) => {
					if bp.checked_abs().is_none_or(|size| size > 1_000_000) {
						return Err(CoreError::Invalid(
							"A percent change is between -10000% and 10000%.".to_owned(),
						));
					}
					Ok(())
				}
			}
		}
		ScenarioChange::GoalContribution {
			goal,
			amount: value,
			..
		} => {
			if !data.goals.contains_key(goal) {
				return Err(CoreError::NotFound {
					entity: "goal",
					id: goal.to_string(),
				});
			}
			amount(*value)
		}
	}
}

fn live_account(data: &BudgetData, id: &AccountId) -> Result<(), CoreError> {
	match data.accounts.get(id) {
		Some(account) if !account.tombstone => Ok(()),
		_ => Err(CoreError::NotFound {
			entity: "account",
			id: id.to_string(),
		}),
	}
}

fn live_category(data: &BudgetData, id: &CategoryId) -> Result<(), CoreError> {
	match data.categories.get(id) {
		Some(category) if !category.tombstone => Ok(()),
		_ => Err(CoreError::NotFound {
			entity: "category",
			id: id.to_string(),
		}),
	}
}
