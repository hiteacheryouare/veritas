//! Category and group edits (R-BUD-5) as Actual makes them: its sort orders,
//! its duplicate-name rule, archive instead of delete, and delete-with-
//! reassign through `category_mapping`. Each returns the rows to write, so
//! the command bus applies them as one transaction and one undo step.

use veritas_domain::{
	BudgetCell, BudgetData, Category, CategoryGroup, CategoryGroupId, CategoryId, CategoryMapping,
	CategoryMeta, CategoryMetaRow, CivilDate, CoreError, Money, Row, YearMonth,
};
use veritas_store::new_id;

/// Actual's `SORT_INCREMENT` (loot-core/src/server/db/sort.ts:1).
const SORT_INCREMENT: f64 = 16384.0;

/// Actual's `shoveSortOrders` (db/sort.ts:24-67): the sort order for an item
/// placed before `target` in `items` (sorted by sort order, then id), plus the
/// neighbours shoved down when the gap is too small. No target, or one not in
/// the list, appends at the end.
fn shove<'a, K: PartialEq>(items: &'a [(K, f64)], target: Option<&K>) -> (Vec<(&'a K, f64)>, f64) {
	let to = target.and_then(|target| items.iter().position(|(id, _)| id == target));
	let Some(to) = to else {
		let order = items
			.last()
			.map_or(SORT_INCREMENT, |(_, order)| order + SORT_INCREMENT);
		return (Vec::new(), order);
	};
	let before = to.checked_sub(1).map(|index| items[index].1);
	let mut updates = Vec::new();
	if items[to].1 - before.unwrap_or(0.0) <= 2.0 {
		let mut order = items[to].1.floor() + SORT_INCREMENT;
		for (id, current) in &items[to..] {
			if order <= *current {
				break;
			}
			updates.push((id, order));
			order += SORT_INCREMENT;
		}
	}
	// Actual's midpoint (db/sort.ts:7-22) reads the orders as they were.
	let order = match before {
		None => items[to].1 / 2.0,
		Some(below) => (below + items[to].1) / 2.0,
	};
	(updates, order)
}

fn live_category<'a>(data: &'a BudgetData, id: &CategoryId) -> Result<&'a Category, CoreError> {
	data.categories
		.get(id)
		.filter(|category| !category.tombstone)
		.ok_or_else(|| CoreError::NotFound {
			entity: "category",
			id: id.to_string(),
		})
}

fn live_group<'a>(
	data: &'a BudgetData,
	id: &CategoryGroupId,
) -> Result<&'a CategoryGroup, CoreError> {
	data.category_groups
		.get(id)
		.filter(|group| !group.tombstone)
		.ok_or_else(|| CoreError::NotFound {
			entity: "category group",
			id: id.to_string(),
		})
}

fn name(text: &str, what: &str) -> Result<String, CoreError> {
	let name = text.trim();
	if name.is_empty() {
		return Err(CoreError::Invalid(format!("Enter a name for the {what}.")));
	}
	Ok(name.to_owned())
}

/// Live categories of a group in Actual's order: sort order, then id.
fn group_order(data: &BudgetData, group: &CategoryGroupId) -> Vec<(CategoryId, f64)> {
	let mut items: Vec<(CategoryId, f64)> = data
		.categories
		.values()
		.filter(|c| !c.tombstone && &c.group == group)
		.map(|c| (c.id.clone(), c.sort_order))
		.collect();
	items.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
	items
}

fn groups_order(data: &BudgetData) -> Vec<(CategoryGroupId, f64)> {
	let mut items: Vec<(CategoryGroupId, f64)> = data
		.category_groups
		.values()
		.filter(|g| !g.tombstone)
		.map(|g| (g.id.clone(), g.sort_order))
		.collect();
	items.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
	items
}

/// Actual refuses a second live category with the same name, ignoring case,
/// in one group (db/index.ts:459-468).
fn ensure_free_name(
	data: &BudgetData,
	group: &CategoryGroupId,
	name: &str,
	except: Option<&CategoryId>,
) -> Result<(), CoreError> {
	let taken = data.categories.values().any(|c| {
		!c.tombstone
			&& &c.group == group
			&& Some(&c.id) != except
			&& c.name.to_uppercase() == name.to_uppercase()
	});
	if taken {
		return Err(CoreError::Invalid(
			"This group already has a category with that name. Choose a different one.".to_owned(),
		));
	}
	Ok(())
}

/// A new category at the top of its group, as Actual's sidebar adds one
/// (db/index.ts:470-490), with its self-mapping (`:497-499`). Returns the rows
/// and the new id.
pub fn create_category(
	data: &BudgetData,
	group: &CategoryGroupId,
	text: &str,
) -> Result<(Vec<Row>, CategoryId), CoreError> {
	let parent = live_group(data, group)?;
	let name = name(text, "category")?;
	ensure_free_name(data, group, &name, None)?;
	let items = group_order(data, group);
	let first = items.first().map(|(id, _)| id.clone());
	let (updates, sort_order) = shove(&items, first.as_ref());
	let id = CategoryId::new(new_id());
	let mut rows = sort_rows(data, &updates);
	rows.push(Row::Category(Category {
		id: id.clone(),
		name,
		group: group.clone(),
		is_income: parent.is_income,
		hidden: false,
		sort_order,
		tombstone: false,
	}));
	rows.push(Row::CategoryMapping(CategoryMapping {
		from: id.clone(),
		to: id.clone(),
	}));
	Ok((rows, id))
}

fn sort_rows(data: &BudgetData, updates: &[(&CategoryId, f64)]) -> Vec<Row> {
	updates
		.iter()
		.filter_map(|(id, order)| {
			data.categories.get(*id).map(|category| {
				Row::Category(Category {
					sort_order: *order,
					..category.clone()
				})
			})
		})
		.collect()
}

pub fn rename_category(
	data: &BudgetData,
	id: &CategoryId,
	text: &str,
) -> Result<Vec<Row>, CoreError> {
	let category = live_category(data, id)?;
	let name = name(text, "category")?;
	ensure_free_name(data, &category.group, &name, Some(id))?;
	Ok(vec![Row::Category(Category {
		name,
		..category.clone()
	})])
}

/// Moves a category before `before` in `group`, or to the end of it with no
/// `before`, as Actual's `moveCategory` does (db/index.ts:517-536). Income and
/// expense categories stay among their own kind.
pub fn move_category(
	data: &BudgetData,
	id: &CategoryId,
	group: &CategoryGroupId,
	before: Option<&CategoryId>,
) -> Result<Vec<Row>, CoreError> {
	let category = live_category(data, id)?;
	let target = live_group(data, group)?;
	if target.is_income != category.is_income {
		return Err(CoreError::Invalid(
			"Income and expense categories can't move into each other's groups.".to_owned(),
		));
	}
	if &category.group != group {
		ensure_free_name(data, group, &category.name, Some(id))?;
	}
	let items = group_order(data, group);
	let (updates, sort_order) = shove(&items, before);
	let mut rows = sort_rows(data, &updates);
	rows.push(Row::Category(Category {
		sort_order,
		group: group.clone(),
		..category.clone()
	}));
	Ok(rows)
}

/// Showing a category again also ends its archive.
pub fn set_category_hidden(
	data: &BudgetData,
	id: &CategoryId,
	hidden: bool,
) -> Result<Vec<Row>, CoreError> {
	let category = live_category(data, id)?;
	let mut rows = vec![Row::Category(Category {
		hidden,
		..category.clone()
	})];
	if let Some(meta) = data
		.category_meta
		.get(id)
		.filter(|meta| !hidden && meta.archived_on.is_some())
	{
		rows.push(Row::CategoryMeta(CategoryMetaRow {
			category: id.clone(),
			meta: CategoryMeta {
				archived_on: None,
				..meta.clone()
			},
		}));
	}
	Ok(rows)
}

/// R-BUD-5's Archive: hide the category and record when, so reports can tell
/// archived from merely hidden (CLS-3). History stays where it is.
pub fn archive_category(
	data: &BudgetData,
	id: &CategoryId,
	on: CivilDate,
) -> Result<Vec<Row>, CoreError> {
	let category = live_category(data, id)?;
	let meta = data.category_meta.get(id).cloned().unwrap_or(CategoryMeta {
		kind: None,
		tax_bucket: None,
		archived_on: None,
	});
	Ok(vec![
		Row::Category(Category {
			hidden: true,
			..category.clone()
		}),
		Row::CategoryMeta(CategoryMetaRow {
			category: id.clone(),
			meta: CategoryMeta {
				archived_on: Some(on),
				..meta
			},
		}),
	])
}

/// What a category has that a plain delete would lose. Actual forces a
/// transfer when it has transactions or any non-zero budget
/// (budget/app.ts:494-521).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CategoryHistory {
	pub transactions: usize,
	pub budgeted_months: usize,
}

impl CategoryHistory {
	pub fn is_empty(&self) -> bool {
		self.transactions == 0 && self.budgeted_months == 0
	}
}

pub fn category_history(data: &BudgetData, id: &CategoryId) -> Result<CategoryHistory, CoreError> {
	live_category(data, id)?;
	let transactions = data
		.transactions
		.values()
		.filter(|t| !t.tombstone)
		.filter_map(|t| t.category.as_ref())
		.filter(|category| *category == id || data.resolve_category(category) == Some(id))
		.count();
	let budgeted_months = data
		.budget_cells
		.values()
		.filter(|cell| &cell.category == id && cell.budgeted != Money::ZERO)
		.count();
	Ok(CategoryHistory {
		transactions,
		budgeted_months,
	})
}

/// Deletes a category with no history. One with history must be archived,
/// or deleted with its history moved to another category.
pub fn delete_category(data: &BudgetData, id: &CategoryId) -> Result<Vec<Row>, CoreError> {
	let category = live_category(data, id)?;
	if !category_history(data, id)?.is_empty() {
		return Err(CoreError::Invalid(
			"This category has history. Archive it, or delete it and move its history to another category."
				.to_owned(),
		));
	}
	Ok(vec![Row::Category(Category {
		tombstone: true,
		..category.clone()
	})])
}

/// Actual's `category-delete` with a transfer target (budget/app.ts:378-422,
/// db/index.ts:538-562): in every created month the target's budget becomes
/// its own plus the deleted category's; every mapping that pointed at the
/// deleted category, and its own, now points at the target; the category is
/// tombstoned. Transactions are untouched and read through the mapping, so
/// all history moves to the target. Behind the app's explicit confirmation.
pub fn delete_category_moving_history(
	data: &BudgetData,
	months: &[YearMonth],
	id: &CategoryId,
	to: &CategoryId,
) -> Result<Vec<Row>, CoreError> {
	let category = live_category(data, id)?;
	let target = live_category(data, to)?;
	if id == to {
		return Err(CoreError::Invalid(
			"Choose a different category to move the history to.".to_owned(),
		));
	}
	if category.is_income != target.is_income {
		return Err(CoreError::Invalid(
			"History can't move between income and expense categories.".to_owned(),
		));
	}
	let mut rows = Vec::new();
	if !category.is_income {
		for month in months {
			let moved = budgeted(data, *month, id);
			if moved == Money::ZERO {
				continue;
			}
			let cell = data.budget_cells.get(&(*month, to.clone()));
			rows.push(Row::BudgetCell(BudgetCell {
				month: *month,
				category: to.clone(),
				budgeted: budgeted(data, *month, to).checked_add(moved)?,
				carryover: cell.is_some_and(|cell| cell.carryover),
				goal: cell.and_then(|cell| cell.goal),
				long_goal: cell.is_some_and(|cell| cell.long_goal),
			}));
		}
	}
	for (from, current) in &data.category_mapping {
		if current == id && from != id {
			rows.push(Row::CategoryMapping(CategoryMapping {
				from: from.clone(),
				to: to.clone(),
			}));
		}
	}
	rows.push(Row::CategoryMapping(CategoryMapping {
		from: id.clone(),
		to: to.clone(),
	}));
	rows.push(Row::Category(Category {
		tombstone: true,
		..category.clone()
	}));
	Ok(rows)
}

fn budgeted(data: &BudgetData, month: YearMonth, category: &CategoryId) -> Money {
	data.budget_cells
		.get(&(month, category.clone()))
		.map_or(Money::ZERO, |cell| cell.budgeted)
}

/// Actual refuses a second live group with the same name, ignoring case
/// (db/index.ts:364-379, `:402-416`).
fn ensure_free_group_name(
	data: &BudgetData,
	name: &str,
	except: Option<&CategoryGroupId>,
) -> Result<(), CoreError> {
	let taken = data.category_groups.values().any(|g| {
		!g.tombstone && Some(&g.id) != except && g.name.to_uppercase() == name.to_uppercase()
	});
	if taken {
		return Err(CoreError::Invalid(
			"There's already a group with that name. Choose a different one.".to_owned(),
		));
	}
	Ok(())
}

/// A new expense group after every other, as Actual adds one: the largest
/// live sort order plus 16384 (db/index.ts:381-383).
pub fn create_group(
	data: &BudgetData,
	text: &str,
) -> Result<(Vec<Row>, CategoryGroupId), CoreError> {
	let name = name(text, "group")?;
	ensure_free_group_name(data, &name, None)?;
	let sort_order = groups_order(data)
		.iter()
		.map(|(_, order)| *order)
		.fold(0.0, f64::max)
		+ SORT_INCREMENT;
	let id = CategoryGroupId::new(new_id());
	Ok((
		vec![Row::CategoryGroup(CategoryGroup {
			id: id.clone(),
			name,
			is_income: false,
			hidden: false,
			sort_order,
			tombstone: false,
		})],
		id,
	))
}

pub fn rename_group(
	data: &BudgetData,
	id: &CategoryGroupId,
	text: &str,
) -> Result<Vec<Row>, CoreError> {
	let group = live_group(data, id)?;
	let name = name(text, "group")?;
	ensure_free_group_name(data, &name, Some(id))?;
	Ok(vec![Row::CategoryGroup(CategoryGroup {
		name,
		..group.clone()
	})])
}

/// Actual's `moveCategoryGroup` (db/index.ts:421-434).
pub fn move_group(
	data: &BudgetData,
	id: &CategoryGroupId,
	before: Option<&CategoryGroupId>,
) -> Result<Vec<Row>, CoreError> {
	let group = live_group(data, id)?;
	let items = groups_order(data);
	let (updates, sort_order) = shove(&items, before);
	let mut rows: Vec<Row> = updates
		.into_iter()
		.filter_map(|(gid, order)| {
			data.category_groups.get(gid).map(|g| {
				Row::CategoryGroup(CategoryGroup {
					sort_order: order,
					..g.clone()
				})
			})
		})
		.collect();
	rows.push(Row::CategoryGroup(CategoryGroup {
		sort_order,
		..group.clone()
	}));
	Ok(rows)
}

pub fn set_group_hidden(
	data: &BudgetData,
	id: &CategoryGroupId,
	hidden: bool,
) -> Result<Vec<Row>, CoreError> {
	let group = live_group(data, id)?;
	Ok(vec![Row::CategoryGroup(CategoryGroup {
		hidden,
		..group.clone()
	})])
}

#[cfg(test)]
mod tests {
	use super::*;

	fn items(orders: &[f64]) -> Vec<(String, f64)> {
		orders
			.iter()
			.enumerate()
			.map(|(index, order)| (format!("c{index}"), *order))
			.collect()
	}

	#[test]
	fn placing_before_the_first_halves_its_order() {
		let list = items(&[16384.0, 32768.0]);
		let (updates, order) = shove(&list, Some(&"c0".to_owned()));
		assert!(updates.is_empty());
		assert_eq!(order, 8192.0);
	}

	#[test]
	fn placing_between_two_takes_the_unrounded_midpoint() {
		let list = items(&[16384.0, 16385.5]);
		let (updates, order) = shove(&list, Some(&"c1".to_owned()));
		assert_eq!(order, 16384.75);
		assert_eq!(updates.len(), 1, "a gap of 2 or less shoves the rest down");
	}

	#[test]
	fn a_missing_target_or_none_appends_at_the_end() {
		let list = items(&[100.0, 200.0]);
		assert_eq!(shove(&list, None).1, 200.0 + SORT_INCREMENT);
		assert_eq!(
			shove(&list, Some(&"nope".to_owned())).1,
			200.0 + SORT_INCREMENT
		);
		assert_eq!(shove(&items(&[]), None).1, SORT_INCREMENT);
	}

	#[test]
	fn a_tight_gap_shoves_neighbours_until_one_already_fits() {
		// Actual's loop stops at the first item whose order is already larger.
		let list = items(&[1.0, 2.0, 3.0, 100_000.0]);
		let (updates, order) = shove(&list, Some(&"c1".to_owned()));
		let shoved: Vec<_> = updates.iter().map(|(id, o)| ((*id).clone(), *o)).collect();
		assert_eq!(
			shoved,
			[("c1".to_owned(), 16386.0), ("c2".to_owned(), 32770.0)]
		);
		assert_eq!(order, 1.5);
	}
}
