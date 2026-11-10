//! R-REP-5, R-REP-7: balances and net worth (ST-2), the Balance Sheet (ST-3),
//! Owner's Equity (ST-4, ST-5), the Cash Flow Statement (ST-6) and the
//! statement edge cases (ST-7), per docs/finance/statements.md.

#![allow(clippy::unwrap_used)]

mod support;

use support::*;
use veritas_domain::{AccountId, AccountKind, AccountMeta, Money, YearMonth};
use veritas_reports::{
	BalanceSheet, OwnersEquity, Side, account_balances, balance_sheet, cash_flow,
	net_profit_by_month, net_worth, net_worth_by_month, owners_equity,
};

fn sheet(book: &Book, on: &str) -> BalanceSheet {
	balance_sheet(&book.budget, &book.classify(), date(on)).unwrap()
}

fn equity(book: &Book, period: &str) -> OwnersEquity {
	let equity = owners_equity(&book.budget, &book.classify(), month(period)).unwrap();
	assert_eq!(
		equity.unexplained().unwrap(),
		Money::ZERO,
		"ST-4 must close on {period}"
	);
	equity
}

fn lines(equity: &OwnersEquity) -> [i64; 8] {
	[
		equity.opening.minor(),
		equity.net_income.total.minor(),
		equity.capitalized_transfers.total.minor(),
		equity.starting_balances.total.minor(),
		equity.reconciliation_adjustments.total.minor(),
		equity.revaluations.total.minor(),
		equity.unpaired_transfers.total.minor(),
		equity.closing.minor(),
	]
}

fn balances(book: &Book, on: &str) -> Vec<(String, i64)> {
	account_balances(&book.budget, &book.classify(), date(on))
		.unwrap()
		.into_iter()
		.map(|line| (line.account.to_string(), line.balance.minor()))
		.collect()
}

fn sheet_accounts(sheet: &BalanceSheet, side: Side) -> Vec<(String, i64)> {
	let sections = match side {
		Side::Asset => &sheet.assets,
		Side::Liability => &sheet.liabilities,
	};
	sections
		.iter()
		.flat_map(|section| &section.accounts)
		.map(|line| (line.account.to_string(), line.amount.minor()))
		.collect()
}

fn named(pairs: &[(&str, i64)]) -> Vec<(String, i64)> {
	pairs
		.iter()
		.map(|(name, amount)| ((*name).to_owned(), *amount))
		.collect()
}

#[test]
fn st5_master_january_balances_and_balance_sheet() {
	let book = master();
	assert_eq!(
		balances(&book, "2027-01-31"),
		named(&[
			("CHK", 388_800),
			("VISA", -7_500),
			("SAV", 520_000),
			("LOAN", -948_000),
			("BRK", 1_015_000),
			("CAR", 50_000),
			("CD", 100_000),
		])
	);
	let sheet = sheet(&book, "2027-01-31");
	assert_eq!(sheet.total_assets.minor(), 2_073_800);
	assert_eq!(sheet.total_liabilities.minor(), 955_500);
	assert_eq!(sheet.net_worth.minor(), 1_118_300);
	assert_eq!(
		sheet_accounts(&sheet, Side::Liability),
		named(&[("VISA", 7_500), ("LOAN", 948_000)])
	);
	assert_eq!(
		net_worth(&book.classify(), date("2027-01-31"))
			.unwrap()
			.minor(),
		1_118_300
	);
}

#[test]
fn st5_master_january_equity_reconciles_to_the_cent() {
	let equity = equity(&master(), "2027-01");
	assert_eq!(
		lines(&equity),
		[
			720_000, 234_500, 50_000, 100_000, -1_200, 15_000, 0, 1_118_300
		]
	);
	assert_eq!(equity.capitalized_transfers.rows, ids(&["T8"]));
	assert_eq!(equity.starting_balances.rows, ids(&["T7"]));
	assert_eq!(equity.reconciliation_adjustments.rows, ids(&["T10"]));
	assert_eq!(equity.revaluations.rows, ids(&["T9"]));
}

#[test]
fn st5_master_february_chains_from_january() {
	let book = master();
	let equity = equity(&book, "2027-02");
	assert_eq!(
		lines(&equity),
		[1_118_300, 297_500, 0, 0, 0, 0, 0, 1_415_800]
	);
	assert_eq!(sheet(&book, "2027-02-28").net_worth.minor(), 1_415_800);
	assert_eq!(
		balances(&book, "2027-02-28"),
		named(&[
			("CHK", 678_800),
			("VISA", 0),
			("SAV", 520_000),
			("LOAN", -948_000),
			("BRK", 1_015_000),
			("CAR", 50_000),
			("CD", 100_000),
		])
	);
}

#[test]
fn st6_master_cash_flow_closes_on_budget_cash() {
	let book = master();
	let january = cash_flow(&book.budget, &book.classify(), month("2027-01")).unwrap();
	assert_eq!(
		[
			january.opening,
			january.net_profit.total,
			january.debt_paydown.total,
			january.savings.total,
			january.net_cash_flow.total,
			january.unreviewed_transfers.total,
			january.change_from_operations,
			january.starting_balances.total,
			january.reconciliation_adjustments.total,
			january.closing,
		]
		.map(Money::minor),
		[
			200_000, 234_500, -32_000, -20_000, 182_500, 0, 182_500, 0, -1_200, 381_300
		]
	);
	assert_eq!(january.unexplained().unwrap(), Money::ZERO);

	let february = cash_flow(&book.budget, &book.classify(), month("2027-02")).unwrap();
	assert_eq!(
		(
			february.opening.minor(),
			february.net_cash_flow.total.minor(),
			february.closing.minor()
		),
		(381_300, 297_500, 678_800)
	);
	assert_eq!(february.unexplained().unwrap(), Money::ZERO);
}

#[test]
fn st7_s1_closed_account_with_a_balance_stays_on_the_sheet() {
	let mut book = master();
	book.budget
		.accounts
		.get_mut(&AccountId::new("SAV"))
		.unwrap()
		.closed = true;
	book.budget
		.accounts
		.get_mut(&AccountId::new("CAR"))
		.unwrap()
		.closed = true;
	let sheet = sheet(&book, "2026-12-31");
	let assets = sheet_accounts(&sheet, Side::Asset);
	assert!(assets.contains(&("SAV".to_owned(), 500_000)));
	assert!(
		!assets.iter().any(|(name, _)| name == "CAR"),
		"a zero-balance closed account is omitted"
	);
	assert!(assets.contains(&("CD".to_owned(), 0)), "open accounts stay");
	let line = sheet.assets[0]
		.accounts
		.iter()
		.find(|line| line.account == AccountId::new("SAV"))
		.unwrap();
	assert!(line.closed);
	assert_eq!(sheet.net_worth.minor(), 720_000);
}

#[test]
fn st7_s2_off_budget_revaluation_alone() {
	let mut book = master_book();
	book.row("Up", "BRK", "2027-01-25", 15_000);
	let equity = equity(&book, "2027-01");
	assert_eq!(lines(&equity), [720_000, 0, 0, 0, 0, 15_000, 0, 735_000]);
}

#[test]
fn st7_s3_starting_balance_is_equity_not_income() {
	let mut book = master_book();
	book.row("Open", "CD", "2027-01-15", 100_000)
		.starting_balance = true;
	let equity = equity(&book, "2027-01");
	assert_eq!(lines(&equity), [720_000, 0, 0, 100_000, 0, 0, 0, 820_000]);
}

#[test]
fn st7_s4_capitalized_transfer_reversed() {
	let mut book = master_book();
	book.transfer("Sold", "CHK", "CAR", "2027-01-20", 50_000)
		.in_category("Car purchase");
	let equity = equity(&book, "2027-01");
	assert_eq!(
		book.net_profit(month("2027-01")).expenses.total.minor(),
		-50_000
	);
	assert_eq!(equity.capitalized_transfers.total.minor(), -50_000);
	assert_eq!(equity.closing, equity.opening);
}

#[test]
fn st7_s5_debt_payment_moves_net_worth_by_the_interest_only() {
	let mut book = master_book();
	book.parent("Pay", "CHK", "2027-01-10", -40_000);
	book.transfer("Principal", "CHK", "LOAN", "2027-01-10", -32_000)
		.in_category("Loan principal")
		.child_of("Pay");
	book.row("Interest", "CHK", "2027-01-10", -8_000)
		.in_category("Loan interest")
		.child_of("Pay");
	let equity = equity(&book, "2027-01");
	assert_eq!(lines(&equity), [720_000, -8_000, 0, 0, 0, 0, 0, 712_000]);
	assert!(balances(&book, "2027-01-31").contains(&("LOAN".to_owned(), -948_000)));
}

#[test]
fn st7_s6_zero_history_period() {
	let equity = equity(&master(), "2031-06");
	assert_eq!(equity.closing, equity.opening);
	assert!(equity.lines().iter().all(|line| line.rows.is_empty()));
}

#[test]
fn st7_s7_unreviewed_transfer_shows_in_cash_flow_review_line() {
	let mut book = master_book();
	book.row("Pay", "CHK", "2027-05-01", 100_000)
		.in_category("Paycheck");
	book.row("Food", "CHK", "2027-05-02", -20_000)
		.in_category("Groceries");
	book.transfer("Move", "CHK", "BRK", "2027-05-03", -10_000);
	let flow = cash_flow(&book.budget, &book.classify(), month("2027-05")).unwrap();
	assert_eq!(flow.net_cash_flow.total.minor(), 80_000);
	assert_eq!(flow.unreviewed_transfers.total.minor(), -10_000);
	assert_eq!(flow.change_from_operations.minor(), 70_000);
	assert_eq!(
		(flow.closing.checked_sub(flow.opening)).unwrap().minor(),
		70_000
	);
	let equity = equity(&book, "2027-05");
	assert_eq!(equity.unpaired_transfers.total.minor(), 0);
	assert_eq!(equity.net_income.total.minor(), 80_000);
}

#[test]
fn st7_s8_unpaired_transfer_is_line_e6() {
	let mut book = master_book();
	book.row("U", "CHK", "2027-05-03", -10_000)
		.in_category("Savings")
		.payee = Some(Book::transfer_payee("SAV"));
	let equity = equity(&book, "2027-05");
	assert_eq!(lines(&equity), [720_000, 0, 0, 0, 0, 0, -10_000, 710_000]);
	assert_eq!(equity.unpaired_transfers.rows, ids(&["U"]));
	assert_eq!(sheet(&book, "2027-05-31").net_worth, equity.closing);
}

#[test]
fn st4_e6_one_way_link_counts_both_rows_and_nets_to_zero() {
	let mut book = master_book();
	book.transfer("S", "CHK", "SAV", "2027-05-12", -20_000)
		.in_category("Savings");
	book.get("Sm").transfer = None;
	let equity = equity(&book, "2027-05");
	assert_eq!(equity.unpaired_transfers.rows, ids(&["S", "Sm"]));
	assert_eq!(equity.unpaired_transfers.total, Money::ZERO);
}

#[test]
fn st2_liability_metadata_wins_and_otherwise_a_negative_balance_is_owed() {
	let mut book = master_book();
	book.budget.account_meta.insert(
		AccountId::new("LOAN"),
		AccountMeta {
			kind: Some(AccountKind::Liability),
			..AccountMeta::default()
		},
	);
	book.budget.account_meta.insert(
		AccountId::new("BRK"),
		AccountMeta {
			kind: Some(AccountKind::Asset),
			..AccountMeta::default()
		},
	);
	book.row("Paid off", "LOAN", "2027-01-05", 985_000);
	book.row("Margin", "BRK", "2027-01-05", -1_100_000);
	book.row("Swipe", "VISA", "2027-01-05", -100)
		.in_category("Groceries");
	let sheet = sheet(&book, "2027-01-31");
	assert_eq!(
		sheet_accounts(&sheet, Side::Liability),
		named(&[("VISA", 100), ("LOAN", -5_000)])
	);
	assert!(sheet_accounts(&sheet, Side::Asset).contains(&("BRK".to_owned(), -100_000)));
	assert_eq!(
		sheet
			.total_assets
			.checked_sub(sheet.total_liabilities)
			.unwrap(),
		sheet.net_worth
	);
}

#[test]
fn st3_balance_sheet_groups_accounts_by_account_group() {
	let mut book = master();
	book.account_group("Cash", 2.0)
		.account_group("Investments", 1.0);
	for (account, group) in [("CHK", "Cash"), ("SAV", "Cash"), ("BRK", "Investments")] {
		book.budget
			.accounts
			.get_mut(&AccountId::new(account))
			.unwrap()
			.group = Some(veritas_domain::AccountGroupId::new(group));
	}
	let sheet = sheet(&book, "2027-01-31");
	let groups: Vec<_> = sheet
		.assets
		.iter()
		.map(|section| {
			(
				section.group.as_ref().map(ToString::to_string),
				section.total.minor(),
			)
		})
		.collect();
	assert_eq!(
		groups,
		[
			(Some("Investments".to_owned()), 1_015_000),
			(Some("Cash".to_owned()), 908_800),
			(None, 150_000),
		]
	);
}

#[test]
fn cls1_orphaned_rows_count_in_no_balance() {
	let mut book = master_book();
	book.row("Ghost", "GONE", "2027-01-04", -500);
	assert_eq!(sheet(&book, "2027-01-31").net_worth.minor(), 720_000);
	assert_eq!(equity(&book, "2027-01").closing.minor(), 720_000);
}

#[test]
fn r_rep_5_net_worth_and_income_vs_expense_by_month() {
	let book = master();
	let points = net_worth_by_month(
		&book.budget,
		&book.classify(),
		YearMonth::new(2026, 12).unwrap(),
		YearMonth::new(2027, 2).unwrap(),
	)
	.unwrap();
	let series: Vec<_> = points
		.iter()
		.map(|point| {
			(
				point.month.to_string(),
				point.assets.minor(),
				point.liabilities.minor(),
				point.net_worth.minor(),
			)
		})
		.collect();
	assert_eq!(
		series,
		[
			("2026-12".to_owned(), 1_700_000, 980_000, 720_000),
			("2027-01".to_owned(), 2_073_800, 955_500, 1_118_300),
			("2027-02".to_owned(), 2_363_800, 948_000, 1_415_800),
		]
	);

	let months = net_profit_by_month(
		&book.budget,
		&book.classify(),
		YearMonth::new(2027, 1).unwrap(),
		YearMonth::new(2027, 2).unwrap(),
	)
	.unwrap();
	let income_vs_expense: Vec<_> = months
		.iter()
		.map(|month| (month.income.total.minor(), month.expenses.total.minor()))
		.collect();
	assert_eq!(income_vs_expense, [(300_000, 65_500), (300_000, 2_500)]);
}
