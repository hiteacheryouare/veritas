use rusqlite::{Connection, OptionalExtension};
use veritas_domain::{CoreError, ReadOnlyReason};

use crate::connection::storage;

pub const VERITAS_SCHEMA_VERSION: i64 = 1;

const SCHEMA_V1: &str = include_str!("../../../contracts/veritas-schema-v1.sql");

pub(crate) fn adopt(conn: &mut Connection) -> Result<(), CoreError> {
	// The checkout may turn the .sql file's line endings into CRLF, and the
	// text lands in sqlite_master, so it is normalized to stay the same bytes.
	let schema = SCHEMA_V1.replace("\r\n", "\n");
	let tx = conn.transaction().map_err(storage)?;
	tx.execute_batch(&schema).map_err(storage)?;
	add_missing_columns(&tx, &schema)?;
	let recorded: Option<String> = tx
		.query_row(
			"SELECT value FROM veritas_meta WHERE key = 'schema_version'",
			[],
			|row| row.get(0),
		)
		.optional()
		.map_err(storage)?;
	match recorded.map(|text| text.parse::<i64>()) {
		None => {
			tx.execute(
				"INSERT INTO veritas_meta (key, value) VALUES ('schema_version', ?1)",
				[VERITAS_SCHEMA_VERSION.to_string()],
			)
			.map_err(storage)?;
		}
		Some(Ok(version)) if version == VERITAS_SCHEMA_VERSION => {}
		Some(Ok(version)) if version > VERITAS_SCHEMA_VERSION => {
			return Err(CoreError::ReadOnly(ReadOnlyReason::NewerVeritasSchema));
		}
		Some(_) => {
			return Err(CoreError::Storage(
				"this budget's veritas_meta.schema_version isn't one Veritas ever wrote".to_owned(),
			));
		}
	}
	tx.commit().map_err(storage)
}

struct Column {
	name: String,
	kind: String,
	not_null: bool,
	default: Option<String>,
}

fn columns(conn: &Connection, table: &str) -> Result<Vec<Column>, CoreError> {
	let mut statement = conn
		.prepare(&format!("PRAGMA table_info(\"{table}\")"))
		.map_err(storage)?;
	statement
		.query_map([], |row| {
			Ok(Column {
				name: row.get(1)?,
				kind: row.get(2)?,
				not_null: row.get::<_, i64>(3)? != 0,
				default: row.get(4)?,
			})
		})
		.map_err(storage)?
		.collect::<Result<Vec<_>, _>>()
		.map_err(storage)
}

/// A budget adopted by an earlier build can have a veritas_* table that's
/// missing a column v1 gained later (CREATE TABLE IF NOT EXISTS leaves old
/// tables alone). Each missing column is added, so adopt stays idempotent.
fn add_missing_columns(conn: &Connection, schema: &str) -> Result<(), CoreError> {
	let reference = Connection::open_in_memory().map_err(storage)?;
	reference.execute_batch(schema).map_err(storage)?;
	let tables: Vec<String> = {
		let mut statement = reference
			.prepare(
				"SELECT name FROM sqlite_master WHERE type = 'table' AND name LIKE 'veritas\\_%' ESCAPE '\\'",
			)
			.map_err(storage)?;
		statement
			.query_map([], |row| row.get(0))
			.map_err(storage)?
			.collect::<Result<Vec<_>, _>>()
			.map_err(storage)?
	};
	for table in tables {
		let present: Vec<String> = columns(conn, &table)?
			.into_iter()
			.map(|column| column.name)
			.collect();
		for Column {
			name,
			kind,
			not_null,
			default,
		} in columns(&reference, &table)?
		{
			if present.contains(&name) {
				continue;
			}
			let default_clause = default
				.as_ref()
				.map(|value| format!(" DEFAULT {value}"))
				.unwrap_or_default();
			let not_null_clause = if not_null { " NOT NULL" } else { "" };
			if not_null && default.is_none() {
				return Err(CoreError::Storage(format!(
					"{table}.{name} can't be added to an existing table without a default"
				)));
			}
			conn.execute_batch(&format!(
				"ALTER TABLE \"{table}\" ADD COLUMN \"{name}\" {kind}{default_clause}{not_null_clause}"
			))
			.map_err(storage)?;
		}
	}
	Ok(())
}
