use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use chrono::{DateTime, TimeDelta, Utc};
use veritas_domain::{BudgetData, BudgetType, CoreError, ReadOnlyReason, Store};
use veritas_interop::{Manifest, import_actual_zip};
use veritas_store::{LoadReport, Loaded};

use crate::bus::{ChangeListener, ChangeSet, Command, CommandBus, Listeners, Permit, Subscription};
use crate::lock::{Access, WriterLock, acquire};
use crate::retention::RetentionPolicy;
use crate::snapshot::{
	SNAPSHOTS_DIR, Snapshot, SnapshotReason, list_snapshots, prune_snapshots, restore_snapshot,
	take_snapshot,
};

pub const DB_FILE: &str = "db.sqlite";

const LOCK_ATTEMPTS: u32 = 5;
const LOCK_RETRY_DELAY: Duration = Duration::from_millis(20);
const SNAPSHOT_INTERVAL: TimeDelta = TimeDelta::hours(1);

/// What a session needs from durable storage beyond writing commands.
/// veritas-store's working copy is the real one.
pub trait Backend: Store + Sized {
	fn open(db: &Path) -> Result<Self, CoreError>;
	fn migration_ids(&self) -> Result<Vec<i64>, CoreError>;
	/// What `PRAGMA integrity_check` reports. Empty means the file is sound.
	fn integrity_problems(&self) -> Result<Vec<String>, CoreError>;
	/// Creates the veritas_* tables. Only ever called on a writable budget.
	/// Tables from a newer Veritas return `ReadOnly(NewerVeritasSchema)`.
	fn adopt(&mut self) -> Result<(), CoreError>;
	fn load(&self) -> Result<Loaded, CoreError>;
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
}

impl<B: Backend> Session<B> {
	/// Opens a budget directory. Another writer, a file from a newer Actual, a
	/// failed integrity check or a tracking budget all open read-only. A
	/// writable budget is snapshotted before anything else happens.
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
		};
		if session.read_only().is_none() {
			session.snapshot(SnapshotReason::Open)?;
		}
		Ok(session)
	}

	/// Copies an Actual export into `budget_dir`, which must not exist yet,
	/// and opens the copy.
	pub fn import_actual_zip(
		zip: &Path,
		budget_dir: &Path,
		options: SessionOptions,
	) -> Result<Self, CoreError> {
		import_actual_zip(zip, budget_dir)?;
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

	pub fn perform<C: Command + ?Sized>(&mut self, command: &C) -> Result<ChangeSet, CoreError> {
		if let Some(reason) = command.snapshot_first() {
			self.snapshot(reason)?;
		}
		let changed = self.bus_mut()?.perform(command)?;
		self.dirty |= !changed.is_empty();
		Ok(changed)
	}

	pub fn undo(&mut self) -> Result<ChangeSet, CoreError> {
		let changed = self.bus_mut()?.undo()?;
		self.dirty |= !changed.is_empty();
		Ok(changed)
	}

	pub fn redo(&mut self) -> Result<ChangeSet, CoreError> {
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
		self.dirty = false;
		self.last_snapshot = Some(now);
		prune_snapshots(&self.snapshots, &self.options.retention, now)?;
		Ok(snapshot)
	}

	pub fn snapshots(&self) -> Result<Vec<Snapshot>, CoreError> {
		list_snapshots(&self.snapshots)
	}

	/// Puts the budget back to a snapshot, named by its file name, and reloads
	/// it. The current state is snapshotted first, and undo history is
	/// cleared because it describes a budget that no longer exists.
	pub fn restore(&mut self, snapshot_id: &str) -> Result<ChangeSet, CoreError> {
		self.ensure_writer()?;
		let snapshot = self.find_snapshot(snapshot_id)?;
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
		let listeners = self.close_bus();
		self.reopen(listeners)?;
		Ok(self.announce_reload())
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
	let writable = lock.is_some();
	let opened = (|| {
		let mut backend = B::open(db)?;
		let mut reason = match Manifest::supported()?.check(&backend.migration_ids()?)? {
			veritas_interop::Access::ReadWrite => None,
			veritas_interop::Access::ReadOnly(reason) => Some(reason),
		};
		if reason.is_none() && !backend.integrity_problems()?.is_empty() {
			reason = Some(ReadOnlyReason::AuditFailed);
		}
		if reason.is_none() && writable {
			match backend.adopt() {
				Ok(()) => {}
				Err(CoreError::ReadOnly(newer)) => reason = Some(newer),
				Err(error) => return Err(error),
			}
		}
		let loaded = backend.load()?;
		if reason.is_none() && loaded.data.budget_type == BudgetType::Tracking {
			reason = Some(ReadOnlyReason::TrackingBudget);
		}
		if reason.is_none() && !writable {
			reason = Some(ReadOnlyReason::OpenElsewhere);
		}
		Ok((backend, loaded, reason))
	})();
	match opened {
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
