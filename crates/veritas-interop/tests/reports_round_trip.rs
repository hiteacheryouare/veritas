//! Custom reports and dashboards through Veritas and back out as an Actual
//! export: untouched rows stay byte for byte, an edit changes only its row.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Scratch, assert_same_dump, fixture, imported};
use veritas_domain::Widget;
use veritas_interop::{
	ExportOutcome, canonical_dump_of_actual_tables, export_actual_zip, import_actual_zip,
};
use veritas_store::{Access, Store};

fn export(store: &Store, metadata: &std::path::Path, to: &std::path::Path) {
	assert!(matches!(
		export_actual_zip(store, metadata, to).unwrap(),
		ExportOutcome::Exported(audit) if audit.is_clean()
	));
}

#[test]
fn reports_and_dashboards_round_trip_and_an_edit_touches_one_row_r_io_3_r_rep_4() {
	let scratch = Scratch::new("reports-round-trip");
	let budget = imported(import_actual_zip(
		&fixture("all-features"),
		&scratch.join("budget"),
	));
	let mut store = Store::open(&budget.db_path, Access::ReadWrite).unwrap();
	let data = store.load().unwrap().data;
	assert!(!data.custom_reports.is_empty() && !data.widgets.is_empty());

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
	let untouched = scratch.join("untouched.zip");
	export(&store, &budget.metadata_path, &untouched);
	let original = canonical_dump_of_actual_tables(&fixture("all-features")).unwrap();
	assert_same_dump(
		&canonical_dump_of_actual_tables(&untouched).unwrap(),
		&original,
		"untouched",
	);

	let widget = data
		.widgets
		.values()
		.find(|w| !w.tombstone)
		.unwrap()
		.clone();
	let moved = Widget {
		x: Some(widget.x.unwrap_or_default() + 3),
		..widget.clone()
	};
	let mut writer = store.writer().unwrap();
	writer.put_widget(Some(&widget), Some(&moved)).unwrap();
	writer.commit().unwrap();
	let edited = scratch.join("edited.zip");
	export(&store, &budget.metadata_path, &edited);
	let after = String::from_utf8(canonical_dump_of_actual_tables(&edited).unwrap()).unwrap();
	let before = String::from_utf8(original).unwrap();
	let differing: Vec<(&str, &str)> = before
		.lines()
		.zip(after.lines())
		.filter(|(a, b)| a != b)
		.collect();
	assert_eq!(differing.len(), 1, "{differing:#?}");
	assert!(
		differing[0]
			.1
			.contains(&format!("\"id\":\"{}\"", widget.id))
	);
	assert!(
		differing[0]
			.1
			.contains(&format!("\"x\":{}", moved.x.unwrap()))
	);
}
