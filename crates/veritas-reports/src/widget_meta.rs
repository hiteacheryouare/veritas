//! A widget's settings as Actual keeps them in `dashboard.meta`, and the
//! rows Actual's dashboard handlers write (`lc/server/dashboard/app.ts`).
//! Settings are written back key by key, so keys Veritas doesn't know keep
//! their text.

use veritas_domain::{
	BudgetData, CoreError, DashboardPage, DashboardPageId, Row, Widget, WidgetId,
};

use crate::dashboard::{GRID_COLUMNS, WidgetType};
use crate::filter::TransactionFilter;
use crate::json::Json;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimeFrame {
	pub mode: String,
	pub start: String,
	pub end: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WidgetSettings {
	pub name: Option<String>,
	pub time_frame: Option<TimeFrame>,
	/// Each condition as Actual stores it.
	pub conditions: Vec<Json>,
	pub conditions_or: bool,
	pub mode: Option<String>,
	/// The summary card's `type`, kept inside its `content` JSON text.
	pub summary: Option<String>,
	/// The markdown card's text.
	pub content: Option<String>,
	pub text_align: Option<String>,
	pub formula: Option<String>,
	/// The custom report a `custom-report` widget shows.
	pub report_id: Option<String>,
}

/// The text Actual gives a new markdown card (`Overview.tsx` `onAddWidget`).
pub const DEFAULT_MARKDOWN: &str =
	"### Text Widget\n\nEdit this widget to change the **markdown** content.";

fn text(meta: Option<&Json>, key: &str) -> Option<String> {
	meta.and_then(|meta| meta.get(key))
		.and_then(Json::as_str)
		.map(str::to_owned)
}

fn summary_content(meta: Option<&Json>) -> Option<Json> {
	text(meta, "content").and_then(|content| Json::parse(&content).ok())
}

pub fn parse_meta(meta: Option<&str>) -> Result<Option<Json>, CoreError> {
	match meta {
		None | Some("null") => Ok(None),
		Some(text) => Json::parse(text)
			.map(Some)
			.map_err(|reason| CoreError::Invalid(format!("a widget's settings: {reason}"))),
	}
}

pub fn read_settings(kind: Option<WidgetType>, meta: Option<&Json>) -> WidgetSettings {
	let time_frame = meta
		.and_then(|meta| meta.get("timeFrame"))
		.filter(|frame| matches!(frame, Json::Object(_)))
		.map(|frame| {
			let field = |key: &str| frame.get(key).and_then(Json::as_str).unwrap_or_default();
			TimeFrame {
				mode: frame
					.get("mode")
					.and_then(Json::as_str)
					.unwrap_or("sliding-window")
					.to_owned(),
				start: field("start").to_owned(),
				end: field("end").to_owned(),
			}
		});
	let summary = kind == Some(WidgetType::Summary);
	let markdown = kind == Some(WidgetType::Markdown);
	WidgetSettings {
		name: text(meta, "name"),
		time_frame,
		conditions: meta
			.and_then(|meta| meta.get("conditions"))
			.and_then(Json::as_array)
			.unwrap_or_default()
			.to_vec(),
		conditions_or: text(meta, "conditionsOp").as_deref() == Some("or"),
		mode: text(meta, "mode"),
		summary: summary
			.then(|| {
				summary_content(meta).and_then(|content| {
					content
						.get("type")
						.and_then(Json::as_str)
						.map(str::to_owned)
				})
			})
			.flatten(),
		content: markdown.then(|| text(meta, "content")).flatten(),
		text_align: text(meta, "text_align"),
		formula: text(meta, "formula"),
		report_id: (kind == Some(WidgetType::CustomReport))
			.then(|| text(meta, "id"))
			.flatten(),
	}
}

fn set(entries: &mut Vec<(String, Json)>, key: &str, value: Option<Json>) {
	let at = entries.iter().position(|(name, _)| name == key);
	match (at, value) {
		(Some(at), Some(value)) => entries[at].1 = value,
		(None, Some(value)) => entries.push((key.to_owned(), value)),
		(Some(at), None) => {
			entries.remove(at);
		}
		(None, None) => {}
	}
}

fn string(value: Option<&String>) -> Option<Json> {
	value.map(|value| Json::String(value.clone()))
}

fn frame(old: Option<&Json>, frame: &TimeFrame) -> Json {
	let mut entries = match old {
		Some(Json::Object(entries)) => entries.clone(),
		_ => Vec::new(),
	};
	set(
		&mut entries,
		"start",
		Some(Json::String(frame.start.clone())),
	);
	set(&mut entries, "end", Some(Json::String(frame.end.clone())));
	set(&mut entries, "mode", Some(Json::String(frame.mode.clone())));
	Json::Object(entries)
}

/// The fields Actual's filter editor gives a condition's `type`
/// (`shared/rules.ts` `FIELD_INFO`).
fn field_type(field: &str) -> Option<&'static str> {
	Some(match field {
		"imported_payee" | "payee_name" | "notes" => "string",
		"payee" | "category" | "category_group" | "account" => "id",
		"date" => "date",
		"amount" => "number",
		"cleared" | "reconciled" | "transfer" | "parent" => "boolean",
		"saved" => "saved",
		_ => return None,
	})
}

/// A condition the way Actual's filter editor saves it: `{field, op, value,
/// options, type}`, `options` only when there are any (`FiltersMenu.tsx`
/// `onValidateAndApply`).
pub fn condition(
	field: &str,
	op: &str,
	value: Json,
	options: Option<Json>,
) -> Result<Json, CoreError> {
	let kind = field_type(field)
		.ok_or_else(|| CoreError::Invalid(format!("Veritas can't filter by {field}.")))?;
	let mut entries = vec![
		("field".to_owned(), Json::String(field.to_owned())),
		("op".to_owned(), Json::String(op.to_owned())),
		("value".to_owned(), value),
	];
	if let Some(options) = options.filter(|options| !matches!(options, Json::Null)) {
		entries.push(("options".to_owned(), options));
	}
	entries.push(("type".to_owned(), Json::String(kind.to_owned())));
	Ok(Json::Object(entries))
}

/// Refuses conditions Veritas can't evaluate, as Actual's `rule-validate`
/// refuses ones it can't run.
pub fn check_conditions(conditions: &[Json], any: bool) -> Result<(), CoreError> {
	TransactionFilter::new(conditions, any).map(|_| ())
}

/// The meta text with `settings` applied. Only keys whose value changed are
/// touched; `None` means nothing changed.
pub fn write_settings(
	kind: Option<WidgetType>,
	meta: Option<&str>,
	settings: &WidgetSettings,
) -> Result<Option<String>, CoreError> {
	let parsed = parse_meta(meta)?;
	let old = read_settings(kind, parsed.as_ref());
	if old == *settings {
		return Ok(None);
	}
	let mut entries = match &parsed {
		Some(Json::Object(entries)) => entries.clone(),
		None => Vec::new(),
		Some(_) => {
			return Err(CoreError::Invalid(
				"a widget's settings aren't an object".to_owned(),
			));
		}
	};
	if old.name != settings.name {
		set(&mut entries, "name", string(settings.name.as_ref()));
	}
	if old.conditions != settings.conditions {
		check_conditions(&settings.conditions, settings.conditions_or)?;
		set(
			&mut entries,
			"conditions",
			Some(Json::Array(settings.conditions.clone())),
		);
	}
	if old.conditions_or != settings.conditions_or {
		let op = if settings.conditions_or { "or" } else { "and" };
		set(
			&mut entries,
			"conditionsOp",
			Some(Json::String(op.to_owned())),
		);
	}
	if old.time_frame != settings.time_frame {
		let value = settings
			.time_frame
			.as_ref()
			.map(|new| frame(parsed.as_ref().and_then(|meta| meta.get("timeFrame")), new));
		set(&mut entries, "timeFrame", value);
	}
	if old.mode != settings.mode {
		set(&mut entries, "mode", string(settings.mode.as_ref()));
	}
	if kind == Some(WidgetType::Summary) && old.summary != settings.summary {
		let mut content = match summary_content(parsed.as_ref()) {
			Some(Json::Object(entries)) => entries,
			_ => Vec::new(),
		};
		set(&mut content, "type", string(settings.summary.as_ref()));
		set(
			&mut entries,
			"content",
			Some(Json::String(Json::Object(content).stringify())),
		);
	}
	if kind == Some(WidgetType::Markdown) && old.content != settings.content {
		set(&mut entries, "content", string(settings.content.as_ref()));
	}
	if old.text_align != settings.text_align {
		set(
			&mut entries,
			"text_align",
			string(settings.text_align.as_ref()),
		);
	}
	if old.formula != settings.formula {
		set(&mut entries, "formula", string(settings.formula.as_ref()));
	}
	if kind == Some(WidgetType::CustomReport) && old.report_id != settings.report_id {
		set(&mut entries, "id", string(settings.report_id.as_ref()));
	}
	Ok(Some(Json::Object(entries).stringify()))
}

fn live_page<'a>(budget: &'a BudgetData, id: &str) -> Result<&'a DashboardPage, CoreError> {
	budget
		.dashboard_pages
		.get(&DashboardPageId::new(id))
		.filter(|page| !page.tombstone)
		.ok_or_else(|| CoreError::NotFound {
			entity: "dashboard",
			id: id.to_owned(),
		})
}

pub fn live_widget<'a>(budget: &'a BudgetData, id: &str) -> Result<&'a Widget, CoreError> {
	budget
		.widgets
		.get(&WidgetId::new(id))
		.filter(|widget| !widget.tombstone && widget.kind.is_some())
		.ok_or_else(|| CoreError::NotFound {
			entity: "widget",
			id: id.to_owned(),
		})
}

fn check_report(budget: &BudgetData, settings: &WidgetSettings) -> Result<(), CoreError> {
	let Some(id) = &settings.report_id else {
		return Err(CoreError::Invalid(
			"Choose the custom report this widget shows.".to_owned(),
		));
	};
	let live = budget
		.custom_reports
		.get(&veritas_domain::CustomReportId::new(id.as_str()))
		.is_some_and(|row| !row.tombstone);
	if live {
		Ok(())
	} else {
		Err(CoreError::NotFound {
			entity: "custom report",
			id: id.clone(),
		})
	}
}

/// A new widget on `page`, as `dashboard-add-widget` inserts it. Settings
/// left at their defaults store `meta` as null, as Actual's add menu does;
/// a markdown card without text gets Actual's starter text.
pub fn add_widget(
	budget: &BudgetData,
	id: WidgetId,
	page: &str,
	kind: &str,
	(x, y, width, height): (i64, i64, i64, i64),
	settings: &WidgetSettings,
) -> Result<Row, CoreError> {
	let page = live_page(budget, page)?;
	let known = WidgetType::parse(kind)
		.ok_or_else(|| CoreError::Invalid(format!("Actual has no {kind} widget.")))?;
	let mut settings = settings.clone();
	if known == WidgetType::CustomReport {
		check_report(budget, &settings)?;
	}
	if known == WidgetType::Markdown && settings.content.is_none() {
		settings.content = Some(DEFAULT_MARKDOWN.to_owned());
	}
	let meta = write_settings(Some(known), None, &settings)?;
	let placed = place(Some(known), x, y, width, height)?;
	Ok(Row::Widget(Widget {
		id,
		kind: Some(kind.to_owned()),
		width: Some(placed.2),
		height: Some(placed.3),
		x: Some(placed.0),
		y: Some(placed.1),
		meta,
		page: Some(page.id.clone()),
		tombstone: false,
	}))
}

/// A frame clamped to the grid and the type's minimum size.
pub(crate) fn place(
	kind: Option<WidgetType>,
	x: i64,
	y: i64,
	width: i64,
	height: i64,
) -> Result<(i64, i64, i64, i64), CoreError> {
	let (min_width, min_height) = kind.map_or((1, 1), WidgetType::minimum_size);
	if y < 0 || x < 0 {
		return Err(CoreError::Invalid(
			"a widget can't sit above or left of the grid".to_owned(),
		));
	}
	let width = width.clamp(min_width, GRID_COLUMNS);
	let height = height.max(min_height);
	Ok((x.min(GRID_COLUMNS - width), y, width, height))
}

pub fn set_widget_settings(
	budget: &BudgetData,
	id: &str,
	settings: &WidgetSettings,
) -> Result<Vec<Row>, CoreError> {
	let widget = live_widget(budget, id)?;
	let kind = widget
		.kind
		.as_deref()
		.and_then(WidgetType::parse)
		.ok_or_else(|| {
			CoreError::Invalid(
				"Veritas can't change the settings of this kind of widget.".to_owned(),
			)
		})?;
	if kind == WidgetType::CustomReport {
		check_report(budget, settings)?;
	}
	Ok(
		match write_settings(Some(kind), widget.meta.as_deref(), settings)? {
			Some(meta) => vec![Row::Widget(Widget {
				meta: Some(meta),
				..widget.clone()
			})],
			None => Vec::new(),
		},
	)
}

/// `dashboard-remove-widget`: the row is tombstoned.
pub fn remove_widget(budget: &BudgetData, id: &str) -> Result<Vec<Row>, CoreError> {
	let widget = live_widget(budget, id)?;
	Ok(vec![Row::Widget(Widget {
		tombstone: true,
		..widget.clone()
	})])
}

fn page_name(name: &str) -> Result<String, CoreError> {
	let name = name.trim();
	if name.is_empty() {
		return Err(CoreError::Invalid(
			"Enter a name for the dashboard.".to_owned(),
		));
	}
	Ok(name.to_owned())
}

/// `dashboard-create`.
pub fn create_page(id: DashboardPageId, name: &str) -> Result<Row, CoreError> {
	Ok(Row::DashboardPage(DashboardPage {
		id,
		name: Some(page_name(name)?),
		tombstone: false,
	}))
}

/// `dashboard-rename`.
pub fn rename_page(budget: &BudgetData, id: &str, name: &str) -> Result<Vec<Row>, CoreError> {
	let page = live_page(budget, id)?;
	let name = page_name(name)?;
	if page.name.as_deref() == Some(name.as_str()) {
		return Ok(Vec::new());
	}
	Ok(vec![Row::DashboardPage(DashboardPage {
		name: Some(name),
		..page.clone()
	})])
}

/// `dashboard-delete`: the page and its live widgets are tombstoned, and the
/// last page can't go.
pub fn delete_page(budget: &BudgetData, id: &str) -> Result<Vec<Row>, CoreError> {
	let page = live_page(budget, id)?;
	let live = budget
		.dashboard_pages
		.values()
		.filter(|page| !page.tombstone)
		.count();
	if live <= 1 {
		return Err(CoreError::Invalid(
			"You can't delete the last dashboard.".to_owned(),
		));
	}
	let mut rows = vec![Row::DashboardPage(DashboardPage {
		tombstone: true,
		..page.clone()
	})];
	rows.extend(
		budget
			.widgets
			.values()
			.filter(|widget| !widget.tombstone && widget.page.as_ref() == Some(&page.id))
			.map(|widget| {
				Row::Widget(Widget {
					tombstone: true,
					..widget.clone()
				})
			}),
	);
	Ok(rows)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn meta(text: &str) -> Option<String> {
		Some(text.to_owned())
	}

	#[test]
	fn unchanged_settings_write_nothing() {
		let stored = r#"{"name":"Cash","future":{"x":1.50},"timeFrame":{"start":"2026-01","end":"2026-06","mode":"static"}}"#;
		let kind = Some(WidgetType::CashFlow);
		let settings = read_settings(kind, parse_meta(Some(stored)).unwrap().as_ref());
		assert_eq!(write_settings(kind, Some(stored), &settings).unwrap(), None);
	}

	#[test]
	fn a_change_touches_only_its_key_and_keeps_unknown_text() {
		let stored = r#"{"name":"Cash","future":{"x":1.50},"timeFrame":{"start":"2026-01","end":"2026-06","mode":"static"}}"#;
		let kind = Some(WidgetType::CashFlow);
		let mut settings = read_settings(kind, parse_meta(Some(stored)).unwrap().as_ref());
		settings.name = Some("Money".to_owned());
		settings.conditions_or = true;
		assert_eq!(
			write_settings(kind, Some(stored), &settings).unwrap(),
			meta(
				r#"{"name":"Money","future":{"x":1.50},"timeFrame":{"start":"2026-01","end":"2026-06","mode":"static"},"conditionsOp":"or"}"#
			)
		);
	}

	#[test]
	fn the_summary_type_lives_inside_its_content_text() {
		let stored = r#"{"content":"{\"type\":\"percentage\",\"divisorConditions\":[]}"}"#;
		let kind = Some(WidgetType::Summary);
		let mut settings = read_settings(kind, parse_meta(Some(stored)).unwrap().as_ref());
		assert_eq!(settings.summary.as_deref(), Some("percentage"));
		settings.summary = Some("sum".to_owned());
		assert_eq!(
			write_settings(kind, Some(stored), &settings).unwrap(),
			meta(r#"{"content":"{\"type\":\"sum\",\"divisorConditions\":[]}"}"#)
		);
	}

	#[test]
	fn default_settings_store_null_meta() {
		assert_eq!(
			write_settings(Some(WidgetType::NetWorth), None, &WidgetSettings::default()).unwrap(),
			None
		);
	}

	#[test]
	fn conditions_are_saved_the_way_actuals_filter_editor_saves_them() {
		let made = condition(
			"amount",
			"gte",
			Json::Number("2000".to_owned()),
			Some(Json::parse(r#"{"outflow":true}"#).unwrap()),
		)
		.unwrap();
		assert_eq!(
			made.stringify(),
			r#"{"field":"amount","op":"gte","value":2000,"options":{"outflow":true},"type":"number"}"#
		);
		assert!(condition("bogus", "is", Json::Null, None).is_err());
	}
}
