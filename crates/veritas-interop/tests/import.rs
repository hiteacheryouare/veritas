//! Importing Actual export zips: the layouts Actual accepts (R-IO-1), the
//! source never changing (R-IO-8) and the version guard (R-IO-7).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::Path;

use common::{Scratch, fixture, pack, unpack, write_zip};
use rusqlite::Connection;
use veritas_domain::{CoreError, ReadOnlyReason};
use veritas_interop::{Access, import_actual_zip};

type Entry<'a> = (&'a str, &'a [u8]);

fn read_entry(zip_path: &Path, name: &str) -> Vec<u8> {
	let mut archive = zip::ZipArchive::new(std::fs::File::open(zip_path).unwrap()).unwrap();
	let mut bytes = Vec::new();
	std::io::copy(&mut archive.by_name(name).unwrap(), &mut bytes).unwrap();
	bytes
}

#[test]
fn a_root_layout_zip_is_copied_and_the_source_is_untouched_r_io_1_r_io_8() {
	let scratch = Scratch::new("import-root");
	let source = scratch.join("small.zip");
	std::fs::copy(fixture("small"), &source).unwrap();
	let before = std::fs::read(&source).unwrap();
	let modified = std::fs::metadata(&source).unwrap().modified().unwrap();

	let budget = import_actual_zip(&source, &scratch.join("budget")).unwrap();

	assert_eq!(budget.access, Access::ReadWrite);
	assert_eq!(
		std::fs::read(&budget.db_path).unwrap(),
		read_entry(&source, "db.sqlite")
	);
	assert_eq!(
		std::fs::read(&budget.metadata_path).unwrap(),
		read_entry(&source, "metadata.json")
	);
	assert_eq!(std::fs::read(&source).unwrap(), before);
	assert_eq!(
		std::fs::metadata(&source).unwrap().modified().unwrap(),
		modified
	);
	let mut names: Vec<String> = std::fs::read_dir(&budget.dir)
		.unwrap()
		.map(|entry| entry.unwrap().file_name().into_string().unwrap())
		.collect();
	names.sort();
	assert_eq!(names, ["db.sqlite", "metadata.json"]);
}

#[test]
fn one_shared_folder_is_accepted_and_the_root_wins_over_it_r_io_1() {
	let scratch = Scratch::new("import-folder");
	let (db_path, metadata) = unpack(&fixture("small"), scratch.path());
	let db = std::fs::read(&db_path).unwrap();

	let nested = scratch.join("nested.zip");
	write_zip(
		&nested,
		&[
			("My Budget/", b""),
			("My Budget/db.sqlite", &db),
			("My Budget/metadata.json", &metadata),
			("My Budget/notes.txt", b"ignored"),
		],
	);
	let budget = import_actual_zip(&nested, &scratch.join("from-folder")).unwrap();
	assert_eq!(std::fs::read(&budget.db_path).unwrap(), db);

	let both = scratch.join("both.zip");
	write_zip(
		&both,
		&[
			("other/db.sqlite", b"not the one"),
			("other/metadata.json", b"{\"budgetName\":\"other\"}"),
			("db.sqlite", &db),
			("metadata.json", &metadata),
		],
	);
	let budget = import_actual_zip(&both, &scratch.join("from-root")).unwrap();
	assert_eq!(std::fs::read(&budget.metadata_path).unwrap(), metadata);
}

#[test]
fn stored_entries_import_like_deflated_ones() {
	let scratch = Scratch::new("import-stored");
	let (db_path, metadata) = unpack(&fixture("small"), scratch.path());
	let zip_path = scratch.join("stored.zip");
	let mut zip = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
	let options =
		zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
	for (name, bytes) in [
		("db.sqlite", std::fs::read(&db_path).unwrap()),
		("metadata.json", metadata),
	] {
		zip.start_file(name, options).unwrap();
		std::io::Write::write_all(&mut zip, &bytes).unwrap();
	}
	zip.finish().unwrap();
	assert!(import_actual_zip(&zip_path, &scratch.join("budget")).is_ok());
}

#[test]
fn zips_actual_would_refuse_are_not_budgets_and_leave_nothing_behind_r_io_1() {
	let scratch = Scratch::new("import-bad");
	let (db_path, metadata) = unpack(&fixture("small"), scratch.path());
	let db = std::fs::read(&db_path).unwrap();
	let not_sqlite = vec![0x5a; 8192];

	let cases: Vec<(&str, Vec<Entry>)> = vec![
		("empty zip", vec![]),
		("only the db", vec![("db.sqlite", &db)]),
		("only metadata", vec![("metadata.json", &metadata)]),
		(
			"files in different folders",
			vec![("a/db.sqlite", &db), ("b/metadata.json", &metadata)],
		),
		(
			"two complete folders",
			vec![
				("a/db.sqlite", &db),
				("a/metadata.json", &metadata),
				("b/db.sqlite", &db),
				("b/metadata.json", &metadata),
			],
		),
		(
			"metadata that isn't JSON",
			vec![("db.sqlite", &db), ("metadata.json", b"{budgetName: 1}")],
		),
		(
			"a path that climbs out",
			vec![
				("db.sqlite", &db),
				("metadata.json", &metadata),
				("../evil", b"x"),
			],
		),
		(
			"an absolute path",
			vec![
				("db.sqlite", &db),
				("metadata.json", &metadata),
				("/etc/evil", b"x"),
			],
		),
		(
			"a backslash",
			vec![
				("db.sqlite", &db),
				("metadata.json", &metadata),
				("a\\b", b"x"),
			],
		),
		(
			"names equal ignoring case",
			vec![
				("db.sqlite", &db),
				("metadata.json", &metadata),
				("DB.SQLITE", b"x"),
			],
		),
		(
			"a db that isn't SQLite",
			vec![("db.sqlite", &not_sqlite), ("metadata.json", &metadata)],
		),
	];
	for (case, entries) in cases {
		let zip_path = scratch.join("case.zip");
		write_zip(&zip_path, &entries);
		let target = scratch.join("budget");
		let result = import_actual_zip(&zip_path, &target);
		assert!(
			matches!(result, Err(CoreError::NotAnActualBudget(_))),
			"{case}: {result:?}"
		);
		assert!(!target.exists(), "{case} left a folder behind");
	}

	let garbage = scratch.join("garbage.zip");
	std::fs::write(&garbage, b"PK\x03\x04 definitely not a zip").unwrap();
	assert!(matches!(
		import_actual_zip(&garbage, &scratch.join("budget")),
		Err(CoreError::NotAnActualBudget(_))
	));
	let missing = scratch.join("missing.zip");
	assert!(matches!(
		import_actual_zip(&missing, &scratch.join("budget")),
		Err(CoreError::Io(_))
	));
}

#[test]
fn a_damaged_database_fails_the_integrity_check_and_is_not_adopted_r_rel_2() {
	let scratch = Scratch::new("import-damaged");
	let (db_path, metadata) = unpack(&fixture("all-features"), scratch.path());
	let conn = Connection::open(&db_path).unwrap();
	let root: i64 = conn
		.query_row(
			"SELECT rootpage FROM sqlite_master WHERE name = 'trans_date'",
			[],
			|row| row.get(0),
		)
		.unwrap();
	let page_size: i64 = conn
		.query_row("PRAGMA page_size", [], |row| row.get(0))
		.unwrap();
	drop(conn);
	let mut bytes = std::fs::read(&db_path).unwrap();
	let start = ((root - 1) * page_size) as usize;
	// Keep the page header so SQLite still reads it as an index page, but
	// scramble its cells: the index no longer matches its table.
	for byte in &mut bytes[start + 100..start + page_size as usize - 8] {
		*byte = byte.wrapping_add(0x55);
	}
	std::fs::write(&db_path, bytes).unwrap();
	let zip_path = scratch.join("damaged.zip");
	pack(&db_path, &metadata, &zip_path);

	let target = scratch.join("budget");
	let result = import_actual_zip(&zip_path, &target);
	assert!(
		matches!(result, Err(CoreError::AuditFailed { violations }) if violations > 0),
		"{result:?}"
	);
	assert!(!target.exists());
}

fn with_migrations(edit: &str, label: &str) -> (Scratch, std::path::PathBuf) {
	let scratch = Scratch::new(label);
	let (db_path, metadata) = unpack(&fixture("small"), scratch.path());
	Connection::open(&db_path)
		.unwrap()
		.execute_batch(edit)
		.unwrap();
	let zip_path = scratch.join("edited.zip");
	pack(&db_path, &metadata, &zip_path);
	(scratch, zip_path)
}

#[test]
fn a_file_from_a_newer_actual_imports_read_only_r_io_7() {
	let (scratch, zip_path) = with_migrations(
		"INSERT INTO __migrations__ (id) VALUES (1795000000000);",
		"import-newer",
	);
	let budget = import_actual_zip(&zip_path, &scratch.join("budget")).unwrap();
	assert_eq!(
		budget.access,
		Access::ReadOnly(ReadOnlyReason::NewerActualVersion)
	);
}

#[test]
fn a_file_from_an_older_actual_must_be_upgraded_in_actual_first_r_io_7() {
	let (scratch, zip_path) = with_migrations(
		"DELETE FROM __migrations__ WHERE id > 1780606215001;",
		"import-older",
	);
	let target = scratch.join("budget");
	let result = import_actual_zip(&zip_path, &target);
	assert!(
		matches!(result, Err(CoreError::UpgradeInActualFirst { ref required }) if required == "26.11.0"),
		"{result:?}"
	);
	assert!(!target.exists());
}

#[test]
fn a_file_without_migration_history_is_not_a_budget() {
	let (scratch, zip_path) = with_migrations("DROP TABLE __migrations__;", "import-no-migrations");
	assert!(matches!(
		import_actual_zip(&zip_path, &scratch.join("budget")),
		Err(CoreError::NotAnActualBudget(_))
	));
}

#[test]
fn importing_never_overwrites_an_existing_folder() {
	let scratch = Scratch::new("import-existing");
	let target = scratch.join("budget");
	std::fs::create_dir(&target).unwrap();
	std::fs::write(target.join("keep.txt"), b"mine").unwrap();
	assert!(matches!(
		import_actual_zip(&fixture("small"), &target),
		Err(CoreError::Io(_))
	));
	assert_eq!(std::fs::read(target.join("keep.txt")).unwrap(), b"mine");
}
