//! R-REP-1: every worked example in docs/finance/classification.md, by case id.

mod support;

use support::*;
use veritas_domain::{
	CategoryGroupId, CategoryKind, PayeeId, SplitRole, TransactionId, TransactionKind::*,
};
use veritas_reports::{AdjustmentSource, Decision, Flags, Leg};

#[test]
fn cls8_master_january_rows_land_on_their_decision_rows() {
	let classification = master().classify();
	let expected = [
		("T1", 12, Income, Leg::Primary),
		("T2", 13, Expense, Leg::Primary),
		("T3", 13, Expense, Leg::Primary),
		("T4", 13, Expense, Leg::Primary),
		("T5a", 6, DebtPrincipal, Leg::Primary),
		("T5b", 13, Expense, Leg::Primary),
		("T5m", 6, DebtPrincipal, Leg::Mirror),
		("T6", 7, SavingsContribution, Leg::Primary),
		("T6m", 7, SavingsContribution, Leg::Mirror),
		("T7", 2, StartingBalance, Leg::Primary),
		("T8", 8, Expense, Leg::Primary),
		("T8m", 8, Expense, Leg::Mirror),
		("T9", 11, Adjustment, Leg::Primary),
		("T10", 3, Adjustment, Leg::Primary),
	];
	for (id, table_row, kind, leg) in expected {
		assert_row(&classification, id, table_row, kind, leg);
		assert_eq!(row(&classification, id).flags, Flags::default(), "{id}");
	}
	assert!(
		classification.get(&TransactionId::new("T5")).is_none(),
		"a split parent is never classified (row 1)"
	);
	assert_eq!(
		row(&classification, "T9").adjustment_source(),
		Some(AdjustmentSource::OffBudgetRevaluation)
	);
	assert_eq!(
		row(&classification, "T10").adjustment_source(),
		Some(AdjustmentSource::Reconciliation)
	);
}

#[test]
fn cls8_master_february_card_payment_is_an_internal_transfer_on_both_legs() {
	let classification = master().classify();
	assert_row(&classification, "T11", 12, Income, Leg::Primary);
	assert_row(&classification, "T12", 13, Expense, Leg::Primary);
	assert_row(&classification, "T13", 4, InternalTransfer, Leg::Primary);
	assert_row(&classification, "T13m", 4, InternalTransfer, Leg::Primary);
	assert!(!row(&classification, "T13").flags.needs_review);
}

#[test]
fn cls8_c3_a_categorized_starting_balance_is_never_income() {
	let mut book = master_book();
	book.account("NEW", false);
	book.row("SB", "NEW", "2027-01-02", 50_000)
		.in_category("Starting Balances")
		.starting_balance = true;
	let classification = book.classify();
	assert_row(&classification, "SB", 2, StartingBalance, Leg::Primary);
	assert_eq!(book.net_profit(month("2027-01")).income.total.minor(), 0);
}

#[test]
fn cls8_c4_principal_category_without_a_transfer_is_misfiled_expense() {
	let mut book = master_book();
	book.row("P", "CHK", "2027-01-10", -32_000)
		.in_category("Loan principal");
	let classification = book.classify();
	assert_row(&classification, "P", 14, Expense, Leg::Primary);
	assert!(row(&classification, "P").flags.misfiled);
	let report = book.net_profit(month("2027-01"));
	assert_eq!(report.expenses.total.minor(), 32_000);
	assert_eq!(report.debt_paydown.total.minor(), 0);
}

#[test]
fn cls8_c5_uncategorized_positive_amount_reduces_expense() {
	let mut book = master_book();
	book.row("U", "CHK", "2027-01-10", 4_000);
	let classification = book.classify();
	assert_row(&classification, "U", 15, Expense, Leg::Primary);
	assert!(row(&classification, "U").flags.needs_review);
	let report = book.net_profit(month("2027-01"));
	assert_eq!(report.expenses.total.minor(), -4_000);
	assert_eq!(report.income.total.minor(), 0);
}

#[test]
fn cls8_c6_uncategorized_on_to_off_transfer_is_unreviewed_not_profit() {
	let mut book = master_book();
	book.transfer("X", "CHK", "BRK", "2027-01-10", -10_000);
	let classification = book.classify();
	assert_row(&classification, "X", 10, InternalTransfer, Leg::Primary);
	assert!(row(&classification, "X").flags.needs_review);
	assert_row(&classification, "Xm", 10, InternalTransfer, Leg::Mirror);
	let report = book.net_profit(month("2027-01"));
	assert_eq!(report.net_profit.total.minor(), 0);
	assert_eq!(report.net_cash_flow.total.minor(), 0);
	assert_eq!(report.unreviewed.total.minor(), 10_000);
	assert_eq!(report.unreviewed.rows, ids(&["X"]));
}

#[test]
fn cls8_c7_unmarked_on_to_off_category_counts_as_spending_until_reviewed() {
	let mut book = master_book();
	book.category("Car Loan", "Debt", None);
	book.transfer("L", "CHK", "LOAN", "2027-01-10", -32_000)
		.in_category("Car Loan");
	let classification = book.classify();
	assert_row(&classification, "L", 8, Expense, Leg::Primary);
	let counted: Vec<_> = classification
		.transfers_counted_as_spending(&book.budget)
		.map(|row| row.id.as_str())
		.collect();
	assert_eq!(counted, ["L"], "CLS-4.2 review count");

	book.meta("Car Loan", CategoryKind::DebtPrincipal);
	let classification = book.classify();
	assert_row(&classification, "L", 6, DebtPrincipal, Leg::Primary);
	assert_eq!(
		classification
			.transfers_counted_as_spending(&book.budget)
			.count(),
		0
	);
}

#[test]
fn cls8_c8_same_side_transfer_changes_no_flow_total() {
	let mut book = master_book();
	book.transfer("V", "CHK", "VISA", "2027-01-10", -7_500);
	let classification = book.classify();
	assert_row(&classification, "V", 4, InternalTransfer, Leg::Primary);
	assert_row(&classification, "Vm", 4, InternalTransfer, Leg::Primary);
	let report = book.net_profit(month("2027-01"));
	assert_eq!(report.net_cash_flow, Default::default());
	assert_eq!(report.unreviewed, Default::default());
}

#[test]
fn cls8_c9_hidden_category_activity_is_included_and_labeled() {
	let mut book = master_book();
	book.category("Old Gym", "Shopping", None);
	book.budget
		.categories
		.get_mut(&cat("Old Gym"))
		.unwrap()
		.hidden = true;
	book.row("G", "CHK", "2027-01-04", -3_000)
		.in_category("Old Gym");
	assert_row(&book.classify(), "G", 13, Expense, Leg::Primary);
	let report = book.net_profit(month("2027-01"));
	assert_eq!(report.expenses.total.minor(), 3_000);
	let line = &report.expenses.categories[0];
	assert_eq!(line.category, Some(cat("Old Gym")));
	assert!(line.labels.hidden);
}

#[test]
fn cls8_c10_kind_conflict_keeps_actuals_income_expense_axis() {
	let mut book = master_book();
	book.category("Side gig", "Shopping", Some(CategoryKind::Income));
	book.row("K", "CHK", "2027-01-04", 5_000)
		.in_category("Side gig");
	let classification = book.classify();
	assert_row(&classification, "K", 13, Expense, Leg::Primary);
	assert!(row(&classification, "K").flags.kind_conflict);
}

#[test]
fn cls3_1_income_category_wins_over_expense_metadata() {
	let mut book = master_book();
	book.meta("Paycheck", CategoryKind::Savings);
	book.row("P", "CHK", "2027-01-01", 300_000)
		.in_category("Paycheck");
	let classification = book.classify();
	assert_row(&classification, "P", 12, Income, Leg::Primary);
	assert!(row(&classification, "P").flags.kind_conflict);
}

#[test]
fn cls8_c11_split_children_classify_and_the_parent_does_not() {
	let mut book = master_book();
	book.category("Dining", "Food", None);
	book.parent("S", "CHK", "2027-01-04", -10_000);
	book.row("S1", "CHK", "2027-01-04", -7_000)
		.in_category("Groceries")
		.child_of("S");
	book.row("S2", "CHK", "2027-01-04", -3_000)
		.in_category("Dining")
		.child_of("S");
	let classification = book.classify();
	assert_row(&classification, "S1", 13, Expense, Leg::Primary);
	assert_row(&classification, "S2", 13, Expense, Leg::Primary);
	assert!(classification.get(&TransactionId::new("S")).is_none());
	let total: i64 = classification
		.rows()
		.iter()
		.map(|row| row.amount.minor())
		.sum();
	let opening = 200_000 + 500_000 - 980_000 + 1_000_000;
	assert_eq!(total - opening, -10_000);
}

#[test]
fn cls8_c12_mirror_leg_copies_its_primary_and_is_never_row_10() {
	let classification = master().classify();
	let mirror = row(&classification, "T5m");
	assert_eq!(mirror.category, None);
	assert_eq!(mirror.decision, Decision::DebtPrincipalTransfer);
	assert_eq!(mirror.leg, Leg::Mirror);
	assert_eq!(mirror.flags, Flags::default());
}

#[test]
fn cls8_c13_unpaired_transfer_classifies_from_transfer_acct() {
	let mut book = master_book();
	book.row("U", "CHK", "2027-01-10", -10_000)
		.in_category("Savings")
		.payee = Some(Book::transfer_payee("BRK"));
	let classification = book.classify();
	assert_row(&classification, "U", 7, SavingsContribution, Leg::Primary);
	assert!(row(&classification, "U").flags.unpaired);
	let report = book.net_profit(month("2027-01"));
	assert_eq!(report.savings.total.minor(), 10_000);
	assert_eq!(report.unreviewed.total.minor(), 0);
}

#[test]
fn cls8_c14_merged_category_classifies_under_its_target() {
	let mut book = master_book();
	book.category("Dining", "Food", None);
	book.category("Dining Old", "Food", Some(CategoryKind::Savings));
	book.budget
		.categories
		.get_mut(&cat("Dining Old"))
		.unwrap()
		.tombstone = true;
	book.budget
		.category_mapping
		.insert(cat("Dining Old"), cat("Dining"));
	book.row("D", "CHK", "2027-01-04", -2_000)
		.in_category("Dining Old");
	let classification = book.classify();
	assert_row(&classification, "D", 13, Expense, Leg::Primary);
	assert_eq!(row(&classification, "D").category, Some(cat("Dining")));
}

#[test]
fn cls8_c15_children_of_tombstoned_or_missing_parents_are_not_rows() {
	let mut book = master_book();
	book.parent("P", "CHK", "2027-01-04", -1_000).tombstone = true;
	book.row("P1", "CHK", "2027-01-04", -1_000)
		.in_category("Groceries")
		.child_of("P");
	book.row("Q1", "CHK", "2027-01-04", -1_000)
		.in_category("Groceries")
		.child_of("missing");
	book.row("Dead", "CHK", "2027-01-04", -1_000)
		.in_category("Groceries")
		.tombstone = true;
	let classification = book.classify();
	for id in ["P", "P1", "Q1", "Dead"] {
		assert!(
			classification.get(&TransactionId::new(id)).is_none(),
			"{id}"
		);
	}
	assert_eq!(book.net_profit(month("2027-01")).expenses.total.minor(), 0);
}

#[test]
fn cls10_c16_gross_paycheck_splits_into_income_taxes_and_savings() {
	let mut book = master_book();
	book.account("401k", true).account("HSA", true);
	book.group("Taxes and deductions", false);
	for category in ["Federal tax", "State tax", "FICA", "Health premium"] {
		book.category(category, "Taxes and deductions", None);
	}
	book.category("Retirement", "Saving", Some(CategoryKind::Savings));
	book.category("HSA contributions", "Saving", Some(CategoryKind::Savings));

	book.parent("Pay", "CHK", "2027-01-15", 234_400);
	for (id, amount, category) in [
		("Gross", 400_000, "Paycheck"),
		("Fed", -50_000, "Federal tax"),
		("State", -20_000, "State tax"),
		("FICA", -30_600, "FICA"),
		("Premium", -15_000, "Health premium"),
	] {
		book.row(id, "CHK", "2027-01-15", amount)
			.in_category(category)
			.child_of("Pay");
	}
	book.transfer("Ret", "CHK", "401k", "2027-01-15", -40_000)
		.in_category("Retirement")
		.child_of("Pay");
	book.transfer("Hsa", "CHK", "HSA", "2027-01-15", -10_000)
		.in_category("HSA contributions")
		.child_of("Pay");

	let classification = book.classify();
	assert_row(&classification, "Gross", 12, Income, Leg::Primary);
	for id in ["Fed", "State", "FICA", "Premium"] {
		assert_row(&classification, id, 13, Expense, Leg::Primary);
	}
	assert_row(&classification, "Ret", 7, SavingsContribution, Leg::Primary);
	assert_row(&classification, "Retm", 7, SavingsContribution, Leg::Mirror);
	assert_row(&classification, "Hsa", 7, SavingsContribution, Leg::Primary);
	assert_row(&classification, "Hsam", 7, SavingsContribution, Leg::Mirror);

	let report = book.net_profit(month("2027-01"));
	assert_eq!(report.income.total.minor(), 400_000);
	assert_eq!(report.expenses.total.minor(), 115_600);
	assert_eq!(report.net_profit.total.minor(), 284_400);
	assert_eq!(report.savings.total.minor(), 50_000);
	assert_eq!(
		report.net_cash_flow.total.minor(),
		234_400,
		"the net deposit"
	);
}

#[test]
fn cls10_retirement_child_without_an_account_is_misfiled() {
	let mut book = master_book();
	book.category("Retirement", "Saving", Some(CategoryKind::Savings));
	book.parent("Pay", "CHK", "2027-01-15", 360_000);
	book.row("Gross", "CHK", "2027-01-15", 400_000)
		.in_category("Paycheck")
		.child_of("Pay");
	book.row("Ret", "CHK", "2027-01-15", -40_000)
		.in_category("Retirement")
		.child_of("Pay");
	let classification = book.classify();
	assert_row(&classification, "Ret", 14, Expense, Leg::Primary);
	assert!(row(&classification, "Ret").flags.misfiled);
}

#[test]
fn cls4_row_4_and_5_flag_a_category_on_a_same_side_transfer() {
	let mut book = master_book();
	book.transfer("OnOn", "CHK", "VISA", "2027-01-04", -1_000)
		.in_category("Groceries");
	book.transfer("OffOff", "SAV", "BRK", "2027-01-04", -1_000);
	let classification = book.classify();
	assert_row(&classification, "OnOn", 4, InternalTransfer, Leg::Primary);
	assert!(row(&classification, "OnOn").flags.needs_review);
	assert!(!row(&classification, "OnOnm").flags.needs_review);
	assert_row(&classification, "OffOff", 5, InternalTransfer, Leg::Primary);
	assert_row(
		&classification,
		"OffOffm",
		5,
		InternalTransfer,
		Leg::Primary,
	);
	assert!(!row(&classification, "OffOff").flags.needs_review);
	assert_eq!(
		book.net_profit(month("2027-01")).net_cash_flow,
		Default::default()
	);
}

#[test]
fn cls4_row_9_income_category_on_an_on_to_off_transfer_is_income() {
	let mut book = master_book();
	book.transfer("Sold", "CHK", "CAR", "2027-01-04", 20_000)
		.in_category("Paycheck");
	let classification = book.classify();
	assert_row(&classification, "Sold", 9, Income, Leg::Primary);
	assert_row(&classification, "Soldm", 9, Income, Leg::Mirror);
	assert_eq!(
		book.net_profit(month("2027-01")).income.total.minor(),
		20_000
	);
}

#[test]
fn cls4_row_11_off_budget_interest_is_an_adjustment_not_income() {
	let mut book = master_book();
	book.row("Int", "SAV", "2027-01-31", 900)
		.in_category("Paycheck");
	let classification = book.classify();
	assert_row(&classification, "Int", 11, Adjustment, Leg::Primary);
	assert_eq!(book.net_profit(month("2027-01")).income.total.minor(), 0);
}

#[test]
fn cls1_off_budget_leg_without_its_on_budget_partner_needs_review() {
	let mut book = master_book();
	book.row("Orphan", "LOAN", "2027-01-10", 32_000).payee = Some(Book::transfer_payee("CHK"));
	let classification = book.classify();
	assert_row(
		&classification,
		"Orphan",
		10,
		InternalTransfer,
		Leg::Primary,
	);
	let flags = row(&classification, "Orphan").flags;
	assert!(flags.unpaired && flags.needs_review);
	let report = book.net_profit(month("2027-01"));
	assert_eq!(
		report.unreviewed.total.minor(),
		0,
		"only on-budget rows are unreviewed"
	);
}

#[test]
fn cls1_a_partner_in_the_wrong_account_leaves_the_row_unpaired() {
	let mut book = master_book();
	book.transfer("V", "CHK", "VISA", "2027-01-10", -7_500);
	book.get("Vm").account = veritas_domain::AccountId::new("SAV");
	let classification = book.classify();
	assert_row(&classification, "V", 4, InternalTransfer, Leg::Primary);
	assert!(row(&classification, "V").flags.unpaired);
	let report = book.net_profit(month("2027-01"));
	assert_eq!(report.unreviewed.total.minor(), 7_500);
}

#[test]
fn cls1_transfer_payees_resolve_through_payee_mapping() {
	let mut book = master_book();
	book.budget
		.payee_mapping
		.insert(PayeeId::new("old-sav"), Book::transfer_payee("SAV"));
	book.transfer("S", "CHK", "SAV", "2027-01-12", -20_000)
		.in_category("Savings")
		.payee = Some(PayeeId::new("old-sav"));
	assert_row(&book.classify(), "S", 7, SavingsContribution, Leg::Primary);
}

#[test]
fn cls5_reconciliation_markers() {
	let mut book = master_book();
	book.row("Actual", "CHK", "2027-01-31", -1_200).notes =
		Some(veritas_reports::RECONCILIATION_NOTE.to_owned());
	book.row("Veritas", "CHK", "2027-01-31", 300)
		.in_category("Groceries");
	book.mark_adjustment("Veritas");
	book.row("Off", "BRK", "2027-01-31", 300).notes =
		Some(veritas_reports::RECONCILIATION_NOTE.to_owned());
	book.row("Uncleared", "CHK", "2027-01-31", -100).notes =
		Some(veritas_reports::RECONCILIATION_NOTE.to_owned());
	book.get("Uncleared").cleared = false;
	book.row("Ruled", "CHK", "2027-01-31", -100)
		.in_category("Groceries")
		.notes = Some(veritas_reports::RECONCILIATION_NOTE.to_owned());
	book.row("Translated", "CHK", "2027-01-31", -100).notes =
		Some("Ajuste de saldo de conciliación".to_owned());

	let classification = book.classify();
	for id in ["Actual", "Veritas", "Off"] {
		assert_row(&classification, id, 3, Adjustment, Leg::Primary);
		assert_eq!(
			row(&classification, id).adjustment_source(),
			Some(AdjustmentSource::Reconciliation)
		);
	}
	assert_row(&classification, "Uncleared", 15, Expense, Leg::Primary);
	assert_row(&classification, "Ruled", 13, Expense, Leg::Primary);
	assert_row(&classification, "Translated", 15, Expense, Leg::Primary);
	assert!(row(&classification, "Translated").flags.needs_review);
}

#[test]
fn cls1_orphaned_row_is_not_classified_and_counts_nowhere() {
	let mut book = master_book();
	book.row("Ghost", "GONE", "2027-01-04", -500)
		.in_category("Groceries");
	let classification = book.classify();
	assert!(classification.get(&TransactionId::new("Ghost")).is_none());
	assert_eq!(classification.orphans(), ids(&["Ghost"]));
	assert_eq!(book.net_profit(month("2027-01")).expenses.total.minor(), 0);
}

#[test]
fn cls1_missing_transfer_account_is_an_unpaired_transfer_toward_off_budget() {
	let mut book = master_book();
	book.account("GONE", false);
	book.transfer("T", "CHK", "GONE", "2027-01-04", -2_000)
		.in_category("Savings");
	book.budget
		.accounts
		.remove(&veritas_domain::AccountId::new("GONE"));
	let classification = book.classify();
	assert_eq!(classification.orphans(), ids(&["Tm"]));
	assert_row(&classification, "T", 7, SavingsContribution, Leg::Primary);
	let flags = row(&classification, "T").flags;
	assert!(flags.unpaired && !flags.needs_review);
	assert_eq!(
		book.net_profit(month("2027-01")).savings.total.minor(),
		2_000
	);
}

#[test]
fn cls1_one_way_link_leaves_both_legs_unpaired() {
	let mut book = master_book();
	book.transfer("V", "CHK", "VISA", "2027-01-10", -7_500);
	book.row("Other", "VISA", "2027-01-10", 7_500).payee = Some(Book::transfer_payee("CHK"));
	book.get("Vm").transfer = Some(TransactionId::new("Other"));
	let classification = book.classify();
	for id in ["V", "Vm", "Other"] {
		assert_row(&classification, id, 4, InternalTransfer, Leg::Primary);
		assert!(row(&classification, id).flags.unpaired, "{id}");
	}
}

#[test]
fn cls1_one_way_link_on_to_off_has_no_mirror() {
	let mut book = master_book();
	book.transfer("S", "CHK", "SAV", "2027-01-12", -20_000)
		.in_category("Savings");
	book.get("Sm").transfer = None;
	let classification = book.classify();
	assert_row(&classification, "S", 7, SavingsContribution, Leg::Primary);
	assert!(row(&classification, "S").flags.unpaired);
	assert_row(&classification, "Sm", 10, InternalTransfer, Leg::Primary);
	let flags = row(&classification, "Sm").flags;
	assert!(flags.unpaired && flags.needs_review);
}

#[test]
fn cls1_a_split_parent_is_never_a_partner() {
	let mut book = master_book();
	book.transfer("V", "CHK", "VISA", "2027-01-10", -7_500);
	book.get("Vm").split = SplitRole::Parent;
	let classification = book.classify();
	assert!(classification.get(&TransactionId::new("Vm")).is_none());
	assert!(row(&classification, "V").flags.unpaired);
}

#[test]
fn cls1_deleted_category_without_a_target_still_classifies_and_is_labeled() {
	let mut book = master_book();
	book.category("Gym", "Shopping", None);
	book.budget
		.categories
		.get_mut(&cat("Gym"))
		.unwrap()
		.tombstone = true;
	book.row("G", "CHK", "2027-01-04", -3_000)
		.in_category("Gym");
	let classification = book.classify();
	assert_row(&classification, "G", 13, Expense, Leg::Primary);
	assert_eq!(row(&classification, "G").category, Some(cat("Gym")));
	let report = book.net_profit(month("2027-01"));
	assert!(report.expenses.categories[0].labels.deleted);
}

#[test]
fn cls1_a_category_with_no_mapping_row_is_uncategorized() {
	let mut book = master_book();
	book.budget.category_mapping.remove(&cat("Groceries"));
	book.row("G", "CHK", "2027-01-04", -3_000)
		.in_category("Groceries");
	let classification = book.classify();
	assert_row(&classification, "G", 15, Expense, Leg::Primary);
	assert_eq!(row(&classification, "G").category, None);
}

#[test]
fn cls4_rows_2_and_3_yield_to_a_paired_same_side_transfer() {
	let mut book = master_book();
	book.transfer("V", "CHK", "VISA", "2027-01-10", -7_500)
		.starting_balance = true;
	let classification = book.classify();
	for id in ["V", "Vm"] {
		assert_row(&classification, id, 4, InternalTransfer, Leg::Primary);
		let flags = row(&classification, id).flags;
		assert!(flags.needs_review && !flags.unpaired, "{id}");
	}
}

#[test]
fn cls4_rows_2_and_3_yield_to_a_paired_on_off_transfer_flagging_the_primary() {
	let mut book = master_book();
	book.transfer("S", "CHK", "SAV", "2027-01-12", -20_000)
		.in_category("Savings");
	book.mark_adjustment("Sm");
	let classification = book.classify();
	assert_row(&classification, "S", 7, SavingsContribution, Leg::Primary);
	assert!(row(&classification, "S").flags.needs_review);
	assert_row(&classification, "Sm", 7, SavingsContribution, Leg::Mirror);
	assert_eq!(row(&classification, "Sm").flags, Flags::default());
	assert_eq!(
		book.net_profit(month("2027-01")).savings.total.minor(),
		20_000
	);
}

#[test]
fn cls4_rows_2_and_3_still_apply_to_an_unpaired_transfer() {
	let mut book = master_book();
	book.row("SB", "CHK", "2027-01-02", 50_000).payee = Some(Book::transfer_payee("VISA"));
	book.get("SB").starting_balance = true;
	book.row("Adj", "VISA", "2027-01-31", -300).payee = Some(Book::transfer_payee("SAV"));
	book.mark_adjustment("Adj");
	let classification = book.classify();
	assert_row(&classification, "SB", 2, StartingBalance, Leg::Primary);
	assert_row(&classification, "Adj", 3, Adjustment, Leg::Primary);
	for id in ["SB", "Adj"] {
		let flags = row(&classification, id).flags;
		assert!(flags.needs_review && flags.unpaired, "{id}");
	}
}

#[test]
fn cls3_2_hiding_a_category_or_its_group_never_reclassifies() {
	let mut book = master_book();
	book.row("G", "VISA", "2027-01-03", -5_000)
		.in_category("Groceries");
	let before = book.classify();
	book.budget
		.category_groups
		.get_mut(&CategoryGroupId::new("Food"))
		.unwrap()
		.hidden = true;
	let after = book.classify();
	assert_eq!(before.rows(), after.rows());
	assert!(
		book.net_profit(month("2027-01")).expenses.categories[0]
			.labels
			.hidden
	);
}

#[test]
fn cls1_rows_are_sorted_by_date_then_id_and_split_children_know_their_parent() {
	let classification = master().classify();
	let rows = classification.rows();
	assert!(
		rows.windows(2)
			.all(|pair| (pair[0].date, &pair[0].id) < (pair[1].date, &pair[1].id))
	);
	assert_eq!(
		row(&classification, "T5a").parent,
		Some(TransactionId::new("T5"))
	);
	assert!(matches!(
		master().budget.transactions[&TransactionId::new("T5")].split,
		SplitRole::Parent
	));
}
