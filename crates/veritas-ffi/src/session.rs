use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use veritas_domain::{CoreError, NewBudget};
use veritas_session::{
	Backend, CancelToken, ChangeSet, Session as CoreSession, SessionOptions, SharedSession,
};

use crate::blocking::{CancelToken as CancelHandle, run};
use crate::budget::BudgetMonth;
use crate::commands::Command;
use crate::records::NewBudgetOptions;
use crate::records::{
	AuditFinding, BackupInfo, Change, NumberFormat, Overview, Retention, SessionState, SnapshotInfo,
};
use crate::{FfiError, Month, guard};

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

	/// A cancel that lands after the budget opened closes it again.
	pub(crate) fn open(directory: &str, cancel: &CancelToken) -> Result<Self, FfiError> {
		guard(|| {
			let session = CoreSession::open(Path::new(directory), SessionOptions::default())?;
			if cancel.is_cancelled() {
				return Err(CoreError::Cancelled);
			}
			Ok(session)
		})
		.map(Core::new)
	}

	pub(crate) fn create_budget(
		name: &str,
		directory: &str,
		format: &NumberFormat,
		options: &NewBudgetOptions,
		cancel: &CancelToken,
	) -> Result<Self, FfiError> {
		let budget = NewBudget {
			name: name.to_owned(),
			format: format.into(),
			retirement_401k: options.retirement_401k,
			hsa: options.hsa,
			accounts: options.accounts.iter().map(Into::into).collect(),
		};
		guard(|| {
			CoreSession::create_budget_until(
				Path::new(directory),
				&budget,
				SessionOptions::default(),
				cancel,
			)
		})
		.map(Core::new)
	}

	pub(crate) fn import_actual_zip(
		zip: &str,
		into_directory: &str,
		cancel: &CancelToken,
	) -> Result<Self, FfiError> {
		guard(|| {
			CoreSession::import_actual_zip_until(
				Path::new(zip),
				Path::new(into_directory),
				SessionOptions::default(),
				cancel,
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
				audit_findings: session
					.audit_report()
					.map(|report| report.violations.len() as u64),
				backup_folder: session
					.backup_folder()
					.map(|folder| folder.display().to_string()),
				last_backup: session.last_backup().map(BackupInfo::from),
			})
		})
	}

	pub(crate) fn overview(&self) -> Result<Overview, FfiError> {
		self.call(|session| session.overview().map(|overview| Overview::from(&overview)))
	}

	pub(crate) fn budget_month(&self, month: Month, today: &str) -> Result<BudgetMonth, FfiError> {
		self.call(|session| crate::budget::budget_month(session.data()?, month, today))
	}

	pub(crate) fn export_actual_zip(
		&self,
		to: &str,
		cancel: &CancelToken,
	) -> Result<Vec<AuditFinding>, FfiError> {
		self.call(|session| {
			session
				.export_actual_zip_until(Path::new(to), cancel)
				.map(|report| report.violations.iter().map(AuditFinding::from).collect())
		})
	}

	pub(crate) fn set_backup_folder(&self, folder: Option<String>) -> Result<(), FfiError> {
		self.call(|session| {
			session.set_backup_folder(folder.map(Into::into));
			Ok(())
		})
	}

	pub(crate) fn back_up_now(&self) -> Result<String, FfiError> {
		self.call(|session| session.back_up_now().map(|file| file.display().to_string()))
	}

	pub(crate) fn audit_report(&self) -> Result<Option<Vec<AuditFinding>>, FfiError> {
		self.call(|session| {
			session.poll();
			Ok(session
				.audit_report()
				.map(|report| report.violations.iter().map(AuditFinding::from).collect()))
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
	core: Arc<Core<veritas_store::Store>>,
}

impl Session {
	fn wrap(core: Core<veritas_store::Store>) -> Arc<Session> {
		Arc::new(Session {
			core: Arc::new(core),
		})
	}
}

#[uniffi::export]
impl Session {
	/// Opens a budget folder. It is usable as soon as this returns, while the
	/// full integrity check finishes in the background. The work runs off the
	/// caller's thread; cancelling `cancel` closes the budget again.
	#[uniffi::constructor]
	pub async fn open(
		directory: String,
		cancel: Arc<CancelHandle>,
	) -> Result<Arc<Session>, FfiError> {
		run(&cancel, move |cancel| Core::open(&directory, cancel))
			.await
			.map(Session::wrap)
	}

	/// Starts a brand-new budget in `directory`, which must not exist yet, and
	/// opens it (R-IO-0). It is built aside and renamed into place, so
	/// cancelling `cancel` before then leaves nothing behind.
	#[uniffi::constructor]
	pub async fn create_budget(
		name: String,
		directory: String,
		format: NumberFormat,
		options: NewBudgetOptions,
		cancel: Arc<CancelHandle>,
	) -> Result<Arc<Session>, FfiError> {
		run(&cancel, move |cancel| {
			Core::create_budget(&name, &directory, &format, &options, cancel)
		})
		.await
		.map(Session::wrap)
	}

	/// Copies an Actual export into `into_directory`, which must not exist
	/// yet, then opens it. The zip itself is only read, and cancelling
	/// `cancel` before the copy is in place leaves nothing behind.
	#[uniffi::constructor]
	pub async fn import_actual_zip(
		zip: String,
		into_directory: String,
		cancel: Arc<CancelHandle>,
	) -> Result<Arc<Session>, FfiError> {
		run(&cancel, move |cancel| {
			Core::import_actual_zip(&zip, &into_directory, cancel)
		})
		.await
		.map(Session::wrap)
	}

	pub fn state(&self) -> Result<SessionState, FfiError> {
		self.core.state()
	}

	/// `None` while the background check still runs, or if it couldn't run.
	pub fn audit_report(&self) -> Result<Option<Vec<AuditFinding>>, FfiError> {
		self.core.audit_report()
	}

	/// The Budget screen for `month`, clamped to the months Actual has created.
	/// `today` is the local civil date, `YYYY-MM-DD`; it decides that range.
	pub fn budget_month(&self, month: Month, today: String) -> Result<BudgetMonth, FfiError> {
		self.core.budget_month(month, &today)
	}

	/// The budget's name and its accounts in the sidebar groups, balances as
	/// of today.
	pub fn overview(&self) -> Result<Overview, FfiError> {
		self.core.overview()
	}

	/// Writes an Actual zip to `to` (R-IO-2), snapshotting a writable budget
	/// first. SQLite damage refuses it with `AuditFailed`. Other audit
	/// findings come back with the export.
	pub async fn export_actual_zip(
		&self,
		to: String,
		cancel: Arc<CancelHandle>,
	) -> Result<Vec<AuditFinding>, FfiError> {
		let core = self.core.clone();
		run(&cancel, move |cancel| core.export_actual_zip(&to, cancel)).await
	}

	/// The folder `back_up_now` writes to. Swift starts security-scoped
	/// access to it before setting it and keeps that access while the budget
	/// is open.
	pub fn set_backup_folder(&self, folder: Option<String>) -> Result<(), FfiError> {
		self.core.set_backup_folder(folder)
	}

	/// Writes today's `YYYY-MM-DD-<budget name>.zip` into the backup folder
	/// and returns its path. The outcome also shows in `state().last_backup`.
	pub fn back_up_now(&self) -> Result<String, FfiError> {
		self.core.back_up_now()
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

	use veritas_session::SnapshotReason;

	use super::*;
	use crate::ReadOnlyKind;
	use crate::records::EntityKind;

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

	fn imported(label: &str) -> (Scratch, Arc<Core<veritas_store::Store>>) {
		let scratch = Scratch::new(label);
		let core = Core::<veritas_store::Store>::import_actual_zip(
			&small_zip(),
			&scratch.budget(),
			&CancelToken::default(),
		)
		.unwrap();
		(scratch, Arc::new(core))
	}

	fn accounts(core: &Core<veritas_store::Store>) -> Vec<(String, String, bool)> {
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

	fn live_account(core: &Core<veritas_store::Store>) -> String {
		accounts(core)
			.into_iter()
			.find(|(_, _, tombstone)| !tombstone)
			.map(|(id, _, _)| id)
			.unwrap()
	}

	/// Calls back into the session from inside the callback, the way a view
	/// model refetching straight away would.
	struct Refetcher {
		core: Mutex<Option<Arc<Core<veritas_store::Store>>>>,
		heard: Mutex<Vec<(Change, bool)>>,
	}

	impl ChangeListener for Refetcher {
		fn changed(&self, change: Change) {
			let core = self.core.lock().unwrap().clone();
			let refetched = core.is_some_and(|core| core.state().is_ok());
			self.heard.lock().unwrap().push((change, refetched));
		}
	}

	fn refetcher(core: &Arc<Core<veritas_store::Store>>) -> Arc<Refetcher> {
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

		let second =
			Core::<veritas_store::Store>::open(&scratch.budget(), &CancelToken::default()).unwrap();
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
		let reopened =
			Core::<veritas_store::Store>::open(&scratch.budget(), &CancelToken::default()).unwrap();
		assert_eq!(reopened.state().unwrap().read_only, None);
	}

	#[test]
	fn the_audit_report_reaches_the_shell_once_the_background_check_is_done() {
		// R-REL-2
		let (_scratch, core) = imported("audit");
		core.call(|session| {
			session.wait_for_open_check();
			Ok(())
		})
		.unwrap();
		assert_eq!(core.state().unwrap().audit_findings, Some(0));
		assert_eq!(core.audit_report().unwrap(), Some(Vec::new()));
	}

	#[test]
	fn the_shell_gets_an_overview_an_export_a_backup_and_the_budget_list() {
		// R-IO-2, R-REL-4, docs/finance/statements.md ST-9
		let (scratch, core) = imported("shell");
		let overview = core.overview().unwrap();
		assert!(overview.budget_name.is_some());
		assert_eq!(overview.groups.len(), 4);
		assert!(
			overview
				.groups
				.iter()
				.any(|group| !group.accounts.is_empty())
		);

		let zip = scratch.0.join("export.zip");
		core.export_actual_zip(&zip.display().to_string(), &CancelToken::default())
			.unwrap();
		assert!(zip.is_file());

		assert!(matches!(core.back_up_now(), Err(FfiError::Invalid { .. })));
		let folder = scratch.0.join("Backups");
		std::fs::create_dir(&folder).unwrap();
		core.set_backup_folder(Some(folder.display().to_string()))
			.unwrap();
		let file = core.back_up_now().unwrap();
		let state = core.state().unwrap();
		assert_eq!(state.backup_folder, Some(folder.display().to_string()));
		let last = state.last_backup.unwrap();
		assert_eq!(last.file, Some(file));
		assert_eq!(last.problem, None);

		let listed = crate::list_budgets(scratch.0.display().to_string()).unwrap();
		assert_eq!(listed.len(), 1);
		assert_eq!(listed[0].folder, scratch.budget());
		assert_eq!(listed[0].name, overview.budget_name);
	}

	/// Runs Actual's own API on a zip (tools/actual-oracle `dump`) with its
	/// clock pinned to `today`: it imports the budget and reports what Actual
	/// computes. `None` without the oracle, unless CI requires it.
	fn actual_reads(zip: &Path, today: &str) -> Option<serde_json::Value> {
		let oracle = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/actual-oracle");
		if !oracle.join("node_modules").exists() {
			assert!(
				std::env::var_os("VERITAS_REQUIRE_ORACLE").is_none(),
				"VERITAS_REQUIRE_ORACLE is set but tools/actual-oracle/node_modules is missing"
			);
			eprintln!("skipping the Actual side: run `cargo xtask oracle setup`");
			return None;
		}
		let output = std::process::Command::new("node")
			.current_dir(&oracle)
			.arg("src/main.ts")
			.arg("dump")
			.arg(zip)
			.args(["--today", today])
			.output()
			.expect("node runs");
		assert!(
			output.status.success(),
			"Actual couldn't read the export: {}",
			String::from_utf8_lossy(&output.stderr)
		);
		Some(serde_json::from_slice(&output.stdout).unwrap())
	}

	#[test]
	fn a_budget_created_in_veritas_exports_into_actual_with_the_same_balances() {
		// R-IO-0, R-IO-2, D52 to D56: create through the FFI with accounts of
		// several kinds and the 401(k) and HSA, export, and let Actual import it.
		use crate::{NewAccount, NewAccountKind, NewBudgetOptions, NumberFormat, NumberStyle};
		let scratch = Scratch::new("create-export");
		let account = |name: &str, kind, starting_balance| NewAccount {
			name: name.to_owned(),
			kind,
			starting_balance,
		};
		let options = NewBudgetOptions {
			retirement_401k: true,
			hsa: true,
			accounts: vec![
				account("Checking", NewAccountKind::Checking, 250_000),
				account("Savings", NewAccountKind::Savings, 1_000_000),
				account("Visa", NewAccountKind::CreditCard, 45_000),
				account("Brokerage", NewAccountKind::Investment, 900_000),
				account("Mortgage", NewAccountKind::Mortgage, 25_000_000),
				account("Car loan", NewAccountKind::Loan, 1_200_000),
			],
		};
		let format = NumberFormat {
			currency: Some("USD".to_owned()),
			style: NumberStyle::CommaDot,
			hide_decimals: false,
		};
		let core = Core::<veritas_store::Store>::create_budget(
			"Household",
			&scratch.budget(),
			&format,
			&options,
			&CancelToken::default(),
		)
		.unwrap();

		let overview = core.overview().unwrap();
		assert_eq!(overview.budget_name.as_deref(), Some("Household"));
		let balances: std::collections::BTreeMap<String, (String, i64)> = overview
			.groups
			.iter()
			.flat_map(|group| group.accounts.iter())
			.map(|account| (account.id.clone(), (account.name.clone(), account.balance)))
			.collect();
		assert_eq!(
			balances.len(),
			8,
			"six accounts plus the 401(k) and HSA: {balances:?}"
		);
		let by_name = |name: &str| balances.values().find(|(n, _)| n == name).unwrap().1;
		assert_eq!(by_name("Visa"), -45_000);
		assert_eq!(by_name("Mortgage"), -25_000_000);
		assert_eq!(by_name("Savings"), 1_000_000);

		let zip = scratch.0.join("Household.zip");
		let findings = core
			.export_actual_zip(&zip.display().to_string(), &CancelToken::default())
			.unwrap();
		assert!(findings.is_empty(), "{findings:?}");

		// The session dates starting balances by its clock; Actual must agree
		// on what today is.
		let today = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
			.format("%Y-%m-%d")
			.to_string();
		let Some(actual) = actual_reads(&zip, &today) else {
			return;
		};
		let computed = &actual["computed"]["accounts"];
		for (id, (name, balance)) in &balances {
			assert_eq!(
				computed[id]["balance"].as_i64(),
				Some(*balance),
				"{name}: Actual reads {}",
				computed[id]
			);
		}
		assert_eq!(computed.as_object().unwrap().len(), balances.len());
	}
	/// A fixed today keeps the month range stable.
	fn this_month() -> (Month, String) {
		(
			Month {
				year: 2026,
				month: 11,
			},
			"2026-11-15".to_owned(),
		)
	}

	fn envelopes(sheet: &crate::BudgetMonth) -> Vec<&crate::EnvelopeRow> {
		sheet
			.groups
			.iter()
			.filter(|group| !group.is_income)
			.flat_map(|group| &group.envelopes)
			.collect()
	}

	#[test]
	fn the_budget_month_lists_every_group_with_income_last() {
		// R-BUD-1
		let (_scratch, core) = imported("budget-month");
		let (month, today) = this_month();
		let sheet = core.budget_month(month, &today).unwrap();
		assert!(!sheet.groups.is_empty());
		assert!(sheet.groups.last().unwrap().is_income);
		assert!(
			sheet.groups[..sheet.groups.len() - 1]
				.iter()
				.all(|g| !g.is_income)
		);
		let budgeted: i64 = sheet
			.groups
			.iter()
			.filter(|g| !g.is_income)
			.map(|g| g.budgeted)
			.sum();
		assert_eq!(budgeted, sheet.budgeted);
		let far = Month {
			year: 2999,
			month: 1,
		};
		assert_eq!(
			core.budget_month(far, &today).unwrap().month,
			sheet.last_month
		);
	}

	#[test]
	fn setting_a_budget_is_one_undo_step() {
		// R-BUD-1, R-REL-5
		let (_scratch, core) = imported("set-budget");
		let (month, today) = this_month();
		let envelope = envelopes(&core.budget_month(month, &today).unwrap())[0].clone();
		core.perform(Command::SetBudget {
			month,
			category_id: envelope.id.clone(),
			amount: envelope.budgeted + 1234,
		})
		.unwrap();
		let after = core.budget_month(month, &today).unwrap();
		let changed = envelopes(&after)
			.into_iter()
			.find(|e| e.id == envelope.id)
			.unwrap()
			.clone();
		assert_eq!(changed.budgeted, envelope.budgeted + 1234);
		assert_eq!(changed.available, envelope.available + 1234);
		assert_eq!(
			after.to_budget,
			core.budget_month(month, &today).unwrap().to_budget
		);
		core.undo().unwrap();
		let undone = core.budget_month(month, &today).unwrap();
		assert_eq!(
			envelopes(&undone)
				.into_iter()
				.find(|e| e.id == envelope.id)
				.unwrap()
				.budgeted,
			envelope.budgeted
		);
	}

	#[test]
	fn moving_budget_takes_from_one_envelope_and_gives_to_another() {
		// R-BUD-4
		let (_scratch, core) = imported("move-budget");
		let (month, today) = this_month();
		let before = core.budget_month(month, &today).unwrap();
		let rows = envelopes(&before);
		let (from, to) = (rows[0].clone(), rows[1].clone());
		core.perform(Command::MoveBudget {
			month,
			amount: 500,
			from_category_id: Some(from.id.clone()),
			to_category_id: Some(to.id.clone()),
		})
		.unwrap();
		let after = core.budget_month(month, &today).unwrap();
		let find = |id: &str| {
			envelopes(&after)
				.into_iter()
				.find(|e| e.id == id)
				.unwrap()
				.clone()
		};
		assert_eq!(find(&from.id).budgeted, from.budgeted - 500);
		assert_eq!(find(&to.id).budgeted, to.budgeted + 500);
		assert_eq!(after.to_budget, before.to_budget);
	}

	#[test]
	fn covering_overspending_brings_the_envelope_back_to_zero() {
		// R-BUD-4
		let (_scratch, core) = imported("cover");
		let (month, today) = this_month();
		let before = core.budget_month(month, &today).unwrap();
		let rows = envelopes(&before);
		let (target, source) = (rows[0].clone(), rows[1].clone());
		core.perform(Command::SetBudget {
			month,
			category_id: target.id.clone(),
			amount: target.budgeted - target.available - 700,
		})
		.unwrap();
		core.perform(Command::SetBudget {
			month,
			category_id: source.id.clone(),
			amount: source.budgeted - source.available + 1_000,
		})
		.unwrap();
		let overspent = core.budget_month(month, &today).unwrap();
		let row = envelopes(&overspent)
			.into_iter()
			.find(|e| e.id == target.id)
			.unwrap()
			.clone();
		assert_eq!(row.available, -700);
		assert_eq!(row.tone, crate::MoneyTone::Overspent);
		core.perform(Command::CoverOverspending {
			month,
			category_id: target.id.clone(),
			from_category_id: Some(source.id.clone()),
			amount: None,
		})
		.unwrap();
		let covered = core.budget_month(month, &today).unwrap();
		let source_after = envelopes(&covered)
			.into_iter()
			.find(|e| e.id == source.id)
			.unwrap()
			.available;
		assert_eq!(source_after, 300);
		assert_eq!(
			envelopes(&covered)
				.into_iter()
				.find(|e| e.id == target.id)
				.unwrap()
				.available,
			0
		);
	}

	#[test]
	fn a_note_is_saved_and_a_blank_one_removed() {
		// R-BUD-6
		let (_scratch, core) = imported("note");
		let (month, today) = this_month();
		let id = envelopes(&core.budget_month(month, &today).unwrap())[0]
			.id
			.clone();
		let note = |text: &str| {
			core.perform(Command::SetCategoryNote {
				category_id: id.clone(),
				text: text.to_owned(),
			})
			.unwrap();
			envelopes(&core.budget_month(month, &today).unwrap())
				.into_iter()
				.find(|e| e.id == id)
				.unwrap()
				.note
				.clone()
		};
		assert_eq!(
			note("  Annual renewal in March  "),
			Some("Annual renewal in March".to_owned())
		);
		assert_eq!(note("  "), None);
	}

	#[test]
	fn commands_name_missing_envelopes() {
		let (_scratch, core) = imported("missing");
		let (month, _) = this_month();
		assert!(matches!(
			core.perform(Command::SetBudget {
				month,
				category_id: "nope".to_owned(),
				amount: 1
			}),
			Err(FfiError::NotFound { .. })
		));
	}

	fn entries(dir: &Path) -> Vec<String> {
		let mut names: Vec<String> = std::fs::read_dir(dir)
			.unwrap()
			.map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
			.collect();
		names.sort();
		names
	}

	/// Runs `work` while another thread cancels it after `delay`, the way
	/// Swift cancels the task when the user closes the setup sheet.
	fn cancelled_after<T>(
		delay: std::time::Duration,
		work: impl FnOnce(&CancelToken) -> Result<T, FfiError>,
	) -> Result<T, FfiError> {
		let cancel = CancelToken::default();
		let canceller = {
			let cancel = cancel.clone();
			std::thread::spawn(move || {
				std::thread::sleep(delay);
				cancel.cancel();
			})
		};
		let result = work(&cancel);
		canceller.join().unwrap();
		result
	}

	#[test]
	fn cancelling_a_create_or_import_at_any_moment_leaves_a_whole_budget_or_nothing() {
		// R-REL-7, PLAN §6.6: a cancel either wins, and the folder is as it was,
		// or lands after the rename, and the budget is whole and free to open.
		use crate::{NewAccount, NewAccountKind, NewBudgetOptions, NumberFormat, NumberStyle};
		let format = NumberFormat {
			currency: Some("USD".to_owned()),
			style: NumberStyle::CommaDot,
			hide_decimals: false,
		};
		let options = NewBudgetOptions {
			retirement_401k: true,
			hsa: true,
			accounts: vec![NewAccount {
				name: "Checking".to_owned(),
				kind: NewAccountKind::Checking,
				starting_balance: 120_000,
			}],
		};
		let create = |into: &str, cancel: &CancelToken| {
			Core::<veritas_store::Store>::create_budget(
				"Household",
				into,
				&format,
				&options,
				cancel,
			)
		};
		let import = |into: &str, cancel: &CancelToken| {
			Core::<veritas_store::Store>::import_actual_zip(&small_zip(), into, cancel)
		};
		let mut outcomes = std::collections::BTreeMap::new();

		for (what, start) in [
			(
				"create",
				&create as &dyn Fn(&str, &CancelToken) -> Result<Core<_>, FfiError>,
			),
			("import", &import),
		] {
			let timing = Scratch::new("cancel-timing");
			let began = std::time::Instant::now();
			start(&timing.budget(), &CancelToken::default()).unwrap();
			let takes = began.elapsed();

			for step in 0..24u32 {
				let scratch = Scratch::new("cancel");
				let delay = takes * step / 16;
				match cancelled_after(delay, |cancel| start(&scratch.budget(), cancel)) {
					Err(FfiError::Cancelled) => {
						assert_eq!(
							entries(&scratch.0),
							Vec::<String>::new(),
							"{what} at {delay:?}"
						);
						*outcomes.entry((what, "cancelled")).or_insert(0) += 1;
					}
					Ok(core) => {
						assert_eq!(entries(&scratch.0), ["budget"], "{what} at {delay:?}");
						assert_eq!(core.state().unwrap().read_only, None);
						core.close().unwrap();
						let reopened = Core::<veritas_store::Store>::open(
							&scratch.budget(),
							&CancelToken::default(),
						)
						.unwrap();
						assert_eq!(
							reopened.state().unwrap().read_only,
							None,
							"{what} at {delay:?}"
						);
						reopened
							.call(|session| {
								session.wait_for_open_check();
								Ok(())
							})
							.unwrap();
						assert_eq!(reopened.audit_report().unwrap(), Some(Vec::new()));
						*outcomes.entry((what, "finished")).or_insert(0) += 1;
					}
					Err(other) => panic!("{what} cancelled at {delay:?}: {other:?}"),
				}
			}
		}
		eprintln!("cancel outcomes: {outcomes:?}");
	}

	#[test]
	fn a_cancel_that_lands_after_open_finished_closes_the_budget_again() {
		// The token is checked once the budget is open, so a late cancel still
		// leaves no window holding the budget's lock.
		let (scratch, core) = imported("cancel-after-open");
		core.close().unwrap();
		let cancel = CancelToken::default();
		cancel.cancel();

		let opened = Core::<veritas_store::Store>::open(&scratch.budget(), &cancel);

		assert!(
			matches!(opened, Err(FfiError::Cancelled)),
			"{:?}",
			opened.err()
		);
		let reopened =
			Core::<veritas_store::Store>::open(&scratch.budget(), &CancelToken::default()).unwrap();
		assert_eq!(reopened.state().unwrap().read_only, None);
	}

	#[test]
	fn an_import_cancelled_mid_flight_leaves_nothing_behind() {
		// The 100k-transaction fixture takes long enough to cancel mid-copy.
		let scratch = Scratch::new("cancel-import");
		let zip = Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("../../fixtures/synthetic/perf-100k.zip")
			.display()
			.to_string();
		let cancel = CancelToken::default();
		let canceller = {
			let cancel = cancel.clone();
			std::thread::spawn(move || {
				std::thread::sleep(std::time::Duration::from_millis(30));
				cancel.cancel();
			})
		};

		let imported =
			Core::<veritas_store::Store>::import_actual_zip(&zip, &scratch.budget(), &cancel);

		canceller.join().unwrap();
		assert!(
			matches!(imported, Err(FfiError::Cancelled)),
			"{:?}",
			imported.err().map(|e| e.to_string())
		);
		assert_eq!(std::fs::read_dir(&scratch.0).unwrap().count(), 0);
	}
}
