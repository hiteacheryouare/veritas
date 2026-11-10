use std::collections::BTreeMap;

use veritas_domain::{BudgetData, CoreError, Money, YearMonth};

use crate::balances::{self, add_to};
use crate::{Classification, NetProfit, Period, months, net_profit};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetWorthPoint {
	pub month: YearMonth,
	pub assets: Money,
	/// What is owed, as a positive amount.
	pub liabilities: Money,
	pub net_worth: Money,
}

/// The Net worth report: assets, liabilities and net worth at each month's end.
pub fn net_worth_by_month(
	budget: &BudgetData,
	classification: &Classification,
	first: YearMonth,
	last: YearMonth,
) -> Result<Vec<NetWorthPoint>, CoreError> {
	let mut sums = BTreeMap::new();
	let mut rows = classification.rows().iter().peekable();
	let mut points = Vec::new();
	for month in months(first, last)? {
		let end = Period::month(month).last();
		while let Some(row) = rows.next_if(|row| row.date <= end) {
			add_to(&mut sums, row)?;
		}
		let (assets, liabilities) = balances::totals(&balances::describe(budget, &sums))?;
		points.push(NetWorthPoint {
			month,
			assets,
			liabilities,
			net_worth: assets.checked_sub(liabilities)?,
		});
	}
	Ok(points)
}

/// Net Profit for each month. The Income vs expense report reads `income` and
/// `expenses` from it, and Spending by category reads `expenses.categories`.
pub fn net_profit_by_month(
	budget: &BudgetData,
	classification: &Classification,
	first: YearMonth,
	last: YearMonth,
) -> Result<Vec<NetProfit>, CoreError> {
	months(first, last)?
		.into_iter()
		.map(|month| net_profit(budget, classification, Period::month(month)))
		.collect()
}
