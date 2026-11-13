//! R-BUD-1, R-BUD-4, R-GOAL-4 and R-PERF-2 in one household's year: August
//! 2026 to July 2027 budgeted on the small fixture, the way a person budgets.
//! Paychecks, bills and groceries every month, overspending covered from
//! another envelope or To Budget, money moved, a hold for next month, a
//! rollover envelope, a split, a refund, a recategorized purchase, copied and
//! averaged budgets, and two goals closed in today's month: one overspent and
//! pulled back from Dining Out, one with money left over released to To
//! Budget. After every op the incremental sheet must equal a rebuild, and at
//! the end Actual, replaying the same ops, must agree on every month and row.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
mod oracle;
mod session;

use common::*;
use serde_json::{Value, json};
use session::{Op, Session, Universe};
use veritas_domain::{
	AccountId, BudgetData, Category, CategoryGroup, CategoryGroupId, TransactionId,
};

const CHECKING: &str = "79664b3b-072e-442d-9082-0c1e02c242c0";
const INCOME: &str = "3c1699a5-522a-435e-86dc-93d900a14f0e";
const GROCERIES: &str = "3eb3739d-9384-490c-8e5f-73a396bf02e4";
const DINING: &str = "4668e3ba-d3bd-4b9d-9e21-1e232be57c69";
const RENT: &str = "6a2a7900-aeac-482b-a592-ce709c1f4b4b";
const UTILITIES: &str = "e4b52b78-abc8-4d7d-9d50-a6ed7900e41c";
const GOALS: &str = "a0000000-0000-4000-8000-000000000001";
const VACATION: &str = "a0000000-0000-4000-8000-000000000002";
const LAPTOP: &str = "a0000000-0000-4000-8000-000000000003";

const G: usize = 0;
const D: usize = 1;
const R: usize = 2;
const U: usize = 3;
const VAC: usize = 4;
const LAP: usize = 5;
const PAY: usize = 6;

const AUG: usize = 3;
const NOV: usize = 6;
const DEC: usize = 7;
const JAN: usize = 8;
const FEB: usize = 9;
const MAR: usize = 10;
const APR: usize = 11;
const MAY: usize = 12;
const JUN: usize = 13;
const JUL: usize = 14;

/// The small fixture plus a Goals group with Vacation and New laptop, which
/// Actual is told to create before the session starts.
fn with_goals(mut budget: BudgetData) -> (BudgetData, Vec<Value>) {
	budget.category_groups.insert(
		CategoryGroupId::new(GOALS),
		CategoryGroup {
			id: CategoryGroupId::new(GOALS),
			name: "Goals".to_owned(),
			is_income: false,
			hidden: false,
			sort_order: 1_000_000.0,
			tombstone: false,
		},
	);
	let mut ops = vec![json!({ "op": "createCategoryGroup", "id": GOALS, "name": "Goals" })];
	for (id, name) in [(VACATION, "Vacation"), (LAPTOP, "New laptop")] {
		budget.category_mapping.insert(cat(id), cat(id));
		budget.categories.insert(
			cat(id),
			Category {
				id: cat(id),
				name: name.to_owned(),
				group: CategoryGroupId::new(GOALS),
				is_income: false,
				hidden: false,
				sort_order: budget.categories.len() as f64,
				tombstone: false,
			},
		);
		ops.push(json!({ "op": "createCategory", "id": id, "name": name, "group": GOALS }));
	}
	(budget, ops)
}

fn universe() -> Universe {
	Universe {
		expenses: [GROCERIES, DINING, RENT, UTILITIES, VACATION, LAPTOP]
			.iter()
			.map(|id| cat(id))
			.collect(),
		income: cat(INCOME),
		account: AccountId::new(CHECKING),
	}
}

fn spend(month: usize, day: u32, category: usize, amount: i64) -> Op {
	Op::AddTransaction {
		month,
		day,
		category: Some(category),
		amount,
	}
}

fn budget(month: usize, category: usize, amount: i64) -> Op {
	Op::SetBudget {
		month,
		category,
		amount,
	}
}

/// Groceries and Dining Out balances and To Budget just before covering
/// Groceries from Dining Out, and the same three right after.
#[derive(Clone, Copy, Debug)]
struct Cover {
	month: usize,
	groceries: i64,
	dining: i64,
	to_budget: i64,
	after: (i64, i64, i64),
}

/// Runs ops one at a time, checking the incremental sheet after each.
struct Year {
	session: Session,
	ops: Vec<Op>,
	covers: Vec<Cover>,
}

impl Year {
	fn run(&mut self, op: Op) {
		self.session.run(&op);
		self.session.assert_consistent(&op);
		self.ops.push(op);
	}

	fn balances(&self, month: usize) -> (i64, i64, i64) {
		let month = self.session.sheet.months()[month];
		let balance = |id: &str| {
			self.session
				.sheet
				.category(month, &cat(id))
				.unwrap()
				.balance
				.minor()
		};
		let to_budget = self.session.sheet.summary(month).unwrap().to_budget.minor();
		(balance(GROCERIES), balance(DINING), to_budget)
	}

	fn added_last(&self) -> TransactionId {
		self.session.added().last().unwrap().clone()
	}
}

fn household_year() -> Option<(Year, Vec<Value>)> {
	if !oracle::available() {
		return None;
	}
	let zip = oracle::fixture("small");
	let today = oracle::today(&oracle::dump(&zip));
	let (start, setup) = with_goals(oracle::load(&zip));
	let mut year = Year {
		session: Session::new(start, today, universe()),
		ops: Vec::new(),
		covers: Vec::new(),
	};
	assert_eq!(year.session.sheet.months()[NOV].to_string(), "2026-11");
	for op in setup {
		year.session.record(op);
	}

	let mut april_dinner = None;
	let mut january_takeout = None;
	for month in AUG..=JUL {
		let step = month as i64;
		if month < FEB {
			year.run(budget(month, R, 150_000));
			year.run(budget(month, U, 12_000));
			year.run(budget(month, G, 50_000));
			year.run(budget(month, D, 15_000));
		} else {
			year.run(Op::CopyPrevious { month });
		}
		if month == AUG {
			year.run(budget(month, VAC, 20_000));
			year.run(Op::CopyUntilYearEnd {
				month,
				category: VAC,
			});
		}
		if month <= NOV {
			year.run(budget(month, LAP, 25_000));
		}
		year.run(spend(month, 1, PAY, 420_000));
		year.run(spend(month, 1, R, -150_000));
		year.run(spend(month, 12, U, -(9_000 + 700 * (step % 4))));
		let groceries = if step % 2 == 1 { 59_000 } else { 46_000 };
		year.run(spend(month, 9, G, -(groceries + 100 * step)));
		year.run(spend(month, 15, D, -(13_000 + 2_100 * (step % 3))));
		if month == APR {
			april_dinner = Some(year.added_last());
		}
		if month == JAN {
			year.run(spend(month, 22, D, -3_400));
			january_takeout = Some(year.added_last());
		}
		let (groceries, dining, to_budget) = year.balances(month);
		year.run(Op::CoverOverspending {
			month,
			to: G,
			from: Some(D),
		});
		if groceries < 0 && dining > 0 {
			let after = year.balances(month);
			year.covers.push(Cover {
				month,
				groceries,
				dining,
				to_budget,
				after,
			});
		}
		year.run(Op::CoverOverspending {
			month,
			to: D,
			from: None,
		});
	}

	year.run(spend(JAN, 20, G, 3_000));
	year.run(spend(NOV, 5, VAC, -92_000));
	year.run(spend(NOV, 6, LAP, -98_000));
	year.run(Op::CloseGoal {
		goal: VAC,
		to: Some(D),
	});
	year.run(Op::CloseGoal {
		goal: LAP,
		to: None,
	});

	year.run(Op::Hold {
		month: DEC,
		amount: 60_000,
	});
	year.run(Op::Carryover {
		month: JAN,
		category: U,
		flag: true,
	});
	year.run(spend(FEB, 25, U, -20_000));
	let takeout = january_takeout.unwrap();
	let index = year.session.live_index(&takeout);
	year.run(Op::DeleteTransaction { index });
	year.run(Op::Move {
		month: MAR,
		amount: 10_000,
		from: D,
		to: Some(G),
	});
	year.run(Op::Move {
		month: MAR,
		amount: 8_000,
		from: D,
		to: Some(U),
	});
	year.run(Op::AddSplit {
		month: APR,
		day: 14,
		children: vec![(Some(G), -30_000), (Some(D), -8_000)],
	});
	let dinner = april_dinner.unwrap();
	let index = year
		.session
		.added()
		.iter()
		.position(|id| *id == dinner)
		.unwrap();
	year.run(Op::Recategorize {
		index,
		category: Some(G),
	});
	year.run(Op::Average {
		month: JUN,
		span: 0,
	});
	year.run(Op::DeleteSplit { index: 0 });
	year.run(Op::TransferAvailable {
		month: MAY,
		amount: 25_000,
		category: G,
	});
	let replayed = oracle::replay(&zip, &[json!(year.session.oracle_ops())]);
	Some((year, replayed))
}

fn months_of(actual: &Value) -> &Value {
	assert!(
		actual["error"].is_null(),
		"Actual refused an op: {}",
		actual["error"]
	);
	&actual["months"]
}

#[test]
fn a_year_of_budgeting_matches_actual_month_by_month() {
	let Some((year, replayed)) = household_year() else {
		return;
	};
	let actual = &replayed[0];
	let mut differences =
		oracle::compare_months(&year.session.sheet, &year.session.budget, months_of(actual));
	differences.extend(oracle::compare_rows(&year.session.budget, actual));
	assert!(
		differences.is_empty(),
		"{} ops, {} differences from Actual:\n{}",
		year.ops.len(),
		differences.len(),
		differences
			.iter()
			.take(12)
			.cloned()
			.collect::<Vec<_>>()
			.join("\n")
	);
}

#[test]
fn closing_goals_in_todays_month_leaves_them_at_zero_in_actual_too() {
	let Some((year, replayed)) = household_year() else {
		return;
	};
	let months = months_of(&replayed[0]);
	let cell = |month: &str, id: &str, field: &str| -> i64 {
		months[month]["categories"][id][field].as_i64().unwrap()
	};
	assert_eq!(cell("2026-11", VACATION, "balance"), 0);
	assert_eq!(cell("2026-11", LAPTOP, "balance"), 0);
	assert_eq!(
		cell("2026-11", VACATION, "budgeted"),
		20_000 + 12_000,
		"the overspent goal was topped up by what it was short"
	);
	assert_eq!(
		cell("2026-11", LAPTOP, "budgeted"),
		25_000 - 2_000,
		"the leftover left the laptop envelope"
	);
	assert_eq!(
		cell("2026-11", DINING, "budgeted"),
		15_000 - 12_000,
		"Dining Out paid for the overspent vacation"
	);
	assert_eq!(
		cell("2026-12", VACATION, "balance"),
		cell("2026-12", VACATION, "budgeted"),
		"nothing carries out of a closed goal (GOAL-4)"
	);
	let note = replayed[0]["budgetNotes"]
		.as_array()
		.unwrap()
		.iter()
		.find(|row| row["id"] == "budget-2026-11")
		.unwrap()["note"]
		.as_str()
		.unwrap()
		.to_owned();
	assert!(
		note.contains("- Reassigned 120.00 from Dining Out \u{2192} Vacation on November 09"),
		"{note}"
	);
	assert!(
		note.contains("- Reassigned 20.00 from New laptop \u{2192} To Budget on November 09"),
		"{note}"
	);

	for goal in [VACATION, LAPTOP] {
		assert!(year.session.budget.categories[&cat(goal)].hidden);
	}
	for month in ["2027-02", "2027-03"] {
		assert!(
			months[month]["categories"][LAPTOP]["budgeted"]
				.as_i64()
				.unwrap_or(0)
				== 0,
			"copying last month skips the hidden goal"
		);
	}
}

#[test]
fn covering_overspending_moves_exactly_what_groceries_was_short_from_dining() {
	let Some((year, _)) = household_year() else {
		return;
	};
	let partial = year
		.covers
		.iter()
		.filter(|c| -c.groceries > c.dining)
		.count();
	assert!(
		partial >= 1 && year.covers.len() > partial,
		"the year needs a cover Dining Out can pay in full and one it can't: {:?}",
		year.covers
	);
	for cover in &year.covers {
		let Cover {
			month,
			groceries,
			dining,
			to_budget,
			after,
		} = *cover;
		let moved = (-groceries).min(dining);
		assert_eq!(
			after,
			(groceries + moved, dining - moved, to_budget),
			"month {month}: covering {groceries} from {dining}"
		);
	}
}
