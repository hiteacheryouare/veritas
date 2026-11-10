//! R-IO-2 and R-IO-3: importing then exporting with no edits gives back the
//! same Actual tables, in a zip shaped like Actual's own exports.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::io::Read;

use common::{Scratch, assert_same_dump, fixture, pack, round_trip, unpack};
use rusqlite::Connection;
use veritas_interop::{canonical_dump_of_actual_tables, canonical_dump_to_vec};

fn assert_round_trips(name: &str) {
	let scratch = Scratch::new(&format!("round-trip-{name}"));
	let exported = round_trip(&fixture(name), &scratch);
	assert_same_dump(
		&canonical_dump_of_actual_tables(&exported).unwrap(),
		&canonical_dump_to_vec(&fixture(name)).unwrap(),
		name,
	);
}

#[test]
fn small_round_trips_r_io_3() {
	assert_round_trips("small");
}

#[test]
fn all_features_round_trips_r_io_3() {
	assert_round_trips("all-features");
}

#[test]
fn perf_100k_round_trips_r_io_3() {
	assert_round_trips("perf-100k");
}

#[test]
fn an_unknown_table_and_column_survive_the_round_trip_r_io_5() {
	let scratch = Scratch::new("round-trip-unknown");
	let (db_path, metadata) = unpack(&fixture("small"), scratch.path());
	Connection::open(&db_path)
		.unwrap()
		.execute_batch(
			"CREATE TABLE future_feature (id TEXT PRIMARY KEY, payload BLOB);
			 INSERT INTO future_feature VALUES ('f1', X'cafe');
			 ALTER TABLE transactions ADD COLUMN future_flag INTEGER DEFAULT 7;",
		)
		.unwrap();
	let source = scratch.join("future.zip");
	pack(&db_path, &metadata, &source);
	let exported = round_trip(&source, &scratch);
	let dump = canonical_dump_of_actual_tables(&exported).unwrap();
	assert_same_dump(
		&dump,
		&canonical_dump_to_vec(&source).unwrap(),
		"unknown table and column",
	);
	assert!(
		String::from_utf8(dump)
			.unwrap()
			.contains("\"future_feature\"")
	);
}

#[test]
fn the_export_is_shaped_like_actuals_r_io_2() {
	let scratch = Scratch::new("export-shape");
	let exported = round_trip(&fixture("all-features"), &scratch);
	let mut archive = zip::ZipArchive::new(std::fs::File::open(&exported).unwrap()).unwrap();
	let names: Vec<String> = archive
		.file_names()
		.map(|name| name.unwrap().into_owned())
		.collect();
	assert_eq!(names, ["db.sqlite", "metadata.json"]);

	let mut metadata = String::new();
	archive
		.by_name("metadata.json")
		.unwrap()
		.read_to_string(&mut metadata)
		.unwrap();
	let original: serde_json::Value =
		serde_json::from_slice(&std::fs::read(scratch.join("budget/metadata.json")).unwrap())
			.unwrap();
	let written: serde_json::Value = serde_json::from_str(&metadata).unwrap();
	assert_eq!(written["resetClock"], serde_json::Value::Bool(true));
	assert_eq!(written["budgetName"], original["budgetName"]);
	assert!(
		!metadata.contains("\": ") && !metadata.contains(", \""),
		"JSON.stringify writes no spaces between tokens: {metadata}"
	);

	let mut db = Vec::new();
	archive
		.by_name("db.sqlite")
		.unwrap()
		.read_to_end(&mut db)
		.unwrap();
	assert_eq!(
		&db[18..20],
		[1, 1],
		"the export uses a rollback journal, not WAL"
	);
	let db_path = scratch.join("check.sqlite");
	std::fs::write(&db_path, db).unwrap();
	let conn = Connection::open(&db_path).unwrap();
	for cache in ["kvcache", "kvcache_key"] {
		let rows: i64 = conn
			.query_row(&format!("SELECT count(*) FROM {cache}"), [], |row| {
				row.get(0)
			})
			.unwrap();
		assert_eq!(rows, 0, "{cache}");
	}
	let version: String = conn
		.query_row(
			"SELECT value FROM veritas_meta WHERE key = 'schema_version'",
			[],
			|row| row.get(0),
		)
		.unwrap();
	assert_eq!(version, "1");

	let leftovers: Vec<_> = std::fs::read_dir(scratch.path())
		.unwrap()
		.map(|entry| entry.unwrap().file_name().into_string().unwrap())
		.filter(|name| name.contains("partial"))
		.collect();
	assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn exporting_replaces_the_old_backup_whole_or_not_at_all_r_rel_4() {
	let scratch = Scratch::new("export-atomic");
	let budget = common::imported(veritas_interop::import_actual_zip(
		&fixture("small"),
		&scratch.join("budget"),
	));
	let store = veritas_store::Store::open(&budget.db_path).unwrap();
	let target = scratch.join("backup.zip");
	std::fs::write(&target, b"last week's backup").unwrap();
	veritas_interop::export_actual_zip(&store, &budget.metadata_path, &target).unwrap();
	assert_ne!(std::fs::read(&target).unwrap(), b"last week's backup");
	assert!(zip::ZipArchive::new(std::fs::File::open(&target).unwrap()).is_ok());

	let missing = scratch.join("no-such-folder/backup.zip");
	assert!(matches!(
		veritas_interop::export_actual_zip(&store, &budget.metadata_path, &missing),
		Err(veritas_domain::CoreError::Io(_))
	));
	assert!(!scratch.join("no-such-folder").exists());
}
