//! The transaction classification layer and every report built on it: Net Profit, Sankey, custom reports, statements and tax buckets.
//!
//! [`classify`] is the only place that decides what a row means. Every report
//! reads its [`Classification`] and never looks at `is_income` or `offbudget`.

mod balances;
mod classify;
mod custom_report;
mod dashboard;
mod filter;
mod json;
mod net_profit;
mod period;
mod report_eval;
mod report_query;
mod sankey;
mod standard;
mod statements;
mod tax;
mod widget_meta;
mod widgets;

pub use balances::{AccountBalance, Side, account_balances, net_worth};
pub use classify::{
	AdjustmentSource, Classification, ClassifiedRow, Decision, Flags, Leg, Partner,
	RECONCILIATION_NOTE, classify,
};
pub use custom_report::{BalanceType, CustomReport, GroupBy, Interval, SortBy};
pub use dashboard::{
	Dashboard, DashboardWidget, GRID_COLUMNS, WidgetContent, WidgetType, dashboards, place_widget,
};
pub use filter::TransactionFilter;
pub use json::Json;
pub use net_profit::{
	CategoryLabels, CategoryLine, Line, NetProfit, Section, labels as category_labels, net_profit,
};
pub use period::{Period, months};
pub use report_eval::{
	Balances, IntervalBalances, ReportData, ReportGroup, ReportInterval, Uncategorized,
	evaluate_report,
};
pub use report_query::{QueryRow, report_range, report_rows, transaction_date_bounds};
pub use sankey::{Flow, Node, NodeKind, Sankey, sankey};
pub use standard::{NetWorthPoint, net_profit_by_month, net_worth_by_month};
pub use statements::{
	AccountGroupSection, BalanceSheet, CashFlow, IncomeStatement, OwnersEquity, SheetLine,
	StatementGroup, balance_sheet, cash_flow, cash_flow_by_month, income_statement, owners_equity,
};
pub use tax::{
	Bucket, BucketTotal, Direction, Source, TaxRow, TaxRowText, TaxSummary, detail_csv,
	extract_tags, row_text, summary_csv, tax_summary,
};
pub use widget_meta::{
	DEFAULT_MARKDOWN, TimeFrame, WidgetSettings, add_widget, check_conditions, condition,
	create_page, delete_page, live_widget, parse_meta, read_settings, remove_widget, rename_page,
	set_widget_settings, write_settings,
};
pub use widgets::{
	AgeOfMoney, AgePoint, BalanceForecastCard, BudgetAnalysis, BudgetAnalysisMonth, CalendarDay,
	CalendarMonth, CashFlowCard, Crossover, CrossoverPoint, DEFAULT_FORMULA,
	FORECAST_UNASSIGNED_ACCOUNT, FontSizeMode, Forecast, ForecastBill, ForecastChartPoint,
	ForecastGranularity, ForecastLowest, ForecastPoint, ForecastRequest, ForecastSource,
	FormulaCard, FormulaError, FormulaErrorKind, FormulaValue, Granularity, MonteCarloCard,
	MonteCarloChartPoint, NetWorthCard, NetWorthInterval, NetWorthPeriod, NetWorthStack,
	SankeyCard, SankeyColor, SankeyLabel, SankeyLayer, SankeyLink, SankeyMode, SankeyNode,
	SankeySort, Spending, SpendingDay, SpendingMode, SummaryContent, SummaryData, TimeRange, Trend,
	age_of_money, balance_forecast_card, budget_analysis, calculate_time_range, calendar,
	card_forecast, cash_flow_card, crossover, formula_card, generate_forecast, monte_carlo,
	monte_carlo_card, net_worth_card, sankey_card, sankey_height_limit, spending, summary,
};
pub use widgets::{register_factor, register_scale};
