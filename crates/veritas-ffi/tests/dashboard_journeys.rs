#![allow(
	clippy::unwrap_used,
	clippy::expect_used,
	clippy::panic,
	reason = "integration tests fail by panicking"
)]

//! Dashboards and custom reports edited through the exported API the way
//! Swift edits them (R-REP-4, D73), then checked twice against Actual: its
//! own handlers making the same edits must write the same rows, and Actual
//! reading Veritas's export must see the same pages, widgets, settings and
//! reports, with saved filters selecting the same rows.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::Thread;

use serde_json::{Value, json};
use veritas_ffi::{
	CancelToken, Change, Command, CustomReportSettings, DashboardPage, DashboardWidget, EntityKind,
	ReportBalanceKind, ReportCondition, ReportGrouping, ReportIntervalUnit, ReportSortOrder,
	Session, WidgetConfig, WidgetTimeFrame, WidgetView,
};

const TODAY: &str = "2026-11-13";
const SPENDING_REPORT: &str = "c7eaad37-4a0f-4f44-8c5e-ca30ff232dd2";
const MAIN: &str = "48f79e8a-0a29-4f3c-a7a3-9c04110766ad";
const DEEP_DIVE: &str = "76608b8e-9055-432d-a3cf-0f3f1623c5a7";
const NET_WORTH_CARD: &str = "a8828813-ceda-452a-8de4-4a24a78c531e";
const SPENDING_CARD: &str = "07fb54b8-dd18-40de-8bca-381826688a49";
const DINING_CARD: &str = "5658fbe9-b6d0-428e-a4ff-77b3eb54803b";

struct Unpark(Thread);

impl Wake for Unpark {
	fn wake(self: Arc<Self>) {
		self.0.unpark();
	}
}

fn block_on<F: Future>(future: F) -> F::Output {
	let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
	let mut context = Context::from_waker(&waker);
	let mut future = pin!(future);
	loop {
		if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
			return output;
		}
		std::thread::park();
	}
}

struct Scratch(PathBuf);

impl Scratch {
	fn new(label: &str) -> Self {
		static NEXT: AtomicUsize = AtomicUsize::new(0);
		let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
			"ffi-dashboards-{label}-{}-{}",
			std::process::id(),
			NEXT.fetch_add(1, Ordering::Relaxed)
		));
		let _ = std::fs::remove_dir_all(&path);
		std::fs::create_dir_all(&path).unwrap();
		Scratch(path)
	}

	fn join(&self, name: &str) -> PathBuf {
		self.0.join(name)
	}
}

impl Drop for Scratch {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.0);
	}
}

fn root() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn text(path: &Path) -> String {
	path.display().to_string()
}

fn perform(session: &Session, command: Command) -> Change {
	session.perform(command).unwrap()
}

fn created(change: &Change, kind: EntityKind) -> String {
	change
		.entities
		.iter()
		.find(|entity| entity.kind == kind && entity.id.is_some())
		.and_then(|entity| entity.id.clone())
		.unwrap()
}

fn dashboards(session: &Session) -> Vec<DashboardPage> {
	block_on(session.dashboards(TODAY.to_owned(), CancelToken::new())).unwrap()
}

fn widget(pages: &[DashboardPage], id: &str) -> DashboardWidget {
	pages
		.iter()
		.flat_map(|page| &page.widgets)
		.find(|widget| widget.id == id)
		.cloned()
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

fn condition(field: &str, op: &str, value: &str, options: Option<&str>) -> ReportCondition {
	ReportCondition {
		field: field.to_owned(),
		op: op.to_owned(),
		value: value.to_owned(),
		options: options.map(str::to_owned),
	}
}

fn oracle() -> Option<PathBuf> {
	let oracle = root().join("tools/actual-oracle");
	if oracle.join("node_modules").exists() {
		return Some(oracle);
	}
	assert!(
		std::env::var_os("VERITAS_REQUIRE_ORACLE").is_none(),
		"VERITAS_REQUIRE_ORACLE is set but tools/actual-oracle/node_modules is missing"
	);
	eprintln!("skipping the Actual side: run `cargo xtask oracle setup`");
	None
}

fn node(oracle: &Path, args: &[&str]) -> Vec<u8> {
	let output = std::process::Command::new("node")
		.current_dir(oracle)
		.arg("src/main.ts")
		.args(args)
		.args(["--today", TODAY])
		.output()
		.expect("node runs");
	assert!(
		output.status.success(),
		"the oracle failed on {args:?}: {}",
		String::from_utf8_lossy(&output.stderr)
	);
	output.stdout
}

fn rows<'a>(dump: &'a Value, table: &str) -> BTreeMap<String, &'a Value> {
	dump["tables"][table]["rows"]
		.as_array()
		.unwrap()
		.iter()
		.map(|row| (row["id"].as_str().unwrap().to_owned(), row))
		.collect()
}

fn group_name(grouping: ReportGrouping) -> &'static str {
	match grouping {
		ReportGrouping::Category => "Category",
		ReportGrouping::Group => "Group",
		ReportGrouping::CategoryGroup => "CategoryGroup",
		ReportGrouping::Payee => "Payee",
		ReportGrouping::Account => "Account",
		ReportGrouping::Tag => "Tag",
		ReportGrouping::Interval => "Interval",
	}
}

fn balance_name(kind: ReportBalanceKind) -> &'static str {
	match kind {
		ReportBalanceKind::Payment => "Payment",
		ReportBalanceKind::Deposit => "Deposit",
		ReportBalanceKind::Net => "Net",
		ReportBalanceKind::NetPayment => "Net Payment",
		ReportBalanceKind::NetDeposit => "Net Deposit",
		ReportBalanceKind::Budgeted => "Budgeted",
	}
}

fn sort_name(order: ReportSortOrder) -> &'static str {
	match order {
		ReportSortOrder::Ascending => "asc",
		ReportSortOrder::Descending => "desc",
		ReportSortOrder::Name => "name",
		ReportSortOrder::Budget => "budget",
	}
}

fn interval_name(unit: ReportIntervalUnit) -> &'static str {
	match unit {
		ReportIntervalUnit::Daily => "Daily",
		ReportIntervalUnit::Weekly => "Weekly",
		ReportIntervalUnit::Monthly => "Monthly",
		ReportIntervalUnit::Yearly => "Yearly",
	}
}

/// The entity Actual's report editor sends to `report/create`. The
/// conditions go in as text so their key order survives.
fn create_entity(settings: &CustomReportSettings, conditions: &str) -> String {
	json!({
		"name": settings.name,
		"startDate": settings.start_date,
		"endDate": settings.end_date,
		"isDateStatic": settings.date_static,
		"dateRange": settings.date_range,
		"mode": if settings.total_mode { "total" } else { "time" },
		"groupBy": group_name(settings.group_by),
		"tagScope": {"mode": "all"},
		"interval": interval_name(settings.interval),
		"balanceType": balance_name(settings.balance_type),
		"sortBy": sort_name(settings.sort_by),
		"showEmpty": settings.show_empty,
		"showOffBudget": settings.show_off_budget,
		"showHiddenCategories": settings.show_hidden,
		"includeCurrentInterval": settings.include_current,
		"showUncategorized": settings.show_uncategorized,
		"trimIntervals": settings.trim_intervals,
		"showTrendLines": false,
		"graphType": settings.graph_type,
		"conditions": "CONDITIONS",
		"conditionsOp": if settings.conditions_or { "or" } else { "and" },
	})
	.to_string()
	.replace("\"CONDITIONS\"", conditions)
}

fn same_conditions(theirs: &Value, ours: &[ReportCondition], context: &str) {
	let theirs = theirs.as_array().unwrap();
	assert_eq!(theirs.len(), ours.len(), "{context}: condition count");
	for (theirs, ours) in theirs.iter().zip(ours) {
		assert_eq!(theirs["field"], ours.field.as_str(), "{context}");
		assert_eq!(theirs["op"], ours.op.as_str(), "{context}");
		let value: Value = serde_json::from_str(&ours.value).unwrap();
		assert_eq!(theirs["value"], value, "{context}");
		let options = ours.options.as_deref().map_or(Value::Null, |options| {
			serde_json::from_str(options).unwrap()
		});
		assert_eq!(
			theirs.get("options").cloned().unwrap_or(Value::Null),
			options,
			"{context}"
		);
	}
}

/// What Actual reads from a widget's meta is what Veritas shows as its
/// settings.
fn same_widget(theirs: &Value, ours: &DashboardWidget, page: &str) {
	let id = &ours.id;
	assert_eq!(theirs["type"], ours.kind.as_str(), "{id}");
	assert_eq!(theirs["dashboard_page_id"], page, "{id}");
	for (key, value) in [
		("x", ours.x),
		("y", ours.y),
		("width", ours.width),
		("height", ours.height),
	] {
		assert_eq!(theirs[key], value, "{id} {key}");
	}
	let meta = &theirs["meta"];
	let config = &ours.config;
	let string = |key: &str| meta.get(key).and_then(Value::as_str).map(str::to_owned);
	assert_eq!(string("name"), config.name, "{id} name");
	assert_eq!(string("mode"), config.mode, "{id} mode");
	assert_eq!(string("text_align"), config.text_align, "{id} text_align");
	assert_eq!(string("formula"), config.formula, "{id} formula");
	match ours.kind.as_str() {
		"markdown-card" => assert_eq!(string("content"), config.content, "{id}"),
		"custom-report" => assert_eq!(string("id"), config.report_id, "{id}"),
		"summary-card" => {
			let content: Value =
				serde_json::from_str(&string("content").unwrap_or_else(|| "{}".to_owned()))
					.unwrap();
			assert_eq!(
				content["type"].as_str().map(str::to_owned),
				config.summary,
				"{id} summary"
			);
		}
		_ => {}
	}
	let frame = config
		.time_frame
		.as_ref()
		.map(|frame| json!({"start": frame.start, "end": frame.end, "mode": frame.mode}));
	let stored = meta
		.get("timeFrame")
		.filter(|frame| frame.is_object())
		.map(|frame| {
			json!({
				"start": frame["start"].as_str().unwrap_or_default(),
				"end": frame["end"].as_str().unwrap_or_default(),
				"mode": frame["mode"].as_str().unwrap_or("sliding-window"),
			})
		});
	assert_eq!(stored, frame, "{id} timeFrame");
	same_conditions(
		meta.get("conditions").unwrap_or(&json!([])),
		&config.conditions,
		id,
	);
	assert_eq!(
		meta.get("conditionsOp").and_then(Value::as_str) == Some("or"),
		config.conditions_or,
		"{id} conditionsOp"
	);
}

fn row_json(row: &veritas_reports::QueryRow) -> Value {
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

fn sorted(values: &[Value]) -> Vec<String> {
	let mut keys: Vec<String> = values.iter().map(Value::to_string).collect();
	keys.sort();
	keys
}

#[test]
fn a_dashboard_rework_reads_back_in_actual_as_veritas_wrote_it() {
	let scratch = Scratch::new("rework");
	let zip = root().join("fixtures/synthetic/all-features.zip");
	let session = block_on(Session::import_actual_zip(
		text(&zip),
		text(&scratch.join("veritas")),
		CancelToken::new(),
	))
	.unwrap();
	let before = dashboards(&session);
	let mut ops: Vec<Value> = Vec::new();

	let page = created(
		&perform(
			&session,
			Command::CreateDashboard {
				name: "Planning".to_owned(),
			},
		),
		EntityKind::DashboardPage,
	);
	ops.push(json!({"op": "createDashboardPage", "id": page, "name": "Planning"}));

	let mut add = |page: &str, kind: &str, config: WidgetConfig, meta: Value| {
		let id = created(
			&perform(
				&session,
				Command::AddWidget {
					page_id: page.to_owned(),
					kind: kind.to_owned(),
					x: 0,
					y: 20,
					width: 4,
					height: if kind == "sankey-card" { 3 } else { 2 },
					config,
				},
			),
			EntityKind::Widget,
		);
		let mut op = json!({
			"op": "addDashboardWidget", "id": id, "page": page, "type": kind,
			"x": 0, "y": 20, "width": 4, "height": if kind == "sankey-card" { 3 } else { 2 },
		});
		match meta {
			Value::String(text) => op["metaText"] = Value::String(text),
			other => op["meta"] = other,
		}
		ops.push(op);
		id
	};
	let net_worth = add(&page, "net-worth-card", blank(), Value::Null);
	let note = add(
		&page,
		"markdown-card",
		blank(),
		json!({"content": "### Text Widget\n\nEdit this widget to change the **markdown** content."}),
	);
	let report_card = add(
		&page,
		"custom-report",
		WidgetConfig {
			report_id: Some(SPENDING_REPORT.to_owned()),
			..blank()
		},
		json!({"id": SPENDING_REPORT}),
	);
	let summary = add(
		MAIN,
		"summary-card",
		WidgetConfig {
			name: Some("Big buys".to_owned()),
			time_frame: Some(WidgetTimeFrame {
				mode: "static".to_owned(),
				start: "2026-01".to_owned(),
				end: "2026-06".to_owned(),
			}),
			conditions: vec![condition("amount", "lt", "-5000", None)],
			summary: Some("avgPerTransact".to_owned()),
			..blank()
		},
		Value::String(
			r#"{"name":"Big buys","conditions":[{"field":"amount","op":"lt","value":-5000,"type":"number"}],"timeFrame":{"start":"2026-01","end":"2026-06","mode":"static"},"content":"{\"type\":\"avgPerTransact\"}"}"#
				.to_owned(),
		),
	);
	let sankey = add(MAIN, "sankey-card", blank(), Value::Null);

	let mut dining = widget(&before, DINING_CARD).config;
	dining.name = Some("Dining".to_owned());
	dining
		.conditions
		.push(condition("notes", "contains", "\"lunch\"", None));
	dining.conditions_or = true;
	dining.time_frame = Some(WidgetTimeFrame {
		mode: "static".to_owned(),
		start: "2026-01".to_owned(),
		end: "2026-06".to_owned(),
	});
	perform(
		&session,
		Command::SetWidgetConfig {
			widget_id: DINING_CARD.to_owned(),
			config: dining,
		},
	);
	ops.push(json!({
		"op": "updateWidgetMeta", "id": DINING_CARD,
		"appendConditionsText": r#"[{"field":"notes","op":"contains","value":"lunch","type":"string"}]"#,
		"setText": r#"{"name":"Dining","conditionsOp":"or","timeFrame":{"start":"2026-01","end":"2026-06","mode":"static"}}"#,
	}));

	perform(
		&session,
		Command::PlaceWidget {
			widget_id: NET_WORTH_CARD.to_owned(),
			x: 3,
			y: 30,
			width: 5,
			height: 3,
		},
	);
	ops.push(
		json!({"op": "moveWidget", "id": NET_WORTH_CARD, "x": 3, "y": 30, "width": 5, "height": 3}),
	);

	perform(
		&session,
		Command::RemoveWidget {
			widget_id: SPENDING_CARD.to_owned(),
		},
	);
	ops.push(json!({"op": "removeWidget", "id": SPENDING_CARD}));

	perform(
		&session,
		Command::RenameDashboard {
			page_id: page.clone(),
			name: "Plans".to_owned(),
		},
	);
	ops.push(json!({"op": "renameDashboardPage", "id": page, "name": "Plans"}));

	let spending = block_on(session.custom_report(
		SPENDING_REPORT.to_owned(),
		TODAY.to_owned(),
		CancelToken::new(),
	))
	.unwrap()
	.settings;
	let mut new_report = spending.clone();
	new_report.name = "Big outflows".to_owned();
	new_report.conditions = vec![condition(
		"amount",
		"gte",
		"2000",
		Some(r#"{"outflow":true}"#),
	)];
	let outflows = created(
		&perform(
			&session,
			Command::CreateCustomReport {
				settings: new_report.clone(),
			},
		),
		EntityKind::CustomReport,
	);
	ops.push(json!({
		"op": "createCustomReport", "id": outflows,
		"reportText": create_entity(
			&new_report,
			r#"[{"field":"amount","op":"gte","value":2000,"options":{"outflow":true},"type":"number"}]"#,
		),
	}));

	let mut edited = spending.clone();
	edited.name = "Food spending".to_owned();
	edited
		.conditions
		.push(condition("amount", "lt", "-1000", None));
	edited.conditions_or = true;
	perform(&session, Command::SaveCustomReport { settings: edited });
	ops.push(json!({
		"op": "updateCustomReport", "id": SPENDING_REPORT,
		"appendConditionsText": r#"[{"field":"amount","op":"lt","value":-1000,"type":"number"}]"#,
		"setText": r#"{"name":"Food spending","conditionsOp":"or"}"#,
	}));

	perform(
		&session,
		Command::DeleteDashboard {
			page_id: DEEP_DIVE.to_owned(),
		},
	);
	ops.push(json!({"op": "deleteDashboardPage", "id": DEEP_DIVE}));

	let after = dashboards(&session);
	let mut names: Vec<&str> = after.iter().map(|page| page.name.as_str()).collect();
	names.sort_unstable();
	assert_eq!(names, ["Main", "Plans"]);
	assert!(matches!(
		widget(&after, &summary).view,
		WidgetView::Summary { .. }
	));
	assert!(matches!(
		widget(&after, &sankey).view,
		WidgetView::Sankey { .. }
	));
	assert!(matches!(
		widget(&after, &net_worth).view,
		WidgetView::NetWorth { .. }
	));
	assert!(matches!(
		widget(&after, &note).view,
		WidgetView::Markdown { .. }
	));
	assert!(matches!(
		widget(&after, &report_card).view,
		WidgetView::CustomReport { .. }
	));
	let report_names: Vec<String> = session
		.custom_reports()
		.unwrap()
		.into_iter()
		.map(|item| item.name)
		.collect();
	assert_eq!(
		report_names,
		["Big outflows", "Food spending", "Net worth over time"]
	);

	let exported = scratch.join("veritas.zip");
	block_on(session.export_actual_zip(text(&exported), CancelToken::new())).unwrap();
	let ours_reports: Vec<CustomReportSettings> = [outflows.as_str(), SPENDING_REPORT]
		.iter()
		.map(|id| {
			block_on(session.custom_report((*id).to_owned(), TODAY.to_owned(), CancelToken::new()))
				.unwrap()
				.settings
		})
		.collect();
	session.close().unwrap();

	let Some(oracle) = oracle() else {
		return;
	};

	// Actual's own handlers, making the same edits from the same file.
	let ops_file = scratch.join("ops.json");
	std::fs::write(&ops_file, json!({"version": 1, "ops": ops}).to_string()).unwrap();
	let theirs_zip = scratch.join("actual.zip");
	node(
		&oracle,
		&[
			"apply",
			&text(&zip),
			&text(&ops_file),
			"--out",
			&text(&theirs_zip),
		],
	);
	let ours_dump: Value =
		serde_json::from_slice(&node(&oracle, &["dump", &text(&exported), "--raw-only"])).unwrap();
	let theirs_dump: Value =
		serde_json::from_slice(&node(&oracle, &["dump", &text(&theirs_zip), "--raw-only"]))
			.unwrap();
	for table in ["dashboard", "dashboard_pages"] {
		assert_eq!(
			rows(&ours_dump, table),
			rows(&theirs_dump, table),
			"Actual writes the same {table} rows"
		);
	}
	let ours_rows = rows(&ours_dump, "custom_reports");
	let theirs_rows = rows(&theirs_dump, "custom_reports");
	assert_eq!(ours_rows[&outflows], theirs_rows[&outflows], "a new report");
	for column in ["name", "conditions", "conditions_op"] {
		assert_eq!(
			ours_rows[SPENDING_REPORT][column], theirs_rows[SPENDING_REPORT][column],
			"a saved report's {column}"
		);
	}

	// Actual reading Veritas's export.
	let readback: Value =
		serde_json::from_slice(&node(&oracle, &["dashboards", &text(&exported)])).unwrap();
	let mut their_pages: Vec<(String, String)> = readback["pages"]
		.as_array()
		.unwrap()
		.iter()
		.map(|page| {
			(
				page["id"].as_str().unwrap().to_owned(),
				page["name"].as_str().unwrap().to_owned(),
			)
		})
		.collect();
	their_pages.sort();
	let mut our_pages: Vec<(String, String)> = after
		.iter()
		.map(|page| (page.id.clone(), page.name.clone()))
		.collect();
	our_pages.sort();
	assert_eq!(their_pages, our_pages);
	let their_widgets: BTreeMap<String, &Value> = readback["widgets"]
		.as_array()
		.unwrap()
		.iter()
		.map(|widget| (widget["id"].as_str().unwrap().to_owned(), widget))
		.collect();
	let widget_count: usize = after.iter().map(|page| page.widgets.len()).sum();
	assert_eq!(their_widgets.len(), widget_count);
	for page in &after {
		for widget in &page.widgets {
			same_widget(their_widgets[&widget.id], widget, &page.id);
		}
	}

	let budget = veritas_store::load(
		&rusqlite::Connection::open(scratch.join("veritas").join("db.sqlite")).unwrap(),
	)
	.unwrap()
	.data;
	let today = chrono::NaiveDate::parse_from_str(TODAY, "%Y-%m-%d").unwrap();
	let mut requests = Vec::new();
	let mut expected = Vec::new();
	for ours in &ours_reports {
		let theirs = readback["reports"]
			.as_array()
			.unwrap()
			.iter()
			.find(|report| report["id"] == ours.id.as_str())
			.unwrap();
		assert_eq!(theirs["name"], ours.name.as_str());
		assert_eq!(theirs["graphType"], json!(ours.graph_type));
		same_conditions(&theirs["conditions"], &ours.conditions, &ours.name);
		assert_eq!(theirs["conditionsOp"] == "or", ours.conditions_or);

		let report = veritas_reports::CustomReport::from_row(
			&budget.custom_reports[&veritas_domain::CustomReportId::new(ours.id.as_str())],
		)
		.unwrap();
		let (start, end) = veritas_reports::report_range(&report, &budget, today, 0).unwrap();
		let (assets, debts) = veritas_reports::report_rows(&budget, &report, &start, &end).unwrap();
		requests.push(json!({
			"id": ours.id,
			"startDate": start,
			"endDate": end,
			"interval": interval_name(ours.interval),
			"conditions": theirs["conditions"],
			"conditionsOp": theirs["conditionsOp"],
		}));
		expected.push((ours.id.clone(), assets, debts));
	}
	let requests_file = scratch.join("requests.json");
	std::fs::write(&requests_file, Value::Array(requests).to_string()).unwrap();
	let selected: Value = serde_json::from_slice(&node(
		&oracle,
		&["report-rows", &text(&exported), &text(&requests_file)],
	))
	.unwrap();
	for (id, assets, debts) in expected {
		for (side, ours) in [("assets", assets), ("debts", debts)] {
			let ours: Vec<Value> = ours.iter().map(row_json).collect();
			assert_eq!(
				sorted(&ours),
				sorted(selected[id.as_str()][side].as_array().unwrap()),
				"the saved filter on {id} selects the same {side} rows"
			);
		}
	}
}
