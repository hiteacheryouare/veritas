//! R-GOAL-1, R-GOAL-3, R-GOAL-4 and R-GOAL-6 through docs/finance/debt.md
//! DEBT-8's worked examples. Today is 2026-11-09, so month M is 2026-11.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;
use veritas_domain::{CoreError, Goal, GoalId, GoalStatus, Money};
use veritas_engine::goals::{self, Suggestion};
use veritas_engine::{BudgetEditor, Envelope, NumberFormat};

const M: &str = "2026-11";

fn goal(target: i64, due: Option<&str>) -> Goal {
	Goal {
		id: GoalId::new("g"),
		category: cat("laptop"),
		target: cents(target),
		target_date: due.map(day),
		icon: None,
		color: None,
		status: GoalStatus::Active,
		reached_month: None,
		closed_month: None,
		released_to: None,
		released: None,
	}
}

/// The seed budget plus a Goals group with the "New laptop" envelope.
fn with_goal() -> Budget {
	let mut budget = Budget::actual_seed();
	budget.account("checking", false);
	budget.group("goals", false, 50_000.0);
	budget.category("laptop", "goals", false).name = "New laptop".to_owned();
	budget.category("dining", "usual", false);
	budget.txn("pay", "checking", "2026-08-01", 1_000_000, Some("income"));
	budget
}

fn suggest(budget: &Budget, goal: &Goal) -> Suggestion {
	goals::suggestion(&budget.sheet(TODAY), goal, ym(M)).unwrap()
}

#[test]
fn g1_suggests_an_even_monthly_amount_through_the_target_month() {
	let budget = with_goal();
	let six = suggest(&budget, &goal(150_000, Some("2027-04-15")));
	assert_eq!((six.months_left, six.monthly), (6, cents(25_000)));
	assert_eq!(
		six.schedule().iter().map(|m| m.minor()).sum::<i64>(),
		150_000
	);
	let seven = suggest(&budget, &goal(150_000, Some("2027-05-15")));
	assert_eq!((seven.months_left, seven.monthly), (7, cents(21_429)));
}

#[test]
fn g1b_this_months_budget_doesnt_change_the_suggestion() {
	let mut budget = with_goal();
	budget.budget(M, "laptop", 25_000);
	let target = goal(150_000, Some("2027-04-15"));
	assert_eq!(suggest(&budget, &target).monthly, cents(25_000));
	budget.budget("2026-12", "laptop", 25_000);
	let december = goals::suggestion(&budget.sheet(TODAY), &target, ym("2026-12")).unwrap();
	assert_eq!(
		(december.remaining, december.months_left, december.monthly),
		(cents(125_000), 5, cents(25_000))
	);
}

#[test]
fn g2_g3_ceiling_rounding_leaves_the_last_month_smaller() {
	let budget = with_goal();
	let three = suggest(&budget, &goal(100_000, Some("2027-01-31")));
	assert_eq!(three.monthly, cents(33_334));
	assert_eq!(three.schedule(), [33_334, 33_334, 33_332].map(cents));
	let tiny = suggest(&budget, &goal(1, Some("2027-01-01")));
	assert_eq!(tiny.schedule(), [1, 0, 0].map(cents));
}

#[test]
fn g4_g5_funded_goals_suggest_nothing_and_past_dates_mean_one_month() {
	let mut budget = with_goal();
	budget.budget("2026-10", "laptop", 60_000);
	assert_eq!(
		suggest(&budget, &goal(50_000, Some("2027-06-01"))).monthly,
		Money::ZERO
	);
	let late = suggest(&budget, &goal(110_000, Some("2026-06-01")));
	assert_eq!((late.months_left, late.monthly), (1, cents(50_000)));
	assert!(goals::suggestion(&budget.sheet(TODAY), &goal(1, None), ym(M)).is_none());
}

#[test]
fn reaching_the_target_is_recorded_once_and_spending_never_undoes_it() {
	let mut budget = with_goal();
	budget.budget("2026-09", "laptop", 150_000);
	let sheet = budget.sheet(TODAY);
	let active = goal(150_000, None);
	let reached = goals::check_reached(&sheet, &active, ym("2026-09")).unwrap();
	assert_eq!(
		(reached.status, reached.reached_month),
		(GoalStatus::Reached, Some(ym("2026-09")))
	);
	assert_eq!(
		goals::first_reached_month(&sheet, &active, ym(M)),
		Some(ym("2026-09"))
	);

	budget.txn("laptop", "checking", "2026-10-02", -148_000, Some("laptop"));
	let sheet = budget.sheet(TODAY);
	assert_eq!(
		goals::progress(&sheet, &reached, ym(M)).available,
		cents(2_000)
	);
	assert_eq!(goals::check_reached(&sheet, &reached, ym(M)), None);
	assert_eq!(
		goals::check_reached(&budget.sheet(TODAY), &active, ym("2026-08")),
		None
	);
}

#[test]
fn reopening_returns_to_reached_only_if_it_ever_was() {
	let mut closed = goal(1, None);
	closed.status = GoalStatus::Closed;
	assert_eq!(goals::reopen(&closed).status, GoalStatus::Active);
	closed.reached_month = Some(ym("2026-09"));
	assert_eq!(goals::reopen(&closed).status, GoalStatus::Reached);
}

fn close(budget: &mut Budget, goal: &Goal, release_to: Envelope) -> goals::GoalClosing {
	let sheet = budget.sheet(TODAY);
	let editor = BudgetEditor::new(&budget.0, &sheet, NumberFormat::CommaDot, false);
	let closing = goals::close(&editor, &sheet, goal, ym(M), &release_to).unwrap();
	closing.writes.apply_to(&mut budget.0);
	closing
}

#[test]
fn r1_a_positive_leftover_moves_to_the_chosen_envelope() {
	let mut budget = with_goal();
	budget.budget("2026-09", "laptop", 150_000);
	budget.txn("laptop", "checking", "2026-10-02", -148_000, Some("laptop"));
	budget.budget(M, "savings", 30_000);
	let transactions = budget.0.transactions.clone();
	let closing = close(
		&mut budget,
		&goal(150_000, None),
		Envelope::Category(cat("savings")),
	);

	assert_eq!(closing.goal.status, GoalStatus::Closed);
	assert_eq!(closing.goal.released, Some(cents(2_000)));
	assert_eq!(closing.goal.released_to, Some(cat("savings")));
	assert_eq!(closing.hide, cat("laptop"));
	let sheet = budget.sheet(TODAY);
	let laptop = sheet.category(ym(M), &cat("laptop")).unwrap();
	assert_eq!(
		(laptop.budgeted, laptop.balance),
		(cents(-2_000), Money::ZERO)
	);
	assert_eq!(
		sheet.category(ym(M), &cat("savings")).unwrap().budgeted,
		cents(32_000)
	);
	assert_eq!(
		budget.0.transactions, transactions,
		"GOAL-1: no transaction is touched"
	);
	assert!(
		budget.0.notes["budget-2026-11"]
			.as_deref()
			.unwrap()
			.contains("Reassigned 20.00 from New laptop \u{2192} savings")
	);
}

#[test]
fn r2_an_overspent_goal_pulls_from_the_chosen_envelope() {
	let mut budget = with_goal();
	budget.budget("2026-09", "laptop", 150_000);
	budget.txn("laptop", "checking", "2026-11-02", -152_000, Some("laptop"));
	budget.budget(M, "dining", 10_000);
	close(
		&mut budget,
		&goal(150_000, None),
		Envelope::Category(cat("dining")),
	);
	let sheet = budget.sheet(TODAY);
	assert_eq!(
		sheet.category(ym(M), &cat("dining")).unwrap().budgeted,
		cents(8_000)
	);
	let laptop = sheet.category(ym(M), &cat("laptop")).unwrap();
	assert_eq!(
		(laptop.budgeted, laptop.balance),
		(cents(2_000), Money::ZERO)
	);
}

#[test]
fn r3_a_zero_leftover_closes_without_a_move() {
	let mut budget = with_goal();
	let closing = close(
		&mut budget,
		&goal(150_000, None),
		Envelope::Category(cat("dining")),
	);
	assert!(closing.writes.is_empty());
	assert_eq!(closing.goal.status, GoalStatus::Closed);
	assert_eq!(closing.goal.closed_month, Some(ym(M)));
}

#[test]
fn r5_r7_releasing_to_to_budget_raises_it_by_the_leftover() {
	let mut budget = with_goal();
	budget.budget("2026-09", "laptop", 160_000);
	budget.txn("laptop", "checking", "2026-10-02", -148_000, Some("laptop"));
	let before = budget.sheet(TODAY).summary(ym(M)).unwrap().to_budget;
	let closing = close(&mut budget, &goal(150_000, None), Envelope::ToBudget);
	assert_eq!(closing.goal.released, Some(cents(12_000)));
	assert_eq!(closing.writes.cells.len(), 1);
	let after = budget.sheet(TODAY).summary(ym(M)).unwrap().to_budget;
	assert_eq!(after.minor() - before.minor(), 12_000);
}

#[test]
fn an_overspent_goal_released_against_to_budget_is_funded_from_it() {
	let mut budget = with_goal();
	budget.txn("laptop", "checking", "2026-11-02", -3_000, Some("laptop"));
	close(&mut budget, &goal(150_000, None), Envelope::ToBudget);
	let laptop = *budget.sheet(TODAY).category(ym(M), &cat("laptop")).unwrap();
	assert_eq!(
		(laptop.budgeted, laptop.balance),
		(cents(3_000), Money::ZERO)
	);
	assert!(
		budget.0.notes["budget-2026-11"]
			.as_deref()
			.unwrap()
			.contains("from To Budget \u{2192} New laptop")
	);
}

#[test]
fn closing_twice_or_into_the_goal_itself_is_refused() {
	let budget = with_goal();
	let sheet = budget.sheet(TODAY);
	let editor = BudgetEditor::new(&budget.0, &sheet, NumberFormat::CommaDot, false);
	let mut closed = goal(1, None);
	closed.status = GoalStatus::Closed;
	let into_itself = Envelope::Category(cat("laptop"));
	assert!(matches!(
		goals::close(&editor, &sheet, &closed, ym(M), &Envelope::ToBudget),
		Err(CoreError::Invalid(_))
	));
	assert!(matches!(
		goals::close(&editor, &sheet, &goal(1, None), ym(M), &into_itself),
		Err(CoreError::Invalid(_))
	));
}

#[test]
fn d36_the_target_is_written_as_a_goal_line_and_monthly_goal_rows() {
	let mut budget = with_goal();
	let sheet = budget.sheet(TODAY);
	let writes =
		goals::target_writes(&budget.0, &sheet, &cat("laptop"), cents(150_000), ym(M)).unwrap();
	assert_eq!(
		writes.notes,
		[("laptop".to_owned(), "#goal 1500.00".to_owned())]
	);
	assert_eq!(
		writes.cells.len(),
		13,
		"November 2026 through the last created month"
	);
	assert!(writes.cells.iter().all(|c| {
		c.amount.is_none()
			&& c.goal
				.is_some_and(|g| g.long_goal && g.goal == Some(cents(150_000)))
	}));

	budget.0.notes.insert(
		"laptop".to_owned(),
		Some("Saving for the M5\n  #goal 1000\n#goal 99".to_owned()),
	);
	let writes =
		goals::target_writes(&budget.0, &sheet, &cat("laptop"), cents(123_456), ym(M)).unwrap();
	assert_eq!(
		writes.notes[0].1, "Saving for the M5\n#goal 1234.56",
		"one goal line, replaced in place"
	);
	writes.apply_to(&mut budget.0);
	let cell = &budget.0.budget_cells[&(ym("2027-05"), cat("laptop"))];
	assert_eq!(
		(cell.goal, cell.long_goal, cell.budgeted),
		(Some(cents(123_456)), true, Money::ZERO)
	);

	let zero = goals::target_writes(&budget.0, &sheet, &cat("laptop"), Money::ZERO, ym(M));
	assert!(matches!(zero, Err(CoreError::Invalid(_))));
}
