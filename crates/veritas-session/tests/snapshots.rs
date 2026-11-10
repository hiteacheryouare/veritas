#![allow(
	clippy::unwrap_used,
	clippy::expect_used,
	clippy::panic,
	reason = "integration tests fail by panicking"
)]

mod support;

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeDelta, Utc};
use rusqlite::Connection;
use support::{TempDir, entries};
use veritas_domain::CoreError;
use veritas_session::{
	RetentionPolicy, SNAPSHOTS_DIR, SnapshotReason, list_snapshots, prune_snapshots,
	restore_snapshot, take_snapshot,
};

fn at(text: &str) -> DateTime<Utc> {
	DateTime::parse_from_rfc3339(text).unwrap().to_utc()
}

struct Budget {
	dir: TempDir,
	db: PathBuf,
	snapshots: PathBuf,
}

/// A budget directory holding a WAL-mode database with one account, the way
/// the session keeps it open.
fn budget(label: &str) -> (Budget, Connection) {
	let dir = TempDir::new(label);
	let db = dir.path().join("db.sqlite");
	let snapshots = dir.path().join(SNAPSHOTS_DIR);
	let connection = Connection::open(&db).unwrap();
	connection
		.execute_batch(
			"PRAGMA journal_mode = WAL;
			CREATE TABLE accounts (id TEXT PRIMARY KEY, name TEXT, balance INTEGER);
			INSERT INTO accounts VALUES ('checking', 'Checking', 120000);",
		)
		.unwrap();
	(Budget { dir, db, snapshots }, connection)
}

fn balance(db: &Path) -> i64 {
	Connection::open(db)
		.unwrap()
		.query_row(
			"SELECT balance FROM accounts WHERE id = 'checking'",
			[],
			|row| row.get(0),
		)
		.unwrap()
}

fn set_balance(connection: &Connection, balance: i64) {
	connection
		.execute(
			"UPDATE accounts SET balance = ?1 WHERE id = 'checking'",
			[balance],
		)
		.unwrap();
}

#[test]
fn a_snapshot_captures_committed_writes_while_the_budget_stays_open() {
	// R-REL-3
	let (budget, connection) = budget("snap-open");
	set_balance(&connection, 98_765);
	assert!(budget.dir.path().join("db.sqlite-wal").exists());

	let snapshot = take_snapshot(
		&budget.db,
		&budget.snapshots,
		SnapshotReason::Open,
		at("2026-11-09T20:30:00Z"),
	)
	.unwrap();

	assert_eq!(
		snapshot.path,
		budget.snapshots.join("20261109T203000000Z-open.sqlite")
	);
	assert_eq!(balance(&snapshot.path), 98_765);
	assert_eq!(
		entries(&budget.snapshots),
		["20261109T203000000Z-open.sqlite"]
	);
	assert_eq!(list_snapshots(&budget.snapshots).unwrap(), [snapshot]);
}

#[test]
fn snapshots_taken_in_the_same_millisecond_get_their_own_files() {
	let (budget, _connection) = budget("snap-same");
	let now = at("2026-11-09T20:30:00Z");
	let first = take_snapshot(&budget.db, &budget.snapshots, SnapshotReason::Import, now).unwrap();
	let second = take_snapshot(&budget.db, &budget.snapshots, SnapshotReason::Import, now).unwrap();
	assert_ne!(first.path, second.path);
	assert_eq!(second.taken_at, now + TimeDelta::milliseconds(1));
	let listed = list_snapshots(&budget.snapshots).unwrap();
	assert_eq!(listed, [second, first]);
}

#[test]
fn restore_brings_back_the_exact_prior_state_in_one_call() {
	// R-REL-3
	let (budget, connection) = budget("snap-restore");
	let before_import = take_snapshot(
		&budget.db,
		&budget.snapshots,
		SnapshotReason::Import,
		at("2026-11-09T20:00:00Z"),
	)
	.unwrap();
	set_balance(&connection, 1);
	drop(connection);

	let safety = restore_snapshot(
		&before_import.path,
		&budget.db,
		&budget.snapshots,
		at("2026-11-09T21:00:00Z"),
	)
	.unwrap()
	.unwrap();

	assert_eq!(balance(&budget.db), 120_000);
	assert_eq!(safety.reason, SnapshotReason::Restore);
	assert_eq!(balance(&safety.path), 1);
	assert_eq!(entries(budget.dir.path()), ["db.sqlite", "snapshots"]);
	let integrity: String = Connection::open(&budget.db)
		.unwrap()
		.query_row("PRAGMA integrity_check", [], |row| row.get(0))
		.unwrap();
	assert_eq!(integrity, "ok");
}

#[test]
fn a_damaged_snapshot_is_refused_and_the_budget_is_untouched() {
	let (budget, connection) = budget("snap-damaged");
	set_balance(&connection, 4_242);
	drop(connection);
	fs::create_dir_all(&budget.snapshots).unwrap();
	let damaged = budget.snapshots.join("20261109T200000000Z-hourly.sqlite");
	fs::write(&damaged, b"not a database at all, just bytes").unwrap();

	let result = restore_snapshot(
		&damaged,
		&budget.db,
		&budget.snapshots,
		at("2026-11-09T22:00:00Z"),
	);

	assert!(matches!(result, Err(CoreError::Storage(_))), "{result:?}");
	assert_eq!(balance(&budget.db), 4_242);
	assert_eq!(
		entries(&budget.snapshots),
		["20261109T200000000Z-hourly.sqlite"]
	);
}

#[test]
fn restore_refuses_while_the_budget_is_still_open() {
	let (budget, connection) = budget("snap-busy");
	let snapshot = take_snapshot(
		&budget.db,
		&budget.snapshots,
		SnapshotReason::Open,
		at("2026-11-09T22:00:00Z"),
	)
	.unwrap();
	let reader = Connection::open(&budget.db).unwrap();
	reader.execute_batch("BEGIN").unwrap();
	let _: i64 = reader
		.query_row("SELECT balance FROM accounts", [], |row| row.get(0))
		.unwrap();
	set_balance(&connection, 7);

	let result = restore_snapshot(
		&snapshot.path,
		&budget.db,
		&budget.snapshots,
		at("2026-11-09T22:00:00Z"),
	);

	assert!(matches!(result, Err(CoreError::Storage(_))), "{result:?}");
	drop(reader);
	assert_eq!(balance(&budget.db), 7);
}

#[test]
fn pruning_applies_the_policy_and_clears_half_written_copies() {
	// R-REL-3
	let (budget, _connection) = budget("snap-prune");
	let start = at("2026-11-01T00:00:00Z");
	for hour in 0..10 {
		take_snapshot(
			&budget.db,
			&budget.snapshots,
			SnapshotReason::Hourly,
			start + TimeDelta::hours(hour * 7),
		)
		.unwrap();
	}
	let partial = budget
		.snapshots
		.join(".20261101T000000000Z-hourly.sqlite.partial");
	fs::write(&partial, b"half").unwrap();
	fs::write(budget.snapshots.join("keep-me.txt"), b"mine").unwrap();

	let policy = RetentionPolicy {
		hourly: 2,
		daily: 1,
		monthly: 1,
	};
	let gone = prune_snapshots(&budget.snapshots, &policy).unwrap();

	assert_eq!(gone.len(), 8);
	assert!(!partial.exists());
	let left: Vec<_> = list_snapshots(&budget.snapshots)
		.unwrap()
		.into_iter()
		.map(|snapshot| snapshot.taken_at)
		.collect();
	assert_eq!(
		left,
		[start + TimeDelta::hours(63), start + TimeDelta::hours(56)]
	);
	assert!(budget.snapshots.join("keep-me.txt").exists());
	assert!(
		prune_snapshots(&budget.snapshots, &policy)
			.unwrap()
			.is_empty()
	);
}

#[test]
fn snapshotting_a_missing_database_fails_without_leaving_files() {
	let dir = TempDir::new("snap-missing");
	let snapshots = dir.path().join(SNAPSHOTS_DIR);
	let result = take_snapshot(
		&dir.path().join("db.sqlite"),
		&snapshots,
		SnapshotReason::Open,
		at("2026-11-09T22:00:00Z"),
	);
	assert!(matches!(result, Err(CoreError::Storage(_))), "{result:?}");
	assert!(entries(&snapshots).is_empty());
	assert_eq!(entries(dir.path()), ["snapshots"]);
}
