//! Upgrade to Veritas features (docs/finance/migration.md). Detection reads
//! the budget and writes nothing (MIG-7.2). Apply writes every ticked item as
//! one command and one undo step (MIG-1.5), and each item keeps the record
//! its own Revert restores from (MIG-7.1).

mod apply;
mod bill;
mod fingerprint;
mod goal;
mod kind;
mod report;
mod revert;

use std::collections::BTreeSet;

use veritas_domain::{
	BudgetData, CategoryKind, CivilDate, CoreError, Money, Row, Setting, SettingKey, WidgetId,
	WidgetOverride, YearMonth,
};
use veritas_engine::EnvelopeSheet;
use veritas_reports::Classification;

pub use apply::{Edits, Selected, Selection, apply, preview};
pub use fingerprint::fingerprint;
pub use revert::{AppliedItem, KindRevert, RevertPlan, applied_items, kind_revert, revert};

/// Everything detection reads, as of the shell's local `today`.
pub struct Inputs<'a> {
	pub data: &'a BudgetData,
	pub sheet: &'a EnvelopeSheet,
	pub classification: &'a Classification,
	pub today: CivilDate,
	/// AI-3.6's name key, which MIG-4.1, MIG-4.2 S3 and MIG-6.2 compare by.
	pub name_key: &'a dyn Fn(&str) -> String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Conversion {
	/// (a) MIG-3: a custom report that imitates a native card.
	Report,
	/// (b) MIG-4: an envelope used as a savings target.
	Goal,
	/// (c) MIG-5: a category used for debt principal or savings transfers.
	Kind,
	/// (d) MIG-6: a bill's Veritas-only name and icon.
	Bill,
}

impl Conversion {
	pub fn prefix(self) -> &'static str {
		match self {
			Conversion::Report => "report",
			Conversion::Goal => "goal",
			Conversion::Kind => "kind",
			Conversion::Bill => "bill",
		}
	}

	/// `<conversion>:<subject id>` (MIG-1.6).
	pub fn key(self, subject: &str) -> String {
		format!("{}:{subject}", self.prefix())
	}

	pub fn parse_key(key: &str) -> Option<(Conversion, &str)> {
		let (prefix, subject) = key.split_once(':')?;
		let conversion = [
			Conversion::Report,
			Conversion::Goal,
			Conversion::Kind,
			Conversion::Bill,
		]
		.into_iter()
		.find(|conversion| conversion.prefix() == prefix)?;
		(!subject.is_empty()).then_some((conversion, subject))
	}
}

/// MIG-1.4. "Not shown" is never returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confidence {
	PreTicked,
	Shown,
}

/// A named figure behind a signal: minor units for money, a count otherwise.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Figure {
	pub name: String,
	pub value: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signal {
	pub code: String,
	pub text: String,
	pub figures: Vec<Figure>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Proposed {
	/// (b), on the budget scale. `None` with S2 alone: the user must enter it.
	pub target: Option<Money>,
	pub target_date: Option<CivilDate>,
	/// (c)
	pub kind: Option<CategoryKind>,
	/// (d)
	pub display_name: Option<String>,
	pub icon: Option<String>,
	/// (a)
	pub native_type: Option<String>,
	pub native_meta: Option<String>,
}

/// One report figure before and after the item is applied (MIG-2.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Effect {
	pub report: String,
	pub month: Option<YearMonth>,
	pub before: Money,
	pub after: Money,
}

/// MIG-2.4's goal lines for the Confirm step: the Goals screen lists the
/// goal, Wrapped counts it in the year of its backfilled `reached_month`,
/// and its DEBT-8.1 monthly suggestion and ETA appear.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoalPreview {
	/// GOAL-9's backfill when the envelope already holds the target.
	pub reached_month: Option<YearMonth>,
	/// DEBT-8.1, with a target date.
	pub monthly: Option<Money>,
	pub months_left: Option<u32>,
	/// D77: outside a 2-place budget, recorded spending means no suggestion
	/// and no ETA.
	pub spending_recorded: bool,
}

/// MIG-5.4's counts, over all history: `rows` change, `transfers` of
/// `amount` move between spending and `to`, `purchases` gain or lose the
/// misfiled flag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KindChange {
	pub rows: u32,
	pub transfers: u32,
	pub amount: Money,
	pub purchases: u32,
	pub kind: CategoryKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
	pub key: String,
	pub conversion: Conversion,
	pub subject_id: String,
	pub subject_name: String,
	pub confidence: Confidence,
	pub signals: Vec<Signal>,
	pub proposed: Proposed,
	/// Differing figures the user picks between (MIG-4.2's conflicts).
	pub conflicts: Vec<String>,
	/// Why it can't be ticked at all, such as "Rename one of them first."
	pub blocked: Option<String>,
	/// It can't be ticked while this other item is ticked (MIG-4.1).
	pub blocked_by: Option<String>,
	/// What the Confirm step says this writes besides its figures.
	pub confirm_lines: Vec<String>,
	pub effects: Vec<Effect>,
	pub kind_change: Option<KindChange>,
	/// (b) MIG-2.4's goal lines, for the proposed or edited target.
	pub goal_preview: Option<GoalPreview>,
	/// (b) Actual already shows the target (a `#goal` line or long-term goal
	/// rows), so "Also show this target in Actual" doesn't apply (MIG-4.5).
	pub target_in_actual: bool,
	/// (b) The Goals group is hidden, so Apply needs the user's yes to show
	/// it again (MIG-4.5).
	pub asks_to_show_goals_group: bool,
	pub dismissed: bool,
}

impl Suggestion {
	fn new(conversion: Conversion, subject_id: &str, subject_name: &str) -> Suggestion {
		Suggestion {
			key: conversion.key(subject_id),
			conversion,
			subject_id: subject_id.to_owned(),
			subject_name: subject_name.to_owned(),
			confidence: Confidence::Shown,
			signals: Vec::new(),
			proposed: Proposed::default(),
			conflicts: Vec::new(),
			blocked: None,
			blocked_by: None,
			confirm_lines: Vec::new(),
			effects: Vec::new(),
			kind_change: None,
			goal_preview: None,
			target_in_actual: false,
			asks_to_show_goals_group: false,
			dismissed: false,
		}
	}
}

fn signal(code: &str, text: impl Into<String>, figures: Vec<Figure>) -> Signal {
	Signal {
		code: code.to_owned(),
		text: text.into(),
		figures,
	}
}

fn figure(name: &str, value: i64) -> Figure {
	Figure {
		name: name.to_owned(),
		value,
	}
}

/// Every suggestion, recomputed from the budget each time (MIG-1.6), in
/// conversion order: reports, goals, kinds, bills. Dismissed items are
/// included with `dismissed` set, for "Show dismissed".
pub fn suggestions(inputs: &Inputs<'_>) -> Result<Vec<Suggestion>, CoreError> {
	let dismissed = dismissed_keys(inputs.data);
	let kinds = kind::suggestions(inputs)?;
	let mut all = report::suggestions(inputs)?;
	all.extend(goal::suggestions(inputs)?);
	all.extend(kinds);
	all.extend(bill::suggestions(inputs)?);
	for suggestion in &mut all {
		suggestion.dismissed = dismissed.contains(&suggestion.key);
	}
	Ok(all)
}

/// The keys in `migration.dismissed`. Text that isn't a JSON array of
/// strings reads as none dismissed.
pub fn dismissed_keys(data: &BudgetData) -> BTreeSet<String> {
	data.settings
		.get(&SettingKey::MigrationDismissed)
		.and_then(|text| serde_json::from_str::<Vec<String>>(text).ok())
		.unwrap_or_default()
		.into_iter()
		.collect()
}

pub fn was_offered(data: &BudgetData) -> bool {
	data.settings
		.get(&SettingKey::MigrationOffered)
		.is_some_and(|value| value == "1")
}

/// MIG-1.1's record that the sheet offered itself, for
/// `Session::write_outside_undo` (MIG-1.8).
pub fn offered_rows(data: &BudgetData) -> Vec<Row> {
	if was_offered(data) {
		return Vec::new();
	}
	vec![Row::Setting(Setting {
		key: SettingKey::MigrationOffered,
		value: Some("1".to_owned()),
	})]
}

/// MIG-1.6's "Don't suggest again", or "Show dismissed" bringing one back,
/// for `Session::write_outside_undo`.
pub fn dismissal_rows(
	data: &BudgetData,
	key: &str,
	dismissed: bool,
) -> Result<Vec<Row>, CoreError> {
	if Conversion::parse_key(key).is_none() {
		return Err(CoreError::Invalid(format!("{key} isn't an upgrade item")));
	}
	let mut keys = dismissed_keys(data);
	let changed = if dismissed {
		keys.insert(key.to_owned())
	} else {
		keys.remove(key)
	};
	if !changed {
		return Ok(Vec::new());
	}
	let value = (!keys.is_empty()).then(|| {
		serde_json::to_string(&keys.into_iter().collect::<Vec<_>>())
			.unwrap_or_else(|_| "[]".to_owned())
	});
	Ok(vec![Row::Setting(Setting {
		key: SettingKey::MigrationDismissed,
		value,
	})])
}

/// How Veritas shows an Actual widget (MIG-3.4, MIG-3.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OverrideState<'a> {
	/// No override: the widget shows as Actual stored it.
	None,
	/// Shown as the native card.
	Active(&'a WidgetOverride),
	/// The report or widget changed in Actual since, so the widget shows as
	/// stored again with "This report changed in Actual, so Veritas shows it
	/// again." The row is kept.
	Stale,
}

pub const STALE_OVERRIDE: &str = "This report changed in Actual, so Veritas shows it again.";

pub fn override_state<'a>(data: &'a BudgetData, widget: &WidgetId) -> OverrideState<'a> {
	let Some(row) = data.widget_overrides.get(widget) else {
		return OverrideState::None;
	};
	let current = data
		.widgets
		.get(widget)
		.and_then(|widget| Some((widget, report::widget_report(data, widget)?)))
		.map(|(widget, report)| fingerprint(report, widget));
	if current.as_deref() == Some(row.fingerprint.as_str()) {
		OverrideState::Active(row)
	} else {
		OverrideState::Stale
	}
}

/// The custom reports an active override stands in for, which Veritas's
/// report list hides unless "Show hidden reports" is on (MIG-3.4).
pub fn overridden_reports(data: &BudgetData) -> BTreeSet<veritas_domain::CustomReportId> {
	data.widget_overrides
		.keys()
		.filter(|widget| matches!(override_state(data, widget), OverrideState::Active(_)))
		.filter_map(|widget| data.widgets.get(widget))
		.filter_map(|widget| report::widget_report(data, widget))
		.map(|report| report.id.clone())
		.collect()
}

/// The last complete month before today's, whose figures the Confirm step
/// shows (MIG-5.4).
fn last_complete_month(today: CivilDate) -> Result<YearMonth, CoreError> {
	today.year_month().offset(-1)
}

#[cfg(test)]
mod tests;
