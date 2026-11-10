use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, NaiveDateTime, TimeDelta, Utc};
use rusqlite::{Connection, OpenFlags};
use veritas_domain::CoreError;

use crate::fsio::{io_error, storage_error, sync_dir, sync_file, utf8};
use crate::lock::WriterLock;
use crate::retention::{RetentionPolicy, expired};

pub const SNAPSHOTS_DIR: &str = "snapshots";

const STAMP: &str = "%Y%m%dT%H%M%S%3fZ";
const EXTENSION: &str = ".sqlite";
const PARTIAL: &str = ".partial";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotReason {
	Open,
	Import,
	Export,
	VersionChange,
	BulkEdit,
	RuleApply,
	Hourly,
	Restore,
}

impl SnapshotReason {
	pub(crate) const ALL: [SnapshotReason; 8] = [
		SnapshotReason::Open,
		SnapshotReason::Import,
		SnapshotReason::Export,
		SnapshotReason::VersionChange,
		SnapshotReason::BulkEdit,
		SnapshotReason::RuleApply,
		SnapshotReason::Hourly,
		SnapshotReason::Restore,
	];

	/// Taken on a schedule rather than before an operation.
	pub fn is_routine(self) -> bool {
		matches!(self, SnapshotReason::Open | SnapshotReason::Hourly)
	}

	fn slug(self) -> &'static str {
		match self {
			SnapshotReason::Open => "open",
			SnapshotReason::Import => "import",
			SnapshotReason::Export => "export",
			SnapshotReason::VersionChange => "version-change",
			SnapshotReason::BulkEdit => "bulk-edit",
			SnapshotReason::RuleApply => "rule-apply",
			SnapshotReason::Hourly => "hourly",
			SnapshotReason::Restore => "restore",
		}
	}

	fn from_slug(slug: &str) -> Option<Self> {
		SnapshotReason::ALL
			.into_iter()
			.find(|reason| reason.slug() == slug)
	}
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
	pub path: PathBuf,
	pub taken_at: DateTime<Utc>,
	pub reason: SnapshotReason,
}

impl Snapshot {
	fn file_name(taken_at: DateTime<Utc>, reason: SnapshotReason) -> String {
		format!("{}-{}{EXTENSION}", taken_at.format(STAMP), reason.slug())
	}

	fn parse(path: PathBuf) -> Option<Snapshot> {
		let name = path.file_name()?.to_str()?;
		let (stamp, slug) = name.strip_suffix(EXTENSION)?.split_once('-')?;
		let taken_at = NaiveDateTime::parse_from_str(stamp, STAMP).ok()?.and_utc();
		let reason = SnapshotReason::from_slug(slug)?;
		Some(Snapshot {
			path,
			taken_at,
			reason,
		})
	}
}

fn partial_path(path: &Path) -> PathBuf {
	let name = path
		.file_name()
		.map(|name| name.to_string_lossy().into_owned())
		.unwrap_or_default();
	path.with_file_name(format!(".{name}{PARTIAL}"))
}

/// Copies the committed state of `db` into `snapshots_dir` with `VACUUM INTO`.
/// It reads through its own connection, so the session can stay open.
pub fn take_snapshot(
	db: &Path,
	snapshots_dir: &Path,
	reason: SnapshotReason,
	at: DateTime<Utc>,
) -> Result<Snapshot, CoreError> {
	fs::create_dir_all(snapshots_dir).map_err(io_error("create", snapshots_dir))?;
	let source =
		Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(storage_error)?;

	let mut taken_at = at;
	let mut path = snapshots_dir.join(Snapshot::file_name(taken_at, reason));
	while path.exists() {
		taken_at += TimeDelta::milliseconds(1);
		path = snapshots_dir.join(Snapshot::file_name(taken_at, reason));
	}
	let partial = partial_path(&path);
	remove_if_present(&partial)?;

	let written = source
		.execute("VACUUM INTO ?1", [utf8(&partial)?])
		.map_err(storage_error)
		.and_then(|_| source.close().map_err(|(_, error)| storage_error(error)))
		.and_then(|()| sync_file(&partial))
		.and_then(|()| fs::rename(&partial, &path).map_err(io_error("save", &path)));
	if let Err(error) = written {
		let _ = fs::remove_file(&partial);
		return Err(error);
	}
	sync_dir(snapshots_dir);
	Ok(Snapshot {
		path,
		taken_at,
		reason,
	})
}

/// Every snapshot in `snapshots_dir`, newest first. Files Veritas didn't name
/// are ignored, never deleted.
pub fn list_snapshots(snapshots_dir: &Path) -> Result<Vec<Snapshot>, CoreError> {
	let entries = match fs::read_dir(snapshots_dir) {
		Ok(entries) => entries,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
		Err(error) => return Err(io_error("read", snapshots_dir)(error)),
	};
	let mut snapshots = Vec::new();
	for entry in entries {
		let entry = entry.map_err(io_error("read", snapshots_dir))?;
		if let Some(snapshot) = Snapshot::parse(entry.path()) {
			snapshots.push(snapshot);
		}
	}
	snapshots.sort_by(|a, b| (b.taken_at, &b.path).cmp(&(a.taken_at, &a.path)));
	Ok(snapshots)
}

/// Deletes the snapshots `policy` no longer keeps, plus copies left half
/// written by a crash. Returns what was deleted.
pub fn prune_snapshots(
	snapshots_dir: &Path,
	policy: &RetentionPolicy,
	now: DateTime<Utc>,
) -> Result<Vec<Snapshot>, CoreError> {
	let gone = expired(&list_snapshots(snapshots_dir)?, policy, now);
	for snapshot in &gone {
		remove_if_present(&snapshot.path)?;
	}
	for entry in fs::read_dir(snapshots_dir)
		.map_err(io_error("read", snapshots_dir))?
		.flatten()
	{
		let name = entry.file_name();
		let name = name.to_string_lossy();
		if name.starts_with('.') && name.ends_with(PARTIAL) {
			remove_if_present(&entry.path())?;
		}
	}
	Ok(gone)
}

/// Replaces `db` with `snapshot` in one call. The current database is
/// snapshotted first, so a restore can itself be undone. It deletes the
/// `-wal` and `-shm` files, which would corrupt any other connection, so it
/// takes the budget's writer lock and the session closes its own connection
/// before calling it.
pub fn restore_snapshot(
	lock: &WriterLock,
	snapshot: &Path,
	db: &Path,
	snapshots_dir: &Path,
	at: DateTime<Utc>,
) -> Result<Option<Snapshot>, CoreError> {
	lock.covers(db)?;
	ensure_intact(snapshot)?;
	let safety = if db.exists() {
		checkpoint(db)?;
		Some(take_snapshot(
			db,
			snapshots_dir,
			SnapshotReason::Restore,
			at,
		)?)
	} else {
		None
	};

	let staged = partial_path(db);
	let copied = fs::copy(snapshot, &staged)
		.map_err(io_error("copy", snapshot))
		.and_then(|_| sync_file(&staged))
		.and_then(|()| remove_if_present(&sidecar(db, "-wal")))
		.and_then(|()| remove_if_present(&sidecar(db, "-shm")))
		.and_then(|()| fs::rename(&staged, db).map_err(io_error("replace", db)));
	if let Err(error) = copied {
		let _ = fs::remove_file(&staged);
		return Err(error);
	}
	if let Some(dir) = db.parent() {
		sync_dir(dir);
	}
	Ok(safety)
}

fn ensure_intact(snapshot: &Path) -> Result<(), CoreError> {
	let connection = Connection::open_with_flags(snapshot, OpenFlags::SQLITE_OPEN_READ_ONLY)
		.map_err(storage_error)?;
	let verdict: String = connection
		.query_row("PRAGMA quick_check", [], |row| row.get(0))
		.map_err(storage_error)?;
	if verdict == "ok" {
		Ok(())
	} else {
		Err(CoreError::Storage(format!(
			"the snapshot {} is damaged, so it wasn't restored",
			snapshot.display()
		)))
	}
}

/// Folds the WAL into the main file so swapping the file can't strand
/// committed pages in a `-wal` that would then apply to the wrong database.
fn checkpoint(db: &Path) -> Result<(), CoreError> {
	let connection = Connection::open(db).map_err(storage_error)?;
	connection
		.busy_timeout(Duration::from_secs(1))
		.map_err(storage_error)?;
	let busy: i64 = connection
		.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))
		.map_err(storage_error)?;
	if busy != 0 {
		return Err(CoreError::Storage(
			"the budget is still in use, so the snapshot wasn't restored".to_owned(),
		));
	}
	connection
		.close()
		.map_err(|(_, error)| storage_error(error))
}

fn sidecar(db: &Path, suffix: &str) -> PathBuf {
	let mut name = db.as_os_str().to_owned();
	name.push(suffix);
	PathBuf::from(name)
}

fn remove_if_present(path: &Path) -> Result<(), CoreError> {
	match fs::remove_file(path) {
		Err(error) if error.kind() != ErrorKind::NotFound => Err(io_error("delete", path)(error)),
		_ => Ok(()),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn snapshot_names_round_trip_through_parsing() {
		let at = DateTime::parse_from_rfc3339("2026-11-09T20:30:05.042Z")
			.unwrap()
			.to_utc();
		for reason in SnapshotReason::ALL {
			let name = Snapshot::file_name(at, reason);
			let parsed = Snapshot::parse(PathBuf::from("snapshots").join(&name)).unwrap();
			assert_eq!((parsed.taken_at, parsed.reason), (at, reason), "{name}");
		}
		assert_eq!(
			Snapshot::file_name(at, SnapshotReason::BulkEdit),
			"20261109T203005042Z-bulk-edit.sqlite"
		);
	}

	#[test]
	fn foreign_and_half_written_files_are_not_snapshots() {
		for name in [
			"notes.txt",
			".20261109T203005042Z-open.sqlite.partial",
			"20261109T203005042Z-someday.sqlite",
			"yesterday-open.sqlite",
		] {
			assert!(Snapshot::parse(PathBuf::from(name)).is_none(), "{name}");
		}
	}
}
