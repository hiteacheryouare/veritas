//! R-REP-4: Actual's custom reports and dashboards on the all-features
//! fixture. Query rows are compared with Actual's own query engine through the
//! oracle (`report-rows`), live ranges with Actual's `getLiveRange` rules, and
//! saved reports must round-trip byte for byte.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::NaiveDate;
use serde_json::{Value, json};
use support::*;
use veritas_domain::{
	BudgetData, CustomReportId, CustomReportRow, DashboardPage, DashboardPageId, Money, Widget,
	WidgetId,
};
use veritas_reports::{
	BalanceType, CustomReport, GroupBy, Json, QueryRow, SortBy, WidgetContent, WidgetType,
	dashboards, evaluate_report, place_widget, report_range, report_rows,
};

const ORACLE_TODAY: &str = "2026-11-09";

static NEXT: AtomicU64 = AtomicU64::new(0);

fn root() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn today() -> NaiveDate {
	NaiveDate::parse_from_str(ORACLE_TODAY, "%Y-%m-%d").unwrap()
}

fn text(row: &rusqlite::Row, name: &str) -> Option<String> {
	row.get::<_, Option<String>>(name).unwrap()
}

fn int(row: &rusqlite::Row, name: &str) -> Option<i64> {
	row.get::<_, Option<i64>>(name).unwrap()
}

/// all-features loaded the way the app loads it, plus the report and
/// dashboard tables, which veritas-store does not read yet.
fn all_features() -> BudgetData {
	let scratch = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
		"reports-af-{}-{}",
		std::process::id(),
		NEXT.fetch_add(1, Ordering::Relaxed)
	));
	let _ = std::fs::remove_dir_all(&scratch);
	let imported = veritas_interop::import_actual_zip(
		&root().join("fixtures/synthetic/all-features.zip"),
		&scratch,
	)
	.unwrap();
	let mut store = veritas_store::Store::open(&imported.db_path).unwrap();
	store.adopt().unwrap();
	let mut budget = store.load().unwrap().data;
	drop(store);
	let conn = rusqlite::Connection::open(&imported.db_path).unwrap();
	let mut reports = conn.prepare("SELECT * FROM custom_reports").unwrap();
	let mut rows = reports.query([]).unwrap();
	while let Some(row) = rows.next().unwrap() {
		let report = CustomReportRow {
			id: text(row, "id").unwrap(),
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
			tombstone: int(row, "tombstone"),
			include_current: int(row, "include_current"),
			sort_by: text(row, "sort_by"),
			trim_intervals: int(row, "trim_intervals"),
			show_trend_lines: int(row, "show_trend_lines"),
		};
		budget
			.custom_reports
			.insert(CustomReportId::new(report.id.clone()), report);
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

fn live_reports(budget: &BudgetData) -> Vec<CustomReport> {
	budget
		.custom_reports
		.values()
		.filter(|row| row.tombstone != Some(1))
		.map(|row| CustomReport::from_row(row).unwrap())
		.collect()
}

fn row_json(row: &QueryRow) -> Value {
	let id = |value: Option<String>| value.map_or(Value::Null, Value::String);
	json!({
		"account": id(row.account.as_ref().map(ToString::to_string)),
		"accountOffBudget": row.account_off_budget,
		"amount": row.amount.minor(),
		"category": id(row.category.as_ref().map(ToString::to_string)),
		"categoryGroup": id(row.category_group.as_ref().map(ToString::to_string)),
		"categoryGroupHidden": row.category_group_hidden,
		"categoryHidden": row.category_hidden,
		"categoryIncome": row.category_income,
		"date": row.date,
		"payee": id(row.payee.as_ref().map(ToString::to_string)),
		"transferAccount": id(row.transfer_account.as_ref().map(ToString::to_string)),
	})
}

fn sorted(values: Vec<Value>) -> Vec<String> {
	let mut keys: Vec<String> = values.iter().map(Value::to_string).collect();
	keys.sort();
	keys
}

/// `report-rows` from the oracle, or `None` when it isn't installed. CI sets
/// VERITAS_REQUIRE_ORACLE, so there a missing oracle fails instead.
fn oracle_rows(requests: &Value) -> Option<Value> {
	let oracle = root().join("tools/actual-oracle");
	if !oracle.join("node_modules").exists() {
		assert!(
			std::env::var_os("VERITAS_REQUIRE_ORACLE").is_none(),
			"VERITAS_REQUIRE_ORACLE is set but tools/actual-oracle/node_modules is missing"
		);
		eprintln!("skipping the custom report oracle comparison: run `cargo xtask oracle setup`");
		return None;
	}
	let file = Path::new(env!("CARGO_TARGET_TMPDIR"))
		.join(format!("report-rows-{}.json", std::process::id()));
	std::fs::write(&file, requests.to_string()).unwrap();
	let output = Command::new("node")
		.current_dir(&oracle)
		.args(["src/main.ts", "report-rows"])
		.arg(root().join("fixtures/synthetic/all-features.zip"))
		.arg(&file)
		.args(["--today", ORACLE_TODAY])
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

#[test]
fn r_rep_4_query_rows_match_actuals_query_engine() {
	let budget = all_features();
	let mut reports = live_reports(&budget);
	assert_eq!(reports.len(), 2, "all-features has two custom reports");
	let base = reports[1].clone();
	for (index, (interval, conditions, or)) in [
		(veritas_reports::Interval::Yearly, "[]", false),
		(veritas_reports::Interval::Weekly, "[]", false),
		(veritas_reports::Interval::Daily, r#"[{"field":"amount","op":"lt","value":-5000,"type":"number"}]"#, false),
		(veritas_reports::Interval::Monthly, r#"[{"field":"notes","op":"contains","value":"PAYMENT","type":"string"},{"field":"category","op":"is","value":null,"type":"id"}]"#, true),
		(veritas_reports::Interval::Monthly, r#"[{"field":"account","op":"isNot","value":"0a1c000f-c328-42b8-8966-a4be59a2504b","type":"id"},{"field":"amount","op":"gte","value":2000,"type":"number","options":{"outflow":true}}]"#, false),
		(veritas_reports::Interval::Monthly, r#"[{"field":"transfer","op":"is","value":true,"type":"boolean"},{"field":"date","op":"is","value":"2026-03","type":"date"}]"#, true),
	]
	.into_iter()
	.enumerate()
	{
		let mut variant = base.clone();
		variant.id = format!("variant-{index}");
		variant.interval = interval;
		variant.conditions = Json::parse(conditions).unwrap().as_array().unwrap().to_vec();
		variant.conditions_or = or;
		variant.row.conditions = Some(conditions.to_owned());
		reports.push(variant);
	}
	let mut requests = Vec::new();
	let mut ours = Vec::new();
	for report in &reports {
		let (start, end) = report_range(report, &budget, today(), 0).unwrap();
		let interval = format!("{:?}", report.interval);
		let conditions: Value =
			serde_json::from_str(report.row.conditions.as_deref().unwrap_or("[]")).unwrap();
		requests.push(json!({
			"id": report.id,
			"startDate": start,
			"endDate": end,
			"interval": interval,
			"conditions": conditions,
			"conditionsOp": if report.conditions_or { "or" } else { "and" },
		}));
		let (assets, debts) = report_rows(&budget, report, &start, &end).unwrap();
		assert!(
			!assets.is_empty() || !debts.is_empty(),
			"{} has rows",
			report.name
		);
		ours.push((report.id.clone(), assets, debts));
	}
	let Some(theirs) = oracle_rows(&Value::Array(requests)) else {
		return;
	};
	for (id, assets, debts) in ours {
		let expected = &theirs[&id];
		for (side, rows) in [("assets", assets), ("debts", debts)] {
			let actual = sorted(rows.iter().map(row_json).collect());
			let wanted = sorted(expected[side].as_array().unwrap().clone());
			assert_eq!(actual, wanted, "{id} {side}");
		}
	}
}

#[test]
fn r_rep_4_saved_reports_round_trip_byte_for_byte() {
	let budget = all_features();
	for row in budget.custom_reports.values() {
		let report = CustomReport::from_row(row).unwrap();
		assert_eq!(&report.to_row(), row, "{} round trip", row.id);
	}
	let row = budget.custom_reports.values().next().unwrap();
	let mut report = CustomReport::from_row(row).unwrap();
	report.name = "Groceries and dining".to_owned();
	let saved = report.to_row();
	assert_eq!(saved.name.as_deref(), Some("Groceries and dining"));
	assert_eq!(
		CustomReportRow {
			name: row.name.clone(),
			..saved.clone()
		},
		row.clone(),
		"only the edited column changes"
	);

	report
		.conditions
		.push(Json::parse(r#"{"field":"amount","op":"lt","value":-500,"type":"number"}"#).unwrap());
	report.sort_by = SortBy::Name;
	let saved = report.to_row();
	assert_eq!(
		saved.conditions.as_deref(),
		Some(
			r#"[{"field":"category","op":"oneOf","value":["fc093a41-2d11-4d98-a211-e5db92205f23","fffcd498-80e7-4984-b378-87cee6b5625c"],"type":"id"},{"field":"amount","op":"lt","value":-500,"type":"number"}]"#
		)
	);
	assert_eq!(saved.sort_by.as_deref(), Some("name"));
	assert_eq!(saved.metadata, row.metadata);
}

fn live_report(range: &str, include_current: bool) -> CustomReport {
	CustomReport::from_row(&CustomReportRow {
		id: "r".to_owned(),
		date_static: Some(0),
		date_range: Some(range.to_owned()),
		include_current: Some(i64::from(include_current)),
		..CustomReportRow::default()
	})
	.unwrap()
}

#[test]
fn r_rep_4_live_ranges_follow_get_live_range() {
	let mut book = Book::new();
	book.account("CHK", false);
	book.row("first", "CHK", "2025-02-14", -100);
	book.row("last", "CHK", "2026-10-30", -100);
	let range = |name: &str, include: bool| {
		let (start, end) =
			report_range(&live_report(name, include), &book.budget, today(), 0).unwrap();
		format!("{start} {end}")
	};
	assert_eq!(range("Last 6 months", false), "2026-05-01 2026-10-31");
	assert_eq!(range("Last 6 months", true), "2026-05-01 2026-11-30");
	assert_eq!(range("Last 12 months", false), "2025-11-01 2026-10-31");
	assert_eq!(range("This month", false), "2026-11-01 2026-11-30");
	assert_eq!(range("Last month", false), "2026-10-01 2026-10-31");
	assert_eq!(
		range("Last month", true),
		"2026-10-01 2026-11-30",
		"the stored flag still counts"
	);
	assert_eq!(range("This week", false), "2026-11-08 2026-11-14");
	assert_eq!(range("Last week", false), "2026-11-01 2026-11-07");
	assert_eq!(range("Year to date", false), "2026-01-01 2026-11-09");
	assert_eq!(
		range("Last year", false),
		"2025-02-14 2025-12-31",
		"clamped to the first row"
	);
	assert_eq!(range("Prior year to date", false), "2025-02-14 2025-11-09");
	assert_eq!(range("Current quarter", false), "2026-10-01 2026-12-31");
	assert_eq!(range("Previous quarter", false), "2026-07-01 2026-09-30");
	assert_eq!(range("Last 30 days", false), "2026-10-11 2026-11-09");
	assert_eq!(range("All time", false), "2025-02-14 2026-10-30");
}

fn spending_report(group_by: &str, balance: &str, sort: &str) -> CustomReportRow {
	CustomReportRow {
		id: "spend".to_owned(),
		start_date: Some("2027-01-01".to_owned()),
		end_date: Some("2027-02-28".to_owned()),
		date_static: Some(1),
		mode: Some("total".to_owned()),
		group_by: Some(group_by.to_owned()),
		balance_type: Some(balance.to_owned()),
		sort_by: Some(sort.to_owned()),
		interval: Some("Monthly".to_owned()),
		show_hidden: Some(0),
		show_offbudget: Some(0),
		show_uncategorized: Some(1),
		..CustomReportRow::default()
	}
}

fn totals(data: &veritas_reports::ReportData) -> Vec<(String, i64)> {
	data.groups
		.iter()
		.map(|group| (group.name.clone(), group.balances.total_debts.minor()))
		.collect()
}

#[test]
fn r_rep_4_payment_by_category_uses_actuals_buckets_filters_and_sort() {
	let mut book = master();
	book.budget
		.categories
		.get_mut(&cat("Car purchase"))
		.unwrap()
		.hidden = true;
	book.row("U", "CHK", "2027-01-22", -700);
	let report = CustomReport::from_row(&spending_report("Category", "Payment", "desc")).unwrap();
	assert_eq!(report.balance_type, BalanceType::TotalDebts);
	let data = evaluate_report(&book.budget, &report, today(), 0).unwrap();
	assert_eq!(
		totals(&data),
		[
			("Loan principal", -32_000),
			("Savings", -20_000),
			("Transfers", -10_000),
			("Loan interest", -8_000),
			("Groceries", -7_500),
			("Clothing", -4_000),
			("Uncategorized", -1_900),
		]
		.map(|(name, amount)| (name.to_owned(), amount))
	);
	assert_eq!(data.totals.total_debts.minor(), -83_400);
	assert_eq!(data.intervals.len(), 2);
	assert_eq!(data.intervals[0].interval.start, "2027-01-01");
	assert_eq!(data.intervals[0].interval.end, "2027-01-31");
	assert_eq!(data.intervals[1].interval.end, "2027-02-28");
	assert_eq!(
		data.intervals[1].interval.balances.total_debts.minor(),
		-12_500
	);
}

#[test]
fn r_rep_4_hidden_and_off_budget_rows_appear_when_asked() {
	let mut book = master();
	book.budget
		.categories
		.get_mut(&cat("Car purchase"))
		.unwrap()
		.hidden = true;
	let mut row = spending_report("Category", "Payment", "asc");
	row.show_hidden = Some(1);
	row.show_offbudget = Some(1);
	let data = evaluate_report(
		&book.budget,
		&CustomReport::from_row(&row).unwrap(),
		today(),
		0,
	)
	.unwrap();
	let car = data
		.groups
		.iter()
		.find(|group| group.name == "Car purchase")
		.unwrap();
	assert_eq!(car.balances.total_debts.minor(), -50_000);
	assert_eq!(
		data.groups.first().map(|g| g.name.as_str()),
		Some("Uncategorized"),
		"asc on debts runs the smallest payments first"
	);
}

#[test]
fn r_rep_4_net_by_group_and_name_sort() {
	let book = master();
	let row = spending_report("Group", "Net", "name");
	let report = CustomReport::from_row(&row).unwrap();
	assert_eq!(report.group_by, GroupBy::Group);
	let data = evaluate_report(&book.budget, &report, today(), 0).unwrap();
	let names: Vec<_> = data
		.groups
		.iter()
		.map(|group| group.name.as_str())
		.collect();
	let mut sorted = names.clone();
	sorted.sort_by_key(|name| name.to_lowercase());
	assert_eq!(names, sorted);
	let income = data
		.groups
		.iter()
		.find(|group| group.name == "Income")
		.unwrap();
	assert_eq!(income.balances.total_totals.minor(), 600_000);
	assert_eq!(
		data.totals.total_totals,
		Money::from_minor(data.totals.total_assets.minor() + data.totals.total_debts.minor())
	);
}

#[test]
fn r_rep_4_unsupported_settings_say_so_instead_of_guessing() {
	let book = master();
	let budgeted =
		CustomReport::from_row(&spending_report("Category", "Budgeted", "desc")).unwrap();
	assert!(evaluate_report(&book.budget, &budgeted, today(), 0).is_err());
	let mut row = spending_report("Category", "Payment", "desc");
	row.conditions = Some(r#"[{"field":"notes","op":"matches","value":"^x"}]"#.to_owned());
	let regex = CustomReport::from_row(&row).unwrap();
	assert!(evaluate_report(&book.budget, &regex, today(), 0).is_err());
}

#[test]
fn r_rep_4_dashboards_keep_unknown_widgets_as_placeholders() {
	let budget = all_features();
	let pages = dashboards(&budget).unwrap();
	let names: Vec<_> = pages.iter().map(|page| page.name.as_str()).collect();
	assert_eq!(names, ["Main", "Deep dive"]);
	let deep = &pages[1];
	assert_eq!(deep.widgets.len(), 10);
	assert!(deep.widgets.iter().any(|widget| widget.content
		== WidgetContent::Placeholder {
			kind: "future-widget-card".to_owned()
		}));
	assert!(deep.widgets.iter().any(|widget| widget.content
		== WidgetContent::CustomReport(CustomReportId::new(
			"c7eaad37-4a0f-4f44-8c5e-ca30ff232dd2"
		))));
	assert!(deep.widgets.iter().any(|widget| matches!(
		&widget.content,
		WidgetContent::Markdown { align: Some(align), .. } if align == "left"
	)));
	let first = &deep.widgets[0];
	assert_eq!((first.x, first.y, first.width, first.height), (0, 0, 8, 2));
	assert!(
		pages[0]
			.widgets
			.iter()
			.all(|widget| widget.y >= 0 && widget.x + widget.width <= 12)
	);

	let placeholder = budget
		.widgets
		.values()
		.find(|widget| widget.kind.as_deref() == Some("future-widget-card"))
		.unwrap();
	let moved = place_widget(placeholder, 11, 7, 1, 1).unwrap();
	assert_eq!(
		(moved.x, moved.y, moved.width, moved.height),
		(Some(11), Some(7), Some(1), Some(1))
	);
	assert_eq!(
		(&moved.kind, &moved.meta),
		(&placeholder.kind, &placeholder.meta),
		"untouched"
	);
	let sankey = budget
		.widgets
		.values()
		.find(|widget| widget.kind.as_deref() == Some("sankey-card"))
		.unwrap();
	let clamped = place_widget(sankey, 10, 0, 1, 1).unwrap();
	assert_eq!(
		(clamped.x, clamped.width, clamped.height),
		(Some(9), Some(3), Some(3))
	);
	assert_eq!(WidgetType::Sankey.minimum_size(), (3, 3));
}
