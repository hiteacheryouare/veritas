//! Random budget sessions shared by the property and oracle tests. Each op
//! runs through `BudgetEditor` and `SheetChange` the way the app will, and is
//! also recorded as the oracle op that makes Actual do the same thing.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;
use serde_json::{Value, json};
use veritas_domain::{
	AccountId, BudgetData, CategoryId, CivilDate, Money, SplitRole, Transaction, TransactionId,
	YearMonth,
};
use veritas_engine::{
	AverageSpan, BudgetEditor, BudgetWrites, Envelope, EnvelopeSheet, NumberFormat, SheetChange,
};

/// The ids ops can name: expense categories by index, then the income category.
#[derive(Clone, Debug)]
pub struct Universe {
	pub expenses: Vec<CategoryId>,
	pub income: CategoryId,
	pub account: AccountId,
}

#[derive(Clone, Debug)]
pub enum Op {
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
	ResetIncomeCarryover {
		month: usize,
	},
	Average {
		month: usize,
		span: usize,
	},
	CategoryAverage {
		month: usize,
		category: usize,
		span: usize,
	},
	CopyPrevious {
		month: usize,
	},
	CopySingle {
		month: usize,
		category: usize,
	},
	CopyUntilYearEnd {
		month: usize,
		category: usize,
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

/// Transactions only land from the fourth month on, so the first transaction
/// (and with it Actual's month range) never moves.
pub fn op_strategy(months: usize, expenses: usize) -> BoxedStrategy<Op> {
	let month = 0..months;
	let category = 0..expenses;
	let amount = -200_000i64..200_000;
	let span = 0..3usize;
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
		month
			.clone()
			.prop_map(|month| Op::ResetIncomeCarryover { month }),
		(month.clone(), span.clone()).prop_map(|(month, span)| Op::Average { month, span }),
		(month.clone(), 0..expenses + 1, span).prop_map(|(month, category, span)| {
			Op::CategoryAverage {
				month,
				category,
				span,
			}
		}),
		month.clone().prop_map(|month| Op::CopyPrevious { month }),
		(month.clone(), category.clone())
			.prop_map(|(month, category)| Op::CopySingle { month, category }),
		(month.clone(), category.clone())
			.prop_map(|(month, category)| Op::CopyUntilYearEnd { month, category }),
		month.clone().prop_map(|month| Op::SetZero { month }),
		(
			3..months,
			1..29u32,
			proptest::option::of(0..expenses + 1),
			-100_000i64..100_000
		)
			.prop_map(|(month, day, category, amount)| Op::AddTransaction {
				month,
				day,
				category,
				amount
			}),
		(0..64usize, 3..months).prop_map(|(index, month)| Op::MoveTransaction { index, month }),
		(0..64usize).prop_map(|index| Op::DeleteTransaction { index }),
	]
	.boxed()
}

const SPANS: [AverageSpan; 3] = [AverageSpan::Three, AverageSpan::Six, AverageSpan::Twelve];

pub struct Session {
	pub budget: BudgetData,
	pub sheet: EnvelopeSheet,
	universe: Universe,
	months: Vec<YearMonth>,
	added: Vec<TransactionId>,
	oracle_ops: Vec<Value>,
}

impl Session {
	pub fn new(budget: BudgetData, today: CivilDate, universe: Universe) -> Self {
		let sheet = EnvelopeSheet::compute(&budget, today).unwrap();
		let months = sheet.months().to_vec();
		Session {
			budget,
			sheet,
			universe,
			months,
			added: Vec::new(),
			oracle_ops: Vec::new(),
		}
	}

	pub fn month_count(&self) -> usize {
		self.months.len()
	}

	/// What Actual has to run to end where this session is.
	pub fn oracle_ops(&self) -> &[Value] {
		&self.oracle_ops
	}

	fn month(&self, index: usize) -> YearMonth {
		self.months[index % self.months.len()]
	}

	fn category(&self, index: usize) -> CategoryId {
		self.universe
			.expenses
			.get(index)
			.cloned()
			.unwrap_or_else(|| self.universe.income.clone())
	}

	fn envelope(&self, index: Option<usize>) -> Envelope {
		index.map_or(Envelope::ToBudget, |i| Envelope::Category(self.category(i)))
	}

	fn edit(
		&mut self,
		action: impl FnOnce(&BudgetEditor<'_>) -> BudgetWrites,
		actual: Vec<(&str, Value)>,
	) {
		let editor = BudgetEditor::new(&self.budget, &self.sheet, NumberFormat::CommaDot, false);
		let writes = action(&editor);
		writes.apply_to(&mut self.budget);
		if let Some(month) = writes.first_month() {
			self.sheet
				.apply(&self.budget, SheetChange::Month(month))
				.unwrap();
		}
		for (name, args) in actual {
			self.oracle_ops
				.push(json!({ "op": "budgetAction", "action": name, "args": args }));
		}
	}

	fn transactions_changed(&mut self, id: TransactionId, actual: Value) {
		self.sheet
			.apply(&self.budget, SheetChange::Transactions(&[id]))
			.unwrap();
		self.oracle_ops.push(actual);
	}

	pub fn run(&mut self, op: &Op) {
		match *op {
			Op::SetBudget {
				month,
				category,
				amount,
			} => {
				let (month, category) = (self.month(month), self.category(category));
				let args = json!({ "month": month.to_string(), "category": category.as_str(), "amount": amount });
				self.edit(
					|e| {
						e.set_budget(month, &category, Money::from_minor(amount))
							.unwrap()
					},
					vec![("budget-amount", args)],
				);
			}
			Op::Move {
				month,
				amount,
				from,
				to,
			} => {
				if to == Some(from) {
					return;
				}
				let (month, from, to) = (self.month(month), self.category(from), self.envelope(to));
				let args = json!({
					"month": month.to_string(),
					"amount": amount,
					"from": from.as_str(),
					"to": envelope_id(&to),
					"currencyCode": ""
				});
				self.edit(
					|e| {
						e.move_budget(month, Money::from_minor(amount), &from, &to)
							.unwrap()
					},
					vec![("transfer-category", args)],
				);
			}
			Op::CoverOverspending { month, to, from } => {
				let (month, to, from) = (self.month(month), self.category(to), self.envelope(from));
				let args = json!({
					"month": month.to_string(),
					"to": to.as_str(),
					"from": envelope_id(&from),
					"currencyCode": ""
				});
				self.edit(
					|e| e.cover_overspending(month, &to, &from, None).unwrap(),
					vec![("cover-overspending", args)],
				);
			}
			Op::CoverOverbudgeted { month, category } => {
				let (month, category) = (self.month(month), self.category(category));
				let args = json!({ "month": month.to_string(), "category": category.as_str(), "currencyCode": "" });
				self.edit(
					|e| e.cover_overbudgeted(month, &category, None).unwrap(),
					vec![("cover-overbudgeted", args)],
				);
			}
			Op::Hold { month, amount } => {
				let month = self.month(month);
				let held = self.held(month);
				let to_budget = self.sheet.summary(month).unwrap().to_budget.minor();
				let args = json!({ "month": month.to_string(), "amount": amount });
				self.edit(
					|e| {
						e.hold_for_next_month(month, Money::from_minor(amount))
							.unwrap()
							.unwrap_or_default()
					},
					vec![("hold-for-next-month", args)],
				);
				let now = self.held(month);
				assert!(now >= 0, "a hold never goes negative");
				assert!(now <= held + to_budget.max(0), "nor beyond To Budget");
			}
			Op::ResetHold { month } => {
				let month = self.month(month);
				self.edit(
					|e| e.reset_hold(month),
					vec![("reset-hold", json!({ "month": month.to_string() }))],
				);
			}
			Op::TransferAvailable {
				month,
				amount,
				category,
			} => {
				let (month, category) = (self.month(month), self.category(category));
				let args = json!({ "month": month.to_string(), "amount": amount, "category": category.as_str() });
				self.edit(
					|e| {
						e.transfer_available(month, Money::from_minor(amount), &category)
							.unwrap()
					},
					vec![("transfer-available", args)],
				);
			}
			Op::Carryover {
				month,
				category,
				flag,
			} => {
				let (month, category) = (self.month(month), self.category(category));
				let args = json!({ "startMonth": month.to_string(), "category": category.as_str(), "flag": flag });
				self.edit(
					|e| e.set_carryover(month, &category, flag).unwrap(),
					vec![("set-carryover", args)],
				);
			}
			Op::AutoHold { month, on } => {
				let (month, income) = (self.month(month), self.universe.income.clone());
				let mut actual = Vec::new();
				if on {
					actual.push(("reset-hold", json!({ "month": month.to_string() })));
				}
				actual.push((
					"set-carryover",
					json!({ "startMonth": month.to_string(), "category": income.as_str(), "flag": on }),
				));
				self.edit(
					|e| e.set_income_auto_hold(month, &income, on).unwrap(),
					actual,
				);
			}
			Op::ResetIncomeCarryover { month } => {
				let month = self.month(month);
				let args = json!({ "month": month.to_string() });
				self.edit(
					|e| e.reset_income_carryover(month),
					vec![("reset-income-carryover", args)],
				);
			}
			Op::Average { month, span } => {
				let month = self.month(month);
				let action = ["set-3month-avg", "set-6month-avg", "set-12month-avg"][span];
				self.edit(
					|e| e.set_average(month, SPANS[span]).unwrap(),
					vec![(action, json!({ "month": month.to_string() }))],
				);
			}
			Op::CategoryAverage {
				month,
				category,
				span,
			} => {
				let (month, category) = (self.month(month), self.category(category));
				let args = json!({ "month": month.to_string(), "N": SPANS[span] as usize, "category": category.as_str() });
				self.edit(
					|e| {
						e.set_category_average(month, &category, SPANS[span])
							.unwrap()
					},
					vec![("set-n-month-avg", args)],
				);
			}
			Op::CopyPrevious { month } => {
				let month = self.month(month);
				let args = json!({ "month": month.to_string() });
				self.edit(
					|e| e.copy_previous_month(month).unwrap(),
					vec![("copy-previous-month", args)],
				);
			}
			Op::CopySingle { month, category } => {
				let (month, category) = (self.month(month), self.category(category));
				let args = json!({ "month": month.to_string(), "category": category.as_str() });
				self.edit(
					|e| e.copy_single_previous_month(month, &category).unwrap(),
					vec![("copy-single-month", args)],
				);
			}
			Op::CopyUntilYearEnd { month, category } => {
				let (month, category) = (self.month(month), self.category(category));
				let args = json!({ "month": month.to_string(), "category": category.as_str() });
				self.edit(
					|e| e.copy_until_year_end(month, &category).unwrap(),
					vec![("copy-until-year-end", args)],
				);
			}
			Op::SetZero { month } => {
				let month = self.month(month);
				self.edit(
					|e| e.set_zero(month).unwrap(),
					vec![("set-zero", json!({ "month": month.to_string() }))],
				);
			}
			Op::AddTransaction {
				month,
				day,
				category,
				amount,
			} => {
				let id =
					TransactionId::new(format!("00000000-0000-4000-8000-{:012}", self.added.len()));
				let date = CivilDate::new(self.month(month).year(), self.month(month).month(), day)
					.unwrap();
				let category = category.map(|c| self.category(c));
				self.budget.transactions.insert(
					id.clone(),
					transaction(&id, &self.universe.account, date, amount, category.clone()),
				);
				self.added.push(id.clone());
				let mut row =
					json!({ "id": id.as_str(), "date": date.to_string(), "amount": amount });
				if let Some(category) = &category {
					row["category"] = json!(category.as_str());
				}
				let actual = json!({ "op": "addTransactions", "account": self.universe.account.as_str(), "transactions": [row] });
				self.transactions_changed(id, actual);
			}
			Op::MoveTransaction { index, month } => {
				let Some(id) = self.pick(index) else { return };
				let date = self.month(month).first_day();
				self.budget.transactions.get_mut(&id).unwrap().date = date;
				let actual = json!({ "op": "updateTransaction", "id": id.as_str(), "fields": { "date": date.to_string() } });
				self.transactions_changed(id, actual);
			}
			Op::DeleteTransaction { index } => {
				let Some(id) = self.pick(index) else { return };
				let transaction = self.budget.transactions.get_mut(&id).unwrap();
				if transaction.tombstone {
					return;
				}
				transaction.tombstone = true;
				let actual = json!({ "op": "deleteTransaction", "id": id.as_str() });
				self.transactions_changed(id, actual);
			}
		}
	}

	fn held(&self, month: YearMonth) -> i64 {
		self.budget
			.holds
			.get(&month)
			.copied()
			.flatten()
			.map_or(0, Money::minor)
	}

	/// Moving or deleting a tombstoned transaction is something only Actual's
	/// API would refuse, so ops pick from the live ones.
	fn pick(&self, index: usize) -> Option<TransactionId> {
		let live: Vec<_> = self
			.added
			.iter()
			.filter(|id| !self.budget.transactions[*id].tombstone)
			.collect();
		live.get(index % live.len().max(1)).map(|id| (*id).clone())
	}

	/// The incremental sheet equals a rebuild, and every month accounts for
	/// each dollar: To Budget + for next month + envelope balances equals the
	/// income and spending of every month so far.
	pub fn assert_consistent(&self, after: &Op) {
		let fresh = EnvelopeSheet::compute(&self.budget, self.sheet.today()).unwrap();
		assert_eq!(self.sheet.months(), fresh.months(), "after {after:?}");
		let mut accounted = 0i64;
		for month in fresh.months() {
			assert_eq!(
				self.sheet.month(*month),
				fresh.month(*month),
				"{month} after {after:?}"
			);
			let summary = fresh.summary(*month).unwrap();
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

fn envelope_id(envelope: &Envelope) -> &str {
	match envelope {
		Envelope::Category(id) => id.as_str(),
		Envelope::ToBudget => "to-budget",
	}
}

fn transaction(
	id: &TransactionId,
	account: &AccountId,
	date: CivilDate,
	amount: i64,
	category: Option<CategoryId>,
) -> Transaction {
	Transaction {
		id: id.clone(),
		account: account.clone(),
		date,
		amount: Money::from_minor(amount),
		payee: None,
		imported_payee: None,
		category,
		notes: None,
		cleared: true,
		reconciled: false,
		split: SplitRole::None,
		transfer: None,
		schedule: None,
		starting_balance: false,
		sort_order: 0.0,
		tombstone: false,
	}
}
