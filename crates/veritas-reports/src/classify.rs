use std::collections::{BTreeSet, HashMap};

use veritas_domain::{
	AccountId, BudgetData, CategoryId, CategoryKind, CivilDate, Money, SplitRole, Transaction,
	TransactionId, TransactionKind,
};

use crate::Period;

/// The notes Actual's reconcile flow writes on the adjustment it creates. It
/// is localized in Actual, so matching it is best effort.
pub const RECONCILIATION_NOTE: &str = "Reconciliation balance adjustment";

/// Which row of the classification decision table decided a row's kind. The
/// variants are in table order, starting at row 2 (split parents are never
/// classified).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Decision {
	StartingBalance,
	Reconciliation,
	OnBudgetTransfer,
	OffBudgetTransfer,
	DebtPrincipalTransfer,
	SavingsTransfer,
	CapitalizedExpense,
	CapitalizedIncome,
	UncategorizedTransfer,
	OffBudgetRevaluation,
	Income,
	Expense,
	MisfiledTransferCategory,
	Uncategorized,
}

impl Decision {
	pub fn table_row(self) -> u8 {
		match self {
			Decision::StartingBalance => 2,
			Decision::Reconciliation => 3,
			Decision::OnBudgetTransfer => 4,
			Decision::OffBudgetTransfer => 5,
			Decision::DebtPrincipalTransfer => 6,
			Decision::SavingsTransfer => 7,
			Decision::CapitalizedExpense => 8,
			Decision::CapitalizedIncome => 9,
			Decision::UncategorizedTransfer => 10,
			Decision::OffBudgetRevaluation => 11,
			Decision::Income => 12,
			Decision::Expense => 13,
			Decision::MisfiledTransferCategory => 14,
			Decision::Uncategorized => 15,
		}
	}

	pub fn kind(self) -> TransactionKind {
		match self {
			Decision::StartingBalance => TransactionKind::StartingBalance,
			Decision::Reconciliation | Decision::OffBudgetRevaluation => {
				TransactionKind::Adjustment
			}
			Decision::OnBudgetTransfer
			| Decision::OffBudgetTransfer
			| Decision::UncategorizedTransfer => TransactionKind::InternalTransfer,
			Decision::DebtPrincipalTransfer => TransactionKind::DebtPrincipal,
			Decision::SavingsTransfer => TransactionKind::SavingsContribution,
			Decision::CapitalizedIncome | Decision::Income => TransactionKind::Income,
			Decision::CapitalizedExpense
			| Decision::Expense
			| Decision::MisfiledTransferCategory
			| Decision::Uncategorized => TransactionKind::Expense,
		}
	}

	pub fn adjustment_source(self) -> Option<AdjustmentSource> {
		match self {
			Decision::Reconciliation => Some(AdjustmentSource::Reconciliation),
			Decision::OffBudgetRevaluation => Some(AdjustmentSource::OffBudgetRevaluation),
			_ => None,
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AdjustmentSource {
	Reconciliation,
	OffBudgetRevaluation,
}

/// A mirror leg is the off-budget side of an on↔off pair. It copies the
/// on-budget leg's kind, counts in balances and is skipped by every flow total.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Leg {
	Primary,
	Mirror,
}

/// Problems surfaced next to a row. They never change its kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Flags {
	pub needs_review: bool,
	pub misfiled: bool,
	pub kind_conflict: bool,
	pub unpaired: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClassifiedRow {
	pub id: TransactionId,
	pub parent: Option<TransactionId>,
	pub account: AccountId,
	pub date: CivilDate,
	pub amount: Money,
	/// After `category_mapping`, so merged categories report under their target.
	pub category: Option<CategoryId>,
	pub on_budget: bool,
	/// The other account of a transfer, from the payee's `transfer_acct`.
	pub transfer_account: Option<AccountId>,
	pub decision: Decision,
	pub leg: Leg,
	pub flags: Flags,
}

impl ClassifiedRow {
	pub fn kind(&self) -> TransactionKind {
		self.decision.kind()
	}

	pub fn adjustment_source(&self) -> Option<AdjustmentSource> {
		self.decision.adjustment_source()
	}

	pub fn is_primary(&self) -> bool {
		self.leg == Leg::Primary
	}
}

/// Every live, non-parent row of a budget with its kind, sorted by date then id.
#[derive(Clone, Debug, Default)]
pub struct Classification {
	rows: Vec<ClassifiedRow>,
	by_id: HashMap<TransactionId, usize>,
}

impl Classification {
	pub fn rows(&self) -> &[ClassifiedRow] {
		&self.rows
	}

	pub fn get(&self, id: &TransactionId) -> Option<&ClassifiedRow> {
		self.by_id.get(id).and_then(|&index| self.rows.get(index))
	}

	pub fn within(&self, period: Period) -> &[ClassifiedRow] {
		let start = self.rows.partition_point(|row| row.date < period.first());
		let end = self.rows.partition_point(|row| row.date <= period.last());
		self.rows.get(start..end).unwrap_or_default()
	}

	/// On↔off transfers counted as spending only because their category has
	/// no Veritas kind yet. The UI shows this count and the one-time wizard
	/// that assigns kinds to those categories.
	pub fn transfers_counted_as_spending<'a>(
		&'a self,
		budget: &'a BudgetData,
	) -> impl Iterator<Item = &'a ClassifiedRow> + 'a {
		self.rows.iter().filter(|row| {
			row.decision == Decision::CapitalizedExpense
				&& row.is_primary()
				&& row
					.category
					.as_ref()
					.is_some_and(|category| !budget.category_meta.contains_key(category))
		})
	}
}

/// Classifies every row of `budget`. `veritas_adjustments` holds the rows
/// Veritas's own reconcile flow created (`veritas_transaction_meta.adjustment`),
/// which `BudgetData` does not carry yet.
pub fn classify(
	budget: &BudgetData,
	veritas_adjustments: &BTreeSet<TransactionId>,
) -> Classification {
	let classifier = Classifier {
		budget,
		veritas_adjustments,
	};
	let mut rows = Vec::new();
	let mut mirrors = Vec::new();
	for transaction in budget.transactions.values() {
		if !is_row(budget, transaction) {
			continue;
		}
		let facts = classifier.facts(transaction);
		let outcome = classifier.decide(transaction, &facts);
		let row = ClassifiedRow {
			id: transaction.id.clone(),
			parent: match &transaction.split {
				SplitRole::Child { parent } => Some(parent.clone()),
				SplitRole::None | SplitRole::Parent => None,
			},
			account: transaction.account.clone(),
			date: transaction.date,
			amount: transaction.amount,
			category: facts.category,
			on_budget: facts.on_budget,
			transfer_account: facts.transfer.map(|transfer| transfer.account),
			decision: Decision::Uncategorized,
			leg: Leg::Primary,
			flags: Flags::default(),
		};
		match outcome {
			Outcome::Decided(decision, flags) => rows.push(ClassifiedRow {
				decision,
				flags,
				..row
			}),
			Outcome::MirrorOf(primary, flags) => mirrors.push((row, primary, flags)),
		}
	}

	let decided: HashMap<TransactionId, Decision> = rows
		.iter()
		.map(|row| (row.id.clone(), row.decision))
		.collect();
	for (row, primary, flags) in mirrors {
		let classified = match decided.get(&primary) {
			Some(&decision) => ClassifiedRow {
				decision,
				leg: Leg::Mirror,
				flags,
				..row
			},
			None => ClassifiedRow {
				decision: Decision::UncategorizedTransfer,
				flags: Flags {
					needs_review: true,
					unpaired: true,
					..flags
				},
				..row
			},
		};
		rows.push(classified);
	}

	rows.sort_by(|a, b| a.date.cmp(&b.date).then_with(|| a.id.cmp(&b.id)));
	let by_id = rows
		.iter()
		.enumerate()
		.map(|(index, row)| (row.id.clone(), index))
		.collect();
	Classification { rows, by_id }
}

/// Actual's `v_transactions_internal_alive` with `is_parent = 0`: not
/// tombstoned, and a child only while its parent exists and is not tombstoned.
fn is_row(budget: &BudgetData, transaction: &Transaction) -> bool {
	if transaction.tombstone {
		return false;
	}
	match &transaction.split {
		SplitRole::None => true,
		SplitRole::Parent => false,
		SplitRole::Child { parent } => budget
			.transactions
			.get(parent)
			.is_some_and(|parent| !parent.tombstone),
	}
}

enum Outcome {
	Decided(Decision, Flags),
	MirrorOf(TransactionId, Flags),
}

struct TransferFacts {
	account: AccountId,
	other_on_budget: bool,
	partner: Option<TransactionId>,
}

struct RowFacts {
	category: Option<CategoryId>,
	on_budget: bool,
	transfer: Option<TransferFacts>,
	unknown_account: bool,
}

struct Classifier<'a> {
	budget: &'a BudgetData,
	veritas_adjustments: &'a BTreeSet<TransactionId>,
}

impl Classifier<'_> {
	fn on_budget(&self, account: &AccountId) -> Option<bool> {
		self.budget
			.accounts
			.get(account)
			.map(|account| !account.off_budget)
	}

	/// An account missing from the budget is treated as off-budget, as
	/// Actual's budget sums skip it, and its rows are flagged for review.
	fn facts(&self, transaction: &Transaction) -> RowFacts {
		let budget = self.budget;
		let own_side = self.on_budget(&transaction.account);
		let category = transaction
			.category
			.as_ref()
			.map(|category| budget.resolve_category(category))
			.filter(|category| budget.categories.contains_key(category));
		let transfer_account = transaction
			.payee
			.as_ref()
			.map(|payee| budget.resolve_payee(payee))
			.and_then(|payee| budget.payees.get(&payee))
			.and_then(|payee| payee.transfer_account.clone());
		let mut unknown_account = own_side.is_none();
		let transfer = transfer_account.map(|account| {
			let other_side = self.on_budget(&account);
			unknown_account |= other_side.is_none();
			let partner = transaction.transfer.clone().filter(|partner| {
				budget
					.transactions
					.get(partner)
					.is_some_and(|partner| partner.account == account && is_row(budget, partner))
			});
			TransferFacts {
				account,
				other_on_budget: other_side.unwrap_or(false),
				partner,
			}
		});
		RowFacts {
			category,
			on_budget: own_side.unwrap_or(false),
			transfer,
			unknown_account,
		}
	}

	fn is_reconciliation(&self, transaction: &Transaction, facts: &RowFacts) -> bool {
		self.veritas_adjustments.contains(&transaction.id)
			|| (transaction.cleared
				&& transaction.payee.is_none()
				&& facts.category.is_none()
				&& transaction.notes.as_deref() == Some(RECONCILIATION_NOTE))
	}

	/// The category's effective kind. Actual's `is_income` wins over Veritas
	/// metadata on the income/expense axis, and a disagreement is a conflict.
	fn category_kind(&self, category: &CategoryId) -> (CategoryKind, bool) {
		let is_income = self
			.budget
			.categories
			.get(category)
			.is_some_and(|category| category.is_income);
		let stored = self
			.budget
			.category_meta
			.get(category)
			.map(|meta| meta.kind);
		match (is_income, stored) {
			(true, None | Some(CategoryKind::Income)) => (CategoryKind::Income, false),
			(true, Some(_)) => (CategoryKind::Income, true),
			(false, None) => (CategoryKind::Expense, false),
			(false, Some(CategoryKind::Income)) => (CategoryKind::Expense, true),
			(false, Some(kind)) => (kind, false),
		}
	}

	fn decide(&self, transaction: &Transaction, facts: &RowFacts) -> Outcome {
		let mut flags = Flags {
			needs_review: facts.unknown_account,
			..Flags::default()
		};
		if transaction.starting_balance {
			return Outcome::Decided(Decision::StartingBalance, flags);
		}
		if self.is_reconciliation(transaction, facts) {
			return Outcome::Decided(Decision::Reconciliation, flags);
		}
		let category_kind = facts
			.category
			.as_ref()
			.map(|category| self.category_kind(category));

		let Some(transfer) = &facts.transfer else {
			if !facts.on_budget {
				return Outcome::Decided(Decision::OffBudgetRevaluation, flags);
			}
			let decision = match category_kind {
				None => {
					flags.needs_review = true;
					Decision::Uncategorized
				}
				Some((kind, conflict)) => {
					flags.kind_conflict = conflict;
					match kind {
						CategoryKind::Income => Decision::Income,
						CategoryKind::Expense => Decision::Expense,
						CategoryKind::DebtPrincipal | CategoryKind::Savings => {
							flags.misfiled = true;
							Decision::MisfiledTransferCategory
						}
					}
				}
			};
			return Outcome::Decided(decision, flags);
		};

		flags.unpaired = transfer.partner.is_none();
		let decision = match (facts.on_budget, transfer.other_on_budget) {
			(true, true) => {
				flags.needs_review |= facts.category.is_some();
				Decision::OnBudgetTransfer
			}
			(false, false) => {
				flags.needs_review |= facts.category.is_some();
				Decision::OffBudgetTransfer
			}
			(false, true) => match &transfer.partner {
				Some(partner) => return Outcome::MirrorOf(partner.clone(), flags),
				None => {
					flags.needs_review = true;
					Decision::UncategorizedTransfer
				}
			},
			(true, false) => match category_kind {
				None => {
					flags.needs_review = true;
					Decision::UncategorizedTransfer
				}
				Some((kind, conflict)) => {
					flags.kind_conflict = conflict;
					match kind {
						CategoryKind::DebtPrincipal => Decision::DebtPrincipalTransfer,
						CategoryKind::Savings => Decision::SavingsTransfer,
						CategoryKind::Expense => Decision::CapitalizedExpense,
						CategoryKind::Income => Decision::CapitalizedIncome,
					}
				}
			},
		};
		Outcome::Decided(decision, flags)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn every_decision_maps_to_its_table_row_and_kind() {
		use TransactionKind::*;
		let table = [
			(Decision::StartingBalance, 2, StartingBalance),
			(Decision::Reconciliation, 3, Adjustment),
			(Decision::OnBudgetTransfer, 4, InternalTransfer),
			(Decision::OffBudgetTransfer, 5, InternalTransfer),
			(Decision::DebtPrincipalTransfer, 6, DebtPrincipal),
			(Decision::SavingsTransfer, 7, SavingsContribution),
			(Decision::CapitalizedExpense, 8, Expense),
			(Decision::CapitalizedIncome, 9, Income),
			(Decision::UncategorizedTransfer, 10, InternalTransfer),
			(Decision::OffBudgetRevaluation, 11, Adjustment),
			(Decision::Income, 12, Income),
			(Decision::Expense, 13, Expense),
			(Decision::MisfiledTransferCategory, 14, Expense),
			(Decision::Uncategorized, 15, Expense),
		];
		for (decision, row, kind) in table {
			assert_eq!(decision.table_row(), row, "{decision:?}");
			assert_eq!(decision.kind(), kind, "{decision:?}");
		}
	}
}
