//! Actual's envelope budget sheet (`packages/loot-core/src/server/budget/
//! envelope.ts` and `base.ts`, v26.11.0) as plain functions over
//! `BudgetData`. Every value matches the cell of the same meaning in Actual:
//! `budget-`, `sum-amount-`, `leftover-` and `carryover-` per category, the
//! group sums, and the month summary that ends in `to-budget`.

use std::collections::{BTreeMap, HashMap};

use veritas_domain::{
	BudgetData, CategoryGroupId, CategoryId, CivilDate, CoreError, Money, SplitRole, YearMonth,
};

use crate::amount_field::MAX_SAFE_MINOR;

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
	/// The manual hold (`zero_budget_months.buffered`).
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonthSheet {
	pub month: YearMonth,
	pub summary: MonthSummary,
	pub groups: BTreeMap<CategoryGroupId, GroupMonth>,
	pub categories: BTreeMap<CategoryId, CategoryMonth>,
}

/// The sheet for every month Actual creates, memoized per month and category.
/// An edit in month M recomputes M through the last month and nothing before.
#[derive(Clone, Debug)]
pub struct EnvelopeSheet {
	today: CivilDate,
	layout: Layout,
	activity: HashMap<(YearMonth, CategoryId), i64>,
	sheets: Vec<MonthSheet>,
}

impl EnvelopeSheet {
	/// `today` decides the month range, exactly as Actual's clock does, so tests
	/// and the oracle pin it.
	pub fn compute(budget: &BudgetData, today: CivilDate) -> Result<Self, CoreError> {
		let layout = Layout::new(budget, today)?;
		let mut sheet = EnvelopeSheet {
			today,
			layout,
			activity: HashMap::new(),
			sheets: Vec::new(),
		};
		sheet.recompute_from(budget, 0)?;
		Ok(sheet)
	}

	/// Brings the sheet up to date after `budget` changed. `earliest_change` is
	/// the first month the change touches: for a transaction moved between
	/// months, the earlier of its old and new month; for a change that affects
	/// every month (an account going off budget, a category mapping), the first
	/// month. Category, group or month-range changes rebuild everything.
	pub fn update(
		&mut self,
		budget: &BudgetData,
		earliest_change: YearMonth,
	) -> Result<(), CoreError> {
		let layout = Layout::new(budget, self.today)?;
		if layout != self.layout {
			*self = EnvelopeSheet::compute(budget, self.today)?;
			return Ok(());
		}
		let from = self
			.layout
			.months
			.iter()
			.position(|m| *m >= earliest_change)
			.unwrap_or(self.layout.months.len());
		self.recompute_from(budget, from)
	}

	pub fn today(&self) -> CivilDate {
		self.today
	}

	/// The created months, oldest first. Actual's API lists all but the last.
	pub fn months(&self) -> &[YearMonth] {
		&self.layout.months
	}

	pub fn month(&self, month: YearMonth) -> Option<&MonthSheet> {
		let index = self.layout.months.binary_search(&month).ok()?;
		self.sheets.get(index)
	}

	pub fn category(&self, month: YearMonth, category: &CategoryId) -> Option<&CategoryMonth> {
		self.month(month)?.categories.get(category)
	}

	/// Live groups in Actual's order (income last, then sort order and id),
	/// each with its live categories in sort order.
	pub fn group_order(&self) -> impl Iterator<Item = (&CategoryGroupId, Vec<&CategoryId>)> {
		self.layout
			.groups
			.iter()
			.map(|group| (&group.id, group.categories.iter().map(|c| &c.id).collect()))
	}

	fn recompute_from(&mut self, budget: &BudgetData, from: usize) -> Result<(), CoreError> {
		self.sheets.truncate(from);
		let Some(&first_changed) = self.layout.months.get(from) else {
			return Ok(());
		};
		let last = self.layout.months.last().copied().unwrap_or(first_changed);
		self.activity.retain(|(month, _), _| *month < first_changed);
		for ((month, category), amount) in activity(budget, first_changed, last)? {
			self.activity.insert((month, category), amount);
		}
		for index in from..self.layout.months.len() {
			let month = self.layout.months[index];
			let previous = index.checked_sub(1).and_then(|i| self.sheets.get(i));
			let sheet = compute_month(&self.layout, &self.activity, budget, month, previous)?;
			self.sheets.push(sheet);
		}
		Ok(())
	}
}

#[derive(Clone, Debug, PartialEq)]
struct Layout {
	months: Vec<YearMonth>,
	groups: Vec<GroupLayout>,
	/// Actual's sheet throws without one; Veritas reads income as 0 (D46).
	income_group: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
struct GroupLayout {
	id: CategoryGroupId,
	is_income: bool,
	categories: Vec<CategoryLayout>,
}

#[derive(Clone, Debug, PartialEq)]
struct CategoryLayout {
	id: CategoryId,
	is_income: bool,
}

impl Layout {
	fn new(budget: &BudgetData, today: CivilDate) -> Result<Self, CoreError> {
		let mut groups: Vec<_> = budget
			.category_groups
			.values()
			.filter(|g| !g.tombstone)
			.collect();
		groups.sort_by(|a, b| {
			a.is_income
				.cmp(&b.is_income)
				.then(a.sort_order.total_cmp(&b.sort_order))
				.then(a.id.cmp(&b.id))
		});
		let groups: Vec<GroupLayout> = groups
			.into_iter()
			.map(|group| {
				let mut categories: Vec<_> = budget
					.categories
					.values()
					.filter(|c| !c.tombstone && c.group == group.id)
					.collect();
				categories
					.sort_by(|a, b| a.sort_order.total_cmp(&b.sort_order).then(a.id.cmp(&b.id)));
				GroupLayout {
					id: group.id.clone(),
					is_income: group.is_income,
					categories: categories
						.into_iter()
						.map(|c| CategoryLayout {
							id: c.id.clone(),
							is_income: c.is_income,
						})
						.collect(),
				}
			})
			.collect();
		let income_group = groups.iter().position(|g| g.is_income);
		Ok(Layout {
			months: created_months(budget, today)?,
			groups,
			income_group,
		})
	}

	fn categories(&self) -> impl Iterator<Item = &CategoryLayout> {
		self.groups.iter().flat_map(|g| &g.categories)
	}
}

/// Actual's `createAllBudgets`: three months before the earlier of the first
/// transaction and today's month, through twelve months after today's month.
/// The first transaction counts tombstoned rows and every account, but not
/// split children.
fn created_months(budget: &BudgetData, today: CivilDate) -> Result<Vec<YearMonth>, CoreError> {
	let current = today.year_month();
	let earliest = budget
		.transactions
		.values()
		.filter(|t| !matches!(t.split, SplitRole::Child { .. }))
		.map(|t| t.date.year_month())
		.min()
		.unwrap_or(current);
	let start = earliest.min(current).offset(-3)?;
	let end = current.offset(12)?;
	let mut months = Vec::new();
	let mut month = start;
	while month <= end {
		months.push(month);
		month = month.offset(1)?;
	}
	Ok(months)
}

/// `sum-amount` for every category and month in `[from, through]`, read the
/// way Actual's `v_transactions_internal_alive` view and budget query read
/// transactions: alive rows on alive accounts' on-budget side, split parents
/// replaced by their alive children, categories resolved through
/// `category_mapping` (a category with no mapping row counts nowhere).
fn activity(
	budget: &BudgetData,
	from: YearMonth,
	through: YearMonth,
) -> Result<HashMap<(YearMonth, CategoryId), i64>, CoreError> {
	let mut sums: HashMap<(YearMonth, CategoryId), i64> = HashMap::new();
	for transaction in budget.transactions.values() {
		if transaction.tombstone {
			continue;
		}
		match &transaction.split {
			SplitRole::Parent => continue,
			SplitRole::Child { parent } => {
				if !budget
					.transactions
					.get(parent)
					.is_some_and(|p| !p.tombstone)
				{
					continue;
				}
			}
			SplitRole::None => {}
		}
		let month = transaction.date.year_month();
		if month < from || month > through {
			continue;
		}
		if !budget
			.accounts
			.get(&transaction.account)
			.is_some_and(|a| !a.off_budget)
		{
			continue;
		}
		let Some(category) = transaction
			.category
			.as_ref()
			.and_then(|c| budget.resolve_category(c))
		else {
			continue;
		};
		let sum = sums.entry((month, category.clone())).or_insert(0);
		*sum = sum
			.checked_add(transaction.amount.minor())
			.ok_or(CoreError::Overflow)?;
	}
	Ok(sums)
}

fn compute_month(
	layout: &Layout,
	activity: &HashMap<(YearMonth, CategoryId), i64>,
	budget: &BudgetData,
	month: YearMonth,
	previous: Option<&MonthSheet>,
) -> Result<MonthSheet, CoreError> {
	let mut categories = BTreeMap::new();
	for category in layout.categories() {
		let spent = activity
			.get(&(month, category.id.clone()))
			.copied()
			.unwrap_or(0);
		let cell = budget.budget_cells.get(&(month, category.id.clone()));
		let budgeted = cell.map_or(0, |c| c.budgeted.minor());
		let carryover = cell.is_some_and(|c| c.carryover);
		let balance = if category.is_income {
			0
		} else {
			let (rolls_over, last_balance) = previous
				.and_then(|p| p.categories.get(&category.id))
				.map_or((false, 0), |p| (p.carryover, p.balance.minor()));
			let carried = if rolls_over {
				last_balance
			} else {
				last_balance.max(0)
			};
			safe(add(add(budgeted, spent)?, carried)?)?
		};
		categories.insert(
			category.id.clone(),
			CategoryMonth {
				is_income: category.is_income,
				budgeted: Money::from_minor(budgeted),
				spent: Money::from_minor(spent),
				balance: Money::from_minor(balance),
				carryover,
			},
		);
	}

	let mut groups = BTreeMap::new();
	for group in &layout.groups {
		let cells = || {
			group
				.categories
				.iter()
				.filter_map(|c| categories.get(&c.id))
		};
		let sum = |field: fn(&CategoryMonth) -> Money| safe_sum(cells().map(|c| field(c).minor()));
		let (budgeted, balance) = if group.is_income {
			(0, 0)
		} else {
			(sum(|c| c.budgeted)?, sum(|c| c.balance)?)
		};
		groups.insert(
			group.id.clone(),
			GroupMonth {
				is_income: group.is_income,
				spent: Money::from_minor(sum(|c| c.spent)?),
				budgeted: Money::from_minor(budgeted),
				balance: Money::from_minor(balance),
			},
		);
	}

	let expense_groups = || {
		layout
			.groups
			.iter()
			.filter(|g| !g.is_income)
			.filter_map(|g| groups.get(&g.id))
	};
	let total_income = layout
		.income_group
		.and_then(|index| layout.groups.get(index))
		.and_then(|g| groups.get(&g.id))
		.map_or(0, |g| g.spent.minor());
	let (last_to_budget, last_for_next_month) = previous.map_or((0, 0), |p| {
		(
			p.summary.to_budget.minor(),
			p.summary.for_next_month.minor(),
		)
	});
	let from_last_month = safe(add(last_to_budget, last_for_next_month)?)?;
	let available_funds = safe(add(total_income, from_last_month)?)?;
	let last_month_overspent = safe_sum(layout.categories().filter(|c| !c.is_income).map(|c| {
		previous
			.and_then(|p| p.categories.get(&c.id))
			.map_or(0, |p| {
				if p.carryover {
					0
				} else {
					p.balance.minor().min(0)
				}
			})
	}))?;
	let total_budgeted = -safe_sum(expense_groups().map(|g| g.budgeted.minor()))?;
	let manual_hold = budget.holds.get(&month).copied();
	let auto_hold = safe_sum(
		layout
			.categories()
			.filter(|c| c.is_income)
			.filter_map(|c| categories.get(&c.id))
			.filter(|c| c.carryover)
			.map(|c| c.spent.minor()),
	)?;
	// A NULL hold is not 0 to Actual's `!== 0` test, so it wins and reads as 0.
	let for_next_month = match manual_hold {
		Some(None) => 0,
		None | Some(Some(Money::ZERO)) => auto_hold,
		Some(Some(hold)) => hold.minor(),
	};
	let to_budget = safe(
		add(add(available_funds, last_month_overspent)?, total_budgeted)?
			.checked_sub(for_next_month)
			.ok_or(CoreError::Overflow)?,
	)?;
	let total_spent = safe_sum(expense_groups().map(|g| g.spent.minor()))?;
	let total_balance = safe_sum(expense_groups().map(|g| g.balance.minor()))?;

	Ok(MonthSheet {
		month,
		summary: MonthSummary {
			total_income: Money::from_minor(total_income),
			from_last_month: Money::from_minor(from_last_month),
			available_funds: Money::from_minor(available_funds),
			last_month_overspent: Money::from_minor(last_month_overspent),
			total_budgeted: Money::from_minor(total_budgeted),
			manual_hold: manual_hold.flatten().unwrap_or(Money::ZERO),
			auto_hold: Money::from_minor(auto_hold),
			for_next_month: Money::from_minor(for_next_month),
			to_budget: Money::from_minor(to_budget),
			total_spent: Money::from_minor(total_spent),
			total_balance: Money::from_minor(total_balance),
		},
		groups,
		categories,
	})
}

fn add(a: i64, b: i64) -> Result<i64, CoreError> {
	a.checked_add(b).ok_or(CoreError::Overflow)
}

/// Actual's `safeNumber`: a formula result beyond ±(2^51 − 1) stops the sheet.
fn safe(value: i64) -> Result<i64, CoreError> {
	if value.unsigned_abs() > MAX_SAFE_MINOR.unsigned_abs() {
		Err(CoreError::Overflow)
	} else {
		Ok(value)
	}
}

fn safe_sum(values: impl Iterator<Item = i64>) -> Result<i64, CoreError> {
	safe(values.into_iter().try_fold(0, add)?)
}
