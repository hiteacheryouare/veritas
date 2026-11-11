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
