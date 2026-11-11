use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use chrono::{DateTime, TimeDelta, Utc};
use veritas_domain::{BudgetData, BudgetType, CivilDate, CoreError, ReadOnlyReason, Store};
use veritas_interop::{ExportOutcome, ImportOutcome, Manifest, import_actual_zip};
use veritas_store::{Access as StoreAccess, AuditReport, LoadReport, Loaded};

use crate::background::{OpenCheck, Pending};
use crate::backup::write_backup;
use crate::bus::{ChangeListener, ChangeSet, Command, CommandBus, Listeners, Permit, Subscription};
use crate::cancel::CancelToken;
use crate::fsio::{io_error, reserve_partial, sweep_stale_folders, sync_dir};
use crate::lock::{Access, WriterLock, acquire};
use crate::overview::{Overview, account_groups};
use crate::retention::RetentionPolicy;
use crate::snapshot::{
	SNAPSHOTS_DIR, Snapshot, SnapshotReason, list_snapshots, prune_snapshots, restore_snapshot,
	take_snapshot,
};

pub const DB_FILE: &str = "db.sqlite";
pub const METADATA_FILE: &str = "metadata.json";

const LOCK_ATTEMPTS: u32 = 5;
const LOCK_RETRY_DELAY: Duration = Duration::from_millis(20);
const SNAPSHOT_INTERVAL: TimeDelta = TimeDelta::hours(1);

/// What a session needs from durable storage beyond writing commands.
/// veritas-store's working copy is the real one.
pub trait Backend: Store + Sized {
	/// A `ReadOnly` open writes nothing, not even the journal mode. The
	/// session opens every budget that way first, and reopens it `ReadWrite`
	/// only once it knows the budget may be written.
	fn open(db: &Path, access: StoreAccess) -> Result<Self, CoreError>;
	fn migration_ids(&self) -> Result<Vec<i64>, CoreError>;
	/// What `PRAGMA quick_check` reports, fast enough to run while opening.
	/// Empty means the file looks sound.
	fn quick_problems(&self) -> Result<Vec<String>, CoreError>;
	/// The full `PRAGMA integrity_check` and, when the file is sound, the
	/// invariant audit, on a connection of its own. The session runs it on a
	/// background thread after opening.
	fn verify(db: &Path) -> Result<AuditReport, CoreError>;
	/// Creates the veritas_* tables. Only ever called on a writable budget.
	/// Tables from a newer Veritas return `ReadOnly(NewerVeritasSchema)`.
	fn adopt(&mut self) -> Result<(), CoreError>;
	fn load(&self) -> Result<Loaded, CoreError>;
	/// Writes an Actual zip of the budget to `to`, after the invariant audit.
	fn export_actual_zip(&self, metadata: &Path, to: &Path) -> Result<ExportOutcome, CoreError>;
	/// Builds a complete new budget (db.sqlite and metadata.json) in `dir`,
	/// which exists and is empty. The session builds in a hidden folder and
	/// renames it into place, so this needn't be atomic itself.
	fn create(dir: &Path, budget: &NewBudget) -> Result<(), CoreError>;
}

/// What setup asks for a new budget (R-IO-0).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewBudget {
	pub name: String,
	pub format: NumberFormat,
	/// Off-budget accounts for a 401(k) and an HSA, so gross-paycheck
	/// contributions classify as savings (D56, CLS-10).
	pub retirement_401k: bool,
	pub hsa: bool,
}

/// Actual's number format preferences: `numberFormat`, `hideFraction` and
/// `defaultCurrencyCode` (loot-core/src/types/prefs.ts:26-31).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NumberFormat {
	/// An ISO 4217 code, or `None` for no currency symbol, Actual's default.
	pub currency: Option<String>,
	pub style: NumberStyle,
	pub hide_decimals: bool,
}

/// Actual's `numberFormat` values (loot-core/src/shared/util.ts:265-268).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumberStyle {
	/// `1,000.33`, Actual's default.
	CommaDot,
	/// `1.000,33`
	DotComma,
	/// `1 000,33`
	SpaceComma,
	/// `1'000.33`
	ApostropheDot,
}

/// How the last backup went. Backup failures are reported here and never
/// block editing (R-REL-4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupResult {
	pub at: DateTime<Utc>,
	/// The backup file, or what went wrong, in words for Ryan.
	pub outcome: Result<PathBuf, String>,
}

pub type Clock = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>;

#[derive(Clone)]
pub struct SessionOptions {
	pub retention: RetentionPolicy,
	pub clock: Clock,
}

impl Default for SessionOptions {
	fn default() -> Self {
		SessionOptions {
			retention: RetentionPolicy::default(),
			clock: Arc::new(|| DateTime::<Utc>::from(SystemTime::now())),
		}
	}
}

enum State<B: Backend> {
	Open(Box<CommandBus<B>>),
	/// A reopen failed. Subscribers wait here until `reload` succeeds.
	Closed(Listeners),
}

/// One open budget directory: its lock, its loaded data, the command bus and
/// its snapshots.
pub struct Session<B: Backend> {
	dir: PathBuf,
	db: PathBuf,
	snapshots: PathBuf,
	lock: Option<WriterLock>,
	options: SessionOptions,
	state: State<B>,
	load_report: LoadReport,
	dirty: bool,
	last_snapshot: Option<DateTime<Utc>>,
	open_check: Option<Pending>,
	snapshot_problem: Option<CoreError>,
	audit: Option<AuditReport>,
	backup_folder: Option<PathBuf>,
	last_backup: Option<BackupResult>,
}

impl<B: Backend> Session<B> {
	/// Opens a budget directory. Another writer, a file from a newer Actual, a
	/// failed quick check or a tracking budget all open read-only. The full
	/// integrity check, the audit and the open snapshot then run in the
	/// background. Commands are allowed meanwhile, because the quick check
	/// passed. If the full check fails, the budget turns read-only and
	/// subscribers are told to refetch.
	pub fn open(dir: &Path, options: SessionOptions) -> Result<Self, CoreError> {
		let db = dir.join(DB_FILE);
		if !db.is_file() {
			return Err(CoreError::Io(format!(
				"{} doesn't hold a budget. Choose the folder Veritas imported it into",
				dir.display()
			)));
		}
		let lock = acquire_with_retry(dir)?;
		let (bus, load_report) =
			open_bus(&db, lock.as_ref(), Listeners::default()).map_err(|(error, _)| error)?;
		let mut session = Session {
			dir: dir.to_path_buf(),
			db,
			snapshots: dir.join(SNAPSHOTS_DIR),
			lock,
			options,
			state: State::Open(Box::new(bus)),
			load_report,
			dirty: false,
			last_snapshot: None,
			open_check: None,
			snapshot_problem: None,
			audit: None,
			backup_folder: None,
			last_backup: None,
		};
		let take_snapshot = session.read_only().is_none();
		session.open_check = Some(
			OpenCheck {
				db: session.db.clone(),
				snapshots: session.snapshots.clone(),
				retention: session.options.retention,
				clock: session.options.clock.clone(),
				take_snapshot,
				verify: B::verify,
			}
			.start(),
		);
		Ok(session)
	}

	/// Starts a brand-new budget in `budget_dir`, which must not exist yet,
	/// from Veritas's pinned template, and opens it. No Actual export is
	/// needed (D52, D53).
	pub fn create_budget(
		budget_dir: &Path,
		budget: &NewBudget,
		options: SessionOptions,
	) -> Result<Self, CoreError> {
		Session::create_budget_until(budget_dir, budget, options, &CancelToken::default())
	}

	pub fn create_budget_until(
		budget_dir: &Path,
		budget: &NewBudget,
		options: SessionOptions,
		cancel: &CancelToken,
	) -> Result<Self, CoreError> {
		if budget.name.trim().is_empty() {
			return Err(CoreError::Invalid(
				"Enter a name for the budget.".to_owned(),
			));
		}
		let trimmed = NewBudget {
			name: budget.name.trim().to_owned(),
			..budget.clone()
		};
		build_aside(budget_dir, "creating", cancel, |staging| {
			B::create(staging, &trimmed)
		})?;
		Session::open(budget_dir, options)
	}

	/// Copies an Actual export into `budget_dir`, which must not exist yet,
	/// and opens the copy.
	pub fn import_actual_zip(
		zip: &Path,
		budget_dir: &Path,
		options: SessionOptions,
	) -> Result<Self, CoreError> {
		Session::import_actual_zip_until(zip, budget_dir, options, &CancelToken::default())
	}

	/// `import_actual_zip`, built in a hidden folder and renamed into place,
	/// so a crash or a cancel never leaves half a budget at `budget_dir`.
	pub fn import_actual_zip_until(
		zip: &Path,
		budget_dir: &Path,
		options: SessionOptions,
		cancel: &CancelToken,
	) -> Result<Self, CoreError> {
		build_aside(budget_dir, "importing", cancel, |staging| {
			// The importer creates its own folder, so it gets a fresh name.
			std::fs::remove_dir(staging).map_err(io_error("prepare", staging))?;
			match import_actual_zip(zip, staging)? {
				ImportOutcome::Imported(_) => Ok(()),
				ImportOutcome::Refused(report) => Err(CoreError::AuditFailed {
					violations: report.violations.len(),
				}),
			}
		})?;
		Session::open(budget_dir, options)
	}

	pub fn dir(&self) -> &Path {
		&self.dir
	}

	pub fn read_only(&self) -> Option<ReadOnlyReason> {
		match &self.state {
			State::Open(bus) => bus.read_only(),
			State::Closed(_) => None,
		}
	}

	/// Rows the loader couldn't represent. They stay in the file untouched.
	pub fn load_report(&self) -> &LoadReport {
		&self.load_report
	}

	pub fn data(&self) -> Result<&BudgetData, CoreError> {
		self.bus().map(CommandBus::data)
	}

	pub fn can_undo(&self) -> bool {
		self.bus().is_ok_and(CommandBus::can_undo)
	}

	pub fn can_redo(&self) -> bool {
		self.bus().is_ok_and(CommandBus::can_redo)
	}

	/// Why the background open snapshot failed, until a later snapshot works.
	/// A failed open snapshot never stops the budget opening.
	pub fn snapshot_problem(&self) -> Option<&CoreError> {
		self.snapshot_problem.as_ref()
	}

	/// What the background check found: SQLite's integrity check and the
	/// invariant audit. `None` until the check finishes, or when it couldn't
	/// run. Findings are reported and never fixed; only damage SQLite itself
	/// finds turns the budget read-only.
	pub fn audit_report(&self) -> Option<&AuditReport> {
		self.audit.as_ref()
	}

	/// True while the full integrity check and open snapshot still run.
	pub fn open_check_pending(&self) -> bool {
		self.open_check.is_some()
	}

	pub fn retention(&self) -> RetentionPolicy {
		self.options.retention
	}

	/// Takes effect at the next prune.
	pub fn set_retention(&mut self, retention: RetentionPolicy) {
		self.options.retention = retention;
	}

	/// Picks up the background open check if it has finished. Returns what
	/// subscribers were told when it turned the budget read-only.
	pub fn poll(&mut self) -> Option<ChangeSet> {
		self.settle_open_check(false)
	}

	/// Waits for the background open check, for callers that need its verdict.
	pub fn wait_for_open_check(&mut self) -> Option<ChangeSet> {
		self.settle_open_check(true)
	}

	pub fn perform<C: Command + ?Sized>(&mut self, command: &C) -> Result<ChangeSet, CoreError> {
		self.poll();
		if let Some(reason) = command.snapshot_first() {
			self.snapshot(reason)?;
		}
		let changed = self.bus_mut()?.perform(command)?;
		self.dirty |= !changed.is_empty();
		Ok(changed)
	}

	pub fn undo(&mut self) -> Result<ChangeSet, CoreError> {
		self.poll();
		let changed = self.bus_mut()?.undo()?;
		self.dirty |= !changed.is_empty();
		Ok(changed)
	}

	pub fn redo(&mut self) -> Result<ChangeSet, CoreError> {
		self.poll();
		let changed = self.bus_mut()?.redo()?;
		self.dirty |= !changed.is_empty();
		Ok(changed)
	}

	pub fn subscribe(&mut self, listener: Arc<dyn ChangeListener>) -> Subscription {
		match &mut self.state {
			State::Open(bus) => bus.subscribe(listener),
			State::Closed(listeners) => listeners.subscribe(listener),
		}
	}

	pub fn unsubscribe(&mut self, subscription: Subscription) {
		match &mut self.state {
			State::Open(bus) => bus.unsubscribe(subscription),
			State::Closed(listeners) => listeners.unsubscribe(subscription),
		}
	}

	/// Takes the hourly snapshot when the budget changed and the last snapshot
	/// is an hour old. The app calls this on a timer.
	pub fn snapshot_if_due(&mut self) -> Result<Option<Snapshot>, CoreError> {
		self.poll();
		if self.open_check.is_some() {
			return Ok(None);
		}
		let due = self
			.last_snapshot
			.is_none_or(|last| self.now() - last >= SNAPSHOT_INTERVAL);
		if self.read_only().is_some() || !self.dirty || !due {
			return Ok(None);
		}
		self.snapshot(SnapshotReason::Hourly).map(Some)
	}

	/// Snapshots the budget as it is now, then prunes old snapshots.
	pub fn snapshot(&mut self, reason: SnapshotReason) -> Result<Snapshot, CoreError> {
		self.ensure_writer()?;
		let now = self.now();
		let snapshot = take_snapshot(&self.db, &self.snapshots, reason, now)?;
		self.snapshot_problem = None;
		self.dirty = false;
		self.last_snapshot = Some(now);
		prune_snapshots(&self.snapshots, &self.options.retention, now)?;
		Ok(snapshot)
	}

	pub fn snapshots(&self) -> Result<Vec<Snapshot>, CoreError> {
		list_snapshots(&self.snapshots)
	}

	/// The shell's first screen: the budget's name and its accounts in the
	/// sidebar groups (docs/finance/statements.md ST-9), balances as of today.
	pub fn overview(&self) -> Result<Overview, CoreError> {
		let today = CivilDate::from(self.now().date_naive());
		Ok(Overview {
			budget_name: self.budget_name()?,
			number_format: None,
			groups: account_groups(self.data()?, today)?,
		})
	}

	/// `budgetName` from metadata.json, as Actual's budget list shows it.
	pub fn budget_name(&self) -> Result<Option<String>, CoreError> {
		budget_name(&self.dir)
	}

	/// Writes an Actual zip of the budget to `to` (R-IO-2). A writable budget
	/// is snapshotted first. The invariant audit runs before anything is
	/// written: SQLite damage refuses the export, and other findings come
	/// back with it.
	pub fn export_actual_zip(&mut self, to: &Path) -> Result<AuditReport, CoreError> {
		self.export_actual_zip_until(to, &CancelToken::default())
	}

	/// `export_actual_zip`, written beside `to` and renamed over it only if
	/// the export wasn't cancelled meanwhile.
	pub fn export_actual_zip_until(
		&mut self,
		to: &Path,
		cancel: &CancelToken,
	) -> Result<AuditReport, CoreError> {
		self.poll();
		if self.ensure_writer().is_ok() {
			self.snapshot(SnapshotReason::Export)?;
		}
		let metadata = self.dir.join(METADATA_FILE);
		let parent = to.parent().unwrap_or(Path::new("."));
		let name = to
			.file_name()
			.map(|name| name.to_string_lossy().into_owned())
			.ok_or_else(|| CoreError::Invalid("Choose a file to export to.".to_owned()))?;
		let (aside, reserved) = reserve_partial(parent, &name)?;
		drop(reserved);
		let exported = self
			.bus()
			.and_then(|bus| bus.store().export_actual_zip(&metadata, &aside))
			.and_then(|outcome| match outcome {
				ExportOutcome::Exported(report) => Ok(report),
				ExportOutcome::Refused(report) => Err(CoreError::AuditFailed {
					violations: report.violations.len(),
				}),
			})
			.and_then(|report| cancel.check().map(|()| report))
			.and_then(|report| {
				std::fs::rename(&aside, to)
					.map(|()| report)
					.map_err(io_error("save the export as", to))
			});
		if exported.is_err() {
			let _ = std::fs::remove_file(&aside);
		}
		sync_dir(parent);
		exported
	}

	/// Where backups go. Swift holds the folder's security-scoped access and
	/// sets this each time it opens the budget.
	pub fn set_backup_folder(&mut self, folder: Option<PathBuf>) {
		self.backup_folder = folder;
	}

	pub fn backup_folder(&self) -> Option<&Path> {
		self.backup_folder.as_deref()
	}

	pub fn last_backup(&self) -> Option<&BackupResult> {
		self.last_backup.as_ref()
	}

	/// Writes today's backup into the backup folder as `YYYY-MM-DD-<budget
	/// name>.zip`, the name Actual gives its own exports, replacing an
	/// earlier one from the same day whole. The result is also kept for
	/// `last_backup`.
	pub fn back_up_now(&mut self) -> Result<PathBuf, CoreError> {
		let folder = self
			.backup_folder
			.clone()
			.ok_or_else(|| CoreError::Invalid("Choose a folder for backups first.".to_owned()))?;
		let at = self.now();
		let result = self.write_backup_into(&folder, at);
		self.last_backup = Some(BackupResult {
			at,
			outcome: result
				.as_ref()
				.map(Clone::clone)
				.map_err(ToString::to_string),
		});
		result
	}

	fn write_backup_into(
		&mut self,
		folder: &Path,
		at: DateTime<Utc>,
	) -> Result<PathBuf, CoreError> {
		let name = backup_file_name(self.budget_name()?.as_deref(), at);
		let (scratch, reserved) = reserve_partial(&self.dir, "backup.zip")?;
		drop(reserved);
		let metadata = self.dir.join(METADATA_FILE);
		let bytes = self
			.bus()
			.and_then(|bus| bus.store().export_actual_zip(&metadata, &scratch))
			.and_then(|outcome| match outcome {
				ExportOutcome::Exported(_) => {
					std::fs::read(&scratch).map_err(io_error("read", &scratch))
				}
				ExportOutcome::Refused(report) => Err(CoreError::AuditFailed {
					violations: report.violations.len(),
				}),
			});
		let _ = std::fs::remove_file(&scratch);
		write_backup(folder, &name, &bytes?)
	}

	/// Puts the budget back to a snapshot, named by its file name, and reloads
	/// it. The current state is snapshotted first, and undo history is
	/// cleared because it describes a budget that no longer exists.
	pub fn restore(&mut self, snapshot_id: &str) -> Result<ChangeSet, CoreError> {
		self.ensure_writer()?;
		let snapshot = self.find_snapshot(snapshot_id)?;
		self.settle_open_check(true);
		let listeners = self.close_bus();
		let restored = match &self.lock {
			Some(lock) => restore_snapshot(
				lock,
				&snapshot.path,
				&self.db,
				&self.snapshots,
				(self.options.clock)(),
			),
			None => Err(CoreError::ReadOnly(ReadOnlyReason::OpenElsewhere)),
		};
		let reopened = self.reopen(listeners);
		let safety = restored?;
		reopened?;
		if safety.is_some() {
			self.last_snapshot = Some(self.now());
		}
		self.dirty = false;
		Ok(self.announce_reload())
	}

	/// Drops everything in memory and loads the budget from disk again, for
	/// recovering after a bug left memory in doubt. Undo history is cleared.
	pub fn reload(&mut self) -> Result<ChangeSet, CoreError> {
		self.settle_open_check(true);
		let listeners = self.close_bus();
		self.reopen(listeners)?;
		Ok(self.announce_reload())
	}

	fn settle_open_check(&mut self, wait: bool) -> Option<ChangeSet> {
		let pending = self.open_check.take()?;
		if !wait && !pending.is_finished() {
			self.open_check = Some(pending);
			return None;
		}
		let checked = pending.wait();
		match checked.snapshot {
			Some(Ok(snapshot)) => {
				self.last_snapshot = Some(snapshot.taken_at);
				self.snapshot_problem = None;
			}
			Some(Err(error)) => self.snapshot_problem = Some(error),
			None => {}
		}
		let sound = checked
			.report
			.as_ref()
			.is_some_and(|report| !report.is_corrupt());
		self.audit = checked.report;
		if sound {
			return None;
		}
		let State::Open(bus) = &mut self.state else {
			return None;
		};
		if rank(Some(ReadOnlyReason::AuditFailed)) >= rank(bus.read_only()) {
			return None;
		}
		bus.make_read_only(ReadOnlyReason::AuditFailed);
		let changed = ChangeSet::everything();
		bus.listeners().notify(&changed);
		Some(changed)
	}

	fn bus(&self) -> Result<&CommandBus<B>, CoreError> {
		match &self.state {
			State::Open(bus) => Ok(bus),
			State::Closed(_) => Err(closed()),
		}
	}

	fn bus_mut(&mut self) -> Result<&mut CommandBus<B>, CoreError> {
		match &mut self.state {
			State::Open(bus) => Ok(bus),
			State::Closed(_) => Err(closed()),
		}
	}

	fn close_bus(&mut self) -> Listeners {
		match std::mem::replace(&mut self.state, State::Closed(Listeners::default())) {
			State::Open(bus) => (*bus).into_listeners(),
			State::Closed(listeners) => listeners,
		}
	}

	fn reopen(&mut self, listeners: Listeners) -> Result<(), CoreError> {
		match open_bus(&self.db, self.lock.as_ref(), listeners) {
			Ok((bus, report)) => {
				self.state = State::Open(Box::new(bus));
				self.load_report = report;
				Ok(())
			}
			Err((error, listeners)) => {
				self.state = State::Closed(listeners);
				Err(error)
			}
		}
	}

	fn announce_reload(&self) -> ChangeSet {
		let changed = ChangeSet::everything();
		if let State::Open(bus) = &self.state {
			bus.listeners().notify(&changed);
		}
		changed
	}

	fn ensure_writer(&self) -> Result<(), CoreError> {
		let reason = self
			.read_only()
			.or(self.lock.is_none().then_some(ReadOnlyReason::OpenElsewhere));
		match reason {
			Some(reason) => Err(CoreError::ReadOnly(reason)),
			None => Ok(()),
		}
	}

	fn find_snapshot(&self, id: &str) -> Result<Snapshot, CoreError> {
		self.snapshots()?
			.into_iter()
			.find(|snapshot| snapshot.path.file_name().is_some_and(|name| name == id))
			.ok_or_else(|| CoreError::NotFound {
				entity: "snapshot",
				id: id.to_owned(),
			})
	}

	fn now(&self) -> DateTime<Utc> {
		(self.options.clock)()
	}
}

/// Lower ranks win: a reason about the file itself beats one about who else
/// has it open.
fn rank(reason: Option<ReadOnlyReason>) -> u8 {
	match reason {
		Some(ReadOnlyReason::NewerActualVersion) => 0,
		Some(ReadOnlyReason::AuditFailed) => 1,
		Some(ReadOnlyReason::NewerVeritasSchema) => 2,
		Some(ReadOnlyReason::TrackingBudget) => 3,
		Some(ReadOnlyReason::OpenElsewhere) => 4,
		None => 5,
	}
}

fn closed() -> CoreError {
	CoreError::Storage("the budget couldn't be reopened. Close it and open it again".to_owned())
}

/// A writer that just closed can hold the lock for a moment longer, for
/// example while a child process it spawned shares its open files.
fn acquire_with_retry(dir: &Path) -> Result<Option<WriterLock>, CoreError> {
	let mut attempt = 1;
	loop {
		match acquire(dir)? {
			Access::Writer(lock) => return Ok(Some(lock)),
			Access::ReadOnly(_) if attempt < LOCK_ATTEMPTS => {
				attempt += 1;
				std::thread::sleep(LOCK_RETRY_DELAY);
			}
			Access::ReadOnly(_) => return Ok(None),
		}
	}
}

/// Reasons a file can't be written come first (newer Actual, failed check,
/// newer Veritas tables, tracking budget), ahead of `OpenElsewhere`, which
/// goes away on its own. On failure the listeners come back.
fn open_bus<B: Backend>(
	db: &Path,
	lock: Option<&WriterLock>,
	listeners: Listeners,
) -> Result<(CommandBus<B>, LoadReport), (CoreError, Listeners)> {
	match open_backend::<B>(db, lock.is_some()) {
		Ok((backend, loaded, reason)) => {
			let permit = match (reason, lock) {
				(None, Some(lock)) => Permit::Write(lock),
				(reason, _) => Permit::ReadOnly(reason.unwrap_or(ReadOnlyReason::OpenElsewhere)),
			};
			Ok((
				CommandBus::with_listeners(loaded.data, backend, permit, listeners),
				loaded.report,
			))
		}
		Err(error) => Err((error, listeners)),
	}
}

/// Reads everything that decides access through a read-only open, so a
/// budget that turns out read-only is never written. Only a writable one is
/// reopened for writing and adopted. The data loaded read-only is kept: the
/// writer lock means nothing else wrote in between, and adopt only adds
/// empty veritas_* tables.
fn open_backend<B: Backend>(
	db: &Path,
	writable: bool,
) -> Result<(B, Loaded, Option<ReadOnlyReason>), CoreError> {
	let reader = B::open(db, StoreAccess::ReadOnly(ReadOnlyReason::OpenElsewhere))?;
	let mut reason = match Manifest::supported()?.check(&reader.migration_ids()?)? {
		veritas_interop::Access::ReadWrite => None,
		veritas_interop::Access::ReadOnly(reason) => Some(reason),
	};
	if reason.is_none() && !reader.quick_problems()?.is_empty() {
		reason = Some(ReadOnlyReason::AuditFailed);
	}
	let loaded = reader.load()?;
	if reason.is_none() && loaded.data.budget_type == BudgetType::Tracking {
		reason = Some(ReadOnlyReason::TrackingBudget);
	}
	if reason.is_some() || !writable {
		let reason = reason.unwrap_or(ReadOnlyReason::OpenElsewhere);
		return Ok((reader, loaded, Some(reason)));
	}
	drop(reader);
	let mut writer = B::open(db, StoreAccess::ReadWrite)?;
	match writer.adopt() {
		Ok(()) => Ok((writer, loaded, None)),
		Err(CoreError::ReadOnly(newer)) => {
			drop(writer);
			let reader = B::open(db, StoreAccess::ReadOnly(newer))?;
			Ok((reader, loaded, Some(newer)))
		}
		Err(error) => Err(error),
	}
}

pub(crate) fn budget_name(dir: &Path) -> Result<Option<String>, CoreError> {
	let path = dir.join(METADATA_FILE);
	let text = match std::fs::read_to_string(&path) {
		Ok(text) => text,
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
		Err(error) => return Err(io_error("read", &path)(error)),
	};
	let metadata: serde_json::Value = serde_json::from_str(&text).map_err(|_| {
		CoreError::NotAnActualBudget("its metadata.json isn't valid JSON".to_owned())
	})?;
	Ok(metadata
		.get("budgetName")
		.and_then(serde_json::Value::as_str)
		.map(str::to_owned))
}

/// Actual names an export `yyyy-MM-dd-<budgetName>.zip`
/// (desktop-client/src/components/settings/Export.tsx:41). Characters a file
/// name can't hold become dashes.
fn backup_file_name(budget_name: Option<&str>, at: DateTime<Utc>) -> String {
	let cleaned: String = budget_name
		.unwrap_or_default()
		.chars()
		.map(|c| {
			if matches!(c, '/' | '\\' | ':') || c.is_control() {
				'-'
			} else {
				c
			}
		})
		.collect();
	let cleaned = cleaned.trim().trim_start_matches('.');
	let name = if cleaned.is_empty() {
		"Budget"
	} else {
		cleaned
	};
	format!("{}-{name}.zip", at.format("%Y-%m-%d"))
}

/// Builds a budget folder in a hidden sibling of `budget_dir`, then renames
/// it into place unless cancelled. On any failure the sibling is removed.
fn build_aside(
	budget_dir: &Path,
	doing: &str,
	cancel: &CancelToken,
	build: impl FnOnce(&Path) -> Result<(), CoreError>,
) -> Result<(), CoreError> {
	if budget_dir.exists() {
		return Err(CoreError::Io(format!(
			"{} already exists. Choose a new folder for the budget",
			budget_dir.display()
		)));
	}
	let parent = budget_dir.parent().unwrap_or(Path::new("."));
	let folder_name = budget_dir
		.file_name()
		.map(|name| name.to_string_lossy().into_owned())
		.ok_or_else(|| CoreError::Invalid("Choose a folder for the budget.".to_owned()))?;
	let prefix = format!(".{folder_name}.{doing}-");
	sweep_stale_folders(parent, &prefix);
	let staging = reserve_folder(parent, &prefix)?;
	let built = build(&staging)
		.and_then(|()| cancel.check())
		.and_then(|()| {
			std::fs::rename(&staging, budget_dir).map_err(io_error("create", budget_dir))
		});
	if let Err(error) = built {
		let _ = std::fs::remove_dir_all(&staging);
		return Err(error);
	}
	sync_dir(parent);
	Ok(())
}

/// A fresh hidden folder `<prefix><pid>-<n>` in `parent`, unique to this
/// process and call.
fn reserve_folder(parent: &Path, prefix: &str) -> Result<PathBuf, CoreError> {
	static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
	loop {
		let staging = parent.join(format!(
			"{prefix}{}-{}",
			std::process::id(),
			NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
		));
		match std::fs::create_dir(&staging) {
			Ok(()) => return Ok(staging),
			Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
			Err(error) => return Err(io_error("create a folder in", parent)(error)),
		}
	}
}
