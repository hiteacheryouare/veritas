//! The numbers behind Actual's dashboard cards, computed the way each card
//! and its spreadsheet compute them (`desktop-client/src/components/reports`).
//! Every function reads the widget's `meta` as Actual stores it.

mod age_of_money;
mod calendar;
mod summary;
mod time_frame;

pub use age_of_money::{AgeOfMoney, AgePoint, Granularity, Trend, age_of_money};
pub use calendar::{CalendarDay, CalendarMonth, calendar};
pub use summary::{SummaryContent, SummaryData, summary};
pub use time_frame::{TimeRange, calculate_time_range};

use veritas_domain::{BudgetData, CoreError, Transaction};

use crate::filter::TransactionFilter;
use crate::json::Json;
use crate::report_query::is_row;

/// The widget's `conditions` and `conditionsOp`, as every filtered card reads
/// them.
pub(crate) fn meta_filter(meta: Option<&Json>) -> Result<TransactionFilter, CoreError> {
	conditions_filter(meta, "conditions", "conditionsOp")
}

pub(crate) fn conditions_filter(
	meta: Option<&Json>,
	conditions: &str,
	op: &str,
) -> Result<TransactionFilter, CoreError> {
	let list = meta
		.and_then(|meta| meta.get(conditions))
		.and_then(Json::as_array)
		.unwrap_or_default();
	let any = meta.and_then(|meta| meta.get(op)).and_then(Json::as_str) == Some("or");
	TransactionFilter::new(list, any)
}

/// The rows `q('transactions')` shows (split children inline, parents not)
/// that pass the filter.
pub(crate) fn filtered_rows<'a>(
	budget: &'a BudgetData,
	filter: &'a TransactionFilter,
) -> impl Iterator<Item = Result<&'a Transaction, CoreError>> + 'a {
	budget
		.transactions
		.values()
		.filter(move |row| is_row(budget, row))
		.filter_map(move |row| match filter.matches(budget, row) {
			Ok(true) => Some(Ok(row)),
			Ok(false) => None,
			Err(error) => Some(Err(error)),
		})
}
