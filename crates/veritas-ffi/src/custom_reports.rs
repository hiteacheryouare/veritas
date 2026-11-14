//! Actual's custom reports, read natively and written back in Actual's own
//! format (R-REP-4).

use std::sync::Arc;

use veritas_domain::{BudgetData, CivilDate, CoreError, CustomReportId, CustomReportRow, Row};
use veritas_reports::{
	self as reports, BalanceType, CustomReport, GroupBy, Interval, Json, SortBy, Uncategorized,
};

use crate::FfiError;
use crate::blocking::{CancelToken as CancelHandle, run};
use crate::budget::civil_date;
use crate::session::Session;

/// Actual's first day of the week until its `firstDayOfWeekIdx` preference
/// is read: Sunday.
pub(crate) const FIRST_DAY_OF_WEEK: u32 = 0;

/// One filter condition as Actual stores it. `value` and `options` are
/// Actual's JSON as text: `"\"<id>\""` for an id or a string, `-5000` for an
/// amount, `true`, `["a","b"]` for `oneOf`, `{"num1":1,"num2":2}` for
/// `isbetween`; `options` is `{"inflow":true}`, `{"outflow":true}`,
/// `{"month":true}` or `{"year":true}`, or `nil`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ReportCondition {
	pub field: String,
	pub op: String,
	pub value: String,
	#[uniffi(default = None)]
	pub options: Option<String>,
}

pub(crate) fn report_condition(stored: &Json) -> ReportCondition {
	let text = |key: &str| {
		stored
			.get(key)
			.and_then(Json::as_str)
			.unwrap_or_default()
			.to_owned()
	};
	ReportCondition {
		field: text("field"),
		op: text("op"),
		value: stored.get("value").unwrap_or(&Json::Null).stringify(),
		options: stored
			.get("options")
			.filter(|options| !matches!(options, Json::Null))
			.map(Json::stringify),
	}
}

fn json(text: &str, what: &str) -> Result<Json, CoreError> {
	Json::parse(text).map_err(|reason| CoreError::Invalid(format!("a filter's {what}: {reason}")))
}

/// The conditions to store. One that matches a stored condition keeps that
/// condition's JSON exactly; a new one is written the way Actual's filter
/// editor writes it.
pub(crate) fn stored_conditions(
	conditions: &[ReportCondition],
	stored: &[Json],
) -> Result<Vec<Json>, CoreError> {
	conditions
		.iter()
		.map(|condition| {
			let value = json(&condition.value, "value")?;
			let options = condition
				.options
				.as_deref()
				.map(|options| json(options, "options"))
				.transpose()?;
			let same = |old: &&Json| {
				let old = report_condition(old);
				old.field == condition.field
					&& old.op == condition.op
					&& old.value == value.stringify()
					&& old.options == options.as_ref().map(Json::stringify)
			};
			match stored.iter().find(same) {
				Some(old) => Ok(old.clone()),
				None => reports::condition(&condition.field, &condition.op, value, options),
			}
		})
		.collect()
}

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
	/// The report's filters, all of which must match unless `conditions_or`.
	pub conditions: Vec<ReportCondition>,
	pub conditions_or: bool,
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
		conditions: report.conditions.iter().map(report_condition).collect(),
		conditions_or: report.conditions_or,
	}
}

pub(crate) fn stored(budget: &BudgetData, id: &str) -> Result<CustomReport, CoreError> {
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
	report.name = report_name(budget, &settings.name, Some(&report.id))?;
	report.conditions = stored_conditions(&settings.conditions, &report.conditions)?;
	report.conditions_or = settings.conditions_or;
	reports::check_conditions(&report.conditions, report.conditions_or)?;
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

fn report_name(
	budget: &BudgetData,
	name: &str,
	except: Option<&CustomReportId>,
) -> Result<String, CoreError> {
	let name = name.trim();
	if name.is_empty() {
		return Err(CoreError::Invalid(
			"Enter a name for the report.".to_owned(),
		));
	}
	let taken = budget.custom_reports.values().any(|row| {
		!row.tombstone
			&& Some(&row.id) != except
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
	Ok(name.to_owned())
}

/// A new report as `report/create` inserts it: every column `fromJS` writes,
/// with Actual's defaults for what the settings don't carry (no trend lines,
/// all tags, `BarGraph`). `settings.id` is ignored.
pub(crate) fn create_custom_report(
	budget: &BudgetData,
	id: CustomReportId,
	settings: &CustomReportSettings,
) -> Result<Row, CoreError> {
	let name = report_name(budget, &settings.name, None)?;
	if settings.group_by == ReportGrouping::Tag
		&& settings.balance_type == ReportBalanceKind::Budgeted
	{
		return Err(CoreError::Invalid(
			"Budgeted reports cannot be split by tags.".to_owned(),
		));
	}
	let conditions = stored_conditions(&settings.conditions, &[])?;
	reports::check_conditions(&conditions, settings.conditions_or)?;
	let blank = CustomReportRow {
		id,
		name: Some(name.clone()),
		start_date: None,
		end_date: None,
		date_static: None,
		date_range: None,
		mode: None,
		group_by: None,
		balance_type: None,
		show_empty: None,
		show_offbudget: None,
		show_hidden: None,
		show_uncategorized: None,
		selected_categories: None,
		graph_type: None,
		conditions: None,
		conditions_op: None,
		metadata: Some(r#"{"tagScope":{"mode":"all"}}"#.to_owned()),
		interval: None,
		color_scheme: None,
		tombstone: false,
		include_current: None,
		sort_by: None,
		trim_intervals: None,
		show_trend_lines: Some(0),
	};
	let report = CustomReport {
		id: blank.id.clone(),
		name,
		start_date: settings.start_date.clone(),
		end_date: settings.end_date.clone(),
		date_static: settings.date_static,
		date_range: settings.date_range.clone(),
		total_mode: settings.total_mode,
		group_by: group_by(settings.group_by),
		balance_type: balance_type(settings.balance_type),
		sort_by: sort_by(settings.sort_by),
		interval: interval(settings.interval),
		show_empty: settings.show_empty,
		show_off_budget: settings.show_off_budget,
		show_hidden: settings.show_hidden,
		show_uncategorized: settings.show_uncategorized,
		include_current: settings.include_current,
		trim_intervals: settings.trim_intervals,
		conditions,
		conditions_or: settings.conditions_or,
		row: blank,
	};
	let mut row = report.to_new_row();
	row.graph_type = Some(
		settings
			.graph_type
			.clone()
			.unwrap_or_else(|| "BarGraph".to_owned()),
	);
	Ok(Row::CustomReport(row))
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
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::Command;
	use crate::session::Core;
	use crate::test_support::{imported, reopened};

	const SPENDING: &str = "c7eaad37-4a0f-4f44-8c5e-ca30ff232dd2";

	fn today() -> CivilDate {
		CivilDate::new(2026, 11, 13).unwrap()
	}

	fn data(core: &Core<veritas_store::Store>) -> BudgetData {
		core.call(|session| session.data().cloned()).unwrap()
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
}
