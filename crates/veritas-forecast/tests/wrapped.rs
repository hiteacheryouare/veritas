//! R-WRAP-1 and R-WRAP-2: every FC-8 worked example (W-1 to W-36).

#![allow(clippy::unwrap_used, clippy::panic)]

mod support;

use support::*;
use veritas_domain::{CategoryGroupId, CategoryId, CategoryKind, GoalId, PayeeId};
use veritas_forecast::{Change, FunFact, Measure, Streak, Wrapped, wrapped, wrapped_years};
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
	assert_eq!(
		wrap(&book, 2027, "2028-01-01").completed_goals,
		[GoalId::new("laptop")]
	);
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

fn series(result: &Wrapped) -> Vec<(String, i64)> {
	result
		.monthly_spending
		.iter()
		.map(|(month, total)| (month.to_string(), total.minor()))
		.collect()
}

#[test]
fn fc_8_monthly_series_and_its_shares_of_total_out() {
	// Fred's example: out 30000; 12000 is 400 tenths, a -450 refund month is
	// -15, and 100 is 3.33, so 3. Each share is rounded on its own.
	let mut book = Book::household();
	book.spend("Groceries", "2027-01-05", -12000);
	book.spend("Fun", "2027-02-07", 450);
	book.spend("Dining", "2027-03-09", -100);
	book.spend("Groceries", "2027-04-11", -18350);
	book.spend("Groceries", "2027-05-25", -99999);
	let result = wrap(&book, 2027, "2027-05-20");
	assert_eq!(result.total_out, cents(30000));
	assert_eq!(
		series(&result),
		[
			("2027-01".to_owned(), 12000),
			("2027-02".to_owned(), -450),
			("2027-03".to_owned(), 100),
			("2027-04".to_owned(), 18350),
			("2027-05".to_owned(), 0),
		],
		"to date: May is in, without the row dated after today"
	);
	assert_eq!(
		result.redacted().months,
		[Some(400), Some(-15), Some(3), Some(612), Some(0)]
	);
	assert_eq!(result.biggest_month, Some((ym("2027-04"), cents(18350))));
}

#[test]
fn fc_8_monthly_series_leaves_out_months_before_the_first_row() {
	let mut book = Book::household();
	book.spend("Paycheck", "2026-03-10", 300000);
	book.spend("Groceries", "2026-04-02", -1000);
	book.spend("Groceries", "2026-12-30", -2000);
	let first_year = wrap(&book, 2026, "2027-02-01");
	let months: Vec<String> = series(&first_year)
		.into_iter()
		.map(|(month, _)| month)
		.collect();
	assert_eq!(months.first().map(String::as_str), Some("2026-03"));
	assert_eq!(months.len(), 10, "March through December");
	assert_eq!(
		series(&first_year)[0].1,
		0,
		"the first row's month has no spending"
	);

	let next_year = wrap(&book, 2027, "2027-02-01");
	assert_eq!(
		series(&next_year),
		[("2027-01".to_owned(), 0), ("2027-02".to_owned(), 0)]
	);
	assert_eq!(
		next_year.redacted().months,
		[None, None],
		"n/a without spending"
	);

	let before = wrap(&book, 2025, "2027-02-01");
	assert!(before.monthly_spending.is_empty());
	assert!(
		wrap(&Book::household(), 2027, "2027-02-01")
			.monthly_spending
			.is_empty()
	);
}

#[test]
fn fc_8_zero_months_in_the_series_never_win_biggest_month() {
	let mut book = Book::household();
	book.spend("Paycheck", "2027-01-05", 300000);
	book.spend("Fun", "2027-02-07", 500);
	let result = wrap(&book, 2027, "2027-03-31");
	assert_eq!(
		series(&result),
		[
			("2027-01".to_owned(), 0),
			("2027-02".to_owned(), -500),
			("2027-03".to_owned(), 0),
		]
	);
	assert_eq!(
		result.biggest_month,
		Some((ym("2027-02"), cents(-500))),
		"only months with expense rows are ranked"
	);
}

/// W-20's quarter: January +30000 of spending, February -5000 (refunds beat
/// spending), March without rows.
fn w_20_book() -> Book {
	let mut book = Book::household();
	book.spend("Groceries", "2027-01-05", -30_000);
	book.spend("Groceries", "2027-02-03", -1_000);
	book.spend("Groceries", "2027-02-10", 6_000);
	book
}

#[test]
fn w_20_the_monthly_series_shows_a_refund_month_as_negative() {
	let result = wrap(&w_20_book(), 2027, "2027-03-31");
	assert_eq!(
		result.monthly_spending,
		[
			(ym("2027-01"), cents(30_000)),
			(ym("2027-02"), cents(-5_000)),
			(ym("2027-03"), cents(0))
		]
	);
	assert_eq!(result.total_out, cents(25_000));
}

#[test]
fn w_21_monthly_bars_are_shares_of_total_out_one_by_one() {
	let result = wrap(&w_20_book(), 2027, "2027-03-31");
	let bars = result.redacted().months;
	assert_eq!(bars, [Some(1200), Some(-200), Some(0)]);
	assert_eq!(bars.iter().flatten().sum::<i64>(), 1000);

	let mut refunds_only = Book::household();
	refunds_only.spend("Groceries", "2027-01-05", 2_000);
	assert_eq!(
		wrap(&refunds_only, 2027, "2027-03-31").redacted().months,
		[None, None, None],
		"out <= 0 reads n/a"
	);
}

#[test]
fn w_22_the_series_never_changes_the_biggest_month() {
	let result = wrap(&w_20_book(), 2027, "2027-03-31");
	assert_eq!(result.biggest_month, Some((ym("2027-01"), cents(30_000))));
}

/// W-23: a year whose only rows are a transfer between two on-budget
/// accounts has data: it is listed and renders, with nothing in or out.
#[test]
fn w_23_a_transfers_only_year_is_listed_and_renders() {
	let mut book = Book::household();
	book.account("Savings", false);
	book.opening("CHK", "2026-12-01", 1_000_000);
	book.transfer("CHK", "Savings", "2027-05-10", -50_000);
	let classification = classify(&book.budget);
	let today = date("2027-12-31");

	assert_eq!(wrapped_years(&classification, today).unwrap(), [2027, 2026]);
	let result = wrapped(&book.budget, &classification, 2027, today).unwrap();
	assert!(result.has_data);
	assert_eq!(result.no_data_text(), None);
	assert_eq!(
		(result.total_in, result.total_out, result.net),
		(cents(0), cents(0), cents(0))
	);
	assert_eq!(
		(
			result.net_worth_start,
			result.net_worth_end,
			result.net_worth_change
		),
		(cents(1_000_000), cents(1_000_000), cents(0))
	);
	let redacted = result.redacted();
	assert_eq!((redacted.out_vs_in, redacted.net), (None, None));
}

/// `has_data` counts non-mirror legs only: the off-budget side of a transfer
/// to savings, landing in the new year, gives that year no data.
#[test]
fn a_year_whose_only_leg_is_a_mirror_has_no_data() {
	let mut book = Book::household();
	book.opening("CHK", "2026-12-01", 1_000_000);
	let sent = book
		.transfer("CHK", "SAV", "2026-12-30", -50_000)
		.id
		.clone();
	book.budget
		.transactions
		.get_mut(&veritas_domain::TransactionId::new(format!("{sent}m")))
		.unwrap()
		.date = date("2027-01-02");
	let classification = classify(&book.budget);
	let today = date("2027-06-01");

	assert_eq!(wrapped_years(&classification, today).unwrap(), [2026]);
	let result = wrapped(&book.budget, &classification, 2027, today).unwrap();
	assert!(!result.has_data);
	assert_eq!(result.no_data_text(), Some("No data for this year"));
}

#[test]
fn fc_8_years_with_data_are_the_years_whose_wrapped_has_data() {
	let mut book = Book::household();
	book.spend("Groceries", "2023-05-01", -1000);
	book.spend("Groceries", "2025-02-01", -1000);
	book.spend("Groceries", "2026-09-01", -1000);
	let classification = classify(&book.budget);
	let today = date("2026-06-01");
	let years = wrapped_years(&classification, today).unwrap();
	assert_eq!(
		years,
		[2025, 2023],
		"newest first; 2026's only row is after today"
	);
	for year in 2023..=2026 {
		let listed = years.contains(&year);
		let result = wrapped(&book.budget, &classification, year, today).unwrap();
		assert_eq!(listed, result.has_data, "{year}");
	}
	assert_eq!(
		wrapped_years(&classify(&Book::household().budget), today).unwrap(),
		Vec::<i32>::new()
	);
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

fn card_book() -> Book {
	let mut book = Book::household();
	book.group("Saving", false)
		.category("Emergency fund", "Saving")
		.kind("Emergency fund", CategoryKind::Savings);
	book.spend("Paycheck", "2027-02-01", 100000);
	book.spend("Groceries", "2027-02-03", -43000);
	book.spend("Electric", "2027-03-03", -7000);
	book.transfer("CHK", "SAV", "2027-03-04", -20000)
		.in_category("Emergency fund");
	book
}

#[test]
fn fc_8_redaction_covers_every_card_as_a_share() {
	let redacted = wrap(&card_book(), 2027, "2027-12-31").redacted();
	assert_eq!(redacted.out_vs_in, Some(500));
	assert_eq!(redacted.net, Some(500), "out and net add up to 1000");
	assert_eq!(redacted.savings, Some(200));
	assert_eq!(redacted.debt_paydown, Some(0));
	assert_eq!(
		redacted.biggest_month,
		Some(860),
		"February's 43000 of 50000"
	);
	assert_eq!(
		redacted.groups,
		[
			(Some(CategoryGroupId::new("Everyday")), 860),
			(Some(CategoryGroupId::new("Bills")), 140),
		]
	);
}

#[test]
fn fc_8_a_negative_out_share_rounds_half_away_from_zero() {
	let mut book = Book::household();
	book.spend("Paycheck", "2027-02-01", 400000);
	book.spend("Fun", "2027-02-02", 1000);
	let result = wrap(&book, 2027, "2027-12-31");
	assert_eq!(result.total_out, cents(-1000));
	let redacted = result.redacted();
	assert_eq!(redacted.out_vs_in, Some(-3), "-2.5 tenths rounds to -3");
	assert_eq!(redacted.net, Some(1003));
	assert_eq!(redacted.biggest_month, None, "no spending to divide by");
}

#[test]
fn fc_8_shares_are_n_a_without_income() {
	let mut book = Book::household();
	book.spend("Groceries", "2027-02-03", -43000);
	let redacted = wrap(&book, 2027, "2027-12-31").redacted();
	assert_eq!(
		(
			redacted.out_vs_in,
			redacted.net,
			redacted.savings,
			redacted.debt_paydown
		),
		(None, None, None, None)
	);
	assert_eq!(redacted.biggest_month, Some(1000));
}

#[test]
fn fc_8_fun_facts_use_only_the_users_own_numbers() {
	let facts = wrap(&w_1_book(), 2027, "2027-01-10").fun_facts();
	assert_eq!(
		facts,
		[
			FunFact::BiggestMonthVsTypical {
				month: ym("2027-01"),
				basis_points: 0
			},
			FunFact::TopCategoryShare {
				category: CategoryId::new("Rent"),
				tenths: 943
			},
			FunFact::NoSpendShare {
				days: 6,
				evaluated_days: 10,
				tenths: 600
			},
		],
		"no prior year to compare with, so no prior-year facts"
	);
}

#[test]
fn fc_8_prior_year_facts_skip_new_and_partial_years() {
	let mut book = Book::household();
	book.opening("CHK", "2025-12-31", 100000);
	book.spend("Groceries", "2026-05-01", -400000);
	book.spend("Groceries", "2027-05-01", -430000);
	book.spend("Paycheck", "2027-05-01", 860000);
	let prior: Vec<FunFact> = wrap(&book, 2027, "2027-12-31")
		.fun_facts()
		.into_iter()
		.filter(|fact| matches!(fact, FunFact::PriorYear { .. }))
		.collect();
	assert_eq!(
		prior,
		[
			FunFact::PriorYear {
				measure: Measure::TotalOut,
				basis_points: 750
			},
			FunFact::PriorYear {
				measure: Measure::Net,
				basis_points: 20750
			},
		],
		"total in was 0 last year, so it's new this year and not compared"
	);

	let mut late = Book::household();
	late.spend("Groceries", "2026-09-01", -20000);
	late.spend("Groceries", "2027-09-01", -30000);
	assert!(
		wrap(&late, 2027, "2027-12-31")
			.fun_facts()
			.iter()
			.all(|fact| !matches!(fact, FunFact::PriorYear { .. })),
		"a prior year that starts late is labeled, not compared"
	);
}

#[test]
fn w_18_top_category_share_rounds_half_away_from_zero() {
	// Categorized spending 10000, top category 3335: 333.5 rounds to 334.
	// The redaction's largest remainder gives the same row 333, because the
	// three smaller rows have bigger remainders, so the fact rounds alone.
	let mut book = Book::household();
	nets(
		&mut book,
		&[
			("Groceries", 3335),
			("Dining", 3329),
			("Fuel", 3319),
			("Fun", 17),
		],
	);
	book.row("CHK", "2027-04-01", -5000);
	let result = wrap(&book, 2027, "2027-12-31");
	assert_eq!(result.redacted().categories[0].1, 333);
	let top = result
		.fun_facts()
		.into_iter()
		.find(|fact| matches!(fact, FunFact::TopCategoryShare { .. }));
	assert_eq!(
		top,
		Some(FunFact::TopCategoryShare {
			category: CategoryId::new("Groceries"),
			tenths: 334
		}),
		"of categorized spending: the uncategorized 5000 isn't in the base"
	);
}

#[test]
fn w_19_no_spend_share_rounds_half_away_from_zero() {
	// 16 evaluated days, 5 of them no-spend: 312.5 rounds to 313.
	let mut book = Book::household();
	for day in 1..=16 {
		if ![3, 5, 8, 12, 14].contains(&day) {
			book.spend("Dining", &format!("2027-01-{day:02}"), -1000);
		}
	}
	let share = wrap(&book, 2027, "2027-01-16")
		.fun_facts()
		.into_iter()
		.find(|fact| matches!(fact, FunFact::NoSpendShare { .. }));
	assert_eq!(
		share,
		Some(FunFact::NoSpendShare {
			days: 5,
			evaluated_days: 16,
			tenths: 313
		})
	);
}

// N1 to N4, the editorial pages' figures (W-24 to W-36).

/// The net worth at each month end, as (month, amount), oldest first.
fn month_ends(result: &Wrapped) -> Vec<(&'static str, i64)> {
	const MONTHS: [&str; 12] = [
		"Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
	];
	result
		.month_ends
		.iter()
		.map(|end| {
			(
				MONTHS[end.month.month() as usize - 1],
				end.net_worth.minor(),
			)
		})
		.collect()
}

/// Rows that move net worth by each month's change, on the 15th.
fn moves(book: &mut Book, year: i32, changes: &[i64]) {
	for (index, change) in changes.iter().enumerate() {
		let on = format!("{year}-{:02}-15", index + 1);
		if *change >= 0 {
			book.spend("Paycheck", &on, *change);
		} else {
			book.spend("Groceries", &on, *change);
		}
	}
}

#[test]
fn w_24_month_end_net_worth() {
	let mut book = Book::household();
	book.opening("CHK", "2026-12-31", 720000);
	moves(
		&mut book,
		2027,
		&[
			15000, -33000, 58000, 20000, 10000, 10000, 15000, 5000, 20000, 60000, 100000, 118300,
		],
	);
	let result = wrap(&book, 2027, "2028-01-10");
	assert_eq!(
		month_ends(&result),
		[
			("Jan", 735000),
			("Feb", 702000),
			("Mar", 760000),
			("Apr", 780000),
			("May", 790000),
			("Jun", 800000),
			("Jul", 815000),
			("Aug", 820000),
			("Sep", 840000),
			("Oct", 900000),
			("Nov", 1000000),
			("Dec", 1118300)
		]
	);
	let lowest = result.lowest_month_end.unwrap();
	let highest = result.highest_month_end.unwrap();
	assert_eq!(
		(lowest.month, lowest.net_worth),
		(ym("2027-02"), cents(702000))
	);
	assert_eq!(
		(highest.month, highest.net_worth),
		(ym("2027-12"), cents(1118300))
	);
	assert_eq!(
		result.month_ends.last().unwrap().net_worth,
		result.net_worth_end
	);
	assert_eq!(result.net_worth_change, cents(398300));
	let redacted = result.redacted();
	assert_eq!(
		redacted.month_ends[..4],
		[Some(21), Some(-25), Some(56), Some(83)]
	);
	assert_eq!(redacted.month_ends[11], Some(553));
	assert_eq!(redacted.lowest_month_end, Some(-25));
	assert_eq!(redacted.highest_month_end, Some(553));
}

#[test]
fn w_25_month_end_net_worth_year_to_date() {
	let mut book = Book::household();
	book.opening("CHK", "2026-12-31", 720000);
	book.spend("Paycheck", "2027-01-15", 15000);
	book.spend("Groceries", "2027-02-15", -33000);
	book.spend("Paycheck", "2027-03-05", 8000);
	let result = wrap(&book, 2027, "2027-03-10");
	assert_eq!(
		month_ends(&result),
		[("Jan", 735000), ("Feb", 702000), ("Mar", 710000)]
	);
	assert_eq!(result.month_ends[2].date, date("2027-03-10"));
	assert_eq!(result.lowest_month_end.unwrap().month, ym("2027-02"));
	assert_eq!(result.highest_month_end.unwrap().month, ym("2027-01"));
	assert_eq!(result.net_worth_end, cents(710000));
	assert_eq!(result.net_worth_change, cents(-10000));
	assert_eq!(result.redacted().month_ends[2], Some(-14));
}

#[test]
fn w_26_months_before_the_first_row_are_left_out_and_have_no_shares() {
	let mut book = Book::household();
	book.opening("CHK", "2027-03-05", 500000);
	book.spend("Groceries", "2027-03-20", -20000);
	book.spend("Paycheck", "2027-04-20", 40000);
	let result = wrap(&book, 2027, "2028-01-01");
	assert_eq!(result.month_ends.len(), 10);
	assert_eq!(result.month_ends[0].month, ym("2027-03"));
	assert_eq!(month_ends(&result)[..2], [("Mar", 480000), ("Apr", 520000)]);
	assert_eq!(result.lowest_month_end.unwrap().month, ym("2027-03"));
	assert_eq!(
		result.highest_month_end.unwrap().month,
		ym("2027-04"),
		"ties to the earliest of April to December"
	);
	assert_eq!(result.net_worth_start, cents(0));
	let redacted = result.redacted();
	assert!(redacted.month_ends.iter().all(Option::is_none));
	assert_eq!(
		(redacted.lowest_month_end, redacted.highest_month_end),
		(None, None)
	);
}

#[test]
fn w_27_negative_opening_net_worth_has_no_shares() {
	let mut book = Book::household();
	book.opening("CHK", "2026-12-31", -50000);
	book.spend("Paycheck", "2027-01-15", 30000);
	let result = wrap(&book, 2027, "2027-01-31");
	assert_eq!(month_ends(&result), [("Jan", -20000)]);
	assert_eq!(result.lowest_month_end.unwrap().net_worth, cents(-20000));
	assert_eq!(result.redacted().month_ends, [None]);
}

#[test]
fn w_28_typical_month_with_an_even_count() {
	let mut book = Book::household();
	book.monthly(
		"Groceries",
		&[
			("2027-01", 30000),
			("2027-02", 36000),
			("2027-03", 42000),
			("2027-04", 50000),
		],
	);
	let result = wrap(&book, 2027, "2027-12-31");
	assert_eq!(result.total_out, cents(158000));
	assert_eq!(result.typical_month, Some(cents(39000)));
	assert_eq!(result.redacted().typical_month, Some(247));
	assert_eq!(result.biggest_vs_typical, Some(2821));
}

#[test]
fn w_29_typical_month_counts_a_refund_month() {
	let mut book = Book::household();
	book.spend("Groceries", "2027-01-15", -30000);
	book.spend("Gifts", "2027-02-15", 5000);
	book.spend("Groceries", "2027-03-15", -20000);
	let result = wrap(&book, 2027, "2027-12-31");
	assert_eq!(result.total_out, cents(45000));
	assert_eq!(result.typical_month, Some(cents(20000)));
	assert_eq!(result.redacted().typical_month, Some(444));
}

#[test]
fn w_30_typical_month_rounds_half_away_from_zero() {
	let mut book = Book::household();
	book.spend("Groceries", "2027-01-15", -10001);
	book.spend("Groceries", "2027-02-15", -10000);
	let result = wrap(&book, 2027, "2027-12-31");
	assert_eq!(result.typical_month, Some(cents(10001)));
	assert_eq!(result.redacted().typical_month, Some(500));

	let mut refunds = Book::household();
	refunds.spend("Gifts", "2027-01-15", 3);
	refunds.spend("Gifts", "2027-02-15", 2);
	assert_eq!(
		wrap(&refunds, 2027, "2027-12-31").typical_month,
		Some(cents(-3))
	);
}

#[test]
fn w_31_a_month_without_expense_rows_is_left_out() {
	let mut book = Book::household();
	book.spend("Groceries", "2027-01-15", -30000);
	book.spend("Paycheck", "2027-02-15", 90000);
	book.spend("Groceries", "2027-03-15", -10000);
	let result = wrap(&book, 2027, "2027-12-31");
	assert_eq!(result.typical_month, Some(cents(20000)));
	assert_eq!(result.redacted().typical_month, Some(500));
}

#[test]
fn w_32_typical_month_not_shown() {
	let mut book = Book::household();
	book.spend("Gifts", "2027-01-15", 5000);
	let result = wrap(&book, 2027, "2027-12-31");
	assert_eq!(result.typical_month, Some(cents(-5000)));
	assert_eq!(result.biggest_vs_typical, None);
	assert_eq!(result.redacted().typical_month, None);
}

/// W-33: W-1's days plus a capitalized transfer, a savings move, a loan
/// payment's interest, a bill-linked purchase and a zero row.
fn w_33_book() -> Book {
	let mut book = w_1_book();
	book.account("CAR", true)
		.category("Savings", "Goals")
		.kind("Savings", CategoryKind::Savings);
	book.transfer("CHK", "CAR", "2027-01-05", -50000)
		.in_category("Gifts");
	book.transfer("CHK", "SAV", "2027-01-04", -20000)
		.in_category("Savings");
	book.row_with_id("pay", "CHK", "2027-01-03", -10000).split = veritas_domain::SplitRole::Parent;
	book.transfer("CHK", "LOAN", "2027-01-03", -8800)
		.in_category("Loan principal")
		.child_of("pay");
	book.spend("Loan interest", "2027-01-03", -1200)
		.child_of("pay");
	book.row("VISA", "2027-01-06", -4000)
		.in_category("Dining")
		.bill("Phone");
	book.row("VISA", "2027-01-07", 0).in_category("Dining");
	book
}

#[test]
fn w_33_no_spend_dates() {
	let result = wrap(&w_33_book(), 2027, "2027-01-10");
	let expected: Vec<_> = ["01", "04", "07", "08", "10"]
		.iter()
		.map(|day| date(&format!("2027-01-{day}")))
		.collect();
	assert_eq!(result.no_spend.dates, expected);
	assert_eq!(result.no_spend.days, 5);
	assert_eq!(
		result.no_spend.longest,
		Some(Streak {
			start: date("2027-01-07"),
			days: 2
		})
	);
	assert_eq!(result.no_spend.current, Some(1));
}

#[test]
fn w_34_the_calendar_has_three_states() {
	let result = wrap(&w_33_book(), 2027, "2027-01-10");
	let evaluated = result.no_spend.evaluated.unwrap();
	assert_eq!(
		(evaluated.first(), evaluated.last()),
		(date("2027-01-01"), date("2027-01-10"))
	);
	let spent: Vec<u32> = (1..=10)
		.filter(|day| {
			!result
				.no_spend
				.dates
				.contains(&date(&format!("2027-01-{day:02}")))
		})
		.collect();
	assert_eq!(spent, [2, 3, 5, 6, 9]);

	let mut late = Book::household();
	late.spend("Dining", "2027-01-05", -1000);
	let result = wrap(&late, 2027, "2027-01-10");
	assert_eq!(
		result.no_spend.evaluated.unwrap().first(),
		date("2027-01-05")
	);
	assert!(
		result
			.no_spend
			.dates
			.iter()
			.all(|day| *day > date("2027-01-05"))
	);
}

fn w_35_book() -> Book {
	let mut book = Book::household();
	for (id, month) in [
		("A", "2027-03"),
		("B", "2027-07"),
		("C", "2026-11"),
		("D", "2027-02"),
	] {
		book.category(id, "Goals");
		book.goal(id, id, 100000, None).reached_month = Some(ym(month));
	}
	book.spend("B", "2027-08-02", -60000);
	book.spend("Groceries", "2026-10-01", -1000);
	book
}

fn reached(result: &Wrapped) -> Vec<(&str, veritas_domain::YearMonth)> {
	result
		.reached_goals
		.iter()
		.map(|goal| (goal.name.as_str(), goal.month))
		.collect()
}

#[test]
fn w_35_reached_month_per_goal() {
	let result = wrap(&w_35_book(), 2027, "2028-01-10");
	assert_eq!(
		reached(&result),
		[
			("D", ym("2027-02")),
			("A", ym("2027-03")),
			("B", ym("2027-07"))
		]
	);
	assert_eq!(result.reached_goals.len(), result.goals_completed);
	assert!(result.reached_goals.iter().all(|goal| !goal.deleted));
	assert_eq!(
		wrap(&w_35_book(), 2026, "2028-01-10").reached_goals.len(),
		1
	);
}

#[test]
fn w_36_reached_month_year_to_date() {
	let result = wrap(&w_35_book(), 2027, "2027-03-10");
	assert_eq!(
		reached(&result),
		[("D", ym("2027-02")), ("A", ym("2027-03"))]
	);
	assert_eq!(result.goals_completed, 2);
}

#[test]
fn a_reached_goal_whose_category_was_deleted_says_so() {
	let mut book = w_35_book();
	book.budget.categories.get_mut(&cat("A")).unwrap().tombstone = true;
	let result = wrap(&book, 2027, "2028-01-10");
	let goal = result
		.reached_goals
		.iter()
		.find(|goal| goal.goal == GoalId::new("A"))
		.unwrap();
	assert!(goal.deleted);
	assert_eq!(goal.name, "A");
}
