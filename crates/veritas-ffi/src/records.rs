use chrono::{DateTime, Utc};
use veritas_domain::{
	Currency as CoreCurrency, Money, NewAccount as CoreNewAccount,
	NewAccountKind as CoreNewAccountKind, NumberFormat as CoreNumberFormat,
	NumberStyle as CoreNumberStyle,
};
use veritas_domain::{EntityRef as CoreEntity, ReadOnlyReason, YearMonth};
use veritas_session::{
	AccountGroupKind, BackupResult, BudgetEntry, ChangeSet, Overview as CoreOverview,
	OverviewGroup, RetentionPolicy, Snapshot, SnapshotReason,
};
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
	TaxTag,
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
			CoreEntity::TaxTag(tag) => row(EntityKind::TaxTag, tag),
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
	pub backup_folder: Option<String>,
	pub last_backup: Option<BackupInfo>,
}

/// UTC as `"YYYY-MM-DDTHH:MM:SSZ"`.
pub(crate) fn utc(at: DateTime<Utc>) -> String {
	at.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// How the last backup went. A failure never blocks editing; the shell
/// shows `problem` and offers to choose another folder.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BackupInfo {
	pub at: String,
	pub file: Option<String>,
	pub problem: Option<String>,
}

impl From<&BackupResult> for BackupInfo {
	fn from(result: &BackupResult) -> Self {
		let (file, problem) = match &result.outcome {
			Ok(file) => (Some(file.display().to_string()), None),
			Err(problem) => (None, Some(problem.clone())),
		};
		BackupInfo {
			at: utc(result.at),
			file,
			problem,
		}
	}
}

/// docs/finance/statements.md ST-9. The shell labels `Investments` as
/// "Investments & Other assets" (D61), since property lands there too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum AccountGroup {
	Cash,
	Credit,
	Investments,
	Loans,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AccountSummary {
	pub id: String,
	pub name: String,
	/// Cents as of today. Liabilities are negative, as in Actual.
	pub balance: i64,
	pub closed: bool,
	/// Deleted, but kept because it still has transactions.
	pub deleted: bool,
	/// Only a negative balance put it here, so offer "Set account type".
	pub placed_by_balance: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AccountGroupSummary {
	pub group: AccountGroup,
	pub accounts: Vec<AccountSummary>,
	pub total: i64,
}

/// The shell's first screen. A `Change` naming an account or transaction
/// means refetch it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Overview {
	pub budget_name: Option<String>,
	/// `None` until the core reads Actual's preferences; use Actual's defaults.
	pub number_format: Option<NumberFormat>,
	pub groups: Vec<AccountGroupSummary>,
}

/// What new-budget setup asks besides the name and number format.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct NewBudgetOptions {
	/// Create off-budget 401(k) and HSA accounts, so gross-paycheck
	/// contributions classify as savings (D56).
	pub retirement_401k: bool,
	pub hsa: bool,
	/// The accounts setup creates, each with its balance today.
	pub accounts: Vec<NewAccount>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct NewAccount {
	pub name: String,
	pub kind: NewAccountKind,
	/// Cents as typed. For a credit card, line of credit, loan or mortgage,
	/// what is owed, as a positive amount; the core stores it negative.
	pub starting_balance: i64,
}

/// What setup offers. The core decides each one's budget flag and side
/// (D8, docs/finance/statements.md ST-9.2): checking, savings, cash, credit
/// cards and lines of credit are on budget; the rest are off budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum NewAccountKind {
	Checking,
	Savings,
	Cash,
	CreditCard,
	LineOfCredit,
	Investment,
	Retirement,
	Property,
	Loan,
	Mortgage,
}

impl From<&NewAccount> for CoreNewAccount {
	fn from(account: &NewAccount) -> Self {
		CoreNewAccount {
			name: account.name.clone(),
			kind: match account.kind {
				NewAccountKind::Checking => CoreNewAccountKind::Checking,
				NewAccountKind::Savings => CoreNewAccountKind::Savings,
				NewAccountKind::Cash => CoreNewAccountKind::Cash,
				NewAccountKind::CreditCard => CoreNewAccountKind::CreditCard,
				NewAccountKind::LineOfCredit => CoreNewAccountKind::LineOfCredit,
				NewAccountKind::Investment => CoreNewAccountKind::Investment,
				NewAccountKind::Retirement => CoreNewAccountKind::Retirement,
				NewAccountKind::Property => CoreNewAccountKind::Property,
				NewAccountKind::Loan => CoreNewAccountKind::Loan,
				NewAccountKind::Mortgage => CoreNewAccountKind::Mortgage,
			},
			starting_balance: Money::from_minor(account.starting_balance),
		}
	}
}

/// One of Actual's currencies, for setup's currency picker.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Currency {
	/// ISO 4217, or empty for "None".
	pub code: String,
	pub name: String,
	pub symbol: String,
	pub decimal_places: u8,
	/// The number format Actual suggests with this currency.
	pub number_format: NumberStyle,
	pub symbol_first: bool,
}

impl From<&CoreCurrency> for Currency {
	fn from(currency: &CoreCurrency) -> Self {
		Currency {
			code: currency.code.to_owned(),
			name: currency.name.to_owned(),
			symbol: currency.symbol.to_owned(),
			decimal_places: currency.decimal_places,
			number_format: currency.number_format.into(),
			symbol_first: currency.symbol_first,
		}
	}
}

/// Actual's number format preferences.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct NumberFormat {
	/// An ISO 4217 code such as `"USD"`, or `None` for no currency symbol.
	pub currency: Option<String>,
	pub style: NumberStyle,
	pub hide_decimals: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum NumberStyle {
	/// `1,000.33`
	CommaDot,
	/// `1.000,33`
	DotComma,
	/// `1 000,33`
	SpaceComma,
	/// `1'000.33`
	ApostropheDot,
	/// `1,00,000.33`
	CommaDotIn,
}

impl From<NumberStyle> for CoreNumberStyle {
	fn from(style: NumberStyle) -> Self {
		match style {
			NumberStyle::CommaDot => CoreNumberStyle::CommaDot,
			NumberStyle::DotComma => CoreNumberStyle::DotComma,
			NumberStyle::SpaceComma => CoreNumberStyle::SpaceComma,
			NumberStyle::ApostropheDot => CoreNumberStyle::ApostropheDot,
			NumberStyle::CommaDotIn => CoreNumberStyle::CommaDotIn,
		}
	}
}

impl From<CoreNumberStyle> for NumberStyle {
	fn from(style: CoreNumberStyle) -> Self {
		match style {
			CoreNumberStyle::CommaDot => NumberStyle::CommaDot,
			CoreNumberStyle::DotComma => NumberStyle::DotComma,
			CoreNumberStyle::SpaceComma => NumberStyle::SpaceComma,
			CoreNumberStyle::ApostropheDot => NumberStyle::ApostropheDot,
			CoreNumberStyle::CommaDotIn => NumberStyle::CommaDotIn,
		}
	}
}

impl From<&NumberFormat> for CoreNumberFormat {
	fn from(format: &NumberFormat) -> Self {
		CoreNumberFormat {
			currency: format.currency.clone(),
			style: format.style.into(),
			hide_decimals: format.hide_decimals,
		}
	}
}

impl From<&CoreNumberFormat> for NumberFormat {
	fn from(format: &CoreNumberFormat) -> Self {
		NumberFormat {
			currency: format.currency.clone(),
			style: format.style.into(),
			hide_decimals: format.hide_decimals,
		}
	}
}

impl From<&OverviewGroup> for AccountGroupSummary {
	fn from(group: &OverviewGroup) -> Self {
		AccountGroupSummary {
			group: match group.kind {
				AccountGroupKind::Cash => AccountGroup::Cash,
				AccountGroupKind::Credit => AccountGroup::Credit,
				AccountGroupKind::Investments => AccountGroup::Investments,
				AccountGroupKind::Loans => AccountGroup::Loans,
			},
			accounts: group
				.accounts
				.iter()
				.map(|account| AccountSummary {
					id: account.id.to_string(),
					name: account.name.clone(),
					balance: account.balance.minor(),
					closed: account.closed,
					deleted: account.deleted,
					placed_by_balance: account.placed_by_balance,
				})
				.collect(),
			total: group.total.minor(),
		}
	}
}

impl From<&CoreOverview> for Overview {
	fn from(overview: &CoreOverview) -> Self {
		Overview {
			budget_name: overview.budget_name.clone(),
			number_format: overview.number_format.as_ref().map(NumberFormat::from),
			groups: overview
				.groups
				.iter()
				.map(AccountGroupSummary::from)
				.collect(),
		}
	}
}

/// One budget folder for the open-a-budget screen.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BudgetListing {
	pub id: String,
	pub name: Option<String>,
	pub folder: String,
	pub last_opened: Option<String>,
}

impl From<&BudgetEntry> for BudgetListing {
	fn from(entry: &BudgetEntry) -> Self {
		BudgetListing {
			id: entry.id.clone(),
			name: entry.name.clone(),
			folder: entry.folder.display().to_string(),
			last_opened: entry.last_opened.map(utc),
		}
	}
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
			taken_at: utc(snapshot.taken_at),
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
