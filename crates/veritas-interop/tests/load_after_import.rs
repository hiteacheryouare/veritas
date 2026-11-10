//! The read path end to end: import a zip, open the working copy, adopt it
//! and load it. Prints the perf-100k timings for R-PERF-1 and R-PERF-5.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::time::Instant;

use common::{Scratch, fixture};
use veritas_interop::{Access, import_actual_zip};
use veritas_store::Store;

#[test]
fn perf_100k_imports_and_loads_every_transaction_r_perf_5() {
	let scratch = Scratch::new("perf-load");
	let started = Instant::now();
	let budget = import_actual_zip(&fixture("perf-100k"), &scratch.join("budget")).unwrap();
	let imported = started.elapsed();
	assert_eq!(budget.access, Access::ReadWrite);

	let started = Instant::now();
	let mut store = Store::open(&budget.db_path).unwrap();
	store.adopt().unwrap();
	let opened = started.elapsed();
	let started = Instant::now();
	let loaded = store.load().unwrap();
	let load = started.elapsed();

	assert_eq!(loaded.data.transactions.len(), 105_186);
	assert_eq!(loaded.data.categories.len(), 502);
	assert!(loaded.report.skipped.is_empty());
	eprintln!(
		"perf-100k: import (copy + integrity_check + manifest) {imported:?}, open + adopt {opened:?}, load {load:?}"
	);
}
