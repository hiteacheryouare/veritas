//! R-WRAP-1 and R-WRAP-2: every FC-8 worked example (W-1 to W-13).

#![allow(clippy::unwrap_used, clippy::panic)]

mod support;

use support::*;
use veritas_domain::{CategoryKind, PayeeId};
use veritas_forecast::{Change, Streak, Wrapped, wrapped};
use veritas_reports::classify;

fn wrap(book: &Book, year: i32, today: &str) -> Wrapped {
	wrapped(&book.budget, &classify(&book.budget), year, date(today)).unwrap()
}

/// W-1's ten days: discretionary spending on the 2nd, 3rd, 6th and 9th.
fn w_1_book() -> Book {
	let mut book = Book::household();
	book.spend("Rent", "2027-01-01", -165000).bill("Rent");
	for day in ["02", "03", "06", "09"] {
		book.row("VISA", &format!("2027-01-{day}"), -2500)
			.in_category("Dining");
	}
	book.row("VISA", "2027-01-07", 1500).in_category("Fun");
	book.transfer("CHK", "VISA", "2027-01-08", -10000);
	book
}

#[test]
fn w_1_no_spend_days_and_streaks() {
	let result = wrap(&w_1_book(), 2027, "2027-01-10");
	assert_eq!(result.no_spend.days, 6);
	assert_eq!(
		result.no_spend.longest,
		Some(Streak {
			start: date("2027-01-04"),
			days: 2
		})
	);
	assert_eq!(result.no_spend.current, Some(1));
}

#[test]
fn w_2_days_before_the_first_row_do_not_count() {
	let mut book = Book::household();
	book.spend("Dining", "2027-01-05", -1000);
	let result = wrap(&book, 2027, "2027-01-10");
	let evaluated = result.no_spend.evaluated.unwrap();
	assert_eq!(
		(evaluated.first(), evaluated.last()),
		(date("2027-01-05"), date("2027-01-10"))
	);
	assert_eq!(result.no_spend.days, 5);
}

#[test]
fn w_3_loan_payment_parts_do_not_break_a_day() {
	let mut book = Book::household();
	book.opening("LOAN", "2027-01-01", -980000);
	book.row_with_id("pay", "CHK", "2027-01-10", -40000).split = veritas_domain::SplitRole::Parent;
	book.transfer("CHK", "LOAN", "2027-01-10", -32000)
		.in_category("Loan principal")
		.child_of("pay");
	book.spend("Loan interest", "2027-01-10", -8000)
		.child_of("pay");
	book.spend("Dining", "2027-01-09", -1000);
	let result = wrap(&book, 2027, "2027-01-10");
	assert_eq!(result.no_spend.days, 9);
	assert_eq!(result.no_spend.current, Some(1));
	assert_eq!(result.total_out, cents(9000));
	assert_eq!(result.debt_paydown, cents(32000));
}

fn nets(book: &mut Book, nets: &[(&str, i64)]) {
	for (category, net) in nets {
		book.spend(category, "2027-03-15", -net);
	}
}

#[test]
fn w_4_top_categories_keep_only_positive_nets() {
	let mut book = Book::household();
	nets(
		&mut book,
		&[
			("Groceries", 43000),
			("Dining", 30000),
			("Fuel", 17000),
			("Gifts", -2000),
			("Fun", 10000),
		],
	);
	let result = wrap(&book, 2027, "2027-12-31");
	let top: Vec<(&str, i64)> = result
		.top_categories
		.iter()
		.map(|rank| (rank.name.as_str(), rank.total.minor()))
		.collect();
	assert_eq!(
		top,
		[
			("Groceries", 43000),
			("Dining", 30000),
			("Fuel", 17000),
			("Fun", 10000)
		]
	);
	assert_eq!(result.other_categories, cents(0));
	assert_eq!(result.total_out, cents(98000));
}

#[test]
fn w_5_ties_go_to_the_earlier_name() {
	let mut book = Book::household();
	nets(&mut book, &[("Fun", 20000), ("Books", 20000)]);
	let result = wrap(&book, 2027, "2027-12-31");
	assert_eq!(result.top_categories[0].name, "Books");
	assert_eq!(result.top_categories[1].name, "Fun");
}

#[test]
fn w_14_everything_else_leaves_out_refunds_and_uncategorized() {
	let mut book = Book::household();
	book.category("Pets", "Everyday")
		.category("Refunds", "Everyday")
		.hide("Books");
	nets(
		&mut book,
		&[
			("Groceries", 43000),
			("Dining", 30000),
			("Fuel", 17000),
			("Fun", 10000),
			("Books", 6000),
			("Gifts", 5000),
			("Pets", 4000),
			("Refunds", -2000),
		],
	);
	book.row("CHK", "2027-04-01", -9000);
	let result = wrap(&book, 2027, "2027-12-31");
	let names: Vec<&str> = result
		.top_categories
		.iter()
		.map(|rank| rank.name.as_str())
		.collect();
	assert_eq!(names, ["Groceries", "Dining", "Fuel", "Fun", "Books"]);
	assert!(result.top_categories[4].labels.hidden);
	assert_eq!(result.other_categories, cents(9000));
	let redacted = result.redacted();
	let shares: Vec<i64> = redacted
		.categories
		.iter()
		.map(|(_, share)| *share)
		.collect();
	assert_eq!(shares, [374, 261, 148, 87, 52, 78]);
	assert_eq!(redacted.categories.last().unwrap().0, None);
}

#[test]
fn w_9b_prior_year_net_from_a_negative_base_year_to_date() {
	let mut book = Book::household();
	book.opening("CHK", "2025-12-31", 100000);
	book.spend("Groceries", "2026-02-10", -20000);
	book.spend("Paycheck", "2026-06-10", 900000);
	book.spend("Paycheck", "2027-02-10", 10000);
	let result = wrap(&book, 2027, "2027-03-10");
	assert_eq!(result.prior_year.net, Change::BasisPoints(15000));
	assert_eq!(result.prior_year.from, None);
}

#[test]
fn fc_8_a_prior_year_that_starts_late_is_labeled() {
	let mut book = Book::household();
	book.spend("Groceries", "2026-09-01", -20000);
	book.spend("Groceries", "2027-09-01", -30000);
	let result = wrap(&book, 2027, "2027-12-31");
	assert_eq!(result.prior_year.from, Some(date("2026-09-01")));
	assert_eq!(result.prior_year.total_out, Change::BasisPoints(5000));
}

#[test]
fn w_15_out_vs_in_needs_positive_income() {
	let mut book = Book::household();
	book.spend("Paycheck", "2027-02-01", 100000);
	book.spend("Groceries", "2027-02-01", -50000);
	assert_eq!(
		wrap(&book, 2027, "2027-12-31").redacted().out_vs_in,
		Some(500)
	);
	let mut negative = Book::household();
	negative.spend("Paycheck", "2027-02-01", -3000);
	negative.spend("Groceries", "2027-02-01", -50000);
	assert_eq!(
		wrap(&negative, 2027, "2027-12-31").redacted().out_vs_in,
		None
	);
}

#[test]
fn top_groups_and_payees() {
	let mut book = Book::household();
	book.payee("Market").payee("Cafe");
	book.spend("Groceries", "2027-02-01", -30000)
		.payee_is("Market");
	book.spend("Groceries", "2027-02-08", -20000)
		.payee_is("Market");
	book.spend("Dining", "2027-02-09", -9000).payee_is("Cafe");
	book.spend("Electric", "2027-02-10", -8000);
	book.transfer("CHK", "SAV", "2027-02-11", -50000)
		.in_category("Fun");
	let result = wrap(&book, 2027, "2027-12-31");
	let payees: Vec<(&str, i64, usize)> = result
		.top_payees
		.iter()
		.map(|rank| (rank.name.as_str(), rank.total.minor(), rank.rows))
		.collect();
	assert_eq!(payees, [("Market", 50000, 2), ("Cafe", 9000, 1)]);
	let groups: Vec<(&str, i64)> = result
		.top_groups
		.iter()
		.map(|rank| (rank.name.as_str(), rank.total.minor()))
		.collect();
	assert_eq!(groups, [("Everyday", 109000), ("Bills", 8000)]);
	assert!(
		result
			.top_payees
			.iter()
			.all(|rank| rank.payee != PayeeId::new("payee-SAV"))
	);
}

#[test]
fn w_8_net_worth_change_and_its_redaction() {
	let mut book = Book::household();
	book.opening("CHK", "2026-12-31", 720000);
	book.spend("Paycheck", "2027-06-01", 398300);
	let result = wrap(&book, 2027, "2028-01-05");
	assert_eq!(result.net_worth_start, cents(720000));
	assert_eq!(result.net_worth_end, cents(1118300));
	assert_eq!(result.net_worth_change, cents(398300));
	let redacted = result.redacted();
	assert_eq!(redacted.net_worth_change, Some(553));
	assert_eq!(redacted.out_vs_in, Some(0));
}

#[test]
fn w_8_first_row_inside_the_period_starts_from_zero() {
	let mut book = Book::household();
	book.opening("CHK", "2027-02-01", 50000);
	let result = wrap(&book, 2027, "2027-12-31");
	assert_eq!(result.net_worth_start, cents(0));
	assert_eq!(result.redacted().net_worth_change, None);
}

#[test]
fn w_9_prior_year_change_and_out_vs_in() {
	let mut book = Book::household();
	book.spend("Groceries", "2026-05-01", -400000);
	book.spend("Groceries", "2027-05-01", -430000);
	book.spend("Paycheck", "2027-05-01", 860000);
	let result = wrap(&book, 2027, "2027-12-31");
	assert_eq!(result.prior_year.total_out, Change::BasisPoints(750));
	assert_eq!(result.prior_year.total_in, Change::NewThisYear);
	assert_eq!(result.redacted().out_vs_in, Some(500));
}

#[test]
fn prior_year_for_the_current_year_uses_the_same_dates() {
	let mut book = Book::household();
	book.spend("Groceries", "2026-02-10", -10000);
	book.spend("Groceries", "2026-08-10", -90000);
	book.spend("Groceries", "2027-02-10", -15000);
	let result = wrap(&book, 2027, "2027-03-10");
	assert_eq!(result.prior_year.total_out, Change::BasisPoints(5000));
}

#[test]
fn w_10_goals_count_in_the_year_they_were_reached() {
	let mut book = Book::household();
	book.category("New laptop", "Goals");
	book.goal("laptop", "New laptop", 150000, None)
		.reached_month = Some(ym("2027-03"));
	book.goal("old", "New laptop", 1000, None).reached_month = Some(ym("2026-11"));
	book.spend("New laptop", "2027-04-02", -148000);
	assert_eq!(wrap(&book, 2027, "2028-01-01").goals_completed, 1);
	assert_eq!(wrap(&book, 2028, "2028-06-01").goals_completed, 0);
}

#[test]
fn w_11_biggest_month_tie_goes_to_january() {
	let mut book = Book::household();
	book.spend("Rent", "2027-01-05", -300000);
	book.spend("Rent", "2027-06-05", -300000);
	book.spend("Rent", "2027-03-05", -100000);
	let result = wrap(&book, 2027, "2027-12-31");
	assert_eq!(result.biggest_month, Some((ym("2027-01"), cents(300000))));
	assert_eq!(result.biggest_vs_typical, Some(0));
}

#[test]
fn biggest_month_against_the_median_month() {
	let mut book = Book::household();
	for (month, amount) in [("01", 100000), ("02", 100000), ("03", 150000)] {
		book.spend("Groceries", &format!("2027-{month}-05"), -amount);
	}
	let result = wrap(&book, 2027, "2027-12-31");
	assert_eq!(result.biggest_vs_typical, Some(5000));
}

#[test]
fn w_12_current_year_runs_to_today() {
	let mut book = Book::household();
	book.spend("Groceries", "2027-01-05", -1000);
	book.spend("Groceries", "2027-03-05", -2000);
	book.spend("Groceries", "2027-03-20", -900000);
	let result = wrap(&book, 2027, "2027-03-10");
	assert_eq!(result.period.last(), date("2027-03-10"));
	assert_eq!(result.biggest_month, Some((ym("2027-03"), cents(2000))));
	assert_eq!(
		result.no_spend.evaluated.unwrap().last(),
		date("2027-03-10")
	);
	assert_eq!(result.no_spend.current, Some(5));
}

#[test]
fn w_13_empty_year() {
	let mut book = Book::household();
	book.spend("Groceries", "2025-05-01", -1000);
	let result = wrap(&book, 2026, "2027-02-01");
	assert!(!result.has_data);
	assert_eq!(result.no_data_text(), Some("No data for this year"));
	assert_eq!((result.total_in, result.total_out), (cents(0), cents(0)));
	assert!(result.top_categories.is_empty());
	assert_eq!(result.biggest_month, None);
	assert_eq!(result.goals_completed, 0);
	let redacted = result.redacted();
	assert!(redacted.categories.is_empty());
	assert_eq!(redacted.out_vs_in, None);
}

#[test]
fn a_future_year_is_rejected() {
	let book = Book::household();
	let result = wrapped(
		&book.budget,
		&classify(&book.budget),
		2028,
		date("2027-06-01"),
	);
	assert!(result.is_err());
}

#[test]
fn savings_and_debt_are_separate_cards_not_out() {
	let mut book = Book::household();
	book.group("Saving", false)
		.category("Emergency fund", "Saving")
		.kind("Emergency fund", CategoryKind::Savings);
	book.transfer("CHK", "SAV", "2027-02-01", -50000)
		.in_category("Emergency fund");
	book.spend("Groceries", "2027-02-01", -1000);
	let result = wrap(&book, 2027, "2027-12-31");
	assert_eq!(
		(result.total_out, result.savings),
		(cents(1000), cents(50000))
	);
}
