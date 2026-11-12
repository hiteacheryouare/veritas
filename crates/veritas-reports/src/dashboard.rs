//! Actual's dashboards as data: pages of widgets on a 12-column grid, each
//! known widget type with its settings, and every other type kept as an
//! "Open in Actual" placeholder that is written back unchanged.

use veritas_domain::{BudgetData, CoreError, CustomReportId, DashboardPageId, Widget, WidgetId};

use crate::json::Json;

/// Widget types Actual 26.11 knows (`lc/server/dashboard/app.ts:31-50`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidgetType {
	NetWorth,
	CashFlow,
	Spending,
	BudgetAnalysis,
	CustomReport,
	Crossover,
	Markdown,
	Summary,
	Calendar,
	Formula,
	Sankey,
	BalanceForecast,
	AgeOfMoney,
	MonteCarlo,
}

const TYPES: [(&str, WidgetType); 14] = [
	("net-worth-card", WidgetType::NetWorth),
	("cash-flow-card", WidgetType::CashFlow),
	("spending-card", WidgetType::Spending),
	("budget-analysis-card", WidgetType::BudgetAnalysis),
	("custom-report", WidgetType::CustomReport),
	("crossover-card", WidgetType::Crossover),
	("markdown-card", WidgetType::Markdown),
	("summary-card", WidgetType::Summary),
	("calendar-card", WidgetType::Calendar),
	("formula-card", WidgetType::Formula),
	("sankey-card", WidgetType::Sankey),
	("balance-forecast-card", WidgetType::BalanceForecast),
	("age-of-money-card", WidgetType::AgeOfMoney),
	("monte-carlo-card", WidgetType::MonteCarlo),
];

impl WidgetType {
	pub fn parse(kind: &str) -> Option<WidgetType> {
		TYPES
			.iter()
			.find(|(name, _)| *name == kind)
			.map(|(_, kind)| *kind)
	}

	pub fn id(self) -> &'static str {
		TYPES
			.iter()
			.find(|(_, kind)| *kind == self)
			.map_or("", |(name, _)| name)
	}

	/// The smallest width and height the grid allows (`Overview.tsx:73-98`).
	pub fn minimum_size(self) -> (i64, i64) {
		let width = match self {
			WidgetType::Formula => 1,
			WidgetType::CustomReport
			| WidgetType::Markdown
			| WidgetType::CashFlow
			| WidgetType::Summary => 2,
			_ => 3,
		};
		let height = match self {
			WidgetType::CustomReport | WidgetType::Markdown | WidgetType::Formula => 1,
			WidgetType::Sankey => 3,
			_ => 2,
		};
		(width, height)
	}
}

#[derive(Clone, Debug, PartialEq)]
pub enum WidgetContent {
	/// A saved custom report, evaluated by `evaluate_report`.
	CustomReport(CustomReportId),
	Markdown {
		content: String,
		align: Option<String>,
	},
	/// Any other known type, with its settings (`meta`) as stored. Its
	/// numbers come from the matching Veritas report.
	Card {
		kind: WidgetType,
		name: Option<String>,
		meta: Option<Json>,
	},
	/// A type Veritas doesn't know: shown as "Open in Actual", never edited.
	Placeholder { kind: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct DashboardWidget {
	pub id: WidgetId,
	pub x: i64,
	pub y: i64,
	pub width: i64,
	pub height: i64,
	pub content: WidgetContent,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Dashboard {
	pub id: DashboardPageId,
	pub name: String,
	/// By row, then column, as the grid lays them out.
	pub widgets: Vec<DashboardWidget>,
}

pub const GRID_COLUMNS: i64 = 12;

/// Every live dashboard page with its live widgets. Rows Actual treats as
/// deleted (tombstoned, or with no type) are left out.
pub fn dashboards(budget: &BudgetData) -> Result<Vec<Dashboard>, CoreError> {
	let mut pages: Vec<Dashboard> = budget
		.dashboard_pages
		.values()
		.filter(|page| !page.tombstone)
		.map(|page| Dashboard {
			id: page.id.clone(),
			name: page.name.clone().unwrap_or_default(),
			widgets: Vec::new(),
		})
		.collect();
	for widget in budget.widgets.values().filter(|widget| !widget.tombstone) {
		let Some(content) = content(widget)? else {
			continue;
		};
		let Some(page) = pages
			.iter_mut()
			.find(|page| widget.page.as_ref() == Some(&page.id))
		else {
			continue;
		};
		page.widgets.push(DashboardWidget {
			id: widget.id.clone(),
			x: widget.x.unwrap_or_default(),
			y: widget.y.unwrap_or_default(),
			width: widget.width.unwrap_or_default(),
			height: widget.height.unwrap_or_default(),
			content,
		});
	}
	for page in &mut pages {
		page.widgets
			.sort_by(|a, b| (a.y, a.x, &a.id).cmp(&(b.y, b.x, &b.id)));
	}
	Ok(pages)
}

fn content(widget: &Widget) -> Result<Option<WidgetContent>, CoreError> {
	let Some(kind) = widget.kind.as_deref() else {
		return Ok(None);
	};
	let meta = match widget.meta.as_deref() {
		None | Some("null") => None,
		Some(text) => Some(Json::parse(text).map_err(|reason| {
			CoreError::Invalid(format!("dashboard widget {}: {reason}", widget.id))
		})?),
	};
	let text = |key: &str| {
		meta.as_ref()
			.and_then(|meta| meta.get(key))
			.and_then(Json::as_str)
			.map(str::to_owned)
	};
	Ok(Some(match WidgetType::parse(kind) {
		None => WidgetContent::Placeholder {
			kind: kind.to_owned(),
		},
		Some(WidgetType::CustomReport) => match text("id") {
			Some(id) => WidgetContent::CustomReport(CustomReportId::new(id)),
			None => WidgetContent::Placeholder {
				kind: kind.to_owned(),
			},
		},
		Some(WidgetType::Markdown) => WidgetContent::Markdown {
			content: text("content").unwrap_or_default(),
			align: text("text_align"),
		},
		Some(kind) => WidgetContent::Card {
			kind,
			name: text("name"),
			meta: meta.clone(),
		},
	}))
}

/// Moves or resizes a widget the way Actual's grid does in edit mode: only
/// `x`, `y`, `width` and `height` change, clamped to the grid and the
/// type's minimum size. A placeholder can move too; its type and settings
/// stay untouched.
pub fn place_widget(
	widget: &Widget,
	x: i64,
	y: i64,
	width: i64,
	height: i64,
) -> Result<Widget, CoreError> {
	let (min_width, min_height) = widget
		.kind
		.as_deref()
		.and_then(WidgetType::parse)
		.map_or((1, 1), WidgetType::minimum_size);
	let width = width.clamp(min_width, GRID_COLUMNS);
	let height = height.max(min_height);
	if y < 0 || x < 0 {
		return Err(CoreError::Invalid(
			"a widget can't sit above or left of the grid".to_owned(),
		));
	}
	Ok(Widget {
		x: Some(x.min(GRID_COLUMNS - width)),
		y: Some(y),
		width: Some(width),
		height: Some(height),
		..widget.clone()
	})
}
