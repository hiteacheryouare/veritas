/// What a transaction (or split child) is, for every report (R-REP-1,
/// docs/finance/classification.md). Exactly one per row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransactionKind {
	Income,
	Expense,
	InternalTransfer,
	DebtPrincipal,
	SavingsContribution,
	StartingBalance,
	Adjustment,
}

/// The purpose of a category, stored in `veritas_category_meta`. Categories
/// with no metadata default from Actual's `is_income` flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CategoryKind {
	Income,
	Expense,
	DebtPrincipal,
	Savings,
}

impl CategoryKind {
	pub fn default_for(is_income: bool) -> Self {
		if is_income {
			CategoryKind::Income
		} else {
			CategoryKind::Expense
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AccountKind {
	Asset,
	Liability,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BudgetType {
	Envelope,
	Tracking,
}

/// R-GOAL-3. Moves forward, except an explicit Reopen (docs/finance/debt.md DEBT-8.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GoalStatus {
	Active,
	Reached,
	Closed,
}
