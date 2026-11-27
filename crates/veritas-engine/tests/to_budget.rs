//! docs/finance/to-budget.md TB-8: the To Budget quick actions' worked
//! examples, month 2026-11, envelopes in Budget screen order Groceries, Gas,
//! Insurance, Old Gym (hidden).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;
use veritas_domain::{Goal, GoalId, GoalStatus, Money};
use veritas_engine::{BudgetEditor, FillReason, Skip, ToBudgetPlan};

const TODAY_TB: &str = "2026-11-22";
const M: &str = "2026-11";

/// The TB-8 envelopes with `income` coming in this month.
fn book(income: i64) -> Budget {
	let mut budget = Budget(veritas_domain::BudgetData::empty(
		veritas_domain::BudgetType::Envelope,
	));
	budget.group("usual", false, 1.0);
	budget.group("income", true, 2.0);
	for (order, (id, name)) in [
		("groceries", "Groceries"),
		("gas", "Gas"),
		("insurance", "Insurance"),
		("old-gym", "Old Gym"),
	]
	.into_iter()
	.enumerate()
	{
		let category = budget.category(id, "usual", false);
		category.name = name.to_owned();
		category.sort_order = order as f64;
	}
	budget.category("salary", "income", true);
	budget.0.categories.get_mut(&cat("old-gym")).unwrap().hidden = true;
	budget.account("checking", false);
	budget.txn("pay", "checking", "2026-11-01", income, Some("salary"));
	budget
}

/// Example 1's state: Groceries and Gas overspent, Old Gym (hidden)
/// overspent, Insurance budgeted 50.00 against a 200.00 monthly goal.
fn example_1(income: i64) -> Budget {
	let mut budget = book(income);
	budget.txn("food", "checking", "2026-11-05", -12_000, Some("groceries"));
	budget.txn("fuel", "checking", "2026-11-06", -3_000, Some("gas"));
	budget.txn("gym", "checking", "2026-11-07", -5_000, Some("old-gym"));
	budget.budget(M, "insurance", 5_000);
	budget.cell(M, "insurance").goal = Some(cents(20_000));
	budget
}

fn plan(budget: &Budget, action: impl FnOnce(&BudgetEditor<'_>) -> ToBudgetPlan) -> ToBudgetPlan {
	let sheet = budget.sheet(TODAY_TB);
	action(&BudgetEditor::new(&budget.0, &sheet))
}

fn to_budget(budget: &Budget) -> Money {
	budget.sheet(TODAY_TB).summary(ym(M)).unwrap().to_budget
}

fn written(plan: &ToBudgetPlan) -> Vec<(&str, i64)> {
	plan.fills
		.iter()
		.map(|fill| (fill.category.as_str(), fill.amount.minor()))
		.collect()
}

fn goal(id: &str, category: &str, target: i64, due: Option<&str>, status: GoalStatus) -> Goal {
	Goal {
		id: GoalId::new(id),
		category: cat(category),
		target: cents(target),
		target_date: due.map(day),
		icon: None,
		color: None,
		status,
		reached_month: None,
		closed_month: None,
		released_to: None,
		released: None,
		activity_seen_through: None,
		activity_seen_total: None,
		close_month_dismissed_available: None,
	}
}

#[test]
fn tb_8_example_1_assign_to_underfunded_covers_then_tops_up_goals() {
	let mut budget = example_1(55_000);
	assert_eq!(to_budget(&budget), cents(50_000));
	let plan = plan(&budget, |editor| {
		editor.assign_to_underfunded(ym(M)).unwrap()
	});
	assert_eq!(
		written(&plan),
		[("groceries", 12_000), ("gas", 3_000), ("insurance", 15_000)],
		"TB-3.2 then TB-3.3"
	);
	assert_eq!(
		plan.fills
			.iter()
			.map(|f| f.reason.clone())
			.collect::<Vec<_>>(),
		[
			FillReason::Overspent,
			FillReason::Overspent,
			FillReason::GoalShortfall
		]
	);
	assert_eq!(plan.total().unwrap(), cents(30_000));
	assert_eq!(plan.envelopes(), 3);
	assert_eq!(
		plan.hidden_left_out, 1,
		"TB-2.3: Old Gym is counted, not filled"
	);
	let note = &plan.writes.notes.single_note();
	assert_eq!(
		note.lines().count(),
		2,
		"TB-10.3: two movement notes appended, none for Insurance"
	);
	assert!(
		note.lines()
			.next()
			.unwrap()
			.starts_with("- Reassigned 120.00 from To Budget \u{2192} Groceries on November 22"),
		"{note}"
	);
	assert!(
		note.lines()
			.nth(1)
			.unwrap()
			.starts_with("- Reassigned 30.00 from To Budget \u{2192} Gas on November 22"),
		"{note}"
	);
	plan.writes.apply_to(&mut budget.0);
	assert_eq!(to_budget(&budget), cents(20_000), "To Budget after");
	assert_eq!(
		budget.0.budget_cells[&(ym(M), cat("insurance"))].budgeted,
		cents(20_000)
	);
}

#[test]
fn tb_8_example_2_the_cap_bites() {
	let budget = example_1(15_000);
	assert_eq!(to_budget(&budget), cents(10_000));
	let plan = plan(&budget, |editor| {
		editor.assign_to_underfunded(ym(M)).unwrap()
	});
	assert_eq!(
		written(&plan),
		[("groceries", 10_000)],
		"TB-2.2: Groceries stays 20.00 short, Gas and Insurance get nothing"
	);
	assert_eq!(plan.fills[0].need, cents(12_000));
	assert_eq!(plan.total().unwrap(), cents(10_000));
	assert_eq!(plan.writes.notes.single_note().lines().count(), 1);
}

#[test]
fn tb_8_example_3_nothing_to_do() {
	let zero = example_1(5_000);
	assert_eq!(to_budget(&zero), Money::ZERO);
	assert!(
		plan(&zero, |editor| editor.assign_to_underfunded(ym(M)).unwrap())
			.fills
			.is_empty(),
		"To Budget is 0"
	);

	let mut calm = book(50_000);
	calm.budget(M, "insurance", 20_000);
	calm.cell(M, "insurance").goal = Some(cents(20_000));
	let plan = plan(&calm, |editor| editor.assign_to_underfunded(ym(M)).unwrap());
	assert!(plan.fills.is_empty(), "Nothing is underfunded");
	assert!(plan.writes.is_empty());
	assert_eq!(plan.hidden_left_out, 0);
}

#[test]
fn tb_3_3_a_long_term_goal_is_not_filled() {
	let mut budget = book(50_000);
	budget.budget(M, "insurance", 5_000);
	let cell = budget.cell(M, "insurance");
	cell.goal = Some(cents(20_000));
	cell.long_goal = true;
	assert!(
		plan(&budget, |editor| editor
			.assign_to_underfunded(ym(M))
			.unwrap())
		.fills
		.is_empty()
	);
}

/// Example 4's goals: New laptop and Trip active with target dates,
/// Emergency reached, and Someday with no target date.
fn example_4(income: i64, laptop_budgeted: i64) -> Budget {
	let mut budget = book(income);
	for (order, id) in ["laptop", "trip", "emergency", "someday"]
		.into_iter()
		.enumerate()
	{
		budget.category(id, "usual", false).sort_order = 10.0 + order as f64;
	}
	budget.budget(M, "laptop", laptop_budgeted);
	budget.budget(M, "trip", 12_000);
	for goal in [
		goal(
			"g-laptop",
			"laptop",
			150_000,
			Some("2027-06-15"),
			GoalStatus::Active,
		),
		goal(
			"g-trip",
			"trip",
			60_000,
			Some("2027-03-31"),
			GoalStatus::Active,
		),
		goal(
			"g-emergency",
			"emergency",
			100_000,
			Some("2027-01-31"),
			GoalStatus::Reached,
		),
		goal("g-someday", "someday", 40_000, None, GoalStatus::Active),
	] {
		budget.0.goals.insert(goal.id.clone(), goal);
	}
	budget
}

#[test]
fn tb_8_example_4_fund_goals_monthly() {
	let budget = example_4(27_000, 5_000);
	assert_eq!(to_budget(&budget), cents(10_000));
	let funded = plan(&budget, |editor| editor.fund_goals_monthly(ym(M)).unwrap());
	assert_eq!(
		written(&funded),
		[("laptop", 10_000)],
		"Trip needs 0, Emergency is reached"
	);
	assert_eq!(
		funded.fills[0].need,
		cents(13_750),
		"ceil(150000 / 8) - 5000: shown $100.00 of $137.50"
	);
	assert_eq!(
		funded.fills[0].reason,
		FillReason::GoalMonthly(GoalId::new("g-laptop"))
	);
	assert_eq!(
		funded.skipped,
		[(GoalId::new("g-someday"), Skip::NoTargetDate)],
		"TB-4.3"
	);

	let again = example_4(37_000, 15_000);
	assert_eq!(to_budget(&again), cents(10_000));
	let topped = plan(&again, |editor| editor.fund_goals_monthly(ym(M)).unwrap());
	assert_eq!(topped.fills[0].need, cents(3_750), "18750 - 15000");
}

#[test]
fn tb_8_example_5_fund_goals_monthly_is_idempotent() {
	let mut budget = example_4(37_000, 5_000);
	assert_eq!(to_budget(&budget), cents(20_000));
	let first = plan(&budget, |editor| editor.fund_goals_monthly(ym(M)).unwrap());
	assert_eq!(written(&first), [("laptop", 13_750)]);
	first.writes.apply_to(&mut budget.0);
	assert_eq!(to_budget(&budget), cents(6_250));
	let second = plan(&budget, |editor| editor.fund_goals_monthly(ym(M)).unwrap());
	assert!(
		second.fills.is_empty(),
		"TB-4.2: budgeted 18750 equals the suggestion"
	);
	assert!(second.writes.is_empty());
}

#[test]
fn tb_4_1_a_goal_in_a_hidden_envelope_is_counted_not_funded() {
	let mut budget = example_4(37_000, 5_000);
	budget.0.categories.get_mut(&cat("laptop")).unwrap().hidden = true;
	let plan = plan(&budget, |editor| editor.fund_goals_monthly(ym(M)).unwrap());
	assert!(plan.fills.is_empty());
	assert_eq!(plan.hidden_left_out, 1);
}

#[test]
fn tb_8_example_6_hold_adds_up_releases_and_clamps() {
	let mut budget = book(35_000);
	let hold = |budget: &mut Budget, amount: i64| {
		let sheet = budget.sheet(TODAY_TB);
		let writes = BudgetEditor::new(&budget.0, &sheet)
			.hold_for_next_month(ym(M), cents(amount))
			.unwrap();
		if let Some(writes) = &writes {
			writes.apply_to(&mut budget.0);
		}
		writes.is_some()
	};
	let held = |budget: &Budget| {
		budget
			.0
			.holds
			.get(&ym(M))
			.copied()
			.flatten()
			.unwrap_or(Money::ZERO)
	};
	assert!(hold(&mut budget, 20_000));
	assert_eq!(
		(held(&budget), to_budget(&budget)),
		(cents(20_000), cents(15_000))
	);
	assert!(hold(&mut budget, -5_000));
	assert_eq!(
		(held(&budget), to_budget(&budget)),
		(cents(15_000), cents(20_000)),
		"TB-5.2: a release"
	);
	assert!(hold(&mut budget, 30_000));
	assert_eq!(
		(held(&budget), to_budget(&budget)),
		(cents(35_000), Money::ZERO),
		"clamped to To Budget"
	);
	assert!(
		!hold(&mut budget, -5_000),
		"with To Budget 0 Actual's hold does nothing, so a release needs Reset"
	);
	let sheet = budget.sheet(TODAY_TB);
	BudgetEditor::new(&budget.0, &sheet)
		.reset_hold(ym(M))
		.apply_to(&mut budget.0);
	assert_eq!(
		(held(&budget), to_budget(&budget)),
		(Money::ZERO, cents(35_000))
	);
}

trait SingleNote {
	fn single_note(&self) -> String;
}

impl SingleNote for Vec<(String, String)> {
	fn single_note(&self) -> String {
		assert_eq!(self.len(), 1, "one note row, written once with every line");
		assert_eq!(self[0].0, format!("budget-{M}"));
		self[0].1.clone()
	}
}
