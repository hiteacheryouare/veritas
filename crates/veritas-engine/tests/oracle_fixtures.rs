//! R-BUD-1, PLAN §6.4 family 2: every synthetic fixture, loaded through
//! veritas-store, gives the same sheet as Actual's own engine for every month
//! its API lists, with today pinned to the oracle's clock.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod oracle;

use veritas_engine::EnvelopeSheet;

fn assert_parity(name: &str) {
	if !oracle::available() {
		return;
	}
	let zip = oracle::fixture(name);
	let dump = oracle::dump(&zip);
	let budget = oracle::load(&zip);
	let sheet = EnvelopeSheet::compute(&budget, oracle::today(&dump)).unwrap();
	let differences = oracle::compare_months(&sheet, &budget, &dump["computed"]["months"]);
	assert!(
		differences.is_empty(),
		"{name}: {} differences, first ones:\n{}",
		differences.len(),
		differences
			.iter()
			.take(30)
			.cloned()
			.collect::<Vec<_>>()
			.join("\n")
	);
}

#[test]
fn small_matches_actual_month_by_month() {
	assert_parity("small");
}

#[test]
fn all_features_matches_actual_month_by_month() {
	assert_parity("all-features");
}

#[test]
fn perf_100k_matches_actual_month_by_month() {
	assert_parity("perf-100k");
}

#[test]
fn the_comparison_notices_a_one_cent_difference() {
	if !oracle::available() {
		return;
	}
	let zip = oracle::fixture("small");
	let dump = oracle::dump(&zip);
	let mut budget = oracle::load(&zip);
	let cell = budget.budget_cells.values_mut().next().unwrap();
	cell.budgeted = veritas_domain::Money::from_minor(cell.budgeted.minor() + 1);
	let sheet = EnvelopeSheet::compute(&budget, oracle::today(&dump)).unwrap();
	let differences = oracle::compare_months(&sheet, &budget, &dump["computed"]["months"]);
	assert!(
		differences.iter().any(|d| d.contains("toBudget")),
		"{differences:?}"
	);
}
