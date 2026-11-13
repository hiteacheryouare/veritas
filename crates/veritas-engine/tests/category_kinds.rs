//! Category kinds, engine side (WS-ENGINE step 6): docs/finance/
//! classification.md CLS-3.1, CLS-3.2, CLS-3.3 and CLS-4.1. CLS-4.2's
//! review count lives in veritas-reports, beside the classifier it reads.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;
use veritas_domain::{CategoryKind, CategoryMeta, CoreError, Goal, GoalId, GoalStatus};
use veritas_engine::category_kinds::{KindTarget, kind_target, set_category_kind};

fn with_loan() -> Budget {
	let mut budget = Budget::actual_seed();
	budget.account("checking", false);
	budget.account("mortgage", true);
	budget.group("debt", false, 60_000.0);
	budget.category("principal", "debt", false);
	budget
}

fn kind(budget: &mut Budget, category: &str, kind: CategoryKind) {
	let write = set_category_kind(&budget.0, &cat(category), kind).unwrap();
	budget.0.category_meta.insert(write.category, write.meta);
}

#[test]
fn cls_3_1_a_kind_never_crosses_actuals_income_expense_flag() {
	let budget = with_loan();
	let refused = |category: &str, kind| {
		matches!(
			set_category_kind(&budget.0, &cat(category), kind),
			Err(CoreError::Invalid(_))
		)
	};
	assert!(refused("income", CategoryKind::Savings));
	assert!(refused("income", CategoryKind::Expense));
	assert!(refused("principal", CategoryKind::Income));
	let write =
		set_category_kind(&budget.0, &cat("principal"), CategoryKind::DebtPrincipal).unwrap();
	assert_eq!(write.meta.kind, Some(CategoryKind::DebtPrincipal));
	assert!(set_category_kind(&budget.0, &cat("income"), CategoryKind::Income).is_ok());
	assert!(matches!(
		set_category_kind(&budget.0, &cat("nowhere"), CategoryKind::Expense),
		Err(CoreError::NotFound { .. })
	));
}

#[test]
fn cls_3_3_a_goal_envelope_stays_an_expense_category() {
	let mut budget = with_loan();
	budget.category("laptop", "usual", false);
	budget.0.goals.insert(
		GoalId::new("g"),
		Goal {
			id: GoalId::new("g"),
			category: cat("laptop"),
			target: cents(150_000),
			target_date: None,
			icon: None,
			color: None,
			status: GoalStatus::Active,
			reached_month: None,
			closed_month: None,
			released_to: None,
			released: None,
		},
	);
	assert!(matches!(
		set_category_kind(&budget.0, &cat("laptop"), CategoryKind::Savings),
		Err(CoreError::Invalid(_))
	));
	assert!(set_category_kind(&budget.0, &cat("laptop"), CategoryKind::Expense).is_ok());

	budget.0.goals.get_mut(&GoalId::new("g")).unwrap().status = GoalStatus::Closed;
	assert!(
		set_category_kind(&budget.0, &cat("laptop"), CategoryKind::Savings).is_ok(),
		"closing the goal frees its envelope, as the refusal's message says"
	);
}

#[test]
fn a_kind_change_keeps_the_rest_of_the_metadata() {
	let mut budget = with_loan();
	let archived = day("2026-01-31");
	budget.0.category_meta.insert(
		cat("principal"),
		CategoryMeta {
			kind: None,
			tax_bucket: Some("mortgage-interest".to_owned()),
			archived_on: Some(archived),
		},
	);
	let write =
		set_category_kind(&budget.0, &cat("principal"), CategoryKind::DebtPrincipal).unwrap();
	assert_eq!(write.meta.tax_bucket.as_deref(), Some("mortgage-interest"));
	assert_eq!(write.meta.archived_on, Some(archived));
}

#[test]
fn cls_4_1_principal_goes_to_the_one_visible_debt_category_or_the_user_picks() {
	let mut budget = with_loan();
	assert_eq!(
		kind_target(&budget.0, CategoryKind::DebtPrincipal).unwrap(),
		KindTarget::None
	);
	kind(&mut budget, "principal", CategoryKind::DebtPrincipal);
	assert_eq!(
		kind_target(&budget.0, CategoryKind::DebtPrincipal).unwrap(),
		KindTarget::One(cat("principal"))
	);

	budget.category("car-loan", "debt", false);
	kind(&mut budget, "car-loan", CategoryKind::DebtPrincipal);
	assert_eq!(
		kind_target(&budget.0, CategoryKind::DebtPrincipal).unwrap(),
		KindTarget::Several(vec![cat("car-loan"), cat("principal")])
	);

	budget.category("car-loan", "debt", false).hidden = true;
	assert_eq!(
		kind_target(&budget.0, CategoryKind::DebtPrincipal).unwrap(),
		KindTarget::One(cat("principal")),
		"CLS-3.2: a hidden category keeps its kind but gets no new money"
	);
	assert_eq!(
		kind_target(&budget.0, CategoryKind::Savings).unwrap(),
		KindTarget::None
	);
	assert!(kind_target(&budget.0, CategoryKind::Expense).is_err());
}

#[test]
fn a_conflicting_stored_kind_doesnt_count_as_that_kind() {
	let mut budget = with_loan();
	budget.0.category_meta.insert(
		cat("starting-balances"),
		CategoryMeta {
			kind: Some(CategoryKind::Savings),
			tax_bucket: None,
			archived_on: None,
		},
	);
	assert_eq!(
		kind_target(&budget.0, CategoryKind::Savings).unwrap(),
		KindTarget::None
	);
}
