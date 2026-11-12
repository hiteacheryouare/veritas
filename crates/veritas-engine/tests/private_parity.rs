//! R-BUD-1 on Ryan's own budget: Veritas's envelope sheet equals Actual's for
//! every month Actual's API lists.
//!
//! It runs only when `VERITAS_PRIVATE_EXPORT` points at the zip, which lives
//! outside the repo (D24). Real money: a failure reports how many cells differ
//! per kind of number, never an id, a month, an amount or an error text.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod oracle;

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use serde_json::Value;
use veritas_domain::BudgetData;
use veritas_engine::EnvelopeSheet;
use veritas_interop::{Access, ImportOutcome, import_actual_zip};
use veritas_store::Store;

const KINDS: &[&str] = &[
	"toBudget",
	"carryover",
	"received",
	"budgeted",
	"spent",
	"balance",
	"buffered",
	"missing",
	"months",
];

fn quiet_dump(zip: &Path) -> Value {
	assert!(oracle::available(), "the oracle isn't installed");
	let output = Command::new("node")
		.current_dir(oracle::repo_root().join("tools/actual-oracle"))
		.args(["src/main.ts", "dump"])
		.arg(zip)
		.output();
	let Ok(output) = output else {
		panic!("starting node failed; details withheld");
	};
	assert!(
		output.status.success(),
		"Actual could not dump the zip (exit {:?}); stderr withheld because it may quote budget data",
		output.status.code()
	);
	let Ok(dump) = serde_json::from_slice(&output.stdout) else {
		panic!("Actual's dump didn't parse; details withheld");
	};
	dump
}

fn quiet_load(zip: &Path, scratch: &oracle::Scratch) -> BudgetData {
	let Ok(ImportOutcome::Imported(imported)) = import_actual_zip(zip, &scratch.join("budget"))
	else {
		panic!("import failed or was refused; details withheld");
	};
	let Ok(mut store) = Store::open(&imported.db_path, Access::ReadWrite) else {
		panic!("the imported budget would not open; details withheld");
	};
	assert!(store.adopt().is_ok(), "adopting failed; details withheld");
	let Ok(loaded) = store.load() else {
		panic!("loading failed; details withheld");
	};
	loaded.data
}

fn kind_of(difference: &str) -> &'static str {
	KINDS
		.iter()
		.copied()
		.find(|kind| difference.contains(kind))
		.unwrap_or("other")
}

#[test]
fn ryans_budget_matches_actual_month_by_month() {
	let Some(source) = std::env::var_os("VERITAS_PRIVATE_EXPORT") else {
		eprintln!("skipping: VERITAS_PRIVATE_EXPORT is not set");
		return;
	};
	let source = Path::new(&source);
	assert!(
		source.is_file(),
		"VERITAS_PRIVATE_EXPORT does not name a file"
	);

	let scratch = oracle::Scratch::new("private-parity");
	let dump = quiet_dump(source);
	let budget = quiet_load(source, &scratch);
	let Ok(sheet) = EnvelopeSheet::compute(&budget, oracle::today(&dump)) else {
		panic!("the sheet could not be computed; details withheld");
	};
	let months = &dump["computed"]["months"];
	let differences = oracle::compare_months(&sheet, &budget, months);

	let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
	for difference in &differences {
		*by_kind.entry(kind_of(difference)).or_default() += 1;
	}
	assert!(
		differences.is_empty(),
		"{} difference(s) with Actual: {by_kind:?}",
		differences.len()
	);
	let month_count = months
		.as_array()
		.map(Vec::len)
		.or_else(|| months.as_object().map(serde_json::Map::len))
		.unwrap_or(0);
	assert!(
		month_count > 0,
		"Actual reported no months, so nothing was compared"
	);
	eprintln!("Ryan's budget matches Actual in all {month_count} months");
}
