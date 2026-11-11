//! Category kinds, engine side (WS-ENGINE step 6): docs/finance/
//! classification.md CLS-3.1, CLS-3.2, CLS-3.3, CLS-4.1 and CLS-4.2.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;
use veritas_domain::{
	CategoryKind, CategoryMeta, CoreError, Goal, GoalId, GoalStatus, TransactionId,
};
use veritas_engine::category_kinds::{
	KindTarget, categories_to_review, kind_target, set_category_kind,
};

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

/// Two legs of an on→off transfer, the on-budget one categorized.
fn transfer(budget: &mut Budget, id: &str, category: Option<&str>) {
	let (on, off) = (format!("{id}-on"), format!("{id}-off"));
	budget
		.txn(&on, "checking", "2026-11-02", -50_000, category)
		.transfer = Some(TransactionId::new(off.clone()));
	budget
		.txn(&off, "mortgage", "2026-11-02", 50_000, None)
		.transfer = Some(TransactionId::new(on));
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

#[test]
fn cls_4_2_on_to_off_transfers_without_a_kind_are_counted_for_review() {
	let mut budget = with_loan();
	transfer(&mut budget, "march", Some("principal"));
	transfer(&mut budget, "april", Some("principal"));
	transfer(&mut budget, "car", Some("general"));
	transfer(&mut budget, "unfiled", None);
	budget.txn("groceries", "checking", "2026-11-03", -4_000, Some("food"));
	let review = categories_to_review(&budget.0);
	assert_eq!(review.get(&cat("principal")), Some(&2));
	assert_eq!(review.get(&cat("general")), Some(&1));
	assert_eq!(
		review.len(),
		2,
		"uncategorized transfers and ordinary spending aren't kind questions"
	);

	kind(&mut budget, "principal", CategoryKind::DebtPrincipal);
	budget
		.0
		.transactions
		.get_mut(&TransactionId::new("car-off"))
		.unwrap()
		.tombstone = true;
	assert!(
		categories_to_review(&budget.0).is_empty(),
		"answered once per category, and dead pairs drop out"
	);
}
