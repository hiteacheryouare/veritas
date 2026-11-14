//! Read-only report queries: Net Profit, the standard reports, the Sankey,
//! drilldowns, statements and the tax summary (R-REP-1 to R-REP-8). Each one
//! reads the session's cached classification, so none re-derives a row's kind.

use std::sync::Arc;

use veritas_domain::{
	AccountGroupId, BudgetData, CategoryId, CivilDate, CoreError, Money, TransactionId, YearMonth,
};
use veritas_reports::{self as reports, CategoryLabels, Classification, NodeKind, Period};
use veritas_session::Backend;

use crate::blocking::{CancelToken as CancelHandle, run};
use crate::budget::{civil_date, year_month};
use crate::session::{Core, Session};
use crate::{FfiError, Month};

/// How a category is labeled in a historical report (R-REP-3). Labels never
/// filter anything out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct ReportLabels {
	pub hidden: bool,
	pub archived: bool,
	pub deleted: bool,
	pub closed_goal: bool,
	/// The uncategorized line, whose rows wait for a category.
	pub needs_review: bool,
}

/// A number and how many rows are behind it, for "Show N transactions".
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct ReportLine {
	pub total: i64,
	pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ReportCategory {
	/// `None` is the uncategorized line.
	pub category_id: Option<String>,
	/// Empty for the uncategorized line.
	pub name: String,
	pub group_id: Option<String>,
	pub group_name: Option<String>,
	pub labels: ReportLabels,
	pub total: i64,
	pub count: u64,
}

/// One Net Profit line and its categories in budget order, which sum to
/// `total`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ReportSection {
	pub total: i64,
	pub count: u64,
	pub categories: Vec<ReportCategory>,
}

/// Net Profit for one period (ST-1). Every amount is a positive-means-normal
/// magnitude; `unreviewed` is positive when money left the budget and is
/// never part of `net_cash_flow`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct NetProfitColumn {
	/// `YYYY-MM-DD`, inclusive.
	pub first: String,
	pub last: String,
	/// Set when the column is one calendar month.
	pub month: Option<Month>,
	pub income: ReportSection,
	pub expenses: ReportSection,
	pub net_profit: ReportLine,
	pub debt_paydown: ReportSection,
	pub savings: ReportSection,
	pub net_cash_flow: ReportLine,
	pub unreviewed: ReportLine,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct NetProfitReport {
	pub months: Vec<NetProfitColumn>,
	/// The whole range as one period, categories netted across it.
	pub total: NetProfitColumn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct NetWorthMonth {
	pub month: Month,
	pub assets: i64,
	/// What is owed, as a positive amount.
	pub liabilities: i64,
	pub net_worth: i64,
}

/// The Cash Flow Statement for one period over the on-budget accounts
/// (ST-6). Debt paydown and savings are negative when money left the budget.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CashFlowColumn {
	pub first: String,
	pub last: String,
	pub month: Option<Month>,
	pub opening: i64,
	pub net_profit: ReportLine,
	pub debt_paydown: ReportLine,
	pub savings: ReportLine,
	pub net_cash_flow: ReportLine,
	pub unreviewed_transfers: ReportLine,
	pub change_from_operations: i64,
	pub in_transit: ReportLine,
	pub starting_balances: ReportLine,
	pub reconciliation_adjustments: ReportLine,
	pub closing: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum SankeyNodeKind {
	IncomeCategory,
	RefundsAndWithdrawals,
	UnreviewedTransfersIn,
	Shortfall,
	Income,
	CategoryGroup,
	Category,
	Savings,
	DebtPaydown,
	IncomeReversals,
	UnreviewedTransfers,
	Unallocated,
}

/// Which node a drilldown opens: its kind, plus the category or group id
/// for those kinds (`None` there is uncategorized spending).
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct SankeyNodeKey {
	pub kind: SankeyNodeKind,
	pub id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SankeyNode {
	pub key: SankeyNodeKey,
	/// The category's or group's name; empty for every other kind.
	pub name: String,
	pub value: i64,
	pub labels: ReportLabels,
	/// Rows behind the node. Shortfall has none; Unallocated counts the
	/// income rows it was computed from.
	pub count: u64,
	/// 0 sources, 1 Income, 2 groups and the other outflows, 3 categories.
	pub column: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct SankeyFlow {
	/// Indexes into `nodes`.
	pub from: u32,
	pub to: u32,
	pub value: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SankeyReport {
	pub first: String,
	pub last: String,
	pub nodes: Vec<SankeyNode>,
	pub flows: Vec<SankeyFlow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum NetProfitLine {
	Income,
	Expenses,
	NetProfit,
	DebtPaydown,
	Savings,
	NetCashFlow,
	Unreviewed,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum DrillCategory {
	Category { id: String },
	Uncategorized,
}

/// The rows behind one number on a report.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ReportDrill {
	/// A Net Profit cell: the line over `first` to `last`, or one category
	/// of it.
	NetProfit {
		first: String,
		last: String,
		line: NetProfitLine,
		category: Option<DrillCategory>,
	},
	SankeyNode {
		first: String,
		last: String,
		node: SankeyNodeKey,
	},
}

/// One row behind a report number, by date then id. `amount` keeps Actual's
/// sign: negative is money out of the account.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ReportTransaction {
	pub id: String,
	pub parent_id: Option<String>,
	pub date: String,
	pub account_id: String,
	pub account_name: String,
	/// The payee's name, or the other account's for a transfer.
	pub payee: Option<String>,
	pub category: Option<String>,
	pub notes: Option<String>,
	pub amount: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SheetAccount {
	pub account_id: String,
	pub name: String,
	/// An asset's balance, or what a liability owes.
	pub amount: i64,
	pub closed: bool,
	pub deleted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SheetSection {
	/// `None` holds the accounts outside any group.
	pub group_id: Option<String>,
	pub group_name: Option<String>,
	pub accounts: Vec<SheetAccount>,
	pub total: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BalanceSheet {
	pub as_of: String,
	pub assets: Vec<SheetSection>,
	pub total_assets: i64,
	pub liabilities: Vec<SheetSection>,
	pub total_liabilities: i64,
	pub net_worth: i64,
}

/// The Statement of Owner's Equity (ST-4). `unexplained` is zero unless a
/// row is misclassified.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct OwnersEquity {
	pub first: String,
	pub last: String,
	pub opening: i64,
	pub net_income: ReportLine,
	pub capitalized_transfers: ReportLine,
	pub starting_balances: ReportLine,
	pub reconciliation_adjustments: ReportLine,
	pub revaluations: ReportLine,
	pub unpaired_transfers: ReportLine,
	pub in_transit: ReportLine,
	pub closing: i64,
	pub unexplained: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TaxBucket {
	W2Income,
	InterestIncome,
	BusinessIncome,
	BusinessExpense,
	Charitable,
	Medical,
	Education,
	Hsa,
	RetirementContribution,
	TaxesWithheld,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct TaxBucketTotal {
	pub bucket: TaxBucket,
	pub inflow: bool,
	/// Negative when refunds exceed the year's amounts. Never clamped.
	pub total: i64,
	pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TaxDetailRow {
	pub transaction_id: String,
	pub parent_id: Option<String>,
	pub date: String,
	/// `None` is a conflict: the row's tags name two or more buckets.
	pub bucket: Option<TaxBucket>,
	pub from_tag: bool,
	pub amount: i64,
	pub counted: Option<i64>,
	pub category: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TaxSummary {
	pub year: i32,
	pub buckets: Vec<TaxBucketTotal>,
	pub detail: Vec<TaxDetailRow>,
	pub conflicts: u64,
	/// The year-end CSVs (TAX-5), 2 places, never rounded.
	pub summary_csv: String,
	pub detail_csv: String,
}

impl From<CategoryLabels> for ReportLabels {
	fn from(labels: CategoryLabels) -> Self {
		ReportLabels {
			hidden: labels.hidden,
			archived: labels.archived,
			deleted: labels.deleted,
			closed_goal: labels.closed_goal,
			needs_review: labels.needs_review,
		}
	}
}

fn line(total: Money, rows: &[TransactionId]) -> ReportLine {
	ReportLine {
		total: total.minor(),
		count: rows.len() as u64,
	}
}

fn report_line(from: &reports::Line) -> ReportLine {
	line(from.total, &from.rows)
}

pub(crate) fn period(first: &str, last: &str) -> Result<Period, CoreError> {
	Period::new(civil_date(first)?, civil_date(last)?)
}

fn months_period(first: YearMonth, last: YearMonth) -> Result<Period, CoreError> {
	Period::new(first.first_day(), Period::month(last).last())
}

fn category_name(budget: &BudgetData, id: &CategoryId) -> String {
	budget
		.categories
		.get(id)
		.map(|category| category.name.clone())
		.unwrap_or_default()
}

fn section(budget: &BudgetData, from: &reports::Section) -> ReportSection {
	ReportSection {
		total: from.total.minor(),
		count: from.rows.len() as u64,
		categories: from
			.categories
			.iter()
			.map(|line| {
				let group = line
					.category
					.as_ref()
					.and_then(|id| budget.categories.get(id))
					.and_then(|category| budget.category_groups.get(&category.group));
				ReportCategory {
					category_id: line.category.as_ref().map(ToString::to_string),
					name: line
						.category
						.as_ref()
						.map(|id| category_name(budget, id))
						.unwrap_or_default(),
					group_id: group.map(|group| group.id.to_string()),
					group_name: group.map(|group| group.name.clone()),
					labels: line.labels.into(),
					total: line.total.minor(),
					count: line.rows.len() as u64,
				}
			})
			.collect(),
	}
}

fn single_month(period: Period) -> Option<Month> {
	let month = period.first().year_month();
	(Period::month(month) == period).then(|| month.into())
}

fn net_profit_column(budget: &BudgetData, report: &reports::NetProfit) -> NetProfitColumn {
	NetProfitColumn {
		first: report.period.first().to_string(),
		last: report.period.last().to_string(),
		month: single_month(report.period),
		income: section(budget, &report.income),
		expenses: section(budget, &report.expenses),
		net_profit: report_line(&report.net_profit),
		debt_paydown: section(budget, &report.debt_paydown),
		savings: section(budget, &report.savings),
		net_cash_flow: report_line(&report.net_cash_flow),
		unreviewed: report_line(&report.unreviewed),
	}
}

pub(crate) fn net_profit_report(
	budget: &BudgetData,
	classification: &Classification,
	first: YearMonth,
	last: YearMonth,
) -> Result<NetProfitReport, CoreError> {
	let months = reports::net_profit_by_month(budget, classification, first, last)?;
	let total = reports::net_profit(budget, classification, months_period(first, last)?)?;
	Ok(NetProfitReport {
		months: months
			.iter()
			.map(|month| net_profit_column(budget, month))
			.collect(),
		total: net_profit_column(budget, &total),
	})
}

pub(crate) fn net_worth_months(
	budget: &BudgetData,
	classification: &Classification,
	first: YearMonth,
	last: YearMonth,
) -> Result<Vec<NetWorthMonth>, CoreError> {
	Ok(
		reports::net_worth_by_month(budget, classification, first, last)?
			.into_iter()
			.map(|point| NetWorthMonth {
				month: point.month.into(),
				assets: point.assets.minor(),
				liabilities: point.liabilities.minor(),
				net_worth: point.net_worth.minor(),
			})
			.collect(),
	)
}

fn cash_flow_column(statement: &reports::CashFlow) -> CashFlowColumn {
	CashFlowColumn {
		first: statement.period.first().to_string(),
		last: statement.period.last().to_string(),
		month: single_month(statement.period),
		opening: statement.opening.minor(),
		net_profit: report_line(&statement.net_profit),
		debt_paydown: report_line(&statement.debt_paydown),
		savings: report_line(&statement.savings),
		net_cash_flow: report_line(&statement.net_cash_flow),
		unreviewed_transfers: report_line(&statement.unreviewed_transfers),
		change_from_operations: statement.change_from_operations.minor(),
		in_transit: report_line(&statement.in_transit),
		starting_balances: report_line(&statement.starting_balances),
		reconciliation_adjustments: report_line(&statement.reconciliation_adjustments),
		closing: statement.closing.minor(),
	}
}

fn node_key(kind: &NodeKind) -> SankeyNodeKey {
	let (kind, id) = match kind {
		NodeKind::IncomeCategory(id) => (SankeyNodeKind::IncomeCategory, Some(id.to_string())),
		NodeKind::RefundsAndWithdrawals => (SankeyNodeKind::RefundsAndWithdrawals, None),
		NodeKind::UnreviewedTransfersIn => (SankeyNodeKind::UnreviewedTransfersIn, None),
		NodeKind::Shortfall => (SankeyNodeKind::Shortfall, None),
		NodeKind::Income => (SankeyNodeKind::Income, None),
		NodeKind::CategoryGroup(id) => (
			SankeyNodeKind::CategoryGroup,
			id.as_ref().map(ToString::to_string),
		),
		NodeKind::Category(id) => (
			SankeyNodeKind::Category,
			id.as_ref().map(ToString::to_string),
		),
		NodeKind::Savings => (SankeyNodeKind::Savings, None),
		NodeKind::DebtPaydown => (SankeyNodeKind::DebtPaydown, None),
		NodeKind::IncomeReversals => (SankeyNodeKind::IncomeReversals, None),
		NodeKind::UnreviewedTransfers => (SankeyNodeKind::UnreviewedTransfers, None),
		NodeKind::Unallocated => (SankeyNodeKind::Unallocated, None),
	};
	SankeyNodeKey { kind, id }
}

fn node_name(budget: &BudgetData, kind: &NodeKind) -> String {
	match kind {
		NodeKind::IncomeCategory(id) | NodeKind::Category(Some(id)) => category_name(budget, id),
		NodeKind::CategoryGroup(Some(id)) => budget
			.category_groups
			.get(id)
			.map(|group| group.name.clone())
			.unwrap_or_default(),
		_ => String::new(),
	}
}

fn node_column(kind: &NodeKind) -> u8 {
	match kind {
		NodeKind::IncomeCategory(_)
		| NodeKind::RefundsAndWithdrawals
		| NodeKind::UnreviewedTransfersIn
		| NodeKind::Shortfall => 0,
		NodeKind::Income => 1,
		NodeKind::Category(_) => 3,
		NodeKind::CategoryGroup(_)
		| NodeKind::Savings
		| NodeKind::DebtPaydown
		| NodeKind::IncomeReversals
		| NodeKind::UnreviewedTransfers
		| NodeKind::Unallocated => 2,
	}
}

pub(crate) fn sankey_report(budget: &BudgetData, sankey: &reports::Sankey) -> SankeyReport {
	SankeyReport {
		first: sankey.period.first().to_string(),
		last: sankey.period.last().to_string(),
		nodes: sankey
			.nodes
			.iter()
			.map(|node| SankeyNode {
				key: node_key(&node.kind),
				name: node_name(budget, &node.kind),
				value: node.value.minor(),
				labels: node.labels.into(),
				count: node.rows.len() as u64,
				column: node_column(&node.kind),
			})
			.collect(),
		flows: sankey
			.flows
			.iter()
			.map(|flow| SankeyFlow {
				from: flow.from as u32,
				to: flow.to as u32,
				value: flow.value.minor(),
			})
			.collect(),
	}
}

fn drill_rows(
	budget: &BudgetData,
	classification: &Classification,
	drill: &ReportDrill,
) -> Result<Vec<TransactionId>, CoreError> {
	match drill {
		ReportDrill::NetProfit {
			first,
			last,
			line,
			category,
		} => {
			let report = reports::net_profit(budget, classification, period(first, last)?)?;
			let section = match line {
				NetProfitLine::Income => &report.income,
				NetProfitLine::Expenses => &report.expenses,
				NetProfitLine::DebtPaydown => &report.debt_paydown,
				NetProfitLine::Savings => &report.savings,
				NetProfitLine::NetProfit => return Ok(report.net_profit.rows),
				NetProfitLine::NetCashFlow => return Ok(report.net_cash_flow.rows),
				NetProfitLine::Unreviewed => return Ok(report.unreviewed.rows),
			};
			let Some(category) = category else {
				return Ok(section.rows.clone());
			};
			let wanted = match category {
				DrillCategory::Category { id } => Some(CategoryId::new(id.as_str())),
				DrillCategory::Uncategorized => None,
			};
			Ok(section
				.categories
				.iter()
				.find(|line| line.category == wanted)
				.map(|line| line.rows.clone())
				.unwrap_or_default())
		}
		ReportDrill::SankeyNode { first, last, node } => {
			let sankey = reports::sankey(budget, classification, period(first, last)?)?;
			Ok(sankey
				.nodes
				.into_iter()
				.find(|candidate| &node_key(&candidate.kind) == node)
				.map(|found| found.rows)
				.unwrap_or_default())
		}
	}
}

fn payee_name(budget: &BudgetData, transaction: &veritas_domain::Transaction) -> Option<String> {
	let parent = match &transaction.split {
		veritas_domain::SplitRole::Child { parent } => budget.transactions.get(parent),
		_ => None,
	};
	let id = transaction
		.payee
		.as_ref()
		.or_else(|| parent.and_then(|parent| parent.payee.as_ref()))?;
	let id = budget.resolve_payee(id).unwrap_or(id);
	let payee = budget.payees.get(id)?;
	match &payee.transfer_account {
		Some(account) => budget
			.accounts
			.get(account)
			.map(|account| account.name.clone()),
		None => Some(payee.name.clone()),
	}
}

pub(crate) fn report_transactions(
	budget: &BudgetData,
	classification: &Classification,
	rows: &[TransactionId],
) -> Vec<ReportTransaction> {
	rows.iter()
		.filter_map(|id| {
			let row = classification.get(id)?;
			let transaction = budget.transactions.get(id);
			Some(ReportTransaction {
				id: id.to_string(),
				parent_id: row.parent.as_ref().map(ToString::to_string),
				date: row.date.to_string(),
				account_id: row.account.to_string(),
				account_name: budget
					.accounts
					.get(&row.account)
					.map(|account| account.name.clone())
					.unwrap_or_default(),
				payee: transaction.and_then(|transaction| payee_name(budget, transaction)),
				category: row.category.as_ref().map(|id| category_name(budget, id)),
				notes: transaction.and_then(|transaction| transaction.notes.clone()),
				amount: row.amount.minor(),
			})
		})
		.collect()
}

fn sheet_sections(
	budget: &BudgetData,
	sections: &[reports::AccountGroupSection],
) -> Vec<SheetSection> {
	let group_name = |id: &AccountGroupId| {
		budget
			.account_groups
			.get(id)
			.map(|group| group.name.clone())
	};
	sections
		.iter()
		.map(|section| SheetSection {
			group_id: section.group.as_ref().map(ToString::to_string),
			group_name: section.group.as_ref().and_then(group_name),
			accounts: section
				.accounts
				.iter()
				.map(|line| SheetAccount {
					account_id: line.account.to_string(),
					name: budget
						.accounts
						.get(&line.account)
						.map(|account| account.name.clone())
						.unwrap_or_default(),
					amount: line.amount.minor(),
					closed: line.closed,
					deleted: line.deleted,
				})
				.collect(),
			total: section.total.minor(),
		})
		.collect()
}

fn balance_sheet(
	budget: &BudgetData,
	classification: &Classification,
	as_of: CivilDate,
) -> Result<BalanceSheet, CoreError> {
	let sheet = reports::balance_sheet(budget, classification, as_of)?;
	Ok(BalanceSheet {
		as_of: sheet.as_of.to_string(),
		assets: sheet_sections(budget, &sheet.assets),
		total_assets: sheet.total_assets.minor(),
		liabilities: sheet_sections(budget, &sheet.liabilities),
		total_liabilities: sheet.total_liabilities.minor(),
		net_worth: sheet.net_worth.minor(),
	})
}

fn owners_equity(
	budget: &BudgetData,
	classification: &Classification,
	period: Period,
) -> Result<OwnersEquity, CoreError> {
	let equity = reports::owners_equity(budget, classification, period)?;
	Ok(OwnersEquity {
		first: period.first().to_string(),
		last: period.last().to_string(),
		opening: equity.opening.minor(),
		net_income: report_line(&equity.net_income),
		capitalized_transfers: report_line(&equity.capitalized_transfers),
		starting_balances: report_line(&equity.starting_balances),
		reconciliation_adjustments: report_line(&equity.reconciliation_adjustments),
		revaluations: report_line(&equity.revaluations),
		unpaired_transfers: report_line(&equity.unpaired_transfers),
		in_transit: report_line(&equity.in_transit),
		closing: equity.closing.minor(),
		unexplained: equity.unexplained()?.minor(),
	})
}

fn tax_bucket(bucket: reports::Bucket) -> TaxBucket {
	match bucket {
		reports::Bucket::W2Income => TaxBucket::W2Income,
		reports::Bucket::InterestIncome => TaxBucket::InterestIncome,
		reports::Bucket::BusinessIncome => TaxBucket::BusinessIncome,
		reports::Bucket::BusinessExpense => TaxBucket::BusinessExpense,
		reports::Bucket::Charitable => TaxBucket::Charitable,
		reports::Bucket::Medical => TaxBucket::Medical,
		reports::Bucket::Education => TaxBucket::Education,
		reports::Bucket::Hsa => TaxBucket::Hsa,
		reports::Bucket::RetirementContribution => TaxBucket::RetirementContribution,
		reports::Bucket::TaxesWithheld => TaxBucket::TaxesWithheld,
	}
}

fn tax_summary(
	budget: &BudgetData,
	classification: &Classification,
	year: i32,
) -> Result<TaxSummary, CoreError> {
	let summary = reports::tax_summary(budget, classification, year)?;
	Ok(TaxSummary {
		year,
		buckets: summary
			.buckets
			.iter()
			.map(|total| TaxBucketTotal {
				bucket: tax_bucket(total.bucket),
				inflow: total.bucket.direction() == reports::Direction::Inflow,
				total: total.total.minor(),
				count: total.rows.len() as u64,
			})
			.collect(),
		detail: summary
			.detail
			.iter()
			.map(|row| TaxDetailRow {
				transaction_id: row.row.to_string(),
				parent_id: row.parent.as_ref().map(ToString::to_string),
				date: row.date.to_string(),
				bucket: row.bucket.map(tax_bucket),
				from_tag: row.source == reports::Source::Tag,
				amount: row.amount.minor(),
				counted: row.counted.map(Money::minor),
				category: row.category.as_ref().map(|id| category_name(budget, id)),
			})
			.collect(),
		conflicts: summary.conflicts().count() as u64,
		summary_csv: reports::summary_csv(&summary),
		detail_csv: reports::detail_csv(budget, &summary),
	})
}

impl<B: Backend> Core<B> {
	/// Runs a report against the cached classification. A cancel that lands
	/// before the read starts stops it; a report has no step to undo after.
	pub(crate) fn report<T>(
		&self,
		cancel: &veritas_session::CancelToken,
		read: impl FnOnce(&BudgetData, &Classification) -> Result<T, CoreError>,
	) -> Result<T, FfiError> {
		if cancel.is_cancelled() {
			return Err(FfiError::Cancelled);
		}
		self.call(|session| session.with_classification(read))
	}
}

#[uniffi::export]
impl Session {
	/// Net Profit for each month from `first` through `last`, and for the
	/// whole range (R-REP-2, ST-1). Income vs expense and Spending by
	/// category read the same columns (R-REP-5).
	pub async fn net_profit(
		&self,
		first: Month,
		last: Month,
		cancel: Arc<CancelHandle>,
	) -> Result<NetProfitReport, FfiError> {
		let core = self.core.clone();
		run(&cancel, move |cancel| {
			core.report(cancel, |budget, classification| {
				net_profit_report(
					budget,
					classification,
					year_month(first)?,
					year_month(last)?,
				)
			})
		})
		.await
	}

	/// Assets, liabilities and net worth at each month's end (R-REP-5).
	pub async fn net_worth(
		&self,
		first: Month,
		last: Month,
		cancel: Arc<CancelHandle>,
	) -> Result<Vec<NetWorthMonth>, FfiError> {
		let core = self.core.clone();
		run(&cancel, move |cancel| {
			core.report(cancel, |budget, classification| {
				net_worth_months(
					budget,
					classification,
					year_month(first)?,
					year_month(last)?,
				)
			})
		})
		.await
	}

	/// One Cash Flow Statement per month, each opening where the last closed
	/// (R-REP-5, ST-6).
	pub async fn cash_flow(
		&self,
		first: Month,
		last: Month,
		cancel: Arc<CancelHandle>,
	) -> Result<Vec<CashFlowColumn>, FfiError> {
		let core = self.core.clone();
		run(&cancel, move |cancel| {
			core.report(cancel, |budget, classification| {
				Ok(reports::cash_flow_by_month(
					budget,
					classification,
					year_month(first)?,
					year_month(last)?,
				)?
				.iter()
				.map(cash_flow_column)
				.collect())
			})
		})
		.await
	}

	/// The Cash Flow Statement for any inclusive date range (R-REP-7).
	pub async fn cash_flow_statement(
		&self,
		first: String,
		last: String,
		cancel: Arc<CancelHandle>,
	) -> Result<CashFlowColumn, FfiError> {
		let core = self.core.clone();
		run(&cancel, move |cancel| {
			core.report(cancel, |budget, classification| {
				reports::cash_flow(budget, classification, period(&first, &last)?)
					.map(|statement| cash_flow_column(&statement))
			})
		})
		.await
	}

	/// Income sources to Income to groups, categories, savings, debt paydown
	/// and Unallocated over any inclusive date range (R-REP-6, ST-8).
	pub async fn sankey(
		&self,
		first: String,
		last: String,
		cancel: Arc<CancelHandle>,
	) -> Result<SankeyReport, FfiError> {
		let core = self.core.clone();
		run(&cancel, move |cancel| {
			core.report(cancel, |budget, classification| {
				reports::sankey(budget, classification, period(&first, &last)?)
					.map(|sankey| sankey_report(budget, &sankey))
			})
		})
		.await
	}

	/// The primary-leg rows behind one report number, by date then id
	/// (ST-1.6, ST-8 rule 6).
	pub async fn report_transactions(
		&self,
		drill: ReportDrill,
		cancel: Arc<CancelHandle>,
	) -> Result<Vec<ReportTransaction>, FfiError> {
		let core = self.core.clone();
		run(&cancel, move |cancel| {
			core.report(cancel, |budget, classification| {
				let rows = drill_rows(budget, classification, &drill)?;
				Ok(report_transactions(budget, classification, &rows))
			})
		})
		.await
	}

	/// Assets and liabilities by account group at the end of `as_of`.
	pub async fn balance_sheet(
		&self,
		as_of: String,
		cancel: Arc<CancelHandle>,
	) -> Result<BalanceSheet, FfiError> {
		let core = self.core.clone();
		run(&cancel, move |cancel| {
			core.report(cancel, |budget, classification| {
				balance_sheet(budget, classification, civil_date(&as_of)?)
			})
		})
		.await
	}

	pub async fn owners_equity(
		&self,
		first: String,
		last: String,
		cancel: Arc<CancelHandle>,
	) -> Result<OwnersEquity, FfiError> {
		let core = self.core.clone();
		run(&cancel, move |cancel| {
			core.report(cancel, |budget, classification| {
				owners_equity(budget, classification, period(&first, &last)?)
			})
		})
		.await
	}

	/// The year's tax buckets, detail and both CSVs (R-REP-8).
	pub async fn tax_summary(
		&self,
		year: i32,
		cancel: Arc<CancelHandle>,
	) -> Result<TaxSummary, FfiError> {
		let core = self.core.clone();
		run(&cancel, move |cancel| {
			core.report(cancel, |budget, classification| {
				tax_summary(budget, classification, year)
			})
		})
		.await
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::test_support::imported;

	fn month(year: i32, month: u32) -> YearMonth {
		YearMonth::new(year, month).unwrap()
	}

	fn read<T>(
		core: &Core<veritas_store::Store>,
		read: impl FnOnce(&BudgetData, &Classification) -> Result<T, CoreError>,
	) -> T {
		core.report(&veritas_session::CancelToken::default(), read)
			.unwrap()
	}

	fn drilled(core: &Core<veritas_store::Store>, drill: ReportDrill) -> Vec<ReportTransaction> {
		read(core, |budget, classification| {
			let rows = drill_rows(budget, classification, &drill)?;
			Ok(report_transactions(budget, classification, &rows))
		})
	}

	#[test]
	fn net_profit_columns_add_up_and_every_number_drills_to_its_rows() {
		let (_scratch, core) = imported("all-features");
		let report = read(&core, |budget, classification| {
			net_profit_report(budget, classification, month(2026, 6), month(2026, 11))
		});
		assert_eq!(report.months.len(), 6);
		assert_eq!(report.months[0].month, Some(month(2026, 6).into()));
		assert_eq!(report.total.month, None);
		assert_eq!(
			(report.total.first.as_str(), report.total.last.as_str()),
			("2026-06-01", "2026-11-30")
		);
		let sum = |pick: fn(&NetProfitColumn) -> i64| report.months.iter().map(pick).sum::<i64>();
		assert_eq!(sum(|column| column.income.total), report.total.income.total);
		assert_eq!(
			sum(|column| column.expenses.total),
			report.total.expenses.total
		);
		assert_ne!(report.total.expenses.total, 0);
		for column in report.months.iter().chain([&report.total]) {
			assert_eq!(
				column.net_profit.total,
				column.income.total - column.expenses.total
			);
			assert_eq!(
				column.net_cash_flow.total,
				column.net_profit.total - column.debt_paydown.total - column.savings.total
			);
			let by_category: i64 = column
				.expenses
				.categories
				.iter()
				.map(|line| line.total)
				.sum();
			assert_eq!(by_category, column.expenses.total);
		}

		let expenses = drilled(
			&core,
			ReportDrill::NetProfit {
				first: report.total.first.clone(),
				last: report.total.last.clone(),
				line: NetProfitLine::Expenses,
				category: None,
			},
		);
		assert_eq!(expenses.len() as u64, report.total.expenses.count);
		assert_eq!(
			-expenses.iter().map(|row| row.amount).sum::<i64>(),
			report.total.expenses.total
		);
		assert!(expenses.windows(2).all(|pair| pair[0].date <= pair[1].date));
		assert!(expenses.iter().all(|row| !row.account_name.is_empty()));

		let line = &report.total.expenses.categories[0];
		let one = drilled(
			&core,
			ReportDrill::NetProfit {
				first: report.total.first.clone(),
				last: report.total.last.clone(),
				line: NetProfitLine::Expenses,
				category: Some(match &line.category_id {
					Some(id) => DrillCategory::Category { id: id.clone() },
					None => DrillCategory::Uncategorized,
				}),
			},
		);
		assert_eq!(one.len() as u64, line.count);
		assert!(line.category_id.is_none() || !line.name.is_empty());
		assert!(line.category_id.is_none() || line.group_name.is_some());
	}

	#[test]
	fn the_sankey_balances_at_every_column_and_its_nodes_drill_down() {
		let (_scratch, core) = imported("all-features");
		let sankey = read(&core, |budget, classification| {
			reports::sankey(budget, classification, Period::month(month(2026, 10)))
				.map(|sankey| sankey_report(budget, &sankey))
		});
		let column = |index: u8| -> i64 {
			sankey
				.nodes
				.iter()
				.filter(|node| node.column == index)
				.map(|node| node.value)
				.sum()
		};
		let income = column(1);
		assert!(income > 0);
		assert_eq!(column(0), income);
		assert_eq!(column(2), income);
		for flow in &sankey.flows {
			assert!(flow.value > 0);
			assert!((flow.from as usize) < sankey.nodes.len());
			assert!((flow.to as usize) < sankey.nodes.len());
		}
		let group = sankey
			.nodes
			.iter()
			.find(|node| node.key.kind == SankeyNodeKind::CategoryGroup)
			.unwrap();
		let children: i64 = sankey
			.flows
			.iter()
			.filter(|flow| sankey.nodes[flow.from as usize].key == group.key)
			.map(|flow| flow.value)
			.sum();
		assert_eq!(children, group.value);
		let rows = drilled(
			&core,
			ReportDrill::SankeyNode {
				first: sankey.first.clone(),
				last: sankey.last.clone(),
				node: group.key.clone(),
			},
		);
		assert_eq!(rows.len() as u64, group.count);
		assert!(!rows.is_empty());
	}

	#[test]
	fn net_worth_cash_flow_and_statements_tie_out() {
		let (_scratch, core) = imported("all-features");
		let (worth, flows, sheet, equity) = read(&core, |budget, classification| {
			let worth = net_worth_months(budget, classification, month(2025, 11), month(2026, 11))?;
			let flows = reports::cash_flow_by_month(
				budget,
				classification,
				month(2026, 1),
				month(2026, 3),
			)?
			.iter()
			.map(cash_flow_column)
			.collect::<Vec<_>>();
			let sheet = balance_sheet(
				budget,
				classification,
				Period::month(month(2026, 11)).last(),
			)?;
			let equity = owners_equity(budget, classification, Period::month(month(2026, 10)))?;
			Ok((worth, flows, sheet, equity))
		});
		assert_eq!(worth.len(), 13);
		let last = worth.last().unwrap();
		assert_eq!(last.net_worth, last.assets - last.liabilities);
		assert_eq!(sheet.net_worth, last.net_worth);
		assert_eq!(
			sheet.total_assets,
			sheet
				.assets
				.iter()
				.map(|section| section.total)
				.sum::<i64>()
		);
		assert_eq!(flows.len(), 3);
		assert_eq!(flows[0].closing, flows[1].opening);
		assert_eq!(flows[0].month, Some(month(2026, 1).into()));
		assert_eq!(equity.unexplained, 0);
	}

	#[test]
	fn the_tax_summary_comes_with_both_csvs() {
		let (_scratch, core) = imported("all-features");
		let summary = read(&core, |budget, classification| {
			tax_summary(budget, classification, 2026)
		});
		assert_eq!(summary.year, 2026);
		assert!(summary.summary_csv.starts_with("tax_year,bucket"));
		assert!(summary.detail_csv.starts_with("tax_year,bucket"));
		for bucket in &summary.buckets {
			let rows = summary
				.detail
				.iter()
				.filter(|row| row.bucket == Some(bucket.bucket))
				.count();
			assert_eq!(rows as u64, bucket.count);
		}
	}
}
