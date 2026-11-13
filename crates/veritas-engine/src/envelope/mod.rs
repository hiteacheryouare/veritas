//! Actual's envelope budget sheet (`packages/loot-core/src/server/budget/
//! envelope.ts` and `base.ts`, v26.11.0) as plain functions over
//! `BudgetData`. Every value matches the cell of the same meaning in Actual:
//! `budget-`, `sum-amount-`, `leftover-` and `carryover-` per category, the
//! group sums, and the month summary that ends in `to-budget`.

mod activity;
mod layout;
mod month;

use std::collections::BTreeMap;

use veritas_domain::{
	BudgetData, CategoryGroupId, CategoryId, CivilDate, CoreError, Money, TransactionId, YearMonth,
};

use activity::Activity;
use layout::{Layout, created_months};
use month::MonthData;

pub(crate) use activity::counted_category;
pub(crate) use month::safe;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CategoryMonth {
	pub is_income: bool,
	/// The month's budgeted amount. Income categories only have one when Actual
	/// stored a budget row for them.
	pub budgeted: Money,
	/// Signed activity: spending is negative, refunds and income positive.
	pub spent: Money,
	/// The envelope's available balance after carryover. Always zero for income.
	pub balance: Money,
	/// "Roll over overspending" for expense categories; "auto hold" for income.
	pub carryover: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GroupMonth {
	pub is_income: bool,
	pub spent: Money,
	/// Zero for income groups, which have no budget cells in Actual.
	pub budgeted: Money,
	pub balance: Money,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MonthSummary {
	/// Activity of the first income group only.
	pub total_income: Money,
	/// Last month's To Budget plus what it held for this month.
	pub from_last_month: Money,
	pub available_funds: Money,
	/// Overspending of last month's envelopes that didn't roll over (≤ 0).
	pub last_month_overspent: Money,
	/// Negated sum of expense budgets (≤ 0 for positive budgets).
	pub total_budgeted: Money,
	/// The manual hold (`zero_budget_months.buffered`); a NULL row reads as 0.
	pub manual_hold: Money,
	/// Income from categories with auto hold on.
	pub auto_hold: Money,
	/// What leaves To Budget for next month: the manual hold if non-zero,
	/// otherwise the auto hold. Actual's UI shows it with the sign inverted.
	pub for_next_month: Money,
	pub to_budget: Money,
	pub total_spent: Money,
	pub total_balance: Money,
}

/// One month of the sheet as plain data, for screen models and tests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonthSheet {
	pub month: YearMonth,
	pub summary: MonthSummary,
	pub groups: BTreeMap<CategoryGroupId, GroupMonth>,
	pub categories: BTreeMap<CategoryId, CategoryMonth>,
}

/// What changed in the budget since the sheet last saw it.
#[derive(Clone, Copy, Debug)]
pub enum SheetChange<'a> {
	/// A budget cell (amount or carryover flag) or the hold of this month.
	Month(YearMonth),
	/// Transactions added, edited, deleted or tombstoned. Split children of a
	/// listed parent are re-read automatically, new ones included.
	Transactions(&'a [TransactionId]),
	/// Categories, groups, accounts or the category mapping: rebuild.
	Structure,
}

/// The sheet for every month Actual creates, memoized per month and category.
/// A change in month M recomputes M through the last month and nothing before.
#[derive(Clone, Debug)]
pub struct EnvelopeSheet {
	today: CivilDate,
	layout: Layout,
	activity: Activity,
	months: Vec<MonthData>,
}

impl EnvelopeSheet {
	/// `today` decides the month range, exactly as Actual's clock does, so tests
	/// and the oracle pin it.
	pub fn compute(budget: &BudgetData, today: CivilDate) -> Result<Self, CoreError> {
		let months = created_months(Activity::earliest_month(budget), today)?;
		let layout = Layout::new(budget, months);
		let activity = Activity::build(budget, &layout)?;
		let mut sheet = EnvelopeSheet {
			today,
			layout,
			activity,
			months: Vec::new(),
		};
		sheet.recompute_from(budget, 0)?;
		Ok(sheet)
	}

	pub fn apply(&mut self, budget: &BudgetData, change: SheetChange<'_>) -> Result<(), CoreError> {
		match change {
			SheetChange::Structure => {
				*self = EnvelopeSheet::compute(budget, self.today)?;
				Ok(())
			}
			SheetChange::Month(month) => {
				let from = self
					.layout
					.months
					.iter()
					.position(|m| *m >= month)
					.unwrap_or(self.layout.months.len());
				self.recompute_from(budget, from)
			}
			SheetChange::Transactions(ids) => {
				let changed = self.activity.refresh(budget, &self.layout, ids)?;
				if created_months(self.activity.earliest(), self.today)? != self.layout.months {
					*self = EnvelopeSheet::compute(budget, self.today)?;
					return Ok(());
				}
				match changed {
					Some(from) => self.recompute_from(budget, from),
					None => Ok(()),
				}
			}
		}
	}

	pub fn today(&self) -> CivilDate {
		self.today
	}

	/// The created months, oldest first. Actual's API lists all but the last.
	pub fn months(&self) -> &[YearMonth] {
		&self.layout.months
	}

	pub fn contains(&self, month: YearMonth) -> bool {
		self.layout.month_index(month).is_some()
	}

	pub fn summary(&self, month: YearMonth) -> Option<&MonthSummary> {
		Some(&self.data(month)?.summary)
	}

	pub fn category(&self, month: YearMonth, category: &CategoryId) -> Option<&CategoryMonth> {
		let index = *self.layout.category_index.get(category)?;
		self.data(month)?.categories.get(index)
	}

	pub fn group(&self, month: YearMonth, group: &CategoryGroupId) -> Option<&GroupMonth> {
		let index = *self.layout.group_index.get(group)?;
		self.data(month)?.groups.get(index)
	}

	pub fn month(&self, month: YearMonth) -> Option<MonthSheet> {
		let data = self.data(month)?;
		Some(MonthSheet {
			month,
			summary: data.summary,
			groups: self
				.layout
				.groups
				.iter()
				.map(|g| g.id.clone())
				.zip(data.groups.iter().copied())
				.collect(),
			categories: self
				.layout
				.categories
				.iter()
				.map(|c| c.id.clone())
				.zip(data.categories.iter().copied())
				.collect(),
		})
	}

	/// Live groups in Actual's order (income last, then sort order and id),
	/// each with its live categories in sort order.
	pub fn group_order(&self) -> impl Iterator<Item = (&CategoryGroupId, Vec<&CategoryId>)> {
		self.layout.groups.iter().map(|group| {
			let categories = group
				.categories
				.iter()
				.filter_map(|&i| self.layout.categories.get(i))
				.map(|c| &c.id)
				.collect();
			(&group.id, categories)
		})
	}

	/// The group whose activity is `total_income`; `None` when the budget has
	/// no income group, which the store's audit reports (D46).
	pub fn income_group(&self) -> Option<&CategoryGroupId> {
		let index = self.layout.income_group?;
		Some(&self.layout.groups.get(index)?.id)
	}

	fn data(&self, month: YearMonth) -> Option<&MonthData> {
		self.months.get(self.layout.month_index(month)?)
	}

	fn recompute_from(&mut self, budget: &BudgetData, from: usize) -> Result<(), CoreError> {
		self.months.truncate(from);
		for index in from..self.layout.months.len() {
			let previous = index.checked_sub(1).and_then(|i| self.months.get(i));
			let data = month::compute(&self.layout, &self.activity, budget, index, previous)?;
			self.months.push(data);
		}
		Ok(())
	}
}
