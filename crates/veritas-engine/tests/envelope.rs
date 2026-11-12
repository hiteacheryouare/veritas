//! R-BUD-1: the envelope sheet reproduces Actual's numbers. The rules come
//! from docs/actual-format.md §5.2–§5.7; the first test replays the oracle's
//! own `apply` test (tools/actual-oracle/test/apply.test.ts) by hand.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;
use veritas_domain::{CategoryGroupId, CoreError, Money, SplitRole, TransactionId};
use veritas_engine::EnvelopeSheet;

#[test]
fn reproduces_the_oracle_apply_test() {
	let mut budget = small_fixture();
	budget.roll_over("2026-10", "2027-11", "groceries");
	budget.budget("2026-10", "groceries", 10_000);
	budget.txn(
		"late-dinner",
		"checking",
		"2026-10-30",
		-1_000,
		Some("dining"),
	);
	budget.hold("2026-10", 50_000);
	let sheet = budget.sheet(TODAY);

	let october = sheet.month(ym("2026-10")).unwrap();
	assert_eq!(october.summary.total_budgeted, cents(-174_000));
	assert_eq!(october.summary.manual_hold, cents(50_000));
	assert_eq!(october.summary.for_next_month, cents(50_000));
	assert_eq!(october.summary.available_funds, cents(847_343));
	assert_eq!(october.summary.last_month_overspent, cents(-657));
	assert_eq!(october.summary.to_budget, cents(622_686));
	let groceries = october.categories[&cat("groceries")];
	assert_eq!(
		(
			groceries.budgeted,
			groceries.spent,
			groceries.balance,
			groceries.carryover
		),
		(cents(10_000), cents(-12_657), cents(-2_657), true)
	);
	let dining = october.categories[&cat("dining")];
	assert_eq!(
		(
			dining.budgeted,
			dining.spent,
			dining.balance,
			dining.carryover
		),
		(cents(5_000), cents(-5_810), cents(-430), false)
	);

	let november = sheet.month(ym("2026-11")).unwrap();
	assert_eq!(
		november.categories[&cat("groceries")].balance,
		cents(-2_657),
		"carryover keeps the overspending"
	);
	assert_eq!(
		november.categories[&cat("dining")].balance,
		Money::ZERO,
		"overspending leaves the envelope"
	);
	assert_eq!(november.summary.last_month_overspent, cents(-430));
	assert_eq!(
		november.summary.from_last_month,
		cents(622_686 + 50_000),
		"held money comes back"
	);
	assert_eq!(november.summary.to_budget, cents(672_686 - 430));
}

#[test]
fn the_small_fixture_follows_the_worked_recurrence() {
	let sheet = small_fixture().sheet(TODAY);
	let to_budget = |m: &str| sheet.month(ym(m)).unwrap().summary.to_budget;
	assert_eq!(to_budget("2026-07"), Money::ZERO);
	assert_eq!(to_budget("2026-08"), cents(224_000));
	assert_eq!(
		to_budget("2026-09"),
		cents(224_000 + 400_000 - 657 - 176_000)
	);
	assert_eq!(
		to_budget("2026-10"),
		cents(447_343 + 400_000 - 657 - 176_000)
	);
	assert_eq!(to_budget("2027-11"), cents(670_686 - 657));

	let september = sheet.month(ym("2026-09")).unwrap();
	assert_eq!(
		september.categories[&cat("utilities")].balance,
		cents(1_154),
		"positive balances roll over"
	);
	assert_eq!(
		september.categories[&cat("groceries")].balance,
		cents(-657),
		"a deficit restarts at zero"
	);
	assert_eq!(september.summary.total_income, cents(400_000));
	assert_eq!(september.summary.total_spent, cents(-175_890));
	let everyday = september.groups[&CategoryGroupId::new("everyday")];
	assert_eq!(
		(everyday.budgeted, everyday.spent, everyday.balance),
		(cents(17_000), cents(-17_467), cents(-277))
	);
	assert_eq!(september.summary.total_balance, cents(1_154 - 657 + 380));
}

#[test]
fn groups_come_in_actuals_order_with_income_last() {
	let sheet = small_fixture().sheet(TODAY);
	let order: Vec<&str> = sheet.group_order().map(|(id, _)| id.as_str()).collect();
	let actual_order = [
		"usual",
		"savings-group",
		"bills-group",
		"everyday",
		"income",
	];
	assert_eq!(order, actual_order);
}

#[test]
fn created_months_run_from_three_before_the_first_transaction_to_twelve_after_today() {
	let sheet = small_fixture().sheet(TODAY);
	assert_eq!(sheet.months().first(), Some(&ym("2026-05")));
	assert_eq!(sheet.months().last(), Some(&ym("2027-11")));
	assert_eq!(sheet.months().len(), 19);
	assert!(sheet.month(ym("2026-04")).is_none());

	let empty = Budget::actual_seed().sheet(TODAY);
	assert_eq!(empty.months().first(), Some(&ym("2026-08")));

	let mut future = Budget::actual_seed();
	future.account("a", false);
	future.txn("future", "a", "2027-05-01", -100, Some("food"));
	assert_eq!(future.sheet(TODAY).months().first(), Some(&ym("2026-08")));
}

#[test]
fn the_first_transaction_counts_tombstoned_and_off_budget_rows_but_not_split_children() {
	let mut budget = Budget::actual_seed();
	budget.account("tracking", true);
	budget
		.txn("deleted", "tracking", "2025-01-15", -100, None)
		.tombstone = true;
	assert_eq!(budget.sheet(TODAY).months().first(), Some(&ym("2024-10")));

	let mut budget = Budget::actual_seed();
	budget.account("a", false);
	budget.txn("parent", "a", "2026-06-10", -100, None).split = SplitRole::Parent;
	budget
		.txn("child", "a", "2024-01-01", -100, Some("food"))
		.split = SplitRole::Child {
		parent: TransactionId::new("parent"),
	};
	assert_eq!(budget.sheet(TODAY).months().first(), Some(&ym("2026-03")));
}

#[test]
fn which_transactions_count_as_activity() {
	let mut budget = Budget::actual_seed();
	budget.account("checking", false);
	budget.account("brokerage", true);
	budget.account("closed", false).closed = true;
	budget.account("deleted-account", false).tombstone = true;
	budget.category("hidden", "usual", false).hidden = true;
	budget.group("archive", false, 99999.0).hidden = true;
	budget.category("in-hidden-group", "archive", false);
	let date = "2026-11-03";
	budget.txn("plain", "checking", date, -1_000, Some("food"));
	budget
		.txn("tombstoned", "checking", date, -2_000, Some("food"))
		.tombstone = true;
	budget.txn("off-budget", "brokerage", date, -4_000, Some("food"));
	budget.txn("closed-account", "closed", date, -8_000, Some("food"));
	budget.txn(
		"deleted-account",
		"deleted-account",
		date,
		-16_000,
		Some("food"),
	);
	budget.txn("no-account-row", "nowhere", date, -32_000, Some("food"));
	budget.txn("uncategorized", "checking", date, -64_000, None);
	budget.txn("hidden", "checking", date, -500, Some("hidden"));
	budget.txn(
		"hidden-group",
		"checking",
		date,
		-700,
		Some("in-hidden-group"),
	);
	budget
		.txn("parent", "checking", date, -300, Some("food"))
		.split = SplitRole::Parent;
	for (id, amount) in [("child-a", -100), ("child-b", -200)] {
		budget
			.txn(id, "checking", date, amount, Some("general"))
			.split = SplitRole::Child {
			parent: TransactionId::new("parent"),
		};
	}
	let dead_parent = budget.txn("dead-parent", "checking", date, -50, None);
	dead_parent.split = SplitRole::Parent;
	dead_parent.tombstone = true;
	budget
		.txn("orphan", "checking", date, -10, Some("general"))
		.split = SplitRole::Child {
		parent: TransactionId::new("dead-parent"),
	};
	budget
		.txn("missing-parent", "checking", date, -20, Some("general"))
		.split = SplitRole::Child {
		parent: TransactionId::new("never-existed"),
	};
	budget
		.txn("dead-child", "checking", date, -40, Some("general"))
		.split = SplitRole::Child {
		parent: TransactionId::new("parent"),
	};
	budget
		.0
		.transactions
		.get_mut(&TransactionId::new("dead-child"))
		.unwrap()
		.tombstone = true;
	budget
		.txn(
			"starting",
			"checking",
			"2026-11-01",
			90_000,
			Some("starting-balances"),
		)
		.starting_balance = true;

	let november = budget.sheet(TODAY).month(ym("2026-11")).unwrap();
	assert_eq!(
		november.categories[&cat("food")].spent,
		cents(-1_000 - 8_000 - 16_000)
	);
	assert_eq!(
		november.categories[&cat("general")].spent,
		cents(-300),
		"split children count, parents don't"
	);
	assert_eq!(
		november.categories[&cat("hidden")].spent,
		cents(-500),
		"hidden categories count fully"
	);
	assert_eq!(
		november.categories[&cat("in-hidden-group")].balance,
		cents(-700)
	);
	assert_eq!(
		november.summary.total_spent,
		cents(-25_000 - 300 - 500 - 700)
	);
	assert_eq!(
		november.summary.total_income,
		cents(90_000),
		"a categorized starting balance is income"
	);
}

#[test]
fn mapped_categories_land_on_their_target() {
	let mut budget = Budget::actual_seed();
	budget.account("a", false);
	budget.category("dining", "usual", false).tombstone = true;
	budget.0.category_mapping.insert(cat("dining"), cat("food"));
	budget.category("gone", "usual", false).tombstone = true;
	budget.category("points-at-gone", "usual", false).tombstone = true;
	budget
		.0
		.category_mapping
		.insert(cat("points-at-gone"), cat("gone"));
	budget.category("unmapped", "usual", false);
	budget.0.category_mapping.remove(&cat("unmapped"));
	budget.txn("dinner", "a", "2026-11-02", -1_200, Some("dining"));
	budget.txn("lunch", "a", "2026-11-02", -300, Some("food"));
	budget.txn("lost", "a", "2026-11-02", -5_000, Some("points-at-gone"));
	budget.txn(
		"no-mapping-row",
		"a",
		"2026-11-02",
		-7_000,
		Some("unmapped"),
	);

	let november = budget.sheet(TODAY).month(ym("2026-11")).unwrap();
	assert_eq!(november.categories[&cat("food")].spent, cents(-1_500));
	assert!(!november.categories.contains_key(&cat("dining")));
	assert_eq!(november.categories[&cat("unmapped")].spent, Money::ZERO);
	assert_eq!(november.summary.total_spent, cents(-1_500));
}

#[test]
fn categories_in_a_tombstoned_group_get_no_cells() {
	let mut budget = Budget::actual_seed();
	budget.account("a", false);
	budget.group("gone", false, 1.0).tombstone = true;
	budget.category("orphaned", "gone", false);
	budget.txn("t", "a", "2026-11-02", -900, Some("orphaned"));
	budget.budget("2026-11", "orphaned", 5_000);
	let november = budget.sheet(TODAY).month(ym("2026-11")).unwrap();
	assert!(!november.categories.contains_key(&cat("orphaned")));
	assert_eq!(november.summary.total_budgeted, Money::ZERO);
	assert_eq!(november.summary.to_budget, Money::ZERO);
}

#[test]
fn a_negative_balance_rolls_over_only_when_that_months_flag_is_on() {
	let mut budget = Budget::actual_seed();
	budget.account("a", false);
	budget.txn("pay", "a", "2026-09-01", 100_000, Some("income"));
	budget.txn("big-shop", "a", "2026-09-05", -3_000, Some("food"));
	budget.budget("2026-09", "food", 1_000);
	budget.cell("2026-09", "food").carryover = true;
	budget.budget("2026-10", "food", 500);
	let sheet = budget.sheet(TODAY);
	let food = |m: &str| sheet.category(ym(m), &cat("food")).copied().unwrap();
	assert_eq!(food("2026-09").balance, cents(-2_000));
	assert_eq!(
		food("2026-10").balance,
		cents(-1_500),
		"September's flag carries the deficit into October"
	);
	assert_eq!(
		food("2026-11").balance,
		Money::ZERO,
		"October's flag is off, so November starts at zero"
	);
	let summary = |m: &str| sheet.month(ym(m)).unwrap().summary;
	assert_eq!(summary("2026-10").last_month_overspent, Money::ZERO);
	assert_eq!(summary("2026-11").last_month_overspent, cents(-1_500));
	assert_eq!(
		summary("2026-11").to_budget,
		cents(100_000 - 1_000 - 500 - 1_500)
	);
}

#[test]
fn only_the_first_income_group_feeds_to_budget() {
	let mut budget = Budget::actual_seed();
	budget.group("side-income", true, 99999.0);
	budget.category("gigs", "side-income", true);
	budget.account("a", false);
	budget.txn("salary", "a", "2026-11-01", 300_000, Some("income"));
	budget.txn("gig", "a", "2026-11-02", 20_000, Some("gigs"));
	let november = budget.sheet(TODAY).month(ym("2026-11")).unwrap();
	assert_eq!(november.summary.total_income, cents(300_000));
	assert_eq!(
		november.groups[&CategoryGroupId::new("side-income")].spent,
		cents(20_000)
	);
	assert_eq!(november.summary.to_budget, cents(300_000));
	assert_eq!(
		november.summary.total_spent,
		Money::ZERO,
		"income groups never count as spending"
	);
}

#[test]
fn a_manual_hold_replaces_the_auto_hold_and_comes_back_next_month() {
	let mut budget = Budget::actual_seed();
	budget.account("a", false);
	budget.txn("salary", "a", "2026-11-01", 300_000, Some("income"));
	budget.cell("2026-11", "income").carryover = true;
	let sheet = budget.sheet(TODAY);
	let november = sheet.month(ym("2026-11")).unwrap().summary;
	assert_eq!(
		(november.auto_hold, november.for_next_month),
		(cents(300_000), cents(300_000))
	);
	assert_eq!(november.to_budget, Money::ZERO);
	assert_eq!(
		sheet.month(ym("2026-12")).unwrap().summary.to_budget,
		cents(300_000)
	);

	budget.hold("2026-11", 1_000);
	let sheet = budget.sheet(TODAY);
	let november = sheet.month(ym("2026-11")).unwrap().summary;
	assert_eq!(
		(november.auto_hold, november.for_next_month),
		(cents(300_000), cents(1_000))
	);
	assert_eq!(november.to_budget, cents(299_000));
	let december = sheet.month(ym("2026-12")).unwrap().summary;
	assert_eq!(december.from_last_month, cents(300_000));
}

#[test]
fn negative_to_budget_carries_forward() {
	let mut budget = Budget::actual_seed();
	budget.budget("2026-11", "bills", 50_000);
	let sheet = budget.sheet(TODAY);
	assert_eq!(
		sheet.month(ym("2026-11")).unwrap().summary.to_budget,
		cents(-50_000)
	);
	assert_eq!(
		sheet.month(ym("2027-03")).unwrap().summary.to_budget,
		cents(-50_000)
	);
	assert_eq!(
		sheet
			.category(ym("2027-03"), &cat("bills"))
			.unwrap()
			.balance,
		cents(50_000)
	);
}

#[test]
fn budget_rows_outside_the_created_months_are_ignored() {
	let mut budget = Budget::actual_seed();
	budget.budget("2026-07", "bills", 10_000);
	budget.budget("2027-12", "bills", 20_000);
	budget.hold("2026-07", 5_000);
	let sheet = budget.sheet(TODAY);
	assert_eq!(
		sheet.month(ym("2026-08")).unwrap().summary.to_budget,
		Money::ZERO
	);
	assert_eq!(
		sheet.month(ym("2027-11")).unwrap().summary.to_budget,
		Money::ZERO
	);
}

#[test]
fn an_income_budget_row_in_an_expense_group_counts_toward_the_group_like_actual() {
	let mut budget = Budget::actual_seed();
	budget.category("odd", "usual", true);
	budget.budget("2026-11", "odd", 700);
	let november = budget.sheet(TODAY).month(ym("2026-11")).unwrap();
	assert_eq!(november.categories[&cat("odd")].balance, Money::ZERO);
	assert_eq!(
		november.groups[&CategoryGroupId::new("usual")].budgeted,
		cents(700)
	);
	assert_eq!(november.summary.total_budgeted, cents(-700));
}

/// D46: Actual's sheet throws without an income group. Veritas still opens
/// the budget and reads income as 0; the store's audit raises the warning.
#[test]
fn a_budget_without_an_income_group_opens_with_no_income() {
	let mut budget = Budget::actual_seed();
	budget
		.0
		.category_groups
		.get_mut(&CategoryGroupId::new("income"))
		.unwrap()
		.tombstone = true;
	budget.account("a", false);
	budget.txn("pay", "a", "2026-11-01", 50_000, Some("income"));
	budget.budget("2026-11", "food", 2_000);
	let november = budget.sheet(TODAY).month(ym("2026-11")).unwrap();
	assert_eq!(november.summary.total_income, Money::ZERO);
	assert_eq!(november.summary.to_budget, cents(-2_000));
}

/// A NULL `zero_budget_months.buffered` passes Actual's `!== 0` test, so it
/// replaces the auto hold and then reads as 0.
#[test]
fn a_null_hold_switches_off_the_auto_hold() {
	let mut budget = Budget::actual_seed();
	budget.account("a", false);
	budget.txn("salary", "a", "2026-11-01", 300_000, Some("income"));
	budget.cell("2026-11", "income").carryover = true;
	budget.0.holds.insert(ym("2026-11"), None);
	let november = budget.sheet(TODAY).month(ym("2026-11")).unwrap().summary;
	assert_eq!(november.auto_hold, cents(300_000));
	assert_eq!(november.for_next_month, Money::ZERO);
	assert_eq!(november.to_budget, cents(300_000));

	budget.hold("2026-11", 0);
	assert_eq!(
		budget
			.sheet(TODAY)
			.month(ym("2026-11"))
			.unwrap()
			.summary
			.for_next_month,
		cents(300_000)
	);
}

/// Actual's `loadUserBudgets` skips rows of tombstoned or missing categories
/// (`lc/server/sheet.ts:231-235`); the store keeps them.
#[test]
fn budget_rows_of_deleted_or_unknown_categories_are_ignored() {
	let mut budget = Budget::actual_seed();
	budget.category("deleted", "usual", false).tombstone = true;
	budget.budget("2026-11", "deleted", 9_000);
	budget.budget("2026-11", "never-existed", 4_000);
	let november = budget.sheet(TODAY).month(ym("2026-11")).unwrap();
	assert_eq!(november.summary.total_budgeted, Money::ZERO);
	assert!(!november.categories.contains_key(&cat("deleted")));
}

#[test]
fn results_beyond_actuals_safe_range_are_an_overflow_error_not_a_panic() {
	let mut budget = Budget::actual_seed();
	budget.budget("2026-11", "food", (1 << 51) - 1);
	assert!(EnvelopeSheet::compute(&budget.0, day(TODAY)).is_ok());
	budget.budget("2026-11", "general", 1);
	let error = EnvelopeSheet::compute(&budget.0, day(TODAY)).unwrap_err();
	assert!(matches!(error, CoreError::Overflow));

	let mut budget = Budget::actual_seed();
	budget.account("a", false);
	budget.txn("x", "a", "2026-11-01", i64::MAX, Some("food"));
	budget.txn("y", "a", "2026-11-01", i64::MAX, Some("food"));
	assert!(matches!(
		EnvelopeSheet::compute(&budget.0, day(TODAY)),
		Err(CoreError::Overflow)
	));

	let mut budget = Budget::actual_seed();
	budget.hold("2026-11", i64::MIN);
	assert!(matches!(
		EnvelopeSheet::compute(&budget.0, day(TODAY)),
		Err(CoreError::Overflow)
	));
}

mod incremental {
	use super::*;
	use veritas_engine::SheetChange;

	fn assert_matches_a_fresh_sheet(sheet: &EnvelopeSheet, budget: &Budget) {
		let fresh = budget.sheet(TODAY);
		assert_eq!(sheet.months(), fresh.months());
		for month in fresh.months() {
			assert_eq!(sheet.month(*month), fresh.month(*month), "{month}");
		}
	}

	/// §6.5: an edit in month M recomputes M through the last month. Each step
	/// edits the budget, tells the sheet what changed, and must equal a rebuild.
	#[test]
	fn applying_each_change_equals_a_full_rebuild() {
		let mut budget = small_fixture();
		let mut sheet = budget.sheet(TODAY);
		let months = sheet.months().to_vec();
		let categories = ["rent", "utilities", "groceries", "dining", "food", "income"];
		for step in 0..150usize {
			let month = months[(step * 7) % months.len()];
			let category = categories[step % categories.len()];
			let label = month.to_string();
			let id = TransactionId::new(format!("step-{step}"));
			let previous = TransactionId::new(format!("step-{}", step - step % 6));
			match step % 6 {
				0 => {
					let date = format!("{label}-{:02}", 1 + step % 28);
					budget.txn(
						id.as_str(),
						"checking",
						&date,
						-(step as i64) * 101,
						Some(category),
					);
					sheet
						.apply(&budget.0, SheetChange::Transactions(&[id]))
						.unwrap();
				}
				1 => {
					budget.budget(&label, category, (step as i64 * 1_337) % 40_000 - 10_000);
					sheet.apply(&budget.0, SheetChange::Month(month)).unwrap();
				}
				2 => {
					budget.cell(&label, category).carryover ^= true;
					budget.hold(&label, (step as i64 % 3) * 2_500);
					sheet.apply(&budget.0, SheetChange::Month(month)).unwrap();
				}
				3 => {
					let moved = month.offset(-2).unwrap().first_day();
					budget.0.transactions.get_mut(&previous).unwrap().date = moved;
					sheet
						.apply(&budget.0, SheetChange::Transactions(&[previous]))
						.unwrap();
				}
				4 => {
					budget.0.transactions.get_mut(&previous).unwrap().split = SplitRole::Parent;
					let date = budget.0.transactions[&previous].date.to_string();
					budget
						.txn(id.as_str(), "checking", &date, -77, Some("dining"))
						.split = SplitRole::Child {
						parent: previous.clone(),
					};
					sheet
						.apply(&budget.0, SheetChange::Transactions(&[previous, id]))
						.unwrap();
				}
				_ => {
					let parent = budget.0.transactions.get_mut(&previous).unwrap();
					parent.tombstone = !parent.tombstone;
					sheet
						.apply(&budget.0, SheetChange::Transactions(&[previous]))
						.unwrap();
				}
			}
			assert_matches_a_fresh_sheet(&sheet, &budget);
		}
	}

	/// PR #36 review: listing only the parent picks up a child the sheet has
	/// never seen, and a child removed from it.
	#[test]
	fn listing_a_split_parent_rereads_its_new_children() {
		let mut budget = small_fixture();
		let mut sheet = budget.sheet(TODAY);
		let parent = TransactionId::new("shop");
		budget
			.txn("shop", "checking", "2026-10-05", -9_000, None)
			.split = SplitRole::Parent;
		sheet
			.apply(
				&budget.0,
				SheetChange::Transactions(std::slice::from_ref(&parent)),
			)
			.unwrap();
		for (id, amount, category) in [
			("shop-food", -6_000, "groceries"),
			("shop-meal", -3_000, "dining"),
		] {
			budget
				.txn(id, "checking", "2026-10-05", amount, Some(category))
				.split = SplitRole::Child {
				parent: parent.clone(),
			};
		}
		sheet
			.apply(
				&budget.0,
				SheetChange::Transactions(std::slice::from_ref(&parent)),
			)
			.unwrap();
		assert_eq!(
			sheet
				.category(ym("2026-10"), &cat("groceries"))
				.unwrap()
				.spent,
			cents(-12_657 - 6_000)
		);
		assert_matches_a_fresh_sheet(&sheet, &budget);

		budget
			.0
			.transactions
			.remove(&TransactionId::new("shop-meal"));
		sheet
			.apply(&budget.0, SheetChange::Transactions(&[parent]))
			.unwrap();
		assert_matches_a_fresh_sheet(&sheet, &budget);
	}

	#[test]
	fn months_before_the_change_keep_their_memoized_values() {
		let mut budget = small_fixture();
		let mut sheet = budget.sheet(TODAY);
		budget.budget("2026-08", "rent", 1);
		budget.budget("2026-10", "rent", 2);
		sheet
			.apply(&budget.0, SheetChange::Month(ym("2026-10")))
			.unwrap();
		let rent = |m: &str| sheet.category(ym(m), &cat("rent")).unwrap().budgeted;
		assert_eq!(rent("2026-08"), cents(150_000));
		assert_eq!(rent("2026-10"), cents(2));
	}

	#[test]
	fn structure_changes_and_a_new_first_month_rebuild_everything() {
		let mut budget = small_fixture();
		let mut sheet = budget.sheet(TODAY);
		budget.category("new", "everyday", false);
		budget.budget("2026-06", "new", 4_000);
		sheet.apply(&budget.0, SheetChange::Structure).unwrap();
		assert_matches_a_fresh_sheet(&sheet, &budget);

		budget.txn("ancient", "checking", "2024-02-02", -1, Some("new"));
		let ancient = [TransactionId::new("ancient")];
		sheet
			.apply(&budget.0, SheetChange::Transactions(&ancient))
			.unwrap();
		assert_eq!(sheet.months().first(), Some(&ym("2023-11")));
		assert_matches_a_fresh_sheet(&sheet, &budget);

		budget.0.transactions.remove(&ancient[0]);
		sheet
			.apply(&budget.0, SheetChange::Transactions(&ancient))
			.unwrap();
		assert_eq!(sheet.months().first(), Some(&ym("2026-05")));
		assert_matches_a_fresh_sheet(&sheet, &budget);
	}
}
