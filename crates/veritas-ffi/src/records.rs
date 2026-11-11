use veritas_domain::{EntityRef as CoreEntity, ReadOnlyReason, YearMonth};
use veritas_session::{ChangeSet, RetentionPolicy, Snapshot, SnapshotReason};
use veritas_store::{Check, Violation};

use crate::{Month, ReadOnlyKind};

impl From<ReadOnlyReason> for ReadOnlyKind {
	fn from(reason: ReadOnlyReason) -> Self {
		match reason {
			ReadOnlyReason::NewerActualVersion => ReadOnlyKind::NewerActualVersion,
			ReadOnlyReason::TrackingBudget => ReadOnlyKind::TrackingBudget,
			ReadOnlyReason::OpenElsewhere => ReadOnlyKind::OpenElsewhere,
			ReadOnlyReason::AuditFailed => ReadOnlyKind::AuditFailed,
			ReadOnlyReason::NewerVeritasSchema => ReadOnlyKind::NewerVeritasSchema,
		}
	}
}

impl From<YearMonth> for Month {
	fn from(month: YearMonth) -> Self {
		Month {
			year: month.year(),
			month: month.month(),
		}
	}
}

/// What one commit changed. View models refetch the screens that show these
/// rows, or everything when `everything` is set (after a restore or reload).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Change {
	pub entities: Vec<EntityRef>,
	pub everything: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum EntityKind {
	Account,
	AccountGroup,
	CategoryGroup,
	Category,
	Payee,
	Transaction,
	BudgetCell,
	Hold,
	Rule,
	Schedule,
	Tag,
	Note,
	CategoryMapping,
	PayeeMapping,
	CategoryMeta,
	AccountMeta,
	TransactionMeta,
	Goal,
}

/// One changed row. `id` is the row's id, absent for a month's hold;
/// `month` is set for budget cells and holds.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct EntityRef {
	pub kind: EntityKind,
	pub id: Option<String>,
	pub month: Option<Month>,
}

impl From<&CoreEntity> for EntityRef {
	fn from(entity: &CoreEntity) -> Self {
		let row = |kind, id: &dyn ToString| EntityRef {
			kind,
			id: Some(id.to_string()),
			month: None,
		};
		match entity {
			CoreEntity::Account(id) => row(EntityKind::Account, id),
			CoreEntity::AccountGroup(id) => row(EntityKind::AccountGroup, id),
			CoreEntity::CategoryGroup(id) => row(EntityKind::CategoryGroup, id),
			CoreEntity::Category(id) => row(EntityKind::Category, id),
			CoreEntity::Payee(id) => row(EntityKind::Payee, id),
			CoreEntity::Transaction(id) => row(EntityKind::Transaction, id),
			CoreEntity::BudgetCell(month, category) => EntityRef {
				kind: EntityKind::BudgetCell,
				id: Some(category.to_string()),
				month: Some((*month).into()),
			},
			CoreEntity::Hold(month) => EntityRef {
				kind: EntityKind::Hold,
				id: None,
				month: Some((*month).into()),
			},
			CoreEntity::Rule(id) => row(EntityKind::Rule, id),
			CoreEntity::Schedule(id) => row(EntityKind::Schedule, id),
			CoreEntity::Tag(id) => row(EntityKind::Tag, id),
			CoreEntity::Note(id) => row(EntityKind::Note, id),
			CoreEntity::CategoryMapping(id) => row(EntityKind::CategoryMapping, id),
			CoreEntity::PayeeMapping(id) => row(EntityKind::PayeeMapping, id),
			CoreEntity::CategoryMeta(id) => row(EntityKind::CategoryMeta, id),
			CoreEntity::AccountMeta(id) => row(EntityKind::AccountMeta, id),
			CoreEntity::TransactionMeta(id) => row(EntityKind::TransactionMeta, id),
			CoreEntity::Goal(id) => row(EntityKind::Goal, id),
		}
	}
}

impl From<&ChangeSet> for Change {
	fn from(changes: &ChangeSet) -> Self {
		Change {
			entities: changes.entities.iter().map(EntityRef::from).collect(),
			everything: changes.everything,
		}
	}
}

/// Everything the app shell shows about an open budget: the read-only banner,
/// the Undo and Redo menu items, and whether a snapshot is failing.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SessionState {
	pub directory: String,
	pub read_only: Option<ReadOnlyKind>,
	pub can_undo: bool,
	pub can_redo: bool,
	/// True while the full integrity check and the open snapshot still run.
	/// The budget is usable meanwhile.
	pub checking: bool,
	pub snapshot_problem: Option<String>,
	/// Rows the loader couldn't represent. They stay in the file untouched.
	pub skipped_rows: u64,
	/// What the invariant audit found, once the background check is done.
	/// `audit_report` lists the findings.
	pub audit_findings: Option<u64>,
}

/// One problem the integrity check or invariant audit found. It names rows
/// by id only, never by amount, name or note.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AuditFinding {
	pub check: AuditCheck,
	/// SQLite's own words for damage, or why a row couldn't be loaded.
	pub detail: Option<String>,
	pub table: String,
	pub id: Option<String>,
	/// The other row involved, such as the missing one a row points at.
	pub related: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum AuditCheck {
	Damage,
	Unrepresentable,
	UnpairedTransfer,
	StrayTransferLink,
	TransferAmountMismatch,
	SplitSumMismatch,
	EmptySplit,
	ChildOfNonParent,
	MissingReference,
	DeletedReference,
	MappingChain,
	MappingCycle,
	DuplicateBudgetCell,
	UnknownTaxBucket,
}

impl From<&Violation> for AuditFinding {
	fn from(violation: &Violation) -> Self {
		let (check, detail) = match &violation.check {
			Check::Integrity(text) => (AuditCheck::Damage, Some(text.clone())),
			Check::Unrepresentable(reason) => {
				(AuditCheck::Unrepresentable, Some((*reason).to_owned()))
			}
			Check::UnpairedTransfer => (AuditCheck::UnpairedTransfer, None),
			Check::StrayTransferLink => (AuditCheck::StrayTransferLink, None),
			Check::TransferAmountMismatch => (AuditCheck::TransferAmountMismatch, None),
			Check::SplitSumMismatch => (AuditCheck::SplitSumMismatch, None),
			Check::EmptySplit => (AuditCheck::EmptySplit, None),
			Check::ChildOfNonParent => (AuditCheck::ChildOfNonParent, None),
			Check::MissingReference(what) => {
				(AuditCheck::MissingReference, Some((*what).to_owned()))
			}
			Check::DeletedReference(what) => {
				(AuditCheck::DeletedReference, Some((*what).to_owned()))
			}
			Check::MappingChain => (AuditCheck::MappingChain, None),
			Check::MappingCycle => (AuditCheck::MappingCycle, None),
			Check::DuplicateBudgetCell => (AuditCheck::DuplicateBudgetCell, None),
			Check::UnknownTaxBucket => (AuditCheck::UnknownTaxBucket, None),
		};
		AuditFinding {
			check,
			detail,
			table: violation.table.to_owned(),
			id: violation.id.clone(),
			related: violation.related.clone(),
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SnapshotKind {
	Open,
	Import,
	Export,
	VersionChange,
	BulkEdit,
	RuleApply,
	Hourly,
	Restore,
}

impl From<SnapshotReason> for SnapshotKind {
	fn from(reason: SnapshotReason) -> Self {
		match reason {
			SnapshotReason::Open => SnapshotKind::Open,
			SnapshotReason::Import => SnapshotKind::Import,
			SnapshotReason::Export => SnapshotKind::Export,
			SnapshotReason::VersionChange => SnapshotKind::VersionChange,
			SnapshotReason::BulkEdit => SnapshotKind::BulkEdit,
			SnapshotReason::RuleApply => SnapshotKind::RuleApply,
			SnapshotReason::Hourly => SnapshotKind::Hourly,
			SnapshotReason::Restore => SnapshotKind::Restore,
		}
	}
}

/// One restorable snapshot. `id` is what `restore_snapshot` takes, and
/// `taken_at` is UTC as `"YYYY-MM-DDTHH:MM:SSZ"`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SnapshotInfo {
	pub id: String,
	pub taken_at: String,
	pub reason: SnapshotKind,
}

impl From<&Snapshot> for SnapshotInfo {
	fn from(snapshot: &Snapshot) -> Self {
		SnapshotInfo {
			id: snapshot
				.path
				.file_name()
				.map(|name| name.to_string_lossy().into_owned())
				.unwrap_or_default(),
			taken_at: snapshot.taken_at.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
			reason: snapshot.reason.into(),
		}
	}
}

/// How many hourly, daily and monthly snapshots to keep, and for how many
/// days to keep the ones taken before an import, export or bulk edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct Retention {
	pub hourly: u32,
	pub daily: u32,
	pub monthly: u32,
	pub operation_days: u32,
}

impl From<RetentionPolicy> for Retention {
	fn from(policy: RetentionPolicy) -> Self {
		let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
		Retention {
			hourly: count(policy.hourly),
			daily: count(policy.daily),
			monthly: count(policy.monthly),
			operation_days: u32::try_from(policy.operation_days).unwrap_or(0),
		}
	}
}

impl From<Retention> for RetentionPolicy {
	fn from(retention: Retention) -> Self {
		let count = |n: u32| usize::try_from(n).unwrap_or(usize::MAX);
		RetentionPolicy {
			hourly: count(retention.hourly),
			daily: count(retention.daily),
			monthly: count(retention.monthly),
			operation_days: i64::from(retention.operation_days),
		}
	}
}
