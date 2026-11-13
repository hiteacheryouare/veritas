//! Hand-built budgets shared by the engine's integration tests.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use veritas_domain::{
	Account, AccountId, BudgetCell, BudgetData, BudgetType, Category, CategoryGroup,
	CategoryGroupId, CategoryId, CivilDate, Money, SplitRole, Transaction, TransactionId,
	YearMonth,
};
use veritas_engine::{BudgetEditor, BudgetWrites, EnvelopeSheet, NumberFormat};

pub fn ym(text: &str) -> YearMonth {
	YearMonth::parse(text).unwrap()
}

pub fn day(text: &str) -> CivilDate {
	let (year, rest) = text.split_once('-').unwrap();
	let (month, day) = rest.split_once('-').unwrap();
	CivilDate::new(
		year.parse().unwrap(),
		month.parse().unwrap(),
		day.parse().unwrap(),
	)
	.unwrap()
}

pub fn cat(id: &str) -> CategoryId {
	CategoryId::new(id)
}

pub fn cents(minor: i64) -> Money {
	Money::from_minor(minor)
}

pub struct Budget(pub BudgetData);

impl Budget {
	/// The groups and categories every new Actual budget starts with.
	pub fn actual_seed() -> Self {
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

	pub fn group(&mut self, id: &str, is_income: bool, sort_order: f64) -> &mut CategoryGroup {
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
	pub fn category(&mut self, id: &str, group: &str, is_income: bool) -> &mut Category {
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

	pub fn account(&mut self, id: &str, off_budget: bool) -> &mut Account {
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

	pub fn txn(
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
				imported_payee: None,
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

	pub fn cell(&mut self, month: &str, category: &str) -> &mut BudgetCell {
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

	pub fn budget(&mut self, month: &str, category: &str, amount: i64) {
		self.cell(month, category).budgeted = cents(amount);
	}

	/// Actual's set-carryover: every month from `from` to the last created one.
	pub fn roll_over(&mut self, from: &str, through: &str, category: &str) {
		let mut month = ym(from);
		while month <= ym(through) {
			self.cell(&month.to_string(), category).carryover = true;
			month = month.offset(1).unwrap();
		}
	}

	pub fn hold(&mut self, month: &str, amount: i64) {
		self.0.holds.insert(ym(month), Some(cents(amount)));
	}

	pub fn sheet(&self, today: &str) -> EnvelopeSheet {
		EnvelopeSheet::compute(&self.0, day(today)).unwrap()
	}
}

/// `fixtures/synthetic/small.zip` as its spec builds it
/// (tools/actual-oracle/src/specs/small.ts).
pub fn small_fixture() -> Budget {
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

pub const TODAY: &str = "2026-11-09";

impl Budget {
	/// Runs one budget action against a fresh sheet and applies its writes.
	pub fn edit(&mut self, action: impl FnOnce(&BudgetEditor<'_>) -> BudgetWrites) -> BudgetWrites {
		let sheet = self.sheet(TODAY);
		let writes = action(&BudgetEditor::new(
			&self.0,
			&sheet,
			NumberFormat::CommaDot,
			false,
		));
		writes.apply_to(&mut self.0);
		writes
	}
}
