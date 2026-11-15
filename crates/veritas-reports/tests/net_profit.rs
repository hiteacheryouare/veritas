//! R-REP-2, R-REP-3: Net Profit per docs/finance/statements.md ST-1 and the
//! CLS-8 master scenario, plus the PLAN §10 loan and card scenarios.

mod support;

use support::*;
use veritas_domain::{
	CategoryKind, CivilDate, Goal, GoalId, GoalStatus, Money, PayeeId, TransactionId,
};
use veritas_reports::{NetProfit, Period, Section};

fn totals(report: &NetProfit) -> [i64; 7] {
	[
		report.income.total,
		report.expenses.total,
		report.net_profit.total,
		report.debt_paydown.total,
		report.savings.total,
		report.net_cash_flow.total,
		report.unreviewed.total,
	]
	.map(Money::minor)
}

fn category_totals(section: &Section) -> Vec<(String, i64)> {
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

#[test]
fn st1_master_january_magnitudes() {
	let report = master().net_profit(month("2027-01"));
	assert_eq!(
		totals(&report),
		[300_000, 65_500, 234_500, 32_000, 20_000, 182_500, 0]
	);
	assert_eq!(
		category_totals(&report.expenses),
		[
			("Groceries".to_owned(), 5_000),
			("Clothing".to_owned(), 2_500),
			("Loan interest".to_owned(), 8_000),
			("Car purchase".to_owned(), 50_000),
		]
	);
}

#[test]
fn st1_6_every_number_drills_to_its_primary_rows_by_date_then_id() {
	let report = master().net_profit(month("2027-01"));
	assert_eq!(report.income.rows, ids(&["T1"]));
	assert_eq!(report.expenses.rows, ids(&["T2", "T3", "T4", "T5b", "T8"]));
	assert_eq!(
		report.net_profit.rows,
		ids(&["T1", "T2", "T3", "T4", "T5b", "T8"])
	);
	assert_eq!(report.debt_paydown.rows, ids(&["T5a"]));
	assert_eq!(report.savings.rows, ids(&["T6"]));
	assert_eq!(
		report.net_cash_flow.rows,
		ids(&["T1", "T2", "T3", "T4", "T5a", "T5b", "T6", "T8"])
	);
	let clothing = &report.expenses.categories[1];
	assert_eq!(clothing.rows, ids(&["T3", "T4"]));
}

#[test]
fn st1_1_transfers_starting_balances_and_adjustments_never_appear() {
	let report = master().net_profit(month("2027-01"));
	let every_row: Vec<_> = [
		&report.income.rows,
		&report.expenses.rows,
		&report.debt_paydown.rows,
		&report.savings.rows,
		&report.unreviewed.rows,
	]
	.into_iter()
	.flatten()
	.map(TransactionId::as_str)
	.collect();
	for excluded in ["T5", "T5m", "T6m", "T7", "T8m", "T9", "T10"] {
		assert!(!every_row.contains(&excluded), "{excluded}");
	}
}

#[test]
fn st1_master_february_counts_the_swipe_once_and_never_the_card_payment() {
	let report = master().net_profit(month("2027-02"));
	assert_eq!(totals(&report), [300_000, 2_500, 297_500, 0, 0, 297_500, 0]);
	assert_eq!(report.expenses.rows, ids(&["T12"]));
}

#[test]
fn plan10_a_400_loan_payment_is_80_expense_and_320_paydown() {
	let mut book = master_book();
	book.parent("Pay", "CHK", "2027-03-10", -40_000).payee = Some(PayeeId::new("Lender"));
	book.transfer("Principal", "CHK", "LOAN", "2027-03-10", -32_000)
		.in_category("Loan principal")
		.child_of("Pay");
	book.row("Interest", "CHK", "2027-03-10", -8_000)
		.in_category("Loan interest")
		.child_of("Pay")
		.payee = Some(PayeeId::new("Lender"));

	let report = book.net_profit(month("2027-03"));
	assert_eq!(report.expenses.total.minor(), 8_000);
	assert_eq!(report.expenses.rows, ids(&["Interest"]));
	assert_eq!(report.debt_paydown.total.minor(), 32_000);
	assert_eq!(report.debt_paydown.rows, ids(&["Principal"]));
	assert_eq!(report.net_profit.total.minor(), -8_000);
	assert_eq!(report.net_cash_flow.total.minor(), -40_000);
}

#[test]
fn plan10_a_50_card_swipe_is_groceries_in_the_swipe_month_and_the_payment_never_spends() {
	let mut book = master_book();
	book.row("Swipe", "VISA", "2027-03-28", -5_000)
		.in_category("Groceries");
	book.transfer("PayCard", "CHK", "VISA", "2027-04-05", -5_000);

	let march = book.net_profit(month("2027-03"));
	assert_eq!(march.expenses.total.minor(), 5_000);
	assert_eq!(
		category_totals(&march.expenses),
		[("Groceries".to_owned(), 5_000)]
	);
	let april = book.net_profit(month("2027-04"));
	assert_eq!(totals(&april), [0; 7]);
	let both = book.net_profit(
		Period::new(
			CivilDate::new(2027, 3, 1).unwrap(),
			CivilDate::new(2027, 4, 30).unwrap(),
		)
		.unwrap(),
	);
	assert_eq!(both.expenses.rows, ids(&["Swipe"]));
}

#[test]
fn cls8_c1_c2_refunds_reduce_expenses_and_can_go_negative() {
	let mut book = master_book();
	book.row("Buy", "VISA", "2027-03-28", -10_000)
		.in_category("Clothing");
	book.row("Refund", "VISA", "2027-04-05", 10_000)
		.in_category("Clothing");
	assert_eq!(
		book.net_profit(month("2027-03")).expenses.total.minor(),
		10_000
	);
	let april = book.net_profit(month("2027-04"));
	assert_eq!(april.expenses.total.minor(), -10_000);
	assert_eq!(april.income.total.minor(), 0, "a refund is never income");

	book.row("More", "VISA", "2027-04-10", -2_000)
		.in_category("Clothing");
	assert_eq!(
		book.net_profit(month("2027-04")).expenses.total.minor(),
		-8_000
	);
}

#[test]
fn st7_s7_unreviewed_transfer_sits_below_net_cash_flow() {
	let mut book = master_book();
	book.row("Pay", "CHK", "2027-05-01", 100_000)
		.in_category("Paycheck");
	book.row("Food", "CHK", "2027-05-02", -20_000)
		.in_category("Groceries");
	book.transfer("Move", "CHK", "BRK", "2027-05-03", -10_000);
	let report = book.net_profit(month("2027-05"));
	assert_eq!(
		totals(&report),
		[100_000, 20_000, 80_000, 0, 0, 80_000, 10_000]
	);
}

#[test]
fn st8_x9_unreviewed_inflow_is_negative() {
	let mut book = master_book();
	book.transfer("In", "CHK", "BRK", "2027-05-03", 10_000);
	assert_eq!(
		book.net_profit(month("2027-05")).unreviewed.total.minor(),
		-10_000
	);
}

#[test]
fn st7_s8_unpaired_savings_transfer_is_still_savings() {
	let mut book = master_book();
	book.row("U", "CHK", "2027-05-03", -10_000)
		.in_category("Savings")
		.payee = Some(Book::transfer_payee("SAV"));
	let report = book.net_profit(month("2027-05"));
	assert_eq!(totals(&report), [0, 0, 0, 0, 10_000, -10_000, 0]);
}

#[test]
fn st7_s6_zero_history_period_is_all_zero() {
	assert_eq!(totals(&master().net_profit(month("2030-01"))), [0; 7]);
}

/// DEBT-6 storage: a split parent in CHK, a principal child that transfers to
/// LOAN, and one expense child per other part.
fn loan_payment(book: &mut Book, id: &str, on: &str, principal: i64, parts: &[(&str, i64)]) {
	let total = -principal + parts.iter().map(|(_, amount)| amount).sum::<i64>();
	book.parent(id, "CHK", on, total);
	book.transfer(&format!("{id}p"), "CHK", "LOAN", on, -principal)
		.in_category("Loan principal")
		.child_of(id);
	for (index, (category, amount)) in parts.iter().enumerate() {
		book.row(&format!("{id}-{index}"), "CHK", on, *amount)
			.in_category(category)
			.child_of(id);
	}
}

#[test]
fn debt5_l4_l5_l6_escrow_fees_and_extra_principal() {
	let mut book = master_book();
	book.category("Escrow", "Debt costs", None);
	book.category("Loan fees", "Debt costs", None);
	loan_payment(
		&mut book,
		"L4",
		"2027-06-01",
		60_000,
		&[("Loan interest", -100_000), ("Escrow", -40_000)],
	);
	loan_payment(
		&mut book,
		"L5",
		"2027-07-01",
		32_000,
		&[("Loan interest", -8_000), ("Loan fees", -2_500)],
	);
	loan_payment(
		&mut book,
		"L6",
		"2027-08-01",
		42_000,
		&[("Loan interest", -8_000)],
	);
	let june = book.net_profit(month("2027-06"));
	assert_eq!(
		(june.expenses.total.minor(), june.debt_paydown.total.minor()),
		(140_000, 60_000)
	);
	assert_eq!(june.net_cash_flow.total.minor(), -200_000);
	let july = book.net_profit(month("2027-07"));
	assert_eq!(
		(july.expenses.total.minor(), july.debt_paydown.total.minor()),
		(10_500, 32_000)
	);
	let august = book.net_profit(month("2027-08"));
	assert_eq!(
		(
			august.expenses.total.minor(),
			august.debt_paydown.total.minor()
		),
		(8_000, 42_000)
	);
}

#[test]
fn debt7_credit_card_events() {
	let mut book = master_book();
	book.category("Card interest", "Debt costs", None);
	book.row("Swipe", "VISA", "2027-06-02", -5_000)
		.in_category("Groceries");
	book.transfer("Pay", "CHK", "VISA", "2027-06-20", -5_000);
	book.row("Refund", "VISA", "2027-06-10", 1_500)
		.in_category("Clothing");
	book.row("Fee", "VISA", "2027-06-15", -2_000)
		.in_category("Card interest");
	book.transfer("Advance", "VISA", "CHK", "2027-06-16", -10_000);
	book.transfer("FromSavings", "VISA", "SAV", "2027-06-25", 5_000)
		.in_category("Savings");

	let report = book.net_profit(month("2027-06"));
	assert_eq!(report.expenses.total.minor(), 5_000 - 1_500 + 2_000);
	assert_eq!(report.income.total.minor(), 0);
	assert_eq!(report.savings.total.minor(), -5_000, "a withdrawal");
	assert_eq!(report.net_cash_flow.total.minor(), -5_500 + 5_000);
	assert_eq!(report.unreviewed.total.minor(), 0);
}

#[test]
fn r_rep_3_hidden_archived_deleted_and_closed_goal_categories_are_labeled() {
	let mut book = master_book();
	book.group("Goals", false);
	book.category("Laptop", "Goals", None);
	book.category("Old Gym", "Shopping", None);
	book.category("Gone", "Shopping", None);
	book.budget
		.categories
		.get_mut(&cat("Laptop"))
		.unwrap()
		.hidden = true;
	book.budget.goals.insert(
		GoalId::new("laptop"),
		Goal {
			id: GoalId::new("laptop"),
			category: cat("Laptop"),
			target: Money::from_minor(150_000),
			target_date: None,
			icon: None,
			color: None,
			status: GoalStatus::Closed,
			reached_month: None,
			closed_month: None,
			released_to: None,
			released: None,
			activity_seen_through: None,
			activity_seen_total: None,
			close_month_dismissed_available: None,
		},
	);
	book.meta("Old Gym", CategoryKind::Expense);
	book.budget
		.category_meta
		.get_mut(&cat("Old Gym"))
		.unwrap()
		.archived_on = Some(CivilDate::new(2027, 6, 1).unwrap());
	book.budget
		.categories
		.get_mut(&cat("Gone"))
		.unwrap()
		.tombstone = true;
	book.row("Buy", "CHK", "2027-02-10", -148_000)
		.in_category("Laptop");
	book.row("Gym", "CHK", "2027-02-11", -3_000)
		.in_category("Old Gym");
	book.row("Old", "CHK", "2027-02-12", -1_000)
		.in_category("Gone");

	let report = book.net_profit(
		Period::new(
			CivilDate::new(2027, 1, 1).unwrap(),
			CivilDate::new(2027, 12, 31).unwrap(),
		)
		.unwrap(),
	);
	assert_eq!(report.expenses.total.minor(), 152_000);
	let find = |name: &str| {
		report
			.expenses
			.categories
			.iter()
			.find(|line| line.category == Some(cat(name)))
			.unwrap()
	};
	let laptop = find("Laptop");
	assert!(laptop.labels.hidden && laptop.labels.closed_goal);
	assert_eq!(laptop.total.minor(), 148_000);
	assert!(find("Old Gym").labels.archived);
	assert!(find("Gone").labels.deleted);
}

#[test]
fn uncategorized_spending_is_its_own_line_after_every_category() {
	let mut book = master_book();
	book.row("A", "CHK", "2027-02-01", -1_000);
	book.row("B", "CHK", "2027-02-02", -2_000)
		.in_category("Car purchase");
	book.row("C", "CHK", "2027-02-03", -3_000)
		.in_category("Groceries");
	let report = book.net_profit(month("2027-02"));
	assert_eq!(
		category_totals(&report.expenses),
		[
			("Groceries".to_owned(), 3_000),
			("Car purchase".to_owned(), 2_000),
			("(none)".to_owned(), 1_000),
		]
	);
}

#[test]
fn an_overflowing_period_is_an_error_not_a_panic() {
	let mut book = master_book();
	book.row("A", "CHK", "2027-02-01", i64::MAX)
		.in_category("Paycheck");
	book.row("B", "CHK", "2027-02-02", 1)
		.in_category("Paycheck");
	let result = veritas_reports::net_profit(&book.budget, &book.classify(), month("2027-02"));
	assert!(matches!(result, Err(veritas_domain::CoreError::Overflow)));
}
