//! The source zip and its folder stay exactly as they were, whether an import
//! succeeds or fails after it has started writing the copy (R-IO-8).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use common::{Scratch, fixture, pack, unpack};
use rusqlite::Connection;
use veritas_interop::{canonical_dump_to_vec, import_actual_zip};

type FolderState = Vec<(String, Vec<u8>, SystemTime, u32)>;

fn folder_state(dir: &Path) -> FolderState {
	let mut state: FolderState = std::fs::read_dir(dir)
		.unwrap()
		.map(|entry| {
			let entry = entry.unwrap();
			let metadata = entry.metadata().unwrap();
			(
				entry.file_name().into_string().unwrap(),
				std::fs::read(entry.path()).unwrap(),
				metadata.modified().unwrap(),
				metadata.permissions().mode(),
			)
		})
		.collect();
	state.sort();
	state
}

fn set_mode(path: &Path, mode: u32) {
	std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// Makes a zip and its folder read-only until dropped, so a failing case
/// still lets the scratch folder be removed.
struct ReadOnlySource {
	dir: PathBuf,
	zip: PathBuf,
}

impl ReadOnlySource {
	fn new(dir: PathBuf, from: &Path) -> ReadOnlySource {
		std::fs::create_dir(&dir).unwrap();
		let zip = dir.join("My Budget.zip");
		std::fs::copy(from, &zip).unwrap();
		set_mode(&zip, 0o444);
		set_mode(&dir, 0o555);
		ReadOnlySource { dir, zip }
	}
}

impl Drop for ReadOnlySource {
	fn drop(&mut self) {
		set_mode(&self.dir, 0o755);
		set_mode(&self.zip, 0o644);
	}
}

/// Rewrites bytes in the middle of the db entry's stored data, so the zip
/// still opens and the damage only shows once extraction is under way.
fn damage_db_entry(zip_path: &Path) {
	let (start, length) = {
		let mut archive = zip::ZipArchive::new(std::fs::File::open(zip_path).unwrap()).unwrap();
		let entry = archive.by_name("db.sqlite").unwrap();
		(
			entry.header_start() as usize,
			entry.compressed_size() as usize,
		)
	};
	let mut bytes = std::fs::read(zip_path).unwrap();
	let name_length = u16::from_le_bytes([bytes[start + 26], bytes[start + 27]]) as usize;
	let extra_length = u16::from_le_bytes([bytes[start + 28], bytes[start + 29]]) as usize;
	let data = start + 30 + name_length + extra_length;
	for byte in &mut bytes[data + length / 2..data + length / 2 + 64] {
		*byte ^= 0xa5;
	}
	std::fs::write(zip_path, bytes).unwrap();
}

fn stored_zip(db: &[u8], metadata: &[u8], path: &Path) {
	let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
	let options =
		zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
	for (name, bytes) in [("db.sqlite", db), ("metadata.json", metadata)] {
		zip.start_file(name, options).unwrap();
		std::io::Write::write_all(&mut zip, bytes).unwrap();
	}
	zip.finish().unwrap();
}

fn edited_zip(scratch: &Scratch, name: &str, sql: &str) -> PathBuf {
	let dir = scratch.join(&format!("edit-{name}"));
	std::fs::create_dir(&dir).unwrap();
	let (db_path, metadata) = unpack(&fixture("small"), &dir);
	Connection::open(&db_path)
		.unwrap()
		.execute_batch(sql)
		.unwrap();
	let zip = dir.join("budget.zip");
	pack(&db_path, &metadata, &zip);
	zip
}

#[test]
fn a_read_only_source_survives_every_import_outcome_unchanged_r_io_8() {
	let scratch = Scratch::new("source-untouched");
	let (db_path, metadata) = unpack(&fixture("all-features"), scratch.path());
	let db = std::fs::read(&db_path).unwrap();

	let stored_damaged = scratch.join("stored-damaged.zip");
	stored_zip(&db, &metadata, &stored_damaged);
	damage_db_entry(&stored_damaged);
	let deflated_damaged = scratch.join("deflated-damaged.zip");
	pack(&db_path, &metadata, &deflated_damaged);
	damage_db_entry(&deflated_damaged);

	// `None` means the import succeeds; otherwise part of the error it gives.
	let cases: Vec<(&str, PathBuf, Option<&str>)> = vec![
		("a good export", fixture("all-features"), None),
		(
			"a newer Actual's file",
			edited_zip(
				&scratch,
				"newer",
				"INSERT INTO __migrations__ (id) VALUES (1795000000000);",
			),
			None,
		),
		(
			"a stored db that fails its checksum at the end",
			stored_damaged,
			Some("damaged"),
		),
		(
			"a deflated db damaged mid-stream",
			deflated_damaged,
			Some("damaged"),
		),
		(
			"a history Actual refuses, found after the copy",
			edited_zip(
				&scratch,
				"refused",
				"INSERT INTO __migrations__ (id) VALUES (1600000000000);",
			),
			Some("migration history is inconsistent"),
		),
		(
			"an older Actual's file, found after the copy",
			edited_zip(
				&scratch,
				"older",
				"DELETE FROM __migrations__ WHERE id = 1788468782000; DROP TABLE messages_pending;",
			),
			Some("Open it in Actual 26.11.0 or newer"),
		),
	];

	for (index, (case, zip, failure)) in cases.iter().enumerate() {
		let source = ReadOnlySource::new(scratch.join(&format!("source-{index}")), zip);
		let before = folder_state(&source.dir);

		let destination = scratch.join(&format!("budgets-{index}"));
		std::fs::create_dir(&destination).unwrap();
		let target = destination.join("budget");
		let result = import_actual_zip(&source.zip, &target);
		if let Some(failure) = failure {
			let message = result.expect_err(case).to_string();
			assert!(message.contains(failure), "{case}: {message}");
			assert!(
				std::fs::read_dir(&destination).unwrap().next().is_none(),
				"{case} left something in the destination"
			);
		} else {
			assert!(result.is_ok(), "{case}: {result:?}");
			canonical_dump_to_vec(&source.zip).unwrap();
		}

		assert_eq!(folder_state(&source.dir), before, "{case}");
	}
}
