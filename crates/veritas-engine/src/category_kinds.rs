//! Category kinds from docs/finance/classification.md CLS-3 and CLS-4.1:
//! which kind a category may be given, and where a loan payment's principal
//! or a savings move should be filed. The effective kind itself is
//! `BudgetData::category_kind`, shared with reports and forecast. The
//! CLS-4.2 review count is `Classification::categories_to_review` in
//! veritas-reports, so it counts exactly what the classifier decides.

use veritas_domain::{BudgetData, CategoryId, CategoryKind, CategoryMeta, CoreError, GoalStatus};

/// The `veritas_category_meta` row to store for a category.
#[derive(Clone, Debug, PartialEq)]
pub struct CategoryMetaWrite {
	pub category: CategoryId,
	pub meta: CategoryMeta,
}

/// Validates and builds a kind change. Actual's `is_income` flag can't be
/// overridden (CLS-3.1), and a goal stays an expense category (CLS-3.3), so
/// those changes are refused rather than stored as a conflict.
pub fn set_category_kind(
	budget: &BudgetData,
	category: &CategoryId,
	kind: CategoryKind,
) -> Result<CategoryMetaWrite, CoreError> {
	let found = budget
		.categories
		.get(category)
		.filter(|c| !c.tombstone)
		.ok_or_else(|| CoreError::NotFound {
			entity: "category",
			id: category.to_string(),
		})?;
	if found.is_income && kind != CategoryKind::Income {
		return Err(CoreError::Invalid(
			"This is an income category in Actual, so it stays income. Pick an expense category instead.".to_owned(),
		));
	}
	if !found.is_income && kind == CategoryKind::Income {
		return Err(CoreError::Invalid(
			"This is an expense category in Actual. Use a category from an income group for income.".to_owned(),
		));
	}
	let is_goal = budget
		.goals
		.values()
		.any(|goal| goal.category == *category && goal.status != GoalStatus::Closed);
	if is_goal && kind != CategoryKind::Expense {
		return Err(CoreError::Invalid(
			"A goal's envelope is always an expense category. Close the goal first to use it for something else.".to_owned(),
		));
	}
	let existing = budget.category_meta.get(category);
	Ok(CategoryMetaWrite {
		category: category.clone(),
		meta: CategoryMeta {
			kind: Some(kind),
			tax_bucket: existing.and_then(|m| m.tax_bucket.clone()),
			archived_on: existing.and_then(|m| m.archived_on),
		},
	})
}

/// Where CLS-4.1 files a debt payment's principal or a savings move.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KindTarget {
	/// No visible category has the kind; the caller offers to create one
	/// (in a "Debt" group for principal).
	None,
	One(CategoryId),
	/// More than one: the user picks.
	Several(Vec<CategoryId>),
}

/// The visible, live categories whose effective kind is `kind`, which must
/// be `DebtPrincipal` or `Savings`. Hidden categories keep their kind
/// (CLS-3.2) but aren't offered for new money.
pub fn kind_target(budget: &BudgetData, kind: CategoryKind) -> Result<KindTarget, CoreError> {
	if !matches!(kind, CategoryKind::DebtPrincipal | CategoryKind::Savings) {
		return Err(CoreError::Invalid(
			"Only debt principal and savings categories are looked up this way.".to_owned(),
		));
	}
	let mut matching: Vec<CategoryId> = budget
		.categories
		.values()
		.filter(|c| !c.tombstone && !c.hidden)
		.filter(|c| {
			budget
				.category_groups
				.get(&c.group)
				.is_some_and(|g| !g.tombstone && !g.hidden)
		})
		.filter(|c| budget.category_kind(&c.id) == kind)
		.map(|c| c.id.clone())
		.collect();
	Ok(match matching.len() {
		0 => KindTarget::None,
		1 => KindTarget::One(matching.remove(0)),
		_ => KindTarget::Several(matching),
	})
}
