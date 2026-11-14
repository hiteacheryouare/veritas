// Test helpers outside #[test] fns, so clippy.toml's test allowances miss them.
#![allow(dead_code, clippy::unwrap_used, clippy::panic)]

pub mod actual;
pub mod household;

use veritas_domain::{
	Account, AccountGroup, AccountGroupId, AccountId, BudgetData, BudgetType, Category,
	CategoryGroup, CategoryGroupId, CategoryId, CategoryKind, CategoryMeta, CivilDate, Money,
	Payee, PayeeId, SplitRole, Transaction, TransactionId, TransactionKind, TransactionMeta,
	YearMonth,
};
use veritas_reports::{
	Classification, ClassifiedRow, Leg, NetProfit, Period, classify, net_profit,
};

pub fn date(text: &str) -> CivilDate {
	let parts: Vec<u32> = text.split('-').map(|part| part.parse().unwrap()).collect();
	CivilDate::new(parts[0] as i32, parts[1], parts[2]).unwrap()
}

pub fn month(text: &str) -> Period {
	Period::month(YearMonth::parse(text).unwrap())
}

pub fn period(first: &str, last: &str) -> Period {
	Period::new(date(first), date(last)).unwrap()
}

pub fn cat(id: &str) -> CategoryId {
	CategoryId::new(id)
}

pub fn ids(ids: &[&str]) -> Vec<TransactionId> {
	ids.iter().map(|id| TransactionId::new(*id)).collect()
}

/// A budget built row by row, the way the finance spec's examples read.
pub struct Book {
	pub budget: BudgetData,
}

impl Book {
	pub fn new() -> Self {
		Book {
			budget: BudgetData::empty(BudgetType::Envelope),
		}
	}

	pub fn transfer_payee(account: &str) -> PayeeId {
		PayeeId::new(format!("payee-{account}"))
	}

	/// Adds an account and the transfer payee Actual creates with it.
	pub fn account(&mut self, id: &str, off_budget: bool) -> &mut Self {
		let order = self.budget.accounts.len() as f64;
		self.budget.accounts.insert(
			AccountId::new(id),
			Account {
				last_reconciled: None,
				id: AccountId::new(id),
				name: id.to_owned(),
				off_budget,
				closed: false,
				group: None,
				sort_order: order,
				tombstone: false,
			},
		);
		let payee = Book::transfer_payee(id);
		self.budget
			.payee_mapping
			.insert(payee.clone(), payee.clone());
		self.budget.payees.insert(
			payee.clone(),
			Payee {
				id: payee,
				name: String::new(),
				transfer_account: Some(AccountId::new(id)),
				favorite: false,
				learn_categories: true,
				tombstone: false,
			},
		);
		self
	}

	pub fn account_group(&mut self, id: &str, sort_order: f64) -> &mut Self {
		self.budget.account_groups.insert(
			AccountGroupId::new(id),
			AccountGroup {
				id: AccountGroupId::new(id),
				name: id.to_owned(),
				sort_order,
				tombstone: false,
			},
		);
		self
	}

	pub fn payee(&mut self, id: &str) -> &mut Self {
		self.budget
			.payee_mapping
			.insert(PayeeId::new(id), PayeeId::new(id));
		self.budget.payees.insert(
			PayeeId::new(id),
			Payee {
				id: PayeeId::new(id),
				name: id.to_owned(),
				transfer_account: None,
				favorite: false,
				learn_categories: true,
				tombstone: false,
			},
		);
		self
	}

	pub fn group(&mut self, id: &str, is_income: bool) -> &mut Self {
		let order = self.budget.category_groups.len() as f64;
		self.budget.category_groups.insert(
			CategoryGroupId::new(id),
			CategoryGroup {
				id: CategoryGroupId::new(id),
				name: id.to_owned(),
				is_income,
				hidden: false,
				sort_order: order,
				tombstone: false,
			},
		);
		self
	}

	/// A category in `group`, income when the group is. `kind` is the Veritas
	/// metadata; `None` leaves the category with Actual's data only.
	pub fn category(&mut self, id: &str, group: &str, kind: Option<CategoryKind>) -> &mut Self {
		let is_income = self.budget.category_groups[&CategoryGroupId::new(group)].is_income;
		let order = self.budget.categories.len() as f64;
		self.budget.category_mapping.insert(cat(id), cat(id));
		self.budget.categories.insert(
			cat(id),
			Category {
				id: cat(id),
				name: id.to_owned(),
				group: CategoryGroupId::new(group),
				is_income,
				hidden: false,
				sort_order: order,
				tombstone: false,
			},
		);
		if let Some(kind) = kind {
			self.meta(id, kind);
		}
		self
	}

	pub fn meta(&mut self, id: &str, kind: CategoryKind) -> &mut Self {
		self.budget.category_meta.insert(
			cat(id),
			CategoryMeta {
				kind: Some(kind),
				tax_bucket: None,
				archived_on: None,
			},
		);
		self
	}

	pub fn hide_category(&mut self, id: &str) -> &mut Self {
		self.budget.categories.get_mut(&cat(id)).unwrap().hidden = true;
		self
	}

	pub fn hide_group(&mut self, id: &str) -> &mut Self {
		self.budget
			.category_groups
			.get_mut(&CategoryGroupId::new(id))
			.unwrap()
			.hidden = true;
		self
	}

	/// Archives a category, keeping whatever kind metadata it already has.
	pub fn archive(&mut self, id: &str, on: &str) -> &mut Self {
		self.budget
			.category_meta
			.entry(cat(id))
			.or_insert(CategoryMeta {
				kind: None,
				tax_bucket: None,
				archived_on: None,
			})
			.archived_on = Some(date(on));
		self
	}

	/// Deletes a category the way Actual does when you pick a transfer target:
	/// tombstone it and point its mapping at `target`.
	pub fn delete_category_into(&mut self, id: &str, target: &str) -> &mut Self {
		self.budget.categories.get_mut(&cat(id)).unwrap().tombstone = true;
		self.budget.category_mapping.insert(cat(id), cat(target));
		self
	}

	pub fn row(&mut self, id: &str, account: &str, on: &str, amount: i64) -> &mut Transaction {
		let id = TransactionId::new(id);
		self.budget.transactions.insert(
			id.clone(),
			Transaction {
				id: id.clone(),
				account: AccountId::new(account),
				date: date(on),
				amount: Money::from_minor(amount),
				payee: None,
				imported_payee: None,
				category: None,
				notes: None,
				cleared: true,
				reconciled: false,
				split: SplitRole::None,
				transfer: None,
				schedule: None,
				starting_balance: false,
				sort_order: 0.0,
				tombstone: false,
				split_error: None,
			},
		);
		self.budget.transactions.get_mut(&id).unwrap()
	}

	/// A transfer of `amount` (Actual's sign, seen from `account`) to `other`.
	/// Actual's counterpart is `<id>m`: the opposite amount, no category.
	pub fn transfer(
		&mut self,
		id: &str,
		account: &str,
		other: &str,
		on: &str,
		amount: i64,
	) -> &mut Transaction {
		self.transfer_with_mirror(id, &format!("{id}m"), account, other, on, amount)
	}

	pub fn transfer_with_mirror(
		&mut self,
		id: &str,
		mirror: &str,
		account: &str,
		other: &str,
		on: &str,
		amount: i64,
	) -> &mut Transaction {
		let counterpart = self.row(mirror, other, on, -amount);
		counterpart.payee = Some(Book::transfer_payee(account));
		counterpart.transfer = Some(TransactionId::new(id));
		counterpart.cleared = false;
		let primary = self.row(id, account, on, amount);
		primary.payee = Some(Book::transfer_payee(other));
		primary.transfer = Some(TransactionId::new(mirror));
		primary
	}

	pub fn get(&mut self, id: &str) -> &mut Transaction {
		self.budget
			.transactions
			.get_mut(&TransactionId::new(id))
			.unwrap()
	}

	pub fn parent(&mut self, id: &str, account: &str, on: &str, amount: i64) -> &mut Transaction {
		let parent = self.row(id, account, on, amount);
		parent.split = SplitRole::Parent;
		parent
	}

	/// Marks a row as made by Veritas's reconcile flow (CLS-5).
	pub fn mark_adjustment(&mut self, id: &str) {
		self.budget.transaction_meta.insert(
			TransactionId::new(id),
			TransactionMeta {
				adjustment: true,
				needs_actual: None,
			},
		);
	}

	pub fn classify(&self) -> Classification {
		classify(&self.budget)
	}

	pub fn net_profit(&self, period: Period) -> NetProfit {
		net_profit(&self.budget, &self.classify(), period).unwrap()
	}
}

pub trait RowExt {
	fn in_category(&mut self, category: &str) -> &mut Self;
	fn child_of(&mut self, parent: &str) -> &mut Self;
}

impl RowExt for Transaction {
	fn in_category(&mut self, category: &str) -> &mut Self {
		self.category = Some(cat(category));
		self
	}

	fn child_of(&mut self, parent: &str) -> &mut Self {
		self.split = SplitRole::Child {
			parent: TransactionId::new(parent),
		};
		self
	}
}

pub fn row<'a>(classification: &'a Classification, id: &str) -> &'a ClassifiedRow {
	classification
		.get(&TransactionId::new(id))
		.unwrap_or_else(|| panic!("{id} was not classified"))
}

/// Asserts a row's decision-table row, kind and leg.
pub fn assert_row(
	classification: &Classification,
	id: &str,
	table_row: u8,
	kind: TransactionKind,
	leg: Leg,
) {
	let classified = row(classification, id);
	assert_eq!(
		(
			classified.decision.table_row(),
			classified.kind(),
			classified.leg
		),
		(table_row, kind, leg),
		"{id}"
	);
}

/// The CLS-8 master scenario's accounts and categories.
pub fn master_book() -> Book {
	let mut book = Book::new();
	book.account("CHK", false)
		.account("VISA", false)
		.account("SAV", true)
		.account("LOAN", true)
		.account("BRK", true)
		.account("CAR", true)
		.account("CD", true)
		.payee("Lender")
		.group("Income", true)
		.group("Food", false)
		.group("Shopping", false)
		.group("Debt", false)
		.group("Debt costs", false)
		.group("Saving", false)
		.group("Big purchases", false)
		.category("Starting Balances", "Income", None)
		.category("Paycheck", "Income", None)
		.category("Groceries", "Food", None)
		.category("Clothing", "Shopping", None)
		.category("Loan principal", "Debt", Some(CategoryKind::DebtPrincipal))
		.category("Loan interest", "Debt costs", None)
		.category("Savings", "Saving", Some(CategoryKind::Savings))
		.category("Car purchase", "Big purchases", None);
	for (id, account, balance) in [
		("open-CHK", "CHK", 200_000),
		("open-SAV", "SAV", 500_000),
		("open-LOAN", "LOAN", -980_000),
		("open-BRK", "BRK", 1_000_000),
	] {
		book.row(id, account, "2026-12-31", balance)
			.starting_balance = true;
	}
	book.get("open-CHK").in_category("Starting Balances");
	book
}

/// The master scenario with January and February 2027 (CLS-8).
pub fn master() -> Book {
	let mut book = master_book();
	book.row("T1", "CHK", "2027-01-01", 300_000)
		.in_category("Paycheck");
	book.row("T2", "VISA", "2027-01-03", -5_000)
		.in_category("Groceries");
	book.row("T3", "VISA", "2027-01-05", -4_000)
		.in_category("Clothing");
	book.row("T4", "VISA", "2027-01-09", 1_500)
		.in_category("Clothing");
	book.parent("T5", "CHK", "2027-01-10", -40_000).payee = Some(PayeeId::new("Lender"));
	book.transfer_with_mirror("T5a", "T5m", "CHK", "LOAN", "2027-01-10", -32_000)
		.in_category("Loan principal")
		.child_of("T5");
	book.row("T5b", "CHK", "2027-01-10", -8_000)
		.in_category("Loan interest")
		.child_of("T5")
		.payee = Some(PayeeId::new("Lender"));
	book.transfer("T6", "CHK", "SAV", "2027-01-12", -20_000)
		.in_category("Savings");
	book.row("T7", "CD", "2027-01-15", 100_000).starting_balance = true;
	book.transfer("T8", "CHK", "CAR", "2027-01-20", -50_000)
		.in_category("Car purchase");
	book.row("T9", "BRK", "2027-01-25", 15_000);
	book.row("T10", "CHK", "2027-01-31", -1_200).notes =
		Some("Reconciliation balance adjustment".to_owned());

	book.row("T11", "CHK", "2027-02-01", 300_000)
		.in_category("Paycheck");
	book.row("T12", "VISA", "2027-02-01", -2_500)
		.in_category("Groceries");
	book.transfer("T13", "CHK", "VISA", "2027-02-03", -10_000);
	book
}

/// Balances summed straight from the transactions, never through the
/// classification: live rows that are not split parents, in accounts that
/// exist. Prefix sums keep a query cheap on a 100k-row fixture.
pub struct RawLedger {
	dates: Vec<CivilDate>,
	net_worth: Vec<i64>,
	budget_cash: Vec<i64>,
}

impl RawLedger {
	pub fn new(budget: &BudgetData) -> Self {
		let mut rows: Vec<(CivilDate, bool, i64)> = budget
			.transactions
			.values()
			.filter(|row| !row.tombstone)
			.filter(|row| match &row.split {
				SplitRole::None => true,
				SplitRole::Parent => false,
				SplitRole::Child { parent } => budget
					.transactions
					.get(parent)
					.is_some_and(|parent| !parent.tombstone),
			})
			.filter_map(|row| {
				let account = budget.accounts.get(&row.account)?;
				Some((row.date, !account.off_budget, row.amount.minor()))
			})
			.collect();
		rows.sort_by_key(|(date, _, _)| *date);
		let mut ledger = RawLedger {
			dates: Vec::with_capacity(rows.len()),
			net_worth: vec![0],
			budget_cash: vec![0],
		};
		for (date, on_budget, amount) in rows {
			ledger.dates.push(date);
			ledger
				.net_worth
				.push(ledger.net_worth.last().unwrap() + amount);
			let cash = if on_budget { amount } else { 0 };
			ledger
				.budget_cash
				.push(ledger.budget_cash.last().unwrap() + cash);
		}
		ledger
	}

	fn through(&self, on: CivilDate) -> usize {
		self.dates.partition_point(|date| *date <= on)
	}

	fn before(&self, first: CivilDate) -> usize {
		self.dates.partition_point(|date| *date < first)
	}

	pub fn net_worth(&self, on: CivilDate) -> i64 {
		self.net_worth[self.through(on)]
	}

	pub fn net_worth_before(&self, first: CivilDate) -> i64 {
		self.net_worth[self.before(first)]
	}

	pub fn budget_cash(&self, on: CivilDate) -> i64 {
		self.budget_cash[self.through(on)]
	}

	pub fn budget_cash_before(&self, first: CivilDate) -> i64 {
		self.budget_cash[self.before(first)]
	}
}
