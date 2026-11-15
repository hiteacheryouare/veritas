//! Category kinds as the app edits them (docs/finance/classification.md
//! CLS-3.3, CLS-4.1). The rules on which kind a category may take are
//! `veritas_engine::category_kinds`.

use std::collections::HashMap;

use veritas_domain::{
	BudgetData, CategoryGroup, CategoryId, CategoryKind, CategoryMetaRow, CoreError, Row,
	TransactionId,
};
use veritas_engine::category_kinds::{self, KindTarget};
use veritas_reports::{Leg, classify};

use crate::categories;
use crate::writes::applied;

pub fn set_kind(
	data: &BudgetData,
	category: &CategoryId,
	kind: CategoryKind,
) -> Result<Vec<Row>, CoreError> {
	let write = category_kinds::set_category_kind(data, category, kind)?;
	Ok(vec![Row::CategoryMeta(CategoryMetaRow {
		category: write.category,
		meta: write.meta,
	})])
}

/// CLS-3.3's confirm step: how many primary-leg rows change kind or flags
/// if the category takes `kind`. A kind change reclassifies history, so the
/// shell shows this before saving.
pub fn kind_change_preview(
	data: &BudgetData,
	category: &CategoryId,
	kind: CategoryKind,
) -> Result<usize, CoreError> {
	let after = applied(data, &set_kind(data, category, kind)?);
	let before = classify(data);
	let seen: HashMap<&TransactionId, _> = before
		.rows()
		.iter()
		.filter(|row| row.leg == Leg::Primary)
		.map(|row| (&row.id, (row.decision.kind(), row.flags)))
		.collect();
	let changed = classify(&after)
		.rows()
		.iter()
		.filter(|row| row.leg == Leg::Primary)
		.filter(|row| seen.get(&row.id) != Some(&(row.decision.kind(), row.flags)))
		.count();
	Ok(changed)
}

pub fn kind_target(data: &BudgetData, kind: CategoryKind) -> Result<KindTarget, CoreError> {
	category_kinds::kind_target(data, kind)
}

/// Where `create_kind_category` would put a new category: the group's name
/// as the user sees it, and whether that group exists but is hidden, in
/// which case the shell confirms unhiding it first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KindCategoryPlan {
	pub group_name: String,
	pub group_is_hidden: bool,
}

enum Home<'a> {
	Existing(&'a CategoryGroup),
	New,
}

/// CLS-4.1's group for a new principal or savings category: a visible
/// "Savings and Debt" group from the starter set, else "Debt" or "Savings",
/// reused even when hidden, or created. Names match ignoring case, the same
/// way Actual refuses a duplicate group name, so a group that would block
/// creating one is always found here instead.
fn home_group(data: &BudgetData, kind: CategoryKind) -> Result<Home<'_>, CoreError> {
	let named = |name: &str| {
		data.category_groups
			.values()
			.filter(|g| !g.tombstone && categories::same_group_name(g.name.trim(), name))
			.collect::<Vec<_>>()
	};
	if let Some(starter) = named("Savings and Debt")
		.into_iter()
		.find(|g| !g.hidden && !g.is_income)
	{
		return Ok(Home::Existing(starter));
	}
	let matches = named(group_name(kind));
	let usable = matches
		.iter()
		.filter(|g| !g.is_income)
		.min_by_key(|g| g.hidden);
	match (usable, matches.first()) {
		(Some(group), _) => Ok(Home::Existing(group)),
		(None, Some(income)) => Err(CoreError::Invalid(format!(
			"\"{}\" is an income group, so it can't hold this category. Rename that group or pick another category.",
			income.name
		))),
		(None, None) => Ok(Home::New),
	}
}

fn group_name(kind: CategoryKind) -> &'static str {
	match kind {
		CategoryKind::Savings => "Savings",
		_ => "Debt",
	}
}

/// CLS-4.1's confirm step before `create_kind_category`.
pub fn kind_category_plan(
	data: &BudgetData,
	kind: CategoryKind,
) -> Result<KindCategoryPlan, CoreError> {
	kind_target(data, kind)?;
	Ok(match home_group(data, kind)? {
		Home::Existing(group) => KindCategoryPlan {
			group_name: group.name.clone(),
			group_is_hidden: group.hidden,
		},
		Home::New => KindCategoryPlan {
			group_name: group_name(kind).to_owned(),
			group_is_hidden: false,
		},
	})
}

/// CLS-4.1's "created if none exists": a new visible category of `kind`
/// (debt principal or savings) in its home group, which is created when
/// missing. A hidden home group is unhidden only when `unhide_group` says
/// the user confirmed it; its other categories keep their own hidden flags.
/// Refused while a visible category of that kind exists.
pub fn create_kind_category(
	data: &BudgetData,
	kind: CategoryKind,
	name: &str,
	unhide_group: bool,
) -> Result<(Vec<Row>, CategoryId), CoreError> {
	if kind_target(data, kind)? != KindTarget::None {
		return Err(CoreError::Invalid(
			"You already have a category for this. Pick it instead.".to_owned(),
		));
	}
	let mut rows = Vec::new();
	let group = match home_group(data, kind)? {
		Home::Existing(group) if group.hidden && !unhide_group => {
			return Err(CoreError::Invalid(format!(
				"Your group \"{}\" is hidden. Confirm showing it again first.",
				group.name
			)));
		}
		Home::Existing(group) => {
			if group.hidden {
				rows.extend(categories::set_group_hidden(data, &group.id, false)?);
			}
			group.id.clone()
		}
		Home::New => {
			let (group_rows, group) = categories::create_group(data, group_name(kind))?;
			rows.extend(group_rows);
			group
		}
	};
	let with_group = applied(data, &rows);
	let (category_rows, category) = categories::create_category(&with_group, &group, name)?;
	rows.extend(category_rows);
	let with_category = applied(data, &rows);
	rows.extend(set_kind(&with_category, &category, kind)?);
	Ok((rows, category))
}
