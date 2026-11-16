#![allow(
	clippy::unwrap_used,
	clippy::expect_used,
	clippy::panic,
	reason = "integration tests fail by panicking"
)]

//! The native dashboard (D73, R-REP-4) through the exported API, beyond the
//! fixture: Actual's own card code run on a budget Actual and Veritas both
//! edited, with tagged, split, transfer and edge-dated rows and filters that
//! use tags and repeating dates, at edge todays; every dashboard edit walked
//! back and forth through undo, redo and a reopen; and settings saves that
//! keep keys Veritas doesn't know exactly as Actual would.

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
	AgeGranularity, AgeTrend, CancelToken, Change, Command, DashboardPage, DashboardWidget,
	EntityKind, FormulaValue, RuleCondition, RuleConditionOp, RuleField, RuleValue, Session,
	SplitLineDraft, TransactionDraft, WidgetConfig, WidgetTimeFrame, WidgetView,
};

const MAIN: &str = "48f79e8a-0a29-4f3c-a7a3-9c04110766ad";
const DEEP_DIVE: &str = "76608b8e-9055-432d-a3cf-0f3f1623c5a7";
const CHECKING: &str = "0a1c000f-c328-42b8-8966-a4be59a2504b";
const SAVINGS: &str = "66260e81-beca-4d3d-8ab4-00c8dc2c2ded";
const VISA: &str = "2cf26d00-3bc3-4cd9-a05c-5d9552dd0e63";
const BROKERAGE: &str = "3128ef6e-0073-4914-b33a-9808cc093fa4";
const GROCERIES: &str = "fc093a41-2d11-4d98-a211-e5db92205f23";
const DINING_OUT: &str = "fffcd498-80e7-4984-b378-87cee6b5625c";
const VACATION: &str = "0c2998b7-d4f3-42d6-aca6-ef72ed399ca8";
const HOUSEHOLD: &str = "e2dcc389-9fe5-4b1f-93d6-0099cd592c17";
const SKYAIR: &str = "23f67807-675f-4225-a732-ac37528cf2aa";
const TRATTORIA: &str = "595fbfc6-8cb7-4e42-b56a-a74acd76be9b";
const SPENDING_REPORT: &str = "c7eaad37-4a0f-4f44-8c5e-ca30ff232dd2";
const SPENDING_CARD: &str = "07fb54b8-dd18-40de-8bca-381826688a49";

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
			"ffi-widgets-{label}-{}-{}",
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

fn all_features() -> PathBuf {
	root().join("fixtures/synthetic/all-features.zip")
}

fn text(path: &Path) -> String {
	path.display().to_string()
}

fn import(zip: &Path, into: &Path) -> Arc<Session> {
	block_on(Session::import_actual_zip(
		text(zip),
		text(into),
		CancelToken::new(),
	))
	.unwrap()
}

fn open(dir: &Path) -> Arc<Session> {
	block_on(Session::open(text(dir), CancelToken::new())).unwrap()
}

fn export(session: &Session, zip: &Path) {
	block_on(session.export_actual_zip(text(zip), CancelToken::new())).unwrap();
}

fn perform(session: &Session, command: Command) -> Change {
	session.perform(command).unwrap()
}

fn created(change: &Change, kind: EntityKind) -> String {
	change
		.entities
		.iter()
		.find(|e| e.kind == kind && e.id.is_some())
		.and_then(|e| e.id.clone())
		.unwrap()
}

fn dashboards(session: &Session, today: &str) -> Vec<DashboardPage> {
	block_on(session.dashboards(today.to_owned(), CancelToken::new())).unwrap()
}

fn widget<'a>(pages: &'a [DashboardPage], id: &str) -> &'a DashboardWidget {
	pages
		.iter()
		.flat_map(|p| &p.widgets)
		.find(|w| w.id == id)
		.unwrap_or_else(|| panic!("no widget {id}"))
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

/// A filter as the Mac's editor builds one, from Actual's field and op
/// names.
fn condition(field: &str, op: &str, value: Value) -> RuleCondition {
	let field = match field {
		"notes" => RuleField::Notes,
		"amount" => RuleField::Amount,
		"account" => RuleField::Account,
		"date" => RuleField::Date,
		other => panic!("no test field {other}"),
	};
	let op = match op {
		"hasTags" => RuleConditionOp::HasTags,
		"hasAnyTag" => RuleConditionOp::HasAnyTag,
		"isNot" => RuleConditionOp::IsNot,
		"lt" => RuleConditionOp::Lt,
		"gte" => RuleConditionOp::Gte,
		"lte" => RuleConditionOp::Lte,
		other => panic!("no test op {other}"),
	};
	let value = match (field, value) {
		(RuleField::Account, Value::String(id)) => RuleValue::Id { id: Some(id) },
		(RuleField::Date, Value::String(date)) => RuleValue::Date { date },
		(_, Value::String(text)) => RuleValue::Text { text },
		(_, Value::Number(amount)) => RuleValue::Amount {
			amount: amount.as_i64().unwrap(),
		},
		(_, other) => panic!("no test value {other}"),
	};
	RuleCondition { field, op, value }
}

fn frame(mode: &str, start: &str, end: &str) -> Option<WidgetTimeFrame> {
	Some(WidgetTimeFrame {
		mode: mode.to_owned(),
		start: start.to_owned(),
		end: end.to_owned(),
	})
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

fn node(oracle: &Path, args: &[&str], today: &str, seed: u32) -> Vec<u8> {
	let output = std::process::Command::new("node")
		.current_dir(oracle)
		.arg("src/main.ts")
		.args(args)
		.args(["--today", today, "--seed", &seed.to_string()])
		.output()
		.expect("node runs");
	assert!(
		output.status.success(),
		"the oracle failed on {args:?}: {}",
		String::from_utf8_lossy(&output.stderr)
	);
	output.stdout
}

fn apply(oracle: &Path, zip: &Path, ops: Value, out: &Path, seed: u32) {
	let file = out.with_extension("ops.json");
	std::fs::write(&file, json!({"version": 1, "ops": ops}).to_string()).unwrap();
	node(
		oracle,
		&["apply", &text(zip), &text(&file), "--out", &text(out)],
		"2026-11-13",
		seed,
	);
}

fn raw_dump(oracle: &Path, zip: &Path) -> Value {
	serde_json::from_slice(&node(
		oracle,
		&["dump", &text(zip), "--raw-only"],
		"2026-11-13",
		1,
	))
	.unwrap()
}

fn table<'a>(dump: &'a Value, name: &str) -> BTreeMap<String, &'a Value> {
	dump["tables"][name]["rows"]
		.as_array()
		.unwrap()
		.iter()
		.map(|row| (row["id"].as_str().unwrap().to_owned(), row))
		.collect()
}

/// A number as the oracle writes it: integers as JSON, anything else as its
/// JS text.
#[allow(clippy::cast_possible_truncation, clippy::float_cmp)]
fn number(value: f64) -> Value {
	if value.is_finite() && value.fract() == 0.0 && value.abs() < 9e15 {
		json!(value as i64)
	} else if value.is_nan() {
		json!("NaN")
	} else if value.is_infinite() {
		json!(if value > 0.0 { "Infinity" } else { "-Infinity" })
	} else {
		json!(value.to_string())
	}
}

fn optional(value: Option<f64>) -> Value {
	value.map_or(Value::Null, number)
}

fn month_label(month: &veritas_ffi::Month) -> String {
	chrono::NaiveDate::from_ymd_opt(month.year, month.month, 1)
		.unwrap()
		.format("%b %Y")
		.to_string()
}

fn month_text(month: &veritas_ffi::Month) -> String {
	format!("{}-{:02}", month.year, month.month)
}

/// What Actual's card code returns, rebuilt from what Veritas shows. `None`
/// for the cards whose whole output `tests/widgets.rs` in veritas-reports
/// compares instead.
fn as_actual(view: &WidgetView) -> Option<Value> {
	Some(match view {
		WidgetView::NetWorth {
			range,
			points,
			net_worth,
			change,
			lowest,
			highest,
			has_negative,
			accounts,
			series,
			..
		} => json!({
			"start": range.start, "end": range.end,
			"points": points.iter().map(|point| point.net_worth).collect::<Vec<_>>(),
			"hasNegative": has_negative, "netWorth": net_worth, "totalChange": change,
			"lowestNetWorth": lowest, "highestNetWorth": highest, "accounts": accounts,
			"details": points.iter().map(|point| json!({
				"assets": point.assets, "debt": point.debt, "change": point.change,
			})).collect::<Vec<_>>(),
			"stacked": series.iter().map(|stack| json!({
				"id": stack.account_id, "name": stack.name, "points": stack.points,
				"color": format!("var(--color-chartQual{})", stack.color + 1),
			})).collect::<Vec<_>>(),
		}),
		WidgetView::CashFlow {
			range,
			income,
			expense,
			change,
		} => {
			assert_eq!(*change, income + expense);
			json!({"start": range.start, "end": range.end, "income": income, "expense": expense})
		}
		WidgetView::Spending {
			compare,
			compare_to,
			difference,
			..
		} => json!({
			"compare": month_text(compare), "compareTo": month_text(compare_to),
			"difference": difference,
		}),
		WidgetView::Formula { value, .. } => match value {
			FormulaValue::Error { error } => json!({"error": error.message}),
			FormulaValue::Number { value } => json!({"value": number(*value), "type": "number"}),
			FormulaValue::Text { text } => json!({"value": text, "type": "string"}),
			FormulaValue::Boolean { value } => json!({"value": value, "type": "boolean"}),
			FormulaValue::Empty => json!({"value": null, "type": "null"}),
		},
		WidgetView::MonteCarlo {
			success_percent,
			start_age,
			end_age,
			..
		} => json!({
			"successPercent": number(*success_percent),
			"startAge": number(*start_age), "endAge": number(*end_age),
		}),
		WidgetView::Summary {
			start,
			end,
			value,
			dividend,
			divisor,
		} => json!({
			"start": start, "end": end,
			"total": number(*value), "dividend": number(*dividend), "divisor": number(*divisor),
		}),
		WidgetView::Calendar { months } => json!({"calendarData": months.iter().map(|m| json!({
			"start": m.start, "end": m.end,
			"totalIncome": m.total_income, "totalExpense": m.total_expense,
			"data": m.days.iter().map(|d| json!({
				"date": d.date, "incomeValue": d.income, "expenseValue": d.expense,
				"incomeSize": number(d.income_size), "expenseSize": number(d.expense_size),
			})).collect::<Vec<_>>(),
		})).collect::<Vec<_>>()}),
		WidgetView::AgeOfMoney {
			granularity,
			points,
			current_days,
			trend,
			insufficient_data,
			..
		} => {
			let label = |period: &str| {
				let date = chrono::NaiveDate::parse_from_str(period, "%Y-%m-%d").unwrap();
				match granularity {
					AgeGranularity::Monthly => date.format("%b %Y").to_string(),
					_ => date.format("%b %-d, %Y").to_string(),
				}
			};
			json!({
				"graphData": points.iter().map(|p| json!({"date": label(&p.period), "ageOfMoney": p.days})).collect::<Vec<_>>(),
				"currentAge": current_days,
				"trend": match trend { AgeTrend::Up => "up", AgeTrend::Down => "down", AgeTrend::Stable => "stable" },
				"insufficientData": insufficient_data,
			})
		}
		WidgetView::BudgetAnalysis {
			start,
			end,
			months,
			total_budgeted,
			total_spent,
			total_overspending_adjustment,
			final_overspending_adjustment,
		} => json!({
			"intervalData": months.iter().map(|m| json!({
				"date": format!("{}-{:02}", m.month.year, m.month.month),
				"budgeted": m.budgeted, "spent": m.spent, "balance": m.balance,
				"overspendingAdjustment": m.overspending_adjustment,
			})).collect::<Vec<_>>(),
			"startDate": start, "endDate": end,
			"totalBudgeted": total_budgeted, "totalSpent": total_spent,
			"totalOverspendingAdjustment": total_overspending_adjustment,
			"finalOverspendingAdjustment": final_overspending_adjustment,
		}),
		WidgetView::Crossover {
			start,
			end,
			points,
			crossover,
			last_known_balance,
			last_known_monthly_income,
			last_known_monthly_expenses,
			historical_return,
			years_to_retire,
			target_monthly_income,
			target_nest_egg,
		} => json!({
			"graphData": {
				"data": points.iter().map(|p| {
					let mut entry = json!({
						"x": month_label(&p.month),
						"investmentIncome": number(p.investment_income),
						"expenses": number(p.expenses),
						"nestEgg": number(p.nest_egg),
					});
					if p.projection {
						entry["adjustedExpenses"] = optional(p.adjusted_expenses);
						entry["isProjection"] = json!(true);
					}
					entry
				}).collect::<Vec<_>>(),
				"start": format!("{}-{:02}", start.year, start.month),
				"end": format!("{}-{:02}", end.year, end.month),
				"crossoverXLabel": crossover.as_ref().map(month_label),
			},
			"lastKnownBalance": number(*last_known_balance),
			"lastKnownMonthlyIncome": number(*last_known_monthly_income),
			"lastKnownMonthlyExpenses": number(*last_known_monthly_expenses),
			"historicalReturn": optional(*historical_return),
			"yearsToRetire": optional(*years_to_retire),
			"targetMonthlyIncome": optional(*target_monthly_income),
			"targetNestEgg": optional(*target_nest_egg),
		}),
		_ => return None,
	})
}

/// The days `summary-spreadsheet.ts` turns the card's range into: the first
/// of the start's month, and the end's month end, or today in the current
/// month.
fn summary_days(result: &Value, today: &str) -> (String, String) {
	let month = |text: &str| text[..7].to_owned();
	let start = format!("{}-01", month(result["start"].as_str().unwrap()));
	let end = month(result["end"].as_str().unwrap());
	let end = if end == month(today) {
		today.to_owned()
	} else {
		let first = chrono::NaiveDate::parse_from_str(&format!("{end}-01"), "%Y-%m-%d").unwrap();
		let next = first.checked_add_months(chrono::Months::new(1)).unwrap();
		(next - chrono::Days::new(1)).to_string()
	};
	(start, end)
}

/// The part of Actual's card output that `as_actual` rebuilds.
fn theirs_comparable(kind: &str, result: &Value, today: &str) -> Value {
	let data = &result["data"];
	match kind {
		"summary-card" => {
			let (start, end) = summary_days(result, today);
			json!({
				"start": start, "end": end,
				"total": data["total"], "dividend": data["dividend"], "divisor": data["divisor"],
			})
		}
		"calendar-card" => json!({"calendarData": data["calendarData"]}),
		"net-worth-card" => {
			let mut shown = data.clone();
			let fields = shown.as_object_mut().unwrap();
			fields.remove("startDate");
			fields.remove("endDate");
			fields.insert("start".to_owned(), result["start"].clone());
			fields.insert("end".to_owned(), result["end"].clone());
			shown
		}
		"cash-flow-card" => json!({
			"start": result["start"], "end": result["end"],
			"income": result["income"], "expense": result["expense"],
		}),
		"spending-card" => json!({
			"compare": result["compare"], "compareTo": result["compareTo"],
			"difference": result["difference"],
		}),
		"formula-card" if !result["error"].is_null() => json!({"error": result["error"]}),
		"formula-card" => json!({"value": result["value"], "type": result["type"]}),
		"monte-carlo-card" => json!({
			"successPercent": result["successPercent"],
			"startAge": result["startAge"], "endAge": result["endAge"],
		}),
		_ => data.clone(),
	}
}

fn entry(id: &str, account: &str, date: &str, amount: i64) -> TransactionDraft {
	TransactionDraft {
		id: Some(id.to_owned()),
		account_id: account.to_owned(),
		date: date.to_owned(),
		amount,
		payee_id: None,
		payee_name: None,
		category_id: None,
		notes: None,
		cleared: true,
		splits: Vec::new(),
	}
}

/// all-features after a few months of real use in Actual: tagged trips,
/// a split dinner whose lines carry their own tags, a transfer, refunds,
/// and rows on month, quarter, leap-free February and year edges, plus
/// cards configured in Actual with tag, repeating-date and amount filters
/// and every edge time frame.
fn edited_in_actual(oracle: &Path, scratch: &Scratch) -> PathBuf {
	let trip = |date: &str, amount: i64, notes: &str| json!({"date": date, "amount": amount, "payee": SKYAIR, "category": VACATION, "notes": notes});
	let card = |id: &str, kind: &str, y: i64, meta: Value| {
		json!({"op": "addDashboardWidget", "id": id, "page": DEEP_DIVE, "type": kind,
			"x": 0, "y": 40 + y, "width": 4, "height": if kind == "crossover-card" || kind == "budget-analysis-card" { 3 } else { 2 },
			"meta": meta})
	};
	let tagged =
		|tags: &str| json!([{"field": "notes", "op": "hasTags", "value": tags, "type": "string"}]);
	let last_days = json!({"start": "2025-11-30", "frequency": "monthly", "interval": 1,
		"patterns": [{"type": "day", "value": -1}], "skipWeekend": true, "weekendSolveMode": "before",
		"endMode": "never"});
	let fortnights =
		json!({"start": "2025-12-29", "frequency": "weekly", "interval": 2, "endMode": "never"});
	let ops = json!([
		{"op": "addTransactions", "account": VISA, "transactions": [
			trip("2025-12-31", -48_000, "Flights #trip"),
			trip("2026-01-01", -12_500, "Hotel #trip #work"),
			trip("2026-02-28", -7_725, "Taxi #trip#work"),
			trip("2026-03-01", 4_000, "Refund ##trip"),
			trip("2026-03-31", -1_999, "Bags #Trip"),
			{"date": "2026-06-30", "amount": -9_000, "payee": TRATTORIA, "notes": "Team dinner #work",
				"subtransactions": [
					{"amount": -6_000, "category": DINING_OUT, "notes": "mine #work"},
					{"amount": -3_000, "category": HOUSEHOLD, "notes": "#trip leftovers"},
				]},
			{"date": "2026-09-30", "amount": -2_500, "payee": TRATTORIA, "category": DINING_OUT, "notes": "lunch"},
		]},
		{"op": "addTransactions", "account": CHECKING, "transactions": [
			{"date": "2026-01-31", "amount": -30_000, "payee": {"transferTo": SAVINGS}, "notes": "save #goal"},
			{"date": "2025-11-30", "amount": 1_500, "payee": TRATTORIA, "category": DINING_OUT, "notes": "refund"},
		]},
		card("cw-trip-sum", "summary-card", 0, json!({"name": "Trips", "content": "{\"type\":\"sum\"}",
			"conditions": tagged("#trip"), "conditionsOp": "and",
			"timeFrame": {"start": "2025-12", "end": "2026-03", "mode": "static"}})),
		card("cw-any-tag", "summary-card", 1, json!({"content": "{\"type\":\"avgPerTransact\"}",
			"conditions": [{"field": "notes", "op": "hasAnyTag", "value": "#trip #work", "type": "string"}],
			"conditionsOp": "and", "timeFrame": {"mode": "yearToDate"}})),
		card("cw-month-ends", "summary-card", 2, json!({"content": "{\"type\":\"sum\"}",
			"conditions": [{"field": "date", "op": "is", "value": last_days, "type": "date"}],
			"conditionsOp": "and", "timeFrame": {"mode": "lastYear"}})),
		card("cw-share", "summary-card", 3, json!({"content": format!("{{\"type\":\"percentage\",\"divisorConditions\":{},\"divisorConditionsOp\":\"or\"}}",
			json!([{"field": "amount", "op": "lt", "value": 0, "type": "number"}])),
			"conditions": tagged("#work"), "conditionsOp": "or", "timeFrame": {"mode": "priorYearToDate"}})),
		card("cw-fortnights", "summary-card", 4, json!({"content": "{\"type\":\"avgPerMonth\"}",
			"conditions": [{"field": "date", "op": "isapprox", "value": fortnights, "type": "date"},
				{"field": "amount", "op": "isbetween", "value": {"num1": 1000, "num2": 50000}, "type": "number", "options": {"outflow": true}}],
			"conditionsOp": "and", "timeFrame": {"mode": "previousQuarter"}})),
		card("cw-quarter", "summary-card", 5, json!({"content": "{\"type\":\"avgPerYear\"}",
			"timeFrame": {"mode": "currentQuarter"}})),
		card("cw-last-month", "summary-card", 6, json!({"timeFrame": {"mode": "lastMonth"}})),
		card("cw-calendar", "calendar-card", 7, json!({"conditions": tagged("#trip"), "conditionsOp": "and",
			"timeFrame": {"start": "2025-12", "end": "2026-03", "mode": "static"}})),
		card("cw-calendar-year-end", "calendar-card", 8, json!({
			"timeFrame": {"start": "2025-12-15", "end": "2026-01-15", "mode": "static"}})),
		card("cw-age-daily", "age-of-money-card", 9, json!({"granularity": "daily",
			"timeFrame": {"start": "2026-01", "end": "2026-03", "mode": "static"},
			"conditions": [{"field": "account", "op": "oneOf", "value": [CHECKING, VISA], "type": "id"}], "conditionsOp": "and"})),
		card("cw-age-weekly", "age-of-money-card", 10, json!({"granularity": "weekly",
			"timeFrame": {"mode": "yearToDate"}})),
		card("cw-analysis", "budget-analysis-card", 11, json!({"showHiddenCategories": true,
			"timeFrame": {"start": "2025-12", "end": "2026-03", "mode": "static"},
			"conditions": [{"field": "category", "op": "oneOf", "value": [VACATION, DINING_OUT], "type": "id"}], "conditionsOp": "and"})),
		card("cw-analysis-quarter", "budget-analysis-card", 12, json!({"timeFrame": {"mode": "previousQuarter"}})),
		card("cw-net-worth-stacked", "net-worth-card", 20, json!({"mode": "stacked",
			"timeFrame": {"mode": "yearToDate"}})),
		card("cw-net-worth", "net-worth-card", 14, json!({"interval": "Weekly",
			"conditions": tagged("#trip"), "conditionsOp": "and",
			"timeFrame": {"start": "2025-12", "end": "2026-03", "mode": "static"}})),
		card("cw-cash-flow", "cash-flow-card", 15, json!({"timeFrame": {"mode": "lastYear"},
			"conditions": [{"field": "amount", "op": "gt", "value": -50_000, "type": "number"}], "conditionsOp": "and"})),
		card("cw-spending-average", "spending-card", 16, json!({"mode": "average",
			"conditions": [{"field": "account", "op": "isNot", "value": VISA, "type": "id"}], "conditionsOp": "and"})),
		card("cw-formula", "formula-card", 17, json!({"formula": "=ROUND(QUERY(\"trips\") / 3, 2)",
			"queries": {"trips": {"conditions": tagged("#trip"), "conditionsOp": "and",
				"timeFrame": {"start": "2025-12", "end": "2026-03", "mode": "static"}}}})),
		card("cw-formula-error", "formula-card", 18, json!({"formula": "=1/0"})),
		card("cw-monte-carlo", "monte-carlo-card", 19, json!({"simulationCount": 1000,
			"currentAge": 40, "targetAge": 90,
			"pots": [{"id": "linked", "accountId": BROKERAGE}, {"id": "cash", "startingBalance": 5_000_000}],
			"spendingPhases": [{"id": "all", "annualWithdrawal": 2_500_000}]})),
		card("cw-crossover", "crossover-card", 13, json!({"incomeAccountIds": [BROKERAGE, SAVINGS],
			"expenseCategoryIds": [GROCERIES, DINING_OUT, VACATION], "safeWithdrawalRate": 0.03,
			"projectionType": "median", "expectedContribution": 25_000,
			"timeFrame": {"start": "2025-12", "end": "2026-09", "mode": "static"}})),
	]);
	let out = scratch.join("edited-in-actual.zip");
	apply(oracle, &all_features(), ops, &out, 7);
	out
}

/// Every card on Veritas's export, run through Actual's card code with the
/// meta Veritas wrote, against what Veritas's own dashboard shows.
fn same_cards_as_actual(oracle: &Path, session: &Session, zip: &Path, today: &str) -> usize {
	let ours = dashboards(session, today);
	let dump = raw_dump(oracle, zip);
	let rows = table(&dump, "dashboard");
	let mut requests = Vec::new();
	let mut expected = Vec::new();
	for widget in ours.iter().flat_map(|p| &p.widgets) {
		let native = [
			"summary-card",
			"calendar-card",
			"age-of-money-card",
			"budget-analysis-card",
			"crossover-card",
			"net-worth-card",
			"cash-flow-card",
			"spending-card",
			"formula-card",
			"monte-carlo-card",
		];
		let Some(shown) = as_actual(&widget.view) else {
			assert!(
				!native.contains(&widget.kind.as_str()),
				"{} {} on {today} isn't drawn: {:?}",
				widget.kind,
				widget.id,
				widget.view
			);
			continue;
		};
		let row = rows[&widget.id];
		let meta: Value = row["meta"]
			.as_str()
			.map_or(Value::Null, |text| serde_json::from_str(text).unwrap());
		requests.push(
			json!({"id": widget.id, "type": widget.kind, "meta": meta, "firstDayOfWeekIdx": "0"}),
		);
		expected.push((widget.id.clone(), widget.kind.clone(), meta, shown));
	}
	let file = zip.with_extension(format!("{today}.widgets.json"));
	std::fs::write(&file, Value::Array(requests).to_string()).unwrap();
	let theirs: Value = serde_json::from_slice(&node(
		oracle,
		&["widgets", &text(zip), &text(&file)],
		today,
		1,
	))
	.unwrap();
	for (id, kind, meta, shown) in &expected {
		let result = &theirs[id.as_str()];
		assert!(
			kind == "formula-card" || result.get("error").is_none(),
			"Actual's {kind} failed on {meta}: {result}"
		);
		assert_eq!(
			shown,
			&theirs_comparable(kind, result, today),
			"{kind} {id} on {today} with {meta}"
		);
	}
	expected.len()
}

#[test]
fn every_native_card_matches_actuals_card_code_on_a_budget_both_apps_edited() {
	let scratch = Scratch::new("edited");
	let Some(oracle) = oracle() else {
		return;
	};
	let actual_made = edited_in_actual(&oracle, &scratch);
	let session = import(&actual_made, &scratch.join("veritas"));

	let mut weekend = entry("weekend-trip", VISA, "2026-05-02", -22_000);
	weekend.payee_id = Some(SKYAIR.to_owned());
	weekend.notes = Some("Weekend away #trip".to_owned());
	weekend.splits = vec![
		SplitLineDraft {
			id: None,
			amount: -15_000,
			payee_id: None,
			payee_name: None,
			category_id: Some(VACATION.to_owned()),
			notes: Some("room #trip".to_owned()),
		},
		SplitLineDraft {
			id: None,
			amount: -7_000,
			payee_id: None,
			payee_name: None,
			category_id: Some(DINING_OUT.to_owned()),
			notes: Some("#work meals".to_owned()),
		},
	];
	perform(
		&session,
		Command::AddTransaction {
			transaction: weekend,
		},
	);
	let mut year_end = entry("year-end", CHECKING, "2025-12-31", -4_200);
	year_end.payee_id = Some(TRATTORIA.to_owned());
	year_end.category_id = Some(DINING_OUT.to_owned());
	year_end.notes = Some("NYE #Trip".to_owned());
	perform(
		&session,
		Command::AddTransaction {
			transaction: year_end,
		},
	);
	let mut trips = widget(&dashboards(&session, "2026-11-13"), "cw-trip-sum")
		.config
		.clone();
	trips
		.conditions
		.push(condition("date", "gte", json!("2025-12-01")));
	trips
		.conditions
		.push(condition("date", "lte", json!("2026-05-31")));
	trips.time_frame = frame("static", "2025-11", "2026-06");
	trips.summary = Some("avgPerMonth".to_owned());
	perform(
		&session,
		Command::SetWidgetConfig {
			widget_id: "cw-trip-sum".to_owned(),
			config: trips,
		},
	);
	perform(
		&session,
		Command::AddWidget {
			page_id: MAIN.to_owned(),
			kind: "calendar-card".to_owned(),
			x: 0,
			y: 60,
			width: 4,
			height: 2,
			config: WidgetConfig {
				conditions: vec![
					condition("notes", "hasAnyTag", json!("#work #goal")),
					condition("account", "isNot", json!(VISA)),
				],
				conditions_or: true,
				time_frame: frame("sliding-window", "2026-01", "2026-03"),
				..blank()
			},
		},
	);
	perform(
		&session,
		Command::AddWidget {
			page_id: MAIN.to_owned(),
			kind: "summary-card".to_owned(),
			x: 4,
			y: 60,
			width: 4,
			height: 2,
			config: WidgetConfig {
				name: Some("Work share".to_owned()),
				summary: Some("percentage".to_owned()),
				conditions: vec![condition("notes", "hasTags", json!("#work"))],
				time_frame: frame("full", "2025-11-01", "2026-11-13"),
				..blank()
			},
		},
	);

	let zip = scratch.join("veritas.zip");
	export(&session, &zip);
	let mut compared = 0;
	for today in ["2026-01-01", "2026-03-31", "2026-04-01", "2026-11-13"] {
		compared += same_cards_as_actual(&oracle, &session, &zip, today);
	}
	assert!(
		compared >= 4 * 20,
		"every native card on both pages, at each today ({compared})"
	);
}

/// The same cards in a 0-place currency (D63): Actual's card code reads raw
/// integers whatever the currency, so Veritas must never rescale a card.
#[test]
fn every_native_card_matches_actuals_card_code_in_a_zero_place_currency() {
	let scratch = Scratch::new("yen");
	let Some(oracle) = oracle() else {
		return;
	};
	let card = |id: &str, kind: &str, y: i64, meta: Value| {
		json!({"op": "addDashboardWidget", "id": id, "page": DEEP_DIVE, "type": kind,
			"x": 0, "y": 60 + 3 * y, "width": 4, "height": 3, "meta": meta})
	};
	let spring = json!({"start": "2026-02", "end": "2026-05", "mode": "static"});
	let ops = json!([
		{"op": "setPreference", "id": "defaultCurrencyCode", "value": "JPY"},
		{"op": "addTransactions", "account": CHECKING, "transactions": [
			{"date": "2026-03-02", "amount": -1_234, "payee": TRATTORIA, "category": DINING_OUT},
			{"date": "2026-03-15", "amount": -98_765, "payee": SKYAIR, "category": VACATION},
			{"date": "2026-04-01", "amount": -7, "category": GROCERIES},
			{"date": "2026-04-30", "amount": -50_001, "payee": {"transferTo": SAVINGS}},
		]},
		{"op": "setBudgetAmount", "month": "2026-04", "category": GROCERIES, "amount": 33_333},
		card("yen-summary", "summary-card", 0, json!({"content": "{\"type\":\"avgPerMonth\"}", "timeFrame": spring})),
		card("yen-calendar", "calendar-card", 1, json!({"timeFrame": spring})),
		card("yen-age", "age-of-money-card", 2, json!({"timeFrame": spring})),
		card("yen-analysis", "budget-analysis-card", 3, json!({"timeFrame": spring})),
		card("yen-crossover", "crossover-card", 4, json!({"incomeAccountIds": [BROKERAGE],
			"expenseCategoryIds": [GROCERIES, DINING_OUT], "timeFrame": spring})),
		card("yen-net-worth", "net-worth-card", 5, json!({"mode": "stacked", "timeFrame": spring})),
		card("yen-cash-flow", "cash-flow-card", 6, json!({"timeFrame": spring})),
		card("yen-spending", "spending-card", 7, json!({"mode": "budget"})),
		card("yen-formula", "formula-card", 8, json!({"formula": "=BUDGET_QUERY(\"budgeted\", {\"fc093a41-2d11-4d98-a211-e5db92205f23\"}, \"2026-04\", \"2026-04\")"})),
		card("yen-monte-carlo", "monte-carlo-card", 9, json!({"simulationCount": 1000,
			"currentAge": 50, "targetAge": 80, "pots": [{"id": "linked", "accountId": CHECKING}],
			"spendingPhases": [{"id": "all", "annualWithdrawal": 1_000_000}]})),
	]);
	let actual_made = scratch.join("yen.zip");
	apply(&oracle, &all_features(), ops, &actual_made, 11);
	let session = import(&actual_made, &scratch.join("veritas"));
	assert_eq!(
		session
			.overview()
			.unwrap()
			.number_format
			.unwrap()
			.currency
			.as_deref(),
		Some("JPY")
	);
	let pages = dashboards(&session, "2026-11-13");
	assert!(
		matches!(
			widget(&pages, "yen-formula").view,
			WidgetView::Formula {
				value: FormulaValue::Number { value },
				..
			} if value == 333.33
		),
		"BUDGET_QUERY reads the yen budget at 2 places, as Actual does: {:?}",
		widget(&pages, "yen-formula").view
	);
	let zip = scratch.join("veritas.zip");
	export(&session, &zip);
	let mut compared = 0;
	for today in ["2026-04-15", "2026-11-13"] {
		compared += same_cards_as_actual(&oracle, &session, &zip, today);
	}
	assert!(
		compared >= 2 * 10,
		"every yen card at each today ({compared})"
	);
}

/// Everything the dashboard editor shows, as text: `WidgetView` holds
/// floats, so it compares through `Debug`.
fn editor(session: &Session) -> String {
	let reports: Vec<_> = session
		.custom_reports()
		.unwrap()
		.into_iter()
		.map(|item| {
			let view = block_on(session.custom_report(
				item.id.clone(),
				"2026-11-13".to_owned(),
				CancelToken::new(),
			))
			.unwrap();
			(item, view.settings)
		})
		.collect();
	format!("{:#?}\n{reports:#?}", dashboards(session, "2026-11-13"))
}

#[test]
fn every_dashboard_edit_is_one_undo_step_and_survives_redo_and_a_reopen() {
	let scratch = Scratch::new("undo");
	let dir = scratch.join("veritas");
	let session = import(&all_features(), &dir);
	let mut shown = vec![editor(&session)];
	let mut step = |command: Command| -> Change {
		let change = perform(&session, command);
		let now = editor(&session);
		assert_ne!(&now, shown.last().unwrap(), "every edit changes the editor");
		shown.push(now);
		change
	};

	let page = created(
		&step(Command::CreateDashboard {
			name: "Trips".to_owned(),
		}),
		EntityKind::DashboardPage,
	);
	let card = created(
		&step(Command::AddWidget {
			page_id: page.clone(),
			kind: "summary-card".to_owned(),
			x: 0,
			y: 0,
			width: 4,
			height: 2,
			config: WidgetConfig {
				name: Some("Trips".to_owned()),
				conditions: vec![condition("notes", "hasTags", json!("#trip"))],
				..blank()
			},
		}),
		EntityKind::Widget,
	);
	let mut config = blank();
	config.name = Some("All trips".to_owned());
	config.summary = Some("avgPerTransact".to_owned());
	config.conditions = vec![
		condition("notes", "hasTags", json!("#trip")),
		condition("amount", "lt", json!(0)),
	];
	config.time_frame = frame("lastYear", "", "");
	step(Command::SetWidgetConfig {
		widget_id: card.clone(),
		config,
	});
	step(Command::PlaceWidget {
		widget_id: card.clone(),
		x: 8,
		y: 1,
		width: 4,
		height: 3,
	});
	step(Command::RenameDashboard {
		page_id: page.clone(),
		name: "Travel".to_owned(),
	});
	let spending = block_on(session.custom_report(
		SPENDING_REPORT.to_owned(),
		"2026-11-13".to_owned(),
		CancelToken::new(),
	))
	.unwrap()
	.settings;
	let mut report = spending.clone();
	report.name = "Trip spending".to_owned();
	report.conditions = vec![condition("notes", "hasTags", json!("#trip"))];
	report.conditions_or = false;
	let report_id = created(
		&step(Command::CreateCustomReport { settings: report }),
		EntityKind::CustomReport,
	);
	step(Command::AddWidget {
		page_id: page.clone(),
		kind: "custom-report".to_owned(),
		x: 0,
		y: 3,
		width: 4,
		height: 2,
		config: WidgetConfig {
			report_id: Some(report_id),
			..blank()
		},
	});
	let mut filtered = spending;
	filtered
		.conditions
		.push(condition("amount", "lt", json!(-1000)));
	filtered.conditions_or = true;
	step(Command::SaveCustomReport { settings: filtered });
	step(Command::RemoveWidget {
		widget_id: SPENDING_CARD.to_owned(),
	});
	step(Command::DeleteDashboard {
		page_id: DEEP_DIVE.to_owned(),
	});

	let steps = shown.len() - 1;
	for n in (0..steps).rev() {
		session.undo().unwrap();
		assert_eq!(editor(&session), shown[n], "undoing step {}", n + 1);
	}
	for (n, expected) in shown.iter().enumerate().skip(1) {
		session.redo().unwrap();
		assert_eq!(&editor(&session), expected, "redoing step {n}");
	}
	session.close().unwrap();
	let reopened = open(&dir);
	assert_eq!(&editor(&reopened), shown.last().unwrap(), "after a reopen");
	assert!(
		dashboards(&reopened, "2026-11-13")
			.iter()
			.all(|page| page.id != DEEP_DIVE)
	);
}

#[test]
fn a_settings_save_keeps_keys_veritas_does_not_know_exactly_as_actuals_save_does() {
	// A card Actual configured, with keys from a newer Actual and nested
	// values Veritas never reads. Veritas changes its name, filters and
	// time frame; Actual makes the same change with its own handler. The
	// stored meta text must come out byte for byte the same.
	let scratch = Scratch::new("meta");
	let Some(oracle) = oracle() else {
		return;
	};
	let content = r#"{"type":"percentage","divisorConditions":[{"field":"amount","op":"lt","value":0,"type":"number"}],"divisorConditionsOp":"and","futureInner":["keep"]}"#;
	let quoted = |text: &str| serde_json::to_string(text).unwrap();
	let meta_text = format!(
		r##"{{"name":"Trips","futureKey":{{"nested":[1,2.5,-0.125,1e+21,"x\"y\\z",null,true]}},"comparisonMode":"previousYear","content":{},"emoji":{},"timeFrame":{{"start":"2026-01","end":"2026-03","mode":"static"}},"conditions":[{{"field":"notes","op":"hasTags","value":"#trip","type":"string","customName":"Trip tag"}}],"conditionsOp":"and","zeroAndBig":[0,12345678901234567000,0.1]}}"##,
		quoted(content),
		quoted("café ☕ 😀\u{2028}line\u{0001}"),
	);
	let base = scratch.join("base.zip");
	apply(
		&oracle,
		&all_features(),
		json!([{"op": "addDashboardWidget", "id": "meta-card", "page": MAIN, "type": "summary-card",
			"x": 0, "y": 50, "width": 4, "height": 2, "metaText": meta_text}]),
		&base,
		7,
	);
	let before = table(&raw_dump(&oracle, &base), "dashboard")["meta-card"]["meta"]
		.as_str()
		.unwrap()
		.to_owned();

	let session = import(&base, &scratch.join("veritas"));
	let mut config = widget(&dashboards(&session, "2026-11-13"), "meta-card")
		.config
		.clone();
	assert_eq!(config.conditions.len(), 1);
	config.name = Some("Trips abroad".to_owned());
	config
		.conditions
		.push(condition("amount", "lt", json!(-1000)));
	config.conditions_or = true;
	config.time_frame = frame("static", "2025-12", "2026-06");
	config.summary = Some("avgPerMonth".to_owned());
	perform(
		&session,
		Command::SetWidgetConfig {
			widget_id: "meta-card".to_owned(),
			config: config.clone(),
		},
	);
	let ours_zip = scratch.join("veritas.zip");
	export(&session, &ours_zip);
	let ours = table(&raw_dump(&oracle, &ours_zip), "dashboard")["meta-card"]["meta"]
		.as_str()
		.unwrap()
		.to_owned();

	let theirs_zip = scratch.join("actual.zip");
	let set_text = format!(
		r#"{{"name":"Trips abroad","conditionsOp":"or","timeFrame":{{"start":"2025-12","end":"2026-06","mode":"static"}},"content":{}}}"#,
		quoted(&content.replace("percentage", "avgPerMonth")),
	);
	apply(
		&oracle,
		&base,
		json!([{"op": "updateWidgetMeta", "id": "meta-card",
			"appendConditionsText": r#"[{"field":"amount","op":"lt","value":-1000,"type":"number"}]"#,
			"setText": set_text}]),
		&theirs_zip,
		8,
	);
	let theirs = table(&raw_dump(&oracle, &theirs_zip), "dashboard")["meta-card"]["meta"]
		.as_str()
		.unwrap()
		.to_owned();
	assert_eq!(
		ours, theirs,
		"Veritas's save against Actual's, from\n{before}"
	);
	for kept in [
		r#""futureKey":{"nested":[1,2.5,-0.125,1e+21,"x\"y\\z",null,true]}"#,
		r#""comparisonMode":"previousYear""#,
		r#""customName":"Trip tag""#,
		r#"\"futureInner\":[\"keep\"]"#,
	] {
		assert!(ours.contains(kept), "{kept} kept in {ours}");
	}

	perform(
		&session,
		Command::SetWidgetConfig {
			widget_id: "meta-card".to_owned(),
			config,
		},
	);
	session.undo().unwrap();
	session.undo().unwrap();
	let restored = widget(&dashboards(&session, "2026-11-13"), "meta-card")
		.config
		.clone();
	assert_eq!(restored.name.as_deref(), Some("Trips"));
	let undone_zip = scratch.join("undone.zip");
	export(&session, &undone_zip);
	assert_eq!(
		table(&raw_dump(&oracle, &undone_zip), "dashboard")["meta-card"]["meta"],
		json!(before),
		"undo puts the original text back byte for byte"
	);
}

#[test]
fn dashboards_are_listed_in_actuals_order() {
	// Actual lists dashboard pages in the order its query returns them,
	// which for this table is the order they were created, and opens the
	// first one. A page created later whose id happens to sort first must
	// not jump ahead of Main in Veritas, and a page Veritas creates goes
	// last in both apps.
	let scratch = Scratch::new("page-order");
	let Some(oracle) = oracle() else {
		return;
	};
	let zip = scratch.join("pages.zip");
	apply(
		&oracle,
		&all_features(),
		json!([
			{"op": "createDashboardPage", "id": "0a000000-0000-4000-8000-000000000001", "name": "Taxes"},
			{"op": "createDashboardPage", "id": "ff000000-0000-4000-8000-000000000002", "name": "Zoo"},
		]),
		&zip,
		7,
	);
	let theirs = |zip: &Path| -> Vec<String> {
		serde_json::from_slice::<Value>(&node(
			&oracle,
			&["dashboards", &text(zip)],
			"2026-11-13",
			1,
		))
		.unwrap()["pages"]
			.as_array()
			.unwrap()
			.iter()
			.map(|page| page["name"].as_str().unwrap().to_owned())
			.collect()
	};
	let ours = |session: &Session| -> Vec<String> {
		dashboards(session, "2026-11-13")
			.into_iter()
			.map(|page| page.name)
			.collect()
	};
	assert_eq!(theirs(&zip), ["Main", "Deep dive", "Taxes", "Zoo"]);
	let dir = scratch.join("veritas");
	let session = import(&zip, &dir);
	assert_eq!(ours(&session), theirs(&zip));

	let taxes = dashboards(&session, "2026-11-13")[2].id.clone();
	perform(&session, Command::DeleteDashboard { page_id: taxes });
	for name in ["Budget", "Annual"] {
		perform(
			&session,
			Command::CreateDashboard {
				name: name.to_owned(),
			},
		);
	}
	let expected = ["Main", "Deep dive", "Zoo", "Budget", "Annual"];
	assert_eq!(ours(&session), expected);
	let edited = scratch.join("edited.zip");
	export(&session, &edited);
	assert_eq!(theirs(&edited), expected, "Actual agrees on the export");
	session.close().unwrap();
	assert_eq!(ours(&open(&dir)), expected, "a reopen keeps the order");
}

#[test]
fn adding_and_moving_a_widget_clamp_its_frame_the_same_way() {
	// A frame dragged above or left of the grid, past its right edge, or
	// below the type's minimum size lands on the grid, whether the widget is
	// new or moved.
	let scratch = Scratch::new("clamp");
	let session = import(&all_features(), &scratch.join("veritas"));
	let main = dashboards(&session, "2026-11-13")[0].id.clone();
	let moved = created(
		&perform(
			&session,
			Command::AddWidget {
				page_id: main.clone(),
				kind: "net-worth-card".to_owned(),
				x: 0,
				y: 30,
				width: 4,
				height: 2,
				config: blank(),
			},
		),
		EntityKind::Widget,
	);
	for ((x, y, width, height), expected) in [
		((-3, -2, 1, 1), (0, 0, 3, 2)),
		((11, -5, 4, 1), (8, 0, 4, 2)),
		((-1, 7, 20, 5), (0, 7, 12, 5)),
	] {
		let added = created(
			&perform(
				&session,
				Command::AddWidget {
					page_id: main.clone(),
					kind: "net-worth-card".to_owned(),
					x,
					y,
					width,
					height,
					config: blank(),
				},
			),
			EntityKind::Widget,
		);
		perform(
			&session,
			Command::PlaceWidget {
				widget_id: moved.clone(),
				x,
				y,
				width,
				height,
			},
		);
		let pages = dashboards(&session, "2026-11-13");
		for id in [&added, &moved] {
			let card = widget(&pages, id);
			assert_eq!(
				(card.x, card.y, card.width, card.height),
				expected,
				"{x}, {y}, {width}x{height}"
			);
		}
	}
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
fn saved_filters_select_the_same_rows_as_actual_on_a_budget_both_apps_edited() {
	// The filter engine on rows the fixture lacks: tags on split lines but
	// not their parent, a tag glued to another, a refund carrying `##trip`,
	// a capitalized tag, a transfer, and dates on month, quarter and year
	// edges, filtered by tags, repeating dates with a weekend rule, amount
	// ranges with outflow and inflow options, and payees on split lines.
	let scratch = Scratch::new("filters");
	let Some(oracle) = oracle() else {
		return;
	};
	let actual_made = edited_in_actual(&oracle, &scratch);
	let dir = scratch.join("veritas");
	let session = import(&actual_made, &dir);
	let mut extra = entry("glued", VISA, "2026-07-31", -3_300);
	extra.payee_id = Some(TRATTORIA.to_owned());
	extra.category_id = Some(DINING_OUT.to_owned());
	extra.notes = Some("#work#trip tapas".to_owned());
	perform(&session, Command::AddTransaction { transaction: extra });
	let zip = scratch.join("veritas.zip");
	export(&session, &zip);
	session.close().unwrap();

	let budget = veritas_store::load(&rusqlite::Connection::open(dir.join("db.sqlite")).unwrap())
		.unwrap()
		.data;
	let base = veritas_reports::CustomReport::from_row(
		&budget.custom_reports[&veritas_domain::CustomReportId::new(SPENDING_REPORT)],
	)
	.unwrap();
	let month_ends = json!({"start": "2025-11-30", "frequency": "monthly", "interval": 1,
		"patterns": [{"type": "day", "value": -1}], "skipWeekend": true, "weekendSolveMode": "after",
		"endMode": "never"});
	let quarterly = json!({"start": "2025-12-31", "frequency": "monthly", "interval": 3,
		"endMode": "after_n_occurrences", "endOccurrences": 3});
	let cases = [
		json!([{"field": "notes", "op": "hasTags", "value": "#trip", "type": "string"}]),
		json!([{"field": "notes", "op": "hasTags", "value": "#trip #work", "type": "string"}]),
		json!([{"field": "notes", "op": "hasAnyTag", "value": "#Trip #goal", "type": "string"}]),
		json!([{"field": "notes", "op": "contains", "value": "#trip", "type": "string"}]),
		json!([{"field": "date", "op": "is", "value": month_ends, "type": "date"}]),
		json!([{"field": "date", "op": "isapprox", "value": quarterly, "type": "date"}]),
		json!([{"field": "date", "op": "is", "value": "2026-03", "type": "date"},
			{"field": "date", "op": "is", "value": "2025-12-31", "type": "date"}]),
		json!([{"field": "amount", "op": "isbetween", "value": {"num1": 5000, "num2": 2000}, "type": "number", "options": {"outflow": true}}]),
		json!([{"field": "amount", "op": "gte", "value": 1500, "type": "number", "options": {"inflow": true}}]),
		json!([{"field": "payee", "op": "is", "value": TRATTORIA, "type": "id"},
			{"field": "category", "op": "isNot", "value": DINING_OUT, "type": "id"}]),
		json!([{"field": "transfer", "op": "is", "value": true, "type": "boolean"},
			{"field": "notes", "op": "hasTags", "value": "#goal", "type": "string"}]),
		json!([{"field": "parent", "op": "is", "value": true, "type": "boolean"}]),
	];
	let mut requests = Vec::new();
	let mut ours = Vec::new();
	for (index, conditions) in cases.iter().enumerate() {
		for or in [false, true] {
			let mut report = base.clone();
			report.id = veritas_domain::CustomReportId::new(format!("filter-{index}-{or}"));
			report.interval = veritas_reports::Interval::Daily;
			report.conditions = veritas_reports::Json::parse(&conditions.to_string())
				.unwrap()
				.as_array()
				.unwrap()
				.to_vec();
			report.conditions_or = or;
			let (start, end) = ("2025-11-01", "2026-12-31");
			requests.push(json!({
				"id": report.id.to_string(), "startDate": start, "endDate": end, "interval": "Daily",
				"conditions": conditions, "conditionsOp": if or { "or" } else { "and" },
			}));
			let (assets, debts) = veritas_reports::report_rows(&budget, &report, start, end)
				.unwrap_or_else(|error| panic!("{conditions}: {error}"));
			ours.push((report.id.to_string(), conditions.clone(), or, assets, debts));
			if conditions.as_array().unwrap().len() == 1 {
				break;
			}
		}
	}
	let file = scratch.join("requests.json");
	std::fs::write(&file, Value::Array(requests).to_string()).unwrap();
	let theirs: Value = serde_json::from_slice(&node(
		&oracle,
		&["report-rows", &text(&zip), &text(&file)],
		"2026-11-13",
		1,
	))
	.unwrap();
	let mut matched_any = false;
	for (id, conditions, or, assets, debts) in ours {
		for (side, rows) in [("assets", assets), ("debts", debts)] {
			let rows: Vec<Value> = rows.iter().map(row_json).collect();
			matched_any |= !rows.is_empty();
			assert_eq!(
				sorted(&rows),
				sorted(theirs[id.as_str()][side].as_array().unwrap()),
				"{conditions} (or: {or}) {side}"
			);
		}
	}
	assert!(matched_any);
}

#[test]
fn page_order_holds_through_deleting_the_first_page_undo_redo_reopen_and_a_round_trip() {
	// The first page is the one both apps open. Deleting it, creating and
	// undoing pages, redoing them, reopening and taking the file through
	// Actual and back must leave Veritas listing pages exactly as Actual
	// reads the same file.
	let scratch = Scratch::new("page-order-edits");
	let Some(oracle) = oracle() else {
		return;
	};
	let theirs = |zip: &Path| -> Vec<String> {
		serde_json::from_slice::<Value>(&node(
			&oracle,
			&["dashboards", &text(zip)],
			"2026-11-13",
			1,
		))
		.unwrap()["pages"]
			.as_array()
			.unwrap()
			.iter()
			.map(|page| page["name"].as_str().unwrap().to_owned())
			.collect()
	};
	let ours = |session: &Session| -> Vec<String> {
		dashboards(session, "2026-11-13")
			.into_iter()
			.map(|page| page.name)
			.collect()
	};
	let mut exports = 0;
	let mut agrees = |session: &Session, expected: &[&str], what: &str| {
		assert_eq!(ours(session), expected, "{what}");
		exports += 1;
		let zip = scratch.join(&format!("check-{exports}.zip"));
		export(session, &zip);
		assert_eq!(theirs(&zip), expected, "Actual reads the export {what}");
	};
	let create = |session: &Session, name: &str| -> String {
		created(
			&perform(
				session,
				Command::CreateDashboard {
					name: name.to_owned(),
				},
			),
			EntityKind::DashboardPage,
		)
	};

	let dir = scratch.join("veritas");
	let session = import(&all_features(), &dir);
	perform(
		&session,
		Command::DeleteDashboard {
			page_id: MAIN.to_owned(),
		},
	);
	agrees(&session, &["Deep dive"], "after deleting the first page");
	for name in ["One", "Two", "Three"] {
		create(&session, name);
	}
	agrees(
		&session,
		&["Deep dive", "One", "Two", "Three"],
		"after three creates",
	);
	session.undo().unwrap();
	session.undo().unwrap();
	agrees(&session, &["Deep dive", "One"], "after undoing two creates");
	session.redo().unwrap();
	agrees(&session, &["Deep dive", "One", "Two"], "after a redo");
	create(&session, "Four");
	agrees(
		&session,
		&["Deep dive", "One", "Two", "Four"],
		"a create after undone ones goes last",
	);
	for _ in 0..4 {
		session.undo().unwrap();
	}
	agrees(
		&session,
		&["Main", "Deep dive"],
		"undoing back to the start",
	);
	for _ in 0..2 {
		session.redo().unwrap();
	}
	agrees(
		&session,
		&["Deep dive", "One"],
		"redoing the delete and a create",
	);
	session.close().unwrap();
	let session = open(&dir);
	agrees(&session, &["Deep dive", "One"], "after a reopen");

	let ours_zip = scratch.join("veritas.zip");
	export(&session, &ours_zip);
	session.close().unwrap();
	let back = scratch.join("back.zip");
	apply(
		&oracle,
		&ours_zip,
		json!([
			{"op": "createDashboardPage", "id": "00000000-0000-4000-8000-0000000000aa", "name": "From Actual"},
		]),
		&back,
		9,
	);
	assert_eq!(theirs(&back), ["Deep dive", "One", "From Actual"]);
	let returned = import(&back, &scratch.join("returned"));
	agrees(
		&returned,
		&["Deep dive", "One", "From Actual"],
		"after a page made in Actual",
	);
	create(&returned, "Five");
	agrees(
		&returned,
		&["Deep dive", "One", "From Actual", "Five"],
		"a Veritas page after Actual's",
	);
}
