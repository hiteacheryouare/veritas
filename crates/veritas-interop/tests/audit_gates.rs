//! The audit as a gate on a working copy that changed after import (R-REL-2):
//! damage refuses the export and leaves the last backup alone, while an
//! inconsistency only reports.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::Path;

use common::{Scratch, fixture, imported};
use rusqlite::Connection;
use veritas_interop::{ExportOutcome, export_actual_zip, import_actual_zip, open_budget};
use veritas_store::Check;

/// Scrambles the cells of an index page and leaves its header, so SQLite
/// still opens the file but `integrity_check` finds the index wrong.
fn damage_index(db_path: &Path) {
	let conn = Connection::open(db_path).unwrap();
	conn.execute_batch("PRAGMA journal_mode = DELETE;").unwrap();
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
	let mut bytes = std::fs::read(db_path).unwrap();
	let start = ((root - 1) * page_size) as usize;
	for byte in &mut bytes[start + 100..start + page_size as usize - 8] {
		*byte = byte.wrapping_add(0x55);
	}
	std::fs::write(db_path, bytes).unwrap();
}

#[test]
fn a_working_copy_damaged_after_import_is_caught_on_reopen_and_not_exported_r_rel_2() {
	let scratch = Scratch::new("gate-damaged");
	let budget = imported(import_actual_zip(
		&fixture("all-features"),
		&scratch.join("budget"),
	));
	{
		let mut store = open_budget(&budget.db_path).unwrap();
		store.adopt().unwrap();
	}
	damage_index(&budget.db_path);

	let store = open_budget(&budget.db_path).unwrap();
	let (loaded, report) = store.open_audit().unwrap();
	assert!(loaded.is_none(), "nothing is loaded from a damaged file");
	assert!(report.is_corrupt(), "{report:?}");

	let backup = scratch.join("backups/budget.zip");
	std::fs::create_dir(scratch.join("backups")).unwrap();
	std::fs::copy(fixture("small"), &backup).unwrap();
	let last_good = std::fs::read(&backup).unwrap();
	let outcome = export_actual_zip(&store, &budget.metadata_path, &backup).unwrap();
	assert!(
		matches!(&outcome, ExportOutcome::Refused(report) if report.is_corrupt()),
		"{outcome:?}"
	);
	assert_eq!(std::fs::read(&backup).unwrap(), last_good);
	let names: Vec<String> = std::fs::read_dir(scratch.join("backups"))
		.unwrap()
		.map(|entry| entry.unwrap().file_name().into_string().unwrap())
		.collect();
	assert_eq!(names, ["budget.zip"]);
}

#[test]
fn an_inconsistent_working_copy_exports_with_its_findings_and_actual_opens_it_r_rel_2() {
	let scratch = Scratch::new("gate-inconsistent");
	let budget = imported(import_actual_zip(
		&fixture("all-features"),
		&scratch.join("budget"),
	));
	assert!(budget.audit.is_clean(), "{:?}", budget.audit);
	let mut store = open_budget(&budget.db_path).unwrap();
	store.adopt().unwrap();
	let leg: String = Connection::open(&budget.db_path)
		.unwrap()
		.query_row(
			"SELECT id FROM transactions WHERE transferred_id IS NOT NULL AND tombstone = 0 \
			 AND isChild = 0 ORDER BY id LIMIT 1",
			[],
			|row| row.get(0),
		)
		.unwrap();
	Connection::open(&budget.db_path)
		.unwrap()
		.execute(
			"UPDATE transactions SET amount = amount + 1 WHERE id = ?1",
			[&leg],
		)
		.unwrap();

	let exported = scratch.join("exported.zip");
	let report = match export_actual_zip(&store, &budget.metadata_path, &exported).unwrap() {
		ExportOutcome::Exported(report) => report,
		refused => panic!("an inconsistency must not block the export: {refused:?}"),
	};
	assert!(!report.is_corrupt());
	assert!(
		report
			.violations
			.iter()
			.any(|violation| violation.check == Check::TransferAmountMismatch),
		"{report:?}"
	);
	let reimported = imported(import_actual_zip(&exported, &scratch.join("again")));
	assert_eq!(
		reimported.audit, report,
		"the export kept the data as it was"
	);
	if let Some(actual) = common::actual_opens(&exported) {
		actual.unwrap();
	}
}
