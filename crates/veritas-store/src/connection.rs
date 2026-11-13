use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, ErrorCode, OpenFlags, TransactionBehavior};
use veritas_domain::CoreError;
use veritas_domain::{BudgetType, ReadOnlyReason};

use crate::audit::{AuditReport, audit};
use crate::load::{self, Loaded};
use crate::schema;
use crate::write::Writer;

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) fn storage(error: rusqlite::Error) -> CoreError {
	match error.sqlite_error_code() {
		Some(ErrorCode::NotADatabase) => {
			CoreError::NotAnActualBudget("db.sqlite isn't a SQLite database".to_owned())
		}
		// SQLITE_READONLY_RECOVERY, _CANTINIT and _ROLLBACK: a writer stopped
		// mid-change, and only a writable connection may replay its journal.
		Some(ErrorCode::ReadOnly)
			if matches!(
				error.sqlite_error().map(|e| e.extended_code),
				Some(264 | 776 | 1288)
			) =>
		{
			CoreError::Storage(format!(
				"{error}: a writer stopped mid-change, so call recover_after_crash while holding the writer lock"
			))
		}
		_ => CoreError::Storage(error.to_string()),
	}
}

/// Whether Veritas may write a budget. A read-only budget is opened without
/// any write, so it isn't switched to WAL, adopted or edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
	ReadWrite,
	ReadOnly(ReadOnlyReason),
}

/// A budget's working copy. Never an Actual source file: those go through
/// [`open_read_only`].
pub struct Store {
	conn: Connection,
	budget_type: BudgetType,
	access: Access,
}

impl Store {
	pub fn open(db_path: &Path, access: Access) -> Result<Store, CoreError> {
		let conn = match access {
			Access::ReadWrite => {
				let conn = Connection::open_with_flags(
					db_path,
					OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
				)
				.map_err(storage)?;
				// SQLite quietly opens a write-protected file read-only.
				if conn.is_readonly("main").map_err(storage)? {
					return Err(CoreError::Io(format!(
						"{} can't be written; check its permissions",
						db_path.display()
					)));
				}
				conn.busy_timeout(BUSY_TIMEOUT).map_err(storage)?;
				// On macOS a plain fsync leaves writes in the drive's cache, where a
				// power cut loses them. F_FULLFSYNC flushes the cache, for commits
				// and for checkpoints (R-REL-1).
				conn.execute_batch("PRAGMA fullfsync = ON; PRAGMA checkpoint_fullfsync = ON;")
					.map_err(storage)?;
				// Every open, because a restored snapshot comes back in DELETE mode.
				let mode: String = conn
					.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
					.map_err(storage)?;
				if !mode.eq_ignore_ascii_case("wal") {
					return Err(CoreError::Storage(format!(
						"SQLite kept journal mode {mode} instead of WAL"
					)));
				}
				conn.execute_batch("PRAGMA synchronous = FULL; PRAGMA foreign_keys = OFF;")
					.map_err(storage)?;
				conn
			}
			Access::ReadOnly(_) => open_read_only(db_path)?,
		};
		let budget_type = load::budget_type(&conn)?;
		Ok(Store {
			conn,
			budget_type,
			access,
		})
	}

	pub fn access(&self) -> Access {
		self.access
	}

	fn ensure_writable(&self) -> Result<(), CoreError> {
		match self.access {
			Access::ReadWrite => Ok(()),
			Access::ReadOnly(reason) => Err(CoreError::ReadOnly(reason)),
		}
	}

	/// Creates any missing veritas_* tables and records the schema version.
	/// Actual's tables, `__migrations__` included, are never touched.
	pub fn adopt(&mut self) -> Result<(), CoreError> {
		self.ensure_writable()?;
		schema::adopt(&mut self.conn)
	}

	pub fn load(&self) -> Result<Loaded, CoreError> {
		load::load(&self.conn)
	}

	/// One command's writes, as one SQLite transaction. Dropping the
	/// writer without committing rolls everything back.
	pub fn writer(&mut self) -> Result<Writer<'_>, CoreError> {
		self.ensure_writable()?;
		let tx = self
			.conn
			.transaction_with_behavior(TransactionBehavior::Immediate)
			.map_err(storage)?;
		Ok(Writer::new(tx, self.budget_type))
	}

	pub fn integrity_check(&self) -> Result<Vec<String>, CoreError> {
		integrity_check(&self.conn)
	}

	/// SQLite's integrity check, then, when the file is
	/// sound, the load and the invariant audit of what it loaded.
	pub fn open_audit(&self) -> Result<(Option<Loaded>, AuditReport), CoreError> {
		audit_connection(&self.conn)
	}

	pub fn migration_ids(&self) -> Result<Vec<i64>, CoreError> {
		migration_ids(&self.conn)
	}

	/// Writes a standalone copy of the budget for an Actual export, the way
	/// Actual's `exportBuffer` shapes one: the caches emptied and a rollback
	/// journal instead of WAL. The working copy itself is not changed.
	pub fn export_copy(&self, to: &Path) -> Result<(), CoreError> {
		let target = to
			.to_str()
			.ok_or_else(|| CoreError::Io(format!("{} isn't a UTF-8 path", to.display())))?;
		self.conn
			.execute("VACUUM INTO ?1", [target])
			.map_err(storage)?;
		let copy = Connection::open_with_flags(
			to,
			OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
		)
		.map_err(storage)?;
		for cache in ["kvcache", "kvcache_key"] {
			let exists: bool = copy
				.query_row(
					"SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
					[cache],
					|row| row.get(0),
				)
				.map_err(storage)?;
			if exists {
				copy.execute(&format!("DELETE FROM {cache}"), [])
					.map_err(storage)?;
			}
		}
		let mode: String = copy
			.query_row("PRAGMA journal_mode = DELETE", [], |row| row.get(0))
			.map_err(storage)?;
		if !mode.eq_ignore_ascii_case("delete") {
			return Err(CoreError::Storage(format!(
				"the export copy kept journal mode {mode}"
			)));
		}
		copy.close().map_err(|(_, error)| storage(error))
	}

	pub fn pragma_text(&self, pragma: &str) -> Result<String, CoreError> {
		self.conn
			.query_row(&format!("PRAGMA {pragma}"), [], |row| {
				row.get::<_, rusqlite::types::Value>(0)
			})
			.map(|value| match value {
				rusqlite::types::Value::Integer(n) => n.to_string(),
				rusqlite::types::Value::Text(text) => text,
				other => format!("{other:?}"),
			})
			.map_err(storage)
	}
}

/// Opens a database without any write, journal-mode change or WAL file, for
/// inspecting an import before Veritas adopts it. If a writer died mid-change,
/// reads fail until [`recover_after_crash`] replays what it left.
pub fn open_read_only(db_path: &Path) -> Result<Connection, CoreError> {
	let conn = Connection::open_with_flags(
		db_path,
		OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
	)
	.map_err(storage)?;
	conn.busy_timeout(BUSY_TIMEOUT).map_err(storage)?;
	Ok(conn)
}

/// Lets SQLite roll back the journal, or recover the WAL, that a writer left
/// when it stopped mid-change. That takes a writable connection, so a
/// read-only look can't do it, but nothing else is written: the journal mode
/// and every row stay as they were. Call it only while holding the budget's
/// writer lock, so no live writer's work is mistaken for a dead one's.
pub fn recover_after_crash(db_path: &Path) -> Result<(), CoreError> {
	let conn = Connection::open_with_flags(
		db_path,
		OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
	)
	.map_err(storage)?;
	conn.busy_timeout(BUSY_TIMEOUT).map_err(storage)?;
	conn.query_row("SELECT count(*) FROM sqlite_master", [], |row| {
		row.get::<_, i64>(0)
	})
	.map_err(storage)?;
	conn.close().map_err(|(_, error)| storage(error))
}

/// The problems `PRAGMA integrity_check` reports. Empty means the file is sound.
/// CHECK constraints are left out: a row that breaks one is bad data for the
/// invariant audit to report, not damage that stops the budget opening.
pub fn integrity_check(conn: &Connection) -> Result<Vec<String>, CoreError> {
	conn.execute_batch("PRAGMA ignore_check_constraints = ON")
		.map_err(storage)?;
	let lines = (|| {
		let mut statement = conn.prepare("PRAGMA integrity_check")?;
		statement
			.query_map([], |row| row.get::<_, String>(0))?
			.collect::<Result<Vec<_>, _>>()
	})();
	conn.execute_batch("PRAGMA ignore_check_constraints = OFF")
		.map_err(storage)?;
	let lines = lines.map_err(storage)?;
	Ok(if lines == ["ok"] { Vec::new() } else { lines })
}

/// The ids in `__migrations__`, ascending, as Actual reads them.
pub fn migration_ids(conn: &Connection) -> Result<Vec<i64>, CoreError> {
	let has_table: bool = conn
		.query_row(
			"SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '__migrations__')",
			[],
			|row| row.get(0),
		)
		.map_err(storage)?;
	if !has_table {
		return Err(CoreError::NotAnActualBudget(
			"its database has no __migrations__ table".to_owned(),
		));
	}
	let mut statement = conn
		.prepare("SELECT id FROM __migrations__ ORDER BY id")
		.map_err(storage)?;
	let mut rows = statement.query([]).map_err(storage)?;
	let mut ids = Vec::new();
	while let Some(row) = rows.next().map_err(storage)? {
		match row.get_ref(0).map_err(storage)? {
			rusqlite::types::ValueRef::Integer(id) => ids.push(id),
			_ => {
				return Err(CoreError::NotAnActualBudget(
					"its __migrations__ table holds a value that isn't a migration id".to_owned(),
				));
			}
		}
	}
	Ok(ids)
}

/// The integrity check, and when it passes, the load and invariant audit.
/// A corrupt file isn't loaded: nothing read from it could be trusted.
pub fn audit_connection(conn: &Connection) -> Result<(Option<Loaded>, AuditReport), CoreError> {
	let problems = integrity_check(conn)?;
	if !problems.is_empty() {
		return Ok((None, AuditReport::from_integrity(problems)));
	}
	let loaded = load::load(conn)?;
	let report = audit(&loaded);
	Ok((Some(loaded), report))
}
