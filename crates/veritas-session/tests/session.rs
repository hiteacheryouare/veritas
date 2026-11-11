#![allow(
	clippy::unwrap_used,
	clippy::expect_used,
	clippy::panic,
	reason = "integration tests fail by panicking"
)]

mod support;

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use rusqlite::Connection;
use support::TempDir;
use veritas_domain::{
	AccountId, BudgetData, Change, CoreError, ReadOnlyReason, Row, Store, StoreTransaction,
};
use veritas_session::{
	Access, Backend, ChangeListener, ChangeSet, Command, Session, SessionOptions, SharedSession,
	SnapshotReason, acquire,
};
use veritas_store::Loaded;

/// veritas-store's real read path, with writes counted instead of saved
/// until the store's write path lands. Everything that reaches the file in
/// these tests (adopt, snapshots, restores) is real.
struct ReadPath {
	store: veritas_store::Store,
	writes: usize,
}

struct Counted<'a> {
	writes: &'a mut usize,
	pending: usize,
}

impl Store for ReadPath {
	type Transaction<'a> = Counted<'a>;

	fn begin(&mut self) -> Result<Counted<'_>, CoreError> {
		Ok(Counted {
			writes: &mut self.writes,
			pending: 0,
		})
	}
}

thread_local! {
	/// Makes the store panic on the nth write of this thread's next command.
	static PANIC_ON_WRITE: Cell<Option<usize>> = const { Cell::new(None) };
	/// Makes adopt answer as veritas-store does for tables from a newer Veritas.
	static NEWER_VERITAS_TABLES: Cell<bool> = const { Cell::new(false) };
}

impl StoreTransaction for Counted<'_> {
	fn apply(&mut self, _: &Change) -> Result<(), CoreError> {
		if PANIC_ON_WRITE.get() == Some(self.pending) {
			PANIC_ON_WRITE.set(None);
			panic!("index out of bounds: the len is 0 but the index is 7");
		}
		self.pending += 1;
		Ok(())
	}

	fn commit(self) -> Result<(), CoreError> {
		*self.writes += self.pending;
		Ok(())
	}

	fn rollback(self) -> Result<(), CoreError> {
		Ok(())
	}
}

impl Backend for ReadPath {
	fn open(db: &Path, access: veritas_store::Access) -> Result<Self, CoreError> {
		veritas_store::Store::open(db, access).map(|store| ReadPath { store, writes: 0 })
	}

	fn migration_ids(&self) -> Result<Vec<i64>, CoreError> {
		self.store.migration_ids()
	}

	fn quick_problems(&self) -> Result<Vec<String>, CoreError> {
		let verdict = self.store.pragma_text("quick_check")?;
		Ok(if verdict == "ok" {
			Vec::new()
		} else {
			vec![verdict]
		})
	}

	fn verify(db: &Path) -> Result<veritas_store::AuditReport, CoreError> {
		let plan = BACKGROUND_CHECKS.lock().unwrap().remove(db);
		if let Some(plan) = plan {
			if let Some(gate) = plan.gate {
				let _ = gate.recv();
			}
			if plan.fails {
				return Ok(veritas_store::AuditReport::from_integrity(vec![
					"page 7 is never used".to_owned(),
				]));
			}
		}
		<veritas_store::Store as Backend>::verify(db)
	}

	fn adopt(&mut self) -> Result<(), CoreError> {
		if NEWER_VERITAS_TABLES.get() {
			return Err(CoreError::ReadOnly(ReadOnlyReason::NewerVeritasSchema));
		}
		self.store.adopt()
	}

	fn load(&self) -> Result<Loaded, CoreError> {
		self.store.load()
	}
}

/// How the background check of one database should behave: wait for the
/// test to release it, and report damage. The check runs on another thread,
/// so this is keyed by database path rather than thread-local.
struct CheckPlan {
	gate: Option<Receiver<()>>,
	fails: bool,
}

static BACKGROUND_CHECKS: Mutex<BTreeMap<PathBuf, CheckPlan>> = Mutex::new(BTreeMap::new());

/// Holds `dir`'s background check until the returned sender sends.
fn hold_background_check(dir: &Path, fails: bool) -> Sender<()> {
	let (release, gate) = channel();
	BACKGROUND_CHECKS.lock().unwrap().insert(
		dir.join("db.sqlite"),
		CheckPlan {
			gate: Some(gate),
			fails,
		},
	);
	release
}

type TestSession = Session<ReadPath>;

fn fixture(name: &str) -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("../../fixtures/synthetic")
		.join(format!("{name}.zip"))
}

/// A clock the test moves by hand.
#[derive(Clone)]
struct TestClock(Arc<Mutex<DateTime<Utc>>>);

impl TestClock {
	fn new() -> Self {
		let start = DateTime::parse_from_rfc3339("2026-11-10T09:00:00Z")
			.unwrap()
			.to_utc();
		TestClock(Arc::new(Mutex::new(start)))
	}

	fn advance(&self, by: TimeDelta) {
		*self.0.lock().unwrap() += by;
	}

	fn options(&self) -> SessionOptions {
		let now = self.0.clone();
		SessionOptions {
			clock: Arc::new(move || *now.lock().unwrap()),
			..SessionOptions::default()
		}
	}
}

/// An imported copy of `small`, editable by SQL before a session opens it.
fn imported(label: &str) -> (TempDir, PathBuf) {
	let root = TempDir::new(label);
	let dir = root.path().join("budget");
	veritas_interop::import_actual_zip(&fixture("small"), &dir).unwrap();
	(root, dir)
}

fn sql(dir: &Path, statement: &str) {
	Connection::open(dir.join("db.sqlite"))
		.unwrap()
		.execute_batch(statement)
		.unwrap();
}

fn first_account(data: &BudgetData) -> AccountId {
	data.accounts.keys().next().unwrap().clone()
}

fn account_name(session: &TestSession, id: &AccountId) -> String {
	session.data().unwrap().accounts[id].name.clone()
}

fn rename(to: &'static str) -> impl Fn(&BudgetData) -> Result<Vec<Row>, CoreError> {
	move |data: &BudgetData| {
		let mut account = data.accounts[&first_account(data)].clone();
		account.name = to.to_owned();
		Ok(vec![Row::Account(account)])
	}
}

struct BulkRename;

impl Command for BulkRename {
	fn plan(&self, data: &BudgetData) -> Result<Vec<Row>, CoreError> {
		rename("Bulk")(data)
	}

	fn snapshot_first(&self) -> Option<SnapshotReason> {
		Some(SnapshotReason::BulkEdit)
	}
}

#[derive(Default)]
struct Recorder(Mutex<Vec<ChangeSet>>);

impl ChangeListener for Recorder {
	fn changed(&self, changes: &ChangeSet) {
		self.0.lock().unwrap().push(changes.clone());
	}
}

fn reasons(session: &TestSession) -> Vec<SnapshotReason> {
	session
		.snapshots()
		.unwrap()
		.into_iter()
		.rev()
		.map(|snapshot| snapshot.reason)
		.collect()
}

#[test]
fn importing_a_zip_opens_a_writable_budget_and_snapshots_it_first() {
	// R-IO-1, R-REL-3, R-REL-6
	let root = TempDir::new("session-import");
	let dir = root.path().join("budget");
	let mut session =
		TestSession::import_actual_zip(&fixture("small"), &dir, TestClock::new().options())
			.unwrap();
	assert_eq!(session.wait_for_open_check(), None);

	assert_eq!(session.read_only(), None);
	assert!(!session.data().unwrap().accounts.is_empty());
	assert!(session.load_report().skipped.is_empty());
	assert_eq!(reasons(&session), [SnapshotReason::Open]);
	assert!(matches!(acquire(&dir).unwrap(), Access::ReadOnly(_)));

	session.perform(&rename("Everyday")).unwrap();
	assert!(session.can_undo());
}

#[test]
fn a_second_session_on_the_same_budget_is_read_only_and_takes_no_snapshots() {
	// R-REL-6
	let (_root, dir) = imported("session-second");
	let clock = TestClock::new();
	let mut first = TestSession::open(&dir, clock.options()).unwrap();
	first.wait_for_open_check();
	let mut second = TestSession::open(&dir, clock.options()).unwrap();
	second.wait_for_open_check();

	assert_eq!(second.read_only(), Some(ReadOnlyReason::OpenElsewhere));
	let refused = |result: Result<ChangeSet, CoreError>| {
		matches!(
			result,
			Err(CoreError::ReadOnly(ReadOnlyReason::OpenElsewhere))
		)
	};
	assert!(refused(second.perform(&rename("Nope"))));
	assert!(refused(second.restore("anything.sqlite")));
	assert!(matches!(
		second.snapshot(SnapshotReason::Hourly),
		Err(CoreError::ReadOnly(_))
	));
	assert_eq!(reasons(&second), [SnapshotReason::Open]);
}

#[test]
fn opening_waits_briefly_for_a_writer_that_is_closing() {
	// R-REL-6
	let (_root, dir) = imported("session-retry");
	let held = acquire(&dir).unwrap();
	assert!(matches!(held, Access::Writer(_)));
	let closing = std::thread::spawn(move || {
		std::thread::sleep(Duration::from_millis(25));
		drop(held);
	});

	let session = TestSession::open(&dir, TestClock::new().options()).unwrap();

	closing.join().unwrap();
	assert_eq!(session.read_only(), None);
}

#[test]
fn a_file_from_a_newer_actual_opens_read_only_without_adopting_it() {
	// R-IO-7
	let (_root, dir) = imported("session-newer");
	sql(
		&dir,
		"INSERT INTO __migrations__ (id) VALUES (1790000000000);",
	);

	let mut session = TestSession::open(&dir, TestClock::new().options()).unwrap();
	session.wait_for_open_check();

	assert_eq!(
		session.read_only(),
		Some(ReadOnlyReason::NewerActualVersion)
	);
	let adopted: bool = Connection::open(dir.join("db.sqlite"))
		.unwrap()
		.query_row(
			"SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE name = 'veritas_meta')",
			[],
			|row| row.get(0),
		)
		.unwrap();
	assert!(!adopted);
	assert!(session.snapshots().unwrap().is_empty());
}

#[test]
fn a_file_from_an_older_actual_is_refused() {
	// R-IO-7
	let (_root, dir) = imported("session-older");
	sql(
		&dir,
		"DELETE FROM __migrations__ WHERE id = (SELECT MAX(id) FROM __migrations__);",
	);
	let result = TestSession::open(&dir, TestClock::new().options());
	assert!(
		matches!(result, Err(CoreError::UpgradeInActualFirst { .. })),
		"{:?}",
		result.err()
	);
}

#[test]
fn tables_from_a_newer_veritas_open_read_only() {
	let (_root, dir) = imported("session-newer-veritas");
	NEWER_VERITAS_TABLES.set(true);
	let session = TestSession::open(&dir, TestClock::new().options());
	NEWER_VERITAS_TABLES.set(false);
	let mut session = session.unwrap();
	session.wait_for_open_check();
	assert_eq!(
		session.read_only(),
		Some(ReadOnlyReason::NewerVeritasSchema)
	);
	assert!(session.snapshots().unwrap().is_empty());
}

#[test]
fn a_tracking_budget_opens_read_only() {
	// R-BUD-2
	let (_root, dir) = imported("session-tracking");
	sql(
		&dir,
		"INSERT INTO preferences (id, value) VALUES ('budgetType', 'tracking')
		ON CONFLICT (id) DO UPDATE SET value = excluded.value;",
	);
	let session = TestSession::open(&dir, TestClock::new().options()).unwrap();
	assert_eq!(session.read_only(), Some(ReadOnlyReason::TrackingBudget));
}

#[test]
fn a_folder_without_a_budget_is_a_file_error() {
	let root = TempDir::new("session-empty");
	let result = TestSession::open(root.path(), TestClock::new().options());
	assert!(
		matches!(result, Err(CoreError::Io(_))),
		"{:?}",
		result.err()
	);
}

#[test]
fn hourly_snapshots_happen_only_while_the_budget_has_changes() {
	// R-REL-3
	let (_root, dir) = imported("session-hourly");
	let clock = TestClock::new();
	let mut session = TestSession::open(&dir, clock.options()).unwrap();
	session.wait_for_open_check();

	clock.advance(TimeDelta::hours(3));
	assert_eq!(session.snapshot_if_due().unwrap(), None);

	session.perform(&rename("Everyday")).unwrap();
	assert!(session.snapshot_if_due().unwrap().is_some());

	clock.advance(TimeDelta::minutes(30));
	session.perform(&rename("Again")).unwrap();
	assert_eq!(session.snapshot_if_due().unwrap(), None);
	clock.advance(TimeDelta::minutes(31));
	assert!(session.snapshot_if_due().unwrap().is_some());

	assert_eq!(
		reasons(&session),
		[
			SnapshotReason::Open,
			SnapshotReason::Hourly,
			SnapshotReason::Hourly
		]
	);
}

#[test]
fn a_bulk_edit_is_snapshotted_before_it_runs() {
	// R-REL-3
	let (_root, dir) = imported("session-bulk");
	let clock = TestClock::new();
	let mut session = TestSession::open(&dir, clock.options()).unwrap();
	session.wait_for_open_check();
	clock.advance(TimeDelta::minutes(5));

	session.perform(&BulkRename).unwrap();

	// The open snapshot shares its hour with the newer one, so it's pruned.
	assert_eq!(reasons(&session), [SnapshotReason::BulkEdit]);
	let id = first_account(session.data().unwrap());
	assert_eq!(account_name(&session, &id), "Bulk");
}

#[test]
fn restore_brings_back_a_snapshot_and_tells_every_view_to_refetch() {
	// R-REL-3
	let (_root, dir) = imported("session-restore");
	let clock = TestClock::new();
	let mut session = TestSession::open(&dir, clock.options()).unwrap();
	session.wait_for_open_check();
	let listener = Arc::new(Recorder::default());
	session.subscribe(listener.clone());
	let id = first_account(session.data().unwrap());
	let original = account_name(&session, &id);
	let opened = session.snapshots().unwrap()[0].clone();

	clock.advance(TimeDelta::minutes(10));
	sql(
		&dir,
		&format!("UPDATE accounts SET name = 'Changed on disk' WHERE id = '{id}';"),
	);
	session.reload().unwrap();
	assert_eq!(account_name(&session, &id), "Changed on disk");

	session.perform(&rename("Edited")).unwrap();
	clock.advance(TimeDelta::minutes(10));
	let opened_id = opened.path.file_name().unwrap().to_str().unwrap();
	let changed = session.restore(opened_id).unwrap();

	assert!(changed.everything);
	assert_eq!(account_name(&session, &id), original);
	let journal: String = Connection::open(dir.join("db.sqlite"))
		.unwrap()
		.query_row("PRAGMA journal_mode", [], |row| row.get(0))
		.unwrap();
	assert_eq!(journal, "wal", "snapshot copies come back in delete mode");
	assert!(!session.can_undo());
	let heard = listener.0.lock().unwrap();
	assert_eq!(heard.iter().filter(|set| set.everything).count(), 2);

	let safety = session.snapshots().unwrap()[0].clone();
	assert_eq!(safety.reason, SnapshotReason::Restore);
	let saved: String = Connection::open(&safety.path)
		.unwrap()
		.query_row(
			&format!("SELECT name FROM accounts WHERE id = '{id}'"),
			[],
			|row| row.get(0),
		)
		.unwrap();
	assert_eq!(saved, "Changed on disk");
}

#[test]
fn restoring_a_snapshot_that_does_not_exist_changes_nothing() {
	let (_root, dir) = imported("session-restore-missing");
	let mut session = TestSession::open(&dir, TestClock::new().options()).unwrap();
	let before = session.data().unwrap().clone();
	for id in ["20990101T000000000Z-open.sqlite", "../db.sqlite", ""] {
		assert!(
			matches!(session.restore(id), Err(CoreError::NotFound { .. })),
			"{id:?}"
		);
	}
	assert_eq!(session.data().unwrap(), &before);
}

#[test]
fn a_panic_in_a_command_reloads_the_budget_and_the_session_keeps_working() {
	// R-REL-7
	let (_root, dir) = imported("session-panic-plan");
	let shared = SharedSession::new(TestSession::open(&dir, TestClock::new().options()).unwrap());
	let listener = Arc::new(Recorder::default());
	shared
		.with(|session| {
			session.subscribe(listener.clone());
			Ok(())
		})
		.unwrap();
	let before = shared.with(|session| session.data().cloned()).unwrap();

	let buggy = |_: &BudgetData| -> Result<Vec<Row>, CoreError> { panic!("a bug in a command") };
	let result = shared.with(|session| session.perform(&buggy));

	match result {
		Err(CoreError::Internal(message)) => {
			assert!(message.contains("a bug in a command"), "{message}");
			assert!(message.contains("reloaded"), "{message}");
		}
		other => panic!("expected an internal error, got {other:?}"),
	}
	assert_eq!(
		shared.with(|session| session.data().cloned()).unwrap(),
		before
	);
	assert!(listener.0.lock().unwrap().iter().any(|set| set.everything));
	shared
		.with(|session| session.perform(&rename("After the bug")))
		.unwrap();
}

#[test]
fn a_store_that_panics_mid_command_leaves_memory_matching_the_disk() {
	// R-REL-1, R-REL-7: the first row reached memory before the second write
	// panicked; the reload puts memory back to what the file holds.
	let (_root, dir) = imported("session-panic-store");
	let shared = SharedSession::new(TestSession::open(&dir, TestClock::new().options()).unwrap());
	let on_disk = shared.with(|session| session.data().cloned()).unwrap();
	let two_rows = |data: &BudgetData| -> Result<Vec<Row>, CoreError> {
		let mut rows = Vec::new();
		for (index, account) in data.accounts.values().take(2).enumerate() {
			let mut account = account.clone();
			account.name = format!("Half done {index}");
			rows.push(Row::Account(account));
		}
		assert_eq!(rows.len(), 2);
		Ok(rows)
	};

	PANIC_ON_WRITE.set(Some(1));
	let result = shared.with(|session| session.perform(&two_rows));

	assert!(matches!(result, Err(CoreError::Internal(_))), "{result:?}");
	assert_eq!(
		shared.with(|session| session.data().cloned()).unwrap(),
		on_disk
	);
	assert!(!shared.with(|session| Ok(session.can_undo())).unwrap());
}

#[test]
fn a_failed_full_check_turns_the_budget_read_only_after_it_opens() {
	// R-REL-2, R-PERF-1: the quick check passes, so the budget opens at once
	// and takes edits. When the full check in the background finds damage, it
	// turns read-only, every view refetches, and no snapshot of it is kept.
	let (_root, dir) = imported("session-background-damage");
	let release = hold_background_check(&dir, true);
	let mut session = TestSession::open(&dir, TestClock::new().options()).unwrap();
	let listener = Arc::new(Recorder::default());
	session.subscribe(listener.clone());

	assert_eq!(session.read_only(), None);
	session.perform(&rename("Before the verdict")).unwrap();
	assert_eq!(session.poll(), None);

	release.send(()).unwrap();
	let changed = session.wait_for_open_check().unwrap();

	assert!(changed.everything);
	assert_eq!(session.read_only(), Some(ReadOnlyReason::AuditFailed));
	assert!(listener.0.lock().unwrap().iter().any(|set| set.everything));
	assert!(matches!(
		session.perform(&rename("After the verdict")),
		Err(CoreError::ReadOnly(ReadOnlyReason::AuditFailed))
	));
	assert!(session.snapshots().unwrap().is_empty());
}

#[test]
fn a_damaged_file_outranks_another_window_having_it_open() {
	let (_root, dir) = imported("session-background-rank");
	let mut first = TestSession::open(&dir, TestClock::new().options()).unwrap();
	first.wait_for_open_check();
	let release = hold_background_check(&dir, true);
	let mut second = TestSession::open(&dir, TestClock::new().options()).unwrap();
	assert_eq!(second.read_only(), Some(ReadOnlyReason::OpenElsewhere));

	release.send(()).unwrap();
	second.wait_for_open_check();

	assert_eq!(second.read_only(), Some(ReadOnlyReason::AuditFailed));
}

#[test]
fn a_failed_open_snapshot_is_reported_without_blocking_the_budget() {
	// R-REL-3, R-REL-4's rule that safety failures alert and never block.
	let (_root, dir) = imported("session-snapshot-problem");
	std::fs::write(dir.join("snapshots"), b"a file where the folder should be").unwrap();
	let mut session = TestSession::open(&dir, TestClock::new().options()).unwrap();

	assert_eq!(session.wait_for_open_check(), None);

	assert!(
		matches!(session.snapshot_problem(), Some(CoreError::Io(_))),
		"{:?}",
		session.snapshot_problem()
	);
	assert_eq!(session.read_only(), None);
	session.perform(&rename("Still editable")).unwrap();
}

#[test]
fn edits_through_the_store_backend_survive_closing_and_reopening() {
	// R-REL-1, R-REL-5 end to end on SQLite: a command, its undo and its redo
	// each land in the file, and a fresh session loads what memory held.
	let (_root, dir) = imported("session-store-backend");
	let clock = TestClock::new();
	let mut session = Session::<veritas_store::Store>::open(&dir, clock.options()).unwrap();
	assert_eq!(session.wait_for_open_check(), None);
	let id = first_account(session.data().unwrap());
	let original = session.data().unwrap().accounts[&id].name.clone();

	session.perform(&rename("Everyday")).unwrap();
	let renamed = session.data().unwrap().clone();
	drop(session);
	let reopened = Session::<veritas_store::Store>::open(&dir, clock.options()).unwrap();
	assert_eq!(reopened.data().unwrap(), &renamed);
	assert_eq!(reopened.data().unwrap().accounts[&id].name, "Everyday");
	drop(reopened);

	let mut session = Session::<veritas_store::Store>::open(&dir, clock.options()).unwrap();
	session.perform(&rename("Again")).unwrap();
	session.undo().unwrap();
	drop(session);
	let reopened = Session::<veritas_store::Store>::open(&dir, clock.options()).unwrap();
	assert_eq!(reopened.data().unwrap().accounts[&id].name, "Everyday");
	assert_ne!(original, "Everyday");
}

#[test]
fn the_open_audit_finds_broken_references_and_keeps_the_report_for_the_shell() {
	// R-REL-2: opening an already-imported budget audits it, not only import.
	// A broken reference is reported and left alone; the budget stays
	// writable, because only damage SQLite itself finds makes it read-only.
	let (_root, dir) = imported("session-open-audit");
	sql(
		&dir,
		"UPDATE transactions SET acct = 'no-such-account'
		WHERE id = (SELECT id FROM transactions WHERE tombstone = 0 AND isParent = 0 LIMIT 1);",
	);
	let mut session =
		Session::<veritas_store::Store>::open(&dir, TestClock::new().options()).unwrap();
	assert!(session.audit_report().is_none());

	assert_eq!(session.wait_for_open_check(), None);

	let report = session.audit_report().unwrap();
	assert!(!report.is_corrupt());
	assert_eq!(report.violations.len(), 1, "{report:?}");
	let violation = &report.violations[0];
	assert_eq!(
		violation.check,
		veritas_store::Check::MissingReference("account")
	);
	assert_eq!(violation.table, "transactions");
	assert_eq!(violation.related.as_deref(), Some("no-such-account"));
	assert_eq!(session.read_only(), None);
}

#[test]
fn closing_a_shared_session_frees_the_budget_at_once() {
	// R-REL-6: the next window gets the writer lock without waiting for the
	// last reference to the closed session to go away.
	let (_root, dir) = imported("session-close");
	let shared = SharedSession::new(TestSession::open(&dir, TestClock::new().options()).unwrap());
	assert!(matches!(acquire(&dir).unwrap(), Access::ReadOnly(_)));

	shared.close().unwrap();

	assert!(matches!(acquire(&dir).unwrap(), Access::Writer(_)));
	assert!(matches!(
		shared.with(|session| session.data().cloned()),
		Err(CoreError::Invalid(_))
	));
}
