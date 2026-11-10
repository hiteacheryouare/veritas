pub(crate) mod json;

use std::io::{self, BufWriter, Write};
use std::path::Path;

use rusqlite::Connection;
use rusqlite::types::ValueRef;
use veritas_domain::CoreError;

use crate::archive::BudgetArchive;
use crate::scratch::ScratchDir;
use json::{Str, Value, write_compact, write_indent, write_pretty, write_string};

pub const DUMP_FORMAT: &str = "actual-canonical-dump";
pub const DUMP_VERSION: i64 = 1;

const EXCLUDED_TABLES: [&str; 2] = ["kvcache", "kvcache_key"];

/// metadata.json keys that describe one local copy rather than the budget.
pub const VOLATILE_METADATA_KEYS: [&str; 7] = [
	"cloudFileId",
	"encryptKeyId",
	"groupId",
	"id",
	"lastSyncedTimestamp",
	"lastUploaded",
	"userId",
];

fn storage(error: rusqlite::Error) -> CoreError {
	CoreError::Storage(error.to_string())
}

fn write_error(error: io::Error) -> CoreError {
	CoreError::Io(error.to_string())
}

fn has_non_finite_number(value: &Value) -> bool {
	match value {
		Value::Number(x) => !x.is_finite(),
		Value::Array(items) => items.iter().any(has_non_finite_number),
		Value::Object(entries) => entries.iter().any(|(_, item)| has_non_finite_number(item)),
		_ => false,
	}
}

/// The raw part of the canonical dump (contracts/canonical-dump.md,
/// docs/oracle.md "Canonical dump, version 1") of an Actual export zip. It is
/// byte-identical to `tools/actual-oracle dump --raw-only`.
pub fn canonical_dump(zip_path: &Path, out: impl Write) -> Result<(), CoreError> {
	let mut archive = BudgetArchive::open(zip_path)?;
	let metadata = archive.metadata_bytes()?;
	let scratch = ScratchDir::new("dump")?;
	let db_path = scratch.path().join("db.sqlite");
	archive.extract_db(&db_path)?;
	let conn = veritas_store::open_read_only(&db_path)?;
	let result = dump_database(&conn, &metadata, out);
	drop(conn);
	result
}

pub fn canonical_dump_to_vec(zip_path: &Path) -> Result<Vec<u8>, CoreError> {
	let mut out = Vec::new();
	canonical_dump(zip_path, &mut out)?;
	Ok(out)
}

fn normalize_metadata(bytes: &[u8]) -> Result<Value, CoreError> {
	let text = String::from_utf8_lossy(bytes);
	let mut metadata =
		json::parse(&text).map_err(|reason| CoreError::NotAnActualBudget(reason.to_owned()))?;
	if has_non_finite_number(&metadata) {
		return Err(CoreError::Invalid(
			"metadata.json has a number too large to write back as JSON".to_owned(),
		));
	}
	if let Value::Object(entries) = &mut metadata {
		entries.retain(|(key, _)| {
			!VOLATILE_METADATA_KEYS
				.iter()
				.any(|volatile| key.eq_ascii(volatile))
		});
	}
	Ok(metadata)
}

fn cell(value: ValueRef<'_>) -> Value {
	match value {
		ValueRef::Null => Value::Null,
		ValueRef::Integer(n) => Value::Integer(n),
		ValueRef::Real(x) => Value::SqlReal(x),
		ValueRef::Text(bytes) => {
			Value::String(Str::Utf8(String::from_utf8_lossy(bytes).into_owned()))
		}
		ValueRef::Blob(bytes) => Value::Blob(bytes.to_vec()),
	}
}

fn quote_identifier(name: &str) -> String {
	format!("\"{}\"", name.replace('"', "\"\""))
}

fn schema(conn: &Connection) -> Result<Value, CoreError> {
	let mut statement = conn
		.prepare(
			"SELECT type, name, tbl_name, sql FROM sqlite_master WHERE name NOT LIKE 'sqlite\\_%' ESCAPE '\\'",
		)
		.map_err(storage)?;
	let mut rows = statement.query([]).map_err(storage)?;
	let mut entries: Vec<(Vec<u8>, Vec<u8>, Value)> = Vec::new();
	while let Some(row) = rows.next().map_err(storage)? {
		let get = |index| row.get_ref(index).map(cell).map_err(storage);
		let (kind, name, table, sql) = (get(0)?, get(1)?, get(2)?, get(3)?);
		let bytes = |value: &Value| match value {
			Value::String(Str::Utf8(text)) => text.as_bytes().to_vec(),
			_ => Vec::new(),
		};
		let sort_key = (bytes(&kind), bytes(&name));
		let object = Value::Object(vec![
			(Str::from("name"), name),
			(Str::from("sql"), sql),
			(Str::from("tableName"), table),
			(Str::from("type"), kind),
		]);
		entries.push((sort_key.0, sort_key.1, object));
	}
	entries.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
	Ok(Value::Array(
		entries.into_iter().map(|(_, _, object)| object).collect(),
	))
}

fn table_names(conn: &Connection) -> Result<Vec<String>, CoreError> {
	let mut statement = conn
		.prepare(
			"SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\'",
		)
		.map_err(storage)?;
	let mut names = statement
		.query_map([], |row| {
			row.get_ref(0).map(|value| match value {
				ValueRef::Text(bytes) => String::from_utf8_lossy(bytes).into_owned(),
				_ => String::new(),
			})
		})
		.map_err(storage)?
		.collect::<Result<Vec<_>, _>>()
		.map_err(storage)?;
	names.retain(|name| !EXCLUDED_TABLES.contains(&name.as_str()));
	names.sort();
	Ok(names)
}

fn primary_key(conn: &Connection, table: &str) -> Result<(Vec<String>, Vec<String>), CoreError> {
	let mut statement = conn
		.prepare(&format!("PRAGMA table_info({})", quote_identifier(table)))
		.map_err(storage)?;
	let columns = statement
		.query_map([], |row| {
			Ok((row.get::<_, String>("name")?, row.get::<_, i64>("pk")?))
		})
		.map_err(storage)?
		.collect::<Result<Vec<_>, _>>()
		.map_err(storage)?;
	let mut keyed: Vec<&(String, i64)> = columns.iter().filter(|(_, pk)| *pk > 0).collect();
	keyed.sort_by_key(|(_, pk)| *pk);
	let key = keyed.into_iter().map(|(name, _)| name.clone()).collect();
	let all = columns.into_iter().map(|(name, _)| name).collect();
	Ok((key, all))
}

fn write_table(conn: &Connection, table: &str, out: &mut impl Write) -> Result<(), CoreError> {
	let (key, columns) = primary_key(conn, table)?;
	let order_by = if key.is_empty() { &columns } else { &key }
		.iter()
		.map(|name| format!("{} COLLATE BINARY", quote_identifier(name)))
		.collect::<Vec<_>>()
		.join(", ");
	let mut statement = conn
		.prepare(&format!(
			"SELECT * FROM {} ORDER BY {order_by}",
			quote_identifier(table)
		))
		.map_err(storage)?;
	let names: Vec<String> = statement.column_names().map(str::to_owned).collect();
	let mut sorted_columns: Vec<usize> = (0..names.len()).collect();
	sorted_columns.sort_by(|&a, &b| names[a].as_bytes().cmp(names[b].as_bytes()));
	let keys: Vec<Str> = sorted_columns
		.iter()
		.map(|&index| Str::Utf8(names[index].clone()))
		.collect();

	out.write_all(b"{\n").map_err(write_error)?;
	write_indent(out, 3).map_err(write_error)?;
	out.write_all(b"\"primaryKey\": ").map_err(write_error)?;
	let key_value = Value::Array(
		key.into_iter()
			.map(|name| Value::String(Str::Utf8(name)))
			.collect(),
	);
	write_pretty(out, &key_value, 3).map_err(write_error)?;
	out.write_all(b",\n").map_err(write_error)?;
	write_indent(out, 3).map_err(write_error)?;
	out.write_all(b"\"rows\": ").map_err(write_error)?;

	let mut rows = statement.query([]).map_err(storage)?;
	let mut first = true;
	while let Some(row) = rows.next().map_err(storage)? {
		out.write_all(if first { b"[\n" } else { b",\n" })
			.map_err(write_error)?;
		first = false;
		let mut cells = Vec::with_capacity(keys.len());
		for (key, &index) in keys.iter().zip(&sorted_columns) {
			cells.push((key.clone(), cell(row.get_ref(index).map_err(storage)?)));
		}
		write_indent(out, 4).map_err(write_error)?;
		write_compact(out, &Value::Object(cells)).map_err(write_error)?;
	}
	if first {
		out.write_all(b"[]\n").map_err(write_error)?;
	} else {
		out.write_all(b"\n").map_err(write_error)?;
		write_indent(out, 3).map_err(write_error)?;
		out.write_all(b"]\n").map_err(write_error)?;
	}
	write_indent(out, 2).map_err(write_error)?;
	out.write_all(b"}").map_err(write_error)
}

pub(crate) fn dump_database(
	conn: &Connection,
	metadata: &[u8],
	out: impl Write,
) -> Result<(), CoreError> {
	let mut out = BufWriter::new(out);
	let metadata = normalize_metadata(metadata)?;
	let schema = schema(conn)?;
	let tables = table_names(conn)?;

	let header = |out: &mut BufWriter<_>, key: &str| -> io::Result<()> {
		write_indent(out, 1)?;
		write_string(out, &Str::from(key))?;
		out.write_all(b": ")
	};
	(|| -> io::Result<()> {
		out.write_all(b"{\n")?;
		header(&mut out, "format")?;
		write_string(&mut out, &Str::from(DUMP_FORMAT))?;
		out.write_all(b",\n")?;
		header(&mut out, "metadata")?;
		write_pretty(&mut out, &metadata, 1)?;
		out.write_all(b",\n")?;
		header(&mut out, "schema")?;
		write_pretty(&mut out, &schema, 1)?;
		out.write_all(b",\n")?;
		header(&mut out, "tables")?;
		out.write_all(if tables.is_empty() { b"{}" } else { b"{\n" })
	})()
	.map_err(write_error)?;

	for (index, table) in tables.iter().enumerate() {
		write_indent(&mut out, 2).map_err(write_error)?;
		write_string(&mut out, &Str::Utf8(table.clone())).map_err(write_error)?;
		out.write_all(b": ").map_err(write_error)?;
		write_table(conn, table, &mut out)?;
		out.write_all(if index + 1 < tables.len() {
			b",\n"
		} else {
			b"\n"
		})
		.map_err(write_error)?;
	}

	(|| -> io::Result<()> {
		if !tables.is_empty() {
			write_indent(&mut out, 1)?;
			out.write_all(b"}")?;
		}
		out.write_all(b",\n")?;
		header(&mut out, "version")?;
		write!(out, "{DUMP_VERSION}")?;
		out.write_all(b"\n}\n")?;
		out.flush()
	})()
	.map_err(write_error)
}
