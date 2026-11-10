use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, ErrorCode, OpenFlags};
use veritas_domain::CoreError;

use crate::audit::{AuditReport, audit};
use crate::load::{self, Loaded};
use crate::schema;

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) fn storage(error: rusqlite::Error) -> CoreError {
	match error.sqlite_error_code() {
		Some(ErrorCode::NotADatabase) => {
			CoreError::NotAnActualBudget("db.sqlite isn't a SQLite database".to_owned())
		}
		_ => CoreError::Storage(error.to_string()),
	}
}

/// A budget's working copy, opened for Veritas to read and write. Never an
/// Actual source file: those go through [`open_read_only`].
pub struct Store {
	conn: Connection,
}

impl Store {
	pub fn open(db_path: &Path) -> Result<Store, CoreError> {
		let conn = Connection::open_with_flags(
			db_path,
			OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
		)
		.map_err(storage)?;
		conn.busy_timeout(BUSY_TIMEOUT).map_err(storage)?;
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
		Ok(Store { conn })
	}

	/// Creates any missing veritas_* tables and records the schema version.
	/// Actual's tables, `__migrations__` included, are never touched.
	pub fn adopt(&mut self) -> Result<(), CoreError> {
		schema::adopt(&mut self.conn)
	}

	pub fn load(&self) -> Result<Loaded, CoreError> {
		load::load(&self.conn)
	}

	pub fn integrity_check(&self) -> Result<Vec<String>, CoreError> {
		integrity_check(&self.conn)
	}

	/// R-REL-2 on open: SQLite's integrity check, then, when the file is
	/// sound, the load and the invariant audit of what it loaded.
	pub fn open_audit(&self) -> Result<(Option<Loaded>, AuditReport), CoreError> {
		audit_connection(&self.conn)
	}

	pub fn migration_ids(&self) -> Result<Vec<i64>, CoreError> {
		migration_ids(&self.conn)
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
/// inspecting an import before Veritas adopts it.
pub fn open_read_only(db_path: &Path) -> Result<Connection, CoreError> {
	let conn = Connection::open_with_flags(
		db_path,
		OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
	)
	.map_err(storage)?;
	conn.busy_timeout(BUSY_TIMEOUT).map_err(storage)?;
	Ok(conn)
}

/// The problems `PRAGMA integrity_check` reports. Empty means the file is sound.
pub fn integrity_check(conn: &Connection) -> Result<Vec<String>, CoreError> {
	let mut statement = conn.prepare("PRAGMA integrity_check").map_err(storage)?;
	let lines = statement
		.query_map([], |row| row.get::<_, String>(0))
		.map_err(storage)?
		.collect::<Result<Vec<_>, _>>()
		.map_err(storage)?;
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
