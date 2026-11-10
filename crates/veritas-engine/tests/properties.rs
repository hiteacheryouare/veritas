//! Random budget sessions (R-BUD-1, R-BUD-4, PLAN §6.4 family 2 before the
//! oracle half lands): every op runs through the same editor and sheet calls
//! the app will use. After each op the incremental sheet must equal a full
//! rebuild, and every month must account for each dollar:
//! To Budget + for next month + envelope balances = income + spending so far.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;
use proptest::prelude::*;
use veritas_domain::{Money, TransactionId, YearMonth};
use veritas_engine::{
	AverageSpan, BudgetEditor, BudgetWrites, Envelope, EnvelopeSheet, NumberFormat, SheetChange,
};

const EXPENSES: [&str; 6] = [
	"rent",
	"utilities",
	"groceries",
	"dining",
	"food",
	"general",
];

#[derive(Clone, Debug)]
enum Op {
	SetBudget {
		month: usize,
		category: usize,
		amount: i64,
	},
	Move {
		month: usize,
		amount: i64,
		from: usize,
		to: Option<usize>,
	},
	CoverOverspending {
		month: usize,
		to: usize,
		from: Option<usize>,
	},
	CoverOverbudgeted {
		month: usize,
		category: usize,
	},
	Hold {
		month: usize,
		amount: i64,
	},
	ResetHold {
		month: usize,
	},
	TransferAvailable {
		month: usize,
		amount: i64,
		category: usize,
	},
	Carryover {
		month: usize,
		category: usize,
		flag: bool,
	},
	AutoHold {
		month: usize,
		on: bool,
	},
	Average {
		month: usize,
		span: usize,
	},
	CopyPrevious {
		month: usize,
	},
	SetZero {
		month: usize,
	},
	AddTransaction {
		month: usize,
		day: u32,
		category: Option<usize>,
		amount: i64,
	},
	MoveTransaction {
		index: usize,
		month: usize,
	},
	DeleteTransaction {
		index: usize,
	},
}

fn op() -> impl Strategy<Value = Op> {
	let month = 0..19usize;
	let category = 0..EXPENSES.len();
	let amount = -200_000i64..200_000;
	prop_oneof![
		(month.clone(), category.clone(), amount.clone()).prop_map(|(month, category, amount)| {
			Op::SetBudget {
				month,
				category,
				amount,
			}
		}),
		(
			month.clone(),
			amount.clone(),
			category.clone(),
			proptest::option::of(category.clone())
		)
			.prop_map(|(month, amount, from, to)| Op::Move {
				month,
				amount,
				from,
				to
			}),
		(
			month.clone(),
			category.clone(),
			proptest::option::of(category.clone())
		)
			.prop_map(|(month, to, from)| Op::CoverOverspending { month, to, from }),
		(month.clone(), category.clone())
			.prop_map(|(month, category)| Op::CoverOverbudgeted { month, category }),
		(month.clone(), amount.clone()).prop_map(|(month, amount)| Op::Hold { month, amount }),
		month.clone().prop_map(|month| Op::ResetHold { month }),
		(month.clone(), amount.clone(), category.clone()).prop_map(|(month, amount, category)| {
			Op::TransferAvailable {
				month,
				amount,
				category,
			}
		}),
		(month.clone(), category.clone(), any::<bool>()).prop_map(|(month, category, flag)| {
			Op::Carryover {
				month,
				category,
				flag,
			}
		}),
		(month.clone(), any::<bool>()).prop_map(|(month, on)| Op::AutoHold { month, on }),
		(month.clone(), 0..3usize).prop_map(|(month, span)| Op::Average { month, span }),
		month.clone().prop_map(|month| Op::CopyPrevious { month }),
		month.clone().prop_map(|month| Op::SetZero { month }),
		(
			3..19usize,
			1..29u32,
			proptest::option::of(0..EXPENSES.len() + 1),
			-100_000i64..100_000
		)
			.prop_map(|(month, day, category, amount)| Op::AddTransaction {
				month,
				day,
				category,
				amount
			}),
		(0..64usize, 3..19usize).prop_map(|(index, month)| Op::MoveTransaction { index, month }),
		(0..64usize).prop_map(|index| Op::DeleteTransaction { index }),
	]
}

/// The category a generated index names; one past the expenses is income.
fn category_at(index: usize) -> &'static str {
	EXPENSES.get(index).copied().unwrap_or("income")
}

fn envelope(index: Option<usize>) -> Envelope {
	index.map_or(Envelope::ToBudget, |i| {
		Envelope::Category(cat(category_at(i)))
	})
}

struct Session {
	budget: Budget,
	sheet: EnvelopeSheet,
	months: Vec<YearMonth>,
	added: Vec<TransactionId>,
}

impl Session {
	fn new() -> Self {
		let budget = small_fixture();
		let sheet = budget.sheet(TODAY);
		let months = sheet.months().to_vec();
		Session {
			budget,
			sheet,
			months,
			added: Vec::new(),
		}
	}

	fn month(&self, index: usize) -> YearMonth {
		self.months[index % self.months.len()]
	}

	fn edit(&mut self, action: impl FnOnce(&BudgetEditor<'_>) -> BudgetWrites) {
		let editor = BudgetEditor::new(&self.budget.0, &self.sheet, NumberFormat::CommaDot, false);
		let writes = action(&editor);
		writes.apply_to(&mut self.budget.0);
		if let Some(month) = writes.first_month() {
			self.sheet
				.apply(&self.budget.0, SheetChange::Month(month))
				.unwrap();
		}
	}

	fn transactions_changed(&mut self, id: TransactionId) {
		self.sheet
			.apply(&self.budget.0, SheetChange::Transactions(&[id]))
			.unwrap();
	}

	fn run(&mut self, op: &Op) {
		match *op {
			Op::SetBudget {
				month,
				category,
				amount,
			} => {
				let month = self.month(month);
				self.edit(|e| {
					e.set_budget(month, &cat(category_at(category)), cents(amount))
						.unwrap()
				});
			}
			Op::Move {
				month,
				amount,
				from,
				to,
			} => {
				let month = self.month(month);
				self.edit(|e| {
					e.move_budget(month, cents(amount), &cat(category_at(from)), &envelope(to))
						.unwrap()
				});
			}
			Op::CoverOverspending { month, to, from } => {
				let month = self.month(month);
				self.edit(|e| {
					e.cover_overspending(month, &cat(category_at(to)), &envelope(from), None)
						.unwrap()
				});
			}
			Op::CoverOverbudgeted { month, category } => {
				let month = self.month(month);
				self.edit(|e| {
					e.cover_overbudgeted(month, &cat(category_at(category)), None)
						.unwrap()
				});
			}
			Op::Hold { month, amount } => {
				let month = self.month(month);
				let held = self
					.budget
					.0
					.holds
					.get(&month)
					.copied()
					.flatten()
					.unwrap_or(Money::ZERO);
				let to_budget = self.sheet.summary(month).unwrap().to_budget;
				self.edit(|e| {
					e.hold_for_next_month(month, cents(amount))
						.unwrap()
						.unwrap_or_default()
				});
				let now = self
					.budget
					.0
					.holds
					.get(&month)
					.copied()
					.flatten()
					.unwrap_or(Money::ZERO);
				assert!(now >= Money::ZERO, "a hold never goes negative");
				assert!(
					now.minor() <= held.minor() + to_budget.minor().max(0),
					"nor beyond To Budget"
				);
			}
			Op::ResetHold { month } => {
				let month = self.month(month);
				self.edit(|e| e.reset_hold(month));
			}
			Op::TransferAvailable {
				month,
				amount,
				category,
			} => {
				let month = self.month(month);
				self.edit(|e| {
					e.transfer_available(month, cents(amount), &cat(category_at(category)))
						.unwrap()
				});
			}
			Op::Carryover {
				month,
				category,
				flag,
			} => {
				let month = self.month(month);
				self.edit(|e| {
					e.set_carryover(month, &cat(category_at(category)), flag)
						.unwrap()
				});
			}
			Op::AutoHold { month, on } => {
				let month = self.month(month);
				self.edit(|e| e.set_income_auto_hold(month, &cat("income"), on).unwrap());
			}
			Op::Average { month, span } => {
				let month = self.month(month);
				let span = [AverageSpan::Three, AverageSpan::Six, AverageSpan::Twelve][span];
				self.edit(|e| e.set_average(month, span).unwrap());
			}
			Op::CopyPrevious { month } => {
				let month = self.month(month);
				self.edit(|e| e.copy_previous_month(month).unwrap());
			}
			Op::SetZero { month } => {
				let month = self.month(month);
				self.edit(|e| e.set_zero(month).unwrap());
			}
			Op::AddTransaction {
				month,
				day,
				category,
				amount,
			} => {
				let id = format!("p-{}", self.added.len());
				let date = format!("{}-{day:02}", self.month(month));
				let category = category.map(category_at);
				self.budget.txn(&id, "checking", &date, amount, category);
				self.added.push(TransactionId::new(id.clone()));
				self.transactions_changed(TransactionId::new(id));
			}
			Op::MoveTransaction { index, month } => {
				if let Some(id) = self.added.get(index % self.added.len().max(1)).cloned() {
					let date = self.month(month).first_day();
					self.budget.0.transactions.get_mut(&id).unwrap().date = date;
					self.transactions_changed(id);
				}
			}
			Op::DeleteTransaction { index } => {
				if let Some(id) = self.added.get(index % self.added.len().max(1)).cloned() {
					let transaction = self.budget.0.transactions.get_mut(&id).unwrap();
					transaction.tombstone = true;
					self.transactions_changed(id);
				}
			}
		}
	}

	fn check(&self, after: &Op) {
		let fresh = self.budget.sheet(TODAY);
		assert_eq!(self.sheet.months(), fresh.months(), "after {after:?}");
		let mut accounted = 0i64;
		for month in fresh.months() {
			let summary = fresh.summary(*month).unwrap();
			assert_eq!(
				self.sheet.month(*month),
				fresh.month(*month),
				"{month} after {after:?}"
			);
			accounted += summary.total_income.minor() + summary.total_spent.minor();
			let held = summary.to_budget.minor()
				+ summary.for_next_month.minor()
				+ summary.total_balance.minor();
			assert_eq!(
				held, accounted,
				"{month} loses track of money after {after:?}"
			);
		}
	}
}

proptest! {
	#![proptest_config(ProptestConfig::with_cases(256))]

	#[test]
	fn random_sessions_stay_consistent(ops in proptest::collection::vec(op(), 1..40)) {
		let mut session = Session::new();
		for op in &ops {
			session.run(op);
			session.check(op);
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
		let mut session = Session::new();
		let month = session.month(month);
		let before = *session.sheet.summary(month).unwrap();
		session.edit(|e| e.move_budget(month, cents(amount), &cat(EXPENSES[from]), &envelope(Some(to))).unwrap());
		let after = *session.sheet.summary(month).unwrap();
		prop_assert_eq!(before.to_budget, after.to_budget);
		prop_assert_eq!(before.total_balance, after.total_balance);
	}
}
