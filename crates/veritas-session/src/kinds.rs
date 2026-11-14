//! Category kinds as the app edits them (docs/finance/classification.md
//! CLS-3.3, CLS-4.1). The rules on which kind a category may take are
//! `veritas_engine::category_kinds`.

use std::collections::HashMap;

use veritas_domain::{
	BudgetData, CategoryGroupId, CategoryId, CategoryKind, CategoryMetaRow, CoreError, Row,
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

/// CLS-4.1's group for a new principal or savings category: a visible
/// "Savings and Debt" group from the starter set, else "Debt" or "Savings",
/// reused case-insensitively or created.
fn home_group(data: &BudgetData, kind: CategoryKind) -> Option<CategoryGroupId> {
	let named = |name: &str| {
		data.category_groups
			.values()
			.filter(|g| !g.tombstone && !g.hidden && !g.is_income)
			.find(|g| g.name.trim().eq_ignore_ascii_case(name))
			.map(|g| g.id.clone())
	};
	named("Savings and Debt").or_else(|| named(group_name(kind)))
}

fn group_name(kind: CategoryKind) -> &'static str {
	match kind {
		CategoryKind::Savings => "Savings",
		_ => "Debt",
	}
}

/// CLS-4.1's "created if none exists": a new visible category of `kind`
/// (debt principal or savings) in its home group, which is created when
/// missing. Refused while a visible category of that kind exists.
pub fn create_kind_category(
	data: &BudgetData,
	kind: CategoryKind,
	name: &str,
) -> Result<(Vec<Row>, CategoryId), CoreError> {
	if kind_target(data, kind)? != KindTarget::None {
		return Err(CoreError::Invalid(
			"You already have a category for this. Pick it instead.".to_owned(),
		));
	}
	let mut rows = Vec::new();
	let group = match home_group(data, kind) {
		Some(group) => group,
		None => {
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
