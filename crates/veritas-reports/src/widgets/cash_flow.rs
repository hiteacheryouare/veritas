//! The cash flow card: `simpleCashFlow` (`spreadsheets/cash-flow-spreadsheet.tsx:18-72`),
//! money in and out of on-budget accounts, leaving out rows whose payee is a
//! transfer.

use chrono::{Datelike, Months, NaiveDate};
use veritas_domain::{BudgetData, CoreError, Money, Transaction};

use super::summary::latest_transaction;
use super::{TimeRange, calculate_time_range, filtered_rows_between, meta_filter};
use crate::json::Json;
use crate::report_query::parse_date;

#[derive(Clone, Debug, PartialEq)]
pub struct CashFlowCard {
	/// The card's months, as `calculateTimeRange` gives them.
	pub range: TimeRange,
	pub income: Money,
	/// Negative, as Actual sums it.
	pub expense: Money,
}

fn first_day(month: &str) -> Option<NaiveDate> {
	parse_date(month).and_then(|date| date.with_day(1))
}

fn last_day(month: &str) -> Option<NaiveDate> {
	first_day(month)
		.and_then(|first| first.checked_add_months(Months::new(1)))
		.and_then(|next| next.pred_opt())
}

/// `'account.offbudget': false` and `'payee.transfer_acct': null`, read
/// through the joins AQL makes: a deleted account or payee isn't joined.
fn counted(budget: &BudgetData, row: &Transaction) -> bool {
	let on_budget = budget
		.accounts
		.get(&row.account)
		.filter(|account| !account.tombstone)
		.is_some_and(|account| !account.off_budget);
	let transfer = row
		.payee
		.as_ref()
		.and_then(|id| budget.resolve_payee(id))
		.and_then(|id| budget.payees.get(id))
		.filter(|payee| !payee.tombstone)
		.is_some_and(|payee| payee.transfer_account.is_some());
	on_budget && !transfer
}

pub fn cash_flow_card(
	budget: &BudgetData,
	meta: Option<&Json>,
	today: NaiveDate,
) -> Result<CashFlowCard, CoreError> {
	let current = today.format("%Y-%m").to_string();
	let range = calculate_time_range(
		meta.and_then(|meta| meta.get("timeFrame")),
		Some((&current, &current, "sliding-window")),
		Some(latest_transaction(budget).unwrap_or(today)),
		today,
	);
	let invalid =
		|| CoreError::Invalid(format!("a cash flow range {}..{}", range.start, range.end));
	let start = first_day(&range.start).ok_or_else(invalid)?;
	let end = last_day(&range.end).ok_or_else(invalid)?.min(today);
	let filter = meta_filter(meta)?;
	let mut income = Money::ZERO;
	let mut expense = Money::ZERO;
	if start <= end {
		for row in filtered_rows_between(budget, &filter, start, end) {
			let row = row?;
			if !counted(budget, row) {
				continue;
			}
			if row.amount > Money::ZERO {
				income = income.checked_add(row.amount)?;
			} else {
				expense = expense.checked_add(row.amount)?;
			}
		}
	}
	Ok(CashFlowCard {
		range,
		income,
		expense,
	})
}
