//! Fixtures loaded the way the app loads them, and the Actual oracle.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;
use veritas_domain::{
	BudgetData, CustomReportId, CustomReportRow, DashboardPage, DashboardPageId, Widget, WidgetId,
};

static NEXT: AtomicU64 = AtomicU64::new(0);

pub fn root() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Runs one oracle command over a fixture with a JSON request file, or
/// `None` when the oracle isn't installed. CI sets VERITAS_REQUIRE_ORACLE,
/// so there a missing oracle fails instead.
pub fn oracle(command: &str, fixture: &str, requests: &Value, today: &str) -> Option<Value> {
	let dir = root().join("tools/actual-oracle");
	if !dir.join("node_modules").exists() {
		assert!(
			std::env::var_os("VERITAS_REQUIRE_ORACLE").is_none(),
			"VERITAS_REQUIRE_ORACLE is set but tools/actual-oracle/node_modules is missing"
		);
		eprintln!("skipping the {command} oracle comparison: run `cargo xtask oracle setup`");
		return None;
	}
	let file = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
		"{command}-{}-{}.json",
		std::process::id(),
		NEXT.fetch_add(1, Ordering::Relaxed)
	));
	std::fs::write(&file, requests.to_string()).unwrap();
	let output = Command::new("node")
		.current_dir(&dir)
		.args(["src/main.ts", command])
		.arg(root().join(format!("fixtures/synthetic/{fixture}.zip")))
		.arg(&file)
		.args(["--today", today])
		.output()
		.expect("node runs");
	let _ = std::fs::remove_file(&file);
	assert!(
		output.status.success(),
		"the oracle failed: {}",
		String::from_utf8_lossy(&output.stderr)
	);
	Some(serde_json::from_slice(&output.stdout).unwrap())
}

pub fn text(row: &rusqlite::Row, name: &str) -> Option<String> {
	row.get::<_, Option<String>>(name).unwrap()
}

pub fn int(row: &rusqlite::Row, name: &str) -> Option<i64> {
	row.get::<_, Option<i64>>(name).unwrap()
}

/// A synthetic fixture loaded the way the app loads it, plus the report and
/// dashboard tables, which veritas-store does not read yet.
pub fn fixture(name: &str) -> BudgetData {
	let scratch = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
		"reports-{name}-{}-{}",
		std::process::id(),
		NEXT.fetch_add(1, Ordering::Relaxed)
	));
	let _ = std::fs::remove_dir_all(&scratch);
	let veritas_interop::ImportOutcome::Imported(imported) = veritas_interop::import_actual_zip(
		&root().join(format!("fixtures/synthetic/{name}.zip")),
		&scratch,
	)
	.unwrap() else {
		panic!("{name} imports cleanly");
	};
	let mut store =
		veritas_store::Store::open(&imported.db_path, veritas_store::Access::ReadWrite).unwrap();
	store.adopt().unwrap();
	let mut budget = store.load().unwrap().data;
	drop(store);
	let conn = rusqlite::Connection::open(&imported.db_path).unwrap();
	let mut reports = conn.prepare("SELECT * FROM custom_reports").unwrap();
	let mut rows = reports.query([]).unwrap();
	while let Some(row) = rows.next().unwrap() {
		let report = CustomReportRow {
			id: CustomReportId::new(text(row, "id").unwrap()),
			name: text(row, "name"),
			start_date: text(row, "start_date"),
			end_date: text(row, "end_date"),
			date_static: int(row, "date_static"),
			date_range: text(row, "date_range"),
			mode: text(row, "mode"),
			group_by: text(row, "group_by"),
			balance_type: text(row, "balance_type"),
			show_empty: int(row, "show_empty"),
			show_offbudget: int(row, "show_offbudget"),
			show_hidden: int(row, "show_hidden"),
			show_uncategorized: int(row, "show_uncategorized"),
			selected_categories: text(row, "selected_categories"),
			graph_type: text(row, "graph_type"),
			conditions: text(row, "conditions"),
			conditions_op: text(row, "conditions_op"),
			metadata: text(row, "metadata"),
			interval: text(row, "interval"),
			color_scheme: text(row, "color_scheme"),
			tombstone: int(row, "tombstone") == Some(1),
			include_current: int(row, "include_current"),
			sort_by: text(row, "sort_by"),
			trim_intervals: int(row, "trim_intervals"),
			show_trend_lines: int(row, "show_trend_lines"),
		};
		budget.custom_reports.insert(report.id.clone(), report);
	}
	let mut pages = conn
		.prepare("SELECT id, name, tombstone FROM dashboard_pages")
		.unwrap();
	let mut rows = pages.query([]).unwrap();
	while let Some(row) = rows.next().unwrap() {
		let id = DashboardPageId::new(text(row, "id").unwrap());
		budget.dashboard_pages.insert(
			id.clone(),
			DashboardPage {
				id,
				name: text(row, "name"),
				tombstone: int(row, "tombstone") == Some(1),
			},
		);
	}
	let mut widgets = conn.prepare("SELECT * FROM dashboard").unwrap();
	let mut rows = widgets.query([]).unwrap();
	while let Some(row) = rows.next().unwrap() {
		let id = WidgetId::new(text(row, "id").unwrap());
		budget.widgets.insert(
			id.clone(),
			Widget {
				id,
				kind: text(row, "type"),
				width: int(row, "width"),
				height: int(row, "height"),
				x: int(row, "x"),
				y: int(row, "y"),
				meta: text(row, "meta"),
				page: text(row, "dashboard_page_id").map(DashboardPageId::new),
				tombstone: int(row, "tombstone") == Some(1),
			},
		);
	}
	let _ = std::fs::remove_dir_all(&scratch);
	budget
}
