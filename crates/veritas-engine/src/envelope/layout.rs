use std::collections::HashMap;

use veritas_domain::{BudgetData, CategoryGroupId, CategoryId, CivilDate, CoreError, YearMonth};

/// Which groups, categories and months get cells, in Actual's order.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Layout {
	pub months: Vec<YearMonth>,
	/// Every live category of every live group, in group order.
	pub categories: Vec<CategoryLayout>,
	pub category_index: HashMap<CategoryId, usize>,
	pub groups: Vec<GroupLayout>,
	pub group_index: HashMap<CategoryGroupId, usize>,
	/// Actual's sheet throws without one; Veritas reads income as 0 (D46).
	pub income_group: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct GroupLayout {
	pub id: CategoryGroupId,
	pub is_income: bool,
	pub categories: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct CategoryLayout {
	pub id: CategoryId,
	pub is_income: bool,
}

impl Layout {
	pub fn new(budget: &BudgetData, months: Vec<YearMonth>) -> Self {
		let mut live_groups: Vec<_> = budget
			.category_groups
			.values()
			.filter(|g| !g.tombstone)
			.collect();
		live_groups.sort_by(|a, b| {
			a.is_income
				.cmp(&b.is_income)
				.then(a.sort_order.total_cmp(&b.sort_order))
				.then(a.id.cmp(&b.id))
		});
		let mut categories = Vec::new();
		let mut groups = Vec::new();
		for group in live_groups {
			let mut members: Vec<_> = budget
				.categories
				.values()
				.filter(|c| !c.tombstone && c.group == group.id)
				.collect();
			members.sort_by(|a, b| a.sort_order.total_cmp(&b.sort_order).then(a.id.cmp(&b.id)));
			let first = categories.len();
			categories.extend(members.into_iter().map(|c| CategoryLayout {
				id: c.id.clone(),
				is_income: c.is_income,
			}));
			groups.push(GroupLayout {
				id: group.id.clone(),
				is_income: group.is_income,
				categories: (first..categories.len()).collect(),
			});
		}
		Layout {
			months,
			category_index: categories
				.iter()
				.enumerate()
				.map(|(i, c)| (c.id.clone(), i))
				.collect(),
			group_index: groups
				.iter()
				.enumerate()
				.map(|(i, g)| (g.id.clone(), i))
				.collect(),
			income_group: groups.iter().position(|g| g.is_income),
			categories,
			groups,
		}
	}

	pub fn month_index(&self, month: YearMonth) -> Option<usize> {
		let first = *self.months.first()?;
		let offset = month_number(month) - month_number(first);
		usize::try_from(offset)
			.ok()
			.filter(|index| *index < self.months.len())
	}
}

fn month_number(month: YearMonth) -> i64 {
	i64::from(month.year()) * 12 + i64::from(month.month())
}

/// Actual's `createAllBudgets`: three months before the earlier of the first
/// transaction's month and today's month, through twelve months after today's.
pub(super) fn created_months(
	earliest: Option<YearMonth>,
	today: CivilDate,
) -> Result<Vec<YearMonth>, CoreError> {
	let current = today.year_month();
	let start = earliest.unwrap_or(current).min(current).offset(-3)?;
	let end = current.offset(12)?;
	let mut months = Vec::new();
	let mut month = start;
	while month <= end {
		months.push(month);
		month = month.offset(1)?;
	}
	Ok(months)
}
