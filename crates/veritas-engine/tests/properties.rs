//! Random budget sessions (R-BUD-1, R-BUD-4): after every op the incremental
//! sheet must equal a full rebuild, and every month must account for each
//! dollar. `oracle_sessions.rs` replays the same sessions through Actual.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
mod session;

use common::*;
use proptest::prelude::*;
use session::{Session, Universe, op_strategy};
use veritas_domain::AccountId;
use veritas_engine::Envelope;

const EXPENSES: [&str; 6] = [
	"rent",
	"utilities",
	"groceries",
	"dining",
	"food",
	"general",
];

fn session() -> Session {
	let universe = Universe {
		expenses: EXPENSES.iter().map(|id| cat(id)).collect(),
		income: cat("income"),
		account: AccountId::new("checking"),
	};
	Session::new(small_fixture().0, day(TODAY), universe)
}

proptest! {
	#![proptest_config(ProptestConfig::with_cases(256))]

	#[test]
	fn random_sessions_stay_consistent(ops in proptest::collection::vec(op_strategy(19, EXPENSES.len()), 1..40)) {
		let mut session = session();
		for op in &ops {
			session.run(op);
			session.assert_consistent(op);
		}
	}

	#[test]
	fn moving_budget_between_envelopes_never_changes_to_budget(
		month in 0..19usize,
		amount in -500_000i64..500_000,
		from in 0..EXPENSES.len(),
		to in 0..EXPENSES.len(),
	) {
		prop_assume!(from != to);
		let budget = small_fixture();
		let sheet = budget.sheet(TODAY);
		let month = sheet.months()[month];
		let before = *sheet.summary(month).unwrap();
		let mut after_budget = budget.0.clone();
		veritas_engine::BudgetEditor::new(&budget.0, &sheet, veritas_engine::NumberFormat::CommaDot, false)
			.move_budget(month, cents(amount), &cat(EXPENSES[from]), &Envelope::Category(cat(EXPENSES[to])))
			.unwrap()
			.apply_to(&mut after_budget);
		let after = *veritas_engine::EnvelopeSheet::compute(&after_budget, day(TODAY)).unwrap().summary(month).unwrap();
		prop_assert_eq!(before.to_budget, after.to_budget);
		prop_assert_eq!(before.total_balance, after.total_balance);
	}
}
