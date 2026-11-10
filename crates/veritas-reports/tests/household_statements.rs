//! R-REP-5, R-REP-7: the household from August 2026 through January 2027,
//! across a year end. Every statement is checked against balances summed
//! straight from the transactions, and against numbers worked out by hand.

#![allow(clippy::unwrap_used)]

mod support;

use support::household::*;
use support::*;
use veritas_domain::{BudgetData, YearMonth};
use veritas_reports::{
	BalanceSheet, CashFlow, Classification, OwnersEquity, Period, Side, account_balances,
	balance_sheet, cash_flow, net_profit_by_month, net_worth_by_month, owners_equity,
};

const MONTHS: [&str; 6] = [
	"2026-08", "2026-09", "2026-10", "2026-11", "2026-12", "2027-01",
];

struct Statements {
	sheet: BalanceSheet,
	equity: OwnersEquity,
	cash: CashFlow,
}

fn statements(budget: &BudgetData, classification: &Classification, period: Period) -> Statements {
	Statements {
		sheet: balance_sheet(budget, classification, period.last()).unwrap(),
		equity: owners_equity(budget, classification, period).unwrap(),
		cash: cash_flow(budget, classification, period).unwrap(),
	}
}

/// Opening, E1 to E7, closing.
fn equity_lines(equity: &OwnersEquity) -> [i64; 9] {
	let [e1, e2, e3, e4, e5, e6, e7] = equity.lines().map(|line| line.total.minor());
	[
		equity.opening.minor(),
		e1,
		e2,
		e3,
		e4,
		e5,
		e6,
		e7,
		equity.closing.minor(),
	]
}

/// Opening, net cash flow, unreviewed, in transit, starting balances,
/// reconciliation, closing.
fn cash_lines(cash: &CashFlow) -> [i64; 7] {
	[
		cash.opening.minor(),
		cash.net_cash_flow.total.minor(),
		cash.unreviewed_transfers.total.minor(),
		cash.in_transit.total.minor(),
		cash.starting_balances.total.minor(),
		cash.reconciliation_adjustments.total.minor(),
		cash.closing.minor(),
	]
}

fn listed(sheet: &BalanceSheet, side: Side) -> Vec<(String, i64, bool)> {
	let sections = match side {
		Side::Asset => &sheet.assets,
		Side::Liability => &sheet.liabilities,
	};
	sections
		.iter()
		.flat_map(|section| &section.accounts)
		.map(|line| (line.account.to_string(), line.amount.minor(), line.closed))
		.collect()
}

fn named(lines: &[(&str, i64, bool)]) -> Vec<(String, i64, bool)> {
	lines
		.iter()
		.map(|(name, amount, closed)| ((*name).to_owned(), *amount, *closed))
		.collect()
}

#[test]
fn every_month_the_statements_close_against_balances_summed_from_the_transactions() {
	let book = through_year_end();
	let classification = book.classify();
	let raw = RawLedger::new(&book.budget);
	for name in MONTHS {
		let period = month(name);
		let Statements {
			sheet,
			equity,
			cash,
		} = statements(&book.budget, &classification, period);
		let opening = raw.net_worth_before(period.first());
		let closing = raw.net_worth(period.last());

		assert_eq!(
			sheet.total_assets.minor() - sheet.total_liabilities.minor(),
			sheet.net_worth.minor(),
			"{name}: the balance sheet balances"
		);
		assert_eq!(sheet.net_worth.minor(), closing, "{name}: sheet vs raw");
		assert_eq!(equity.opening.minor(), opening, "{name}: opening equity");
		assert_eq!(equity.closing.minor(), closing, "{name}: closing equity");
		let explained: i64 = equity.lines().iter().map(|line| line.total.minor()).sum();
		assert_eq!(opening + explained, closing, "{name}: opening + E1..E7");

		assert_eq!(
			cash.opening.minor(),
			raw.budget_cash_before(period.first()),
			"{name}: cash opening"
		);
		assert_eq!(
			cash.closing.minor(),
			raw.budget_cash(period.last()),
			"{name}: cash closing"
		);
		assert_eq!(
			cash.opening.minor()
				+ cash.change_from_operations.minor()
				+ cash.in_transit.total.minor()
				+ cash.starting_balances.total.minor()
				+ cash.reconciliation_adjustments.total.minor(),
			cash.closing.minor(),
			"{name}: the cash flow statement ties"
		);
		assert_eq!(
			cash.net_profit.total, equity.net_income.total,
			"{name}: one net income everywhere"
		);
	}
}

#[test]
fn november_brings_in_new_accounts_and_a_card_payment_still_in_transit() {
	let book = through_year_end();
	let s = statements(&book.budget, &book.classify(), month("2026-11"));

	let net_income = (2 * GROSS + 105)
		- (2 * (FEDERAL + STATE + FICA) + 165_000 + 11_218 + 400 + 14_820 + 7_215 + 6_500);
	assert_eq!(net_income, 625_105 - 359_165);
	let new_accounts = -60_000 + 800_000 + 15_000;
	let revaluations = 4_180 + 12_345;
	let card_payment_out_today_in_tomorrow = -80_812;
	assert_eq!(
		2_838_126 + net_income + new_accounts + revaluations + card_payment_out_today_in_tomorrow,
		3_794_779
	);
	assert_eq!(
		equity_lines(&s.equity),
		[
			2_838_126, 265_940, 0, 755_000, 0, 16_525, 0, -80_812, 3_794_779
		]
	);

	let net_cash_flow = 265_940 - (19_782 + 30_000) - (2 * RETIREMENT + 50_000 - 14_000);
	assert_eq!(net_cash_flow, 142_658);
	assert_eq!(714_918 - 80_812 + 142_658 - 80_812, 805_299 - 109_347);
	assert_eq!(
		cash_lines(&s.cash),
		[634_106, 142_658, 0, -80_812, 0, 0, 695_952]
	);

	assert_eq!(
		listed(&s.sheet, Side::Asset),
		named(&[
			("CHK", 805_299, false),
			("HYSA", 1_386_920, false),
			("401K", 3_250_000, false),
			("BRK", 812_345, false),
			("OLDSAV", 1_000, true),
			("CU", 0, false),
		]),
		"an open account with nothing in it yet is still listed"
	);
	assert_eq!(
		listed(&s.sheet, Side::Liability),
		named(&[
			("VISA", 109_347, false),
			("LOAN", 2_321_438, false),
			("AUTO", 30_000, true),
		])
	);
	assert_eq!(805_299 + 1_386_920 + 3_250_000 + 812_345 + 1_000, 6_255_564);
	assert_eq!(109_347 + 2_321_438 + 30_000, 2_460_785);
	assert_eq!(
		(
			s.sheet.total_assets.minor(),
			s.sheet.total_liabilities.minor(),
			s.sheet.net_worth.minor()
		),
		(6_255_564, 2_460_785, 3_794_779)
	);
}

#[test]
fn december_overpays_the_car_loan_into_an_asset_and_saves_across_the_year_end() {
	let book = through_year_end();
	let s = statements(&book.budget, &book.classify(), month("2026-12"));

	let net_income = (2 * GROSS + 142)
		- (2 * (FEDERAL + STATE + FICA) + 165_000 + 11_123 + 120 + 21_404 + 35_000);
	assert_eq!(net_income, 625_142 - 386_659);
	let card_payment_lands = 80_812;
	let year_end_savings_leaves = -50_000;
	assert_eq!(card_payment_lands + year_end_savings_leaves, 30_812);
	assert_eq!(
		3_794_779 + 238_483 + 25_000 + (4_310 - 8_210) + 30_812,
		4_085_174
	);
	assert_eq!(
		equity_lines(&s.equity),
		[
			3_794_779, 238_483, 0, 25_000, 0, -3_900, 0, 30_812, 4_085_174
		]
	);

	let savings = 2 * RETIREMENT + 50_000 + 50_000;
	let net_cash_flow = 238_483 - (19_877 + 30_500) - savings;
	assert_eq!(net_cash_flow, 50_606);
	assert_eq!(
		695_952 + 50_606 + 80_812 + 25_000,
		883_774 - 56_404 + 25_000
	);
	assert_eq!(
		cash_lines(&s.cash),
		[695_952, 50_606, 0, 80_812, 25_000, 0, 852_370]
	);

	assert_eq!(
		listed(&s.sheet, Side::Asset),
		named(&[
			("CHK", 883_774, false),
			("HYSA", 1_441_230, false),
			("401K", 3_287_500, false),
			("AUTO", 500, true),
			("BRK", 804_135, false),
			("OLDSAV", 1_000, true),
			("CU", 25_000, false),
		])
	);
	assert_eq!(
		listed(&s.sheet, Side::Liability),
		named(&[("VISA", 56_404, false), ("LOAN", 2_301_561, false)])
	);
	assert_eq!(
		883_774 + 1_441_230 + 3_287_500 + 500 + 804_135 + 1_000 + 25_000,
		6_443_139
	);
	assert_eq!(
		(
			s.sheet.total_assets.minor(),
			s.sheet.total_liabilities.minor(),
			s.sheet.net_worth.minor()
		),
		(6_443_139, 2_357_965, 4_085_174)
	);
}

#[test]
fn january_receives_the_year_end_savings_and_the_car_loan_refund_closes_it_out() {
	let book = through_year_end();
	let classification = book.classify();
	let s = statements(&book.budget, &classification, month("2027-01"));

	let net_income = (2 * GROSS + 133) - (2 * (FEDERAL + STATE + FICA) + 165_000 + 11_028 + 13_377);
	assert_eq!(net_income, 281_716);
	let bank_fee = -75;
	let revaluations = 4_420 + 20_000;
	let savings_lands = 50_000;
	assert_eq!(
		4_085_174 + net_income + bank_fee + revaluations + savings_lands,
		4_441_235
	);
	assert_eq!(
		equity_lines(&s.equity),
		[4_085_174, 281_716, 0, 0, -75, 24_420, 0, 50_000, 4_441_235]
	);

	let refund_of_overpaid_principal = -500;
	let paydown = 19_972 + refund_of_overpaid_principal;
	let net_cash_flow = 281_716 - paydown - (2 * RETIREMENT + 50_000);
	assert_eq!(net_cash_flow, 174_744);
	let report =
		veritas_reports::net_profit(&book.budget, &classification, month("2027-01")).unwrap();
	assert_eq!(report.debt_paydown.total.minor(), 19_472);
	assert_eq!(
		cash_lines(&s.cash),
		[852_370, 174_744, 0, 0, 0, -75, 1_015_491 - 13_377 + 24_925]
	);

	assert_eq!(
		listed(&s.sheet, Side::Asset),
		named(&[
			("CHK", 1_015_491, false),
			("HYSA", 1_545_650, false),
			("401K", 3_325_000, false),
			("BRK", 824_135, false),
			("OLDSAV", 1_000, true),
			("CU", 24_925, false),
		]),
		"the car loan is closed at zero, so it leaves the sheet"
	);
	assert_eq!(
		listed(&s.sheet, Side::Liability),
		named(&[("VISA", 13_377, false), ("LOAN", 2_281_589, false)])
	);
	let auto = account_balances(&book.budget, &classification, date("2027-01-31"))
		.unwrap()
		.into_iter()
		.find(|line| line.account.to_string() == "AUTO")
		.unwrap();
	assert_eq!((auto.balance.minor(), auto.side), (0, Side::Liability));
}

#[test]
fn the_2026_books_close_with_the_year_end_savings_leg_in_transit() {
	let book = through_year_end();
	let classification = book.classify();
	let year = owners_equity(
		&book.budget,
		&classification,
		period("2026-01-01", "2026-12-31"),
	)
	.unwrap();

	let net_income = 161_932 + 242_728 + 213_960 + 265_940 + 238_483;
	let starting_balances = (350_000 - 42_000 + 1_200_000 + 3_100_000 - 2_400_000)
		+ (-60_000 + 800_000 + 15_000)
		+ 25_000;
	let revaluations = 4_100 + 4_350 + 4_290 + (4_180 + 12_345) + (4_310 - 8_210);
	assert_eq!(
		(net_income, starting_balances, revaluations),
		(1_123_043, 2_988_000, 25_365)
	);
	assert_eq!(
		net_income + starting_balances - 1_234 + revaluations - 50_000,
		4_085_174
	);
	assert_eq!(
		equity_lines(&year),
		[
			0, 1_123_043, 0, 2_988_000, -1_234, 25_365, 0, -50_000, 4_085_174
		]
	);

	let winter = owners_equity(
		&book.budget,
		&classification,
		period("2026-11-01", "2027-01-31"),
	)
	.unwrap();
	assert_eq!(
		winter.in_transit.total.minor(),
		0,
		"both legs of each straddling transfer are inside November to January"
	);
	assert_eq!(
		winter.closing.minor() - winter.opening.minor(),
		4_441_235 - 2_838_126
	);
}

#[test]
fn net_worth_and_income_vs_expense_by_month_match_the_monthly_statements() {
	let book = through_year_end();
	let classification = book.classify();
	let raw = RawLedger::new(&book.budget);
	let first = YearMonth::parse("2026-08").unwrap();
	let last = YearMonth::parse("2027-01").unwrap();

	let net_worth: Vec<_> = net_worth_by_month(&book.budget, &classification, first, last)
		.unwrap()
		.iter()
		.map(|point| {
			(
				point.assets.minor(),
				point.liabilities.minor(),
				point.net_worth.minor(),
			)
		})
		.collect();
	assert_eq!(
		net_worth,
		[
			(4_883_206, 2_509_174, 2_374_032),
			(5_028_767, 2_408_891, 2_619_876),
			(5_260_158, 2_422_032, 2_838_126),
			(6_255_564, 2_460_785, 3_794_779),
			(6_443_139, 2_357_965, 4_085_174),
			(6_736_201, 2_294_966, 4_441_235),
		]
	);
	for (name, (_, _, worth)) in MONTHS.iter().zip(&net_worth) {
		assert_eq!(*worth, raw.net_worth(month(name).last()), "{name}");
	}

	let income_vs_expense: Vec<_> = net_profit_by_month(&book.budget, &classification, first, last)
		.unwrap()
		.iter()
		.map(|month| (month.income.total.minor(), month.expenses.total.minor()))
		.collect();
	assert_eq!(
		income_vs_expense,
		[
			(625_118, 463_186),
			(625_131, 382_403),
			(625_097, 411_137),
			(625_105, 359_165),
			(625_142, 386_659),
			(625_133, 343_417),
		]
	);
}
