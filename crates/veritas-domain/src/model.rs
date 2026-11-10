//! The loaded budget as plain data (PLAN §6.5). veritas-store fills it from
//! Actual's tables; every crate above the store reads only these types.
//! Tombstoned rows are kept, because history and mapping resolution need them.

use std::collections::BTreeMap;

use crate::{
	AccountGroupId, AccountId, AccountKind, BudgetType, CategoryGroupId, CategoryId, CategoryKind,
	CivilDate, GoalId, GoalStatus, Money, PayeeId, RuleId, ScheduleId, TagId, TransactionId,
	YearMonth,
};

#[derive(Clone, Debug, PartialEq)]
pub struct Account {
	pub id: AccountId,
	pub name: String,
	pub off_budget: bool,
	pub closed: bool,
	pub group: Option<AccountGroupId>,
	pub sort_order: f64,
	pub tombstone: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AccountGroup {
	pub id: AccountGroupId,
	pub name: String,
	pub sort_order: f64,
	pub tombstone: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CategoryGroup {
	pub id: CategoryGroupId,
	pub name: String,
	pub is_income: bool,
	pub hidden: bool,
	pub sort_order: f64,
	pub tombstone: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Category {
	pub id: CategoryId,
	pub name: String,
	pub group: CategoryGroupId,
	pub is_income: bool,
	pub hidden: bool,
	pub sort_order: f64,
	pub tombstone: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Payee {
	pub id: PayeeId,
	pub name: String,
	/// Set on the one payee Actual keeps per account for transfers to it.
	pub transfer_account: Option<AccountId>,
	pub favorite: bool,
	pub learn_categories: bool,
	pub tombstone: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SplitRole {
	None,
	Parent,
	Child { parent: TransactionId },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Transaction {
	pub id: TransactionId,
	pub account: AccountId,
	pub date: CivilDate,
	pub amount: Money,
	pub payee: Option<PayeeId>,
	pub category: Option<CategoryId>,
	pub notes: Option<String>,
	pub cleared: bool,
	pub reconciled: bool,
	pub split: SplitRole,
	/// The other leg of a transfer pair.
	pub transfer: Option<TransactionId>,
	pub schedule: Option<ScheduleId>,
	pub starting_balance: bool,
	pub sort_order: f64,
	pub tombstone: bool,
}

/// One envelope in one month (Actual's `zero_budgets` row).
#[derive(Clone, Debug, PartialEq)]
pub struct BudgetCell {
	pub month: YearMonth,
	pub category: CategoryId,
	pub budgeted: Money,
	pub carryover: bool,
	pub goal: Option<Money>,
	pub long_goal: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleStage {
	Pre,
	Default,
	Post,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConditionsOp {
	And,
	Or,
}

/// Conditions and actions stay as Actual's JSON text, so rules Veritas can't
/// evaluate pass through byte for byte (R-RULE-3). veritas-rules parses them.
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
	pub id: RuleId,
	pub stage: RuleStage,
	pub conditions_op: ConditionsOp,
	pub conditions_json: String,
	pub actions_json: String,
	pub tombstone: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Schedule {
	pub id: ScheduleId,
	pub name: Option<String>,
	pub rule: RuleId,
	pub posts_transaction: bool,
	pub completed: bool,
	pub next_date: Option<CivilDate>,
	pub tombstone: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tag {
	pub id: TagId,
	pub name: String,
	pub color: Option<String>,
	pub description: Option<String>,
	pub hidden: bool,
	pub tombstone: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CategoryMeta {
	pub kind: Option<CategoryKind>,
	pub tax_bucket: Option<String>,
	pub archived_on: Option<CivilDate>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct AccountMeta {
	pub kind: Option<AccountKind>,
	pub loan_rate_bps: Option<i64>,
	pub loan_payment: Option<Money>,
	pub loan_payment_day: Option<u8>,
	pub lender: Option<String>,
}

/// docs/finance/classification.md CLS-5: a reconciliation adjustment Veritas made.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct TransactionMeta {
	pub adjustment: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Goal {
	pub id: GoalId,
	pub category: CategoryId,
	pub target: Money,
	pub target_date: Option<CivilDate>,
	pub icon: Option<String>,
	pub color: Option<String>,
	pub status: GoalStatus,
	pub reached_month: Option<YearMonth>,
	pub closed_month: Option<YearMonth>,
	pub released_to: Option<CategoryId>,
	pub released: Option<Money>,
}

/// Everything Veritas models about one budget. Collections are keyed by id;
/// `BTreeMap` keeps iteration deterministic for tests and dumps.
#[derive(Clone, Debug, PartialEq)]
pub struct BudgetData {
	pub budget_type: BudgetType,
	pub accounts: BTreeMap<AccountId, Account>,
	pub account_groups: BTreeMap<AccountGroupId, AccountGroup>,
	pub category_groups: BTreeMap<CategoryGroupId, CategoryGroup>,
	pub categories: BTreeMap<CategoryId, Category>,
	pub payees: BTreeMap<PayeeId, Payee>,
	pub transactions: BTreeMap<TransactionId, Transaction>,
	pub budget_cells: BTreeMap<(YearMonth, CategoryId), BudgetCell>,
	/// Actual's manual hold for next month (`zero_budget_months.buffered`).
	pub holds: BTreeMap<YearMonth, Money>,
	pub rules: BTreeMap<RuleId, Rule>,
	pub schedules: BTreeMap<ScheduleId, Schedule>,
	pub tags: BTreeMap<TagId, Tag>,
	/// Actual's `notes` table: category, account and budget-month notes by id.
	pub notes: BTreeMap<String, String>,
	/// Where a deleted or merged category or payee now points.
	pub category_mapping: BTreeMap<CategoryId, CategoryId>,
	pub payee_mapping: BTreeMap<PayeeId, PayeeId>,
	pub category_meta: BTreeMap<CategoryId, CategoryMeta>,
	pub account_meta: BTreeMap<AccountId, AccountMeta>,
	pub transaction_meta: BTreeMap<TransactionId, TransactionMeta>,
	pub goals: BTreeMap<GoalId, Goal>,
}

impl BudgetData {
	pub fn empty(budget_type: BudgetType) -> Self {
		BudgetData {
			budget_type,
			accounts: BTreeMap::new(),
			account_groups: BTreeMap::new(),
			category_groups: BTreeMap::new(),
			categories: BTreeMap::new(),
			payees: BTreeMap::new(),
			transactions: BTreeMap::new(),
			budget_cells: BTreeMap::new(),
			holds: BTreeMap::new(),
			rules: BTreeMap::new(),
			schedules: BTreeMap::new(),
			tags: BTreeMap::new(),
			notes: BTreeMap::new(),
			category_mapping: BTreeMap::new(),
			payee_mapping: BTreeMap::new(),
			category_meta: BTreeMap::new(),
			account_meta: BTreeMap::new(),
			transaction_meta: BTreeMap::new(),
			goals: BTreeMap::new(),
		}
	}

	/// Follows `category_mapping` the way Actual's reads do. Chains from repeated
	/// deletes are followed, and a cycle (corrupt input, flagged by the audit)
	/// stops at the first repeat instead of looping.
	pub fn resolve_category(&self, id: &CategoryId) -> CategoryId {
		let mut current = id.clone();
		for _ in 0..=self.category_mapping.len() {
			match self.category_mapping.get(&current) {
				Some(next) if next != &current => current = next.clone(),
				_ => break,
			}
		}
		current
	}

	pub fn resolve_payee(&self, id: &PayeeId) -> PayeeId {
		let mut current = id.clone();
		for _ in 0..=self.payee_mapping.len() {
			match self.payee_mapping.get(&current) {
				Some(next) if next != &current => current = next.clone(),
				_ => break,
			}
		}
		current
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn deleted_categories_resolve_through_chains_of_mappings() {
		let mut budget = BudgetData::empty(BudgetType::Envelope);
		let id = CategoryId::new;
		budget.category_mapping.insert(id("dining"), id("food"));
		budget.category_mapping.insert(id("food"), id("everyday"));
		budget
			.category_mapping
			.insert(id("everyday"), id("everyday"));
		assert_eq!(budget.resolve_category(&id("dining")), id("everyday"));
		assert_eq!(budget.resolve_category(&id("rent")), id("rent"));
	}

	#[test]
	fn a_corrupt_mapping_cycle_terminates() {
		let mut budget = BudgetData::empty(BudgetType::Envelope);
		let id = PayeeId::new;
		budget.payee_mapping.insert(id("a"), id("b"));
		budget.payee_mapping.insert(id("b"), id("a"));
		let resolved = budget.resolve_payee(&id("a"));
		assert!(resolved == id("a") || resolved == id("b"));
	}
}
