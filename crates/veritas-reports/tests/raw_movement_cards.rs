#![allow(
	clippy::unwrap_used,
	clippy::expect_used,
	clippy::panic,
	reason = "integration tests fail by panicking"
)]

//! ST-10.10's worked examples, pinned as they read in
//! docs/finance/statements.md.

mod support;

use chrono::NaiveDate;
use support::{Book, RowExt};
use veritas_reports::{Json, monte_carlo_card, net_worth_card, spending};

fn day(text: &str) -> NaiveDate {
	NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
}

/// "One month has Groceries -5000, an uncategorized purchase -4000, an
/// on-budget transfer pair of -40000 and +40000, and an uncategorized
/// transfer to off-budget savings of -10000. The card shows 19000."
#[test]
fn the_spending_card_counts_everything_outside_income_categories() {
	let mut book = Book::new();
	book.account("CHK", false)
		.account("VISA", false)
		.account("SAV", true)
		.group("Food", false)
		.category("Groceries", "Food", None);
	book.row("groceries", "CHK", "2026-03-03", -5_000)
		.in_category("Groceries");
	book.row("purchase", "CHK", "2026-03-04", -4_000);
	book.transfer("card", "CHK", "VISA", "2026-03-05", -40_000);
	book.transfer("save", "CHK", "SAV", "2026-03-06", -10_000);

	let card = spending(&book.budget, None, day("2026-03-20")).unwrap();
	let through = card.days[card.headline_day as usize - 1].clone();
	assert_eq!(
		through.compare.map(|spent| spent.minor().abs()),
		Some(19_000)
	);
}

/// "If CHK has -20000 on January 31 and SAV has +20000 on February 1, the
/// card ending January shows no change (0)."
#[test]
fn the_net_worth_card_leaves_money_in_transit_where_it_was() {
	let mut book = Book::new();
	book.account("CHK", false).account("SAV", true);
	book.transfer("move", "CHK", "SAV", "2026-01-31", -20_000);
	book.get("movem").date = support::date("2026-02-01");

	let january =
		Json::parse(r#"{"timeFrame":{"start":"2026-01","end":"2026-01","mode":"static"}}"#)
			.unwrap();
	let card = net_worth_card(&book.budget, Some(&january), day("2026-02-15"), 0).unwrap();
	let last = card.periods.last().unwrap();
	assert_eq!(last.start, day("2026-01-01"));
	assert_eq!(last.net_worth.minor(), 0);
	assert_eq!(last.change.minor(), 0);
	assert_eq!(card.total_change.minor(), 0);
}

/// "if 4,250 last to age 90 the headline reads 'Success rate to age 90:
/// 85%'. A setting of 500 would run 1,000."
#[test]
fn the_monte_carlo_headline_is_the_share_of_runs_that_last() {
	assert_eq!(
		veritas_reports::monte_carlo::success_percent(4_250.0 / 5_000.0),
		85.0
	);
	let few = Json::parse(r#"{"simulationCount":500,"currentAge":60,"targetAge":90}"#).unwrap();
	let card = monte_carlo_card(&Book::new().budget, Some(&few)).unwrap();
	assert_eq!(card.result.simulation_count, 1_000);
	assert_eq!(card.end_age, 90.0);
}
