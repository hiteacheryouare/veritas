//! The loaded budget as plain data (PLAN §6.5). veritas-store fills it from
//! Actual's tables; every crate above the store reads only these types.
//! Tombstoned rows are kept, because history and mapping resolution need them.

use std::collections::BTreeMap;

use crate::{
	AccountGroupId, AccountId, AccountKind, BudgetType, CategoryGroupId, CategoryId, CategoryKind,
	CivilDate, CustomReportId, DashboardPageId, GoalId, GoalStatus, Money, PayeeId, RuleId,
	Scenario, ScenarioId, ScheduleId, TagId, TransactionId, WidgetId, YearMonth,
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
	/// When a reconcile last finished, as Actual stores it: `String(Date.now())`,
	/// epoch milliseconds as text (docs/actual-format.md §4.7).
	pub last_reconciled: Option<String>,
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
	/// The payee text from a bank import (`imported_description`), which rules can match.
	pub imported_payee: Option<String>,
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
	/// A split parent's stored `error` JSON (`SplitTransactionError`), kept
	/// as Actual wrote it.
	pub split_error: Option<String>,
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

/// A `schedules_json_paths` row: JSON paths such as `$[2]` into the rule's
/// conditions, which `v_schedules` reads the payee, account, amount and date through.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ScheduleJsonPaths {
	pub payee: Option<String>,
	pub account: Option<String>,
	pub amount: Option<String>,
	pub date: Option<String>,
}

/// Pause and End are both Actual's Complete; this only says which one the
/// bill screen shows. It never changes what posts.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ScheduleMeta {
	pub paused: bool,
}

/// docs/finance/classification.md CLS-5: a reconciliation adjustment Veritas made.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TransactionMeta {
	pub adjustment: bool,
	/// Why the row may need a look in Actual, such as a rule action Veritas
	/// can't run. Written by veritas-rules.
	pub needs_actual: Option<String>,
}

/// Actual's `custom_reports` row as stored. Its text columns stay as Actual
/// wrote them, so a report Veritas never edits is written back unchanged;
/// veritas-reports reads the settings out of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomReportRow {
	pub id: CustomReportId,
	pub name: Option<String>,
	pub start_date: Option<String>,
	pub end_date: Option<String>,
	pub date_static: Option<i64>,
	pub date_range: Option<String>,
	pub mode: Option<String>,
	pub group_by: Option<String>,
	pub balance_type: Option<String>,
	pub show_empty: Option<i64>,
	pub show_offbudget: Option<i64>,
	pub show_hidden: Option<i64>,
	pub show_uncategorized: Option<i64>,
	pub selected_categories: Option<String>,
	pub graph_type: Option<String>,
	pub conditions: Option<String>,
	pub conditions_op: Option<String>,
	pub metadata: Option<String>,
	pub interval: Option<String>,
	pub color_scheme: Option<String>,
	pub tombstone: bool,
	pub include_current: Option<i64>,
	pub sort_by: Option<String>,
	pub trim_intervals: Option<i64>,
	pub show_trend_lines: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DashboardPage {
	pub id: DashboardPageId,
	pub name: Option<String>,
	pub tombstone: bool,
}

/// One `dashboard` row. `kind` is Actual's widget `type`, kept as text so
/// widget types Veritas doesn't know survive untouched, and `meta` is the
/// stored JSON text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Widget {
	pub id: WidgetId,
	pub kind: Option<String>,
	pub width: Option<i64>,
	pub height: Option<i64>,
	pub x: Option<i64>,
	pub y: Option<i64>,
	pub meta: Option<String>,
	pub page: Option<DashboardPageId>,
	pub tombstone: bool,
}

/// Actual's synced number-format preferences, as stored: `numberFormat`
/// (`comma-dot`, `dot-comma`, `space-comma`, `apostrophe-dot` or another
/// value a newer Actual writes), `hideFraction` and `defaultCurrencyCode`.
/// `None` means the preference isn't set, which Actual reads as its default.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FormatPrefs {
	pub number_format: Option<String>,
	pub hide_fraction: bool,
	pub currency_code: Option<String>,
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
	/// GOAL-6's dismiss (D72): activity through this month, totalling
	/// `activity_seen_total`, was seen, and the close month's ending available
	/// was `close_month_dismissed_available`.
	pub activity_seen_through: Option<YearMonth>,
	pub activity_seen_total: Option<Money>,
	pub close_month_dismissed_available: Option<Money>,
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
	/// Actual's manual hold for next month (`zero_budget_months.buffered`). A row
	/// whose value is NULL is `None`, which Actual reads as "ignore the auto hold".
	pub holds: BTreeMap<YearMonth, Option<Money>>,
	pub rules: BTreeMap<RuleId, Rule>,
	pub schedules: BTreeMap<ScheduleId, Schedule>,
	pub tags: BTreeMap<TagId, Tag>,
	/// Actual's `notes` table: category, account and budget-month notes by id.
	/// `None` is a row whose note is NULL, which is how Actual clears one.
	pub notes: BTreeMap<String, Option<String>>,
	/// Where each category or payee id reads as, including self-mappings. A
	/// deleted or merged id points at its replacement.
	pub category_mapping: BTreeMap<CategoryId, CategoryId>,
	pub payee_mapping: BTreeMap<PayeeId, PayeeId>,
	pub category_meta: BTreeMap<CategoryId, CategoryMeta>,
	pub account_meta: BTreeMap<AccountId, AccountMeta>,
	pub transaction_meta: BTreeMap<TransactionId, TransactionMeta>,
	/// `veritas_tax_tag`: Actual tag name (without `#`) to TAX-2 bucket id.
	pub tax_tags: BTreeMap<String, String>,
	/// `schedules_json_paths`: where each schedule condition sits in its rule.
	pub schedule_json_paths: BTreeMap<ScheduleId, ScheduleJsonPaths>,
	/// `veritas_schedule_meta`: how Veritas labels a completed bill.
	pub schedule_meta: BTreeMap<ScheduleId, ScheduleMeta>,
	pub format: FormatPrefs,
	pub goals: BTreeMap<GoalId, Goal>,
	pub custom_reports: BTreeMap<CustomReportId, CustomReportRow>,
	pub dashboard_pages: BTreeMap<DashboardPageId, DashboardPage>,
	pub widgets: BTreeMap<WidgetId, Widget>,
	pub scenarios: BTreeMap<ScenarioId, Scenario>,
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
			tax_tags: BTreeMap::new(),
			schedule_json_paths: BTreeMap::new(),
			schedule_meta: BTreeMap::new(),
			format: FormatPrefs::default(),
			goals: BTreeMap::new(),
			custom_reports: BTreeMap::new(),
			dashboard_pages: BTreeMap::new(),
			widgets: BTreeMap::new(),
			scenarios: BTreeMap::new(),
		}
	}

	/// One lookup, exactly as Actual's views join `category_mapping`: Actual keeps
	/// the table flattened when it deletes, and a category with no mapping row
	/// resolves to nothing, so its transactions count nowhere.
	pub fn resolve_category(&self, id: &CategoryId) -> Option<&CategoryId> {
		self.category_mapping.get(id)
	}

	/// The kind a category acts as (CLS-3.1), from its `is_income` flag and
	/// its `veritas_category_meta` kind. A category the budget doesn't have
	/// counts as not income.
	pub fn category_kind(&self, id: &CategoryId) -> CategoryKind {
		self.effective_category_kind(id).0
	}

	/// [`BudgetData::category_kind`] plus CLS-3.1's `kind_conflict`: true when
	/// the stored kind disagrees with Actual's income/expense flag.
	pub fn effective_category_kind(&self, id: &CategoryId) -> (CategoryKind, bool) {
		let is_income = self.categories.get(id).is_some_and(|c| c.is_income);
		let stored = self.category_meta.get(id).and_then(|meta| meta.kind);
		CategoryKind::effective(is_income, stored)
	}

	pub fn resolve_payee(&self, id: &PayeeId) -> Option<&PayeeId> {
		self.payee_mapping.get(id)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// CLS-3.1 through the budget: the category's flag plus its stored kind.
	#[test]
	fn cls_3_1_category_kinds_come_from_the_flag_and_the_metadata() {
		let mut budget = BudgetData::empty(BudgetType::Envelope);
		let id = CategoryId::new;
		for (name, is_income) in [
			("salary", true),
			("loan", false),
			("odd", false),
			("rent", false),
		] {
			budget.categories.insert(
				id(name),
				Category {
					id: id(name),
					name: name.to_owned(),
					group: CategoryGroupId::new("g"),
					is_income,
					hidden: false,
					sort_order: 0.0,
					tombstone: false,
				},
			);
		}
		let meta = |kind| CategoryMeta {
			kind: Some(kind),
			tax_bucket: None,
			archived_on: None,
		};
		budget
			.category_meta
			.insert(id("loan"), meta(CategoryKind::DebtPrincipal));
		budget
			.category_meta
			.insert(id("odd"), meta(CategoryKind::Income));
		assert_eq!(
			budget.effective_category_kind(&id("salary")),
			(CategoryKind::Income, false)
		);
		assert_eq!(
			budget.category_kind(&id("loan")),
			CategoryKind::DebtPrincipal
		);
		assert_eq!(
			budget.effective_category_kind(&id("odd")),
			(CategoryKind::Expense, true)
		);
		assert_eq!(budget.category_kind(&id("rent")), CategoryKind::Expense);
		assert_eq!(budget.category_kind(&id("missing")), CategoryKind::Expense);
	}

	#[test]
	fn categories_resolve_with_one_lookup_like_actuals_views() {
		let mut budget = BudgetData::empty(BudgetType::Envelope);
		let id = CategoryId::new;
		budget.category_mapping.insert(id("food"), id("food"));
		budget.category_mapping.insert(id("dining"), id("food"));
		assert_eq!(budget.resolve_category(&id("dining")), Some(&id("food")));
		assert_eq!(budget.resolve_category(&id("food")), Some(&id("food")));
		assert_eq!(budget.resolve_category(&id("unmapped")), None);
	}
}
