//! R-REP-7: on every synthetic fixture, imported and loaded the way the app
//! opens a budget, the Balance Sheet balances, Owner's Equity closes and the
//! Cash Flow Statement ties to the cent in every month that has rows.

#![allow(clippy::unwrap_used, clippy::panic)]

mod support;

use std::path::{Path, PathBuf};

use support::*;
use veritas_domain::BudgetData;
use veritas_interop::{ImportOutcome, import_actual_zip};
use veritas_reports::{Period, balance_sheet, cash_flow, classify, months, owners_equity};
use veritas_store::{Access, Store};

struct Scratch(PathBuf);

impl Drop for Scratch {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.0);
	}
}

fn load_fixture(name: &str) -> BudgetData {
	let zip = Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("../../fixtures/synthetic")
		.join(format!("{name}.zip"));
	let scratch = Scratch(
		Path::new(env!("CARGO_TARGET_TMPDIR"))
			.join(format!("statements-{name}-{}", std::process::id())),
	);
	let _ = std::fs::remove_dir_all(&scratch.0);
	std::fs::create_dir_all(&scratch.0).unwrap();
	let ImportOutcome::Imported(budget) =
		import_actual_zip(&zip, &scratch.0.join("budget")).unwrap()
	else {
		panic!("{name} imports cleanly");
	};
	let mut store = Store::open(&budget.db_path, Access::ReadWrite).unwrap();
	store.adopt().unwrap();
	store.load().unwrap().data
}

fn assert_statements_close_every_month(name: &str) {
	let budget = load_fixture(name);
	let classification = classify(&budget);
	let raw = RawLedger::new(&budget);
	let rows = classification.rows();
	let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
		panic!("{name} has no rows to report on");
	};
	let range = months(first.date.year_month(), last.date.year_month()).unwrap();
	assert!(range.len() > 1, "{name} spans more than one month");

	let whole = Period::new(first.date, last.date).unwrap();
	for period in range.into_iter().map(Period::month).chain([whole]) {
		let label = format!("{name} {}..{}", period.first(), period.last());
		let sheet = balance_sheet(&budget, &classification, period.last()).unwrap();
		let closing = raw.net_worth(period.last());
		assert_eq!(
			sheet.total_assets.minor() - sheet.total_liabilities.minor(),
			sheet.net_worth.minor(),
			"{label}: the balance sheet balances"
		);
		assert_eq!(sheet.net_worth.minor(), closing, "{label}: sheet vs raw");

		let equity = owners_equity(&budget, &classification, period).unwrap();
		let opening = raw.net_worth_before(period.first());
		let explained: i64 = equity.lines().iter().map(|line| line.total.minor()).sum();
		assert_eq!(
			(equity.opening.minor(), equity.closing.minor()),
			(opening, closing),
			"{label}: equity opening and closing are net worth"
		);
		assert_eq!(opening + explained, closing, "{label}: opening + E1..E7");

		let cash = cash_flow(&budget, &classification, period).unwrap();
		assert_eq!(
			(cash.opening.minor(), cash.closing.minor()),
			(
				raw.budget_cash_before(period.first()),
				raw.budget_cash(period.last())
			),
			"{label}: cash opening and closing are the budget accounts"
		);
		assert_eq!(
			cash.opening.minor()
				+ cash.change_from_operations.minor()
				+ cash.in_transit.total.minor()
				+ cash.starting_balances.total.minor()
				+ cash.reconciliation_adjustments.total.minor(),
			cash.closing.minor(),
			"{label}: the cash flow statement ties"
		);
	}
}

#[test]
fn small_fixture_statements_close_every_month() {
	assert_statements_close_every_month("small");
}

#[test]
fn all_features_fixture_statements_close_every_month() {
	assert_statements_close_every_month("all-features");
}

#[test]
fn perf_100k_fixture_statements_close_every_month() {
	assert_statements_close_every_month("perf-100k");
}
