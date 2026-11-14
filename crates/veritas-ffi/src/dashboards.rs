//! Actual's dashboards: every widget drawn natively with its settings (D73),
//! and the edits Actual's dashboard handlers make, written in its format.

use std::sync::Arc;

use chrono::NaiveDate;
use veritas_domain::{
	BudgetData, CivilDate, CoreError, CustomReportId, DashboardPageId, Row, WidgetId, YearMonth,
};
use veritas_engine::EnvelopeSheet;
use veritas_reports::{
	self as reports, Classification, CustomReport, DefaultFrame, Json, Period, WidgetContent,
	WidgetSettings, WidgetType,
};

use crate::blocking::{CancelToken as CancelHandle, run};
use crate::budget::civil_date;
use crate::custom_reports::{
	CustomReportView, FIRST_DAY_OF_WEEK, ReportCondition, report_condition, stored_conditions, view,
};
use crate::reports::{NetWorthMonth, SankeyReport, net_worth_months, sankey_report};
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
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct WidgetConfig {
	pub name: Option<String>,
	pub time_frame: Option<WidgetTimeFrame>,
	pub conditions: Vec<ReportCondition>,
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
	/// Negative, as Actual sums it.
	pub expense: i64,
	/// Each bar's height against the month's largest day, 0 to 1.
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

/// What a widget draws. Money is in stored minor units; the `f64` figures
/// are Actual's own unrounded values, rounded only for display.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
#[expect(
	clippy::large_enum_variant,
	reason = "UniFFI can't box a record, and a dashboard holds only a few dozen widgets"
)]
pub enum WidgetView {
	NetWorth {
		points: Vec<NetWorthMonth>,
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
	/// A known widget, or a setting of one, Veritas doesn't compute yet.
	Pending,
	/// The custom report it shows was deleted.
	MissingReport,
	/// The card's settings are ones Actual's own card can't draw either.
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

fn config(settings: WidgetSettings) -> WidgetConfig {
	WidgetConfig {
		name: settings.name,
		time_frame: settings.time_frame.map(|frame| WidgetTimeFrame {
			mode: frame.mode,
			start: frame.start,
			end: frame.end,
		}),
		conditions: settings.conditions.iter().map(report_condition).collect(),
		conditions_or: settings.conditions_or,
		mode: settings.mode,
		summary: settings.summary,
		content: settings.content,
		text_align: settings.text_align,
		formula: settings.formula,
		report_id: settings.report_id,
	}
}

fn settings(config: &WidgetConfig, stored: &[Json]) -> Result<WidgetSettings, CoreError> {
	Ok(WidgetSettings {
		name: config.name.clone(),
		time_frame: config.time_frame.as_ref().map(|frame| reports::TimeFrame {
			mode: frame.mode.clone(),
			start: frame.start.clone(),
			end: frame.end.clone(),
		}),
		conditions: stored_conditions(&config.conditions, stored)?,
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

struct Inputs<'a> {
	budget: &'a BudgetData,
	classification: &'a Classification,
	sheet: Option<&'a EnvelopeSheet>,
	today: CivilDate,
}

/// The net worth, cash flow, spending and Sankey cards, drawn from
/// Veritas's reports. Their filters and their other modes aren't ported yet.
fn veritas_card(
	inputs: &Inputs,
	kind: WidgetType,
	meta: Option<&Json>,
) -> Result<WidgetView, CoreError> {
	let Inputs {
		budget,
		classification,
		today,
		..
	} = *inputs;
	if has_conditions(meta) {
		return Ok(WidgetView::Pending);
	}
	let latest = classification.rows().last().map(|row| row.date);
	let months = |default| reports::widget_months(meta, default, today, latest);
	Ok(match kind {
		WidgetType::NetWorth => {
			let (first, last) = months(DefaultFrame::SixMonths)?;
			WidgetView::NetWorth {
				points: net_worth_months(budget, classification, first, last)?,
			}
		}
		WidgetType::CashFlow => {
			let (first, last) = months(DefaultFrame::ThisMonth)?;
			let period = Period::new(first.first_day(), Period::month(last).last())?;
			let report = reports::net_profit(budget, classification, period)?;
			WidgetView::CashFlow {
				first: first.into(),
				last: last.into(),
				income: report.income.total.minor(),
				expenses: report.expenses.total.minor(),
				net_profit: report.net_profit.total.minor(),
			}
		}
		WidgetType::Spending => match reports::spending_months(meta, today)? {
			Some((month, compare)) => WidgetView::Spending {
				month: month.into(),
				spent: expenses(budget, classification, month)?,
				compare_month: compare.into(),
				compare_spent: expenses(budget, classification, compare)?,
			},
			None => WidgetView::Pending,
		},
		WidgetType::Sankey => {
			let budgeted = meta
				.and_then(|meta| meta.get("mode"))
				.and_then(Json::as_str)
				.is_some_and(|mode| mode != "spent");
			if budgeted {
				return Ok(WidgetView::Pending);
			}
			let (first, last) = months(DefaultFrame::SixMonths)?;
			let period = Period::new(first.first_day(), Period::month(last).last())?;
			WidgetView::Sankey {
				report: sankey_report(budget, &reports::sankey(budget, classification, period)?),
			}
		}
		_ => WidgetView::Pending,
	})
}

fn card(inputs: &Inputs, kind: WidgetType, meta: Option<&Json>) -> Result<WidgetView, CoreError> {
	let budget = inputs.budget;
	let today = inputs.today.naive();
	Ok(match kind {
		WidgetType::NetWorth | WidgetType::CashFlow | WidgetType::Spending | WidgetType::Sankey => {
			veritas_card(inputs, kind, meta)?
		}
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
		WidgetType::Formula
		| WidgetType::BalanceForecast
		| WidgetType::MonteCarlo
		| WidgetType::CustomReport
		| WidgetType::Markdown => WidgetView::Pending,
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
		!widget.tombstone && widget.kind.as_deref() == Some(WidgetType::BudgetAnalysis.id())
	})
}

fn dashboards(inputs: &Inputs) -> Result<Vec<DashboardPage>, CoreError> {
	let budget = inputs.budget;
	let mut pages = Vec::new();
	for page in reports::dashboards(budget)? {
		let mut widgets = Vec::new();
		for widget in &page.widgets {
			let stored = budget.widgets.get(&widget.id);
			let kind = stored
				.and_then(|stored| stored.kind.clone())
				.unwrap_or_default();
			let known = WidgetType::parse(&kind);
			let (min_width, min_height) = known.map_or((1, 1), WidgetType::minimum_size);
			let meta = reports::parse_meta(stored.and_then(|stored| stored.meta.as_deref()))?;
			let view = match &widget.content {
				WidgetContent::Placeholder { kind } if known.is_none() => {
					WidgetView::Unknown { kind: kind.clone() }
				}
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
				WidgetContent::Card { kind, meta, .. } => {
					problem(card(inputs, *kind, meta.as_ref()))?
				}
			};
			widgets.push(DashboardWidget {
				id: widget.id.to_string(),
				kind,
				x: widget.x,
				y: widget.y,
				width: widget.width,
				height: widget.height,
				min_width,
				min_height,
				config: config(reports::read_settings(known, meta.as_ref())),
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
		&settings(config, &[])?,
	)?;
	Ok(vec![row])
}

pub(crate) fn set_widget_config(
	budget: &BudgetData,
	widget_id: &str,
	config: &WidgetConfig,
) -> Result<Vec<Row>, CoreError> {
	let stored = stored_widget_conditions(budget, widget_id)?;
	reports::set_widget_settings(budget, widget_id, &settings(config, &stored)?)
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

pub(crate) fn create_dashboard(name: &str) -> Result<Vec<Row>, CoreError> {
	Ok(vec![reports::create_page(
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
				session.with_reports(today, sheet, |budget, classification, sheet| {
					dashboards(&Inputs {
						budget,
						classification,
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
			session.with_reports(today(), sheet, |budget, classification, sheet| {
				dashboards(&Inputs {
					budget,
					classification,
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
		let WidgetView::NetWorth { points } = &net_worth.view else {
			panic!("the net worth card should draw natively");
		};
		assert_eq!(points.len(), 6);
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
