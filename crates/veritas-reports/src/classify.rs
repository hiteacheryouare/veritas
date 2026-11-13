use std::collections::{BTreeMap, HashMap};

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
	/// The other leg of a paired transfer. Its date may differ, since Actual
	/// does not keep the two legs' dates in sync.
	pub partner: Option<Partner>,
	pub decision: Decision,
	pub leg: Leg,
	pub flags: Flags,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Partner {
	pub id: TransactionId,
	pub date: CivilDate,
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

	/// A paired leg inside `period` whose partner is dated outside it.
	pub fn in_transit(&self, period: Period) -> bool {
		period.contains(self.date)
			&& self
				.partner
				.as_ref()
				.is_some_and(|partner| !period.contains(partner.date))
	}
}

/// Every live, non-parent row of a budget with its kind, sorted by date then id.
#[derive(Clone, Debug, Default)]
pub struct Classification {
	rows: Vec<ClassifiedRow>,
	/// Positions in `rows`, in id order, for lookup by id.
	by_id: Vec<usize>,
	orphans: Vec<TransactionId>,
}

impl Classification {
	pub fn rows(&self) -> &[ClassifiedRow] {
		&self.rows
	}

	pub fn get(&self, id: &TransactionId) -> Option<&ClassifiedRow> {
		let found = self
			.by_id
			.binary_search_by(|&position| {
				self.rows
					.get(position)
					.map_or(std::cmp::Ordering::Less, |row| row.id.cmp(id))
			})
			.ok()?;
		self.by_id
			.get(found)
			.and_then(|&position| self.rows.get(position))
	}

	pub fn within(&self, period: Period) -> &[ClassifiedRow] {
		let start = self.rows.partition_point(|row| row.date < period.first());
		let end = self.rows.partition_point(|row| row.date <= period.last());
		self.rows.get(start..end).unwrap_or_default()
	}

	/// Every row dated on or before `date`: what an account balance on that
	/// date sums.
	pub fn through(&self, date: CivilDate) -> &[ClassifiedRow] {
		let end = self.rows.partition_point(|row| row.date <= date);
		self.rows.get(..end).unwrap_or_default()
	}

	pub fn before(&self, date: CivilDate) -> &[ClassifiedRow] {
		let end = self.rows.partition_point(|row| row.date < date);
		self.rows.get(..end).unwrap_or_default()
	}

	/// Live rows whose own account is missing from the budget. No account holds
	/// them, so they are never classified, count in no total, and each one
	/// needs review.
	pub fn orphans(&self) -> &[TransactionId] {
		&self.orphans
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
				&& row.category.as_ref().is_some_and(|category| {
					budget
						.category_meta
						.get(category)
						.and_then(|meta| meta.kind)
						.is_none()
				})
		})
	}

	/// CLS-4.2: each category that [`Self::transfers_counted_as_spending`]
	/// files under, with how many of those transfers it holds. These are the
	/// categories the one-time wizard asks about.
	pub fn categories_to_review(&self, budget: &BudgetData) -> BTreeMap<CategoryId, usize> {
		let mut counts = BTreeMap::new();
		for row in self.transfers_counted_as_spending(budget) {
			if let Some(category) = &row.category {
				*counts.entry(category.clone()).or_insert(0) += 1;
			}
		}
		counts
	}
}

pub fn classify(budget: &BudgetData) -> Classification {
	let mut orphans = Vec::new();
	let mut candidates = Vec::with_capacity(budget.transactions.len());
	for transaction in budget.transactions.values() {
		if !is_row(budget, transaction) {
			continue;
		}
		match Candidate::new(budget, transaction) {
			Some(candidate) => candidates.push(candidate),
			None => orphans.push(transaction.id.clone()),
		}
	}
	let index: HashMap<&TransactionId, usize> = candidates
		.iter()
		.enumerate()
		.map(|(position, candidate)| (&candidate.transaction.id, position))
		.collect();

	let mut decided: Vec<Option<ClassifiedRow>> = Vec::with_capacity(candidates.len());
	let mut mirrors = Vec::new();
	for (position, candidate) in candidates.iter().enumerate() {
		let partner = partner(&candidates, &index, candidate);
		let partner_candidate = partner.and_then(|other| candidates.get(other));
		let row = ClassifiedRow {
			partner: partner_candidate.map(|other| Partner {
				id: other.transaction.id.clone(),
				date: other.transaction.date,
			}),
			..candidate.row()
		};
		match decide(budget, candidate, partner_candidate) {
			Outcome::Decided(decision, flags) => decided.push(Some(ClassifiedRow {
				decision,
				flags,
				..row
			})),
			Outcome::Mirror => {
				decided.push(None);
				mirrors.push((position, row, partner));
			}
		}
	}
	for (position, row, primary) in mirrors {
		let primary = primary
			.and_then(|primary| decided.get(primary))
			.and_then(|primary| primary.as_ref().map(|primary| primary.decision));
		let classified = match primary {
			Some(decision) => ClassifiedRow {
				decision,
				leg: Leg::Mirror,
				..row
			},
			None => ClassifiedRow {
				decision: Decision::UncategorizedTransfer,
				flags: Flags {
					needs_review: true,
					unpaired: true,
					..Flags::default()
				},
				..row
			},
		};
		if let Some(slot) = decided.get_mut(position) {
			*slot = Some(classified);
		}
	}

	// Rows arrive in id order, so a stable sort by date gives date-then-id.
	let mut order: Vec<usize> = (0..decided.len()).collect();
	order.sort_by_key(|&position| {
		decided
			.get(position)
			.and_then(Option::as_ref)
			.map(|row| row.date)
	});
	let mut by_id = vec![0; decided.len()];
	for (sorted, &position) in order.iter().enumerate() {
		if let Some(slot) = by_id.get_mut(position) {
			*slot = sorted;
		}
	}
	let mut slots: Vec<Option<ClassifiedRow>> = decided;
	let rows = order
		.iter()
		.filter_map(|&position| slots.get_mut(position).and_then(Option::take))
		.collect();
	Classification {
		rows,
		by_id,
		orphans,
	}
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

struct Transfer {
	account: AccountId,
	/// A counterpart account missing from the budget counts as off-budget.
	other_on_budget: bool,
}

/// A live row and the facts the decision table reads, before pairing.
struct Candidate<'a> {
	transaction: &'a Transaction,
	category: Option<CategoryId>,
	on_budget: bool,
	transfer: Option<Transfer>,
	starting_balance: bool,
	reconciliation: bool,
}

impl<'a> Candidate<'a> {
	/// `None` when the row's own account is missing: an orphaned row.
	fn new(budget: &BudgetData, transaction: &'a Transaction) -> Option<Self> {
		let on_budget = !budget.accounts.get(&transaction.account)?.off_budget;
		let category = transaction
			.category
			.as_ref()
			.and_then(|category| budget.resolve_category(category))
			.filter(|category| budget.categories.contains_key(*category))
			.cloned();
		let payee = transaction
			.payee
			.as_ref()
			.and_then(|payee| budget.resolve_payee(payee))
			.and_then(|payee| budget.payees.get(payee));
		let transfer = payee
			.and_then(|payee| payee.transfer_account.clone())
			.map(|account| Transfer {
				other_on_budget: budget
					.accounts
					.get(&account)
					.is_some_and(|account| !account.off_budget),
				account,
			});
		let reconciliation = budget
			.transaction_meta
			.get(&transaction.id)
			.is_some_and(|meta| meta.adjustment)
			|| (transaction.cleared
				&& payee.is_none()
				&& category.is_none()
				&& transaction.notes.as_deref() == Some(RECONCILIATION_NOTE));
		Some(Candidate {
			transaction,
			category,
			on_budget,
			transfer,
			starting_balance: transaction.starting_balance,
			reconciliation,
		})
	}

	fn marked(&self) -> bool {
		self.starting_balance || self.reconciliation
	}

	fn row(&self) -> ClassifiedRow {
		let transaction = self.transaction;
		ClassifiedRow {
			id: transaction.id.clone(),
			parent: match &transaction.split {
				SplitRole::Child { parent } => Some(parent.clone()),
				SplitRole::None | SplitRole::Parent => None,
			},
			account: transaction.account.clone(),
			date: transaction.date,
			amount: transaction.amount,
			category: self.category.clone(),
			on_budget: self.on_budget,
			transfer_account: self
				.transfer
				.as_ref()
				.map(|transfer| transfer.account.clone()),
			partner: None,
			decision: Decision::Uncategorized,
			leg: Leg::Primary,
			flags: Flags::default(),
		}
	}
}

/// The other leg, only when the link is reciprocal (each row points at the
/// other and sits in the other's transfer account) and the amounts net to
/// zero. Dates may differ.
fn partner(
	candidates: &[Candidate],
	index: &HashMap<&TransactionId, usize>,
	candidate: &Candidate,
) -> Option<usize> {
	let own = candidate.transaction;
	let transfer = candidate.transfer.as_ref()?;
	let id = own.transfer.as_ref().filter(|id| *id != &own.id)?;
	let position = *index.get(id)?;
	let other = candidates.get(position)?;
	let points_back = other.transaction.transfer.as_ref() == Some(&own.id);
	let other_points_here = other
		.transfer
		.as_ref()
		.is_some_and(|transfer| transfer.account == own.account);
	let nets_to_zero = own
		.amount
		.checked_add(other.transaction.amount)
		.is_ok_and(|sum| sum == Money::ZERO);
	(points_back
		&& other_points_here
		&& nets_to_zero
		&& other.transaction.account == transfer.account)
		.then_some(position)
}

enum Outcome {
	Decided(Decision, Flags),
	Mirror,
}

fn decide(budget: &BudgetData, candidate: &Candidate, partner: Option<&Candidate>) -> Outcome {
	let mut flags = Flags::default();
	let category_kind = candidate
		.category
		.as_ref()
		.map(|category| budget.effective_category_kind(category));

	let Some(transfer) = &candidate.transfer else {
		let decision = if candidate.starting_balance {
			Decision::StartingBalance
		} else if candidate.reconciliation {
			Decision::Reconciliation
		} else if !candidate.on_budget {
			Decision::OffBudgetRevaluation
		} else {
			match category_kind {
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
			}
		};
		return Outcome::Decided(decision, flags);
	};

	let Some(partner) = partner else {
		flags.unpaired = true;
		if candidate.marked() {
			flags.needs_review = true;
			let decision = if candidate.starting_balance {
				Decision::StartingBalance
			} else {
				Decision::Reconciliation
			};
			return Outcome::Decided(decision, flags);
		}
		return Outcome::Decided(
			transfer_decision(candidate, transfer, category_kind, &mut flags),
			flags,
		);
	};

	if !candidate.on_budget && transfer.other_on_budget {
		return Outcome::Mirror;
	}
	flags.needs_review = candidate.marked() || partner.marked();
	Outcome::Decided(
		transfer_decision(candidate, transfer, category_kind, &mut flags),
		flags,
	)
}

/// Rows 4 to 10 for a transfer that is not a mirror leg.
fn transfer_decision(
	candidate: &Candidate,
	transfer: &Transfer,
	category_kind: Option<(CategoryKind, bool)>,
	flags: &mut Flags,
) -> Decision {
	match (candidate.on_budget, transfer.other_on_budget) {
		(true, true) => {
			flags.needs_review |= candidate.category.is_some();
			Decision::OnBudgetTransfer
		}
		(false, false) => {
			flags.needs_review |= candidate.category.is_some();
			Decision::OffBudgetTransfer
		}
		(false, true) => {
			flags.needs_review = true;
			Decision::UncategorizedTransfer
		}
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
