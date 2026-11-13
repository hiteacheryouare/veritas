//! Actual's custom reports and dashboards, read natively and written back in
//! Actual's own format (R-REP-4).

use std::sync::Arc;

use veritas_domain::{BudgetData, CivilDate, CoreError, CustomReportId, Row, WidgetId, YearMonth};
use veritas_reports::{
	self as reports, BalanceType, Classification, CustomReport, DefaultFrame, GroupBy, Interval,
	Json, Period, SortBy, Uncategorized, WidgetContent, WidgetType,
};

use crate::blocking::{CancelToken as CancelHandle, run};
use crate::budget::civil_date;
use crate::reports::{SankeyReport, net_worth_months, sankey_report};
use crate::session::Session;
use crate::{FfiError, Month};

/// Actual's first day of the week until its `firstDayOfWeekIdx` preference
/// is read: Sunday.
const FIRST_DAY_OF_WEEK: u32 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ReportGrouping {
	Category,
	Group,
	CategoryGroup,
	Payee,
	Account,
	Tag,
	Interval,
}

/// Actual's balance types, by the names its report editor shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ReportBalanceKind {
	Payment,
	Deposit,
	Net,
	NetPayment,
	NetDeposit,
	Budgeted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ReportSortOrder {
	Ascending,
	Descending,
	Name,
	Budget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ReportIntervalUnit {
	Daily,
	Weekly,
	Monthly,
	Yearly,
}

/// A custom report's settings, with Actual's defaults filled in. Saving
/// writes back only what changed; the filter conditions are kept as stored.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CustomReportSettings {
	pub id: String,
	pub name: String,
	/// `YYYY-MM` or `YYYY-MM-DD`, as Actual stores them.
	pub start_date: String,
	pub end_date: String,
	/// True for fixed dates; false follows `date_range` (e.g. "Last 6 months").
	pub date_static: bool,
	pub date_range: Option<String>,
	/// True shows one total per group; false shows each interval.
	pub total_mode: bool,
	pub group_by: ReportGrouping,
	pub balance_type: ReportBalanceKind,
	pub sort_by: ReportSortOrder,
	pub interval: ReportIntervalUnit,
	/// Actual's graph name, such as `BarGraph` or `DonutGraph`.
	pub graph_type: Option<String>,
	pub show_empty: bool,
	pub show_off_budget: bool,
	pub show_hidden: bool,
	pub show_uncategorized: bool,
	pub include_current: bool,
	pub trim_intervals: bool,
	/// Filter conditions stay read-only in Veritas.
	pub condition_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct CustomReportBalances {
	pub total_assets: i64,
	pub total_debts: i64,
	pub net_assets: i64,
	pub net_debts: i64,
	pub total_totals: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum UncategorizedKind {
	Uncategorized,
	OffBudget,
	Transfers,
	All,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CustomReportCell {
	/// `YYYY-MM`, `YYYY` or the first day for daily and weekly reports.
	pub interval: String,
	pub start: String,
	pub end: String,
	/// The report's balance type, read from `balances`.
	pub amount: i64,
	pub balances: CustomReportBalances,
	pub change: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CustomReportGroup {
	/// Empty for the Uncategorized entries, as in Actual.
	pub id: String,
	pub name: String,
	pub uncategorized: Option<UncategorizedKind>,
	pub amount: i64,
	pub balances: CustomReportBalances,
	pub intervals: Vec<CustomReportCell>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct StackedAmount {
	/// The group's id, or its name for the Uncategorized entries.
	pub key: String,
	pub amount: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CustomReportInterval {
	pub interval: CustomReportCell,
	pub stacked: Vec<StackedAmount>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CustomReportData {
	pub start: String,
	pub end: String,
	pub groups: Vec<CustomReportGroup>,
	pub intervals: Vec<CustomReportInterval>,
	pub total: i64,
	pub totals: CustomReportBalances,
}

/// A report as the viewer draws it. `problem` says why there is no `data`:
/// a setting Veritas can't evaluate yet, shown with "Open in Actual".
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CustomReportView {
	pub settings: CustomReportSettings,
	pub data: Option<CustomReportData>,
	pub problem: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CustomReportItem {
	pub id: String,
	pub name: String,
	/// Set when Veritas can't read the report's settings.
	pub problem: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum PlaceholderReason {
	/// A widget type Actual added that Veritas doesn't know.
	UnknownType,
	/// A known type Veritas doesn't draw yet.
	NotNative,
	/// The card filters its transactions, which Veritas reports don't do yet.
	Filtered,
	/// The custom report it shows was deleted.
	MissingReport,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
#[expect(
	clippy::large_enum_variant,
	reason = "UniFFI can't box a record, and a dashboard holds only a few dozen widgets"
)]
pub enum WidgetView {
	NetWorth {
		points: Vec<crate::reports::NetWorthMonth>,
	},
	/// Net Profit over the card's range.
	CashFlow {
		first: Month,
		last: Month,
		income: i64,
		expenses: i64,
		net_profit: i64,
	},
	/// Expenses in `month` against `compare_month`.
	Spending {
		month: Month,
		spent: i64,
		compare_month: Month,
		compare_spent: i64,
	},
	Sankey {
		report: SankeyReport,
	},
	CustomReport {
		report: CustomReportView,
	},
	Markdown {
		text: String,
		align: Option<String>,
	},
	/// Shown as "Open in Actual" and written back untouched.
	OpenInActual {
		reason: PlaceholderReason,
	},
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DashboardWidget {
	pub id: String,
	/// Actual's widget type, such as `net-worth-card`.
	pub kind: String,
	pub name: Option<String>,
	/// On Actual's 12-column grid.
	pub x: i64,
	pub y: i64,
	pub width: i64,
	pub height: i64,
	pub min_width: i64,
	pub min_height: i64,
	pub view: WidgetView,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DashboardPage {
	pub id: String,
	pub name: String,
	pub widgets: Vec<DashboardWidget>,
}

fn grouping(group_by: GroupBy) -> ReportGrouping {
	match group_by {
		GroupBy::Category => ReportGrouping::Category,
		GroupBy::Group => ReportGrouping::Group,
		GroupBy::CategoryGroup => ReportGrouping::CategoryGroup,
		GroupBy::Payee => ReportGrouping::Payee,
		GroupBy::Account => ReportGrouping::Account,
		GroupBy::Tag => ReportGrouping::Tag,
		GroupBy::Interval => ReportGrouping::Interval,
	}
}

fn group_by(grouping: ReportGrouping) -> GroupBy {
	match grouping {
		ReportGrouping::Category => GroupBy::Category,
		ReportGrouping::Group => GroupBy::Group,
		ReportGrouping::CategoryGroup => GroupBy::CategoryGroup,
		ReportGrouping::Payee => GroupBy::Payee,
		ReportGrouping::Account => GroupBy::Account,
		ReportGrouping::Tag => GroupBy::Tag,
		ReportGrouping::Interval => GroupBy::Interval,
	}
}

fn balance_kind(balance: BalanceType) -> ReportBalanceKind {
	match balance {
		BalanceType::TotalDebts => ReportBalanceKind::Payment,
		BalanceType::TotalAssets => ReportBalanceKind::Deposit,
		BalanceType::TotalTotals => ReportBalanceKind::Net,
		BalanceType::NetDebts => ReportBalanceKind::NetPayment,
		BalanceType::NetAssets => ReportBalanceKind::NetDeposit,
		BalanceType::TotalBudgeted => ReportBalanceKind::Budgeted,
	}
}

fn balance_type(kind: ReportBalanceKind) -> BalanceType {
	match kind {
		ReportBalanceKind::Payment => BalanceType::TotalDebts,
		ReportBalanceKind::Deposit => BalanceType::TotalAssets,
		ReportBalanceKind::Net => BalanceType::TotalTotals,
		ReportBalanceKind::NetPayment => BalanceType::NetDebts,
		ReportBalanceKind::NetDeposit => BalanceType::NetAssets,
		ReportBalanceKind::Budgeted => BalanceType::TotalBudgeted,
	}
}

fn sort_order(sort: SortBy) -> ReportSortOrder {
	match sort {
		SortBy::Asc => ReportSortOrder::Ascending,
		SortBy::Desc => ReportSortOrder::Descending,
		SortBy::Name => ReportSortOrder::Name,
		SortBy::Budget => ReportSortOrder::Budget,
	}
}

fn sort_by(order: ReportSortOrder) -> SortBy {
	match order {
		ReportSortOrder::Ascending => SortBy::Asc,
		ReportSortOrder::Descending => SortBy::Desc,
		ReportSortOrder::Name => SortBy::Name,
		ReportSortOrder::Budget => SortBy::Budget,
	}
}

fn interval_unit(interval: Interval) -> ReportIntervalUnit {
	match interval {
		Interval::Daily => ReportIntervalUnit::Daily,
		Interval::Weekly => ReportIntervalUnit::Weekly,
		Interval::Monthly => ReportIntervalUnit::Monthly,
		Interval::Yearly => ReportIntervalUnit::Yearly,
	}
}

fn interval(unit: ReportIntervalUnit) -> Interval {
	match unit {
		ReportIntervalUnit::Daily => Interval::Daily,
		ReportIntervalUnit::Weekly => Interval::Weekly,
		ReportIntervalUnit::Monthly => Interval::Monthly,
		ReportIntervalUnit::Yearly => Interval::Yearly,
	}
}

pub(crate) fn settings(report: &CustomReport) -> CustomReportSettings {
	CustomReportSettings {
		id: report.id.to_string(),
		name: report.name.clone(),
		start_date: report.start_date.clone(),
		end_date: report.end_date.clone(),
		date_static: report.date_static,
		date_range: report.date_range.clone(),
		total_mode: report.total_mode,
		group_by: grouping(report.group_by),
		balance_type: balance_kind(report.balance_type),
		sort_by: sort_order(report.sort_by),
		interval: interval_unit(report.interval),
		graph_type: report.row.graph_type.clone(),
		show_empty: report.show_empty,
		show_off_budget: report.show_off_budget,
		show_hidden: report.show_hidden,
		show_uncategorized: report.show_uncategorized,
		include_current: report.include_current,
		trim_intervals: report.trim_intervals,
		condition_count: u32::try_from(report.conditions.len()).unwrap_or(u32::MAX),
	}
}

fn stored(budget: &BudgetData, id: &str) -> Result<CustomReport, CoreError> {
	let row = budget
		.custom_reports
		.get(&CustomReportId::new(id))
		.filter(|row| !row.tombstone)
		.ok_or_else(|| CoreError::NotFound {
			entity: "custom report",
			id: id.to_owned(),
		})?;
	CustomReport::from_row(row)
}

/// The stored report with `settings` applied. Only the fields the editor
/// shows change; everything else in the row stays as Actual wrote it.
pub(crate) fn edited(
	budget: &BudgetData,
	settings: &CustomReportSettings,
) -> Result<CustomReport, CoreError> {
	let mut report = stored(budget, &settings.id)?;
	let name = settings.name.trim();
	if name.is_empty() {
		return Err(CoreError::Invalid(
			"Enter a name for the report.".to_owned(),
		));
	}
	let taken = budget.custom_reports.values().any(|row| {
		!row.tombstone
			&& row.id != report.id
			&& row
				.name
				.as_deref()
				.is_some_and(|other| other.trim().eq_ignore_ascii_case(name))
	});
	if taken {
		return Err(CoreError::Invalid(format!(
			"There's already a report named {name}."
		)));
	}
	report.name = name.to_owned();
	report.start_date = settings.start_date.clone();
	report.end_date = settings.end_date.clone();
	report.date_static = settings.date_static;
	report.date_range = settings.date_range.clone();
	report.total_mode = settings.total_mode;
	report.group_by = group_by(settings.group_by);
	report.balance_type = balance_type(settings.balance_type);
	report.sort_by = sort_by(settings.sort_by);
	report.interval = interval(settings.interval);
	report.show_empty = settings.show_empty;
	report.show_off_budget = settings.show_off_budget;
	report.show_hidden = settings.show_hidden;
	report.show_uncategorized = settings.show_uncategorized;
	report.include_current = settings.include_current;
	report.trim_intervals = settings.trim_intervals;
	if report.row.graph_type != settings.graph_type {
		report.row.graph_type = settings.graph_type.clone();
	}
	Ok(report)
}

pub(crate) fn save_custom_report(
	budget: &BudgetData,
	settings: &CustomReportSettings,
) -> Result<Vec<Row>, CoreError> {
	let report = edited(budget, settings)?;
	let mut row = report.to_row();
	row.graph_type = report.row.graph_type.clone();
	Ok(vec![Row::CustomReport(row)])
}

pub(crate) fn place_widget(
	budget: &BudgetData,
	widget_id: &str,
	(x, y, width, height): (i64, i64, i64, i64),
) -> Result<Vec<Row>, CoreError> {
	let widget = budget
		.widgets
		.get(&WidgetId::new(widget_id))
		.filter(|widget| !widget.tombstone)
		.ok_or_else(|| CoreError::NotFound {
			entity: "widget",
			id: widget_id.to_owned(),
		})?;
	Ok(vec![Row::Widget(reports::place_widget(
		widget, x, y, width, height,
	)?)])
}

fn balances(from: &reports::Balances) -> CustomReportBalances {
	CustomReportBalances {
		total_assets: from.total_assets.minor(),
		total_debts: from.total_debts.minor(),
		net_assets: from.net_assets.minor(),
		net_debts: from.net_debts.minor(),
		total_totals: from.total_totals.minor(),
	}
}

fn cell(from: &reports::IntervalBalances, balance: BalanceType) -> CustomReportCell {
	CustomReportCell {
		interval: from.interval.clone(),
		start: from.start.clone(),
		end: from.end.clone(),
		amount: from.balances.get(balance).minor(),
		balances: balances(&from.balances),
		change: from.change.minor(),
	}
}

fn uncategorized(kind: Uncategorized) -> UncategorizedKind {
	match kind {
		Uncategorized::Other => UncategorizedKind::Uncategorized,
		Uncategorized::OffBudget => UncategorizedKind::OffBudget,
		Uncategorized::Transfer => UncategorizedKind::Transfers,
		Uncategorized::All => UncategorizedKind::All,
	}
}

fn report_data(data: &reports::ReportData, balance: BalanceType) -> CustomReportData {
	CustomReportData {
		start: data.start.clone(),
		end: data.end.clone(),
		groups: data
			.groups
			.iter()
			.map(|group| CustomReportGroup {
				id: group.id.clone(),
				name: group.name.clone(),
				uncategorized: group.uncategorized.map(uncategorized),
				amount: group.balances.get(balance).minor(),
				balances: balances(&group.balances),
				intervals: group
					.intervals
					.iter()
					.map(|interval| cell(interval, balance))
					.collect(),
			})
			.collect(),
		intervals: data
			.intervals
			.iter()
			.map(|interval| CustomReportInterval {
				interval: cell(&interval.interval, balance),
				stacked: interval
					.stacked
					.iter()
					.map(|(key, amount)| StackedAmount {
						key: key.clone(),
						amount: amount.minor(),
					})
					.collect(),
			})
			.collect(),
		total: data.totals.get(balance).minor(),
		totals: balances(&data.totals),
	}
}

pub(crate) fn view(
	budget: &BudgetData,
	report: &CustomReport,
	today: CivilDate,
) -> Result<CustomReportView, CoreError> {
	match reports::evaluate_report(budget, report, today.naive(), FIRST_DAY_OF_WEEK) {
		Ok(data) => Ok(CustomReportView {
			settings: settings(report),
			data: Some(report_data(&data, report.balance_type)),
			problem: None,
		}),
		Err(CoreError::Invalid(problem)) => Ok(CustomReportView {
			settings: settings(report),
			data: None,
			problem: Some(problem),
		}),
		Err(error) => Err(error),
	}
}

fn custom_reports(budget: &BudgetData) -> Vec<CustomReportItem> {
	let mut items: Vec<CustomReportItem> = budget
		.custom_reports
		.values()
		.filter(|row| !row.tombstone)
		.map(|row| CustomReportItem {
			id: row.id.to_string(),
			name: row.name.clone().unwrap_or_default(),
			problem: CustomReport::from_row(row)
				.err()
				.map(|error| error.to_string()),
		})
		.collect();
	items.sort_by(|a, b| {
		a.name
			.to_lowercase()
			.cmp(&b.name.to_lowercase())
			.then_with(|| a.id.cmp(&b.id))
	});
	items
}

fn has_conditions(meta: Option<&Json>) -> bool {
	meta.and_then(|meta| meta.get("conditions"))
		.and_then(Json::as_array)
		.is_some_and(|conditions| !conditions.is_empty())
}

fn expenses(
	budget: &BudgetData,
	classification: &Classification,
	month: YearMonth,
) -> Result<i64, CoreError> {
	Ok(
		reports::net_profit(budget, classification, Period::month(month))?
			.expenses
			.total
			.minor(),
	)
}

fn card(
	budget: &BudgetData,
	classification: &Classification,
	kind: WidgetType,
	meta: Option<&Json>,
	today: CivilDate,
) -> Result<WidgetView, CoreError> {
	let placeholder = |reason| Ok(WidgetView::OpenInActual { reason });
	if has_conditions(meta) {
		return placeholder(PlaceholderReason::Filtered);
	}
	let latest = classification.rows().last().map(|row| row.date);
	let months = |default| reports::widget_months(meta, default, today, latest);
	match kind {
		WidgetType::NetWorth => {
			let (first, last) = months(DefaultFrame::SixMonths)?;
			Ok(WidgetView::NetWorth {
				points: net_worth_months(budget, classification, first, last)?,
			})
		}
		WidgetType::CashFlow => {
			let (first, last) = months(DefaultFrame::ThisMonth)?;
			let period = Period::new(first.first_day(), Period::month(last).last())?;
			let report = reports::net_profit(budget, classification, period)?;
			Ok(WidgetView::CashFlow {
				first: first.into(),
				last: last.into(),
				income: report.income.total.minor(),
				expenses: report.expenses.total.minor(),
				net_profit: report.net_profit.total.minor(),
			})
		}
		WidgetType::Spending => match reports::spending_months(meta, today)? {
			Some((month, compare)) => Ok(WidgetView::Spending {
				month: month.into(),
				spent: expenses(budget, classification, month)?,
				compare_month: compare.into(),
				compare_spent: expenses(budget, classification, compare)?,
			}),
			None => placeholder(PlaceholderReason::NotNative),
		},
		WidgetType::Sankey => {
			let budgeted = meta
				.and_then(|meta| meta.get("mode"))
				.and_then(Json::as_str)
				.is_some_and(|mode| mode != "spent");
			if budgeted {
				return placeholder(PlaceholderReason::NotNative);
			}
			let (first, last) = months(DefaultFrame::SixMonths)?;
			let period = Period::new(first.first_day(), Period::month(last).last())?;
			let sankey = reports::sankey(budget, classification, period)?;
			Ok(WidgetView::Sankey {
				report: sankey_report(budget, &sankey),
			})
		}
		_ => placeholder(PlaceholderReason::NotNative),
	}
}

pub(crate) fn dashboards(
	budget: &BudgetData,
	classification: &Classification,
	today: CivilDate,
) -> Result<Vec<DashboardPage>, CoreError> {
	let mut pages = Vec::new();
	for page in reports::dashboards(budget)? {
		let mut widgets = Vec::new();
		for widget in &page.widgets {
			let stored = budget.widgets.get(&widget.id);
			let kind = stored
				.and_then(|stored| stored.kind.clone())
				.unwrap_or_default();
			let (min_width, min_height) =
				WidgetType::parse(&kind).map_or((1, 1), WidgetType::minimum_size);
			let (name, view) = match &widget.content {
				WidgetContent::Placeholder { .. } => (
					None,
					WidgetView::OpenInActual {
						reason: if WidgetType::parse(&kind).is_some() {
							PlaceholderReason::MissingReport
						} else {
							PlaceholderReason::UnknownType
						},
					},
				),
				WidgetContent::Markdown { content, align } => (
					None,
					WidgetView::Markdown {
						text: content.clone(),
						align: align.clone(),
					},
				),
				WidgetContent::CustomReport(id) => match stored_report(budget, id) {
					Some(report) => (
						Some(report.name.clone()),
						WidgetView::CustomReport {
							report: view(budget, &report, today)?,
						},
					),
					None => (
						None,
						WidgetView::OpenInActual {
							reason: PlaceholderReason::MissingReport,
						},
					),
				},
				WidgetContent::Card { kind, name, meta } => (
					name.clone(),
					card(budget, classification, *kind, meta.as_ref(), today)?,
				),
			};
			widgets.push(DashboardWidget {
				id: widget.id.to_string(),
				kind,
				name,
				x: widget.x,
				y: widget.y,
				width: widget.width,
				height: widget.height,
				min_width,
				min_height,
				view,
			});
		}
		pages.push(DashboardPage {
			id: page.id.to_string(),
			name: page.name,
			widgets,
		});
	}
	Ok(pages)
}

fn stored_report(budget: &BudgetData, id: &CustomReportId) -> Option<CustomReport> {
	budget
		.custom_reports
		.get(id)
		.filter(|row| !row.tombstone)
		.and_then(|row| CustomReport::from_row(row).ok())
}

#[uniffi::export]
impl Session {
	/// Every saved custom report, by name.
	pub fn custom_reports(&self) -> Result<Vec<CustomReportItem>, FfiError> {
		self.core.call(|session| session.data().map(custom_reports))
	}

	/// A saved report evaluated as of `today` (`YYYY-MM-DD`), the way
	/// Actual's report page computes it.
	pub async fn custom_report(
		&self,
		id: String,
		today: String,
		cancel: Arc<CancelHandle>,
	) -> Result<CustomReportView, FfiError> {
		let core = self.core.clone();
		run(&cancel, move |cancel| {
			core.report(cancel, |budget, _| {
				view(budget, &stored(budget, &id)?, civil_date(&today)?)
			})
		})
		.await
	}

	/// What a report would show with `settings`, for the editor, without
	/// saving anything.
	pub async fn preview_custom_report(
		&self,
		settings: CustomReportSettings,
		today: String,
		cancel: Arc<CancelHandle>,
	) -> Result<CustomReportView, FfiError> {
		let core = self.core.clone();
		run(&cancel, move |cancel| {
			core.report(cancel, |budget, _| {
				view(budget, &edited(budget, &settings)?, civil_date(&today)?)
			})
		})
		.await
	}

	/// Every dashboard page with each widget's numbers as of `today`.
	pub async fn dashboards(
		&self,
		today: String,
		cancel: Arc<CancelHandle>,
	) -> Result<Vec<DashboardPage>, FfiError> {
		let core = self.core.clone();
		run(&cancel, move |cancel| {
			core.report(cancel, |budget, classification| {
				dashboards(budget, classification, civil_date(&today)?)
			})
		})
		.await
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::Command;
	use crate::session::Core;
	use crate::test_support::{imported, reopened};

	const SPENDING: &str = "c7eaad37-4a0f-4f44-8c5e-ca30ff232dd2";
	const NET_WORTH_CARD: &str = "a8828813-ceda-452a-8de4-4a24a78c531e";

	fn today() -> CivilDate {
		CivilDate::new(2026, 11, 13).unwrap()
	}

	fn data(core: &Core<veritas_store::Store>) -> BudgetData {
		core.call(|session| session.data().cloned()).unwrap()
	}

	fn pages(core: &Core<veritas_store::Store>) -> Vec<DashboardPage> {
		core.report(
			&veritas_session::CancelToken::default(),
			|budget, classification| dashboards(budget, classification, today()),
		)
		.unwrap()
	}

	fn widget<'a>(pages: &'a [DashboardPage], id: &str) -> &'a DashboardWidget {
		pages
			.iter()
			.flat_map(|page| &page.widgets)
			.find(|widget| widget.id == id)
			.unwrap()
	}

	#[test]
	fn saved_reports_are_listed_by_name_and_evaluate_as_actual_does() {
		let (_scratch, core) = imported("all-features");
		let budget = data(&core);
		let names: Vec<String> = custom_reports(&budget)
			.into_iter()
			.map(|item| item.name)
			.collect();
		assert_eq!(names, ["Net worth over time", "Spending by category"]);
		let report = view(&budget, &stored(&budget, SPENDING).unwrap(), today()).unwrap();
		assert_eq!(report.problem, None);
		assert_eq!(report.settings.group_by, ReportGrouping::Category);
		assert_eq!(report.settings.balance_type, ReportBalanceKind::Payment);
		assert_eq!(report.settings.graph_type.as_deref(), Some("BarGraph"));
		let data = report.data.unwrap();
		assert!(!data.groups.is_empty());
		assert_eq!(
			data.groups.iter().map(|group| group.amount).sum::<i64>(),
			data.total
		);
	}

	#[test]
	fn a_setting_veritas_cannot_evaluate_comes_back_as_a_problem_not_an_error() {
		let (_scratch, core) = imported("all-features");
		let budget = data(&core);
		let mut settings = settings(&stored(&budget, SPENDING).unwrap());
		settings.balance_type = ReportBalanceKind::Budgeted;
		let report = view(&budget, &edited(&budget, &settings).unwrap(), today()).unwrap();
		assert_eq!(report.data, None);
		assert!(report.problem.unwrap().contains("Budgeted"));
	}

	#[test]
	fn saving_a_report_writes_only_what_changed_and_undoes() {
		let (scratch, core) = imported("all-features");
		let before = data(&core).custom_reports[&CustomReportId::new(SPENDING)].clone();
		let mut edit = settings(&CustomReport::from_row(&before).unwrap());
		edit.name = "  Where it went  ".to_owned();
		edit.graph_type = Some("DonutGraph".to_owned());
		core.perform(Command::SaveCustomReport {
			settings: edit.clone(),
		})
		.unwrap();
		drop(core);

		let core = reopened(&scratch);
		let after = data(&core).custom_reports[&CustomReportId::new(SPENDING)].clone();
		assert_eq!(after.name.as_deref(), Some("Where it went"));
		assert_eq!(after.graph_type.as_deref(), Some("DonutGraph"));
		assert_eq!(
			veritas_domain::CustomReportRow {
				name: before.name.clone(),
				graph_type: before.graph_type.clone(),
				..after
			},
			before
		);

		edit.name = "net worth OVER time".to_owned();
		assert!(matches!(
			core.perform(Command::SaveCustomReport { settings: edit }),
			Err(FfiError::Invalid { .. })
		));
	}

	#[test]
	fn dashboards_draw_known_cards_and_keep_the_rest_as_placeholders() {
		let (_scratch, core) = imported("all-features");
		let pages = pages(&core);
		let names: Vec<&str> = pages.iter().map(|page| page.name.as_str()).collect();
		assert_eq!(names, ["Main", "Deep dive"]);

		let WidgetView::NetWorth { points } = &widget(&pages, NET_WORTH_CARD).view else {
			panic!("the net worth card should draw natively");
		};
		assert_eq!(points.len(), 6);
		assert_eq!(
			points.last().unwrap().month,
			YearMonth::new(2026, 11).unwrap().into()
		);

		let view = |id: &str| widget(&pages, id).view.clone();
		assert!(matches!(
			view("07fb54b8-dd18-40de-8bca-381826688a49"),
			WidgetView::Spending { .. }
		));
		assert_eq!(
			view("e24fe8dc-a2ab-4c62-8e34-43eef7285413"),
			WidgetView::OpenInActual {
				reason: PlaceholderReason::NotNative
			}
		);
		assert_eq!(
			view("b595baa8-f97d-4f1a-ac8f-d7c2a1d7a0b3"),
			WidgetView::OpenInActual {
				reason: PlaceholderReason::UnknownType
			}
		);
		assert_eq!(
			view("5658fbe9-b6d0-428e-a4ff-77b3eb54803b"),
			WidgetView::OpenInActual {
				reason: PlaceholderReason::Filtered
			}
		);
		assert!(matches!(
			view("8bb52bc7-1c5b-4af7-8236-aa36926a85da"),
			WidgetView::Sankey { .. }
		));
		assert!(matches!(
			view("a7456f7b-ad43-448c-a280-7cea8714f699"),
			WidgetView::Markdown { text, .. } if text.contains("**in full**")
		));
		let report = widget(&pages, "8de603a2-58a0-411b-8e1c-cca26d049f0c");
		assert_eq!(report.name.as_deref(), Some("Spending by category"));
		assert!(
			matches!(&report.view, WidgetView::CustomReport { report } if report.data.is_some())
		);
		assert_eq!(widget(&pages, NET_WORTH_CARD).min_width, 3);
	}

	#[test]
	fn placing_a_widget_moves_only_its_frame_and_undoes() {
		let (scratch, core) = imported("all-features");
		let before = data(&core).widgets[&WidgetId::new(NET_WORTH_CARD)].clone();
		core.perform(Command::PlaceWidget {
			widget_id: NET_WORTH_CARD.to_owned(),
			x: 6,
			y: 12,
			width: 1,
			height: 2,
		})
		.unwrap();
		drop(core);

		let core = reopened(&scratch);
		let moved = widget(&pages(&core), NET_WORTH_CARD).clone();
		assert_eq!((moved.x, moved.y, moved.width, moved.height), (6, 12, 3, 2));
		let stored = data(&core).widgets[&WidgetId::new(NET_WORTH_CARD)].clone();
		assert_eq!(stored.meta, before.meta);
		assert_eq!(stored.kind, before.kind);

		core.perform(Command::PlaceWidget {
			widget_id: NET_WORTH_CARD.to_owned(),
			x: 0,
			y: 2,
			width: 6,
			height: 2,
		})
		.unwrap();
		core.undo().unwrap();
		let undone = widget(&pages(&core), NET_WORTH_CARD).clone();
		assert_eq!((undone.x, undone.y, undone.width), (6, 12, 3));
	}
}
