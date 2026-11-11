//! Category kinds from docs/finance/classification.md CLS-3 and CLS-4: which
//! kind a category may be given, where a loan payment's principal or a
//! savings move should be filed, and which categories the one-time kind
//! wizard should ask about. The effective kind itself is
//! `BudgetData::category_kind`, shared with reports and forecast.

use std::collections::BTreeMap;

use veritas_domain::{BudgetData, CategoryId, CategoryKind, CategoryMeta, CoreError, Transaction};

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
	let is_goal = budget.goals.values().any(|goal| goal.category == *category);
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

/// CLS-4.2: expense categories with no stored kind that carry on↔off
/// transfers, which reports count as spending until the user says what they
/// are. Each comes with how many such transfers it has, for the review count.
pub fn categories_to_review(budget: &BudgetData) -> BTreeMap<CategoryId, usize> {
	let mut counts = BTreeMap::new();
	for transaction in budget.transactions.values() {
		let Some(category) = on_to_off_category(budget, transaction) else {
			continue;
		};
		let unset = budget
			.category_meta
			.get(category)
			.and_then(|m| m.kind)
			.is_none();
		let expense = budget
			.categories
			.get(category)
			.is_some_and(|c| !c.is_income);
		if unset && expense {
			*counts.entry(category.clone()).or_insert(0) += 1;
		}
	}
	counts
}

/// The category of an alive on-budget leg whose alive partner is off budget.
fn on_to_off_category<'a>(
	budget: &'a BudgetData,
	transaction: &'a Transaction,
) -> Option<&'a CategoryId> {
	if transaction.tombstone {
		return None;
	}
	let partner = budget.transactions.get(transaction.transfer.as_ref()?)?;
	if partner.tombstone {
		return None;
	}
	let off_budget = |t: &Transaction| budget.accounts.get(&t.account).map(|a| a.off_budget);
	if off_budget(transaction)? || !off_budget(partner)? {
		return None;
	}
	let category = budget.resolve_category(transaction.category.as_ref()?)?;
	budget
		.categories
		.get(category)
		.filter(|c| !c.tombstone)
		.map(|c| &c.id)
}
