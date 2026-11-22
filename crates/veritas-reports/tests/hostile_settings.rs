//! R-REL-7 for the report and dashboard settings a user's Actual file hands
//! Veritas: `dashboard.meta`, `custom_reports` rows and formula text parse to
//! a value or a typed refusal, and every card or report built from a value
//! that parsed answers or refuses without panicking or hanging.
//!
//! Inputs grow from the real settings in fixtures/synthetic/all-features.zip,
//! mutated the way a damaged, hand-edited or newer file can be.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, mpsc};
use std::time::Duration;

use chrono::NaiveDate;
use proptest::prelude::*;
use proptest::test_runner::{RngSeed, TestRunner};
use serde_json::{Value, json};
use support::actual::fixture;
use veritas_domain::{BudgetData, CivilDate, CustomReportRow};
use veritas_engine::EnvelopeSheet;
use veritas_reports::{
	CustomReport, Json, WidgetType, age_of_money, balance_forecast_card, budget_analysis, calendar,
	card_forecast, cash_flow_card, crossover, dashboards, evaluate_report, formula_card,
	monte_carlo_card, net_worth_card, parse_meta, read_settings, sankey_card, spending, summary,
};

fn today() -> NaiveDate {
	NaiveDate::from_ymd_opt(2026, 11, 9).unwrap()
}

struct Book {
	budget: BudgetData,
	sheet: EnvelopeSheet,
}

/// all-features loaded once, the way the app loads it, and shared by every case.
fn book() -> Arc<Book> {
	static BOOK: OnceLock<Arc<Book>> = OnceLock::new();
	BOOK.get_or_init(|| {
		let budget = fixture("all-features");
		let sheet = EnvelopeSheet::compute(&budget, CivilDate::from(today())).unwrap();
		Arc::new(Book { budget, sheet })
	})
	.clone()
}

fn config(cases: u32, seed: u64) -> ProptestConfig {
	ProptestConfig {
		cases,
		rng_seed: RngSeed::Fixed(seed),
		failure_persistence: None,
		max_shrink_iters: 2_000,
		..ProptestConfig::default()
	}
}

/// Runs `work` on its own thread and fails, naming `input`, if it doesn't
/// finish in time, so a hang is a red test instead of a stuck run.
fn within<T: Send + 'static>(input: &str, work: impl FnOnce() -> T + Send + 'static) -> T {
	let (done, finished) = mpsc::channel();
	let worker = std::thread::spawn(move || {
		let result = work();
		let _ = done.send(());
		result
	});
	match finished.recv_timeout(Duration::from_secs(20)) {
		Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => match worker.join() {
			Ok(result) => result,
			Err(panic) => {
				eprintln!("panicked on input: {input}");
				std::panic::resume_unwind(panic)
			}
		},
		Err(mpsc::RecvTimeoutError::Timeout) => {
			panic!("still running after 20 s on input: {input}")
		}
	}
}

/// Settings text exactly as all-features.zip stores it, plus the widget
/// shapes the cards read that the fixture doesn't use.
const METAS: &[(&str, &str)] = &[
	(
		"summary-card",
		r#"{"name":"Total Income (YTD)","content":"{\"type\":\"sum\",\"fontSize\":20}","timeFrame":{"start":"2024-01-01","end":"2024-12-31","mode":"yearToDate"},"conditions":[{"field":"amount","op":"gt","value":0},{"field":"account","op":"onBudget","value":""},{"field":"transfer","op":"is","value":false}],"conditionsOp":"and"}"#,
	),
	(
		"summary-card",
		r#"{"name":"Avg dining","content":"{\"type\":\"avgPerMonth\"}","conditions":[{"field":"category","op":"is","value":"fffcd498-80e7-4984-b378-87cee6b5625c","type":"id"}],"conditionsOp":"and"}"#,
	),
	(
		"calendar-card",
		r#"{"name":"Transaction Calendar","timeFrame":{"start":"2024-01-01","end":"2024-03-31","mode":"sliding-window"},"conditions":[{"field":"transfer","op":"is","value":false}],"conditionsOp":"and"}"#,
	),
	(
		"net-worth-card",
		r#"{"name":"Net worth","mode":"stacked","interval":"Monthly"}"#,
	),
	(
		"cash-flow-card",
		r#"{"name":"Cash flow","showBalance":true}"#,
	),
	(
		"spending-card",
		r#"{"name":"Spending","mode":"average","averageRange":{"mode":"last-n-months","months":6}}"#,
	),
	(
		"spending-card",
		r#"{"name":"This Month","mode":"single-month"}"#,
	),
	(
		"sankey-card",
		r#"{"name":"Where it went","mode":"spent","topNcategories":8}"#,
	),
	(
		"formula-card",
		r#"{"name":"Savings rate","formula":"=1-(SPENT/INCOME)"}"#,
	),
	(
		"formula-card",
		r#"{"name":"Groceries","formula":"=ROUND(QUERY(\"g\")/3,2)","queries":{"g":{"conditions":[{"field":"category","op":"is","value":"fffcd498-80e7-4984-b378-87cee6b5625c","type":"id"}],"timeFrame":{"start":"2026-01","end":"2026-06","mode":"static"}}},"fontSizeMode":"static","staticFontSize":24}"#,
	),
	(
		"markdown-card",
		r###"{"content":"## Budget notes\n\nPay the card **in full**.","text_align":"left"}"###,
	),
	(
		"custom-report",
		r#"{"id":"c7eaad37-4a0f-4f44-8c5e-ca30ff232dd2"}"#,
	),
	(
		"crossover-card",
		r#"{"name":"Crossover","timeFrame":{"start":"2025-01","end":"2026-10","mode":"static"},"showHiddenCategories":false}"#,
	),
	(
		"budget-analysis-card",
		r#"{"name":"Budget analysis","timeFrame":{"start":"2026-01","end":"2026-10","mode":"sliding-window"},"interval":"Monthly"}"#,
	),
	(
		"balance-forecast-card",
		r#"{"name":"Forecast","timeFrame":{"start":"2026-11","end":"2027-04","mode":"static"},"granularity":"Daily"}"#,
	),
	("age-of-money-card", r#"{"name":"Age of money"}"#),
	(
		"monte-carlo-card",
		r#"{"name":"Retirement","simulations":200,"currentAge":40,"endAge":60}"#,
	),
	(
		"future-widget-card",
		r#"{"anything":["Veritas","does","not","know","this"]}"#,
	),
];

/// What a damaged, hand-edited or newer file can hold where Actual writes a
/// well-formed value.
const HOSTILE: &[&str] = &[
	"null",
	"true",
	"0",
	"-1",
	"0.5",
	"1e308",
	"-1e308",
	"9007199254740993",
	"18446744073709551616",
	"-9223372036854775809",
	"\"\"",
	"\"0\"",
	"\"99999999\"",
	"\"20261340\"",
	"\"2026-13\"",
	"\"0000-00\"",
	"\"0000-00-00\"",
	"\"262142-12\"",
	"\"275760-09-13\"",
	"\"2026-02-30\"",
	"\"-2026-01\"",
	"\"static\"",
	"\"sliding-window\"",
	"\"full\"",
	"\"yearToDate\"",
	"\"bogus\"",
	"\"Daily\"",
	"\"Weekly\"",
	"\"Yearly\"",
	"\"=1/0\"",
	"\"{\\\"type\\\":\\\"sum\\\"\"",
	"\"🙂 Café\"",
	"\"\\u0000\\u202e\"",
	"[]",
	"{}",
	"[null]",
	"[1,[2,[3]]]",
	"{\"mode\":\"static\",\"start\":\"0000-01\",\"end\":\"262142-12\"}",
	"{\"mode\":\"sliding-window\",\"start\":\"2026-01\",\"end\":\"1900-01\"}",
	"{\"mode\":\"last-n-months\",\"months\":1e9}",
	"[{\"field\":\"amount\",\"op\":\"isbetween\",\"value\":{\"num1\":1e308}}]",
	"[{\"field\":\"date\",\"op\":\"is\",\"value\":{\"frequency\":\"daily\",\"start\":\"0100-01-01\"}}]",
	"[{\"field\":\"notes\",\"op\":\"matches\",\"value\":\"(a+)+$\"}]",
	"[{\"field\":\"category\",\"op\":\"oneOf\",\"value\":\"not a list\"}]",
	"[{\"op\":\"and\"}]",
	"10000",
	"100000000",
	"-1000000000",
];

/// Keys the cards read, so additions land where they matter.
const KEYS: &[&str] = &[
	"name",
	"content",
	"timeFrame",
	"start",
	"end",
	"mode",
	"conditions",
	"conditionsOp",
	"interval",
	"showBalance",
	"averageRange",
	"months",
	"topNcategories",
	"formula",
	"queries",
	"fontSizeMode",
	"staticFontSize",
	"showTitle",
	"id",
	"text_align",
	"showHiddenCategories",
	"granularity",
	"simulations",
	"currentAge",
	"endAge",
	"field",
	"op",
	"value",
];

fn count(value: &Value) -> usize {
	1 + match value {
		Value::Array(items) => items.iter().map(count).sum(),
		Value::Object(entries) => entries.values().map(count).sum(),
		_ => 0,
	}
}

fn node_at(value: &mut Value, at: usize) -> &mut Value {
	fn walk<'a>(value: &'a mut Value, at: &mut usize) -> Option<&'a mut Value> {
		if *at == 0 {
			return Some(value);
		}
		*at -= 1;
		match value {
			Value::Array(items) => items.iter_mut().find_map(|item| walk(item, at)),
			Value::Object(entries) => entries.values_mut().find_map(|item| walk(item, at)),
			_ => None,
		}
	}
	let mut at = at % count(value);
	walk(value, &mut at).unwrap()
}

fn hostile(index: usize) -> Value {
	serde_json::from_str(HOSTILE[index % HOSTILE.len()]).unwrap()
}

/// One structural edit: (where, what, which value).
type Edit = (usize, u8, usize);

fn apply(value: &mut Value, edits: &[Edit]) {
	for &(at, what, pick) in edits {
		let node = node_at(value, at);
		let handled = match &mut *node {
			Value::Object(entries) if what % 4 == 1 => {
				entries.insert(KEYS[pick % KEYS.len()].to_owned(), hostile(pick / 7));
				true
			}
			Value::Object(entries) if what % 4 == 2 && !entries.is_empty() => {
				let key = entries.keys().nth(pick % entries.len()).unwrap().clone();
				entries.remove(&key);
				true
			}
			Value::Array(items) if what % 4 == 3 => {
				items.push(hostile(pick));
				true
			}
			_ => false,
		};
		if !handled {
			*node = hostile(pick);
		}
	}
}

/// Text-level damage: a truncated write, a stray byte, a key Actual never
/// writes. Applied after the structural edits.
fn damage(text: &str, cut: Option<usize>) -> String {
	match cut {
		Some(at) => text.chars().take(at % (text.chars().count() + 1)).collect(),
		None => text.to_owned(),
	}
}

fn edits() -> impl Strategy<Value = Vec<Edit>> {
	prop::collection::vec((0..64usize, 0..4u8, 0..512usize), 0..4)
}

/// Builds every card the way the dashboard does for a widget of `kind`.
fn render(book: &Book, kind: WidgetType, meta: Option<&Json>) {
	let budget = &book.budget;
	let sheet = &book.sheet;
	let today = today();
	let _ = read_settings(Some(kind), meta);
	let _ = match kind {
		WidgetType::NetWorth => net_worth_card(budget, meta, today, 0).map(drop),
		WidgetType::CashFlow => cash_flow_card(budget, meta, today).map(drop),
		WidgetType::Spending => spending(budget, meta, today).map(drop),
		WidgetType::BudgetAnalysis => budget_analysis(budget, sheet, meta, today).map(drop),
		WidgetType::Crossover => crossover(budget, meta, today).map(drop),
		WidgetType::Summary => summary(budget, meta, today).map(drop),
		WidgetType::Calendar => calendar(budget, meta, today, 1).map(drop),
		WidgetType::Formula => formula_card(budget, sheet, meta, today).map(drop),
		WidgetType::Sankey => sankey_card(budget, sheet, meta, today, Some(8)).map(drop),
		WidgetType::BalanceForecast => balance_forecast_card(budget, sheet, meta, today)
			.map(drop)
			.and_then(|()| card_forecast(budget, sheet, meta, today).map(drop)),
		WidgetType::AgeOfMoney => age_of_money(budget, meta, today).map(drop),
		WidgetType::MonteCarlo => monte_carlo_card(budget, meta).map(drop),
		WidgetType::CustomReport | WidgetType::Markdown => Ok(()),
	};
}

/// Formulas the card evaluates, from Actual's docs and the D73 suite, with
/// the tokens a damaged or hand-typed formula is made of.
const FORMULAS: &[&str] = &[
	"=SUM(1, 2, 3)",
	"=1-(SPENT/INCOME)",
	"=ROUND(QUERY(\"g\")/3,2)",
	"=IF(QUERY(\"g\")<0,\"spent\",\"earned\")",
	"=BUDGET_QUERY(\"spent\",\"fffcd498-80e7-4984-b378-87cee6b5625c\",\"2026-01\",\"2026-02\")",
	"=QUERY_EXTRACT_TIMEFRAME_START(\"g\")&\"|\"&QUERY_EXTRACT_TIMEFRAME_END(\"g\")",
	"=DATE(2026,2,30)+EOMONTH(TODAY(),-1)",
	"=CONCATENATE(UPPER(\"a\"),LOWER(\"B\"),RIGHT(\"hello\",2),LEN(\"🙂\"))",
	"=AVERAGE(1,2,\"3\")*MIN(4,-5)/ABS(-0)",
	"=NOT(OR(TRUE(),FALSE()))",
	"=YEAR(1e300)+MONTH(-1)+DAY(\"x\")",
	"=10^308*10",
	"=\"a\"<\"á\"",
];

const TOKENS: &[&str] = &[
	"(",
	")",
	"(",
	")",
	",",
	";",
	"+",
	"-",
	"*",
	"/",
	"^",
	"&",
	"=",
	"<",
	">",
	"<=",
	"<>",
	"%",
	"\"",
	"\"\"",
	"{",
	"}",
	":",
	"!",
	"$",
	"1",
	"0",
	"9.5e300",
	"1e-400",
	"SUM",
	"IF",
	"QUERY",
	"BUDGET_QUERY",
	"DATE",
	"EOMONTH",
	"ROUND",
	"TRUE",
	"A1",
	"R1C1",
	"#REF!",
	" ",
	"\t",
	"\n",
	"é",
	"🙂",
	"\u{0}",
	"\\ud800",
	"\\u",
	"\\",
	"1e999",
	"-0",
];

fn formula_text() -> impl Strategy<Value = String> {
	let seeded = (
		0..FORMULAS.len(),
		prop::collection::vec((0..64usize, 0..TOKENS.len(), any::<bool>()), 0..6),
	)
		.prop_map(|(seed, edits)| {
			let mut chars: Vec<String> = FORMULAS[seed].chars().map(String::from).collect();
			for (at, token, replace) in edits {
				let at = at % (chars.len() + 1);
				if replace && at < chars.len() {
					chars[at] = TOKENS[token].to_owned();
				} else {
					chars.insert(at, TOKENS[token].to_owned());
				}
			}
			chars.concat()
		});
	let built = prop::collection::vec(0..TOKENS.len(), 0..40)
		.prop_map(|tokens| format!("={}", tokens.iter().map(|t| TOKENS[*t]).collect::<String>()));
	prop_oneof![3 => seeded, 1 => built]
}

proptest! {
	#![proptest_config(config(2_000, 0x5EED_0001))]

	#[test]
	fn json_text_from_a_damaged_file_parses_to_a_value_that_round_trips_or_an_error(
		seed in 0..METAS.len(),
		edits in edits(),
		cut in prop::option::weighted(0.3, 0..2_000usize),
		splice in prop::collection::vec((0..2_000usize, 0..TOKENS.len()), 0..3),
	) {
		let mut value: Value = serde_json::from_str(METAS[seed].1).unwrap();
		apply(&mut value, &edits);
		let mut chars: Vec<char> = damage(&value.to_string(), cut).chars().collect();
		for (at, token) in splice {
			let at = at % (chars.len() + 1);
			chars.splice(at..at, TOKENS[token].chars());
		}
		let text: String = chars.into_iter().collect();
		let input = text.clone();
		within(&input, move || {
			if let Ok(parsed) = Json::parse(&text) {
				let written = parsed.stringify();
				assert_eq!(Json::parse(&written).as_ref(), Ok(&parsed), "{written}");
			}
		});
	}
}

/// Runs `test` on `cases` inputs from `strategy`, failing with proptest's
/// shrunk input, so a test can check how many cases got past parsing.
fn run_cases<S: Strategy>(cases: u32, seed: u64, strategy: S, test: impl Fn(S::Value))
where
	S::Value: std::fmt::Debug,
{
	let mut runner = TestRunner::new(config(cases, seed));
	if let Err(failure) = runner.run(&strategy, |value| {
		test(value);
		Ok(())
	}) {
		panic!("{failure}");
	}
}

#[test]
fn widget_settings_from_a_damaged_file_read_back_or_refuse_and_every_card_answers() {
	let rendered = Arc::new(AtomicUsize::new(0));
	let strategy = (
		0..METAS.len(),
		edits(),
		prop::option::weighted(0.15, 0..2_000usize),
		prop::option::weighted(0.25, 0..METAS.len()),
	);
	run_cases(600, 0x5EED_0002, strategy, |(seed, edits, cut, as_kind)| {
		let mut value: Value = serde_json::from_str(METAS[seed].1).unwrap();
		apply(&mut value, &edits);
		let text = damage(&value.to_string(), cut);
		let kind = WidgetType::parse(METAS[as_kind.unwrap_or(seed)].0);
		let input = format!("{kind:?} {text}");
		let book = book();
		let rendered = rendered.clone();
		within(&input, move || {
			let Ok(meta) = parse_meta(Some(&text)) else {
				return;
			};
			match kind {
				Some(kind) => render(&book, kind, meta.as_ref()),
				None => drop(read_settings(None, meta.as_ref())),
			}
			rendered.fetch_add(1, Ordering::Relaxed);
		});
	});
	let rendered = rendered.load(Ordering::Relaxed);
	assert!(rendered > 400, "only {rendered} of 600 settings parsed");
}

proptest! {
	#![proptest_config(config(2_000, 0x5EED_0003))]

	#[test]
	fn formula_text_from_anywhere_is_a_value_an_error_or_a_refusal(text in formula_text()) {
		let meta = json!({
			"formula": text,
			"queries": {"g": {"conditions": [{"field": "category", "op": "is", "value": "fffcd498-80e7-4984-b378-87cee6b5625c", "type": "id"}], "timeFrame": {"start": "2026-01", "end": "2026-06", "mode": "static"}}},
		});
		let input = meta.to_string();
		let book = book();
		within(&input.clone(), move || {
			let meta = Json::parse(&input).unwrap();
			let _ = formula_card(&book.budget, &book.sheet, Some(&meta), today());
		});
	}
}

const DATES: &[Option<&str>] = &[
	None,
	Some(""),
	Some("2025-11-01"),
	Some("2026-10-31"),
	Some("2026-01"),
	Some("2026-13"),
	Some("0000-01"),
	Some("1900-01-01"),
	Some("262142-12"),
	Some("2026-02-30"),
	Some("99999999"),
	Some("🙂"),
];

const FLAGS: &[Option<i64>] = &[
	None,
	Some(0),
	Some(1),
	Some(-1),
	Some(2),
	Some(i64::MAX),
	Some(i64::MIN),
];

const REPORT_CONDITIONS: &[&str] = &[
	r#"[{"field":"category","op":"oneOf","value":["fc093a41-2d11-4d98-a211-e5db92205f23","fffcd498-80e7-4984-b378-87cee6b5625c"],"type":"id"}]"#,
	"[]",
	r#"[{"field":"amount","op":"lt","value":0,"type":"number"},{"field":"date","op":"gte","value":"2026-01-01","type":"date"}]"#,
	r##"[{"field":"notes","op":"hasTags","value":"#food","type":"string"}]"##,
];

/// One stored column: usually a value Actual writes, sometimes one it never does.
fn column(values: &'static [&'static str]) -> impl Strategy<Value = Option<String>> {
	let written = prop::sample::select(values).prop_map(|value| Some(value.to_owned()));
	let other = prop::sample::select(vec![None, Some(""), Some("bogus"), Some("🙂")])
		.prop_map(|value| value.map(str::to_owned));
	prop_oneof![6 => written, 1 => other]
}

#[test]
fn a_custom_report_row_from_a_damaged_file_loads_or_refuses_and_evaluates() {
	let evaluated = Arc::new(AtomicUsize::new(0));
	let names = (
		column(&[
			"Category",
			"Group",
			"CategoryGroup",
			"Payee",
			"Account",
			"Tag",
			"Interval",
		]),
		column(&[
			"Payment",
			"Expense",
			"Deposit",
			"Net",
			"Net Payment",
			"Net Deposit",
			"Budgeted",
		]),
		column(&["Daily", "Weekly", "Monthly", "Yearly"]),
		column(&["asc", "desc", "name", "budget"]),
		column(&["time", "total"]),
		column(&["and", "or"]),
		column(&[
			"This month",
			"Last month",
			"Last 3 months",
			"Last 6 months",
			"Last 12 months",
			"This week",
			"Last week",
			"Year to date",
			"Last year",
			"Prior year to date",
			"Current quarter",
			"Previous quarter",
			"Last 30 days",
			"All time",
		]),
	);
	let stored = (
		prop::sample::select(DATES),
		prop::sample::select(DATES),
		prop::collection::vec(prop::sample::select(FLAGS), 8),
		0..REPORT_CONDITIONS.len(),
		edits(),
		prop::option::weighted(0.15, 0..400usize),
		prop::sample::select(vec![0u32, 1, 6, 7, u32::MAX]),
	);
	run_cases(600, 0x5EED_0004, (names, stored), |(names, stored)| {
		let (group_by, balance_type, interval, sort_by, mode, conditions_op, date_range) = names;
		let (start_date, end_date, flags, conditions, edits, cut, first_day) = stored;
		let book = book();
		let base = book.budget.custom_reports.values().next().unwrap().clone();
		let mut value: Value = serde_json::from_str(REPORT_CONDITIONS[conditions]).unwrap();
		apply(&mut value, &edits);
		let row = CustomReportRow {
			start_date: start_date.map(str::to_owned),
			end_date: end_date.map(str::to_owned),
			date_range,
			mode,
			group_by,
			balance_type,
			interval,
			sort_by,
			conditions_op,
			date_static: flags[0],
			show_empty: flags[1],
			show_offbudget: flags[2],
			show_hidden: flags[3],
			show_uncategorized: flags[4],
			include_current: flags[5],
			trim_intervals: flags[6],
			show_trend_lines: flags[7],
			conditions: Some(damage(&value.to_string(), cut)),
			..base
		};
		let input = format!("{row:?} first day {first_day}");
		let evaluated = evaluated.clone();
		within(&input, move || {
			let Ok(report) = CustomReport::from_row(&row) else {
				return;
			};
			let _ = report.to_row();
			let _ = evaluate_report(&book.budget, &report, today(), first_day);
			evaluated.fetch_add(1, Ordering::Relaxed);
		});
	});
	let evaluated = evaluated.load(Ordering::Relaxed);
	assert!(evaluated > 150, "only {evaluated} of 600 rows loaded");
}

#[test]
fn every_widget_in_the_fixture_renders_after_its_settings_are_reread() {
	// The baseline the mutated cases start from: the real settings all parse
	// and every card answers, so a refusal above comes from the damage.
	let book = book();
	for (kind, text) in METAS {
		let meta = parse_meta(Some(text)).unwrap();
		if let Some(kind) = WidgetType::parse(kind) {
			let input = format!("{kind:?} {text}");
			let book = book.clone();
			within(&input, move || render(&book, kind, meta.as_ref()));
		}
	}
	assert!(dashboards(&book.budget).is_ok());
}

/// The nesting a damaged or hostile file can hold in one settings column.
/// Actual's `JSON.parse` reads it (V8's parser doesn't recurse); Veritas
/// must refuse it rather than run out of stack.
const DEEP: usize = 100_000;

#[test]
#[ignore = "helper process, spawned by the deep nesting test"]
fn parse_deeply_nested_settings() {
	if std::env::var_os("VERITAS_DEEP_JSON").is_none() {
		return;
	}
	let arrays = format!("{}{}", "[".repeat(DEEP), "]".repeat(DEEP));
	let objects = format!("{}1{}", "{\"a\":".repeat(DEEP), "}".repeat(DEEP));
	for text in [&arrays, &objects] {
		let _ = Json::parse(text);
		let _ = parse_meta(Some(text));
	}
	println!("deep-json survived");
}

#[test]
fn settings_nested_past_the_stack_are_refused_without_crashing_the_app() {
	// Known bug, left failing: `Json::parse` recurses once per level with no
	// depth limit, so a widget or report column nested 100,000 deep overflows
	// the stack. That aborts the whole process (no panic to catch at the FFI
	// boundary), and the app dies whenever the dashboard is shown.
	let output = std::process::Command::new(std::env::current_exe().unwrap())
		.args([
			"--exact",
			"parse_deeply_nested_settings",
			"--ignored",
			"--nocapture",
			"--test-threads=1",
		])
		.env("VERITAS_DEEP_JSON", "1")
		.output()
		.unwrap();
	let stdout = String::from_utf8_lossy(&output.stdout);
	assert!(
		output.status.success() && stdout.contains("deep-json survived"),
		"parsing {DEEP}-deep settings killed the process: {:?}\n{}",
		output.status,
		String::from_utf8_lossy(&output.stderr)
	);
}
