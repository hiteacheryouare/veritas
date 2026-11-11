//! R-REP-1, R-REP-2, R-REP-3: three months of one household's real-looking
//! money (Aug to Oct 2026), checked the way a person checks a spreadsheet.

mod support;

use std::collections::{BTreeMap, BTreeSet};

use support::household::*;
use support::*;
use veritas_domain::{BudgetData, SplitRole, TransactionId};
use veritas_reports::{CategoryLine, Line, NetProfit, Period, Section};

fn quarter() -> Period {
	period("2026-08-01", "2026-10-31")
}

/// Income, expenses, net profit, debt paydown, savings, net cash flow, unreviewed.
fn totals(report: &NetProfit) -> [i64; 7] {
	[
		report.income.total.minor(),
		report.expenses.total.minor(),
		report.net_profit.total.minor(),
		report.debt_paydown.total.minor(),
		report.savings.total.minor(),
		report.net_cash_flow.total.minor(),
		report.unreviewed.total.minor(),
	]
}

fn by_category(section: &Section) -> BTreeMap<String, i64> {
	section
		.categories
		.iter()
		.map(|line| {
			(
				line.category
					.as_ref()
					.map_or("(none)".to_owned(), |category| category.to_string()),
				line.total.minor(),
			)
		})
		.collect()
}

fn line<'a>(section: &'a Section, category: &str) -> Option<&'a CategoryLine> {
	section
		.categories
		.iter()
		.find(|line| line.category == Some(cat(category)))
}

fn raw_sum(budget: &BudgetData, rows: &[TransactionId]) -> i64 {
	rows.iter()
		.map(|id| {
			budget
				.transactions
				.get(id)
				.map_or(0, |row| row.amount.minor())
		})
		.sum()
}

/// What a bank or card statement shows: deposits and charges as single lines
/// (a split's parent, never its children), live rows only.
fn statement_movement(budget: &BudgetData, accounts: &[&str], period: Period) -> i64 {
	budget
		.transactions
		.values()
		.filter(|row| !row.tombstone && !matches!(row.split, SplitRole::Child { .. }))
		.filter(|row| accounts.contains(&row.account.to_string().as_str()))
		.filter(|row| period.contains(row.date))
		.map(|row| row.amount.minor())
		.sum()
}

#[test]
fn august_net_profit_matches_the_hand_computed_spreadsheet() {
	let report = household().net_profit(month("2026-08"));

	let income = 2 * GROSS + 118;
	let taxes = 2 * (FEDERAL + STATE + FICA);
	let rent = 165_000;
	let movers = 89_000;
	let dining_with_coffee = 650 + 575 + 4_250;
	let loan_interest = 11_500;
	let groceries = 14_237 + 11_863;
	let gym = 4_000;
	let streaming = 1_599;
	let phone = 6_500;
	let expenses = taxes
		+ rent + movers
		+ dining_with_coffee
		+ loan_interest
		+ groceries
		+ gym + streaming
		+ phone;
	let savings = 50_000 + 2 * RETIREMENT;
	assert_eq!((income, taxes, expenses), (625_118, 154_012, 463_186));
	assert_eq!(savings, 87_500);
	assert_eq!(625_118 - 463_186, 161_932);
	assert_eq!(161_932 - 19_500 - 87_500, 54_932);

	assert_eq!(
		totals(&report),
		[625_118, 463_186, 161_932, 19_500, 87_500, 54_932, 0]
	);
	assert_eq!(
		by_category(&report.expenses),
		BTreeMap::from([
			("Federal tax".to_owned(), 2 * FEDERAL),
			("State tax".to_owned(), 2 * STATE),
			("Social Security and Medicare".to_owned(), 2 * FICA),
			("Rent".to_owned(), rent),
			("Phone".to_owned(), phone),
			("Groceries".to_owned(), groceries),
			("Dining".to_owned(), dining_with_coffee),
			("Gym".to_owned(), gym),
			("Student loan interest".to_owned(), loan_interest),
			("Streaming".to_owned(), streaming),
			("Moving costs".to_owned(), movers),
		])
	);
	assert_eq!(
		by_category(&report.income),
		BTreeMap::from([
			("Paycheck".to_owned(), 625_000),
			("Interest".to_owned(), 118)
		])
	);
}

#[test]
fn september_refund_reduces_clothing_and_reconciliation_stays_out() {
	let report = household().net_profit(month("2026-09"));

	let income = 2 * GROSS + 131;
	let clothing = 12_000 - 4_500;
	let expenses =
		2 * (FEDERAL + STATE + FICA)
			+ 165_000 + (12_450 + 13_115)
			+ 11_407 + clothing
			+ 4_000 + 1_599
			+ 6_820 + 6_500;
	assert_eq!((income, expenses), (625_131, 382_403));
	assert_eq!(625_131 - 382_403, 242_728);
	assert_eq!(242_728 - 19_593 - 87_500, 135_635);

	assert_eq!(
		totals(&report),
		[625_131, 382_403, 242_728, 19_593, 87_500, 135_635, 0]
	);
	assert_eq!(
		line(&report.expenses, "Clothing").unwrap().total.minor(),
		7_500
	);
}

#[test]
fn october_late_refund_makes_clothing_negative_and_a_savings_withdrawal_lowers_savings() {
	let report = household().net_profit(month("2026-10"));

	let income = 2 * GROSS + 97;
	let clothing = -7_500;
	let expenses =
		2 * (FEDERAL + STATE + FICA)
			+ 165_000 + clothing
			+ 11_313 + 15_022
			+ 61_480 + 5_310
			+ 6_500;
	let savings = 2 * RETIREMENT + 50_000 - 30_000;
	assert_eq!((income, expenses, savings), (625_097, 411_137, 57_500));
	assert_eq!(625_097 - 411_137, 213_960);
	assert_eq!(213_960 - 19_687 - 57_500, 136_773);

	assert_eq!(
		totals(&report),
		[625_097, 411_137, 213_960, 19_687, 57_500, 136_773, 0]
	);
	assert_eq!(
		line(&report.expenses, "Clothing").unwrap().total.minor(),
		-7_500
	);
	assert_eq!(
		report.income.total.minor(),
		income,
		"a refund is never income"
	);
}

#[test]
fn the_quarter_is_the_sum_of_its_months() {
	let book = household();
	let report = book.net_profit(quarter());

	assert_eq!(625_118 + 625_131 + 625_097, 1_875_346);
	assert_eq!(463_186 + 382_403 + 411_137, 1_256_726);
	assert_eq!(19_500 + 19_593 + 19_687, 58_780);
	assert_eq!(87_500 + 87_500 + 57_500, 232_500);
	assert_eq!(1_875_346 - 1_256_726, 618_620);
	assert_eq!(618_620 - 58_780 - 232_500, 327_340);
	assert_eq!(
		totals(&report),
		[1_875_346, 1_256_726, 618_620, 58_780, 232_500, 327_340, 0]
	);
	assert_eq!(
		by_category(&report.savings),
		BTreeMap::from([
			("Retirement".to_owned(), 6 * RETIREMENT),
			("Savings".to_owned(), 3 * 50_000 - 30_000),
		])
	);
	assert_eq!(
		by_category(&report.debt_paydown),
		BTreeMap::from([("Student loan principal".to_owned(), 58_780)])
	);
}

#[test]
fn net_cash_flow_is_what_the_checking_and_card_statements_moved() {
	let book = household();
	let on_budget = ["CHK", "VISA"];
	let flow = |period| book.net_profit(period).net_cash_flow.total.minor();
	let moved = |period| statement_movement(&book.budget, &on_budget, period);

	let opening_balances = 350_000 - 42_000;
	assert_eq!(
		moved(month("2026-08")),
		flow(month("2026-08")) + opening_balances
	);
	let reconciliation = -1_234;
	assert_eq!(
		moved(month("2026-09")),
		flow(month("2026-09")) + reconciliation
	);
	assert_eq!(moved(month("2026-10")), flow(month("2026-10")));
	assert_eq!(moved(month("2026-10")), 136_773);
}

#[test]
fn debt_paydown_and_savings_match_what_the_off_budget_balances_moved() {
	let book = household();
	let report = book.net_profit(quarter());

	let loan_moved = statement_movement(&book.budget, &["LOAN"], quarter());
	assert_eq!(loan_moved, -2_400_000 + report.debt_paydown.total.minor());
	assert_eq!(-2_400_000 + 58_780, -2_341_220);

	let saved_moved = statement_movement(&book.budget, &["HYSA", "401K"], quarter());
	let opening = 1_200_000 + 3_100_000;
	let hysa_interest = 4_100 + 4_350 + 4_290;
	assert_eq!(
		saved_moved,
		opening + hysa_interest + report.savings.total.minor(),
		"off-budget interest moves the balance but is not savings or income"
	);
}

fn assert_section_drills_to_its_rows(budget: &BudgetData, section: &Section, sign: i64) {
	assert_eq!(sign * raw_sum(budget, &section.rows), section.total.minor());
	let mut from_lines = BTreeSet::new();
	for category in &section.categories {
		assert_eq!(
			sign * raw_sum(budget, &category.rows),
			category.total.minor(),
			"{:?}",
			category.category
		);
		for id in &category.rows {
			assert!(from_lines.insert(id.clone()), "{id} is on two lines");
		}
	}
	assert_eq!(from_lines, section.rows.iter().cloned().collect());
}

fn assert_line_drills_to_its_rows(budget: &BudgetData, line: &Line) {
	assert_eq!(raw_sum(budget, &line.rows), line.total.minor());
}

#[test]
fn every_number_drills_down_to_rows_that_sum_back_to_it() {
	let book = household();
	for period in [
		month("2026-08"),
		month("2026-09"),
		month("2026-10"),
		quarter(),
	] {
		let report = book.net_profit(period);
		assert_section_drills_to_its_rows(&book.budget, &report.income, 1);
		assert_section_drills_to_its_rows(&book.budget, &report.expenses, -1);
		assert_section_drills_to_its_rows(&book.budget, &report.debt_paydown, -1);
		assert_section_drills_to_its_rows(&book.budget, &report.savings, -1);
		assert_line_drills_to_its_rows(&book.budget, &report.net_profit);
		assert_line_drills_to_its_rows(&book.budget, &report.net_cash_flow);

		let sections = [
			&report.income.rows,
			&report.expenses.rows,
			&report.debt_paydown.rows,
			&report.savings.rows,
		];
		let mut seen = BTreeSet::new();
		for id in sections.into_iter().flatten() {
			assert!(seen.insert(id.clone()), "{id} is in two sections");
			let transaction = &book.budget.transactions[id];
			assert!(
				period.contains(transaction.date),
				"{id} is outside {period:?}"
			);
			assert!(
				["CHK", "VISA"].contains(&transaction.account.to_string().as_str()),
				"{id} is an off-budget leg in a drilldown"
			);
		}
		assert_eq!(seen, report.net_cash_flow.rows.iter().cloned().collect());
	}
}

#[test]
fn opening_balances_card_payments_interest_on_savings_and_reconciliation_never_drill_in() {
	let book = household();
	let report = book.net_profit(quarter());
	let listed: BTreeSet<String> = report
		.net_cash_flow
		.rows
		.iter()
		.map(ToString::to_string)
		.collect();

	for id in [
		"open-CHK",
		"open-VISA",
		"open-HYSA",
		"open-401K",
		"open-LOAN",
		"aug-card",
		"aug-cardm",
		"sep-card",
		"sep-cardm",
		"oct-card",
		"oct-cardm",
		"aug-hysa-interest",
		"sep-hysa-interest",
		"oct-hysa-interest",
		"sep-reconcile",
		"aug-pay-1",
		"oct-loan",
		"aug-savem",
		"oct-loan-credited",
		"sep-pay-2-401k-deposit",
	] {
		assert!(!listed.contains(id), "{id} drilled into net cash flow");
	}
	assert!(line(&report.income, "Starting Balances").is_none());
	assert!(
		report
			.expenses
			.categories
			.iter()
			.all(|line| line.category.is_some()),
		"every household row is categorized, so nothing may land on the uncategorized line"
	);
	assert!(report.unreviewed.rows.is_empty());

	let paycheck_children = 6 * 5;
	let loan_children = 3 * 2;
	let (august_purchases_and_moves, september, october) = (12, 11, 9);
	assert_eq!(
		listed.len(),
		paycheck_children + loan_children + august_purchases_and_moves + september + october
	);
	assert_eq!(listed.len(), 68);
}

#[test]
fn retired_categories_keep_their_history_and_say_why() {
	let book = household();
	let report = book.net_profit(quarter());

	let gym = line(&report.expenses, "Gym").unwrap();
	assert_eq!(gym.total.minor(), 4_000 + 4_000);
	assert_eq!(gym.rows, ids(&["aug-gym", "sep-gym"]));
	assert!(gym.labels.hidden && !gym.labels.archived);

	let streaming = line(&report.expenses, "Streaming").unwrap();
	assert_eq!(streaming.total.minor(), 1_599 + 1_599);
	assert!(streaming.labels.archived && !streaming.labels.hidden);

	let movers = line(&report.expenses, "Moving costs").unwrap();
	assert_eq!(movers.total.minor(), 89_000);
	assert!(movers.labels.hidden, "its group is hidden");

	let dining = line(&report.expenses, "Dining").unwrap();
	assert_eq!(dining.total.minor(), (650 + 575 + 4_250) + 6_820 + 5_310);
	assert!(dining.rows.contains(&TransactionId::new("aug-coffee-1")));
	assert!(dining.rows.contains(&TransactionId::new("aug-coffee-2")));
	assert_eq!(dining.labels, Default::default());
	assert!(
		line(&report.expenses, "Coffee").is_none(),
		"a deleted category's history reports under the category it was merged into"
	);

	let august = book.net_profit(month("2026-08"));
	assert!(line(&august.expenses, "Gym").unwrap().labels.hidden);
	assert!(line(&august.expenses, "Streaming").unwrap().labels.archived);
}
