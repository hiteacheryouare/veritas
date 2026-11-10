//! Every import verdict against what `@actual-app/api` does with the same
//! zip (R-IO-1, R-IO-7). A refusal must be a file Actual also refuses; an
//! "upgrade in Actual first" must be a file Actual can actually open.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::{Path, PathBuf};

use common::{Scratch, actual_opens, actual_reexport, fixture, unpack, write_zip};
use rusqlite::Connection;
use veritas_domain::{CoreError, ReadOnlyReason};
use veritas_interop::{Access, import_actual_zip};

#[derive(Debug)]
enum Verdict {
	ReadWrite,
	ReadOnly,
	UpgradeInActualFirst,
	/// Carries the error Actual itself reports for the same zip.
	Refused(&'static str),
}

fn edited_db(scratch: &Scratch, name: &str, sql: &str) -> Vec<u8> {
	let dir = scratch.join(&format!("edit-{name}"));
	std::fs::create_dir_all(&dir).unwrap();
	let (db_path, _) = unpack(&fixture("small"), &dir);
	Connection::open(&db_path)
		.unwrap()
		.execute_batch(sql)
		.unwrap();
	std::fs::read(&db_path).unwrap()
}

fn zip_of(scratch: &Scratch, name: &str, entries: &[(&str, &[u8])]) -> PathBuf {
	let path = scratch.join(&format!("{name}.zip"));
	write_zip(&path, entries);
	path
}

fn veritas_verdict(zip: &Path, scratch: &Scratch, name: &str) -> Result<Verdict, CoreError> {
	let target = scratch.join(&format!("imported-{name}"));
	let verdict = match import_actual_zip(zip, &target) {
		Ok(budget) => match budget.access {
			Access::ReadWrite => Verdict::ReadWrite,
			Access::ReadOnly(ReadOnlyReason::NewerActualVersion) => Verdict::ReadOnly,
			Access::ReadOnly(other) => panic!("{name}: unexpected read-only reason {other:?}"),
		},
		Err(CoreError::UpgradeInActualFirst { required }) => {
			assert_eq!(required, "26.11.0", "{name}");
			Verdict::UpgradeInActualFirst
		}
		Err(CoreError::NotAnActualBudget(_)) => Verdict::Refused(""),
		Err(other) => return Err(other),
	};
	Ok(verdict)
}

#[test]
fn veritas_and_actual_agree_on_which_zips_are_budgets_r_io_1_r_io_7() {
	let scratch = Scratch::new("agree");
	let (db_path, metadata) = unpack(&fixture("small"), scratch.path());
	let db = std::fs::read(&db_path).unwrap();

	// A newer Actual only ever adds: a table, a nullable column and its id.
	let newer = edited_db(
		&scratch,
		"newer",
		"CREATE TABLE future_feature (id TEXT PRIMARY KEY, payload TEXT);
		 ALTER TABLE accounts ADD COLUMN future_flag INTEGER DEFAULT NULL;
		 INSERT INTO __migrations__ (id) VALUES (1795000000000);",
	);
	// Undo the last two 26.11.0 migrations, effects included, so the file is
	// shaped like one an earlier Actual wrote.
	let older = edited_db(
		&scratch,
		"older",
		"DROP TABLE messages_pending;
		 DROP TABLE account_groups;
		 ALTER TABLE accounts DROP COLUMN account_group_id;
		 DELETE FROM __migrations__ WHERE id IN (1787013118115, 1788468782000);",
	);
	let unknown_pre_cutoff = edited_db(
		&scratch,
		"unknown-pre-cutoff",
		"INSERT INTO __migrations__ (id) VALUES (1600000000000);",
	);
	let newer_with_a_gap = edited_db(
		&scratch,
		"newer-with-gap",
		"INSERT INTO __migrations__ (id) VALUES (1795000000000);
		 DELETE FROM __migrations__ WHERE id = 1783004650757;",
	);

	let mut cases: Vec<(&str, PathBuf, Verdict)> = vec![
		("Actual's own export", fixture("small"), Verdict::ReadWrite),
		(
			"one shared folder",
			zip_of(
				&scratch,
				"folder",
				&[
					("Budget 2026/db.sqlite", &db),
					("Budget 2026/metadata.json", &metadata),
				],
			),
			Verdict::ReadWrite,
		),
		(
			"junk next to the budget files",
			zip_of(
				&scratch,
				"junk",
				&[
					("README.txt", b"exported from Actual"),
					("db.sqlite", &db),
					("backups/db.sqlite", b"an old copy, no metadata beside it"),
					("metadata.json", &metadata),
					("notes/", b""),
				],
			),
			Verdict::ReadWrite,
		),
		(
			"a newer Actual's additive migration",
			zip_of(
				&scratch,
				"newer",
				&[("db.sqlite", &newer), ("metadata.json", &metadata)],
			),
			Verdict::ReadOnly,
		),
		(
			"an earlier Actual's file",
			zip_of(
				&scratch,
				"older",
				&[("db.sqlite", &older), ("metadata.json", &metadata)],
			),
			Verdict::UpgradeInActualFirst,
		),
		(
			"an unknown id before the additive cutoff",
			zip_of(
				&scratch,
				"unknown-pre-cutoff",
				&[
					("db.sqlite", &unknown_pre_cutoff),
					("metadata.json", &metadata),
				],
			),
			Verdict::Refused("out-of-sync-migrations"),
		),
		(
			"a newer id while a known one is missing",
			zip_of(
				&scratch,
				"newer-with-gap",
				&[
					("db.sqlite", &newer_with_a_gap),
					("metadata.json", &metadata),
				],
			),
			Verdict::Refused("out-of-sync-migrations"),
		),
		(
			"a junk entry repeating a name in another case",
			zip_of(
				&scratch,
				"case-duplicate",
				&[
					("db.sqlite", &db),
					("metadata.json", &metadata),
					("Metadata.JSON", b"{}"),
				],
			),
			Verdict::Refused("zip-too-large"),
		),
		(
			"a junk entry that climbs out of the folder",
			zip_of(
				&scratch,
				"traversal",
				&[
					("db.sqlite", &db),
					("metadata.json", &metadata),
					("extras/../../evil.txt", b"x"),
				],
			),
			Verdict::Refused("zip-too-large"),
		),
		(
			"a junk entry with a Windows path",
			zip_of(
				&scratch,
				"backslash",
				&[
					("db.sqlite", &db),
					("metadata.json", &metadata),
					("extras\\notes.txt", b"x"),
				],
			),
			Verdict::Refused("zip-too-large"),
		),
	];
	if let Some(reexported) = actual_reexport(&fixture("all-features"), &scratch) {
		cases.push((
			"a budget Actual imported and exported again",
			reexported,
			Verdict::ReadWrite,
		));
	}

	for (name, zip, expected) in &cases {
		let ours = veritas_verdict(zip, &scratch, name).unwrap();
		match (expected, &ours) {
			(Verdict::Refused(_), Verdict::Refused(_))
			| (Verdict::ReadWrite, Verdict::ReadWrite)
			| (Verdict::ReadOnly, Verdict::ReadOnly)
			| (Verdict::UpgradeInActualFirst, Verdict::UpgradeInActualFirst) => {}
			_ => panic!("{name}: Veritas said {ours:?}, expected {expected:?}"),
		}
		let Some(actual) = actual_opens(zip) else {
			continue;
		};
		match (expected, actual) {
			(Verdict::Refused(reason), Err(stderr)) => assert!(
				stderr.contains(reason),
				"{name}: Actual refused, but not with {reason}: {stderr}"
			),
			(Verdict::Refused(reason), Ok(())) => {
				panic!("{name}: Veritas refused it, but Actual opens it (expected {reason})")
			}
			(_, Err(stderr)) => panic!("{name}: Veritas accepts it, but Actual refuses: {stderr}"),
			(_, Ok(())) => {}
		}
	}
}

/// Finder's Compress (ditto) wraps the folder and adds `__MACOSX/._*`
/// AppleDouble entries for the quarantine flag every browser download carries.
#[cfg(target_os = "macos")]
#[test]
fn a_folder_compressed_in_finder_imports_like_the_export_inside_it_r_io_1() {
	use std::process::Command;

	let scratch = Scratch::new("finder");
	let folder = scratch.join("My Budget");
	std::fs::create_dir(&folder).unwrap();
	let (db_path, metadata) = unpack(&fixture("all-features"), &folder);
	std::fs::write(folder.join("metadata.json"), &metadata).unwrap();
	std::fs::write(folder.join(".DS_Store"), b"\0\0\0\x01Bud1").unwrap();
	for file in ["db.sqlite", "metadata.json", ".DS_Store"] {
		let status = Command::new("xattr")
			.args(["-w", "com.apple.quarantine", "0083;6730a2b1;Safari;"])
			.arg(folder.join(file))
			.status()
			.unwrap();
		assert!(status.success());
	}
	let zip = scratch.join("My Budget.zip");
	let status = Command::new("ditto")
		.args(["-c", "-k", "--sequesterRsrc", "--keepParent"])
		.arg(&folder)
		.arg(&zip)
		.status()
		.unwrap();
	assert!(status.success());

	let archive = zip::ZipArchive::new(std::fs::File::open(&zip).unwrap()).unwrap();
	let names: Vec<String> = archive
		.file_names()
		.map(|name| name.unwrap().into_owned())
		.collect();
	assert!(
		names
			.iter()
			.any(|name| name == "__MACOSX/My Budget/._db.sqlite"),
		"{names:?}"
	);
	assert!(
		names.iter().any(|name| name == "My Budget/.DS_Store"),
		"{names:?}"
	);

	let budget = import_actual_zip(&zip, &scratch.join("imported")).unwrap();
	assert_eq!(budget.access, Access::ReadWrite);
	assert_eq!(
		std::fs::read(&budget.db_path).unwrap(),
		std::fs::read(&db_path).unwrap()
	);
	assert_eq!(std::fs::read(&budget.metadata_path).unwrap(), metadata);
	if let Some(actual) = actual_opens(&zip) {
		actual.unwrap();
	}
}
