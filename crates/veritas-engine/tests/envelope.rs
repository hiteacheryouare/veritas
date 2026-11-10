//! R-BUD-1: the envelope sheet reproduces Actual's numbers. The rules come
//! from docs/actual-format.md §5.2–§5.7; the first test replays the oracle's
//! own `apply` test (tools/actual-oracle/test/apply.test.ts) by hand.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use veritas_domain::{
	Account, AccountId, BudgetCell, BudgetData, BudgetType, Category, CategoryGroup,
	CategoryGroupId, CategoryId, CivilDate, CoreError, Money, SplitRole, Transaction,
	TransactionId, YearMonth,
};
use veritas_engine::EnvelopeSheet;

fn ym(text: &str) -> YearMonth {
	YearMonth::parse(text).unwrap()
}

fn day(text: &str) -> CivilDate {
	let (year, rest) = text.split_once('-').unwrap();
	let (month, day) = rest.split_once('-').unwrap();
	CivilDate::new(
		year.parse().unwrap(),
		month.parse().unwrap(),
		day.parse().unwrap(),
	)
	.unwrap()
}

fn cat(id: &str) -> CategoryId {
	CategoryId::new(id)
}

fn cents(minor: i64) -> Money {
	Money::from_minor(minor)
}

struct Budget(BudgetData);

impl Budget {
	/// The groups and categories every new Actual budget starts with.
	fn actual_seed() -> Self {
		let mut budget = Budget(BudgetData::empty(BudgetType::Envelope));
		budget.group("usual", false, 16384.0);
		budget.group("income", true, 32768.0);
		budget.group("savings-group", false, 32768.0);
		for (id, group) in [
			("food", "usual"),
			("general", "usual"),
			("bills", "usual"),
			("flexible", "usual"),
			("savings", "savings-group"),
		] {
			budget.category(id, group, false);
		}
		budget.category("income", "income", true);
		budget.category("starting-balances", "income", true);
		budget
	}

	fn group(&mut self, id: &str, is_income: bool, sort_order: f64) -> &mut CategoryGroup {
		let id = CategoryGroupId::new(id);
		self.0
			.category_groups
			.entry(id.clone())
			.or_insert(CategoryGroup {
				id,
				name: String::new(),
				is_income,
				hidden: false,
				sort_order,
				tombstone: false,
			})
	}

	/// Also writes the self-mapping row Actual writes on every category create.
	fn category(&mut self, id: &str, group: &str, is_income: bool) -> &mut Category {
		self.0.category_mapping.insert(cat(id), cat(id));
		self.0.categories.entry(cat(id)).or_insert(Category {
			id: cat(id),
			name: id.to_owned(),
			group: CategoryGroupId::new(group),
			is_income,
			hidden: false,
			sort_order: 0.0,
			tombstone: false,
		})
	}

	fn account(&mut self, id: &str, off_budget: bool) -> &mut Account {
		let id = AccountId::new(id);
		self.0.accounts.entry(id.clone()).or_insert(Account {
			id,
			name: String::new(),
			off_budget,
			closed: false,
			group: None,
			sort_order: 0.0,
			tombstone: false,
		})
	}

	fn txn(
		&mut self,
		id: &str,
		account: &str,
		date: &str,
		amount: i64,
		category: Option<&str>,
	) -> &mut Transaction {
		let id = TransactionId::new(id);
		self.0.transactions.insert(
			id.clone(),
			Transaction {
				id: id.clone(),
				account: AccountId::new(account),
				date: day(date),
				amount: cents(amount),
				payee: None,
				category: category.map(cat),
				notes: None,
				cleared: true,
				reconciled: false,
				split: SplitRole::None,
				transfer: None,
				schedule: None,
				starting_balance: false,
				sort_order: 0.0,
				tombstone: false,
			},
		);
		self.0.transactions.get_mut(&id).unwrap()
	}

	fn cell(&mut self, month: &str, category: &str) -> &mut BudgetCell {
		self.0
			.budget_cells
			.entry((ym(month), cat(category)))
			.or_insert(BudgetCell {
				month: ym(month),
				category: cat(category),
				budgeted: Money::ZERO,
				carryover: false,
				goal: None,
				long_goal: false,
			})
	}

	fn budget(&mut self, month: &str, category: &str, amount: i64) {
		self.cell(month, category).budgeted = cents(amount);
	}

	/// Actual's set-carryover: every month from `from` to the last created one.
	fn roll_over(&mut self, from: &str, through: &str, category: &str) {
		let mut month = ym(from);
		while month <= ym(through) {
			self.cell(&month.to_string(), category).carryover = true;
			month = month.offset(1).unwrap();
		}
	}

	fn hold(&mut self, month: &str, amount: i64) {
		self.0.holds.insert(ym(month), Some(cents(amount)));
	}

	fn sheet(&self, today: &str) -> EnvelopeSheet {
		EnvelopeSheet::compute(&self.0, day(today)).unwrap()
	}
}

/// `fixtures/synthetic/small.zip` as its spec builds it
/// (tools/actual-oracle/src/specs/small.ts).
fn small_fixture() -> Budget {
	let mut budget = Budget::actual_seed();
	budget.account("checking", false);
	budget.account("savings-account", false);
	budget.group("bills-group", false, 49152.0);
	budget.group("everyday", false, 65536.0);
	budget.category("rent", "bills-group", false);
	budget.category("utilities", "bills-group", false);
	budget.category("groceries", "everyday", false);
	budget.category("dining", "everyday", false);
	for month in ["2026-08", "2026-09", "2026-10"] {
		let on = |d: u32| format!("{month}-{d:02}");
		budget.txn(
			&format!("{month}-pay"),
			"checking",
			&on(1),
			400_000,
			Some("income"),
		);
		budget.txn(
			&format!("{month}-rent"),
			"checking",
			&on(2),
			-150_000,
			Some("rent"),
		);
		budget.txn(
			&format!("{month}-power"),
			"checking",
			&on(12),
			-8_423,
			Some("utilities"),
		);
		budget.txn(
			&format!("{month}-groceries"),
			"checking",
			&on(9),
			-12_657,
			Some("groceries"),
		);
		budget.txn(
			&format!("{month}-dinner"),
			"checking",
			&on(20),
			-4_810,
			Some("dining"),
		);
		budget.budget(month, "rent", 150_000);
		budget.budget(month, "utilities", 9_000);
		budget.budget(month, "groceries", 12_000);
		budget.budget(month, "dining", 5_000);
	}
	budget.txn("to-savings", "checking", "2026-09-15", -50_000, None);
	budget.txn(
		"from-checking",
		"savings-account",
		"2026-09-15",
		50_000,
		None,
	);
	budget
}

const TODAY: &str = "2026-11-09";

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

	let november = budget.sheet(TODAY).month(ym("2026-11")).cloned().unwrap();
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

	let november = budget.sheet(TODAY).month(ym("2026-11")).cloned().unwrap();
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
	let november = budget.sheet(TODAY).month(ym("2026-11")).cloned().unwrap();
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
	let november = budget.sheet(TODAY).month(ym("2026-11")).cloned().unwrap();
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
	let november = budget.sheet(TODAY).month(ym("2026-11")).cloned().unwrap();
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
	let november = budget.sheet(TODAY).month(ym("2026-11")).cloned().unwrap();
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
	let november = budget.sheet(TODAY).month(ym("2026-11")).cloned().unwrap();
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

	fn assert_matches_a_fresh_sheet(sheet: &EnvelopeSheet, budget: &Budget) {
		let fresh = budget.sheet(TODAY);
		assert_eq!(sheet.months(), fresh.months());
		for month in fresh.months() {
			assert_eq!(sheet.month(*month), fresh.month(*month), "{month}");
		}
	}

	/// §6.5: an edit in month M recomputes M through the last month. Each step
	/// edits a month, updates from that month, and must equal a full rebuild.
	#[test]
	fn updating_from_the_edited_month_equals_a_full_rebuild() {
		let mut budget = small_fixture();
		let mut sheet = budget.sheet(TODAY);
		let months = sheet.months().to_vec();
		let categories = ["rent", "utilities", "groceries", "dining", "food", "income"];
		for step in 0..120usize {
			let month = months[(step * 7) % months.len()];
			let category = categories[step % categories.len()];
			let label = month.to_string();
			match step % 5 {
				0 => budget.budget(&label, category, (step as i64 * 1_337) % 40_000 - 10_000),
				1 => budget.cell(&label, category).carryover ^= true,
				2 => budget.hold(&label, (step as i64 % 3) * 2_500),
				3 => {
					let date = format!("{label}-{:02}", 1 + step % 28);
					budget.txn(
						&format!("step-{step}"),
						"checking",
						&date,
						-(step as i64) * 101,
						Some(category),
					);
				}
				_ => {
					let id = TransactionId::new(format!("step-{}", step - 1));
					let moved_to = month.offset(-2).unwrap();
					let old = budget.0.transactions[&id].date.year_month();
					budget.0.transactions.get_mut(&id).unwrap().date = moved_to.first_day();
					sheet.update(&budget.0, old.min(moved_to)).unwrap();
					assert_matches_a_fresh_sheet(&sheet, &budget);
					continue;
				}
			}
			sheet.update(&budget.0, month).unwrap();
			assert_matches_a_fresh_sheet(&sheet, &budget);
		}
	}

	#[test]
	fn months_before_the_edit_keep_their_memoized_values() {
		let mut budget = small_fixture();
		let mut sheet = budget.sheet(TODAY);
		budget.budget("2026-08", "rent", 1);
		budget.budget("2026-10", "rent", 2);
		sheet.update(&budget.0, ym("2026-10")).unwrap();
		assert_eq!(
			sheet
				.category(ym("2026-08"), &cat("rent"))
				.unwrap()
				.budgeted,
			cents(150_000)
		);
		assert_eq!(
			sheet
				.category(ym("2026-10"), &cat("rent"))
				.unwrap()
				.budgeted,
			cents(2)
		);
	}

	#[test]
	fn a_change_to_the_categories_or_month_range_rebuilds_everything() {
		let mut budget = small_fixture();
		let mut sheet = budget.sheet(TODAY);
		budget.category("new", "everyday", false);
		budget.budget("2026-06", "new", 4_000);
		sheet.update(&budget.0, ym("2027-11")).unwrap();
		assert_matches_a_fresh_sheet(&sheet, &budget);

		budget.txn("ancient", "checking", "2024-02-02", -1, Some("new"));
		sheet.update(&budget.0, ym("2027-11")).unwrap();
		assert_eq!(sheet.months().first(), Some(&ym("2023-11")));
		assert_matches_a_fresh_sheet(&sheet, &budget);
	}
}
