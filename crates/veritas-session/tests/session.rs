#![allow(
	clippy::unwrap_used,
	clippy::expect_used,
	clippy::panic,
	reason = "integration tests fail by panicking"
)]

mod support;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use rusqlite::Connection;
use support::TempDir;
use veritas_domain::{
	AccountId, BudgetData, Change, CoreError, ReadOnlyReason, Row, Store, StoreTransaction,
};
use veritas_session::{
	Access, Backend, ChangeListener, ChangeSet, Command, Session, SessionOptions, SnapshotReason,
	acquire,
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

impl StoreTransaction for Counted<'_> {
	fn apply(&mut self, _: &Change) -> Result<(), CoreError> {
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
	fn open(db: &Path) -> Result<Self, CoreError> {
		veritas_store::Store::open(db).map(|store| ReadPath { store, writes: 0 })
	}

	fn migration_ids(&self) -> Result<Vec<i64>, CoreError> {
		self.store.migration_ids()
	}

	fn integrity_problems(&self) -> Result<Vec<String>, CoreError> {
		self.store.integrity_check()
	}

	fn adopt(&mut self) -> Result<(), CoreError> {
		self.store.adopt()
	}

	fn load(&self) -> Result<Loaded, CoreError> {
		self.store.load()
	}
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
	let _first = TestSession::open(&dir, clock.options()).unwrap();
	let mut second = TestSession::open(&dir, clock.options()).unwrap();

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

	let session = TestSession::open(&dir, TestClock::new().options()).unwrap();

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
