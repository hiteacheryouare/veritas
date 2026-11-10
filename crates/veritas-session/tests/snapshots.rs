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
	Access, LOCK_FILE, RetentionPolicy, SNAPSHOTS_DIR, SnapshotReason, WriterLock, acquire,
	list_snapshots, prune_snapshots, restore_snapshot, take_snapshot,
};

fn at(text: &str) -> DateTime<Utc> {
	DateTime::parse_from_rfc3339(text).unwrap().to_utc()
}

fn writer(dir: &TempDir) -> WriterLock {
	match acquire(dir.path()).unwrap() {
		Access::Writer(lock) => lock,
		Access::ReadOnly(reason) => panic!("expected the writer lock, got {reason:?}"),
	}
}

struct Budget {
	lock: WriterLock,
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
	(
		Budget {
			lock: writer(&dir),
			dir,
			db,
			snapshots,
		},
		connection,
	)
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
		&budget.lock,
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
	assert_eq!(
		entries(budget.dir.path()),
		["db.sqlite", "snapshots", LOCK_FILE]
	);
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
		&budget.lock,
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
		&budget.lock,
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
		operation_days: 30,
	};
	let now = start + TimeDelta::days(3);
	let gone = prune_snapshots(&budget.snapshots, &policy, now).unwrap();

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
		prune_snapshots(&budget.snapshots, &policy, now)
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

/// A budget shaped like Actual's: several tables, an index, a view, a trigger,
/// a `WITHOUT ROWID` table and a stamped `user_version`, with enough rows to
/// span many pages.
fn realistic_budget(label: &str) -> (Budget, Connection) {
	let dir = TempDir::new(label);
	let db = dir.path().join("db.sqlite");
	let snapshots = dir.path().join(SNAPSHOTS_DIR);
	let connection = Connection::open(&db).unwrap();
	connection
		.execute_batch(
			"PRAGMA journal_mode = WAL;
			PRAGMA user_version = 26;
			CREATE TABLE accounts (id TEXT PRIMARY KEY, name TEXT, offbudget INTEGER, tombstone INTEGER DEFAULT 0);
			CREATE TABLE categories (id TEXT PRIMARY KEY, name TEXT, cat_group TEXT, tombstone INTEGER DEFAULT 0);
			CREATE TABLE transactions (id TEXT PRIMARY KEY, acct TEXT, category TEXT, amount INTEGER, date INTEGER, notes TEXT, tombstone INTEGER DEFAULT 0);
			CREATE INDEX trans_category_date ON transactions (category, date);
			CREATE TABLE zero_budgets (id TEXT PRIMARY KEY, month INTEGER, category TEXT, amount INTEGER) WITHOUT ROWID;
			CREATE TABLE audit (at INTEGER, what TEXT);
			CREATE TRIGGER log_delete AFTER DELETE ON transactions BEGIN INSERT INTO audit VALUES (old.date, old.id); END;
			CREATE VIEW v_live AS SELECT * FROM transactions WHERE tombstone = 0;
			INSERT INTO accounts VALUES ('checking', 'Checking', 0, 0), ('visa', 'Visa', 0, 0), ('401k', '401(k)', 1, 0);
			INSERT INTO categories VALUES ('groceries', 'Groceries', 'food', 0), ('rent', 'Rent', 'bills', 0), ('fun', 'Fun money', 'wants', 0);",
		)
		.unwrap();
	let mut insert = connection
		.prepare("INSERT INTO transactions VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)")
		.unwrap();
	for n in 0..3_000_i64 {
		insert
			.execute(rusqlite::params![
				format!("t{n:05}"),
				["checking", "visa"][(n % 2) as usize],
				["groceries", "rent", "fun"][(n % 3) as usize],
				-(n * 137 % 25_000),
				20_250_101 + n % 28,
				(n % 7 == 0).then(|| format!("memo {n} ☕ Café — split w/ Sam")),
			])
			.unwrap();
	}
	drop(insert);
	for month in 202_501..=202_512 {
		connection
			.execute(
				"INSERT INTO zero_budgets VALUES (?1, ?2, 'groceries', 60000)",
				rusqlite::params![format!("{month}-groceries"), month],
			)
			.unwrap();
	}
	(
		Budget {
			lock: writer(&dir),
			dir,
			db,
			snapshots,
		},
		connection,
	)
}

/// Every schema object and every row, read through a fresh connection, so two
/// databases with the same dump hold the same budget.
fn dump(db: &Path) -> (i64, Vec<String>) {
	let connection = Connection::open(db).unwrap();
	let user_version = connection
		.query_row("PRAGMA user_version", [], |row| row.get(0))
		.unwrap();
	let mut lines = Vec::new();
	let mut schema = connection
		.prepare("SELECT type, name, sql FROM sqlite_master ORDER BY type, name")
		.unwrap();
	let objects: Vec<(String, String, Option<String>)> = schema
		.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
		.unwrap()
		.map(Result::unwrap)
		.collect();
	for (kind, name, sql) in &objects {
		lines.push(format!("{kind} {name}: {sql:?}"));
		if kind != "table" {
			continue;
		}
		let mut select = connection
			.prepare(&format!("SELECT * FROM \"{name}\""))
			.unwrap();
		let width = select.column_count();
		let mut rows: Vec<String> = select
			.query_map([], |row| {
				let values: Vec<rusqlite::types::Value> =
					(0..width).map(|i| row.get(i)).collect::<Result<_, _>>()?;
				Ok(format!("{name} {values:?}"))
			})
			.unwrap()
			.map(Result::unwrap)
			.collect();
		rows.sort();
		lines.extend(rows);
	}
	(user_version, lines)
}

fn integrity(db: &Path) -> String {
	Connection::open(db)
		.unwrap()
		.query_row("PRAGMA integrity_check", [], |row| row.get(0))
		.unwrap()
}

#[test]
fn restoring_after_a_bulk_edit_brings_back_every_row_and_schema_object() {
	// R-REL-3: a snapshot before a bulk edit, then deletes, inserts, updates
	// and schema changes, then one-call restore. The restored file is the
	// snapshot byte for byte, and its contents match the pre-edit budget.
	let (budget, connection) = realistic_budget("snap-bulk");
	let before_edit = dump(&budget.db);
	let snapshot = take_snapshot(
		&budget.db,
		&budget.snapshots,
		SnapshotReason::BulkEdit,
		at("2026-11-09T20:00:00Z"),
	)
	.unwrap();
	assert_eq!(dump(&snapshot.path), before_edit);

	connection
		.execute_batch(
			"BEGIN;
			DELETE FROM transactions WHERE category = 'fun';
			UPDATE transactions SET category = 'groceries', notes = 'recategorized' WHERE category = 'rent';
			INSERT INTO transactions VALUES ('t-new', 'checking', 'groceries', -4512, 20251109, NULL, 0);
			UPDATE zero_budgets SET amount = 0;
			ALTER TABLE accounts ADD COLUMN veritas_extra TEXT;
			CREATE TABLE veritas_scratch (id TEXT);
			DROP INDEX trans_category_date;
			PRAGMA user_version = 27;
			COMMIT;",
		)
		.unwrap();
	drop(connection);
	let after_edit = dump(&budget.db);
	assert_ne!(after_edit, before_edit);

	let safety = restore_snapshot(
		&budget.lock,
		&snapshot.path,
		&budget.db,
		&budget.snapshots,
		at("2026-11-09T20:30:00Z"),
	)
	.unwrap()
	.unwrap();

	assert_eq!(
		fs::read(&budget.db).unwrap(),
		fs::read(&snapshot.path).unwrap()
	);
	assert_eq!(dump(&budget.db), before_edit);
	assert_eq!(integrity(&budget.db), "ok");
	assert_eq!(dump(&safety.path), after_edit);

	restore_snapshot(
		&budget.lock,
		&safety.path,
		&budget.db,
		&budget.snapshots,
		at("2026-11-09T20:31:00Z"),
	)
	.unwrap();
	assert_eq!(dump(&budget.db), after_edit);
	assert_eq!(integrity(&budget.db), "ok");
}

#[test]
fn a_snapshot_taken_mid_command_holds_only_committed_work() {
	// R-REL-3: an hourly snapshot can fire while a command's transaction is
	// still open. It must capture the last commit, never half a command.
	let (budget, connection) = realistic_budget("snap-mid-command");
	let committed = dump(&budget.db);
	connection
		.execute_batch(
			"BEGIN IMMEDIATE;
			UPDATE transactions SET amount = 0;
			DELETE FROM zero_budgets;",
		)
		.unwrap();

	let snapshot = take_snapshot(
		&budget.db,
		&budget.snapshots,
		SnapshotReason::Hourly,
		at("2026-11-09T21:00:00Z"),
	)
	.unwrap();

	connection.execute_batch("COMMIT").unwrap();
	assert_eq!(dump(&snapshot.path), committed);
	assert_eq!(integrity(&snapshot.path), "ok");
	assert_ne!(dump(&budget.db), committed);
}

#[test]
fn a_month_of_daily_use_keeps_every_recent_hour_and_the_end_of_every_day() {
	// R-REL-3 with the default policy (48 hourly, 30 daily, 12 monthly): 40
	// days of evenings with edits, an open snapshot each evening, an import
	// every Sunday and a bulk edit every Wednesday, pruned after every
	// snapshot the way the session will.
	let (budget, connection) = budget("snap-month");
	let policy = RetentionPolicy::default();
	let start = at("2026-10-01T00:00:00Z");
	let mut taken = Vec::new();
	for day in 0..40 {
		let evening = start + TimeDelta::days(day) + TimeDelta::hours(17);
		let mut snap = |offset: TimeDelta, reason: SnapshotReason| {
			let snapshot =
				take_snapshot(&budget.db, &budget.snapshots, reason, evening + offset).unwrap();
			taken.push(snapshot.taken_at);
			prune_snapshots(&budget.snapshots, &policy, evening + offset).unwrap();
		};
		snap(TimeDelta::minutes(2), SnapshotReason::Open);
		if day % 7 == 3 {
			snap(TimeDelta::minutes(10), SnapshotReason::Import);
		}
		if day % 7 == 6 {
			snap(TimeDelta::minutes(40), SnapshotReason::BulkEdit);
		}
		for hour in 1..=5 {
			set_balance(&connection, day * 100 + hour);
			snap(TimeDelta::hours(hour), SnapshotReason::Hourly);
		}
	}

	let kept: Vec<DateTime<Utc>> = list_snapshots(&budget.snapshots)
		.unwrap()
		.iter()
		.map(|snapshot| snapshot.taken_at)
		.collect();
	let newest = taken.iter().max().copied().unwrap();
	let newest_in =
		|within: &dyn Fn(&DateTime<Utc>) -> bool| taken.iter().filter(|t| within(t)).max().copied();

	let mut hours: Vec<_> = taken
		.iter()
		.map(|t| t.format("%Y-%m-%dT%H").to_string())
		.collect();
	hours.dedup();
	for hour in hours.iter().rev().take(policy.hourly) {
		let wanted = newest_in(&|t| t.format("%Y-%m-%dT%H").to_string() == *hour).unwrap();
		assert!(kept.contains(&wanted), "lost the newest snapshot of {hour}");
	}
	for day in 0..policy.daily as i64 {
		let date = (newest - TimeDelta::days(day)).date_naive();
		let wanted = newest_in(&|t| t.date_naive() == date).unwrap();
		assert!(kept.contains(&wanted), "lost the end of {date}");
	}
	for month in ["2026-10", "2026-11"] {
		let wanted = newest_in(&|t| t.format("%Y-%m").to_string() == month).unwrap();
		assert!(kept.contains(&wanted), "lost the end of {month}");
	}
	assert!(kept.contains(&newest));
	let recent_operations = list_snapshots(&budget.snapshots)
		.unwrap()
		.iter()
		.filter(|snapshot| {
			!snapshot.reason.is_routine()
				&& newest - snapshot.taken_at < TimeDelta::days(policy.operation_days)
		})
		.count();
	assert!(recent_operations > 0);
	assert!(
		kept.len() <= policy.hourly + policy.daily + policy.monthly + recent_operations,
		"{} snapshots kept",
		kept.len()
	);
	assert_eq!(
		entries(&budget.snapshots).len(),
		kept.len(),
		"stray files left in the snapshots folder"
	);
	let latest = &list_snapshots(&budget.snapshots).unwrap()[0];
	assert_eq!(balance(&latest.path), 39 * 100 + 5);
}

#[test]
fn a_snapshot_taken_before_a_risky_operation_survives_the_next_prune_in_the_same_hour() {
	// R-REL-3 and §10 "restoring a snapshot brings back the exact prior state":
	// the user imports at 14:20 and exports at 14:50, then wants the budget
	// from before the import back. Likewise the safety snapshot a restore
	// takes is lost to the snapshot the session takes when it reopens.
	let (budget, connection) = budget("snap-risky");
	let policy = RetentionPolicy::default();
	let before_import = take_snapshot(
		&budget.db,
		&budget.snapshots,
		SnapshotReason::Import,
		at("2026-11-10T14:20:00Z"),
	)
	.unwrap();
	set_balance(&connection, 55);
	take_snapshot(
		&budget.db,
		&budget.snapshots,
		SnapshotReason::Export,
		at("2026-11-10T14:50:00Z"),
	)
	.unwrap();
	prune_snapshots(&budget.snapshots, &policy, at("2026-11-10T14:50:00Z")).unwrap();
	assert!(
		before_import.path.exists(),
		"the pre-import snapshot was pruned"
	);

	drop(connection);
	let safety = restore_snapshot(
		&budget.lock,
		&before_import.path,
		&budget.db,
		&budget.snapshots,
		at("2026-11-10T15:30:00Z"),
	)
	.unwrap()
	.unwrap();
	take_snapshot(
		&budget.db,
		&budget.snapshots,
		SnapshotReason::Open,
		at("2026-11-10T15:30:01Z"),
	)
	.unwrap();
	prune_snapshots(&budget.snapshots, &policy, at("2026-11-10T15:30:01Z")).unwrap();
	assert!(
		safety.path.exists(),
		"the restore's safety snapshot was pruned"
	);
}

#[test]
fn restore_refuses_a_lock_held_for_another_budget() {
	// The lock is what keeps every other connection away while -wal and
	// -shm are deleted, so it has to be this budget's lock.
	let (mine, connection) = budget("snap-other-lock");
	let snapshot = take_snapshot(
		&mine.db,
		&mine.snapshots,
		SnapshotReason::Open,
		at("2026-11-09T20:00:00Z"),
	)
	.unwrap();
	set_balance(&connection, 3);
	drop(connection);
	let (other, _other_connection) = budget("snap-other-lock-2");

	let result = restore_snapshot(
		&other.lock,
		&snapshot.path,
		&mine.db,
		&mine.snapshots,
		at("2026-11-09T21:00:00Z"),
	);

	assert!(matches!(result, Err(CoreError::Internal(_))), "{result:?}");
	assert_eq!(balance(&mine.db), 3);
}
