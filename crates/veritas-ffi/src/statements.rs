//! The income statement and the statements' header name (R-REP-7, ST-3).
//! The balance sheet, owner's equity and cash flow statement are in
//! `reports`.

use std::sync::Arc;

use veritas_domain::{BudgetData, CategoryGroupId, CoreError};
use veritas_reports::{self as reports, Classification, Period};
use veritas_session::Backend;

use crate::FfiError;
use crate::blocking::{CancelToken as CancelHandle, run};
use crate::reports::{ReportCategory, ReportLine, period, report_category, report_line};
use crate::session::{Core, Session};

/// One category group on the income statement, its categories in budget
/// order. `group_id` `nil` is uncategorized spending, which comes last and
/// reads "Uncategorized (needs review)" (ST-10.3).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct StatementGroup {
	pub group_id: Option<String>,
	pub group_name: Option<String>,
	pub categories: Vec<ReportCategory>,
	pub total: i64,
}

/// Net Profit in accountant format (ST-3). `net_income` is always Net
/// Profit's for the same range; expenses go negative when refunds win.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct IncomeStatement {
	pub first: String,
	pub last: String,
	pub revenue: Vec<StatementGroup>,
	pub total_revenue: ReportLine,
	pub expenses: Vec<StatementGroup>,
	pub total_expenses: ReportLine,
	pub net_income: ReportLine,
}

/// The name on every statement's header and on the tax summary (ST-3).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct StatementEntity {
	/// What the header shows: the name the user set, else the budget's.
	pub name: String,
	/// What the user set, for the editor; `nil` shows the budget's name.
	pub custom_name: Option<String>,
}

fn statement_groups(
	budget: &BudgetData,
	groups: &[reports::StatementGroup],
) -> Vec<StatementGroup> {
	let group_name = |id: &CategoryGroupId| {
		budget
			.category_groups
			.get(id)
			.map(|group| group.name.clone())
	};
	groups
		.iter()
		.map(|group| StatementGroup {
			group_id: group.group.as_ref().map(ToString::to_string),
			group_name: group.group.as_ref().and_then(group_name),
			categories: group
				.categories
				.iter()
				.map(|line| report_category(budget, line))
				.collect(),
			total: group.total.minor(),
		})
		.collect()
}

pub(crate) fn income_statement(
	budget: &BudgetData,
	classification: &Classification,
	period: Period,
) -> Result<IncomeStatement, CoreError> {
	let statement = reports::income_statement(budget, classification, period)?;
	Ok(IncomeStatement {
		first: period.first().to_string(),
		last: period.last().to_string(),
		revenue: statement_groups(budget, &statement.revenue),
		total_revenue: report_line(&statement.total_revenue),
		expenses: statement_groups(budget, &statement.expenses),
		total_expenses: report_line(&statement.total_expenses),
		net_income: report_line(&statement.net_income),
	})
}

impl<B: Backend> Core<B> {
	pub(crate) fn statement_entity(&self) -> Result<StatementEntity, FfiError> {
		self.call(|session| {
			let budget_name = session.budget_name()?;
			let name =
				veritas_session::settings::entity_name(session.data()?, budget_name.as_deref());
			Ok(StatementEntity {
				name: name.shown,
				custom_name: name.custom,
			})
		})
	}
}

#[uniffi::export]
impl Session {
	/// Revenue by income category and expenses by group then category, with
	/// net income, over any inclusive date range (R-REP-7, ST-3).
	pub async fn income_statement(
		&self,
		first: String,
		last: String,
		cancel: Arc<CancelHandle>,
	) -> Result<IncomeStatement, FfiError> {
		let core = self.core.clone();
		run(&cancel, move |cancel| {
			core.report(cancel, |budget, classification| {
				income_statement(budget, classification, period(&first, &last)?)
			})
		})
		.await
	}

	/// The name statements and the tax summary print in their header.
	pub fn statement_entity(&self) -> Result<StatementEntity, FfiError> {
		self.core.statement_entity()
	}
}
