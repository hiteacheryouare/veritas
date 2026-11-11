// Test helpers outside #[test] fns, so clippy.toml's test allowances miss them.
#![allow(dead_code, clippy::unwrap_used, clippy::panic)]

use veritas_domain::{
	Account, AccountId, BudgetCell, BudgetData, BudgetType, Category, CategoryGroup,
	CategoryGroupId, CategoryId, CategoryKind, CategoryMeta, CivilDate, Goal, GoalId, GoalStatus,
	Money, Payee, PayeeId, ScheduleId, SplitRole, Transaction, TransactionId, YearMonth,
};
use veritas_forecast::{Certainty, FlowKind, KnownFlow};

pub fn date(text: &str) -> CivilDate {
	let parts: Vec<u32> = text.split('-').map(|part| part.parse().unwrap()).collect();
	CivilDate::new(parts[0] as i32, parts[1], parts[2]).unwrap()
}

pub fn ym(text: &str) -> YearMonth {
	YearMonth::parse(text).unwrap()
}

pub fn cat(id: &str) -> CategoryId {
	CategoryId::new(id)
}

pub fn acct(id: &str) -> AccountId {
	AccountId::new(id)
}

pub fn cents(minor: i64) -> Money {
	Money::from_minor(minor)
}

/// A budget built row by row, the way forecasting.md's examples read. Every
/// category is named after its id; every group is too.
pub struct Book {
	pub budget: BudgetData,
	next: usize,
}

impl Book {
	pub fn new() -> Self {
		Book {
			budget: BudgetData::empty(BudgetType::Envelope),
			next: 0,
		}
	}

	/// The usual household: checking, a card, off-budget savings and a loan,
	/// an income group and a few expense groups.
	pub fn household() -> Self {
		let mut book = Book::new();
		book.account("CHK", false)
			.account("VISA", false)
			.account("SAV", true)
			.account("LOAN", true)
			.group("Income", true)
			.group("Bills", false)
			.group("Everyday", false)
			.group("Debt", false)
			.group("Goals", false)
			.category("Paycheck", "Income")
			.category("Side gigs", "Income")
			.category("Rent", "Bills")
			.category("Electric", "Bills")
			.category("Groceries", "Everyday")
			.category("Dining", "Everyday")
			.category("Fuel", "Everyday")
			.category("Gifts", "Everyday")
			.category("Fun", "Everyday")
			.category("Books", "Everyday")
			.category("Loan interest", "Debt")
			.category("Loan principal", "Debt")
			.kind("Loan principal", CategoryKind::DebtPrincipal);
		book
	}

	pub fn transfer_payee(account: &str) -> PayeeId {
		PayeeId::new(format!("payee-{account}"))
	}

	pub fn account(&mut self, id: &str, off_budget: bool) -> &mut Self {
		let order = self.budget.accounts.len() as f64;
		self.budget.accounts.insert(
			acct(id),
			Account {
				id: acct(id),
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
				transfer_account: Some(acct(id)),
				favorite: false,
				learn_categories: true,
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

	pub fn category(&mut self, id: &str, group: &str) -> &mut Self {
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
		self
	}

	pub fn kind(&mut self, id: &str, kind: CategoryKind) -> &mut Self {
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

	pub fn hide(&mut self, id: &str) -> &mut Self {
		self.budget.categories.get_mut(&cat(id)).unwrap().hidden = true;
		self
	}

	pub fn row(&mut self, account: &str, on: &str, amount: i64) -> &mut Transaction {
		self.next += 1;
		let id = TransactionId::new(format!("t{:05}", self.next));
		self.row_with_id(id.as_str(), account, on, amount)
	}

	pub fn row_with_id(
		&mut self,
		id: &str,
		account: &str,
		on: &str,
		amount: i64,
	) -> &mut Transaction {
		let id = TransactionId::new(id);
		self.budget.transactions.insert(
			id.clone(),
			Transaction {
				id: id.clone(),
				account: acct(account),
				date: date(on),
				amount: cents(amount),
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
			},
		);
		self.budget.transactions.get_mut(&id).unwrap()
	}

	/// A categorized row on CHK. Spending is negative, as Actual stores it.
	pub fn spend(&mut self, category: &str, on: &str, amount: i64) -> &mut Transaction {
		let row = self.row("CHK", on, amount);
		row.category = Some(cat(category));
		row
	}

	/// `-x` in `category` on the 15th of each listed month (FC-1's `x`).
	pub fn monthly(&mut self, category: &str, months: &[(&str, i64)]) -> &mut Self {
		for (month, x) in months {
			if *x != 0 {
				self.spend(category, &format!("{month}-15"), -x);
			}
		}
		self
	}

	pub fn opening(&mut self, account: &str, on: &str, amount: i64) -> &mut Self {
		self.row(account, on, amount).starting_balance = true;
		self
	}

	/// A transfer of `amount` from `account`'s side, with Actual's counterpart.
	pub fn transfer(
		&mut self,
		account: &str,
		other: &str,
		on: &str,
		amount: i64,
	) -> &mut Transaction {
		self.next += 1;
		let id = format!("x{:05}", self.next);
		let mirror = format!("{id}m");
		let counterpart = self.row_with_id(&mirror, other, on, -amount);
		counterpart.payee = Some(Book::transfer_payee(account));
		counterpart.transfer = Some(TransactionId::new(&id));
		let primary = self.row_with_id(&id, account, on, amount);
		primary.payee = Some(Book::transfer_payee(other));
		primary.transfer = Some(TransactionId::new(mirror));
		primary
	}

	pub fn budgeted(&mut self, month: &str, category: &str, amount: i64) -> &mut Self {
		self.budget.budget_cells.insert(
			(ym(month), cat(category)),
			BudgetCell {
				month: ym(month),
				category: cat(category),
				budgeted: cents(amount),
				carryover: false,
				goal: None,
				long_goal: false,
			},
		);
		self
	}

	pub fn goal(
		&mut self,
		id: &str,
		category: &str,
		target: i64,
		target_date: Option<&str>,
	) -> &mut Goal {
		self.budget.goals.insert(
			GoalId::new(id),
			Goal {
				id: GoalId::new(id),
				category: cat(category),
				target: cents(target),
				target_date: target_date.map(date),
				icon: None,
				color: None,
				status: GoalStatus::Active,
				reached_month: None,
				closed_month: None,
				released_to: None,
				released: None,
			},
		);
		self.budget.goals.get_mut(&GoalId::new(id)).unwrap()
	}
}

pub trait RowExt {
	fn bill(&mut self, schedule: &str) -> &mut Self;
	fn payee_is(&mut self, payee: &str) -> &mut Self;
	fn in_category(&mut self, category: &str) -> &mut Self;
	fn child_of(&mut self, parent: &str) -> &mut Self;
}

impl RowExt for Transaction {
	fn bill(&mut self, schedule: &str) -> &mut Self {
		self.schedule = Some(ScheduleId::new(schedule));
		self
	}

	fn payee_is(&mut self, payee: &str) -> &mut Self {
		self.payee = Some(PayeeId::new(payee));
		self
	}

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

/// An exact bill or paycheck occurrence on CHK.
pub fn known(on: &str, category: &str, amount: i64, kind: FlowKind) -> KnownFlow {
	KnownFlow {
		date: date(on),
		account: acct("CHK"),
		amount: cents(amount),
		kind,
		category: Some(cat(category)),
		schedule: Some(ScheduleId::new(category)),
		name: category.to_owned(),
		certainty: Certainty::Exact,
	}
}

/// The same flow on the 1st of `count` months starting at `first`.
pub fn monthly_flows(
	first: &str,
	count: i32,
	category: &str,
	amount: i64,
	kind: FlowKind,
) -> Vec<KnownFlow> {
	(0..count)
		.map(|step| {
			let month = ym(first).offset(step).unwrap();
			known(&format!("{month}-01"), category, amount, kind.clone())
		})
		.collect()
}
