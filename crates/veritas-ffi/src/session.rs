use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use veritas_domain::CoreError;
use veritas_session::{
	Backend, ChangeSet, Session as CoreSession, SessionOptions, SharedSession, StoreBackend,
};

use crate::commands::Command;
use crate::records::{Change, Retention, SessionState, SnapshotInfo};
use crate::{FfiError, guard};

/// Implemented in Swift. Called after every commit, undo, redo, restore or
/// reload, on the thread that made the call, once the session is free again,
/// so a listener may call straight back into the session.
#[uniffi::export(with_foreign)]
pub trait ChangeListener: Send + Sync {
	fn changed(&self, change: Change);
}

type Subscribers = Mutex<Vec<(u64, Arc<dyn ChangeListener>)>>;

/// Collects what the core announces while it holds the session, so the
/// announcements can go out after the lock is released.
#[derive(Default)]
struct Outbox(Mutex<Vec<ChangeSet>>);

impl veritas_session::ChangeListener for Outbox {
	fn changed(&self, changes: &ChangeSet) {
		self.0
			.lock()
			.unwrap_or_else(PoisonError::into_inner)
			.push(changes.clone());
	}
}

/// The FFI session over any backend, so tests can run it without SQLite
/// writes. `Session` below is this over veritas-store.
pub(crate) struct Core<B: Backend> {
	shared: SharedSession<B>,
	outbox: Arc<Outbox>,
	subscribers: Subscribers,
	next_subscription: Mutex<u64>,
}

impl<B: Backend> Core<B> {
	pub(crate) fn new(mut session: CoreSession<B>) -> Self {
		let outbox = Arc::new(Outbox::default());
		session.subscribe(outbox.clone());
		Core {
			shared: SharedSession::new(session),
			outbox,
			subscribers: Mutex::new(Vec::new()),
			next_subscription: Mutex::new(0),
		}
	}

	pub(crate) fn open(directory: &str) -> Result<Self, FfiError> {
		guard(|| CoreSession::open(Path::new(directory), SessionOptions::default())).map(Core::new)
	}

	pub(crate) fn import_actual_zip(zip: &str, into_directory: &str) -> Result<Self, FfiError> {
		guard(|| {
			CoreSession::import_actual_zip(
				Path::new(zip),
				Path::new(into_directory),
				SessionOptions::default(),
			)
		})
		.map(Core::new)
	}

	/// Runs one call on the session, then hands what changed to subscribers.
	fn call<T>(
		&self,
		call: impl FnOnce(&mut CoreSession<B>) -> Result<T, CoreError>,
	) -> Result<T, FfiError> {
		let result = guard(|| self.shared.with(call));
		self.deliver();
		result
	}

	fn deliver(&self) {
		let pending =
			std::mem::take(&mut *self.outbox.0.lock().unwrap_or_else(PoisonError::into_inner));
		if pending.is_empty() {
			return;
		}
		let subscribers: Vec<Arc<dyn ChangeListener>> = self
			.subscribers
			.lock()
			.unwrap_or_else(PoisonError::into_inner)
			.iter()
			.map(|(_, listener)| listener.clone())
			.collect();
		for changes in &pending {
			let change = Change::from(changes);
			for listener in &subscribers {
				listener.changed(change.clone());
			}
		}
	}

	pub(crate) fn state(&self) -> Result<SessionState, FfiError> {
		self.call(|session| {
			session.poll();
			Ok(SessionState {
				directory: session.dir().display().to_string(),
				read_only: session.read_only().map(Into::into),
				can_undo: session.can_undo(),
				can_redo: session.can_redo(),
				checking: session.open_check_pending(),
				snapshot_problem: session.snapshot_problem().map(ToString::to_string),
				skipped_rows: session.load_report().skipped.len() as u64,
			})
		})
	}

	pub(crate) fn perform(&self, command: Command) -> Result<Change, FfiError> {
		self.call(|session| session.perform(&command))
			.map(|changes| Change::from(&changes))
	}

	pub(crate) fn undo(&self) -> Result<Change, FfiError> {
		self.call(CoreSession::undo)
			.map(|changes| Change::from(&changes))
	}

	pub(crate) fn redo(&self) -> Result<Change, FfiError> {
		self.call(CoreSession::redo)
			.map(|changes| Change::from(&changes))
	}

	pub(crate) fn subscribe(&self, listener: Arc<dyn ChangeListener>) -> Result<u64, FfiError> {
		guard(|| {
			let mut next = self
				.next_subscription
				.lock()
				.unwrap_or_else(PoisonError::into_inner);
			let id = *next;
			*next += 1;
			self.subscribers
				.lock()
				.unwrap_or_else(PoisonError::into_inner)
				.push((id, listener));
			Ok(id)
		})
	}

	pub(crate) fn unsubscribe(&self, subscription: u64) -> Result<(), FfiError> {
		guard(|| {
			self.subscribers
				.lock()
				.unwrap_or_else(PoisonError::into_inner)
				.retain(|(id, _)| *id != subscription);
			Ok(())
		})
	}

	pub(crate) fn snapshots(&self) -> Result<Vec<SnapshotInfo>, FfiError> {
		self.call(|session| {
			session
				.snapshots()
				.map(|snapshots| snapshots.iter().map(SnapshotInfo::from).collect())
		})
	}

	pub(crate) fn restore_snapshot(&self, id: &str) -> Result<Change, FfiError> {
		self.call(|session| session.restore(id))
			.map(|changes| Change::from(&changes))
	}

	pub(crate) fn retention(&self) -> Result<Retention, FfiError> {
		self.call(|session| Ok(session.retention().into()))
	}

	pub(crate) fn set_retention(&self, retention: Retention) -> Result<(), FfiError> {
		self.call(|session| {
			session.set_retention(retention.into());
			Ok(())
		})
	}

	pub(crate) fn maintain(&self) -> Result<Change, FfiError> {
		self.call(|session| {
			let flipped = session.poll();
			session.snapshot_if_due()?;
			Ok(flipped.unwrap_or_default())
		})
		.map(|changes| Change::from(&changes))
	}

	pub(crate) fn close(&self) -> Result<(), FfiError> {
		guard(|| self.shared.close())
	}
}

/// An open budget directory. Writing needs the budget's lock: a second window
/// on the same budget opens read-only, and `state().read_only` says why.
#[derive(uniffi::Object)]
pub struct Session {
	core: Core<StoreBackend>,
}

#[uniffi::export]
impl Session {
	/// Opens a budget folder Veritas imported earlier. It is usable as soon as
	/// this returns, while the full integrity check finishes in the background.
	#[uniffi::constructor]
	pub fn open(directory: String) -> Result<Arc<Session>, FfiError> {
		Core::open(&directory).map(|core| Arc::new(Session { core }))
	}

	/// Copies an Actual export into `into_directory`, which must not exist
	/// yet, then opens it. The zip itself is only read.
	#[uniffi::constructor]
	pub fn import_actual_zip(
		zip: String,
		into_directory: String,
	) -> Result<Arc<Session>, FfiError> {
		Core::import_actual_zip(&zip, &into_directory).map(|core| Arc::new(Session { core }))
	}

	pub fn state(&self) -> Result<SessionState, FfiError> {
		self.core.state()
	}

	pub fn perform(&self, command: Command) -> Result<Change, FfiError> {
		self.core.perform(command)
	}

	pub fn undo(&self) -> Result<Change, FfiError> {
		self.core.undo()
	}

	pub fn redo(&self) -> Result<Change, FfiError> {
		self.core.redo()
	}

	/// Returns an id for `unsubscribe`.
	pub fn subscribe(&self, listener: Arc<dyn ChangeListener>) -> Result<u64, FfiError> {
		self.core.subscribe(listener)
	}

	pub fn unsubscribe(&self, subscription: u64) -> Result<(), FfiError> {
		self.core.unsubscribe(subscription)
	}

	/// Newest first.
	pub fn snapshots(&self) -> Result<Vec<SnapshotInfo>, FfiError> {
		self.core.snapshots()
	}

	/// Puts the budget back to a snapshot. The current state is snapshotted
	/// first, and undo history is cleared.
	pub fn restore_snapshot(&self, id: String) -> Result<Change, FfiError> {
		self.core.restore_snapshot(&id)
	}

	pub fn retention(&self) -> Result<Retention, FfiError> {
		self.core.retention()
	}

	pub fn set_retention(&self, retention: Retention) -> Result<(), FfiError> {
		self.core.set_retention(retention)
	}

	/// The app calls this on a timer, every few minutes. It collects the
	/// background integrity check and takes the hourly snapshot when one is
	/// due. The change is empty unless the check turned the budget read-only.
	pub fn maintain(&self) -> Result<Change, FfiError> {
		self.core.maintain()
	}

	/// Releases the budget's connection and lock now. Later calls fail.
	pub fn close(&self) -> Result<(), FfiError> {
		self.core.close()
	}
}

#[cfg(test)]
mod tests {
	use std::path::{Path, PathBuf};
	use std::sync::atomic::{AtomicUsize, Ordering};

	use veritas_domain::{Change as RowChange, StoreTransaction};
	use veritas_session::SnapshotReason;
	use veritas_store::Loaded;

	use super::*;
	use crate::ReadOnlyKind;
	use crate::records::EntityKind;

	/// veritas-store's read path with writes counted, until the store writes.
	struct Counting {
		store: veritas_store::Store,
	}

	struct CountingTransaction;

	impl veritas_domain::Store for Counting {
		type Transaction<'a> = CountingTransaction;

		fn begin(&mut self) -> Result<CountingTransaction, CoreError> {
			Ok(CountingTransaction)
		}
	}

	impl StoreTransaction for CountingTransaction {
		fn apply(&mut self, _: &RowChange) -> Result<(), CoreError> {
			Ok(())
		}

		fn commit(self) -> Result<(), CoreError> {
			Ok(())
		}

		fn rollback(self) -> Result<(), CoreError> {
			Ok(())
		}
	}

	impl Backend for Counting {
		fn open(db: &Path) -> Result<Self, CoreError> {
			veritas_store::Store::open(db).map(|store| Counting { store })
		}

		fn migration_ids(&self) -> Result<Vec<i64>, CoreError> {
			self.store.migration_ids()
		}

		fn quick_problems(&self) -> Result<Vec<String>, CoreError> {
			Ok(Vec::new())
		}

		fn verify(_: &Path) -> Result<Vec<String>, CoreError> {
			Ok(Vec::new())
		}

		fn adopt(&mut self) -> Result<(), CoreError> {
			self.store.adopt()
		}

		fn load(&self) -> Result<Loaded, CoreError> {
			self.store.load()
		}
	}

	struct Scratch(PathBuf);

	impl Scratch {
		fn new(label: &str) -> Self {
			static NEXT: AtomicUsize = AtomicUsize::new(0);
			let path = Path::new(env!("CARGO_MANIFEST_DIR"))
				.join("../../target/tmp")
				.join(format!(
					"ffi-{label}-{}-{}",
					std::process::id(),
					NEXT.fetch_add(1, Ordering::Relaxed)
				));
			let _ = std::fs::remove_dir_all(&path);
			std::fs::create_dir_all(&path).unwrap();
			Scratch(path)
		}

		fn budget(&self) -> String {
			self.0.join("budget").display().to_string()
		}
	}

	impl Drop for Scratch {
		fn drop(&mut self) {
			let _ = std::fs::remove_dir_all(&self.0);
		}
	}

	fn small_zip() -> String {
		Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("../../fixtures/synthetic/small.zip")
			.display()
			.to_string()
	}

	fn imported(label: &str) -> (Scratch, Arc<Core<Counting>>) {
		let scratch = Scratch::new(label);
		let core = Core::<Counting>::import_actual_zip(&small_zip(), &scratch.budget()).unwrap();
		(scratch, Arc::new(core))
	}

	fn accounts(core: &Core<Counting>) -> Vec<(String, String, bool)> {
		core.call(|session| {
			Ok(session
				.data()?
				.accounts
				.values()
				.map(|account| {
					(
						account.id.to_string(),
						account.name.clone(),
						account.tombstone,
					)
				})
				.collect())
		})
		.unwrap()
	}

	fn live_account(core: &Core<Counting>) -> String {
		accounts(core)
			.into_iter()
			.find(|(_, _, tombstone)| !tombstone)
			.map(|(id, _, _)| id)
			.unwrap()
	}

	/// Calls back into the session from inside the callback, the way a view
	/// model refetching straight away would.
	struct Refetcher {
		core: Mutex<Option<Arc<Core<Counting>>>>,
		heard: Mutex<Vec<(Change, bool)>>,
	}

	impl ChangeListener for Refetcher {
		fn changed(&self, change: Change) {
			let core = self.core.lock().unwrap().clone();
			let refetched = core.is_some_and(|core| core.state().is_ok());
			self.heard.lock().unwrap().push((change, refetched));
		}
	}

	fn refetcher(core: &Arc<Core<Counting>>) -> Arc<Refetcher> {
		let listener = Arc::new(Refetcher {
			core: Mutex::new(Some(core.clone())),
			heard: Mutex::new(Vec::new()),
		});
		core.subscribe(listener.clone()).unwrap();
		listener
	}

	fn rename(account_id: &str, name: &str) -> Command {
		Command::RenameAccount {
			account_id: account_id.to_owned(),
			name: name.to_owned(),
		}
	}

	#[test]
	fn a_listener_hears_each_commit_and_can_call_straight_back_in() {
		// R-REL-5, and no deadlock when Swift refetches inside the callback.
		let (_scratch, core) = imported("listener");
		let listener = refetcher(&core);
		let account = live_account(&core);

		let change = core.perform(rename(&account, "  Everyday  ")).unwrap();
		core.undo().unwrap();
		core.redo().unwrap();

		assert_eq!(
			change.entities,
			[crate::EntityRef {
				kind: EntityKind::Account,
				id: Some(account.clone()),
				month: None,
			}]
		);
		let heard = listener.heard.lock().unwrap().clone();
		assert_eq!(heard.len(), 3);
		assert!(
			heard
				.iter()
				.all(|(heard, refetched)| *heard == change && *refetched)
		);
		assert!(accounts(&core).contains(&(account, "Everyday".to_owned(), false)));
		*listener.core.lock().unwrap() = None;
	}

	#[test]
	fn an_unsubscribed_listener_hears_nothing_more() {
		let (_scratch, core) = imported("unsubscribe");
		let listener = refetcher(&core);
		core.unsubscribe(0).unwrap();
		core.perform(rename(&live_account(&core), "Everyday"))
			.unwrap();
		assert!(listener.heard.lock().unwrap().is_empty());
		*listener.core.lock().unwrap() = None;
	}

	#[test]
	fn rename_account_validates_the_way_actual_does() {
		let (_scratch, core) = imported("rename-rules");
		let all = accounts(&core);
		let live: Vec<_> = all.iter().filter(|(_, _, tombstone)| !tombstone).collect();
		assert!(live.len() >= 2, "small has at least two open accounts");
		let (first, second) = (&live[0].0, &live[1].1);

		assert!(matches!(
			core.perform(rename("no-such-account", "Anything")),
			Err(FfiError::NotFound { .. })
		));
		assert!(matches!(
			core.perform(rename(first, "   ")),
			Err(FfiError::Invalid { .. })
		));
		match core.perform(rename(first, second)) {
			Err(FfiError::Invalid { message }) => assert!(!message.contains(second.as_str())),
			other => panic!("a duplicate name should be refused, got {other:?}"),
		}
		assert_eq!(accounts(&core), all);
		assert!(!core.state().unwrap().can_undo);
	}

	#[test]
	fn the_shell_reads_its_whole_state_in_one_call() {
		// R-REL-6: the second window shows the banner and refuses edits.
		let (scratch, core) = imported("state");
		let state = core.state().unwrap();
		assert_eq!(state.read_only, None);
		assert!(!state.can_undo);
		assert_eq!(state.snapshot_problem, None);
		assert!(state.directory.ends_with("budget"));

		let second = Core::<Counting>::open(&scratch.budget()).unwrap();
		assert_eq!(
			second.state().unwrap().read_only,
			Some(ReadOnlyKind::OpenElsewhere)
		);
		assert!(matches!(
			second.perform(rename(&live_account(&core), "Nope")),
			Err(FfiError::ReadOnly {
				reason: ReadOnlyKind::OpenElsewhere
			})
		));
	}

	#[test]
	fn restoring_a_snapshot_tells_every_listener_to_refetch_everything() {
		// R-REL-3
		let (_scratch, core) = imported("restore");
		core.call(|session| {
			session.wait_for_open_check();
			Ok(())
		})
		.unwrap();
		let snapshots = core.snapshots().unwrap();
		assert_eq!(snapshots.len(), 1);
		assert_eq!(snapshots[0].reason, crate::SnapshotKind::Open);
		assert!(snapshots[0].taken_at.ends_with('Z'));
		let listener = refetcher(&core);

		let change = core.restore_snapshot(&snapshots[0].id).unwrap();

		assert!(change.everything);
		assert_eq!(listener.heard.lock().unwrap().len(), 1);
		let reasons: Vec<_> = core
			.call(|session| session.snapshots())
			.unwrap()
			.into_iter()
			.map(|snapshot| snapshot.reason)
			.collect();
		assert!(reasons.contains(&SnapshotReason::Restore));
		*listener.core.lock().unwrap() = None;
	}

	#[test]
	fn retention_changes_round_trip() {
		let (_scratch, core) = imported("retention");
		let retention = Retention {
			hourly: 24,
			daily: 14,
			monthly: 6,
			operation_days: 7,
		};
		core.set_retention(retention).unwrap();
		assert_eq!(core.retention().unwrap(), retention);
	}

	#[test]
	fn a_closed_session_frees_the_budget_and_refuses_calls() {
		// R-REL-6
		let (scratch, core) = imported("close");
		core.close().unwrap();
		assert!(matches!(core.state(), Err(FfiError::Invalid { .. })));
		let reopened = Core::<Counting>::open(&scratch.budget()).unwrap();
		assert_eq!(reopened.state().unwrap().read_only, None);
	}
}
