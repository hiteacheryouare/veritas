use std::ops::Bound;

use veritas_domain::{BudgetData, CategoryId, CoreError, Money};

use super::activity::Activity;
use super::layout::Layout;
use super::{CategoryMonth, GroupMonth, MonthSummary};
use crate::amount_field::MAX_SAFE_MINOR;

#[derive(Clone, Debug)]
pub(super) struct MonthData {
	pub summary: MonthSummary,
	/// Indexed like `Layout::groups` and `Layout::categories`.
	pub groups: Vec<GroupMonth>,
	pub categories: Vec<CategoryMonth>,
}

/// One month's cells from `envelope.ts:33-226`, given the month before (`None`
/// for the first month, whose predecessor is Actual's blank sheet of zeros).
pub(super) fn compute(
	layout: &Layout,
	activity: &Activity,
	budget: &BudgetData,
	index: usize,
	previous: Option<&MonthData>,
) -> Result<MonthData, CoreError> {
	let month = layout
		.months
		.get(index)
		.copied()
		.ok_or(CoreError::Internal(
			"the sheet asked for a month outside its range".to_owned(),
		))?;

	let mut cells = vec![(0i64, false); layout.categories.len()];
	let from = (month, CategoryId::new(""));
	let to = (month.offset(1)?, CategoryId::new(""));
	for ((_, category), cell) in budget
		.budget_cells
		.range((Bound::Included(from), Bound::Excluded(to)))
	{
		if let Some(&i) = layout.category_index.get(category) {
			cells[i] = (cell.budgeted.minor(), cell.carryover);
		}
	}

	let mut categories = Vec::with_capacity(layout.categories.len());
	for (i, category) in layout.categories.iter().enumerate() {
		let (budgeted, carryover) = cells[i];
		let spent = activity.get(index, i);
		let balance = if category.is_income {
			0
		} else {
			let (rolls_over, last) = previous
				.and_then(|p| p.categories.get(i))
				.map_or((false, 0), |p| (p.carryover, p.balance.minor()));
			let carried = if rolls_over { last } else { last.max(0) };
			safe(add(add(budgeted, spent)?, carried)?)?
		};
		categories.push(CategoryMonth {
			is_income: category.is_income,
			budgeted: Money::from_minor(budgeted),
			spent: Money::from_minor(spent),
			balance: Money::from_minor(balance),
			carryover,
		});
	}

	let mut groups = Vec::with_capacity(layout.groups.len());
	for group in &layout.groups {
		let members = || group.categories.iter().filter_map(|&i| categories.get(i));
		let sum =
			|field: fn(&CategoryMonth) -> Money| safe_sum(members().map(|c| field(c).minor()));
		let (budgeted, balance) = if group.is_income {
			(0, 0)
		} else {
			(sum(|c| c.budgeted)?, sum(|c| c.balance)?)
		};
		groups.push(GroupMonth {
			is_income: group.is_income,
			spent: Money::from_minor(sum(|c| c.spent)?),
			budgeted: Money::from_minor(budgeted),
			balance: Money::from_minor(balance),
		});
	}

	let expense_groups = || {
		layout
			.groups
			.iter()
			.zip(&groups)
			.filter(|(g, _)| !g.is_income)
			.map(|(_, sums)| sums)
	};
	let total_income = layout
		.income_group
		.and_then(|i| groups.get(i))
		.map_or(0, |g| g.spent.minor());
	let (last_to_budget, last_for_next_month) = previous.map_or((0, 0), |p| {
		(
			p.summary.to_budget.minor(),
			p.summary.for_next_month.minor(),
		)
	});
	let from_last_month = safe(add(last_to_budget, last_for_next_month)?)?;
	let available_funds = safe(add(total_income, from_last_month)?)?;
	let last_month_overspent = match previous {
		None => 0,
		Some(p) => safe_sum(
			layout
				.categories
				.iter()
				.zip(&p.categories)
				.filter(|(c, last)| !c.is_income && !last.carryover)
				.map(|(_, last)| last.balance.minor().min(0)),
		)?,
	};
	let total_budgeted = -safe_sum(expense_groups().map(|g| g.budgeted.minor()))?;
	let manual_hold = budget.holds.get(&month).copied();
	let auto_hold = safe_sum(
		categories
			.iter()
			.filter(|c| c.is_income && c.carryover)
			.map(|c| c.spent.minor()),
	)?;
	// A NULL hold passes Actual's `!== 0` test, so it wins and then reads as 0.
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

	Ok(MonthData {
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
pub(crate) fn safe(value: i64) -> Result<i64, CoreError> {
	if value.unsigned_abs() > MAX_SAFE_MINOR.unsigned_abs() {
		Err(CoreError::Overflow)
	} else {
		Ok(value)
	}
}

fn safe_sum(values: impl Iterator<Item = i64>) -> Result<i64, CoreError> {
	safe(values.into_iter().try_fold(0, add)?)
}
