//! v25, docs/finance/migration.md: the "Upgrade to Veritas features" read,
//! its one Apply command, per-item Revert, and the two settings the sheet
//! records outside undo (MIG-1.8).

use veritas_domain::{BudgetData, CategoryGroupId, CivilDate, CoreError, Money, Row};
use veritas_engine::EnvelopeSheet;
use veritas_reports::Classification;
use veritas_session::migration::{self as core, Inputs};

use crate::budget::civil_date;
use crate::money::CategoryKind;
use crate::records::Change;
use crate::session::Session;
use crate::{FfiError, Month};

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MigrationConversion {
	/// (a) A custom report that rebuilds the native Net worth card (MIG-3).
	Report,
	/// (b) An envelope used as a savings target becomes a goal (MIG-4).
	Goal,
	/// (c) A category used for debt or savings transfers gets its kind (MIG-5).
	Kind,
	/// (d) A bill's Veritas-only name and icon (MIG-6).
	Bill,
}

impl From<core::Conversion> for MigrationConversion {
	fn from(conversion: core::Conversion) -> Self {
		match conversion {
			core::Conversion::Report => MigrationConversion::Report,
			core::Conversion::Goal => MigrationConversion::Goal,
			core::Conversion::Kind => MigrationConversion::Kind,
			core::Conversion::Bill => MigrationConversion::Bill,
		}
	}
}

/// MIG-1.4: pre-ticked, or shown unticked. "Not shown" is never sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MigrationConfidence {
	PreTicked,
	Shown,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MigrationFigure {
	pub name: String,
	/// Minor units for money, a count otherwise.
	pub value: i64,
}

/// One "Why we suggest this" line.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MigrationSignal {
	pub code: String,
	pub text: String,
	pub figures: Vec<MigrationFigure>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MigrationProposal {
	/// (b) On the budget scale. `None`: the user must enter one (MIG-4.4).
	pub target: Option<i64>,
	/// (b) `YYYY-MM-DD`.
	pub target_date: Option<String>,
	pub kind: Option<CategoryKind>,
	pub display_name: Option<String>,
	pub icon: Option<String>,
	pub native_type: Option<String>,
	pub native_meta: Option<String>,
}

/// A report figure before and after the item (MIG-2.4).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MigrationEffect {
	pub report: String,
	pub month: Option<Month>,
	pub before: i64,
	pub after: i64,
}

/// MIG-5.4's counts over all history: "{rows} rows will change: {transfers}
/// transfers of {amount} move …, and {purchases} purchases …".
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MigrationKindChange {
	pub rows: u32,
	pub transfers: u32,
	pub amount: i64,
	pub purchases: u32,
	pub kind: CategoryKind,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MigrationSuggestion {
	/// `<conversion>:<subject id>`.
	pub key: String,
	pub conversion: MigrationConversion,
	pub subject_id: String,
	pub subject_name: String,
	pub confidence: MigrationConfidence,
	pub signals: Vec<MigrationSignal>,
	pub proposal: MigrationProposal,
	/// The sources whose figures differ, which the user picks between.
	pub conflicts: Vec<String>,
	/// Why it can't be ticked, such as "Rename one of them first."
	pub blocked: Option<String>,
	/// It can't be ticked while this other key is ticked.
	pub blocked_by: Option<String>,
	/// What the Confirm step says it writes.
	pub confirm_lines: Vec<String>,
	pub effects: Vec<MigrationEffect>,
	pub kind_change: Option<MigrationKindChange>,
	/// (b) Actual already shows the target, so there's no "Also show this
	/// target in Actual" box.
	pub target_in_actual: bool,
	/// "Don't suggest again" was chosen; shown only with "Show dismissed".
	pub dismissed: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, uniffi::Record)]
pub struct MigrationEdits {
	pub target: Option<i64>,
	pub target_date: Option<String>,
	pub write_note_in_actual: bool,
	pub kind: Option<CategoryKind>,
	pub display_name: Option<String>,
	pub icon: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MigrationItem {
	pub key: String,
	pub edits: MigrationEdits,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MigrationSelection {
	pub items: Vec<MigrationItem>,
	pub consent_show_goals_group: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AppliedMigration {
	pub key: String,
	pub conversion: MigrationConversion,
	pub subject_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum MigrationRevert {
	/// Reverted as one undo step. `notes` say what was left as it is.
	Reverted { change: Change, notes: Vec<String> },
	/// Nothing written: the goal's group is gone (MIG-4.7). Revert again
	/// with the group the user picks.
	ChooseGroup { message: String },
	/// Nothing written: show MIG-5.6's reversed confirm, then revert again
	/// with `confirmed`.
	ConfirmKind {
		change: MigrationKindChange,
		effects: Vec<MigrationEffect>,
		/// A kind the user set since, which the Revert overwrites.
		replaces: Option<CategoryKind>,
	},
}

fn money(value: Money) -> i64 {
	value.minor()
}

fn kind_change(change: &core::KindChange) -> MigrationKindChange {
	MigrationKindChange {
		rows: change.rows,
		transfers: change.transfers,
		amount: money(change.amount),
		purchases: change.purchases,
		kind: change.kind.into(),
	}
}

fn effect(effect: &core::Effect) -> MigrationEffect {
	MigrationEffect {
		report: effect.report.clone(),
		month: effect.month.map(Month::from),
		before: money(effect.before),
		after: money(effect.after),
	}
}

impl From<&core::Suggestion> for MigrationSuggestion {
	fn from(suggestion: &core::Suggestion) -> Self {
		MigrationSuggestion {
			key: suggestion.key.clone(),
			conversion: suggestion.conversion.into(),
			subject_id: suggestion.subject_id.clone(),
			subject_name: suggestion.subject_name.clone(),
			confidence: match suggestion.confidence {
				core::Confidence::PreTicked => MigrationConfidence::PreTicked,
				core::Confidence::Shown => MigrationConfidence::Shown,
			},
			signals: suggestion
				.signals
				.iter()
				.map(|signal| MigrationSignal {
					code: signal.code.clone(),
					text: signal.text.clone(),
					figures: signal
						.figures
						.iter()
						.map(|figure| MigrationFigure {
							name: figure.name.clone(),
							value: figure.value,
						})
						.collect(),
				})
				.collect(),
			proposal: MigrationProposal {
				target: suggestion.proposed.target.map(money),
				target_date: suggestion.proposed.target_date.map(|date| date.to_string()),
				kind: suggestion.proposed.kind.map(Into::into),
				display_name: suggestion.proposed.display_name.clone(),
				icon: suggestion.proposed.icon.clone(),
				native_type: suggestion.proposed.native_type.clone(),
				native_meta: suggestion.proposed.native_meta.clone(),
			},
			conflicts: suggestion.conflicts.clone(),
			blocked: suggestion.blocked.clone(),
			blocked_by: suggestion.blocked_by.clone(),
			confirm_lines: suggestion.confirm_lines.clone(),
			effects: suggestion.effects.iter().map(effect).collect(),
			kind_change: suggestion.kind_change.as_ref().map(kind_change),
			target_in_actual: suggestion.target_in_actual,
			dismissed: suggestion.dismissed,
		}
	}
}

fn selection(selection: &MigrationSelection) -> Result<core::Selection, CoreError> {
	let date = |text: &Option<String>| text.as_deref().map(civil_date).transpose();
	let items = selection
		.items
		.iter()
		.map(|item| {
			Ok(core::Selected {
				key: item.key.clone(),
				edits: core::Edits {
					target: item.edits.target.map(Money::from_minor),
					target_date: date(&item.edits.target_date)?,
					write_note_in_actual: item.edits.write_note_in_actual,
					kind: item.edits.kind.map(Into::into),
					display_name: item.edits.display_name.clone(),
					icon: item.edits.icon.clone(),
				},
			})
		})
		.collect::<Result<_, CoreError>>()?;
	Ok(core::Selection {
		items,
		consent_show_goals_group: selection.consent_show_goals_group,
	})
}

fn with_inputs<T>(
	today: CivilDate,
	read: impl FnOnce(&Inputs<'_>) -> Result<T, CoreError>,
) -> impl FnOnce(&BudgetData, &EnvelopeSheet, &Classification) -> Result<T, CoreError> {
	move |data, sheet, classification| {
		read(&Inputs {
			data,
			sheet,
			classification,
			today,
			name_key: &crate::names::name_key,
		})
	}
}

#[uniffi::export]
impl Session {
	/// Every suggestion, recomputed now (MIG-1.6). Writes nothing (MIG-7.2).
	/// `today` is the local date, `YYYY-MM-DD`.
	pub fn migration_suggestions(
		&self,
		today: String,
	) -> Result<Vec<MigrationSuggestion>, FfiError> {
		self.core.call(|session| {
			let today = civil_date(&today)?;
			session.with_sheet_and_classification(
				today,
				with_inputs(today, |inputs| {
					Ok(core::suggestions(inputs)?
						.iter()
						.map(MigrationSuggestion::from)
						.collect())
				}),
			)
		})
	}

	/// MIG-1.1: whether the sheet already offered itself in this file.
	pub fn migration_offered(&self) -> Result<bool, FfiError> {
		self.core
			.call(|session| Ok(core::was_offered(session.data()?)))
	}

	/// MIG-1.1: records the offer, as a settings write that is not an undo
	/// step (MIG-1.8).
	pub fn mark_migration_offered(&self) -> Result<Change, FfiError> {
		self.core
			.call(|session| {
				let rows = core::offered_rows(session.data()?);
				session.write_outside_undo(rows)
			})
			.map(|changes| Change::from(&changes))
	}

	/// MIG-1.6: "Don't suggest again", or with `dismissed` false, "Show
	/// dismissed" bringing one back. Not an undo step (MIG-1.8).
	pub fn set_migration_dismissed(
		&self,
		key: String,
		dismissed: bool,
	) -> Result<Change, FfiError> {
		self.core
			.call(|session| {
				let rows = core::dismissal_rows(session.data()?, &key, dismissed)?;
				session.write_outside_undo(rows)
			})
			.map(|changes| Change::from(&changes))
	}

	/// MIG-1.5: every ticked item as one command and one undo step, named
	/// "Upgrade to Veritas features". A key that is no longer suggested, or a
	/// blocked one, refuses it with nothing written.
	pub fn apply_migration(
		&self,
		selection: MigrationSelection,
		today: String,
	) -> Result<Change, FfiError> {
		self.core
			.call(|session| {
				let today = civil_date(&today)?;
				let chosen = self::selection(&selection)?;
				let rows: Vec<Row> = session.with_sheet_and_classification(
					today,
					with_inputs(today, |inputs| core::apply(inputs, &chosen)),
				)?;
				session.perform(&move |_: &BudgetData| Ok(rows.clone()))
			})
			.map(|changes| Change::from(&changes))
	}

	/// The converted items a Revert can still undo, after a reopen too.
	pub fn applied_migrations(&self) -> Result<Vec<AppliedMigration>, FfiError> {
		self.core.call(|session| {
			Ok(core::applied_items(session.data()?)
				.into_iter()
				.map(|item| AppliedMigration {
					key: item.key,
					conversion: item.conversion.into(),
					subject_id: item.subject_id,
				})
				.collect())
		})
	}

	/// One item's Revert, its own undo step (MIG-2.3). `group_id` answers
	/// `ChooseGroup`; `confirmed` answers `ConfirmKind`.
	pub fn revert_migration(
		&self,
		key: String,
		group_id: Option<String>,
		confirmed: bool,
		today: String,
	) -> Result<MigrationRevert, FfiError> {
		self.core.call(|session| {
			let today = civil_date(&today)?;
			let group = group_id.map(CategoryGroupId::new);
			let plan = session.with_sheet_and_classification(
				today,
				with_inputs(today, |inputs| {
					core::revert(inputs, &key, group.as_ref(), confirmed)
				}),
			)?;
			Ok(match plan {
				core::RevertPlan::Rows { rows, notes } => {
					let changes = session.perform(&move |_: &BudgetData| Ok(rows.clone()))?;
					MigrationRevert::Reverted {
						change: Change::from(&changes),
						notes,
					}
				}
				core::RevertPlan::ChooseGroup { message } => {
					MigrationRevert::ChooseGroup { message }
				}
				core::RevertPlan::ConfirmKind(confirm) => MigrationRevert::ConfirmKind {
					change: kind_change(&confirm.change),
					effects: confirm.effects.iter().map(effect).collect(),
					replaces: confirm.replaces.map(Into::into),
				},
			})
		})
	}
}
