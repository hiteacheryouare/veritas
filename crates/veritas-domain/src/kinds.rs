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

	/// The kind a category acts as (docs/finance/classification.md CLS-3.1):
	/// Actual's `is_income` wins on the income/expense axis, and the `bool` is
	/// true when the stored kind disagrees with it (`kind_conflict`).
	pub fn effective(is_income: bool, stored: Option<CategoryKind>) -> (CategoryKind, bool) {
		match (is_income, stored) {
			(true, None | Some(CategoryKind::Income)) => (CategoryKind::Income, false),
			(true, Some(_)) => (CategoryKind::Income, true),
			(false, None) => (CategoryKind::Expense, false),
			(false, Some(CategoryKind::Income)) => (CategoryKind::Expense, true),
			(false, Some(kind)) => (kind, false),
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

#[cfg(test)]
mod tests {
	use super::CategoryKind::{self, DebtPrincipal, Expense, Income, Savings};

	/// CLS-3.1 and CLS-3.4: Actual's flag wins, a disagreement is a conflict,
	/// and no metadata means the default for the flag.
	#[test]
	fn cls_3_1_actual_wins_on_the_income_expense_axis() {
		assert_eq!(CategoryKind::effective(true, None), (Income, false));
		assert_eq!(CategoryKind::effective(true, Some(Income)), (Income, false));
		assert_eq!(CategoryKind::effective(true, Some(Savings)), (Income, true));
		assert_eq!(CategoryKind::effective(false, None), (Expense, false));
		assert_eq!(
			CategoryKind::effective(false, Some(Income)),
			(Expense, true)
		);
		assert_eq!(
			CategoryKind::effective(false, Some(DebtPrincipal)),
			(DebtPrincipal, false)
		);
		assert_eq!(
			CategoryKind::effective(false, Some(Savings)),
			(Savings, false)
		);
	}
}
