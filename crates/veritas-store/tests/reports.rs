//! Custom reports, dashboard pages and widgets: loaded raw, tombstones
//! included, and written back one changed column at a time.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Scratch, count, fixture_db};
use rusqlite::Connection;
use rusqlite::types::Value;
use veritas_domain::{CustomReportRow, Widget};
use veritas_store::{Access, Store};

fn row(conn: &Connection, table: &str, id: &str) -> Vec<(String, Value)> {
	let mut statement = conn
		.prepare(&format!("SELECT * FROM {table} WHERE id = ?1"))
		.unwrap();
	let names: Vec<String> = statement.column_names().map(str::to_owned).collect();
	statement
		.query_row([id], |row| {
			Ok(names
				.iter()
				.enumerate()
				.map(|(index, name)| (name.clone(), row.get::<_, Value>(index).unwrap()))
				.collect())
		})
		.unwrap()
}

fn changed(before: &[(String, Value)], after: &[(String, Value)]) -> Vec<String> {
	before
		.iter()
		.zip(after)
		.filter(|(a, b)| a.1 != b.1)
		.map(|(a, _)| a.0.clone())
		.collect()
}

#[test]
fn every_report_page_and_widget_loads_tombstones_included_r_rep_4() {
	let scratch = Scratch::new("reports-load");
	let db = fixture_db("all-features", &scratch);
	let raw = Connection::open(&db).unwrap();
	raw.execute_batch(
		"UPDATE dashboard SET tombstone = 1 WHERE id = (SELECT id FROM dashboard ORDER BY id LIMIT 1);
		 UPDATE custom_reports SET tombstone = 1 WHERE id = (SELECT id FROM custom_reports ORDER BY id LIMIT 1);",
	)
	.unwrap();
	let loaded = Store::open(&db, Access::ReadWrite).unwrap().load().unwrap();
	assert_eq!(loaded.report.skipped, vec![]);
	let data = &loaded.data;
	assert_eq!(
		data.custom_reports.len(),
		count(&raw, "SELECT count(*) FROM custom_reports")
	);
	assert_eq!(
		data.dashboard_pages.len(),
		count(&raw, "SELECT count(*) FROM dashboard_pages")
	);
	assert_eq!(
		data.widgets.len(),
		count(&raw, "SELECT count(*) FROM dashboard")
	);
	assert!(data.custom_reports.len() >= 2 && data.dashboard_pages.len() >= 2);
	assert_eq!(data.widgets.values().filter(|w| w.tombstone).count(), 1);
	assert_eq!(
		data.custom_reports.values().filter(|r| r.tombstone).count(),
		1
	);

	let (id, kind, meta): (String, String, Option<String>) = raw
		.query_row(
			"SELECT id, type, meta FROM dashboard WHERE meta IS NOT NULL ORDER BY id LIMIT 1",
			[],
			|row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
		)
		.unwrap();
	let widget = &data.widgets[&veritas_domain::WidgetId::new(id)];
	assert_eq!(widget.kind.as_deref(), Some(kind.as_str()));
	assert_eq!(widget.meta, meta, "meta stays the stored JSON text");
	assert!(widget.page.is_some());
}

#[test]
fn rewriting_reports_and_widgets_unchanged_leaves_the_file_byte_identical() {
	let scratch = Scratch::new("reports-noop");
	let db = fixture_db("all-features", &scratch);
	let mut store = Store::open(&db, Access::ReadWrite).unwrap();
	let data = store.load().unwrap().data;
	let before = std::fs::read(&db).unwrap();
	let mut writer = store.writer().unwrap();
	for report in data.custom_reports.values() {
		writer
			.put_custom_report(Some(report), Some(report))
			.unwrap();
	}
	for page in data.dashboard_pages.values() {
		writer.put_dashboard_page(Some(page), Some(page)).unwrap();
	}
	for widget in data.widgets.values() {
		writer.put_widget(Some(widget), Some(widget)).unwrap();
	}
	writer.commit().unwrap();
	drop(store);
	assert_eq!(std::fs::read(&db).unwrap(), before);
}

#[test]
fn moving_a_widget_or_renaming_a_report_writes_only_those_columns() {
	let scratch = Scratch::new("reports-edit");
	let db = fixture_db("all-features", &scratch);
	let mut store = Store::open(&db, Access::ReadWrite).unwrap();
	let data = store.load().unwrap().data;
	let raw = Connection::open(&db).unwrap();
	let widget = data
		.widgets
		.values()
		.find(|w| !w.tombstone)
		.unwrap()
		.clone();
	let report = data
		.custom_reports
		.values()
		.find(|r| !r.tombstone)
		.unwrap()
		.clone();
	let widget_before = row(&raw, "dashboard", widget.id.as_str());
	let report_before = row(&raw, "custom_reports", report.id.as_str());

	let moved = Widget {
		x: Some(widget.x.unwrap_or_default() + 1),
		y: Some(widget.y.unwrap_or_default() + 2),
		width: Some(widget.width.unwrap_or(4) + 1),
		height: Some(widget.height.unwrap_or(2) + 1),
		..widget.clone()
	};
	let renamed = CustomReportRow {
		name: Some("Spending by payee".to_owned()),
		..report.clone()
	};
	let mut writer = store.writer().unwrap();
	writer.put_widget(Some(&widget), Some(&moved)).unwrap();
	writer
		.put_custom_report(Some(&report), Some(&renamed))
		.unwrap();
	writer.commit().unwrap();

	assert_eq!(
		changed(&widget_before, &row(&raw, "dashboard", widget.id.as_str())),
		["width", "height", "x", "y"]
	);
	assert_eq!(
		changed(
			&report_before,
			&row(&raw, "custom_reports", report.id.as_str())
		),
		["name"]
	);
	let reloaded = store.load().unwrap().data;
	assert_eq!(reloaded.widgets[&widget.id], moved);
	assert_eq!(reloaded.custom_reports[&report.id], renamed);
}
