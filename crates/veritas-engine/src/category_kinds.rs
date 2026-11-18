//! Category kinds from docs/finance/classification.md CLS-3 and CLS-4.1:
//! which kind a category may be given, and where a loan payment's principal
//! or a savings move should be filed. The effective kind itself is
//! `BudgetData::category_kind`, shared with reports and forecast. The
//! CLS-4.2 review count is `Classification::categories_to_review` in
//! veritas-reports, so it counts exactly what the classifier decides.

use veritas_domain::{
	BudgetData, Category, CategoryGroupId, CategoryId, CategoryKind, CategoryMeta, CoreError,
	GoalStatus,
};

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
/// Visible matches always win; the hidden and archived ones count only
/// when there is none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KindTarget {
	/// No category has the kind at all; the caller offers to create one
	/// (in a "Debt" group for principal).
	None,
	One(CategoryId),
	/// More than one visible: the user picks.
	Several(Vec<CategoryId>),
	/// No visible match and one hidden one: 'Show "{category}" again and
	/// use it?'
	OneHidden(HiddenMatch),
	/// No visible match and one archived one: 'Restore "{category}" and use
	/// it?', or create a new category under a free name.
	OneArchived(HiddenMatch),
	/// No visible match and several hidden or archived ones, in budget
	/// order: the user picks one or creates a new category.
	SeveralHidden(Vec<HiddenMatch>),
}

/// A category of the kind that isn't visible: hidden itself, in a hidden
/// group, or archived. Reviving it shows both again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HiddenMatch {
	pub category: CategoryId,
	pub name: String,
	pub group: CategoryGroupId,
	pub group_name: String,
	pub group_hidden: bool,
	pub archived: bool,
}

/// Live, not hidden, not archived and not in a hidden group (CLS-4.1).
fn visible(budget: &BudgetData, category: &Category) -> bool {
	!category.hidden
		&& budget
			.category_meta
			.get(&category.id)
			.is_none_or(|meta| meta.archived_on.is_none())
		&& budget
			.category_groups
			.get(&category.group)
			.is_some_and(|g| !g.hidden)
}

/// CLS-4.1's lookup for `kind`, which must be `DebtPrincipal` or `Savings`.
/// Hidden and archived categories keep their kind (CLS-3.2) and are offered
/// back only when no visible category has it.
pub fn kind_target(budget: &BudgetData, kind: CategoryKind) -> Result<KindTarget, CoreError> {
	if !matches!(kind, CategoryKind::DebtPrincipal | CategoryKind::Savings) {
		return Err(CoreError::Invalid(
			"Only debt principal and savings categories are looked up this way.".to_owned(),
		));
	}
	let mut matching: Vec<&Category> = budget
		.categories
		.values()
		.filter(|c| !c.tombstone)
		.filter(|c| {
			budget
				.category_groups
				.get(&c.group)
				.is_some_and(|g| !g.tombstone)
		})
		.filter(|c| budget.category_kind(&c.id) == kind)
		.collect();
	let mut shown: Vec<CategoryId> = matching
		.iter()
		.filter(|c| visible(budget, c))
		.map(|c| c.id.clone())
		.collect();
	if !shown.is_empty() {
		return Ok(match shown.len() {
			1 => KindTarget::One(shown.remove(0)),
			_ => KindTarget::Several(shown),
		});
	}
	let group_order = |c: &Category| {
		budget
			.category_groups
			.get(&c.group)
			.map_or(0.0, |g| g.sort_order)
	};
	matching.sort_by(|a, b| {
		group_order(a)
			.total_cmp(&group_order(b))
			.then(a.sort_order.total_cmp(&b.sort_order))
			.then(a.id.cmp(&b.id))
	});
	let mut hidden: Vec<HiddenMatch> = matching
		.into_iter()
		.filter_map(|c| {
			let group = budget.category_groups.get(&c.group)?;
			Some(HiddenMatch {
				category: c.id.clone(),
				name: c.name.clone(),
				group: group.id.clone(),
				group_name: group.name.clone(),
				group_hidden: group.hidden,
				archived: budget
					.category_meta
					.get(&c.id)
					.is_some_and(|meta| meta.archived_on.is_some()),
			})
		})
		.collect();
	Ok(match hidden.len() {
		0 => KindTarget::None,
		1 => {
			let only = hidden.remove(0);
			if only.archived {
				KindTarget::OneArchived(only)
			} else {
				KindTarget::OneHidden(only)
			}
		}
		_ => KindTarget::SeveralHidden(hidden),
	})
}

/// Whether a visible category already has `kind`, so nothing hidden is
/// offered and nothing new is created (CLS-4.1's visible-wins rule).
pub fn has_visible(budget: &BudgetData, kind: CategoryKind) -> Result<bool, CoreError> {
	Ok(matches!(
		kind_target(budget, kind)?,
		KindTarget::One(_) | KindTarget::Several(_)
	))
}
