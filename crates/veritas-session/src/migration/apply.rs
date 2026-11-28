//! MIG-1.5 and MIG-7.1: the ticked items as one command, with each item's
//! record for its own Revert.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use veritas_domain::{
	BudgetCell, CategoryGroupId, CategoryId, CategoryKind, CivilDate, CoreError, Goal, GoalId,
	GoalStatus, MigrationApplied, MigrationAppliedRow, Money, Row, ScheduleId, ScheduleMeta,
	ScheduleMetaRow, WidgetId, WidgetOverride, WidgetOverrideRow, is_goal_icon,
};
use veritas_engine::goals as engine;

use super::goal::goals_group;
use super::{Conversion, Inputs, Suggestion, fingerprint, report, suggestions};
use crate::categories;
use crate::writes::{applied, rows as write_rows};

/// Every value the user edited for one item (MIG-7.1). Unset values take the
/// proposal.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Edits {
	pub target: Option<Money>,
	pub target_date: Option<CivilDate>,
	/// (b) "Also show this target in Actual", off by default.
	pub write_note_in_actual: bool,
	pub kind: Option<CategoryKind>,
	pub display_name: Option<String>,
	pub icon: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selected {
	pub key: String,
	pub edits: Edits,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
	pub items: Vec<Selected>,
	/// The user's yes to showing a hidden Goals group again (MIG-4.5).
	pub consent_show_goals_group: bool,
}

fn refuse(message: impl Into<String>) -> CoreError {
	CoreError::Invalid(message.into())
}

/// The rows of "Upgrade to Veritas features". A key that is no longer a
/// suggestion, or a blocked one, refuses the whole command (MIG-7.1).
pub fn apply(inputs: &Inputs<'_>, selection: &Selection) -> Result<Vec<Row>, CoreError> {
	let current: BTreeMap<String, Suggestion> = suggestions(inputs)?
		.into_iter()
		.map(|suggestion| (suggestion.key.clone(), suggestion))
		.collect();
	let mut picked: Vec<(&Suggestion, &Edits)> = Vec::new();
	let mut keys = BTreeSet::new();
	for item in &selection.items {
		let suggestion = current.get(&item.key).ok_or_else(|| {
			refuse("This budget changed since the sheet opened. Close it and open it again.")
		})?;
		if let Some(reason) = &suggestion.blocked {
			return Err(refuse(reason.clone()));
		}
		if !keys.insert(item.key.clone()) {
			return Err(refuse(format!("{} is ticked twice", item.key)));
		}
		picked.push((suggestion, &item.edits));
	}
	for (suggestion, _) in &picked {
		if let Some(other) = &suggestion.blocked_by
			&& keys.contains(other)
		{
			return Err(refuse(super::goal::RENAME_FIRST));
		}
	}
	let mut planned: Vec<Row> = Vec::new();
	let goals: Vec<(&Suggestion, &Edits)> = picked
		.iter()
		.copied()
		.filter(|(suggestion, _)| suggestion.conversion == Conversion::Goal)
		.collect();
	if !goals.is_empty() {
		planned.extend(goal_rows(
			inputs,
			&goals,
			selection.consent_show_goals_group,
		)?);
	}
	for (suggestion, edits) in picked {
		match suggestion.conversion {
			Conversion::Goal => {}
			Conversion::Report => planned.extend(report_rows(inputs, suggestion)?),
			Conversion::Kind => planned.extend(kind_rows(inputs, suggestion, edits)?),
			Conversion::Bill => planned.extend(bill_rows(inputs, suggestion, edits)?),
		}
	}
	Ok(planned)
}

/// The ticked suggestions with their Confirm figures recomputed for what the
/// user edited (MIG-2.4): a goal's lines for its edited target and date, and
/// a kind's counts and report figures for the kind the user picked.
pub fn preview(inputs: &Inputs<'_>, selection: &Selection) -> Result<Vec<Suggestion>, CoreError> {
	let current: BTreeMap<String, Suggestion> = suggestions(inputs)?
		.into_iter()
		.map(|suggestion| (suggestion.key.clone(), suggestion))
		.collect();
	let mut previewed = Vec::new();
	for item in &selection.items {
		let Some(suggestion) = current.get(&item.key) else {
			continue;
		};
		let mut suggestion = suggestion.clone();
		match suggestion.conversion {
			Conversion::Goal => {
				let target = item.edits.target.or(suggestion.proposed.target);
				let date = item.edits.target_date.or(suggestion.proposed.target_date);
				let category = CategoryId::new(suggestion.subject_id.as_str());
				suggestion.goal_preview = super::goal::preview(inputs, &category, target, date)?;
			}
			Conversion::Kind => {
				if let Some(kind) = item
					.edits
					.kind
					.filter(|kind| Some(*kind) != suggestion.proposed.kind)
				{
					let category = CategoryId::new(suggestion.subject_id.as_str());
					let (change, effects) = super::kind::change_to(inputs, &category, Some(kind))?;
					suggestion.proposed.kind = Some(kind);
					suggestion.kind_change = Some(change);
					suggestion.effects = effects;
				}
			}
			Conversion::Report | Conversion::Bill => {}
		}
		previewed.push(suggestion);
	}
	Ok(previewed)
}

fn record(key: &str, prior: Value) -> Row {
	Row::MigrationApplied(MigrationAppliedRow {
		key: key.to_owned(),
		value: Some(MigrationApplied {
			key: key.to_owned(),
			prior: prior.to_string(),
		}),
	})
}

/// (a) MIG-3.4: the override row, and an empty record (MIG-7.1).
fn report_rows(inputs: &Inputs<'_>, suggestion: &Suggestion) -> Result<Vec<Row>, CoreError> {
	let data = inputs.data;
	let id = WidgetId::new(suggestion.subject_id.as_str());
	let widget = data
		.widgets
		.get(&id)
		.ok_or_else(|| refuse("That dashboard card is gone"))?;
	let report =
		report::widget_report(data, widget).ok_or_else(|| refuse("That report is gone"))?;
	let value = WidgetOverride {
		widget: id.clone(),
		native_type: suggestion
			.proposed
			.native_type
			.clone()
			.unwrap_or_else(|| report::NET_WORTH_CARD.to_owned()),
		native_meta: suggestion.proposed.native_meta.clone().unwrap_or_default(),
		fingerprint: fingerprint(report, widget),
	};
	Ok(vec![
		Row::WidgetOverride(WidgetOverrideRow {
			widget: id,
			value: Some(value),
		}),
		record(&suggestion.key, Value::Null),
	])
}

/// (c) MIG-5.6: one upsert of the stored kind, and the previous meta row.
fn kind_rows(
	inputs: &Inputs<'_>,
	suggestion: &Suggestion,
	edits: &Edits,
) -> Result<Vec<Row>, CoreError> {
	let category = CategoryId::new(suggestion.subject_id.as_str());
	let kind = edits
		.kind
		.or(suggestion.proposed.kind)
		.ok_or_else(|| refuse("Choose debt principal or savings"))?;
	if !matches!(kind, CategoryKind::DebtPrincipal | CategoryKind::Savings) {
		return Err(refuse("Choose debt principal or savings"));
	}
	let previous = inputs.data.category_meta.get(&category);
	let prior = json!({
		"meta": previous.map(super::revert::meta_json),
		"kind": super::revert::kind_text(kind),
	});
	let mut rows = crate::kinds::set_kind(inputs.data, &category, kind)?;
	rows.push(record(&suggestion.key, prior));
	Ok(rows)
}

/// (d) MIG-6.5: the Veritas-only name and icon, and what they were.
fn bill_rows(
	inputs: &Inputs<'_>,
	suggestion: &Suggestion,
	edits: &Edits,
) -> Result<Vec<Row>, CoreError> {
	let schedule = ScheduleId::new(suggestion.subject_id.as_str());
	let before = inputs
		.data
		.schedule_meta
		.get(&schedule)
		.cloned()
		.unwrap_or_default();
	let display_name = edits
		.display_name
		.clone()
		.or_else(|| suggestion.proposed.display_name.clone())
		.map(|name| name.trim().to_owned())
		.filter(|name| !name.is_empty())
		.or_else(|| before.display_name.clone());
	let icon = edits
		.icon
		.clone()
		.or_else(|| suggestion.proposed.icon.clone())
		.filter(|icon| !icon.is_empty())
		.or_else(|| before.icon.clone());
	if let Some(icon) = &icon
		&& !is_goal_icon(icon)
	{
		return Err(refuse("Choose a symbol or one emoji."));
	}
	let prior = json!({ "display_name": before.display_name, "icon": before.icon });
	Ok(vec![
		Row::ScheduleMeta(ScheduleMetaRow {
			schedule,
			meta: ScheduleMeta {
				display_name,
				icon,
				..before
			},
		}),
		record(&suggestion.key, prior),
	])
}

/// (b) MIG-4.5, for every ticked goal in Budget screen order: the Goals
/// group (made or shown again if needed), each move to the top of it, the
/// goal row, and the opt-in `#goal` line and cells.
fn goal_rows(
	inputs: &Inputs<'_>,
	goals: &[(&Suggestion, &Edits)],
	consent_show_goals_group: bool,
) -> Result<Vec<Row>, CoreError> {
	let data = inputs.data;
	let sheet = inputs.sheet;
	let month = inputs.today.year_month();
	let adopted: BTreeSet<CategoryId> = goals
		.iter()
		.map(|(suggestion, _)| CategoryId::new(suggestion.subject_id.as_str()))
		.collect();
	let order: Vec<(CategoryGroupId, Vec<CategoryId>)> = sheet
		.group_order()
		.map(|(group, categories)| (group.clone(), categories.into_iter().cloned().collect()))
		.collect();
	let mut planned: Vec<Row> = Vec::new();
	let (group, created, unhid) = match goals_group(data) {
		Some(group) if group.hidden => {
			if !consent_show_goals_group {
				return Err(refuse(format!(
					"Your group \"{}\" is hidden. Show it again and add these goals to it?",
					group.name
				)));
			}
			planned.extend(categories::set_group_hidden(data, &group.id, false)?);
			(group.id.clone(), None, true)
		}
		Some(group) => (group.id.clone(), None, false),
		None => {
			let (rows, id) = categories::create_group(data, super::goal::GOALS)?;
			planned.extend(rows);
			(id.clone(), Some(id), false)
		}
	};
	let with_group = applied(data, &planned);
	let anchor: Option<CategoryId> =
		order
			.iter()
			.find(|(id, _)| *id == group)
			.and_then(|(_, categories)| {
				categories
					.iter()
					.find(|category| !adopted.contains(*category))
					.cloned()
			});
	let mut current = with_group;
	for (_, categories) in &order {
		for (position, category) in categories.iter().enumerate() {
			let Some((suggestion, edits)) = goals
				.iter()
				.find(|(suggestion, _)| suggestion.subject_id == category.as_str())
			else {
				continue;
			};
			let original = data
				.categories
				.get(category)
				.ok_or_else(|| refuse("That category is gone"))?;
			let neighbour = categories
				.iter()
				.skip(position + 1)
				.find(|next| !adopted.contains(*next))
				.cloned();
			let target = edits
				.target
				.or(suggestion.proposed.target)
				.filter(|target| *target > Money::ZERO)
				.ok_or_else(|| refuse(format!("Enter a target for {}", original.name)))?;
			let target_date = edits.target_date.or(suggestion.proposed.target_date);
			let mut rows = categories::move_category(&current, category, &group, anchor.as_ref())?;
			current = applied(&current, &rows);
			let mut note = Value::Null;
			let mut cells = Vec::new();
			if edits.write_note_in_actual && !suggestion.target_in_actual {
				let writes = engine::target_writes(&current, sheet, category, target, month)?;
				let written = write_rows(&current, &writes);
				for row in &written {
					match row {
						Row::Note(written) => {
							note = json!({ "id": written.id, "text": written.text });
						}
						Row::BudgetCell(cell) => cells.push(cell_json(cell)),
						_ => {}
					}
				}
				current = applied(&current, &written);
				rows.extend(written);
			}
			let mut goal = Goal {
				id: GoalId::new(veritas_store::new_id()),
				category: category.clone(),
				target,
				target_date,
				icon: None,
				color: None,
				status: GoalStatus::Active,
				reached_month: None,
				closed_month: None,
				released_to: None,
				released: None,
				activity_seen_through: None,
				activity_seen_total: None,
				close_month_dismissed_available: None,
			};
			let available = sheet
				.category(month, category)
				.map_or(Money::ZERO, |cell| cell.balance);
			if available >= target {
				goal.status = GoalStatus::Reached;
				goal.reached_month =
					engine::first_reached_month(sheet, &goal, month).or(Some(month));
			}
			let prior = json!({
				"goal": goal.id.as_str(),
				"group": original.group.as_str(),
				"before": neighbour.as_ref().map(CategoryId::as_str),
				"sort_order": original.sort_order,
				"created_group": created.as_ref().map(CategoryGroupId::as_str),
				"unhid_group": unhid,
				"note": note,
				"cells": cells,
			});
			rows.push(Row::Goal(goal));
			rows.push(record(&suggestion.key, prior));
			planned.extend(rows);
		}
	}
	Ok(planned)
}

fn cell_json(cell: &BudgetCell) -> Value {
	json!({
		"month": cell.month.to_string(),
		"goal": cell.goal.map(Money::minor),
		"long_goal": cell.long_goal,
	})
}
