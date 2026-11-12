//! The transaction classification layer and every report built on it: Net Profit, Sankey, custom reports, statements and tax buckets.
//!
//! [`classify`] is the only place that decides what a row means. Every report
//! reads its [`Classification`] and never looks at `is_income` or `offbudget`.

mod balances;
mod classify;
mod custom_report;
mod dashboard;
mod json;
mod net_profit;
mod period;
mod report_eval;
mod report_query;
mod sankey;
mod standard;
mod statements;
mod tax;

pub use balances::{AccountBalance, Side, account_balances, net_worth};
pub use classify::{
	AdjustmentSource, Classification, ClassifiedRow, Decision, Flags, Leg, Partner,
	RECONCILIATION_NOTE, classify,
};
pub use custom_report::{BalanceType, CustomReport, GroupBy, Interval, SortBy};
pub use dashboard::{
	Dashboard, DashboardWidget, GRID_COLUMNS, WidgetContent, WidgetType, dashboards, place_widget,
};
pub use json::Json;
pub use net_profit::{CategoryLabels, CategoryLine, Line, NetProfit, Section, net_profit};
pub use period::{Period, months};
pub use report_eval::{
	Balances, IntervalBalances, ReportData, ReportGroup, ReportInterval, Uncategorized,
	evaluate_report,
};
pub use report_query::{QueryRow, report_range, report_rows, transaction_date_bounds};
pub use sankey::{Flow, Node, NodeKind, Sankey, sankey};
pub use standard::{NetWorthPoint, net_profit_by_month, net_worth_by_month};
pub use statements::{
	AccountGroupSection, BalanceSheet, CashFlow, OwnersEquity, SheetLine, balance_sheet, cash_flow,
	cash_flow_by_month, owners_equity,
};
pub use tax::{
	Bucket, BucketTotal, Direction, Source, TaxRow, TaxSummary, detail_csv, extract_tags,
	summary_csv, tax_summary,
};
