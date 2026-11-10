//! The transaction classification layer and every report built on it: Net Profit, Sankey, custom reports, statements and tax buckets.
//!
//! [`classify`] is the only place that decides what a row means. Every report
//! reads its [`Classification`] and never looks at `is_income` or `offbudget`.

mod classify;
mod net_profit;
mod period;

pub use classify::{
	AdjustmentSource, Classification, ClassifiedRow, Decision, Flags, Leg, RECONCILIATION_NOTE,
	classify,
};
pub use net_profit::{CategoryLabels, CategoryLine, Line, NetProfit, Section, net_profit};
pub use period::Period;
