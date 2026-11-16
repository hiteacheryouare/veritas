//! Actual's dashboards: every widget drawn natively with its settings (D73),
//! and the edits Actual's dashboard handlers make, written in its format.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::NaiveDate;
use veritas_domain::{
	BudgetData, CivilDate, CoreError, CustomReportId, DashboardPageId, Money, Row, WidgetId,
	YearMonth,
};
use veritas_engine::EnvelopeSheet;
use veritas_reports::{
	self as reports, CustomReport, Json, WidgetContent, WidgetSettings, WidgetType,
};

use crate::blocking::{CancelToken as CancelHandle, run};
use crate::budget::civil_date;
use crate::custom_reports::{CustomReportView, FIRST_DAY_OF_WEEK, view};
use crate::rules::{RuleCondition, filter_conditions, stored_filters};
use crate::session::Session;
use crate::{FfiError, Month};

/// A card's `timeFrame`. `mode` is Actual's (`sliding-window`, `static`,
/// `full`, `yearToDate`, `lastYear`, `lastMonth`, `priorYearToDate`,
/// `currentQuarter`, `previousQuarter`); `start` and `end` are `YYYY-MM` or
/// `YYYY-MM-DD`, as Actual stores them.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct WidgetTimeFrame {
	pub mode: String,
	pub start: String,
	pub end: String,
}

/// A widget's settings, as Actual keeps them in its `meta`. Keys Veritas
/// doesn't show are kept as stored.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct WidgetConfig {
	pub name: Option<String>,
	pub time_frame: Option<WidgetTimeFrame>,
	pub conditions: Vec<RuleCondition>,
	pub conditions_or: bool,
	/// Net worth `trend` or `stacked`; spending `single-month`, `average` or
	/// `budget`; Sankey `spent` or `budgeted`.
	pub mode: Option<String>,
	/// The summary card's `sum`, `avgPerMonth`, `avgPerYear`,
	/// `avgPerTransact` or `percentage`.
	pub summary: Option<String>,
	/// The markdown card's text.
	pub content: Option<String>,
	/// The markdown card's `left`, `center` or `right`.
	pub text_align: Option<String>,
	pub formula: Option<String>,
	/// The saved report a custom-report widget shows.
	pub report_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct CalendarDayCell {
	pub date: String,
	pub income: i64,
	/// A positive magnitude, as Actual takes `Math.abs`.
	pub expense: i64,
	/// Each day's percentage, 0 to 100, of the month's income or expense total.
	pub income_size: f64,
	pub expense_size: f64,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct CalendarCardMonth {
	pub start: String,
	pub end: String,
	pub days: Vec<CalendarDayCell>,
	pub total_income: i64,
	pub total_expense: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum AgeGranularity {
	Daily,
	Weekly,
	Monthly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum AgeTrend {
	Up,
	Down,
	Stable,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AgePoint {
	/// The first day of the period.
	pub period: String,
	pub days: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BudgetAnalysisPoint {
	pub month: Month,
	pub budgeted: i64,
	pub spent: i64,
	pub balance: i64,
	pub overspending_adjustment: i64,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct CrossoverCardPoint {
	pub month: Month,
	pub investment_income: f64,
	pub expenses: f64,
	pub nest_egg: f64,
	pub adjusted_expenses: Option<f64>,
	/// A projected month rather than history.
	pub projection: bool,
}

/// A card's months or days as `calculateTimeRange` gives them: `start` and
/// `end` are `YYYY-MM` or `YYYY-MM-DD`, and `mode` is Actual's.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct WidgetRange {
	pub start: String,
	pub end: String,
	pub mode: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum NetWorthCardInterval {
	Daily,
	Weekly,
	Monthly,
	Yearly,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct NetWorthCardPoint {
	/// The period's first day (a week's first day when weekly).
	pub start: String,
	pub net_worth: i64,
	/// Positive balances, and negative ones as a magnitude.
	pub assets: i64,
	pub debt: i64,
	/// Against the point before, or the balance before the first point.
	pub change: i64,
}

/// One account's area in the stacked mode.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct NetWorthCardSeries {
	pub account_id: String,
	pub name: String,
	/// The account's balance at each of the view's `points`.
	pub points: Vec<i64>,
	/// `chartQual1` to `chartQual9`, as 0 to 8.
	pub color: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SpendingCardMode {
	SingleMonth,
	Budget,
	Average,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SpendingCardDay {
	/// 1 to 28.
	pub day: u32,
	/// The running total in `compare_month`, `nil` for days after today.
	pub compare: Option<i64>,
	pub compare_to: Option<i64>,
	/// The averaged months' running total, rounded.
	pub average: i64,
	/// The month's budget spread evenly over its days, so far: negative and
	/// fractional minor units, as Actual accumulates it.
	pub budget: f64,
}

/// A node's words. The Mac words each case as ST-10.10 says.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum SankeyLabel {
	/// A payee, account, category group or category, by its own name.
	Name {
		name: String,
	},
	Budgeted,
	AvailableFunds,
	/// Money budgeted back out of a category.
	FromCategory {
		category: String,
	},
	ToBudget,
	Overbudgeted,
	/// Every account as one node, with Group accounts on.
	AllAccounts,
	FromMonth {
		month: Month,
	},
	ForMonth {
		month: Month,
	},
	Overspent,
	Other,
	/// A spacer that keeps a node in its column; never drawn.
	Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SankeyCardColor {
	/// `chartQual1` to `chartQual9`, as 0 to 8.
	Palette {
		index: u8,
	},
	ToBudgetPositive,
	ToBudgetNegative,
	Gray,
	Blue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SankeyCardMode {
	/// Actual's `spent`: categorized rows.
	Categorized,
	Budgeted,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SankeyCardNode {
	/// Actual's node key: an id, an id with a suffix, or a fixed name.
	pub key: String,
	pub label: SankeyLabel,
	/// The node's share of its column as Actual writes it ("12.5%"); empty
	/// for hidden nodes.
	pub percentage: String,
	pub color: SankeyCardColor,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SankeyFolded {
	pub name: String,
	pub value: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SankeyCardLink {
	pub source: u32,
	/// `nil` when the layer range cut the target; draw nothing for it.
	pub target: Option<u32>,
	/// Links into or out of a hidden node carry -1.
	pub value: i64,
	/// The nodes an Other node holds along this link, largest first.
	pub folded: Vec<SankeyFolded>,
	pub color: Option<SankeyCardColor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ForecastCardSource {
	Schedules,
	TrackingBudget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ForecastCardGranularity {
	Daily,
	Monthly,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ForecastCardPoint {
	/// `YYYY-MM` when monthly, `YYYY-MM-DD` when daily.
	pub date: String,
	pub balance: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FormulaErrorKind {
	/// The text doesn't start with `=`.
	NotAFormula,
	Cycle,
	DivByZero,
	Error,
	Na,
	Name,
	Num,
	Ref,
	Spill,
	Value,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FormulaError {
	pub kind: FormulaErrorKind,
	/// Actual's exact text: `Formula error: DIV_BY_ZERO` or `Formula must
	/// start with =`.
	pub message: String,
}

/// The formula's result. A number is HyperFormula's, already rounded to 10
/// significant digits, not yet formatted.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum FormulaValue {
	Number { value: f64 },
	Text { text: String },
	Boolean { value: bool },
	Empty,
	Error { error: FormulaError },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FormulaFontSize {
	Dynamic,
	Static,
}

/// One year of the fan chart: the card shades p10 to p90 and p25 to p75 and
/// draws p50.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MonteCarloCardPoint {
	/// 0 is the start.
	pub year: u32,
	/// The current age plus `year`; ages needn't be whole.
	pub age: f64,
	/// Whole minor units, or NaN or infinite when a run's balance is, as
	/// Actual draws it.
	pub p10: f64,
	pub p25: f64,
	pub p50: f64,
	pub p75: f64,
	pub p90: f64,
}

/// What a widget draws: data only, worded by the Mac (ST-10.10). Money is
/// in stored minor units; the `f64` figures are Actual's own unrounded
/// values, rounded only for display.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum WidgetView {
	/// `change` is the headline: the last period's balance less the first's.
	/// `accounts` are the ids with a balance in some period, in Actual's
	/// order; `series` are the same accounts in the stacked mode's order,
	/// first at the bottom.
	NetWorth {
		range: WidgetRange,
		interval: NetWorthCardInterval,
		points: Vec<NetWorthCardPoint>,
		net_worth: i64,
		change: i64,
		lowest: Option<i64>,
		highest: Option<i64>,
		has_negative: bool,
		accounts: Vec<String>,
		series: Vec<NetWorthCardSeries>,
	},
	/// `income` is money in and `expense` money out (negative), and
	/// `change` is `income + expense`, the headline.
	CashFlow {
		range: WidgetRange,
		income: i64,
		expense: i64,
		change: i64,
	},
	/// `difference` is the headline: the chosen comparison's running total
	/// less `compare`'s, through `headline_day`, rounded.
	Spending {
		mode: SpendingCardMode,
		compare: Month,
		compare_to: Month,
		average_first: Option<Month>,
		average_last: Option<Month>,
		days: Vec<SpendingCardDay>,
		headline_day: u32,
		difference: i64,
	},
	/// `first` and `last` are the months drawn: the range clamped to the
	/// months with transactions.
	Sankey {
		range: WidgetRange,
		first: Month,
		last: Month,
		mode: SankeyCardMode,
		show_percentages: bool,
		nodes: Vec<SankeyCardNode>,
		links: Vec<SankeyCardLink>,
	},
	/// `low` is the lowest point when it isn't the last; `today` indexes
	/// `points` at this month (monthly only). `scheduled` counts bill
	/// occurrences on the days drawn.
	BalanceForecast {
		range: WidgetRange,
		source: ForecastCardSource,
		granularity: ForecastCardGranularity,
		zero_based: bool,
		has_forecast: bool,
		points: Vec<ForecastCardPoint>,
		low: Option<ForecastCardPoint>,
		scheduled: u32,
		filtered: bool,
		today: Option<u32>,
	},
	Formula {
		formula: String,
		value: FormulaValue,
		show_title: bool,
		font_size: FormulaFontSize,
		static_font_size: f64,
	},
	/// `success_percent` is Actual's, to one decimal; `simulations` is the
	/// number of lifetimes actually run.
	MonteCarlo {
		success_percent: f64,
		start_age: f64,
		end_age: f64,
		simulations: u32,
		chart: Vec<MonteCarloCardPoint>,
	},
	CustomReport {
		report: CustomReportView,
	},
	Markdown {
		text: String,
		align: Option<String>,
	},
	/// `value` is minor units, or a percentage for `percentage`; it can be
	/// fractional, infinite or NaN, as in Actual.
	Summary {
		start: String,
		end: String,
		value: f64,
		dividend: f64,
		divisor: f64,
	},
	Calendar {
		months: Vec<CalendarCardMonth>,
	},
	AgeOfMoney {
		start: String,
		end: String,
		granularity: AgeGranularity,
		points: Vec<AgePoint>,
		current_days: Option<i64>,
		trend: AgeTrend,
		insufficient_data: bool,
	},
	BudgetAnalysis {
		start: String,
		end: String,
		months: Vec<BudgetAnalysisPoint>,
		total_budgeted: i64,
		total_spent: i64,
		total_overspending_adjustment: i64,
		final_overspending_adjustment: i64,
	},
	Crossover {
		start: Month,
		end: Month,
		points: Vec<CrossoverCardPoint>,
		crossover: Option<Month>,
		last_known_balance: f64,
		last_known_monthly_income: f64,
		last_known_monthly_expenses: f64,
		historical_return: Option<f64>,
		years_to_retire: Option<f64>,
		target_monthly_income: Option<f64>,
		target_nest_egg: Option<f64>,
	},
	/// The custom report it shows was deleted.
	MissingReport,
	/// Why the card can't draw: settings Actual's own card can't draw
	/// either, or a budgeted Sankey month with no budget.
	Problem {
		message: String,
	},
	/// A widget type newer than Veritas. It moves, resizes and deletes, and
	/// its settings are kept as stored.
	Unknown {
		kind: String,
	},
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DashboardWidget {
	pub id: String,
	/// Actual's widget type, such as `net-worth-card`.
	pub kind: String,
	/// On Actual's 12-column grid.
	pub x: i64,
	pub y: i64,
	pub width: i64,
	pub height: i64,
	pub min_width: i64,
	pub min_height: i64,
	pub config: WidgetConfig,
	pub view: WidgetView,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DashboardPage {
	pub id: String,
	pub name: String,
	pub widgets: Vec<DashboardWidget>,
}

fn config(budget: &BudgetData, settings: WidgetSettings) -> WidgetConfig {
	WidgetConfig {
		name: settings.name,
		time_frame: settings.time_frame.map(|frame| WidgetTimeFrame {
			mode: frame.mode,
			start: frame.start,
			end: frame.end,
		}),
		conditions: filter_conditions(budget, &settings.conditions),
		conditions_or: settings.conditions_or,
		mode: settings.mode,
		summary: settings.summary,
		content: settings.content,
		text_align: settings.text_align,
		formula: settings.formula,
		report_id: settings.report_id,
	}
}

fn settings(
	budget: &BudgetData,
	config: &WidgetConfig,
	stored: &[Json],
) -> Result<WidgetSettings, CoreError> {
	Ok(WidgetSettings {
		name: config.name.clone(),
		time_frame: config.time_frame.as_ref().map(|frame| reports::TimeFrame {
			mode: frame.mode.clone(),
			start: frame.start.clone(),
			end: frame.end.clone(),
		}),
		conditions: stored_filters(budget, &config.conditions, stored)?,
		conditions_or: config.conditions_or,
		mode: config.mode.clone(),
		summary: config.summary.clone(),
		content: config.content.clone(),
		text_align: config.text_align.clone(),
		formula: config.formula.clone(),
		report_id: config.report_id.clone(),
	})
}

fn day(date: NaiveDate) -> String {
	date.format("%Y-%m-%d").to_string()
}

fn range(range: reports::TimeRange) -> WidgetRange {
	WidgetRange {
		start: range.start,
		end: range.end,
		mode: range.mode,
	}
}

fn count(value: usize) -> u32 {
	u32::try_from(value).unwrap_or(u32::MAX)
}

struct Inputs<'a> {
	budget: &'a BudgetData,
	sheet: Option<&'a EnvelopeSheet>,
	today: CivilDate,
}

impl Inputs<'_> {
	fn sheet(&self, card: &str) -> Result<&EnvelopeSheet, CoreError> {
		self.sheet
			.ok_or_else(|| CoreError::Internal(format!("the {card} card needs the envelope sheet")))
	}
}

fn net_worth_view(card: reports::NetWorthCard) -> WidgetView {
	WidgetView::NetWorth {
		range: range(card.range),
		interval: match card.interval {
			reports::NetWorthInterval::Daily => NetWorthCardInterval::Daily,
			reports::NetWorthInterval::Weekly => NetWorthCardInterval::Weekly,
			reports::NetWorthInterval::Monthly => NetWorthCardInterval::Monthly,
			reports::NetWorthInterval::Yearly => NetWorthCardInterval::Yearly,
		},
		points: card
			.periods
			.into_iter()
			.map(|period| NetWorthCardPoint {
				start: day(period.start),
				net_worth: period.net_worth.minor(),
				assets: period.assets.minor(),
				debt: period.debt.minor(),
				change: period.change.minor(),
			})
			.collect(),
		net_worth: card.net_worth.minor(),
		change: card.total_change.minor(),
		lowest: card.lowest.map(Money::minor),
		highest: card.highest.map(Money::minor),
		has_negative: card.has_negative,
		accounts: card.accounts.iter().map(ToString::to_string).collect(),
		series: card
			.stacked
			.into_iter()
			.map(|stack| NetWorthCardSeries {
				account_id: stack.account.to_string(),
				name: stack.name,
				points: stack.balances.into_iter().map(Money::minor).collect(),
				color: stack.color,
			})
			.collect(),
	}
}

fn cash_flow_view(card: reports::CashFlowCard) -> Result<WidgetView, CoreError> {
	Ok(WidgetView::CashFlow {
		change: card.income.checked_add(card.expense)?.minor(),
		range: range(card.range),
		income: card.income.minor(),
		expense: card.expense.minor(),
	})
}

fn spending_view(card: reports::Spending) -> WidgetView {
	WidgetView::Spending {
		mode: match card.mode {
			reports::SpendingMode::SingleMonth => SpendingCardMode::SingleMonth,
			reports::SpendingMode::Budget => SpendingCardMode::Budget,
			reports::SpendingMode::Average => SpendingCardMode::Average,
		},
		compare: card.compare.into(),
		compare_to: card.compare_to.into(),
		average_first: card.average_months.map(|(first, _)| first.into()),
		average_last: card.average_months.map(|(_, last)| last.into()),
		days: card
			.days
			.into_iter()
			.map(|day| SpendingCardDay {
				day: day.day,
				compare: day.compare.map(Money::minor),
				compare_to: day.compare_to.map(Money::minor),
				average: day.average.minor(),
				budget: day.budget,
			})
			.collect(),
		headline_day: card.headline_day,
		difference: card.difference,
	}
}

fn sankey_label(label: reports::SankeyLabel) -> SankeyLabel {
	use reports::SankeyLabel as L;
	match label {
		L::Name(name) => SankeyLabel::Name { name },
		L::Budgeted => SankeyLabel::Budgeted,
		L::AvailableFunds => SankeyLabel::AvailableFunds,
		L::FromCategory(category) => SankeyLabel::FromCategory { category },
		L::ToBudget => SankeyLabel::ToBudget,
		L::Overbudgeted => SankeyLabel::Overbudgeted,
		L::AllAccounts => SankeyLabel::AllAccounts,
		L::FromMonth(month) => SankeyLabel::FromMonth {
			month: month.into(),
		},
		L::ForMonth(month) => SankeyLabel::ForMonth {
			month: month.into(),
		},
		L::Overspent => SankeyLabel::Overspent,
		L::Other => SankeyLabel::Other,
		L::Hidden => SankeyLabel::Hidden,
	}
}

fn sankey_color(color: reports::SankeyColor) -> SankeyCardColor {
	match color {
		reports::SankeyColor::Palette(index) => SankeyCardColor::Palette { index },
		reports::SankeyColor::ToBudgetPositive => SankeyCardColor::ToBudgetPositive,
		reports::SankeyColor::ToBudgetNegative => SankeyCardColor::ToBudgetNegative,
		reports::SankeyColor::Gray => SankeyCardColor::Gray,
		reports::SankeyColor::Blue => SankeyCardColor::Blue,
	}
}

fn sankey_view(card: reports::SankeyCard) -> WidgetView {
	WidgetView::Sankey {
		range: range(card.range),
		first: card.first_month.into(),
		last: card.last_month.into(),
		mode: match card.mode {
			reports::SankeyMode::Spent => SankeyCardMode::Categorized,
			reports::SankeyMode::Budgeted => SankeyCardMode::Budgeted,
		},
		show_percentages: card.show_percentages,
		nodes: card
			.nodes
			.into_iter()
			.map(|node| SankeyCardNode {
				key: node.key,
				label: sankey_label(node.label),
				percentage: node.percentage,
				color: sankey_color(node.color),
			})
			.collect(),
		links: card
			.links
			.into_iter()
			.map(|link| SankeyCardLink {
				source: count(link.source),
				target: link.target.map(count),
				value: link.value.minor(),
				folded: link
					.folded
					.into_iter()
					.map(|(name, value)| SankeyFolded {
						name,
						value: value.minor(),
					})
					.collect(),
				color: link.color.map(sankey_color),
			})
			.collect(),
	}
}

/// A budgeted Sankey over a month with no budget, which Actual's card
/// fails on, says so.
fn sankey_or_problem(
	result: Result<reports::SankeyCard, CoreError>,
) -> Result<WidgetView, CoreError> {
	match result {
		Ok(card) => Ok(sankey_view(card)),
		Err(CoreError::NotFound {
			entity: "budget month",
			id,
		}) => {
			let month = YearMonth::parse(&id)?.first_day().naive();
			Ok(WidgetView::Problem {
				message: format!("There's no budget for {}.", month.format("%B %Y")),
			})
		}
		Err(error) => Err(error),
	}
}

fn forecast_point(point: reports::ForecastChartPoint) -> ForecastCardPoint {
	ForecastCardPoint {
		date: point.date,
		balance: point.balance.minor(),
	}
}

fn balance_forecast_view(card: reports::BalanceForecastCard) -> WidgetView {
	WidgetView::BalanceForecast {
		range: range(card.range),
		source: match card.source {
			reports::ForecastSource::Schedules => ForecastCardSource::Schedules,
			reports::ForecastSource::TrackingBudget => ForecastCardSource::TrackingBudget,
		},
		granularity: match card.granularity {
			reports::ForecastGranularity::Daily => ForecastCardGranularity::Daily,
			reports::ForecastGranularity::Monthly => ForecastCardGranularity::Monthly,
		},
		zero_based: card.zero_based,
		has_forecast: card.has_forecast,
		points: card.points.into_iter().map(forecast_point).collect(),
		low: card.low.map(forecast_point),
		scheduled: count(card.scheduled),
		filtered: card.filtered,
		today: card.today.map(count),
	}
}

fn formula_error(error: reports::FormulaError) -> FormulaError {
	use reports::FormulaErrorKind as K;
	FormulaError {
		kind: match error.kind {
			K::NotAFormula => FormulaErrorKind::NotAFormula,
			K::Cycle => FormulaErrorKind::Cycle,
			K::DivByZero => FormulaErrorKind::DivByZero,
			K::Error => FormulaErrorKind::Error,
			K::Na => FormulaErrorKind::Na,
			K::Name => FormulaErrorKind::Name,
			K::Num => FormulaErrorKind::Num,
			K::Ref => FormulaErrorKind::Ref,
			K::Spill => FormulaErrorKind::Spill,
			K::Value => FormulaErrorKind::Value,
		},
		message: error.message,
	}
}

fn formula_view(card: reports::FormulaCard) -> WidgetView {
	WidgetView::Formula {
		formula: card.formula,
		value: match card.value {
			reports::FormulaValue::Number(value) => FormulaValue::Number { value },
			reports::FormulaValue::Text(text) => FormulaValue::Text { text },
			reports::FormulaValue::Boolean(value) => FormulaValue::Boolean { value },
			reports::FormulaValue::Empty => FormulaValue::Empty,
			reports::FormulaValue::Error(error) => FormulaValue::Error {
				error: formula_error(error),
			},
		},
		show_title: card.show_title,
		font_size: match card.font_size_mode {
			reports::FontSizeMode::Dynamic => FormulaFontSize::Dynamic,
			reports::FontSizeMode::Static => FormulaFontSize::Static,
		},
		static_font_size: card.static_font_size,
	}
}

fn monte_carlo_view(card: reports::MonteCarloCard) -> WidgetView {
	WidgetView::MonteCarlo {
		success_percent: card.success_percent,
		start_age: card.start_age,
		end_age: card.end_age,
		simulations: count(card.result.simulation_count),
		chart: card
			.chart
			.into_iter()
			.map(|point| MonteCarloCardPoint {
				year: count(point.year),
				age: point.age,
				p10: point.p10,
				p25: point.p25,
				p50: point.p50,
				p75: point.p75,
				p90: point.p90,
			})
			.collect(),
	}
}

fn card(inputs: &Inputs, kind: WidgetType, meta: Option<&Json>) -> Result<WidgetView, CoreError> {
	let budget = inputs.budget;
	let today = inputs.today.naive();
	Ok(match kind {
		WidgetType::NetWorth => net_worth_view(reports::net_worth_card(
			budget,
			meta,
			today,
			FIRST_DAY_OF_WEEK,
		)?),
		WidgetType::CashFlow => cash_flow_view(reports::cash_flow_card(budget, meta, today)?)?,
		WidgetType::Spending => spending_view(reports::spending(budget, meta, today)?),
		WidgetType::Sankey => sankey_or_problem(reports::sankey_card(
			budget,
			inputs.sheet("Sankey")?,
			meta,
			today,
			None,
		))?,
		WidgetType::BalanceForecast => balance_forecast_view(reports::balance_forecast_card(
			budget,
			inputs.sheet("balance forecast")?,
			meta,
			today,
		)?),
		WidgetType::Formula => formula_view(reports::formula_card(
			budget,
			inputs.sheet("formula")?,
			meta,
			today,
		)?),
		WidgetType::MonteCarlo => monte_carlo_view(reports::monte_carlo_card(budget, meta)?),
		WidgetType::Summary => {
			let data = reports::summary(budget, meta, today)?;
			WidgetView::Summary {
				start: day(data.start),
				end: day(data.end),
				value: data.total,
				dividend: data.dividend,
				divisor: data.divisor,
			}
		}
		WidgetType::Calendar => WidgetView::Calendar {
			months: reports::calendar(budget, meta, today, FIRST_DAY_OF_WEEK)?
				.into_iter()
				.map(|month| CalendarCardMonth {
					start: day(month.start),
					end: day(month.end),
					days: month
						.days
						.into_iter()
						.map(|cell| CalendarDayCell {
							date: day(cell.date),
							income: cell.income.minor(),
							expense: cell.expense.minor(),
							income_size: cell.income_size,
							expense_size: cell.expense_size,
						})
						.collect(),
					total_income: month.total_income.minor(),
					total_expense: month.total_expense.minor(),
				})
				.collect(),
		},
		WidgetType::AgeOfMoney => {
			let data = reports::age_of_money(budget, meta, today)?;
			WidgetView::AgeOfMoney {
				start: data.range.start,
				end: data.range.end,
				granularity: match data.granularity {
					reports::Granularity::Daily => AgeGranularity::Daily,
					reports::Granularity::Weekly => AgeGranularity::Weekly,
					reports::Granularity::Monthly => AgeGranularity::Monthly,
				},
				points: data
					.points
					.into_iter()
					.map(|point| AgePoint {
						period: day(point.period),
						days: point.age,
					})
					.collect(),
				current_days: data.current_age,
				trend: match data.trend {
					reports::Trend::Up => AgeTrend::Up,
					reports::Trend::Down => AgeTrend::Down,
					reports::Trend::Stable => AgeTrend::Stable,
				},
				insufficient_data: data.insufficient_data,
			}
		}
		WidgetType::BudgetAnalysis => {
			let sheet = inputs.sheet.ok_or_else(|| {
				CoreError::Internal("the budget analysis card needs the envelope sheet".to_owned())
			})?;
			let data = reports::budget_analysis(budget, sheet, meta, today)?;
			WidgetView::BudgetAnalysis {
				start: day(data.start),
				end: day(data.end),
				months: data
					.months
					.into_iter()
					.map(|month| BudgetAnalysisPoint {
						month: month.month.into(),
						budgeted: month.budgeted.minor(),
						spent: month.spent.minor(),
						balance: month.balance.minor(),
						overspending_adjustment: month.overspending_adjustment.minor(),
					})
					.collect(),
				total_budgeted: data.total_budgeted.minor(),
				total_spent: data.total_spent.minor(),
				total_overspending_adjustment: data.total_overspending_adjustment.minor(),
				final_overspending_adjustment: data.final_overspending_adjustment.minor(),
			}
		}
		WidgetType::Crossover => {
			let data = reports::crossover(budget, meta, today)?;
			WidgetView::Crossover {
				start: data.start.into(),
				end: data.end.into(),
				points: data
					.points
					.into_iter()
					.map(|point| CrossoverCardPoint {
						month: point.month.into(),
						investment_income: point.investment_income,
						expenses: point.expenses,
						nest_egg: point.nest_egg,
						adjusted_expenses: point.adjusted_expenses,
						projection: point.projection,
					})
					.collect(),
				crossover: data.crossover.map(Into::into),
				last_known_balance: data.last_known_balance,
				last_known_monthly_income: data.last_known_monthly_income,
				last_known_monthly_expenses: data.last_known_monthly_expenses,
				historical_return: data.historical_return,
				years_to_retire: data.years_to_retire,
				target_monthly_income: data.target_monthly_income,
				target_nest_egg: data.target_nest_egg,
			}
		}
		WidgetType::CustomReport | WidgetType::Markdown => {
			return Err(CoreError::Internal(format!(
				"the {} widget has no card",
				kind.id()
			)));
		}
	})
}

/// A card whose settings Actual can't draw either shows why, instead of
/// failing the whole dashboard.
fn problem(result: Result<WidgetView, CoreError>) -> Result<WidgetView, CoreError> {
	match result {
		Err(CoreError::Invalid(message)) => Ok(WidgetView::Problem { message }),
		other => other,
	}
}

fn stored_report(budget: &BudgetData, id: &CustomReportId) -> Option<CustomReport> {
	budget
		.custom_reports
		.get(id)
		.filter(|row| !row.tombstone)
		.and_then(|row| CustomReport::from_row(row).ok())
}

fn needs_sheet(budget: &BudgetData) -> bool {
	budget.widgets.values().any(|widget| {
		!widget.tombstone
			&& widget
				.kind
				.as_deref()
				.and_then(WidgetType::parse)
				.is_some_and(|kind| {
					matches!(
						kind,
						WidgetType::BudgetAnalysis
							| WidgetType::Sankey | WidgetType::BalanceForecast
							| WidgetType::Formula
					)
				})
	})
}

fn widget_view(
	inputs: &Inputs,
	widget: &reports::DashboardWidget,
	known: bool,
) -> Result<WidgetView, CoreError> {
	let budget = inputs.budget;
	Ok(match &widget.content {
		WidgetContent::Placeholder { kind } if !known => WidgetView::Unknown { kind: kind.clone() },
		WidgetContent::Placeholder { .. } => WidgetView::MissingReport,
		WidgetContent::Markdown { content, align } => WidgetView::Markdown {
			text: content.clone(),
			align: align.clone(),
		},
		WidgetContent::CustomReport(id) => match stored_report(budget, id) {
			Some(report) => WidgetView::CustomReport {
				report: view(budget, &report, inputs.today)?,
			},
			None => WidgetView::MissingReport,
		},
		WidgetContent::Card { kind, meta, .. } => problem(card(inputs, *kind, meta.as_ref()))?,
	})
}

/// Roughly how long a card takes on a large budget, so the slow ones start
/// first and the page ends with the quick ones.
fn cost(widget: &reports::DashboardWidget) -> u8 {
	match &widget.content {
		WidgetContent::Card { kind, .. } => match kind {
			WidgetType::Sankey | WidgetType::AgeOfMoney | WidgetType::NetWorth => 3,
			WidgetType::BalanceForecast | WidgetType::Crossover | WidgetType::MonteCarlo => 2,
			_ => 1,
		},
		WidgetContent::CustomReport(_) => 2,
		_ => 0,
	}
}

/// Every widget's view, in order. Cards read the budget only, so they run
/// on all cores: on perf-100k the page is several times faster than one
/// card after another (R-PERF-4).
fn widget_views(
	inputs: &Inputs,
	widgets: &[(&reports::DashboardWidget, bool)],
) -> Result<Vec<WidgetView>, CoreError> {
	let threads = std::thread::available_parallelism()
		.map_or(1, std::num::NonZero::get)
		.min(widgets.len());
	if threads <= 1 {
		return widgets
			.iter()
			.map(|(widget, known)| widget_view(inputs, widget, *known))
			.collect();
	}
	let mut order: Vec<usize> = (0..widgets.len()).collect();
	order.sort_by_key(|&index| std::cmp::Reverse(cost(widgets[index].0)));
	let next = AtomicUsize::new(0);
	let mut done: Vec<(usize, Result<WidgetView, CoreError>)> = std::thread::scope(|scope| {
		let workers: Vec<_> = (0..threads)
			.map(|_| {
				scope.spawn(|| {
					let mut views = Vec::new();
					loop {
						let Some(&index) = order.get(next.fetch_add(1, Ordering::Relaxed)) else {
							return views;
						};
						let (widget, known) = widgets[index];
						views.push((index, widget_view(inputs, widget, known)));
					}
				})
			})
			.collect();
		workers
			.into_iter()
			.flat_map(|worker| match worker.join() {
				Ok(views) => views,
				Err(panic) => std::panic::resume_unwind(panic),
			})
			.collect()
	});
	done.sort_by_key(|(index, _)| *index);
	done.into_iter().map(|(_, view)| view).collect()
}

fn dashboards(inputs: &Inputs) -> Result<Vec<DashboardPage>, CoreError> {
	let budget = inputs.budget;
	let pages = reports::dashboards(budget)?;
	let kinds: Vec<(String, Option<WidgetType>)> = pages
		.iter()
		.flat_map(|page| &page.widgets)
		.map(|widget| {
			let kind = budget
				.widgets
				.get(&widget.id)
				.and_then(|stored| stored.kind.clone())
				.unwrap_or_default();
			let known = WidgetType::parse(&kind);
			(kind, known)
		})
		.collect();
	let all: Vec<(&reports::DashboardWidget, bool)> = pages
		.iter()
		.flat_map(|page| &page.widgets)
		.zip(&kinds)
		.map(|(widget, (_, known))| (widget, known.is_some()))
		.collect();
	let mut views = widget_views(inputs, &all)?.into_iter();
	let mut kinds = kinds.into_iter();
	let mut shown = Vec::new();
	for page in &pages {
		let mut widgets = Vec::new();
		for widget in &page.widgets {
			let (kind, known) = kinds.next().unwrap_or_default();
			let view = views
				.next()
				.ok_or_else(|| CoreError::Internal("a widget's view went missing".to_owned()))?;
			let (min_width, min_height) = known.map_or((1, 1), WidgetType::minimum_size);
			let stored = budget.widgets.get(&widget.id);
			let meta = reports::parse_meta(stored.and_then(|stored| stored.meta.as_deref()))?;
			widgets.push(DashboardWidget {
				id: widget.id.to_string(),
				kind,
				x: widget.x,
				y: widget.y,
				width: widget.width,
				height: widget.height,
				min_width,
				min_height,
				config: config(budget, reports::read_settings(known, meta.as_ref())),
				view,
			});
		}
		shown.push(DashboardPage {
			id: page.id.to_string(),
			name: page.name.clone(),
			widgets,
		});
	}
	Ok(shown)
}

fn stored_widget_conditions(budget: &BudgetData, widget_id: &str) -> Result<Vec<Json>, CoreError> {
	let widget = reports::live_widget(budget, widget_id)?;
	let kind = widget.kind.as_deref().and_then(WidgetType::parse);
	let meta = reports::parse_meta(widget.meta.as_deref())?;
	Ok(reports::read_settings(kind, meta.as_ref()).conditions)
}

pub(crate) fn add_widget(
	budget: &BudgetData,
	page_id: &str,
	kind: &str,
	frame: (i64, i64, i64, i64),
	config: &WidgetConfig,
) -> Result<Vec<Row>, CoreError> {
	let row = reports::add_widget(
		budget,
		WidgetId::new(veritas_store::new_id()),
		page_id,
		kind,
		frame,
		&settings(budget, config, &[])?,
	)?;
	Ok(vec![row])
}

pub(crate) fn set_widget_config(
	budget: &BudgetData,
	widget_id: &str,
	config: &WidgetConfig,
) -> Result<Vec<Row>, CoreError> {
	let stored = stored_widget_conditions(budget, widget_id)?;
	reports::set_widget_settings(budget, widget_id, &settings(budget, config, &stored)?)
}

pub(crate) fn place_widget(
	budget: &BudgetData,
	widget_id: &str,
	(x, y, width, height): (i64, i64, i64, i64),
) -> Result<Vec<Row>, CoreError> {
	let widget = reports::live_widget(budget, widget_id)?;
	Ok(vec![Row::Widget(reports::place_widget(
		widget, x, y, width, height,
	)?)])
}

pub(crate) fn create_dashboard(budget: &BudgetData, name: &str) -> Result<Vec<Row>, CoreError> {
	Ok(vec![reports::create_page(
		budget,
		DashboardPageId::new(veritas_store::new_id()),
		name,
	)?])
}

#[uniffi::export]
impl Session {
	/// Every dashboard page with each widget's settings and numbers as of
	/// `today`.
	pub async fn dashboards(
		&self,
		today: String,
		cancel: Arc<CancelHandle>,
	) -> Result<Vec<DashboardPage>, FfiError> {
		let core = self.core.clone();
		run(&cancel, move |cancel| {
			if cancel.is_cancelled() {
				return Err(FfiError::Cancelled);
			}
			let today = civil_date(&today)?;
			core.call(|session| {
				let sheet = needs_sheet(session.data()?);
				session.with_sheet_if(today, sheet, |budget, sheet| {
					dashboards(&Inputs {
						budget,
						sheet,
						today,
					})
				})
			})
		})
		.await
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::Command;
	use crate::rules::{RuleField, RuleValue};
	use crate::session::Core;
	use crate::test_support::{imported, reopened};

	const NET_WORTH_CARD: &str = "a8828813-ceda-452a-8de4-4a24a78c531e";

	fn today() -> CivilDate {
		CivilDate::new(2026, 11, 13).unwrap()
	}

	fn data(core: &Core<veritas_store::Store>) -> BudgetData {
		core.call(|session| session.data().cloned()).unwrap()
	}

	fn pages(core: &Core<veritas_store::Store>) -> Vec<DashboardPage> {
		core.call(|session| {
			let sheet = needs_sheet(session.data()?);
			session.with_sheet_if(today(), sheet, |budget, sheet| {
				dashboards(&Inputs {
					budget,
					sheet,
					today: today(),
				})
			})
		})
		.unwrap()
	}

	fn widget<'a>(pages: &'a [DashboardPage], id: &str) -> &'a DashboardWidget {
		pages
			.iter()
			.flat_map(|page| &page.widgets)
			.find(|widget| widget.id == id)
			.unwrap()
	}

	fn created(change: &crate::Change, kind: crate::EntityKind) -> String {
		change
			.entities
			.iter()
			.find(|entity| entity.kind == kind)
			.and_then(|entity| entity.id.clone())
			.unwrap()
	}

	fn blank() -> WidgetConfig {
		WidgetConfig {
			name: None,
			time_frame: None,
			conditions: Vec::new(),
			conditions_or: false,
			mode: None,
			summary: None,
			content: None,
			text_align: None,
			formula: None,
			report_id: None,
		}
	}

	#[test]
	fn every_widget_draws_natively_with_its_settings() {
		let (_scratch, core) = imported("all-features");
		let pages = pages(&core);
		let names: Vec<&str> = pages.iter().map(|page| page.name.as_str()).collect();
		assert_eq!(names, ["Main", "Deep dive"]);

		let net_worth = widget(&pages, NET_WORTH_CARD);
		let WidgetView::NetWorth {
			points,
			net_worth: total,
			change,
			series,
			..
		} = &net_worth.view
		else {
			panic!("the net worth card should draw natively");
		};
		assert_eq!(points.len(), 7, "six months and the month before them");
		assert_eq!(
			*change,
			points.last().unwrap().net_worth - points[0].net_worth
		);
		assert_eq!(*total, points.last().unwrap().net_worth);
		for (index, point) in points.iter().enumerate() {
			let stacked: i64 = series.iter().map(|stack| stack.points[index]).sum();
			assert_eq!(
				stacked, point.net_worth,
				"the stack adds up to the net worth"
			);
			assert_eq!(point.assets - point.debt, point.net_worth);
		}
		assert_eq!(net_worth.min_width, 3);

		let view = |id: &str| widget(&pages, id).view.clone();
		assert!(matches!(
			view("07fb54b8-dd18-40de-8bca-381826688a49"),
			WidgetView::Spending { .. }
		));
		assert_eq!(
			view("b595baa8-f97d-4f1a-ac8f-d7c2a1d7a0b3"),
			WidgetView::Unknown {
				kind: widget(&pages, "b595baa8-f97d-4f1a-ac8f-d7c2a1d7a0b3")
					.kind
					.clone()
			}
		);
		let filtered = widget(&pages, "5658fbe9-b6d0-428e-a4ff-77b3eb54803b");
		assert!(!filtered.config.conditions.is_empty());
		assert!(matches!(
			view("8bb52bc7-1c5b-4af7-8236-aa36926a85da"),
			WidgetView::Sankey { .. }
		));
		let markdown = widget(&pages, "a7456f7b-ad43-448c-a280-7cea8714f699");
		assert!(
			matches!(&markdown.view, WidgetView::Markdown { text, .. } if text.contains("**in full**"))
		);
		assert!(
			markdown
				.config
				.content
				.as_deref()
				.unwrap()
				.contains("**in full**")
		);
		let report = widget(&pages, "8de603a2-58a0-411b-8e1c-cca26d049f0c");
		assert!(report.config.report_id.is_some());
		assert!(
			matches!(&report.view, WidgetView::CustomReport { report } if report.data.is_some())
		);
		for page in &pages {
			for widget in &page.widgets {
				assert!(
					!matches!(widget.view, WidgetView::Problem { .. }),
					"{} {:?}",
					widget.kind,
					widget.view
				);
			}
		}
	}

	/// R-PERF-4 for the whole dashboard query. Run with
	/// `cargo test --release -p veritas-ffi -- --ignored --nocapture perf_100k`.
	#[test]
	#[ignore = "a timing, meaningful only in release"]
	fn the_perf_100k_dashboard_meets_r_perf_4() {
		let (_scratch, core) = imported("perf-100k");
		let started = std::time::Instant::now();
		let cold = pages(&core);
		let cold_time = started.elapsed();
		let started = std::time::Instant::now();
		pages(&core);
		let warm_time = started.elapsed();
		let widgets: usize = cold.iter().map(|page| page.widgets.len()).sum();
		eprintln!("perf-100k dashboard, {widgets} widgets: cold {cold_time:?}, warm {warm_time:?}");
		assert!(warm_time.as_millis() <= 100, "warm {warm_time:?}");
	}

	fn add(core: &Core<veritas_store::Store>, kind: &str, config: WidgetConfig) -> WidgetView {
		let page = pages(core)[0].id.clone();
		let change = core
			.perform(Command::AddWidget {
				page_id: page,
				kind: kind.to_owned(),
				x: 0,
				y: 50,
				width: 4,
				height: 3,
				config,
			})
			.unwrap();
		let id = created(&change, crate::EntityKind::Widget);
		widget(&pages(core), &id).view.clone()
	}

	#[test]
	fn the_formula_balance_forecast_and_monte_carlo_cards_draw_natively() {
		let (_scratch, core) = imported("all-features");
		let formula = add(
			&core,
			"formula-card",
			WidgetConfig {
				formula: Some("=1/0".to_owned()),
				..blank()
			},
		);
		let WidgetView::Formula {
			value: FormulaValue::Error { error },
			..
		} = formula
		else {
			panic!("{formula:?}");
		};
		assert_eq!(error.kind, FormulaErrorKind::DivByZero);
		assert_eq!(error.message, "Formula error: DIV_BY_ZERO");
		assert!(matches!(
			add(&core, "formula-card", blank()),
			WidgetView::Formula {
				value: FormulaValue::Number { value },
				..
			} if (value - 6.0).abs() < f64::EPSILON
		));
		assert!(matches!(
			add(&core, "balance-forecast-card", blank()),
			WidgetView::BalanceForecast {
				has_forecast: true,
				..
			}
		));
		let WidgetView::MonteCarlo {
			success_percent,
			start_age,
			end_age,
			chart,
			..
		} = add(&core, "monte-carlo-card", blank())
		else {
			panic!("the Monte Carlo card should draw natively");
		};
		assert!((0.0..=100.0).contains(&success_percent));
		assert!(end_age > start_age);
		assert!(chart.iter().all(|point| point.p10 <= point.p90));
	}

	#[test]
	fn a_budgeted_sankey_outside_the_budget_says_there_is_no_budget() {
		let (_scratch, core) = imported("all-features");
		let account = data(&core)
			.accounts
			.values()
			.find(|account| !account.tombstone && !account.off_budget)
			.unwrap()
			.id
			.to_string();
		core.perform(Command::AddTransaction {
			transaction: crate::TransactionDraft {
				id: None,
				account_id: account,
				date: "2030-06-01".to_owned(),
				amount: -100,
				payee_id: None,
				payee_name: None,
				category_id: None,
				notes: None,
				cleared: false,
				splits: Vec::new(),
			},
		})
		.unwrap();
		let view = add(
			&core,
			"sankey-card",
			WidgetConfig {
				mode: Some("budgeted".to_owned()),
				time_frame: Some(WidgetTimeFrame {
					mode: "static".to_owned(),
					start: "2030-01".to_owned(),
					end: "2030-01".to_owned(),
				}),
				..blank()
			},
		);
		assert_eq!(
			view,
			WidgetView::Problem {
				message: "There's no budget for January 2030.".to_owned()
			}
		);
	}

	#[test]
	fn typed_filters_save_as_actuals_filter_editor_saves_them() {
		let (_scratch, core) = imported("all-features");
		let page = pages(&core)[0].id.clone();
		let conditions = vec![
			RuleCondition {
				field: RuleField::AmountOutflow,
				op: crate::RuleConditionOp::Gte,
				value: RuleValue::Amount { amount: 2000 },
			},
			RuleCondition {
				field: RuleField::Date,
				op: crate::RuleConditionOp::Is,
				value: RuleValue::Month {
					month: "2026-03".to_owned(),
				},
			},
			RuleCondition {
				field: RuleField::Payee,
				op: crate::RuleConditionOp::IsNot,
				value: RuleValue::Id { id: None },
			},
		];
		let change = core
			.perform(Command::AddWidget {
				page_id: page,
				kind: "summary-card".to_owned(),
				x: 0,
				y: 50,
				width: 4,
				height: 2,
				config: WidgetConfig {
					conditions: conditions.clone(),
					..blank()
				},
			})
			.unwrap();
		let id = created(&change, crate::EntityKind::Widget);
		assert_eq!(
			data(&core).widgets[&WidgetId::new(id.as_str())]
				.meta
				.as_deref(),
			Some(
				r#"{"conditions":[{"field":"amount","op":"gte","value":2000,"options":{"outflow":true},"type":"number"},{"field":"date","op":"is","value":"2026-03","options":{"month":true},"type":"date"},{"field":"payee","op":"isNot","value":null,"type":"id"}]}"#
			)
		);
		assert_eq!(widget(&pages(&core), &id).config.conditions, conditions);

		let mut raw = widget(&pages(&core), &id).config.clone();
		raw.conditions[0].value = RuleValue::Raw {
			json: "2000".to_owned(),
		};
		assert!(matches!(
			core.perform(Command::SetWidgetConfig {
				widget_id: id,
				config: raw,
			}),
			Err(FfiError::Invalid { .. })
		));
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

	#[test]
	fn a_new_widget_lands_where_asked_and_undo_takes_it_away() {
		let (_scratch, core) = imported("all-features");
		let page = pages(&core)[0].id.clone();
		let change = core
			.perform(Command::AddWidget {
				page_id: page.clone(),
				kind: "summary-card".to_owned(),
				x: 0,
				y: 40,
				width: 4,
				height: 2,
				config: WidgetConfig {
					name: Some("Spent".to_owned()),
					summary: Some("avgPerMonth".to_owned()),
					..blank()
				},
			})
			.unwrap();
		let id = created(&change, crate::EntityKind::Widget);
		let added = widget(&pages(&core), &id).clone();
		assert_eq!((added.x, added.y, added.width, added.height), (0, 40, 4, 2));
		assert!(matches!(added.view, WidgetView::Summary { .. }));
		assert_eq!(added.config.summary.as_deref(), Some("avgPerMonth"));
		assert_eq!(
			data(&core).widgets[&WidgetId::new(id.as_str())]
				.meta
				.as_deref(),
			Some(r#"{"name":"Spent","content":"{\"type\":\"avgPerMonth\"}"}"#)
		);
		core.undo().unwrap();
		assert!(
			pages(&core)
				.iter()
				.flat_map(|page| &page.widgets)
				.all(|widget| widget.id != id)
		);
		assert!(matches!(
			core.perform(Command::AddWidget {
				page_id: page,
				kind: "space-card".to_owned(),
				x: 0,
				y: 0,
				width: 4,
				height: 2,
				config: blank(),
			}),
			Err(FfiError::Invalid { .. })
		));
	}

	#[test]
	fn changing_settings_keeps_the_keys_veritas_does_not_show() {
		let (_scratch, core) = imported("all-features");
		let id = "5658fbe9-b6d0-428e-a4ff-77b3eb54803b";
		let before = data(&core).widgets[&WidgetId::new(id)]
			.meta
			.clone()
			.unwrap();
		let mut config = widget(&pages(&core), id).config.clone();
		let change = core
			.perform(Command::SetWidgetConfig {
				widget_id: id.to_owned(),
				config: config.clone(),
			})
			.unwrap();
		assert!(
			change.entities.is_empty(),
			"unchanged settings write nothing"
		);
		config.name = Some("Renamed".to_owned());
		core.perform(Command::SetWidgetConfig {
			widget_id: id.to_owned(),
			config,
		})
		.unwrap();
		let after = data(&core).widgets[&WidgetId::new(id)]
			.meta
			.clone()
			.unwrap();
		let unnamed = |text: &str| {
			let Json::Object(entries) = Json::parse(text).unwrap() else {
				panic!("meta is an object");
			};
			entries
				.into_iter()
				.filter(|(key, _)| key != "name")
				.collect::<Vec<_>>()
		};
		assert_eq!(unnamed(&after), unnamed(&before));
		assert_eq!(
			widget(&pages(&core), id).config.name.as_deref(),
			Some("Renamed")
		);
	}

	#[test]
	fn dashboards_are_created_renamed_and_deleted_with_their_widgets() {
		let (scratch, core) = imported("all-features");
		let change = core
			.perform(Command::CreateDashboard {
				name: " Planning ".to_owned(),
			})
			.unwrap();
		let page = created(&change, crate::EntityKind::DashboardPage);
		let widget_change = core
			.perform(Command::AddWidget {
				page_id: page.clone(),
				kind: "markdown-card".to_owned(),
				x: 0,
				y: 0,
				width: 4,
				height: 2,
				config: blank(),
			})
			.unwrap();
		let note = created(&widget_change, crate::EntityKind::Widget);
		core.perform(Command::RenameDashboard {
			page_id: page.clone(),
			name: "Plans".to_owned(),
		})
		.unwrap();
		let listed = pages(&core);
		let plans = listed.iter().find(|p| p.id == page).unwrap();
		assert_eq!(plans.name, "Plans");
		assert!(
			matches!(&plans.widgets[0].view, WidgetView::Markdown { text, .. } if text == reports::DEFAULT_MARKDOWN)
		);
		core.perform(Command::DeleteDashboard {
			page_id: page.clone(),
		})
		.unwrap();
		drop(core);

		let core = reopened(&scratch);
		let budget = data(&core);
		assert!(budget.dashboard_pages[&DashboardPageId::new(page.as_str())].tombstone);
		assert!(budget.widgets[&WidgetId::new(note.as_str())].tombstone);
		assert!(pages(&core).iter().all(|p| p.id != page));

		let only: Vec<String> = pages(&core).iter().map(|p| p.id.clone()).collect();
		core.perform(Command::DeleteDashboard {
			page_id: only[1].clone(),
		})
		.unwrap();
		assert!(matches!(
			core.perform(Command::DeleteDashboard {
				page_id: only[0].clone(),
			}),
			Err(FfiError::Invalid { .. })
		));
	}

	#[test]
	fn removing_a_widget_tombstones_it() {
		let (_scratch, core) = imported("all-features");
		core.perform(Command::RemoveWidget {
			widget_id: NET_WORTH_CARD.to_owned(),
		})
		.unwrap();
		assert!(data(&core).widgets[&WidgetId::new(NET_WORTH_CARD)].tombstone);
		core.undo().unwrap();
		assert!(!data(&core).widgets[&WidgetId::new(NET_WORTH_CARD)].tombstone);
	}
}
