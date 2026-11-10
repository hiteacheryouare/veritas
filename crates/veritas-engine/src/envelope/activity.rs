use std::collections::{BTreeMap, HashMap};

use veritas_domain::{
	BudgetData, CategoryId, CoreError, SplitRole, Transaction, TransactionId, YearMonth,
};

use super::layout::Layout;

/// Where a transaction lands in Actual's budget query: alive, on an existing
/// on-budget account (closed and tombstoned accounts still count), not a split
/// parent, a child only of an alive parent, and a category that resolves
/// through `category_mapping` (`v_transactions_internal_alive`, `base.ts:133-150`).
pub(crate) fn counted_category<'a>(
	budget: &'a BudgetData,
	transaction: &Transaction,
) -> Option<&'a CategoryId> {
	if transaction.tombstone {
		return None;
	}
	match &transaction.split {
		SplitRole::Parent => return None,
		SplitRole::Child { parent } => {
			budget.transactions.get(parent).filter(|p| !p.tombstone)?;
		}
		SplitRole::None => {}
	}
	budget
		.accounts
		.get(&transaction.account)
		.filter(|a| !a.off_budget)?;
	budget.resolve_category(transaction.category.as_ref()?)
}

/// A transaction's place in the sums: month index, category index, amount.
type Cell = (usize, usize, i64);

/// What one transaction contributed when the sheet last saw it.
#[derive(Clone, Debug)]
struct Record {
	/// Its month, when it counts toward the month range (every non-child row).
	range_month: Option<YearMonth>,
	cell: Option<Cell>,
	parent: Option<TransactionId>,
}

/// `sum-amount` for every created month and live category, kept up to date
/// one transaction at a time.
#[derive(Clone, Debug, Default)]
pub(super) struct Activity {
	columns: usize,
	sums: Vec<i64>,
	records: HashMap<TransactionId, Record>,
	children: HashMap<TransactionId, Vec<TransactionId>>,
	range_months: BTreeMap<YearMonth, usize>,
}

impl Activity {
	/// The first transaction's month for Actual's range: tombstoned rows and
	/// every account count, split children don't (`base.ts:423-425`).
	pub fn earliest_month(budget: &BudgetData) -> Option<YearMonth> {
		budget
			.transactions
			.values()
			.filter(|t| !matches!(t.split, SplitRole::Child { .. }))
			.map(|t| t.date.year_month())
			.min()
	}

	pub fn build(budget: &BudgetData, layout: &Layout) -> Result<Self, CoreError> {
		let mut activity = Activity {
			columns: layout.categories.len(),
			sums: vec![0; layout.months.len() * layout.categories.len()],
			..Activity::default()
		};
		for transaction in budget.transactions.values() {
			activity.insert(budget, layout, transaction)?;
		}
		Ok(activity)
	}

	pub fn get(&self, month: usize, category: usize) -> i64 {
		self.sums
			.get(month * self.columns + category)
			.copied()
			.unwrap_or(0)
	}

	pub fn earliest(&self) -> Option<YearMonth> {
		self.range_months.keys().next().copied()
	}

	/// Re-reads the given transactions (and the children of any split parent
	/// among them, before or after the change) and returns the earliest month
	/// index whose sums changed.
	pub fn refresh(
		&mut self,
		budget: &BudgetData,
		layout: &Layout,
		ids: &[TransactionId],
	) -> Result<Option<usize>, CoreError> {
		let mut affected: Vec<TransactionId> = ids.to_vec();
		for id in ids {
			if let Some(children) = self.children.get(id) {
				affected.extend(children.iter().cloned());
			}
		}
		let mut earliest: Option<usize> = None;
		let mut note = |cell: Option<Cell>| {
			if let Some((month, _, _)) = cell {
				earliest = Some(earliest.map_or(month, |e| e.min(month)));
			}
		};
		let mut seen = std::collections::HashSet::new();
		let mut index = 0;
		while index < affected.len() {
			let id = affected[index].clone();
			index += 1;
			if !seen.insert(id.clone()) {
				continue;
			}
			if let Some(old) = self.remove(&id)? {
				note(old);
			}
			if let Some(transaction) = budget.transactions.get(&id) {
				note(self.insert(budget, layout, transaction)?);
			}
		}
		Ok(earliest)
	}

	fn insert(
		&mut self,
		budget: &BudgetData,
		layout: &Layout,
		transaction: &Transaction,
	) -> Result<Option<Cell>, CoreError> {
		let parent = match &transaction.split {
			SplitRole::Child { parent } => Some(parent.clone()),
			_ => None,
		};
		let range_month = parent.is_none().then(|| transaction.date.year_month());
		let month = transaction.date.year_month();
		let cell = match counted_category(budget, transaction) {
			Some(category) => layout
				.category_index
				.get(category)
				.copied()
				.zip(layout.month_index(month))
				.map(|(category, month)| (month, category, transaction.amount.minor())),
			None => None,
		};
		if let Some((month, category, amount)) = cell {
			self.add(month, category, amount)?;
		}
		if let Some(month) = range_month {
			*self.range_months.entry(month).or_insert(0) += 1;
		}
		if let Some(parent) = &parent {
			self.children
				.entry(parent.clone())
				.or_default()
				.push(transaction.id.clone());
		}
		self.records.insert(
			transaction.id.clone(),
			Record {
				range_month,
				cell,
				parent,
			},
		);
		Ok(cell)
	}

	fn remove(&mut self, id: &TransactionId) -> Result<Option<Option<Cell>>, CoreError> {
		let Some(record) = self.records.remove(id) else {
			return Ok(None);
		};
		if let Some((month, category, amount)) = record.cell {
			let negated = amount.checked_neg().ok_or(CoreError::Overflow)?;
			self.add(month, category, negated)?;
		}
		if let Some(month) = record.range_month
			&& let Some(count) = self.range_months.get_mut(&month)
		{
			*count -= 1;
			if *count == 0 {
				self.range_months.remove(&month);
			}
		}
		if let Some(parent) = &record.parent
			&& let Some(children) = self.children.get_mut(parent)
		{
			children.retain(|child| child != id);
		}
		Ok(Some(record.cell))
	}

	fn add(&mut self, month: usize, category: usize, amount: i64) -> Result<(), CoreError> {
		let index = month * self.columns + category;
		if let Some(sum) = self.sums.get_mut(index) {
			*sum = sum.checked_add(amount).ok_or(CoreError::Overflow)?;
		}
		Ok(())
	}
}
