use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use veritas_domain::{CoreError, NewBudget};
use veritas_session::{
	Backend, CancelToken, ChangeSet, Session as CoreSession, SessionOptions, SharedSession,
};

use crate::accounts::{
	AccountList, PayeeDetail, PayeeMemory, Reconciliation, RegisterFilter, RegisterPage,
	RegisterRow, RegisterScope, RegisterSort,
};
use crate::blocking::{CancelToken as CancelHandle, run};
use crate::budget::BudgetMonth;
use crate::commands::Command;
use crate::goals::{GoalClosePreview, GoalSummary};
use crate::records::NewBudgetOptions;
use crate::records::{
	AuditFinding, BackupInfo, CategoryHistory, Change, NumberFormat, Overview, Retention,
	SessionState, SnapshotInfo,
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
	pub(crate) fn call<T>(
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

	pub(crate) fn category_history(&self, category_id: &str) -> Result<CategoryHistory, FfiError> {
		self.call(|session| {
			let history = veritas_session::categories::category_history(
				session.data()?,
				&veritas_domain::CategoryId::new(category_id),
			)?;
			Ok(CategoryHistory {
				transactions: history.transactions as u64,
				budgeted_months: history.budgeted_months as u64,
				tax_bucket: history.tax_bucket,
			})
		})
	}

	pub(crate) fn accounts(&self) -> Result<AccountList, FfiError> {
		self.call(|session| crate::accounts::account_list(session.data()?))
	}

	pub(crate) fn register(
		&self,
		scope: &RegisterScope,
		filter: &RegisterFilter,
		sort: RegisterSort,
		offset: u64,
		limit: u64,
	) -> Result<RegisterPage, FfiError> {
		self.call(|session| {
			crate::accounts::register_page(session, scope, filter, sort, offset, limit)
		})
	}

	pub(crate) fn register_row(&self, id: &str) -> Result<RegisterRow, FfiError> {
		self.call(|session| crate::accounts::register_row(session.data()?, id))
	}

	pub(crate) fn payees(&self) -> Result<Vec<PayeeDetail>, FfiError> {
		self.call(|session| Ok(crate::accounts::payee_list(session.data()?)))
	}

	pub(crate) fn payee_memory(&self, payee: &str) -> Result<Option<PayeeMemory>, FfiError> {
		self.call(|session| Ok(crate::accounts::payee_memory(session.data()?, payee)))
	}

	pub(crate) fn reconciliation(
		&self,
		account: &str,
		statement: i64,
	) -> Result<Reconciliation, FfiError> {
		self.call(|session| crate::accounts::reconciliation(session.data()?, account, statement))
	}

	pub(crate) fn goals(&self, today: &str) -> Result<Vec<GoalSummary>, FfiError> {
		self.call(|session| {
			session.with_sheet(crate::budget::civil_date(today)?, crate::goals::summaries)
		})
	}

	pub(crate) fn goal_close_preview(
		&self,
		goal_id: &str,
		release_to_category_id: &Option<String>,
		today: &str,
	) -> Result<GoalClosePreview, FfiError> {
		self.call(|session| {
			session.with_sheet(crate::budget::civil_date(today)?, |data, sheet| {
				crate::goals::close_preview(data, sheet, goal_id, release_to_category_id)
			})
		})
	}

	pub(crate) fn budget_month(&self, month: Month, today: &str) -> Result<BudgetMonth, FfiError> {
		self.call(|session| {
			let today = crate::budget::civil_date(today)?;
			session.with_sheet(today, |data, sheet| {
				crate::budget::budget_month(data, sheet, month)
			})
		})
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

	pub(crate) fn maintain(&self, today: &str) -> Result<Change, FfiError> {
		self.call(|session| {
			let today = crate::budget::civil_date(today)?;
			let flipped = session.poll();
			session.snapshot_if_due()?;
			let posted = session.run_schedules(today)?;
			Ok(match flipped {
				Some(flipped) => flipped,
				None => posted,
			})
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
	pub(crate) core: Arc<Core<veritas_store::Store>>,
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
	/// Whether a category can simply be deleted, or has history to archive or
	/// move first.
	pub fn category_history(&self, category_id: String) -> Result<CategoryHistory, FfiError> {
		self.core.category_history(&category_id)
	}

	/// Every live account with its balances, note and last reconcile, and the
	/// account groups (R-ACC-1).
	pub fn accounts(&self) -> Result<AccountList, FfiError> {
		self.core.accounts()
	}

	/// One page of the register (R-ACC-2). `offset` rows in, at most `limit`.
	/// The running balance is set for one account in Actual's order with no
	/// filter.
	pub fn register(
		&self,
		scope: RegisterScope,
		filter: RegisterFilter,
		sort: RegisterSort,
		offset: u64,
		limit: u64,
	) -> Result<RegisterPage, FfiError> {
		self.core.register(&scope, &filter, sort, offset, limit)
	}

	/// One row as the register shows it, to refresh it after a `Change`.
	pub fn register_row(&self, transaction_id: String) -> Result<RegisterRow, FfiError> {
		self.core.register_row(&transaction_id)
	}

	/// Every live payee by name, transfer payees under their account's name.
	pub fn payees(&self) -> Result<Vec<PayeeDetail>, FfiError> {
		self.core.payees()
	}

	/// D38: the category and amount of the payee's most recent row, for quick
	/// entry. `nil` for a payee with no rows.
	pub fn payee_memory(&self, payee_id: String) -> Result<Option<PayeeMemory>, FfiError> {
		self.core.payee_memory(&payee_id)
	}

	/// The reconcile sheet's numbers for a statement balance (R-ACC-3).
	pub fn reconciliation(
		&self,
		account_id: String,
		statement_balance: i64,
	) -> Result<Reconciliation, FfiError> {
		self.core.reconciliation(&account_id, statement_balance)
	}

	/// Every goal, in budget order, as of `today` (`YYYY-MM-DD`).
	pub fn goals(&self, today: String) -> Result<Vec<GoalSummary>, FfiError> {
		self.core.goals(&today)
	}

	/// What `Command::CloseGoal` would move, before the user confirms. `None`
	/// releases to To Budget.
	pub fn goal_close_preview(
		&self,
		goal_id: String,
		release_to_category_id: Option<String>,
		today: String,
	) -> Result<GoalClosePreview, FfiError> {
		self.core
			.goal_close_preview(&goal_id, &release_to_category_id, &today)
	}

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

	/// The app calls this right after opening a budget and then on a timer,
	/// every few minutes, with the local date (`YYYY-MM-DD`). It collects the
	/// background integrity check, takes the hourly snapshot when one is due,
	/// and once a day runs Actual's schedule service: due and missed
	/// auto-post bills are posted and paid ones move on, as one undo step,
	/// gated by `metadata.json`'s `lastScheduleRun` as Actual gates it
	/// (R-BILL-2). The change says what that posted, or that the check
	/// turned the budget read-only.
	pub fn maintain(&self, today: String) -> Result<Change, FfiError> {
		self.core.maintain(&today)
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
		let format = overview.number_format.clone().unwrap();
		let prefs = data(&core).format;
		assert_eq!(
			format.currency,
			prefs.currency_code.filter(|c| !c.is_empty())
		);
		assert_eq!(format.hide_decimals, prefs.hide_fraction);
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

	fn data(core: &Core<veritas_store::Store>) -> veritas_domain::BudgetData {
		core.call(|session| Ok(session.data()?.clone())).unwrap()
	}

	/// An expense group of the small fixture with at least two live categories.
	fn busy_group(data: &veritas_domain::BudgetData) -> (String, Vec<String>) {
		let mut groups: std::collections::BTreeMap<String, Vec<(f64, String)>> = Default::default();
		for category in data
			.categories
			.values()
			.filter(|c| !c.tombstone && !c.is_income)
		{
			groups
				.entry(category.group.to_string())
				.or_default()
				.push((category.sort_order, category.id.to_string()));
		}
		let (group, mut members) = groups.into_iter().find(|(_, m)| m.len() >= 2).unwrap();
		members.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
		(group, members.into_iter().map(|(_, id)| id).collect())
	}

	fn created_category(change: &Change) -> String {
		change
			.entities
			.iter()
			.find(|e| e.kind == crate::EntityKind::Category && e.id.is_some())
			.and_then(|e| e.id.clone())
			.unwrap()
	}

	#[test]
	fn a_new_category_lands_at_the_top_of_its_group_and_undo_takes_it_away() {
		// R-BUD-5, Actual's insertCategory (db/index.ts:470-499).
		let (_scratch, core) = imported("category-create");
		let (group, members) = busy_group(&data(&core));
		let change = core
			.perform(Command::CreateCategory {
				group_id: group.clone(),
				name: "  Coffee  ".to_owned(),
			})
			.unwrap();
		let id = created_category(&change);
		let after = data(&core);
		let created = &after.categories[&veritas_domain::CategoryId::new(id.as_str())];
		assert_eq!(created.name, "Coffee");
		let first = &after.categories[&veritas_domain::CategoryId::new(members[0].as_str())];
		assert!(created.sort_order < first.sort_order);
		assert_eq!(
			after.category_mapping.get(&created.id),
			Some(&created.id),
			"every new category maps to itself"
		);
		let (month, today) = this_month();
		let sheet = core.budget_month(month, &today).unwrap();
		assert!(envelopes(&sheet).iter().any(|e| e.id == id));

		assert!(matches!(
			core.perform(Command::CreateCategory {
				group_id: group,
				name: "COFFEE".to_owned(),
			}),
			Err(FfiError::Invalid { .. })
		));
		core.undo().unwrap();
		assert!(data(&core).categories[&created.id].tombstone);
	}

	#[test]
	fn categories_reorder_move_between_groups_and_hide() {
		// R-BUD-5, Actual's moveCategory (db/index.ts:517-536).
		let (_scratch, core) = imported("category-move");
		let before = data(&core);
		let (group, members) = busy_group(&before);
		let last = members.last().unwrap().clone();

		core.perform(Command::MoveCategory {
			category_id: last.clone(),
			group_id: group.clone(),
			before_category_id: Some(members[0].clone()),
		})
		.unwrap();
		let (_, order) = busy_group(&data(&core));
		assert_eq!(order.first(), Some(&last));

		let change = core
			.perform(Command::CreateCategoryGroup {
				name: "Somewhere else".to_owned(),
			})
			.unwrap();
		let new_group = change.entities[0].id.clone().unwrap();
		core.perform(Command::MoveCategory {
			category_id: last.clone(),
			group_id: new_group.clone(),
			before_category_id: None,
		})
		.unwrap();
		let moved = &data(&core).categories[&veritas_domain::CategoryId::new(last.as_str())];
		assert_eq!(moved.group.to_string(), new_group);

		let income_group = before
			.category_groups
			.values()
			.find(|g| g.is_income && !g.tombstone)
			.unwrap();
		assert!(matches!(
			core.perform(Command::MoveCategory {
				category_id: last.clone(),
				group_id: income_group.id.to_string(),
				before_category_id: None,
			}),
			Err(FfiError::Invalid { .. })
		));

		core.perform(Command::SetCategoryHidden {
			category_id: last.clone(),
			hidden: true,
		})
		.unwrap();
		assert!(data(&core).categories[&moved.id].hidden);
	}

	#[test]
	fn a_category_with_history_is_archived_or_deleted_with_its_history_moved() {
		// R-BUD-5, Actual's category-delete with transferId (budget/app.ts:378-422,
		// db/index.ts:538-562).
		let (scratch, core) = imported("category-delete");
		let before = data(&core);
		let with_history = before
			.categories
			.values()
			.find(|c| {
				!c.tombstone
					&& !c.is_income && core.category_history(c.id.as_str()).unwrap().transactions > 0
			})
			.unwrap()
			.clone();
		let target = before
			.categories
			.values()
			.find(|c| !c.tombstone && !c.is_income && c.id != with_history.id)
			.unwrap()
			.clone();

		assert!(matches!(
			core.perform(Command::DeleteCategory {
				category_id: with_history.id.to_string()
			}),
			Err(FfiError::Invalid { .. })
		));
		let (month, today) = this_month();
		core.budget_month(month, &today).unwrap();
		core.perform(Command::ArchiveCategory {
			category_id: with_history.id.to_string(),
		})
		.unwrap();
		let archived = data(&core);
		assert!(archived.categories[&with_history.id].hidden);
		assert!(
			archived.category_meta[&with_history.id]
				.archived_on
				.is_some()
		);
		core.undo().unwrap();
		assert!(!data(&core).categories[&with_history.id].hidden);

		let moved_transactions = core
			.category_history(with_history.id.as_str())
			.unwrap()
			.transactions;
		let target_before = core
			.category_history(target.id.as_str())
			.unwrap()
			.transactions;
		core.perform(Command::DeleteCategoryMovingHistory {
			category_id: with_history.id.to_string(),
			to_category_id: target.id.to_string(),
		})
		.unwrap();
		let after = data(&core);
		assert!(after.categories[&with_history.id].tombstone);
		assert_eq!(
			after.category_mapping.get(&with_history.id),
			Some(&target.id)
		);
		assert_eq!(
			core.category_history(target.id.as_str())
				.unwrap()
				.transactions,
			target_before + moved_transactions
		);

		core.close().unwrap();
		let reopened =
			Core::<veritas_store::Store>::open(&scratch.budget(), &CancelToken::default()).unwrap();
		let reloaded = data(&reopened);
		assert!(reloaded.categories[&with_history.id].tombstone);
		assert_eq!(
			reloaded.category_mapping.get(&with_history.id),
			Some(&target.id)
		);
	}

	#[test]
	fn a_category_with_no_history_simply_deletes() {
		let (_scratch, core) = imported("category-delete-empty");
		let (group, _) = busy_group(&data(&core));
		let id = created_category(
			&core
				.perform(Command::CreateCategory {
					group_id: group,
					name: "Nothing yet".to_owned(),
				})
				.unwrap(),
		);
		assert_eq!(
			core.category_history(&id).unwrap(),
			crate::CategoryHistory {
				transactions: 0,
				budgeted_months: 0,
				tax_bucket: None,
			}
		);
		core.perform(Command::DeleteCategory {
			category_id: id.clone(),
		})
		.unwrap();
		assert!(data(&core).categories[&veritas_domain::CategoryId::new(id.as_str())].tombstone);
	}

	#[test]
	fn groups_are_created_renamed_reordered_and_hidden() {
		// R-BUD-5, Actual's group rules (db/index.ts:364-434).
		let (_scratch, core) = imported("group-edits");
		let change = core
			.perform(Command::CreateCategoryGroup {
				name: "Travel".to_owned(),
			})
			.unwrap();
		let travel = change.entities[0].id.clone().unwrap();
		let after = data(&core);
		let created =
			&after.category_groups[&veritas_domain::CategoryGroupId::new(travel.as_str())];
		let highest = after
			.category_groups
			.values()
			.filter(|g| !g.tombstone && g.id != created.id)
			.map(|g| g.sort_order)
			.fold(0.0, f64::max);
		assert_eq!(created.sort_order, highest + 16384.0);
		assert!(matches!(
			core.perform(Command::CreateCategoryGroup {
				name: "travel".to_owned()
			}),
			Err(FfiError::Invalid { .. })
		));

		core.perform(Command::RenameCategoryGroup {
			group_id: travel.clone(),
			name: "Trips".to_owned(),
		})
		.unwrap();
		let first = after
			.category_groups
			.values()
			.filter(|g| !g.tombstone)
			.min_by(|a, b| a.sort_order.total_cmp(&b.sort_order))
			.unwrap()
			.id
			.to_string();
		core.perform(Command::MoveCategoryGroup {
			group_id: travel.clone(),
			before_group_id: Some(first.clone()),
		})
		.unwrap();
		core.perform(Command::SetCategoryGroupHidden {
			group_id: travel.clone(),
			hidden: true,
		})
		.unwrap();
		let done = data(&core);
		let trips = &done.category_groups[&veritas_domain::CategoryGroupId::new(travel.as_str())];
		assert_eq!(trips.name, "Trips");
		assert!(trips.hidden);
		let first_order =
			done.category_groups[&veritas_domain::CategoryGroupId::new(first.as_str())].sort_order;
		assert!(trips.sort_order < first_order);
	}

	const NOVEMBER: Month = Month {
		year: 2026,
		month: 11,
	};
	const DECEMBER: Month = Month {
		year: 2026,
		month: 12,
	};

	fn goal_of(core: &Core<veritas_store::Store>, id: &str) -> crate::GoalSummary {
		let (_, today) = this_month();
		core.goals(&today)
			.unwrap()
			.into_iter()
			.find(|goal| goal.id == id)
			.unwrap()
	}

	fn new_goal(
		core: &Core<veritas_store::Store>,
		name: &str,
		target: i64,
		due: Option<&str>,
	) -> String {
		let (_, today) = this_month();
		core.goals(&today).unwrap();
		let change = core
			.perform(Command::CreateGoal {
				name: name.to_owned(),
				target,
				target_date: due.map(str::to_owned),
				icon: Some("laptopcomputer".to_owned()),
				color: None,
			})
			.unwrap();
		change
			.entities
			.iter()
			.find(|e| e.kind == crate::EntityKind::Goal)
			.and_then(|e| e.id.clone())
			.unwrap()
	}

	fn budget(core: &Core<veritas_store::Store>, month: Month, category: &str, amount: i64) {
		core.perform(Command::SetBudget {
			month,
			category_id: category.to_owned(),
			amount,
		})
		.unwrap();
	}

	fn budgeted(core: &Core<veritas_store::Store>, month: Month, category: &str) -> i64 {
		let data = data(core);
		let month = veritas_domain::YearMonth::new(month.year, month.month).unwrap();
		data.budget_cells
			.get(&(month, veritas_domain::CategoryId::new(category)))
			.map_or(0, |cell| cell.budgeted.minor())
	}

	/// An expense category of the fixture that isn't a goal.
	fn other_envelope(core: &Core<veritas_store::Store>, not: &str) -> String {
		let data = data(core);
		data.categories
			.values()
			.find(|c| !c.tombstone && !c.is_income && !c.hidden && c.id.as_str() != not)
			.unwrap()
			.id
			.to_string()
	}

	#[test]
	fn a_new_goal_is_an_envelope_in_the_goals_group_with_actuals_long_term_goal() {
		// R-GOAL-1, DEBT-8 Definitions, D36, G-1.
		let (_scratch, core) = imported("goal-create");
		let id = new_goal(&core, "New laptop", 150_000, Some("2027-04-15"));
		let goal = goal_of(&core, &id);
		assert_eq!(goal.state, crate::GoalState::Active);
		assert_eq!(goal.name, "New laptop");
		assert_eq!(
			(goal.months_left, goal.suggested_monthly),
			(Some(6), Some(25_000))
		);
		assert_eq!(goal.target_date.as_deref(), Some("2027-04-15"));

		let after = data(&core);
		let category =
			&after.categories[&veritas_domain::CategoryId::new(goal.category_id.as_str())];
		assert_eq!(after.category_groups[&category.group].name, "Goals");
		assert_eq!(
			after.notes[goal.category_id.as_str()].as_deref(),
			Some("#goal 1500.00")
		);
		let november = veritas_domain::YearMonth::new(2026, 11).unwrap();
		let cell = &after.budget_cells[&(november, category.id.clone())];
		assert_eq!(
			(cell.goal, cell.long_goal),
			(Some(veritas_domain::Money::from_minor(150_000)), true)
		);

		let second = new_goal(&core, "Vacation", 10_000, None);
		let groups = data(&core)
			.category_groups
			.values()
			.filter(|g| !g.tombstone && g.name == "Goals")
			.count();
		assert_eq!(groups, 1, "the second goal reuses the Goals group");
		assert_eq!(goal_of(&core, &second).suggested_monthly, None);

		core.undo().unwrap();
		core.undo().unwrap();
		let (_, today) = this_month();
		assert!(core.goals(&today).unwrap().is_empty());
	}

	#[test]
	fn budgeting_the_target_reaches_the_goal_in_the_same_undo_step() {
		// DEBT-8.2 active → reached, R-REL-1.
		let (_scratch, core) = imported("goal-reach");
		let id = new_goal(&core, "New laptop", 100_000, Some("2027-01-31"));
		let category = goal_of(&core, &id).category_id;
		budget(&core, NOVEMBER, &category, 60_000);
		assert_eq!(goal_of(&core, &id).state, crate::GoalState::Active);
		budget(&core, DECEMBER, &category, 40_000);
		assert_eq!(
			goal_of(&core, &id).state,
			crate::GoalState::Active,
			"future budget never reaches a goal (GOAL-9)"
		);
		budget(&core, NOVEMBER, &category, 100_000);
		let reached = goal_of(&core, &id);
		assert_eq!(reached.state, crate::GoalState::Reached);
		assert_eq!(reached.reached_month, Some(NOVEMBER));

		budget(&core, NOVEMBER, &category, 10_000);
		assert_eq!(
			goal_of(&core, &id).state,
			crate::GoalState::ReachedSpent,
			"it stays reached, and now holds less than its target"
		);
		core.undo().unwrap();
		core.undo().unwrap();
		let undone = goal_of(&core, &id);
		assert_eq!(
			(undone.state, undone.reached_month),
			(crate::GoalState::Active, None)
		);
	}

	struct Rows(Vec<veritas_domain::Row>);

	impl veritas_session::Command for Rows {
		fn plan(
			&self,
			_: &veritas_domain::BudgetData,
		) -> Result<Vec<veritas_domain::Row>, CoreError> {
			Ok(self.0.clone())
		}
	}

	#[test]
	fn a_reached_goal_reads_spent_only_once_it_holds_less_than_its_target() {
		// DEBT-8.2, GOAL-10, FC-5, FC-6 with Fred's examples: overfunded to
		// 200000 on a 150000 target, 40000 spent, still holds 160000: Reached.
		// Budget moved away below the target: Reached → Spent. Neither gets a
		// suggestion or an ETA.
		let (_scratch, core) = imported("goal-spent");
		let id = new_goal(&core, "New laptop", 150_000, Some("2027-06-30"));
		let category = goal_of(&core, &id).category_id;
		budget(&core, NOVEMBER, &category, 200_000);
		let reached = goal_of(&core, &id);
		assert_eq!(reached.state, crate::GoalState::Reached);
		assert_eq!(
			(reached.suggested_monthly, reached.months_left),
			(None, None)
		);

		let account = data(&core)
			.accounts
			.values()
			.find(|a| !a.tombstone && !a.off_budget && !a.closed)
			.unwrap()
			.id
			.clone();
		let bought = veritas_domain::Transaction {
			id: veritas_domain::TransactionId::new("laptop-bought"),
			account,
			date: veritas_domain::CivilDate::new(2026, 11, 10).unwrap(),
			amount: veritas_domain::Money::from_minor(-40_000),
			payee: None,
			imported_payee: None,
			category: Some(veritas_domain::CategoryId::new(category.as_str())),
			notes: None,
			cleared: true,
			reconciled: false,
			split: veritas_domain::SplitRole::None,
			transfer: None,
			schedule: None,
			starting_balance: false,
			sort_order: 1.0,
			tombstone: false,
			split_error: None,
		};
		core.call(|session| {
			session.perform(&Rows(vec![veritas_domain::Row::Transaction(bought)]))?;
			Ok(())
		})
		.unwrap();
		let after_spending = goal_of(&core, &id);
		assert_eq!(after_spending.available, 160_000);
		assert_eq!(after_spending.state, crate::GoalState::Reached);
		assert_eq!(after_spending.spent, 40_000);

		core.perform(Command::MoveBudget {
			month: NOVEMBER,
			amount: 50_000,
			from_category_id: Some(category.clone()),
			to_category_id: None,
		})
		.unwrap();
		let spent = goal_of(&core, &id);
		assert_eq!(spent.state, crate::GoalState::ReachedSpent);
		assert_eq!((spent.suggested_monthly, spent.months_left), (None, None));
	}

	#[test]
	fn closing_a_goal_releases_this_months_leftover_and_flags_later_budget() {
		// DEBT-8.3 R-1/R-4/R-5, GOAL-1, GOAL-4, D66, GOAL-10.
		let (_scratch, core) = imported("goal-close");
		let id = new_goal(&core, "New laptop", 100_000, Some("2027-06-30"));
		let category = goal_of(&core, &id).category_id;
		let destination = other_envelope(&core, &category);
		budget(&core, NOVEMBER, &category, 50_000);
		budget(&core, DECEMBER, &category, 30_000);
		let (_, today) = this_month();
		let destination_available = |core: &Core<veritas_store::Store>| {
			envelopes(&core.budget_month(NOVEMBER, &today).unwrap())
				.iter()
				.find(|e| e.id == destination)
				.unwrap()
				.available
		};
		let dest_before = destination_available(&core);
		let dest_budget = budgeted(&core, NOVEMBER, &destination);
		let dest_december = budgeted(&core, DECEMBER, &destination);

		let preview = core
			.goal_close_preview(&id, &Some(destination.clone()), &today)
			.unwrap();
		assert_eq!(preview.leftover, 50_000);
		assert_eq!(
			preview.later_budget,
			vec![crate::MonthAmount {
				month: DECEMBER,
				amount: 30_000
			}]
		);
		assert_eq!(preview.destination_available_after, dest_before + 50_000);

		let transactions = data(&core).transactions;
		core.perform(Command::CloseGoal {
			goal_id: id.clone(),
			release_to_category_id: Some(destination.clone()),
			also_move_later: false,
		})
		.unwrap();
		let closed = goal_of(&core, &id);
		assert_eq!(closed.state, crate::GoalState::Closed);
		assert_eq!(closed.closed_month, Some(NOVEMBER));
		assert_eq!(closed.available, 0);
		assert!(!closed.new_activity);
		assert_eq!(closed.later_budget, preview.later_budget);
		assert_eq!(budgeted(&core, NOVEMBER, &category), 0);
		assert_eq!(
			budgeted(&core, NOVEMBER, &destination),
			dest_budget + 50_000
		);
		assert_eq!(destination_available(&core), dest_before + 50_000);
		let after = data(&core);
		assert!(after.categories[&veritas_domain::CategoryId::new(category.as_str())].hidden);
		assert_eq!(after.transactions, transactions, "GOAL-1");
		assert!(matches!(
			core.perform(Command::CloseGoal {
				goal_id: id.clone(),
				release_to_category_id: None,
				also_move_later: false,
			}),
			Err(FfiError::Invalid { .. })
		));

		core.undo().unwrap();
		assert_eq!(goal_of(&core, &id).state, crate::GoalState::Active);
		assert_eq!(budgeted(&core, NOVEMBER, &category), 50_000);

		core.perform(Command::CloseGoal {
			goal_id: id.clone(),
			release_to_category_id: Some(destination.clone()),
			also_move_later: true,
		})
		.unwrap();
		assert!(goal_of(&core, &id).later_budget.is_empty());
		assert_eq!(budgeted(&core, DECEMBER, &category), 0);
		let december_note = data(&core).notes["budget-2026-12"]
			.clone()
			.unwrap_or_default();
		assert!(
			december_note.contains("New laptop"),
			"GOAL-7: the later move writes its movement note: {december_note:?}"
		);
		assert_eq!(
			budgeted(&core, DECEMBER, &destination),
			dest_december + 30_000
		);
	}

	#[test]
	fn dismissing_new_activity_on_a_closed_goal_waits_for_something_new() {
		// GOAL-6 R-16 (D72), in cents.
		use crate::NewAccountKind::Checking;
		let (_scratch, core) = imported("goal-dismiss");
		let checking = new_account(&core, "Everyday", Checking, 0);
		let id = new_goal(&core, "New laptop", 100_000, None);
		let category = goal_of(&core, &id).category_id;
		let march = Month {
			year: 2027,
			month: 3,
		};
		let on = |today: &str| {
			core.goals(today)
				.unwrap()
				.into_iter()
				.find(|goal| goal.id == id)
				.unwrap()
		};
		on("2027-03-10");
		core.perform(Command::CloseGoal {
			goal_id: id.clone(),
			release_to_category_id: None,
			also_move_later: false,
		})
		.unwrap();
		assert_eq!(on("2027-03-10").closed_month, Some(march));
		let buy = |date: &str, amount: i64| {
			let mut draft = typed(&checking, amount);
			draft.date = date.to_owned();
			draft.category_id = Some(category.clone());
			add(&core, draft);
		};
		let dismiss = || {
			let before = data(&core);
			core.perform(Command::DismissGoalActivity {
				goal_id: id.clone(),
			})
			.unwrap();
			let after = data(&core);
			assert_eq!(after.budget_cells, before.budget_cells, "no money moves");
			assert_eq!(after.transactions, before.transactions, "no money moves");
			let goal = after.goals[&veritas_domain::GoalId::new(id.as_str())].clone();
			(
				goal.activity_seen_through.map(|m| (m.year(), m.month())),
				goal.activity_seen_total.map(|m| m.minor()),
				goal.close_month_dismissed_available.map(|m| m.minor()),
			)
		};

		buy("2027-04-05", -1000);
		assert!(
			on("2027-04-20").new_activity,
			"(B) after the April purchase"
		);
		assert_eq!(dismiss(), (Some((2027, 4)), Some(-1000), None));
		assert!(!on("2027-04-20").new_activity);
		buy("2027-04-21", -200);
		assert!(on("2027-04-21").new_activity, "April's total is now -1200");
		assert_eq!(dismiss(), (Some((2027, 4)), Some(-1200), None));
		assert!(!on("2027-04-21").new_activity);

		buy("2027-05-03", -500);
		assert!(on("2027-05-03").new_activity, "May is later than 2027-04");
		assert_eq!(dismiss(), (Some((2027, 5)), Some(-1700), None));
		assert!(!on("2027-05-03").new_activity);

		budget(&core, march, &category, 300);
		assert!(on("2027-05-03").new_activity, "(A): March ends at +300");
		assert_eq!(dismiss(), (Some((2027, 5)), Some(-1700), Some(300)));
		assert!(!on("2027-05-03").new_activity);
		budget(&core, march, &category, 0);
		assert!(!on("2027-05-03").new_activity, "March back at 0");
		budget(&core, march, &category, -200);
		assert!(on("2027-05-03").new_activity, "-200 is neither 0 nor 300");

		core.perform(Command::ReopenGoal {
			goal_id: id.clone(),
		})
		.unwrap();
		let reopened = &data(&core).goals[&veritas_domain::GoalId::new(id.as_str())];
		assert_eq!(
			(
				reopened.activity_seen_through,
				reopened.activity_seen_total,
				reopened.close_month_dismissed_available
			),
			(None, None, None),
			"reopen clears all three"
		);
	}

	#[test]
	fn an_overspent_goal_released_to_to_budget_ends_at_zero() {
		// GOAL-8, R-5 with L < 0. The goal is overspent by moving its budget away.
		let (_scratch, core) = imported("goal-close-to-budget");
		let id = new_goal(&core, "New laptop", 100_000, None);
		let category = goal_of(&core, &id).category_id;
		budget(&core, NOVEMBER, &category, -2_000);
		let (_, today) = this_month();
		let to_budget = core.budget_month(NOVEMBER, &today).unwrap().to_budget;
		let preview = core.goal_close_preview(&id, &None, &today).unwrap();
		assert_eq!(preview.leftover, -2_000);
		assert_eq!(preview.destination_available_after, to_budget - 2_000);
		core.perform(Command::CloseGoal {
			goal_id: id.clone(),
			release_to_category_id: None,
			also_move_later: false,
		})
		.unwrap();
		assert_eq!(goal_of(&core, &id).available, 0);
		assert_eq!(
			core.budget_month(NOVEMBER, &today).unwrap().to_budget,
			to_budget - 2_000
		);
	}

	#[test]
	fn reopening_follows_the_money_and_retargeting_keeps_the_first_reach() {
		// DEBT-8.2 Reopen and Re-target, R-13, D71.
		let (scratch, core) = imported("goal-reopen");
		let id = new_goal(&core, "New laptop", 50_000, None);
		let category = goal_of(&core, &id).category_id;
		assert!(matches!(
			core.perform(Command::ReopenGoal {
				goal_id: id.clone()
			}),
			Err(FfiError::Invalid { .. })
		));
		budget(&core, NOVEMBER, &category, 50_000);
		assert_eq!(goal_of(&core, &id).state, crate::GoalState::Reached);
		core.perform(Command::CloseGoal {
			goal_id: id.clone(),
			release_to_category_id: None,
			also_move_later: false,
		})
		.unwrap();
		assert!(
			matches!(
				core.perform(Command::RetargetGoal {
					goal_id: id.clone(),
					target: 80_000,
					target_date: None,
				}),
				Err(FfiError::Invalid { .. })
			),
			"a closed goal is reopened first"
		);

		core.perform(Command::ReopenGoal {
			goal_id: id.clone(),
		})
		.unwrap();
		let reopened = goal_of(&core, &id);
		assert_eq!(
			(reopened.state, reopened.available, reopened.reached_month),
			(crate::GoalState::Active, 0, Some(NOVEMBER)),
			"R-13: the money was released, so it comes back active"
		);
		let category_id = veritas_domain::CategoryId::new(category.as_str());
		assert!(!data(&core).categories[&category_id].hidden);

		core.perform(Command::RetargetGoal {
			goal_id: id.clone(),
			target: 80_000,
			target_date: Some("2027-02-28".to_owned()),
		})
		.unwrap();
		let retargeted = goal_of(&core, &id);
		assert_eq!(retargeted.state, crate::GoalState::Active);
		assert_eq!(retargeted.target, 80_000);
		assert_eq!(retargeted.reached_month, Some(NOVEMBER), "kept as history");
		assert_eq!(
			(retargeted.months_left, retargeted.suggested_monthly),
			(Some(4), Some(20_000))
		);
		let after = data(&core);
		assert_eq!(
			after.notes[category.as_str()].as_deref(),
			Some("#goal 800.00")
		);
		assert_eq!(after.goals.len(), 1, "D71 updates the same row");

		core.close().unwrap();
		let reopened =
			Core::<veritas_store::Store>::open(&scratch.budget(), &CancelToken::default()).unwrap();
		assert_eq!(goal_of(&reopened, &id), retargeted);
		budget(&reopened, NOVEMBER, &category, 80_000);
		let again = goal_of(&reopened, &id);
		assert_eq!(
			(again.state, again.reached_month),
			(crate::GoalState::Reached, Some(NOVEMBER))
		);
	}

	#[test]
	fn actual_reads_a_closed_goal_and_its_long_term_goal_as_veritas_wrote_them() {
		// R-IO-2, D36, DEBT-8.3 parity with Actual's own sheet.
		let (scratch, core) = imported("goal-oracle");
		let id = new_goal(&core, "New laptop", 100_000, Some("2027-06-30"));
		let category = goal_of(&core, &id).category_id;
		let destination = other_envelope(&core, &category);
		budget(&core, NOVEMBER, &category, 50_000);
		core.perform(Command::CloseGoal {
			goal_id: id,
			release_to_category_id: Some(destination.clone()),
			also_move_later: false,
		})
		.unwrap();
		let (_, today) = this_month();
		let ours = core.budget_month(NOVEMBER, &today).unwrap();
		let zip = scratch.0.join("Goals.zip");
		core.export_actual_zip(&zip.display().to_string(), &CancelToken::default())
			.unwrap();
		let Some(actual) = actual_reads(&zip, &today) else {
			return;
		};
		let november = &actual["computed"]["months"]["2026-11"]["categories"];
		assert_eq!(november[&category]["balance"].as_i64(), Some(0));
		assert_eq!(november[&category]["budgeted"].as_i64(), Some(0));
		let available = envelopes(&ours)
			.iter()
			.find(|e| e.id == destination)
			.unwrap()
			.available;
		assert_eq!(november[&destination]["balance"].as_i64(), Some(available));
		assert_eq!(
			actual["computed"]["months"]["2026-11"]["toBudget"].as_i64(),
			Some(ours.to_budget)
		);
		let notes = actual["tables"]["notes"]["rows"].as_array().unwrap();
		assert!(
			notes
				.iter()
				.any(|row| row["id"] == category.as_str() && row["note"] == "#goal 1000.00")
		);
		let cells = actual["tables"]["zero_budgets"]["rows"].as_array().unwrap();
		let goal_cell = cells
			.iter()
			.find(|row| row["category"] == category.as_str() && row["month"] == 202611)
			.unwrap();
		assert_eq!(goal_cell["goal"].as_i64(), Some(100_000));
		assert_eq!(goal_cell["long_goal"].as_i64(), Some(1));
	}

	fn month(year: i32, month: u32) -> Month {
		Month { year, month }
	}

	/// A goal with budget in August through October 2026 (the complete months
	/// before November), November's budget, and spending from it if any.
	fn paced_goal(
		target: i64,
		due: Option<&str>,
		history: &[i64],
		november: i64,
		spent_in_october: i64,
	) -> crate::GoalSummary {
		let (_scratch, core) = imported("goal-pace");
		let id = new_goal(&core, "New laptop", target, due);
		let category = goal_of(&core, &id).category_id;
		let months = [month(2026, 8), month(2026, 9), month(2026, 10)];
		for (month, amount) in months[3 - history.len()..].iter().zip(history) {
			budget(&core, *month, &category, *amount);
		}
		if spent_in_october != 0 {
			let account = data(&core)
				.accounts
				.values()
				.find(|a| !a.tombstone && !a.off_budget && !a.closed)
				.unwrap()
				.id
				.clone();
			let spending = veritas_domain::Transaction {
				id: veritas_domain::TransactionId::new("goal-spending"),
				account,
				date: veritas_domain::CivilDate::new(2026, 10, 20).unwrap(),
				amount: veritas_domain::Money::from_minor(-spent_in_october),
				payee: None,
				imported_payee: None,
				category: Some(veritas_domain::CategoryId::new(category.as_str())),
				notes: None,
				cleared: true,
				reconciled: false,
				split: veritas_domain::SplitRole::None,
				transfer: None,
				schedule: None,
				starting_balance: false,
				sort_order: 1.0,
				tombstone: false,
				split_error: None,
			};
			core.call(|session| {
				session.perform(&Rows(vec![veritas_domain::Row::Transaction(spending)]))?;
				Ok(())
			})
			.unwrap();
		}
		budget(&core, NOVEMBER, &category, november);
		goal_of(&core, &id)
	}

	#[test]
	fn the_goal_card_paces_by_fc6_like_the_forecast() {
		// FC-6 E-1 to E-6, moved to November 2026 as the current month.
		use crate::GoalPace::{Behind, NoHistory, NotAdding, OnTrack};
		// E-1: available 112000, contributions 25000, 25000, 20000: remaining
		// 38000, pace 25000, 2 months: January 2027.
		let e1 = paced_goal(
			150_000,
			Some("2027-01-31"),
			&[25_000, 25_000, 20_000],
			42_000,
			0,
		);
		assert_eq!(e1.available, 112_000);
		assert_eq!(
			e1.pace,
			Some(OnTrack {
				eta_month: month(2027, 1)
			})
		);
		// The same goal due in December is behind and needs DEBT-8.1's
		// monthly amount: carry 70000, remaining 80000 over 2 months.
		let late = paced_goal(
			150_000,
			Some("2026-12-31"),
			&[25_000, 25_000, 20_000],
			42_000,
			0,
		);
		assert_eq!(
			late.pace,
			Some(Behind {
				needed_monthly: 40_000
			})
		);
		// E-2: contributions 30000, 20000, 10000: the median 20000 still gives
		// 2 months.
		let e2 = paced_goal(
			150_000,
			Some("2027-01-31"),
			&[30_000, 20_000, 10_000],
			52_000,
			0,
		);
		assert_eq!(
			e2.pace,
			Some(OnTrack {
				eta_month: month(2027, 1)
			})
		);
		// E-3, E-14: contributions 0, 0, 0: "No ETA: nothing is being added".
		let e3 = paced_goal(150_000, Some("2027-04-30"), &[0, 0, 0], 0, 0);
		assert_eq!(
			e3.pace,
			Some(NotAdding {
				needed_monthly: 25_000
			})
		);
		// E-13: a brand-new goal, no complete month of history, needs 25000 a
		// month: "No ETA yet".
		let new = paced_goal(150_000, Some("2027-04-30"), &[], 25_000, 0);
		assert_eq!(
			new.pace,
			Some(NoHistory {
				needed_monthly: 25_000
			})
		);
		// E-4: available 150000 is reached: no pace.
		let e4 = paced_goal(150_000, Some("2027-04-30"), &[50_000, 50_000, 50_000], 0, 0);
		assert_eq!((e4.state, e4.pace), (crate::GoalState::Reached, None));
		// E-5: one month of history at 25000.
		let e5 = paced_goal(150_000, Some("2027-04-30"), &[25_000], 25_000, 0);
		assert_eq!(
			e5.pace,
			Some(OnTrack {
				eta_month: month(2027, 3)
			})
		);
		// E-6: budgeted 25000, available 25000, pace 25000, due April 2027:
		// remaining 125000 over 5 months lands on the target month, and the
		// suggestion agrees at 25000.
		let e6 = paced_goal(150_000, Some("2027-04-15"), &[25_000], 25_000, 25_000);
		assert_eq!(e6.available, 25_000);
		assert_eq!(
			e6.pace,
			Some(OnTrack {
				eta_month: month(2027, 4)
			})
		);
		assert_eq!(e6.suggested_monthly, Some(25_000));
		// No target date: no pace.
		let undated = paced_goal(150_000, None, &[25_000], 25_000, 0);
		assert_eq!(undated.pace, None);
	}

	fn entity(change: &Change, kind: crate::EntityKind) -> String {
		change
			.entities
			.iter()
			.find(|e| e.kind == kind && e.id.is_some())
			.and_then(|e| e.id.clone())
			.unwrap()
	}

	fn new_account(
		core: &Core<veritas_store::Store>,
		name: &str,
		kind: crate::NewAccountKind,
		starting_balance: i64,
	) -> String {
		let (_, today) = this_month();
		core.goals(&today).unwrap();
		let change = core
			.perform(Command::CreateAccount {
				account: crate::NewAccount {
					name: name.to_owned(),
					kind,
					starting_balance,
				},
			})
			.unwrap();
		entity(&change, crate::EntityKind::Account)
	}

	fn account_of(core: &Core<veritas_store::Store>, id: &str) -> crate::AccountDetail {
		core.accounts()
			.unwrap()
			.accounts
			.into_iter()
			.find(|a| a.id == id)
			.unwrap()
	}

	fn transfer_payee_of(core: &Core<veritas_store::Store>, account: &str) -> String {
		core.payees()
			.unwrap()
			.into_iter()
			.find(|p| p.transfer_account_id.as_deref() == Some(account))
			.unwrap()
			.id
	}

	fn typed(account: &str, amount: i64) -> crate::TransactionDraft {
		static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
		let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
		crate::TransactionDraft {
			id: Some(format!("typed-{n}")),
			account_id: account.to_owned(),
			date: "2026-11-12".to_owned(),
			amount,
			payee_id: None,
			payee_name: None,
			category_id: None,
			notes: None,
			cleared: false,
			splits: Vec::new(),
		}
	}

	fn add(core: &Core<veritas_store::Store>, draft: crate::TransactionDraft) -> String {
		let id = draft.id.clone().unwrap();
		core.perform(Command::AddTransaction { transaction: draft })
			.unwrap();
		id
	}

	fn row(core: &Core<veritas_store::Store>, id: &str) -> veritas_domain::Transaction {
		data(core).transactions[&veritas_domain::TransactionId::new(id)].clone()
	}

	fn an_expense(core: &Core<veritas_store::Store>) -> String {
		let (_, members) = busy_group(&data(core));
		members[0].clone()
	}

	#[test]
	fn accounts_are_created_grouped_noted_moved_closed_and_reopened() {
		// R-ACC-1, docs/actual-format.md §4.6, §4.8, §4.9.
		use crate::NewAccountKind::{Checking, CreditCard};
		let (_scratch, core) = imported("accounts");
		let checking = new_account(&core, "Everyday", Checking, 100_000);
		let card = new_account(&core, "Travel card", CreditCard, 50_000);
		assert!(matches!(
			core.perform(Command::CreateAccount {
				account: crate::NewAccount {
					name: " Everyday ".to_owned(),
					kind: Checking,
					starting_balance: 0,
				},
			}),
			Err(FfiError::Invalid { .. })
		));
		let opened = account_of(&core, &card);
		assert_eq!(
			(opened.total, opened.cleared, opened.uncleared),
			(-50_000, -50_000, 0)
		);
		assert!(!opened.off_budget);

		let group = entity(
			&core
				.perform(Command::CreateAccountGroup {
					name: "Cards".to_owned(),
				})
				.unwrap(),
			crate::EntityKind::AccountGroup,
		);
		core.perform(Command::SetAccountGroup {
			account_id: card.clone(),
			group_id: Some(group.clone()),
		})
		.unwrap();
		core.perform(Command::SetAccountNote {
			account_id: card.clone(),
			text: "Paid off monthly".to_owned(),
		})
		.unwrap();
		let first = core.accounts().unwrap().accounts[0].id.clone();
		core.perform(Command::MoveAccount {
			account_id: card.clone(),
			before_account_id: Some(first),
		})
		.unwrap();
		let list = core.accounts().unwrap();
		assert_eq!(list.accounts[0].id, card);
		assert_eq!(list.accounts[0].group_id.as_deref(), Some(group.as_str()));
		assert_eq!(list.accounts[0].note.as_deref(), Some("Paid off monthly"));
		assert_eq!(
			list.groups
				.iter()
				.map(|g| g.name.as_str())
				.collect::<Vec<_>>(),
			["Cards"]
		);

		assert!(matches!(
			core.perform(Command::CloseAccount {
				account_id: card.clone(),
				transfer_to_account_id: None,
				category_id: None,
			}),
			Err(FfiError::Invalid { .. })
		));
		core.perform(Command::CloseAccount {
			account_id: card.clone(),
			transfer_to_account_id: Some(checking.clone()),
			category_id: None,
		})
		.unwrap();
		let closed = account_of(&core, &card);
		assert!(closed.closed);
		assert_eq!(closed.total, 0);
		assert_eq!(account_of(&core, &checking).total, 50_000);
		let closing = data(&core)
			.transactions
			.values()
			.find(|t| t.notes.as_deref() == Some("Closing account") && t.account.as_str() == card)
			.unwrap()
			.clone();
		assert_eq!(closing.amount.minor(), 50_000);
		assert_eq!(
			closing.category, None,
			"on budget to on budget: no category"
		);
		let other = row(&core, closing.transfer.as_ref().unwrap().as_str());
		assert_eq!(
			(
				other.account.as_str(),
				other.amount.minor(),
				other.transfer.as_ref()
			),
			(checking.as_str(), -50_000, Some(&closing.id))
		);

		core.perform(Command::ReopenAccount {
			account_id: card.clone(),
		})
		.unwrap();
		assert!(!account_of(&core, &card).closed);

		let empty = new_account(&core, "Never used", Checking, 0);
		core.perform(Command::CloseAccount {
			account_id: empty.clone(),
			transfer_to_account_id: None,
			category_id: None,
		})
		.unwrap();
		assert!(
			data(&core).accounts[&veritas_domain::AccountId::new(empty.as_str())].tombstone,
			"an account with no rows is deleted, not closed"
		);
		core.undo().unwrap();
		assert!(!data(&core).accounts[&veritas_domain::AccountId::new(empty.as_str())].tombstone);
	}

	#[test]
	fn a_typed_transfer_gets_its_counterpart_and_edits_drive_it() {
		// R-ACC-2, R-XFER via WS-RULES' transfers(), §4.2.
		use crate::NewAccountKind::{Checking, Savings};
		let (_scratch, core) = imported("transfers");
		let checking = new_account(&core, "Everyday", Checking, 100_000);
		let savings = new_account(&core, "Rainy day", Savings, 0);
		let mut draft = typed(&checking, -10_000);
		draft.payee_id = Some(transfer_payee_of(&core, &savings));
		draft.notes = Some("to savings".to_owned());
		draft.category_id = Some(an_expense(&core));
		let id = add(&core, draft.clone());
		let source = row(&core, &id);
		assert_eq!(
			source.category, None,
			"on budget to on budget clears the category"
		);
		let other = row(&core, source.transfer.as_ref().unwrap().as_str());
		assert_eq!(other.account.as_str(), savings);
		assert_eq!(other.amount.minor(), 10_000);
		assert!(!other.cleared);
		assert_eq!(other.notes.as_deref(), Some("to savings"));
		assert_eq!(
			other.payee.as_ref().map(|p| p.to_string()),
			Some(transfer_payee_of(&core, &checking))
		);
		assert_eq!(
			other.transfer.as_ref().map(|t| t.as_str()),
			Some(id.as_str())
		);
		let shown = core.register_row(&id).unwrap();
		assert_eq!(shown.transfer_account_id.as_deref(), Some(savings.as_str()));
		assert_eq!(shown.payee_name.as_deref(), Some("Rainy day"));

		draft.amount = -12_000;
		draft.notes = Some("more".to_owned());
		core.perform(Command::EditTransaction {
			transaction_id: id.clone(),
			transaction: draft.clone(),
		})
		.unwrap();
		let other = row(&core, &other.id.to_string());
		assert_eq!(
			(other.amount.minor(), other.notes.as_deref()),
			(12_000, Some("more"))
		);

		draft.payee_id = None;
		draft.payee_name = Some("Corner Grocer".to_owned());
		core.perform(Command::EditTransaction {
			transaction_id: id.clone(),
			transaction: draft,
		})
		.unwrap();
		assert!(
			row(&core, &other.id.to_string()).tombstone,
			"no longer a transfer"
		);
		assert_eq!(row(&core, &id).transfer, None);
		core.undo().unwrap();
		assert!(!row(&core, &other.id.to_string()).tombstone);

		core.perform(Command::DeleteTransaction {
			transaction_id: id.clone(),
		})
		.unwrap();
		assert!(row(&core, &id).tombstone && row(&core, &other.id.to_string()).tombstone);
		assert_eq!(account_of(&core, &savings).total, 0);
	}

	#[test]
	fn splits_keep_their_lines_and_refuse_to_unbalance() {
		// R-ACC-2, §4.3, edge case 6.
		let (_scratch, core) = imported("splits");
		let checking = new_account(&core, "Everyday", crate::NewAccountKind::Checking, 100_000);
		let (_, members) = busy_group(&data(&core));
		let line = |amount, category: &str, id: Option<String>| crate::SplitLineDraft {
			id,
			amount,
			payee_id: None,
			payee_name: None,
			category_id: Some(category.to_owned()),
			notes: None,
		};
		let mut draft = typed(&checking, -9_000);
		draft.payee_name = Some("Warehouse".to_owned());
		draft.splits = vec![
			line(-6_000, &members[0], None),
			line(-3_000, &members[1], None),
		];
		let id = add(&core, draft.clone());
		let shown = core.register_row(&id).unwrap();
		assert_eq!(shown.category_id, None);
		assert_eq!(shown.lines.len(), 2);
		assert_eq!(shown.split_difference, None);
		assert!(shown.payee_id.is_some());
		assert!(
			shown
				.lines
				.iter()
				.all(|line| line.payee_id == shown.payee_id),
			"makeChild: lines without a payee take the parent's"
		);
		let first = shown.lines[0].id.clone();

		draft.splits = vec![
			line(-5_000, &members[0], Some(first.clone())),
			line(-3_000, &members[1], Some(shown.lines[1].id.clone())),
			line(-2_000, &members[1], None),
		];
		assert!(matches!(
			core.perform(Command::EditTransaction {
				transaction_id: id.clone(),
				transaction: draft.clone(),
			}),
			Err(FfiError::Invalid { .. })
		));
		draft.amount = -10_000;
		core.perform(Command::EditTransaction {
			transaction_id: id.clone(),
			transaction: draft.clone(),
		})
		.unwrap();
		let edited = core.register_row(&id).unwrap();
		assert_eq!(edited.lines.len(), 3);
		assert_eq!(edited.lines[0].id, first, "a kept line keeps its row");
		assert_eq!(edited.lines[0].amount, -5_000);

		draft.splits.clear();
		draft.category_id = Some(members[0].clone());
		core.perform(Command::EditTransaction {
			transaction_id: id.clone(),
			transaction: draft,
		})
		.unwrap();
		let plain = core.register_row(&id).unwrap();
		assert!(plain.lines.is_empty());
		assert_eq!(plain.category_id.as_deref(), Some(members[0].as_str()));
		assert!(row(&core, &first).tombstone);

		let copy = "the-copy".to_owned();
		core.perform(Command::DuplicateTransaction {
			transaction_id: id.clone(),
			new_id: Some(copy.clone()),
		})
		.unwrap();
		let copied = row(&core, &copy);
		assert_eq!(
			(copied.amount.minor(), copied.cleared, copied.reconciled),
			(-10_000, false, false)
		);
		assert_ne!(copy, id);
	}

	#[test]
	fn quick_entry_remembers_a_payee_and_learns_its_category() {
		// R-ACC-4, D38, §4.5 learn_categories.
		let (_scratch, core) = imported("quick-entry");
		let checking = new_account(&core, "Everyday", crate::NewAccountKind::Checking, 100_000);
		let category = an_expense(&core);
		let rules_before = data(&core).rules.values().filter(|r| !r.tombstone).count();
		for amount in [-450, -500, -475] {
			let mut draft = typed(&checking, amount);
			draft.payee_name = Some("Corner Cafe".to_owned());
			draft.category_id = Some(category.clone());
			add(&core, draft);
		}
		let payee = core
			.payees()
			.unwrap()
			.into_iter()
			.find(|p| p.name == "Corner Cafe")
			.unwrap();
		assert_eq!(
			payee.transactions, 3,
			"the name was reused, not made three times"
		);
		let memory = core.payee_memory(&payee.id).unwrap().unwrap();
		assert_eq!(memory.category_id.as_deref(), Some(category.as_str()));
		assert!([-450, -500, -475].contains(&memory.amount));

		let after = data(&core);
		let learned: Vec<_> = after
			.rules
			.values()
			.filter(|r| !r.tombstone && r.conditions_json.contains(payee.id.as_str()))
			.collect();
		assert_eq!(
			after.rules.values().filter(|r| !r.tombstone).count(),
			rules_before + 1
		);
		assert_eq!(learned.len(), 1);
		assert!(learned[0].actions_json.contains(category.as_str()));

		let mut typed_in = typed(&checking, -520);
		typed_in.payee_id = Some(payee.id.clone());
		let id = add(&core, typed_in);
		assert_eq!(
			row(&core, &id).category.map(|c| c.to_string()),
			Some(category),
			"the learned rule fills the empty category"
		);
	}

	#[test]
	fn reconciling_checks_off_adjusts_and_locks() {
		// R-ACC-3, §4.7, CLS-5.
		let (_scratch, core) = imported("reconcile");
		let checking = new_account(&core, "Everyday", crate::NewAccountKind::Checking, 100_000);
		let lunch = add(&core, typed(&checking, -1_500));
		let coffee = add(&core, typed(&checking, -400));
		core.perform(Command::SetCleared {
			transaction_ids: vec![lunch.clone()],
			cleared: true,
		})
		.unwrap();
		let state = core.reconciliation(&checking, 98_000).unwrap();
		assert_eq!((state.cleared, state.difference), (98_500, -500));

		core.perform(Command::FinishReconcile {
			account_id: checking.clone(),
			statement_balance: 98_000,
		})
		.unwrap();
		assert!(
			!row(&core, &lunch).reconciled,
			"a difference leaves rows unlocked"
		);
		assert!(account_of(&core, &checking).last_reconciled.is_some());

		core.perform(Command::AddReconcileAdjustment {
			account_id: checking.clone(),
			statement_balance: 98_000,
		})
		.unwrap();
		let after = data(&core);
		let adjustment = after
			.transactions
			.values()
			.find(|t| t.notes.as_deref() == Some("Reconciliation balance adjustment"))
			.unwrap();
		assert_eq!(
			(
				adjustment.amount.minor(),
				adjustment.cleared,
				adjustment.payee.as_ref()
			),
			(-500, true, None)
		);
		assert!(after.transaction_meta[&adjustment.id].adjustment);
		assert_eq!(
			core.reconciliation(&checking, 98_000).unwrap().difference,
			0
		);

		core.perform(Command::FinishReconcile {
			account_id: checking.clone(),
			statement_balance: 98_000,
		})
		.unwrap();
		assert!(row(&core, &lunch).reconciled);
		assert!(!row(&core, &coffee).reconciled, "uncleared rows stay open");
		core.perform(Command::SetCleared {
			transaction_ids: vec![lunch.clone()],
			cleared: false,
		})
		.unwrap();
		assert!(
			row(&core, &lunch).cleared,
			"batch clearing skips reconciled rows"
		);
		let mut noted = typed(&checking, -1_500);
		noted.notes = Some("with Sam".to_owned());
		noted.cleared = true;
		core.perform(Command::EditTransaction {
			transaction_id: lunch.clone(),
			transaction: noted.clone(),
		})
		.unwrap();
		let mut unchecked = noted;
		unchecked.cleared = false;
		let refused = core.perform(Command::EditTransaction {
			transaction_id: lunch.clone(),
			transaction: unchecked.clone(),
		});
		assert!(
			matches!(refused, Err(FfiError::Invalid { .. })),
			"Actual never unchecks a locked row, it only offers to unlock it"
		);
		let locked = row(&core, &lunch);
		assert!(locked.reconciled && locked.cleared);
		core.perform(Command::UnlockReconciled {
			transaction_id: lunch.clone(),
		})
		.unwrap();
		let unlocked = row(&core, &lunch);
		assert!(!unlocked.reconciled && unlocked.cleared);
		core.perform(Command::EditTransaction {
			transaction_id: lunch.clone(),
			transaction: unchecked,
		})
		.unwrap();
		assert!(!row(&core, &lunch).cleared, "once unlocked it unchecks");
	}

	#[test]
	fn payees_rename_favorite_and_merge_with_their_mappings() {
		// R-ACC-5, §4.5.
		let (_scratch, core) = imported("payees");
		let checking = new_account(&core, "Everyday", crate::NewAccountKind::Checking, 0);
		let mut ids = Vec::new();
		for name in ["Cafe A", "Cafe B", "Cafe C"] {
			let mut draft = typed(&checking, -300);
			draft.payee_name = Some(name.to_owned());
			let id = add(&core, draft);
			ids.push(row(&core, &id).payee.unwrap().to_string());
		}
		core.perform(Command::RenamePayee {
			payee_id: ids[2].clone(),
			name: "The Cafe".to_owned(),
		})
		.unwrap();
		core.perform(Command::SetPayeeFavorite {
			payee_id: ids[2].clone(),
			favorite: true,
		})
		.unwrap();
		core.perform(Command::MergePayees {
			target_payee_id: ids[1].clone(),
			payee_ids: vec![ids[0].clone()],
		})
		.unwrap();
		core.perform(Command::MergePayees {
			target_payee_id: ids[2].clone(),
			payee_ids: vec![ids[1].clone(), transfer_payee_of(&core, &checking)],
		})
		.unwrap();
		let after = data(&core);
		let mapped = |id: &str| after.payee_mapping[&veritas_domain::PayeeId::new(id)].to_string();
		assert_eq!(
			mapped(&ids[0]),
			ids[2],
			"an earlier merge's mapping follows the chain"
		);
		assert_eq!(mapped(&ids[1]), ids[2]);
		assert!(
			!after.payees
				[&veritas_domain::PayeeId::new(transfer_payee_of(&core, &checking).as_str())]
				.tombstone
		);
		let the_cafe = core
			.payees()
			.unwrap()
			.into_iter()
			.find(|p| p.id == ids[2])
			.unwrap();
		assert_eq!(
			(
				the_cafe.name.as_str(),
				the_cafe.favorite,
				the_cafe.transactions
			),
			("The Cafe", true, 3)
		);
		assert!(matches!(
			core.perform(Command::MergePayees {
				target_payee_id: transfer_payee_of(&core, &checking),
				payee_ids: vec![ids[2].clone()],
			}),
			Err(FfiError::Invalid { .. })
		));
	}

	#[test]
	fn merging_payees_and_clearing_many_rows_snapshot_the_budget_first() {
		// R-REL-3: a snapshot before each bulk edit, and restoring it undoes
		// the edit. Clearing one row is not a bulk edit.
		let (_scratch, core) = imported("bulk-snapshots");
		core.call(|session| {
			session.wait_for_open_check();
			Ok(())
		})
		.unwrap();
		let checking = new_account(&core, "Everyday", crate::NewAccountKind::Checking, 0);
		let mut payees = Vec::new();
		let mut rows = Vec::new();
		for name in ["Cafe A", "Cafe B"] {
			let mut draft = typed(&checking, -300);
			draft.payee_name = Some(name.to_owned());
			let id = add(&core, draft);
			payees.push(row(&core, &id).payee.unwrap().to_string());
			rows.push(id);
		}
		let bulk = |core: &Core<veritas_store::Store>| {
			core.snapshots()
				.unwrap()
				.into_iter()
				.filter(|s| s.reason == crate::SnapshotKind::BulkEdit)
				.collect::<Vec<_>>()
		};

		core.perform(Command::SetCleared {
			transaction_ids: vec![rows[0].clone()],
			cleared: true,
		})
		.unwrap();
		assert!(bulk(&core).is_empty(), "one row is not a bulk edit");
		core.perform(Command::SetCleared {
			transaction_ids: rows.clone(),
			cleared: true,
		})
		.unwrap();
		assert_eq!(bulk(&core).len(), 1);
		core.perform(Command::MergePayees {
			target_payee_id: payees[1].clone(),
			payee_ids: vec![payees[0].clone()],
		})
		.unwrap();
		let snapshots = bulk(&core);
		assert_eq!(snapshots.len(), 2);
		assert!(!core.payees().unwrap().iter().any(|p| p.id == payees[0]));

		core.restore_snapshot(&snapshots[0].id).unwrap();
		assert!(
			core.payees().unwrap().iter().any(|p| p.id == payees[0]),
			"the snapshot was taken before the merge"
		);
		assert!(row(&core, &rows[1]).cleared);
	}

	#[test]
	fn the_register_pages_sorts_filters_and_runs_a_balance() {
		// R-ACC-2.
		let (_scratch, core) = imported("register");
		let checking = new_account(&core, "Everyday", crate::NewAccountKind::Checking, 100_000);
		for (amount, notes) in [
			(-1_000, "rent #home"),
			(-2_000, "food"),
			(-3_000, "fuel #car"),
		] {
			let mut draft = typed(&checking, amount);
			draft.notes = Some(notes.to_owned());
			add(&core, draft);
		}
		let scope = crate::RegisterScope::Account {
			account_id: checking.clone(),
		};
		let default_sort = crate::RegisterSort {
			key: crate::RegisterSortKey::Date,
			ascending: false,
		};
		let all = core
			.register(
				&scope,
				&crate::RegisterFilter::default(),
				default_sort,
				0,
				100,
			)
			.unwrap();
		assert_eq!(all.total, 4);
		assert_eq!(all.rows[0].running_balance, Some(94_000));
		let oldest = all.rows.last().unwrap();
		assert_eq!(oldest.running_balance, Some(oldest.amount));
		let page = core
			.register(
				&scope,
				&crate::RegisterFilter::default(),
				default_sort,
				1,
				2,
			)
			.unwrap();
		assert_eq!(
			page.rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
			all.rows[1..3]
				.iter()
				.map(|r| r.id.clone())
				.collect::<Vec<_>>()
		);
		let tagged = core
			.register(
				&scope,
				&crate::RegisterFilter {
					search: Some("FUEL".to_owned()),
					..Default::default()
				},
				default_sort,
				0,
				100,
			)
			.unwrap();
		assert_eq!(tagged.total, 1);
		assert_eq!(tagged.rows[0].tags, ["car"]);
		assert_eq!(
			tagged.rows[0].running_balance, None,
			"no running balance under a filter"
		);
		let by_amount = core
			.register(
				&scope,
				&crate::RegisterFilter::default(),
				crate::RegisterSort {
					key: crate::RegisterSortKey::Amount,
					ascending: true,
				},
				0,
				100,
			)
			.unwrap();
		assert_eq!(by_amount.rows[0].amount, -3_000);
	}

	#[test]
	fn an_imported_unbalanced_split_survives_an_edit_elsewhere_and_the_round_trip() {
		// Edge case 6: shown, never fixed or rewritten.
		let (scratch, core) = imported("unbalanced");
		let checking = new_account(&core, "Everyday", crate::NewAccountKind::Checking, 0);
		core.close().unwrap();
		let db = std::path::Path::new(&scratch.budget()).join(veritas_session::DB_FILE);
		let error = r#"{"type":"SplitTransactionError","version":1,"difference":-500}"#;
		{
			let sql = rusqlite::Connection::open(&db).unwrap();
			sql.execute_batch(&format!(
				"INSERT INTO transactions (id, acct, date, amount, isParent, isChild, cleared, sort_order, tombstone, error)
				 VALUES ('odd', '{checking}', 20261101, -3000, 1, 0, 0, 3, 0, '{error}');
				 INSERT INTO transactions (id, acct, date, amount, isParent, isChild, parent_id, cleared, sort_order, tombstone)
				 VALUES ('odd-1', '{checking}', 20261101, -2500, 0, 1, 'odd', 0, -1, 0);"
			))
			.unwrap();
		}
		let core =
			Core::<veritas_store::Store>::open(&scratch.budget(), &CancelToken::default()).unwrap();
		let shown = core.register_row("odd").unwrap();
		assert_eq!(shown.split_difference, Some(-500));
		core.perform(Command::SetCleared {
			transaction_ids: vec!["odd".to_owned()],
			cleared: true,
		})
		.unwrap();
		let other = add(&core, typed(&checking, -100));
		core.perform(Command::DeleteTransaction {
			transaction_id: other,
		})
		.unwrap();
		let stored = |path: &std::path::Path| -> Option<String> {
			rusqlite::Connection::open(path)
				.unwrap()
				.query_row("SELECT error FROM transactions WHERE id = 'odd'", [], |r| {
					r.get(0)
				})
				.unwrap()
		};
		assert_eq!(stored(&db).as_deref(), Some(error));
		assert!(core.register_row("odd").unwrap().cleared);

		let zip = scratch.0.join("odd.zip");
		core.export_actual_zip(&zip.display().to_string(), &CancelToken::default())
			.unwrap();
		let (_, today) = this_month();
		let Some(actual) = actual_reads(&zip, &today) else {
			return;
		};
		let rows = actual["tables"]["transactions"]["rows"].as_array().unwrap();
		let odd = rows.iter().find(|r| r["id"] == "odd").unwrap();
		assert_eq!(odd["error"].as_str(), Some(error));
		assert_eq!(odd["cleared"].as_i64(), Some(1));
	}

	#[test]
	fn actual_reads_the_register_veritas_wrote_with_the_same_balances() {
		// R-ACC-1..3 parity: accounts, a transfer, a split, a close with its
		// closing transfer and a reconcile adjustment, read back by Actual.
		use crate::NewAccountKind::{Checking, CreditCard, Savings};
		let (scratch, core) = imported("register-oracle");
		let checking = new_account(&core, "Everyday", Checking, 100_000);
		let savings = new_account(&core, "Rainy day", Savings, 0);
		let card = new_account(&core, "Old card", CreditCard, 20_000);
		let mut transfer = typed(&checking, -15_000);
		transfer.payee_id = Some(transfer_payee_of(&core, &savings));
		add(&core, transfer);
		let (_, members) = busy_group(&data(&core));
		let mut split = typed(&checking, -9_000);
		split.payee_name = Some("Warehouse".to_owned());
		split.splits = [(-6_000, 0), (-3_000, 1)]
			.into_iter()
			.map(|(amount, n)| crate::SplitLineDraft {
				id: None,
				amount,
				payee_id: None,
				payee_name: None,
				category_id: Some(members[n].clone()),
				notes: None,
			})
			.collect();
		add(&core, split);
		core.perform(Command::CloseAccount {
			account_id: card.clone(),
			transfer_to_account_id: Some(checking.clone()),
			category_id: None,
		})
		.unwrap();
		core.perform(Command::AddReconcileAdjustment {
			account_id: savings.clone(),
			statement_balance: 15_250,
		})
		.unwrap();

		let (_, today) = this_month();
		let ours = core.accounts().unwrap();
		let budget = core.budget_month(NOVEMBER, &today).unwrap();
		let zip = scratch.0.join("register.zip");
		core.export_actual_zip(&zip.display().to_string(), &CancelToken::default())
			.unwrap();
		let Some(actual) = actual_reads(&zip, &today) else {
			return;
		};
		for account in &ours.accounts {
			assert_eq!(
				actual["computed"]["accounts"][&account.id]["balance"].as_i64(),
				Some(account.total),
				"{}",
				account.name
			);
		}
		assert_eq!(
			actual["computed"]["months"]["2026-11"]["toBudget"].as_i64(),
			Some(budget.to_budget)
		);
	}

	#[test]
	fn a_closed_goal_whose_category_changed_kind_reopens_only_as_an_expense() {
		// CLS-3.3: reopening never flips the kind.
		let (_scratch, core) = imported("goal-kind");
		let id = new_goal(&core, "New laptop", 50_000, None);
		let category = veritas_domain::CategoryId::new(goal_of(&core, &id).category_id.as_str());
		core.perform(Command::CloseGoal {
			goal_id: id.clone(),
			release_to_category_id: None,
			also_move_later: false,
		})
		.unwrap();
		let set_kind = |kind| {
			core.call(|session| {
				let write = veritas_engine::category_kinds::set_category_kind(
					session.data()?,
					&category,
					kind,
				)?;
				session.perform(&Rows(vec![veritas_domain::Row::CategoryMeta(
					veritas_domain::CategoryMetaRow {
						category: write.category,
						meta: write.meta,
					},
				)]))?;
				Ok(())
			})
			.unwrap();
		};
		set_kind(veritas_domain::CategoryKind::Savings);
		match core.perform(Command::ReopenGoal {
			goal_id: id.clone(),
		}) {
			Err(FfiError::Invalid { message }) => assert_eq!(
				message,
				"This category is now a savings category. Change it back to an expense category to reopen this goal."
			),
			other => panic!("{other:?}"),
		}
		assert_eq!(
			data(&core).category_kind(&category),
			veritas_domain::CategoryKind::Savings
		);
		set_kind(veritas_domain::CategoryKind::Expense);
		core.perform(Command::ReopenGoal {
			goal_id: id.clone(),
		})
		.unwrap();
		assert_eq!(goal_of(&core, &id).state, crate::GoalState::Active);
	}

	/// Runs Actual's own ops on `zip` (tools/actual-oracle `apply`) into `out`.
	/// `false` without the oracle, unless CI requires it.
	fn actual_apply(zip: &Path, ops: serde_json::Value, out: &Path, today: &str) -> bool {
		let oracle = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/actual-oracle");
		if !oracle.join("node_modules").exists() {
			assert!(
				std::env::var_os("VERITAS_REQUIRE_ORACLE").is_none(),
				"VERITAS_REQUIRE_ORACLE is set but tools/actual-oracle/node_modules is missing"
			);
			return false;
		}
		let file = out.with_extension("ops.json");
		std::fs::write(
			&file,
			serde_json::json!({"version": 1, "ops": ops}).to_string(),
		)
		.unwrap();
		let output = std::process::Command::new("node")
			.current_dir(&oracle)
			.arg("src/main.ts")
			.arg("apply")
			.arg(zip)
			.arg(&file)
			.arg("--out")
			.arg(out)
			.args(["--today", today])
			.output()
			.expect("node runs");
		assert!(
			output.status.success(),
			"Actual couldn't apply the ops: {}",
			String::from_utf8_lossy(&output.stderr)
		);
		true
	}

	fn table<'a>(dump: &'a serde_json::Value, name: &str) -> &'a Vec<serde_json::Value> {
		dump["tables"][name]["rows"].as_array().unwrap()
	}

	/// A Veritas budget with two open accounts, exported as the base both
	/// sides start from.
	fn two_account_base(label: &str) -> (Scratch, std::path::PathBuf, String, String, String) {
		use crate::NewAccountKind::{Checking, Savings};
		let (scratch, core) = imported(label);
		let checking = new_account(&core, "Everyday", Checking, 0);
		let savings = new_account(&core, "Rainy day", Savings, 0);
		let category = an_expense(&core);
		let base = scratch.0.join("base.zip");
		core.export_actual_zip(&base.display().to_string(), &CancelToken::default())
			.unwrap();
		core.close().unwrap();
		(scratch, base, checking, savings, category)
	}

	fn imported_from(scratch: &Scratch, zip: &Path, name: &str) -> Core<veritas_store::Store> {
		Core::<veritas_store::Store>::import_actual_zip(
			&zip.display().to_string(),
			&scratch.0.join(name).display().to_string(),
			&CancelToken::default(),
		)
		.unwrap()
	}

	#[test]
	fn a_learned_category_rule_is_stored_as_actual_stores_it() {
		// §4.5 learn_categories, against Actual's own addTransactions with
		// learnCategories on the same base.
		let (_, today) = this_month();
		let (scratch, base, checking, _, category) = two_account_base("learn-oracle");
		let core = imported_from(&scratch, &base, "veritas");
		core.call(|session| {
			let id = veritas_domain::PayeeId::new("cafe");
			session.perform(&Rows(vec![
				veritas_domain::Row::Payee(veritas_domain::Payee {
					id: id.clone(),
					name: "Corner Cafe".to_owned(),
					transfer_account: None,
					favorite: false,
					learn_categories: true,
					tombstone: false,
				}),
				veritas_domain::Row::PayeeMapping(veritas_domain::PayeeMapping {
					from: id.clone(),
					to: id,
				}),
			]))?;
			Ok(())
		})
		.unwrap();
		core.goals(&today).unwrap();
		for amount in [-450, -500, -475] {
			let mut draft = typed(&checking, amount);
			draft.payee_id = Some("cafe".to_owned());
			draft.category_id = Some(category.clone());
			draft.cleared = true;
			add(&core, draft);
		}
		let ours = data(&core);
		let ours: Vec<_> = ours
			.rules
			.values()
			.filter(|r| r.conditions_json.contains("cafe"))
			.collect();
		assert_eq!(ours.len(), 1);

		let out = scratch.0.join("actual.zip");
		let transactions: Vec<serde_json::Value> = [-450, -500, -475]
			.iter()
			.map(|amount| {
				serde_json::json!({"date": "2026-11-12", "amount": amount, "payee": "cafe", "category": category})
			})
			.collect();
		let ops = serde_json::json!([
			{"op": "createPayee", "id": "cafe", "name": "Corner Cafe"},
			{"op": "addTransactions", "account": checking, "learnCategories": true, "transactions": transactions},
		]);
		if !actual_apply(&base, ops, &out, &today) {
			return;
		}
		let actual = actual_reads(&out, &today).unwrap();
		let theirs: Vec<&serde_json::Value> = table(&actual, "rules")
			.iter()
			.filter(|r| r["conditions"].as_str().is_some_and(|c| c.contains("cafe")))
			.collect();
		assert_eq!(theirs.len(), 1, "Actual learned one rule too");
		let parse = |text: &str| serde_json::from_str::<serde_json::Value>(text).unwrap();
		assert_eq!(
			parse(&ours[0].conditions_json),
			parse(theirs[0]["conditions"].as_str().unwrap())
		);
		assert_eq!(
			parse(&ours[0].actions_json),
			parse(theirs[0]["actions"].as_str().unwrap())
		);
		assert_eq!(theirs[0]["stage"], serde_json::Value::Null);
		assert_eq!(theirs[0]["conditions_op"], "and");
	}

	#[test]
	fn a_transfer_pair_is_written_as_actual_writes_it_on_add_and_edit() {
		// §4.2 addTransfer and updateTransfer, against Actual's own
		// addTransactions and updateTransaction on the same base.
		let (_, today) = this_month();
		let (scratch, base, checking, savings, _) = two_account_base("transfer-oracle");
		let core = imported_from(&scratch, &base, "veritas");
		core.goals(&today).unwrap();
		let mut draft = typed(&checking, -10_000);
		draft.payee_id = Some(transfer_payee_of(&core, &savings));
		draft.notes = Some("to savings".to_owned());
		draft.cleared = true;
		let id = add(&core, draft.clone());
		let pair = |core: &Core<veritas_store::Store>| {
			let source = row(core, &id);
			let other = row(core, source.transfer.as_ref().unwrap().as_str());
			let view = |t: &veritas_domain::Transaction| {
				serde_json::json!({
					"acct": t.account.as_str(),
					"amount": t.amount.minor(),
					"description": t.payee.as_ref().map(|p| p.as_str()),
					"notes": t.notes,
					"cleared": i64::from(t.cleared),
					"reconciled": i64::from(t.reconciled),
					"category": t.category.as_ref().map(|c| c.as_str()),
				})
			};
			(view(&source), view(&other))
		};
		let added = pair(&core);
		draft.amount = -12_000;
		draft.notes = Some("more".to_owned());
		core.perform(Command::EditTransaction {
			transaction_id: id.clone(),
			transaction: draft,
		})
		.unwrap();
		let edited = pair(&core);

		let theirs = |dump: &serde_json::Value| {
			let rows = table(dump, "transactions");
			let source = rows
				.iter()
				.find(|r| {
					r["acct"] == checking.as_str()
						&& r["transferred_id"].is_string()
						&& r["tombstone"] == 0
				})
				.unwrap()
				.clone();
			let other = rows
				.iter()
				.find(|r| r["id"] == source["transferred_id"])
				.unwrap()
				.clone();
			let view = |r: &serde_json::Value| {
				serde_json::json!({
					"acct": r["acct"], "amount": r["amount"], "description": r["description"],
					"notes": r["notes"], "cleared": r["cleared"], "reconciled": r["reconciled"],
					"category": r["category"],
				})
			};
			(
				source["id"].as_str().unwrap().to_owned(),
				view(&source),
				view(&other),
			)
		};
		let first = scratch.0.join("actual-added.zip");
		let ops = serde_json::json!([{"op": "addTransactions", "account": checking, "transactions": [
			{"date": "2026-11-12", "amount": -10_000, "payee": {"transferTo": savings}, "notes": "to savings"}
		]}]);
		if !actual_apply(&base, ops, &first, &today) {
			return;
		}
		let (source_id, source, other) = theirs(&actual_reads(&first, &today).unwrap());
		assert_eq!((source, other), added, "after the add");
		let second = scratch.0.join("actual-edited.zip");
		let ops = serde_json::json!([{"op": "updateTransaction", "id": source_id, "fields": {"amount": -12_000, "notes": "more"}}]);
		assert!(actual_apply(&first, ops, &second, &today));
		let (_, source, other) = theirs(&actual_reads(&second, &today).unwrap());
		assert_eq!((source, other), edited, "after the edit");
	}

	#[test]
	fn a_schedule_linked_on_the_savings_leg_is_written_back_as_actual_does() {
		// transfer.ts:81-85: a rule on the savings side links the leg, and
		// Actual puts the same schedule on the checking row.
		let (_, today) = this_month();
		let (scratch, base, checking, savings, _) = two_account_base("link-back-oracle");
		let linked = scratch.0.join("linked.zip");
		let ops = serde_json::json!([
			{"op": "createSchedule", "id": "savings-plan", "name": "Savings plan", "amount": 999_999, "date": "2030-01-01"},
			{"op": "createRule", "id": "link-savings", "conditions": [{"field": "account", "op": "is", "value": savings}],
			 "actions": [{"op": "link-schedule", "value": "savings-plan"}]},
		]);
		if !actual_apply(&base, ops, &linked, &today) {
			return;
		}
		let core = imported_from(&scratch, &linked, "veritas");
		let mut draft = typed(&checking, -10_000);
		draft.payee_id = Some(transfer_payee_of(&core, &savings));
		let id = add(&core, draft);
		let source = row(&core, &id);
		let leg = row(&core, source.transfer.as_ref().unwrap().as_str());
		let ours = (
			source.schedule.as_ref().map(|s| s.as_str().to_owned()),
			leg.schedule.as_ref().map(|s| s.as_str().to_owned()),
		);
		assert_eq!(
			ours,
			(
				Some("savings-plan".to_owned()),
				Some("savings-plan".to_owned())
			)
		);

		let out = scratch.0.join("actual.zip");
		let ops = serde_json::json!([{"op": "addTransactions", "account": checking, "transactions": [
			{"date": "2026-11-12", "amount": -10_000, "payee": {"transferTo": savings}}
		]}]);
		assert!(actual_apply(&linked, ops, &out, &today));
		let dump = actual_reads(&out, &today).unwrap();
		let rows = table(&dump, "transactions");
		let theirs = rows
			.iter()
			.find(|r| r["acct"] == checking.as_str() && r["transferred_id"].is_string())
			.unwrap();
		let other = rows
			.iter()
			.find(|r| r["id"] == theirs["transferred_id"])
			.unwrap();
		let schedule = |r: &serde_json::Value| r["schedule"].as_str().map(str::to_owned);
		assert_eq!((schedule(theirs), schedule(other)), ours);
	}

	mod bills {
		// Bills through the FFI (R-BILL-1 to R-BILL-5), checked against Actual's
		// own schedule handlers on the same base where Actual has one.

		use serde_json::{Value, json};

		use super::*;
		use crate::{
			BillAmount, BillDate, BillDraft, BillFrequency, BillRepeat, BillRepeatEnd, BillState,
			BillStatus,
		};

		const TODAY: &str = "2026-11-15";
		const LANDLORD: &str = "11111111-1111-4111-8111-111111111111";

		fn last_run(metadata: &Path) -> Option<String> {
			let text = std::fs::read_to_string(metadata).unwrap();
			let value: Value = serde_json::from_str(&text).unwrap();
			value["lastScheduleRun"].as_str().map(str::to_owned)
		}

		fn set_last_run(metadata: &Path, day: Option<&str>) {
			let mut value: Value =
				serde_json::from_str(&std::fs::read_to_string(metadata).unwrap()).unwrap();
			match day {
				Some(day) => value["lastScheduleRun"] = json!(day),
				None => {
					value.as_object_mut().unwrap().remove("lastScheduleRun");
				}
			}
			std::fs::write(metadata, value.to_string()).unwrap();
		}

		fn monthly(start: &str) -> BillDate {
			BillDate::Repeat {
				repeat: BillRepeat {
					start: start.to_owned(),
					interval: 1,
					frequency: BillFrequency::Monthly,
					patterns: Vec::new(),
					weekend: None,
					end: BillRepeatEnd::Never,
				},
			}
		}

		fn rent(account: &str, amount: BillAmount, date: BillDate, auto_post: bool) -> BillDraft {
			BillDraft {
				name: Some("Rent".to_owned()),
				payee_id: Some(LANDLORD.to_owned()),
				account_id: Some(account.to_owned()),
				amount,
				date,
				category_id: None,
				auto_post,
			}
		}

		fn add_landlord(core: &Core<veritas_store::Store>) {
			core.call(|session| {
				let id = veritas_domain::PayeeId::new(LANDLORD);
				session.perform(&Rows(vec![
					veritas_domain::Row::Payee(veritas_domain::Payee {
						id: id.clone(),
						name: "Maple Court".to_owned(),
						transfer_account: None,
						favorite: false,
						learn_categories: true,
						tombstone: false,
					}),
					veritas_domain::Row::PayeeMapping(veritas_domain::PayeeMapping {
						from: id.clone(),
						to: id,
					}),
				]))?;
				Ok(())
			})
			.unwrap();
		}

		/// A budget with one checking account and the landlord, exported as the base
		/// both sides start from.
		fn bill_base(
			label: &str,
			bill: Option<&dyn Fn(&str) -> BillDraft>,
		) -> (Scratch, PathBuf, String) {
			let (scratch, core) = imported(label);
			let checking = new_account(&core, "Everyday", crate::NewAccountKind::Checking, 0);
			add_landlord(&core);
			if let Some(bill) = bill {
				core.perform(Command::CreateBill {
					bill_id: Some("bill-1".to_owned()),
					bill: bill(&checking),
				})
				.unwrap();
			}
			let base = scratch.0.join("base.zip");
			core.export_actual_zip(&base.display().to_string(), &CancelToken::default())
				.unwrap();
			core.close().unwrap();
			(scratch, base, checking)
		}

		fn export(core: &Core<veritas_store::Store>, scratch: &Scratch, name: &str) -> PathBuf {
			let zip = scratch.0.join(name);
			core.export_actual_zip(&zip.display().to_string(), &CancelToken::default())
				.unwrap();
			zip
		}

		fn schedule_rule<'a>(dump: &'a Value, schedule: &str) -> &'a Value {
			let rule = table(dump, "schedules")
				.iter()
				.find(|row| row["id"] == schedule)
				.unwrap()["rule"]
				.clone();
			table(dump, "rules")
				.iter()
				.find(|row| row["id"] == rule)
				.unwrap()
		}

		fn parsed(value: &Value) -> Value {
			serde_json::from_str(value.as_str().unwrap()).unwrap()
		}

		fn without(row: &Value, keys: &[&str]) -> Value {
			let mut row = row.clone();
			for key in keys {
				row.as_object_mut().unwrap().remove(*key);
			}
			row
		}

		fn posted_for(dump: &Value, schedule: &str) -> Vec<Value> {
			table(dump, "transactions")
				.iter()
				.filter(|row| row["schedule"] == schedule && row["tombstone"] == 0)
				.cloned()
				.collect()
		}

		#[test]
		fn a_bill_is_created_edited_paused_resumed_and_skipped_as_actual_does_it() {
			// R-BILL-1, R-BILL-4, D65: against Actual's createSchedule, schedule/update
			// and skip-next-date on the same base.
			let (scratch, base, checking) = bill_base("bill-oracle", None);
			let core = imported_from(&scratch, &base, "veritas");
			core.goals(TODAY).unwrap();
			let created = rent(
				&checking,
				BillAmount::Exact { amount: -150_000 },
				monthly("2026-11-20"),
				false,
			);
			core.perform(Command::CreateBill {
				bill_id: Some("bill-1".to_owned()),
				bill: created,
			})
			.unwrap();
			let first = data(&core);
			let first_rule =
				&first.rules[&first.schedules[&veritas_domain::ScheduleId::new("bill-1")].rule];
			let first_conditions: Value =
				serde_json::from_str(&first_rule.conditions_json).unwrap();
			let edited = rent(
				&checking,
				BillAmount::About { amount: -155_000 },
				monthly("2026-11-25"),
				false,
			);
			core.perform(Command::EditBill {
				bill_id: "bill-1".to_owned(),
				bill: edited,
			})
			.unwrap();
			let second = data(&core);
			let second_rule =
				&second.rules[&second.schedules[&veritas_domain::ScheduleId::new("bill-1")].rule];
			let second_conditions: Value =
				serde_json::from_str(&second_rule.conditions_json).unwrap();
			for command in [
				Command::PauseBill {
					bill_id: "bill-1".to_owned(),
				},
				Command::ResumeBill {
					bill_id: "bill-1".to_owned(),
				},
				Command::SkipBill {
					bill_id: "bill-1".to_owned(),
				},
			] {
				core.perform(command).unwrap();
			}
			let ours_zip = export(&core, &scratch, "ours.zip");

			let date = first_conditions
				.as_array()
				.unwrap()
				.iter()
				.find(|c| c["field"] == "date")
				.unwrap()["value"]
				.clone();
			let ops = json!([
								{"op": "createSchedule", "id": "bill-1", "name": "Rent", "postsTransaction": false,
					"payee": LANDLORD, "account": checking, "amount": -150_000, "amountOp": "is", "date": date},
				{"op": "tick"},
					{"op": "updateSchedule", "id": "bill-1", "fields": {"name": "Rent", "posts_transaction": false},
					"conditions": second_conditions},
				{"op": "tick"},
					{"op": "updateSchedule", "id": "bill-1", "fields": {"completed": true}},
				{"op": "tick"},
					{"op": "updateSchedule", "id": "bill-1", "fields": {"completed": false}, "resetNextDate": true},
				{"op": "tick"},
					{"op": "skipSchedule", "id": "bill-1"},
			]);
			let theirs_zip = scratch.0.join("theirs.zip");
			if !actual_apply(&base, ops, &theirs_zip, TODAY) {
				return;
			}
			let ours = actual_reads(&ours_zip, TODAY).unwrap();
			let theirs = actual_reads(&theirs_zip, TODAY).unwrap();
			assert_eq!(
				ours["computed"]["schedules"]["bill-1"], theirs["computed"]["schedules"]["bill-1"],
				"Actual reads the same next date and upcoming dates"
			);
			let row = |dump: &Value| {
				without(
					table(dump, "schedules")
						.iter()
						.find(|row| row["id"] == "bill-1")
						.unwrap(),
					&["rule"],
				)
			};
			assert_eq!(row(&ours), row(&theirs));
			let (our_rule, their_rule) = (
				schedule_rule(&ours, "bill-1"),
				schedule_rule(&theirs, "bill-1"),
			);
			assert_eq!(
				parsed(&our_rule["conditions"]),
				parsed(&their_rule["conditions"])
			);
			assert_eq!(parsed(&our_rule["actions"]), parsed(&their_rule["actions"]));
			assert_eq!(
				without(our_rule, &["id", "conditions", "actions"]),
				without(their_rule, &["id", "conditions", "actions"])
			);
			let paths = |dump: &Value| {
				table(dump, "schedules_json_paths")
					.iter()
					.find(|row| row["schedule_id"] == "bill-1")
					.cloned()
			};
			assert!(paths(&ours).is_some());
			assert_eq!(paths(&ours), paths(&theirs));
		}

		#[test]
		fn veritas_and_actual_opening_the_same_budget_never_post_a_due_bill_twice() {
			// R-BILL-2, docs/actual-format.md §6.3: the service runs once a day on
			// open on either side, gated by lastScheduleRun.
			let due = |account: &str| {
				rent(
					account,
					BillAmount::Exact { amount: -150_000 },
					monthly(TODAY),
					true,
				)
			};
			let (scratch, base, _) = bill_base("bill-twice", Some(&due));

			// Veritas opens first, then Actual, which also forces a second run.
			let core = imported_from(&scratch, &base, "veritas-first");
			let posted = core.maintain(TODAY).unwrap();
			assert!(!posted.entities.is_empty());
			assert!(
				core.maintain(TODAY).unwrap().entities.is_empty(),
				"once a day"
			);
			assert_eq!(
				last_run(&scratch.0.join("veritas-first/metadata.json")).as_deref(),
				Some(TODAY)
			);
			let ours = export(&core, &scratch, "veritas-first.zip");
			core.close().unwrap();
			let then_actual = scratch.0.join("then-actual.zip");
			let ops = json!([{"op": "openBudgetSchedules"}, {"op": "runSchedules"}]);
			if !actual_apply(&ours, ops, &then_actual, TODAY) {
				return;
			}
			let dump = actual_reads(&then_actual, TODAY).unwrap();
			let rows = posted_for(&dump, "bill-1");
			assert_eq!(rows.len(), 1, "{rows:?}");
			assert_eq!(rows[0]["date"], 20_261_115);
			assert_eq!(rows[0]["amount"], -150_000);

			// Actual opens first, then Veritas.
			let actual_first = scratch.0.join("actual-first.zip");
			assert!(actual_apply(
				&base,
				json!([{"op": "openBudgetSchedules"}]),
				&actual_first,
				TODAY
			));
			assert_eq!(
				posted_for(&actual_reads(&actual_first, TODAY).unwrap(), "bill-1").len(),
				1
			);
			let core = imported_from(&scratch, &actual_first, "actual-first");
			core.maintain(TODAY).unwrap();
			let again = export(&core, &scratch, "actual-then-veritas.zip");
			let dump = actual_reads(&again, TODAY).unwrap();
			assert_eq!(posted_for(&dump, "bill-1").len(), 1);

			// Even with the day cleared, Veritas sees the occurrence paid.
			set_last_run(&scratch.0.join("actual-first/metadata.json"), None);
			core.maintain(TODAY).unwrap();
			let cleared = export(&core, &scratch, "cleared.zip");
			assert_eq!(
				posted_for(&actual_reads(&cleared, TODAY).unwrap(), "bill-1").len(),
				1
			);
		}

		#[test]
		fn the_service_run_is_one_undo_step_and_saving_an_auto_post_bill_lets_it_run_again() {
			let (scratch, core) = imported("bill-service");
			let checking = new_account(&core, "Everyday", crate::NewAccountKind::Checking, 0);
			add_landlord(&core);
			core.goals(TODAY).unwrap();
			let metadata = Path::new(&scratch.budget()).join("metadata.json");
			set_last_run(&metadata, Some("2026-11-14"));
			core.perform(Command::CreateBill {
				bill_id: Some("bill-1".to_owned()),
				bill: rent(
					&checking,
					BillAmount::Exact { amount: -150_000 },
					monthly("2026-11-15"),
					true,
				),
			})
			.unwrap();
			assert_eq!(
				last_run(&metadata),
				None,
				"createSchedule clears it for an auto-post bill"
			);
			let bills = |core: &Core<veritas_store::Store>| {
				core.call(|session| {
					crate::bills::bills(session.data()?, crate::budget::civil_date(TODAY)?)
				})
				.unwrap()
			};
			assert_eq!(bills(&core)[0].status, BillStatus::Due);
			let upcoming = core
				.call(|session| {
					crate::bills::upcoming(session.data()?, 40, crate::budget::civil_date(TODAY)?)
				})
				.unwrap();
			assert_eq!(
				upcoming
					.iter()
					.map(|u| (u.date.as_str(), u.status))
					.collect::<Vec<_>>(),
				[
					("2026-11-15", BillStatus::Due),
					("2026-12-15", BillStatus::Upcoming)
				]
			);

			core.maintain(TODAY).unwrap();
			let after = data(&core);
			assert_eq!(
				after
					.transactions
					.values()
					.filter(|t| t.schedule.as_ref().map(|s| s.as_str()) == Some("bill-1"))
					.count(),
				1
			);
			// As in Actual, the occurrence is posted and the bill moves on at
			// the next day's run, once it counts as paid.
			assert_eq!(bills(&core)[0].next_date.as_deref(), Some(TODAY));
			assert_eq!(bills(&core)[0].status, BillStatus::Paid);
			assert_eq!(last_run(&metadata).as_deref(), Some(TODAY));
			core.undo().unwrap();
			let undone = data(&core);
			assert!(
				undone
					.transactions
					.values()
					.filter(|t| t.schedule.as_ref().map(|s| s.as_str()) == Some("bill-1"))
					.all(|t| t.tombstone)
			);
			assert_eq!(bills(&core)[0].next_date.as_deref(), Some(TODAY));
			assert_eq!(
				last_run(&metadata).as_deref(),
				Some(TODAY),
				"undo doesn't touch metadata.json"
			);

			core.perform(Command::PauseBill {
				bill_id: "bill-1".to_owned(),
			})
			.unwrap();
			assert_eq!(last_run(&metadata), None);
			assert_eq!(bills(&core)[0].state, BillState::Paused);
			core.perform(Command::EndBill {
				bill_id: "bill-1".to_owned(),
			})
			.unwrap();
			assert_eq!(bills(&core)[0].state, BillState::Ended);
			assert_eq!(bills(&core)[0].status, BillStatus::Completed);
			core.perform(Command::ResumeBill {
				bill_id: "bill-1".to_owned(),
			})
			.unwrap();
			assert_eq!(bills(&core)[0].state, BillState::Active);
		}

		#[test]
		fn bills_actual_would_misread_are_refused_and_income_is_a_bill_too() {
			// R-BILL-4, R-BILL-5.
			let (_scratch, core) = imported("bill-checks");
			let checking = new_account(&core, "Everyday", crate::NewAccountKind::Checking, 0);
			add_landlord(&core);
			core.goals(TODAY).unwrap();
			let refused = |bill: BillDraft| match core.perform(Command::CreateBill {
				bill_id: None,
				bill,
			}) {
				Err(FfiError::Invalid { message }) => message,
				other => panic!("{other:?}"),
			};
			let mut ended = rent(
				&checking,
				BillAmount::Exact { amount: -100 },
				monthly("2026-11-20"),
				false,
			);
			if let BillDate::Repeat { repeat } = &mut ended.date {
				repeat.end = BillRepeatEnd::OnDate {
					date: "2026-11-01".to_owned(),
				};
			}
			assert!(refused(ended).contains("never comes due"));
			let range = rent(
				&checking,
				BillAmount::Between {
					low: -100,
					high: -100,
				},
				monthly("2026-11-20"),
				false,
			);
			assert_eq!(refused(range), "A range needs two different amounts");
			let mut weekly = rent(
				&checking,
				BillAmount::Exact { amount: -100 },
				monthly("2026-11-20"),
				false,
			);
			if let BillDate::Repeat { repeat } = &mut weekly.date {
				repeat.frequency = BillFrequency::Weekly;
				repeat.patterns = vec![crate::BillRepeatPattern::Day { day: 3 }];
			}
			assert_eq!(
				refused(weekly),
				"Only monthly bills can repeat on chosen days"
			);

			let mut paycheck = rent(
				&checking,
				BillAmount::Exact { amount: 250_000 },
				BillDate::Once {
					date: "2026-11-30".to_owned(),
				},
				false,
			);
			paycheck.name = Some("Paycheck".to_owned());
			core.perform(Command::CreateBill {
				bill_id: Some("pay".to_owned()),
				bill: paycheck.clone(),
			})
			.unwrap();
			assert!(matches!(
				core.perform(Command::CreateBill {
					bill_id: None,
					bill: paycheck,
				}),
				Err(FfiError::Invalid { .. })
			));
			let detail = core
				.call(|session| {
					crate::bills::bill_detail(
						session.data()?,
						"pay",
						crate::budget::civil_date(TODAY)?,
					)
				})
				.unwrap();
			assert!(detail.bill.is_income);
			assert_eq!(detail.next_dates, ["2026-11-30"]);
			assert!(matches!(
				core.perform(Command::SkipBill {
					bill_id: "pay".to_owned(),
				}),
				Err(FfiError::Invalid { .. })
			));
			core.perform(Command::PayBill {
				bill_id: "pay".to_owned(),
				amount: 251_000,
				date: "2026-11-29".to_owned(),
			})
			.unwrap();
			let paid = data(&core);
			let row = paid
				.transactions
				.values()
				.find(|t| t.schedule.as_ref().map(|s| s.as_str()) == Some("pay"))
				.unwrap();
			assert_eq!(
				(row.amount.minor(), row.date.to_string().as_str()),
				(251_000, "2026-11-29")
			);
			// Inside the 2-day window of a confirm-first bill, so it reads paid,
			// and the next service run completes it.
			let pay = &paid.schedules[&veritas_domain::ScheduleId::new("pay")];
			assert!(!pay.completed);
			assert_eq!(
				crate::bills::bill_detail(&paid, "pay", crate::budget::civil_date(TODAY).unwrap())
					.unwrap()
					.bill
					.status,
				BillStatus::Paid
			);
		}
	}
}
