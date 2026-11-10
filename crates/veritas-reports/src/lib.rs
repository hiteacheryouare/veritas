//! The transaction classification layer and every report built on it: Net Profit, Sankey, custom reports, statements and tax buckets.
//!
//! [`classify`] is the only place that decides what a row means. Every report
//! reads its [`Classification`] and never looks at `is_income` or `offbudget`.

mod balances;
mod classify;
mod net_profit;
mod period;
mod standard;
mod statements;

pub use balances::{AccountBalance, Side, account_balances, net_worth};
pub use classify::{
	AdjustmentSource, Classification, ClassifiedRow, Decision, Flags, Leg, Partner,
	RECONCILIATION_NOTE, classify,
};
pub use net_profit::{CategoryLabels, CategoryLine, Line, NetProfit, Section, net_profit};
pub use period::{Period, months};
pub use standard::{NetWorthPoint, net_profit_by_month, net_worth_by_month};
pub use statements::{
	AccountGroupSection, BalanceSheet, CashFlow, OwnersEquity, SheetLine, balance_sheet, cash_flow,
	owners_equity,
};
