//! PLAN §10: Ryan's own Actual export, imported and immediately exported, gives
//! a canonical dump identical to the original, both Veritas's and Actual's.
//!
//! It runs only when `VERITAS_PRIVATE_EXPORT` points at the zip, which lives
//! outside the repo (D24). Real money: every message here is a count or a
//! table name, never a row, an amount or an error text that could quote one.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use common::{Scratch, oracle_dir};
use veritas_interop::{
	ExportOutcome, ImportOutcome, canonical_dump_of_actual_tables, export_actual_zip,
	import_actual_zip,
};

/// Unwraps without printing the error, which may quote the budget's path, ids or rows.
fn quiet<T, E>(result: Result<T, E>, what: &str) -> T {
	match result {
		Ok(value) => value,
		Err(_) => panic!("{what} failed; details withheld because they may quote budget data"),
	}
}

fn private_export() -> Option<std::path::PathBuf> {
	let path = std::env::var_os("VERITAS_PRIVATE_EXPORT")?;
	let path = std::path::PathBuf::from(path);
	assert!(
		path.is_file(),
		"VERITAS_PRIVATE_EXPORT does not name a file"
	);
	Some(path)
}

fn differing_lines(a: &[u8], b: &[u8]) -> usize {
	let left: BTreeSet<&[u8]> = a.split(|&byte| byte == b'\n').collect();
	let right: BTreeSet<&[u8]> = b.split(|&byte| byte == b'\n').collect();
	left.symmetric_difference(&right).count()
}

/// Where two dumps differ, as key paths with a count of differing array
/// elements. Only key names (table and column names) appear, never values.
fn differing_sections(a: &[u8], b: &[u8]) -> Vec<String> {
	let a = without_veritas_tables(quiet(serde_json::from_slice(a), "parsing Actual's dump"));
	let b = without_veritas_tables(quiet(serde_json::from_slice(b), "parsing Actual's dump"));
	let mut out = Vec::new();
	walk("", &a, &b, &mut out);
	out
}

/// contracts/canonical-dump.md: R-IO-3 compares Actual's tables, so the
/// `veritas_*` tables adopting adds (R-IO-6) and their schema rows are left out.
fn without_veritas_tables(mut dump: serde_json::Value) -> serde_json::Value {
	let is_veritas = |name: Option<&str>| name.is_some_and(|name| name.starts_with("veritas_"));
	if let Some(tables) = dump.get_mut("tables").and_then(|t| t.as_object_mut()) {
		tables.retain(|name, _| !is_veritas(Some(name)));
	}
	if let Some(schema) = dump.get_mut("schema").and_then(|s| s.as_array_mut()) {
		schema.retain(|entry| {
			!is_veritas(entry.get("tbl_name").and_then(|v| v.as_str()))
				&& !is_veritas(entry.get("name").and_then(|v| v.as_str()))
		});
	}
	dump
}

fn walk(path: &str, a: &serde_json::Value, b: &serde_json::Value, out: &mut Vec<String>) {
	use serde_json::Value;
	match (a, b) {
		(Value::Object(left), Value::Object(right)) => {
			let keys: BTreeSet<&String> = left.keys().chain(right.keys()).collect();
			for key in keys {
				let child = if path.is_empty() {
					key.clone()
				} else {
					format!("{path}.{key}")
				};
				match (left.get(key), right.get(key)) {
					(Some(x), Some(y)) => walk(&child, x, y, out),
					(Some(_), None) => out.push(format!("{child} (missing after)")),
					(None, Some(_)) => out.push(format!("{child} (added after)")),
					(None, None) => {}
				}
			}
		}
		(Value::Array(left), Value::Array(right)) => {
			let left: BTreeSet<String> = left.iter().map(Value::to_string).collect();
			let right: BTreeSet<String> = right.iter().map(Value::to_string).collect();
			let changed = left.symmetric_difference(&right).count();
			if changed > 0 {
				out.push(format!("{path}: {changed} element(s)"));
			}
		}
		(x, y) if x != y => out.push(format!("{path} (value)")),
		_ => {}
	}
}

fn quiet_oracle_dump(zip: &Path) -> Vec<u8> {
	assert!(
		oracle_dir().join("node_modules").exists(),
		"the oracle isn't installed; run `cargo xtask oracle setup`"
	);
	let output = Command::new("node")
		.current_dir(oracle_dir())
		.args(["src/main.ts", "dump"])
		.arg(zip)
		.arg("--raw-only")
		.output();
	let output = quiet(output, "starting node");
	assert!(
		output.status.success(),
		"Actual could not dump the zip (exit {:?}); stderr withheld because it may quote budget data",
		output.status.code()
	);
	output.stdout
}

#[test]
fn ryans_export_round_trips_unchanged() {
	let Some(source) = private_export() else {
		eprintln!("skipping: VERITAS_PRIVATE_EXPORT is not set");
		return;
	};
	let scratch = Scratch::new("private-round-trip");

	let budget = match import_actual_zip(&source, &scratch.join("budget")) {
		Ok(ImportOutcome::Imported(budget)) => budget,
		Ok(ImportOutcome::Refused(report)) => panic!(
			"import refused: {} audit finding(s)",
			report.violations.len()
		),
		Err(_) => panic!("import failed; error withheld because it may quote budget data"),
	};
	let mut store = quiet(
		veritas_store::Store::open(&budget.db_path, veritas_store::Access::ReadWrite),
		"opening the imported budget",
	);
	quiet(store.adopt(), "adopting the imported budget");
	let exported = scratch.join("exported.zip");
	match export_actual_zip(&store, &budget.metadata_path, &exported) {
		Ok(ExportOutcome::Exported(audit)) => assert!(
			audit.is_clean(),
			"export audit: {} finding(s)",
			audit.violations.len()
		),
		Ok(_) => panic!("export refused"),
		Err(_) => panic!("export failed; error withheld because it may quote budget data"),
	}

	let ours_before = quiet(
		canonical_dump_of_actual_tables(&source),
		"dumping the source",
	);
	let ours_after = quiet(
		canonical_dump_of_actual_tables(&exported),
		"dumping the export",
	);
	let ours = differing_lines(&ours_before, &ours_after);
	assert_eq!(
		ours, 0,
		"Veritas's canonical dump differs on {ours} line(s)"
	);

	let actual_before = quiet_oracle_dump(&source);
	let actual_after = quiet_oracle_dump(&exported);
	let actual = differing_sections(&actual_before, &actual_after);
	assert!(
		actual.is_empty(),
		"Actual's canonical dump differs in: {}",
		actual.join(", ")
	);

	eprintln!(
		"Ryan's export round-trips unchanged: {} dump lines compared in Veritas, {} in Actual",
		ours_before.split(|&byte| byte == b'\n').count(),
		actual_before.split(|&byte| byte == b'\n').count()
	);
}
