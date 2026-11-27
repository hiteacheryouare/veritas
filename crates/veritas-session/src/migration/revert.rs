//! MIG-2.3's Revert: one converted item, by intent, from its stored record
//! alone, so it works after a reopen (MIG-7.4).

use serde_json::{Value, json};
use veritas_domain::{
	BudgetCell, CategoryGroupId, CategoryId, CategoryKind, CategoryMeta, CategoryMetaRow,
	CoreError, GoalId, GoalRemoval, MigrationAppliedRow, Money, Note, Row, ScheduleId,
	ScheduleMetaRow, WidgetId, WidgetOverrideRow, YearMonth,
};

use super::{Conversion, Effect, Inputs, KindChange, kind};
use crate::categories;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedItem {
	pub key: String,
	pub conversion: Conversion,
	pub subject_id: String,
}

/// Every converted item that can still be reverted.
pub fn applied_items(data: &veritas_domain::BudgetData) -> Vec<AppliedItem> {
	data.migration_applied
		.keys()
		.filter_map(|key| {
			let (conversion, subject) = Conversion::parse_key(key)?;
			Some(AppliedItem {
				key: key.clone(),
				conversion,
				subject_id: subject.to_owned(),
			})
		})
		.collect()
}

/// MIG-5.6's reversed CLS-3.3 confirm, shown before a (c) Revert writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KindRevert {
	pub change: KindChange,
	pub effects: Vec<Effect>,
	/// The kind the user set since the upgrade, which the Revert overwrites.
	pub replaces: Option<CategoryKind>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RevertPlan {
	/// Write these rows. `notes` are what Revert left alone and why.
	Rows { rows: Vec<Row>, notes: Vec<String> },
	/// MIG-4.7: the goal's original group is gone, so nothing is written
	/// until the user picks one.
	ChooseGroup { message: String },
	/// MIG-5.6: show this, then revert again with `confirmed`.
	ConfirmKind(KindRevert),
}

fn refuse(message: impl Into<String>) -> CoreError {
	CoreError::Invalid(message.into())
}

fn prior(inputs: &Inputs<'_>, key: &str) -> Result<(Conversion, String, Value), CoreError> {
	let record = inputs
		.data
		.migration_applied
		.get(key)
		.ok_or_else(|| refuse("This was already reverted."))?;
	let (conversion, subject) =
		Conversion::parse_key(key).ok_or_else(|| refuse(format!("{key} isn't an upgrade item")))?;
	let prior = serde_json::from_str(&record.prior).unwrap_or(Value::Null);
	Ok((conversion, subject.to_owned(), prior))
}

fn forget(key: &str) -> Row {
	Row::MigrationApplied(MigrationAppliedRow {
		key: key.to_owned(),
		value: None,
	})
}

/// One item's Revert. `group` answers a `ChooseGroup`, and `confirmed`
/// answers a `ConfirmKind`.
pub fn revert(
	inputs: &Inputs<'_>,
	key: &str,
	group: Option<&CategoryGroupId>,
	confirmed: bool,
) -> Result<RevertPlan, CoreError> {
	let (conversion, subject, prior) = prior(inputs, key)?;
	match conversion {
		Conversion::Report => Ok(RevertPlan::Rows {
			rows: vec![
				Row::WidgetOverride(WidgetOverrideRow {
					widget: WidgetId::new(subject.as_str()),
					value: None,
				}),
				forget(key),
			],
			notes: Vec::new(),
		}),
		Conversion::Kind if !confirmed => Ok(RevertPlan::ConfirmKind(kind_revert(inputs, key)?)),
		Conversion::Kind => revert_kind(inputs, key, &CategoryId::new(subject.as_str()), &prior),
		Conversion::Bill => {
			let schedule = ScheduleId::new(subject.as_str());
			let current = inputs
				.data
				.schedule_meta
				.get(&schedule)
				.cloned()
				.unwrap_or_default();
			let text = |field: &str| prior.get(field).and_then(Value::as_str).map(str::to_owned);
			Ok(RevertPlan::Rows {
				rows: vec![
					Row::ScheduleMeta(ScheduleMetaRow {
						schedule,
						meta: veritas_domain::ScheduleMeta {
							display_name: text("display_name"),
							icon: text("icon"),
							..current
						},
					}),
					forget(key),
				],
				notes: Vec::new(),
			})
		}
		Conversion::Goal => revert_goal(
			inputs,
			key,
			&CategoryId::new(subject.as_str()),
			&prior,
			group,
		),
	}
}

pub(super) fn meta_json(meta: &CategoryMeta) -> Value {
	json!({
		"kind": meta.kind.map(kind_text),
		"tax_bucket": meta.tax_bucket,
		"archived_on": meta.archived_on.map(|date| date.to_string()),
	})
}

pub(super) fn kind_text(kind: CategoryKind) -> &'static str {
	match kind {
		CategoryKind::Income => "income",
		CategoryKind::Expense => "expense",
		CategoryKind::DebtPrincipal => "debt_principal",
		CategoryKind::Savings => "savings",
	}
}

fn parse_kind(text: &str) -> Option<CategoryKind> {
	[
		CategoryKind::Income,
		CategoryKind::Expense,
		CategoryKind::DebtPrincipal,
		CategoryKind::Savings,
	]
	.into_iter()
	.find(|kind| kind_text(*kind) == text)
}

/// The kind the record says the category had before the upgrade.
fn prior_kind(prior: &Value) -> Option<CategoryKind> {
	prior
		.get("meta")
		.and_then(|meta| meta.get("kind"))
		.and_then(Value::as_str)
		.and_then(parse_kind)
}

/// MIG-5.6's reversed confirm: the counts and figures if the category goes
/// back to its prior kind, and any kind the user set since.
pub fn kind_revert(inputs: &Inputs<'_>, key: &str) -> Result<KindRevert, CoreError> {
	let (conversion, subject, prior) = prior(inputs, key)?;
	if conversion != Conversion::Kind {
		return Err(refuse(format!("{key} isn't a category kind")));
	}
	let category = CategoryId::new(subject.as_str());
	let applied = prior
		.get("kind")
		.and_then(Value::as_str)
		.and_then(parse_kind);
	let current = inputs
		.data
		.category_meta
		.get(&category)
		.and_then(|meta| meta.kind);
	let (change, effects) = kind::change_to(inputs, &category, prior_kind(&prior))?;
	Ok(KindRevert {
		change: KindChange {
			kind: current.or(applied).unwrap_or(change.kind),
			..change
		},
		effects,
		replaces: current.filter(|current| Some(*current) != applied),
	})
}

/// Only the kind goes back: a tax bucket or archive set since is kept.
fn revert_kind(
	inputs: &Inputs<'_>,
	key: &str,
	category: &CategoryId,
	prior: &Value,
) -> Result<RevertPlan, CoreError> {
	let current = inputs
		.data
		.category_meta
		.get(category)
		.cloned()
		.unwrap_or(CategoryMeta {
			kind: None,
			tax_bucket: None,
			archived_on: None,
		});
	Ok(RevertPlan::Rows {
		rows: vec![
			Row::CategoryMeta(CategoryMetaRow {
				category: category.clone(),
				meta: CategoryMeta {
					kind: prior_kind(prior),
					..current
				},
			}),
			forget(key),
		],
		notes: Vec::new(),
	})
}

/// MIG-4.7: back into the recorded group, directly before the recorded
/// neighbour or last, from wherever it is now. The goal row goes, and the
/// opt-in line and cells only if they are still what the upgrade wrote.
fn revert_goal(
	inputs: &Inputs<'_>,
	key: &str,
	category: &CategoryId,
	prior: &Value,
	chosen: Option<&CategoryGroupId>,
) -> Result<RevertPlan, CoreError> {
	let data = inputs.data;
	let name = data
		.categories
		.get(category)
		.filter(|category| !category.tombstone)
		.map(|category| category.name.clone())
		.ok_or_else(|| refuse("That category is gone"))?;
	let text = |field: &str| prior.get(field).and_then(Value::as_str).map(str::to_owned);
	let recorded = text("group").map(CategoryGroupId::new);
	let live = |group: &CategoryGroupId| {
		data.category_groups
			.get(group)
			.is_some_and(|group| !group.tombstone && !group.is_income)
	};
	let group = match chosen {
		Some(group) if live(group) => group.clone(),
		Some(_) => return Err(refuse("Choose a group that still exists")),
		None => match recorded.clone().filter(|group| live(group)) {
			Some(group) => group,
			None => {
				let group_name = recorded
					.as_ref()
					.and_then(|group| data.category_groups.get(group))
					.map_or_else(|| "it was in".to_owned(), |group| group.name.clone());
				return Ok(RevertPlan::ChooseGroup {
					message: format!(
						"The group {group_name} no longer exists. Choose a group for {name}."
					),
				});
			}
		},
	};
	let neighbour = text("before").map(CategoryId::new).filter(|next| {
		data.categories
			.get(next)
			.is_some_and(|next| !next.tombstone && next.group == group)
	});
	let mut rows = categories::move_category(data, category, &group, neighbour.as_ref())?;
	let goal = text("goal")
		.map(GoalId::new)
		.filter(|goal| data.goals.contains_key(goal))
		.or_else(|| {
			data.goals
				.values()
				.find(|goal| &goal.category == category)
				.map(|goal| goal.id.clone())
		});
	if let Some(goal) = goal {
		rows.push(Row::GoalRemoval(GoalRemoval {
			id: goal,
			goal: None,
		}));
	}
	let mut notes = Vec::new();
	if let Some(written) = prior.get("note").filter(|note| !note.is_null()) {
		let id = written
			.get("id")
			.and_then(Value::as_str)
			.unwrap_or(category.as_str());
		let text = written.get("text").and_then(Value::as_str);
		let now = data.notes.get(id).and_then(|note| note.as_deref());
		if now == text {
			let kept: Vec<&str> = now
				.unwrap_or("")
				.lines()
				.filter(|line| {
					!line
						.find('#')
						.is_some_and(|start| line[start..].starts_with("#goal"))
				})
				.collect();
			let kept = kept.join("\n");
			rows.push(Row::Note(Note {
				id: id.to_owned(),
				text: (!kept.trim().is_empty()).then_some(kept),
			}));
		} else {
			notes.push(format!(
				"The note for {name} changed since, so its #goal line was left as it is."
			));
		}
	}
	let mut edited = false;
	for cell in prior
		.get("cells")
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
	{
		let Some(month) = cell
			.get("month")
			.and_then(Value::as_str)
			.and_then(|month| YearMonth::parse(month).ok())
		else {
			continue;
		};
		let goal = cell
			.get("goal")
			.and_then(Value::as_i64)
			.map(Money::from_minor);
		let long_goal = cell
			.get("long_goal")
			.and_then(Value::as_bool)
			.unwrap_or(false);
		let Some(now) = data.budget_cells.get(&(month, category.clone())) else {
			continue;
		};
		if now.goal == goal && now.long_goal == long_goal {
			rows.push(Row::BudgetCell(BudgetCell {
				goal: None,
				long_goal: false,
				..now.clone()
			}));
		} else {
			edited = true;
		}
	}
	if edited {
		notes.push(format!(
			"Some of {name}'s goal figures in Actual changed since, so those months were left as they are."
		));
	}
	rows.push(forget(key));
	Ok(RevertPlan::Rows { rows, notes })
}
