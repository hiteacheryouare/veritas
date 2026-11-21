//! R-REL-7 for importing a hostile Actual file: fixtures/synthetic/small.zip
//! with its database edited to hold what a damaged, hand-edited or
//! other-tool-written file can (NULLs where Actual always writes a value,
//! text in integer columns, amounts past 2^53, impossible dates, rule,
//! schedule and widget JSON that is truncated, nested or the wrong type).
//!
//! Each import must end in a typed error, a refusal, or a budget that opens,
//! read-only or not, and never in a panic, a hang or a half-made folder. A
//! budget that opens is then used the way the app uses it on open: the
//! budget sheet, reports, rules on every transaction, the schedule service,
//! the bill screen, and every dashboard card and custom report.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use chrono::NaiveDate;
use veritas_domain::{BudgetData, CivilDate, CoreError, PayeeId, ScheduleId};
use veritas_engine::EnvelopeSheet;
use veritas_interop::{ImportOutcome, import_actual_zip, open_budget};
use veritas_reports::{
	CustomReport, WidgetType, account_balances, age_of_money, balance_forecast_card,
	budget_analysis, calendar, cash_flow_card, classify, crossover, dashboards, evaluate_report,
	formula_card, monte_carlo_card, net_worth_card, parse_meta, sankey_card, spending, summary,
};
use veritas_rules::{
	BudgetPayees, RuleBook, RuleTransaction, advance_schedules, bill_dates, bill_status, read_bill,
};
use veritas_store::Access;

fn today() -> NaiveDate {
	NaiveDate::from_ymd_opt(2026, 11, 20).unwrap()
}

struct Scratch(PathBuf);

impl Scratch {
	fn new(label: &str) -> Self {
		static NEXT: AtomicU64 = AtomicU64::new(0);
		let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
			"hostile-import-{label}-{}-{}",
			std::process::id(),
			NEXT.fetch_add(1, Ordering::Relaxed)
		));
		let _ = std::fs::remove_dir_all(&path);
		std::fs::create_dir_all(&path).unwrap();
		Scratch(path)
	}
}

impl Drop for Scratch {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.0);
	}
}

/// small.zip's two files, read once.
fn small() -> (Vec<u8>, Vec<u8>) {
	let path = support::actual::root().join("fixtures/synthetic/small.zip");
	let mut archive = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
	let mut read = |name: &str| {
		let mut bytes = Vec::new();
		archive
			.by_name(name)
			.unwrap()
			.read_to_end(&mut bytes)
			.unwrap();
		bytes
	};
	(read("db.sqlite"), read("metadata.json"))
}

fn write_zip(path: &Path, db: &[u8], metadata: &[u8]) {
	let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
	let options = zip::write::SimpleFileOptions::default()
		.compression_method(zip::CompressionMethod::Deflated);
	for (name, bytes) in [("db.sqlite", db), ("metadata.json", metadata)] {
		zip.start_file(name, options).unwrap();
		zip.write_all(bytes).unwrap();
	}
	zip.finish().unwrap();
}

/// The rows small.zip lacks but every lived-in budget has, shaped as
/// all-features.zip stores them: a bill with its rule and next date, a
/// payee rule, a custom report on the dashboard and a formula card.
const FURNISH: &str = r#"
INSERT INTO rules (id, stage, conditions, actions, tombstone, conditions_op) VALUES
 ('rule-bill', NULL,
  '[{"op":"is","field":"description","value":"8adebb90-d2c2-481b-8403-3d35ae4fa1b9"},{"op":"is","field":"acct","value":"79664b3b-072e-442d-9082-0c1e02c242c0"},{"op":"isapprox","field":"date","value":{"start":"2025-11-03","frequency":"monthly","interval":1,"patterns":[],"skipWeekend":true,"weekendSolveMode":"after","endMode":"never"}},{"op":"is","field":"amount","value":-180000}]',
  '[{"op":"link-schedule","value":"sched-rent"}]', 0, 'and'),
 ('rule-grocer', 'pre',
  '[{"field":"imported_description","op":"contains","value":"CORNER GROCER","type":"imported_payee"}]',
  '[{"field":"description","op":"set","value":"836cf416-110a-4fdd-8190-abff53421c2c","type":"id"},{"op":"append-notes","value":" #food","type":"string"}]', 0, 'and');
INSERT INTO schedules (id, rule, active, completed, posts_transaction, tombstone, name, custom_upcoming_length, sort_order) VALUES
 ('sched-rent', 'rule-bill', 0, 0, 1, 0, 'Rent', NULL, 0.0);
INSERT INTO schedules_next_date (id, schedule_id, local_next_date, local_next_date_ts, base_next_date, base_next_date_ts, tombstone) VALUES
 ('next-rent', 'sched-rent', 20261103, 1794225600003, 20261103, 1794225600003, 0);
INSERT INTO schedules_json_paths (schedule_id, payee, account, amount, date) VALUES
 ('sched-rent', '$[0]', '$[1]', '$[3]', '$[2]');
INSERT INTO custom_reports (id, name, start_date, end_date, date_static, date_range, mode, group_by, balance_type, show_empty, show_offbudget, show_hidden, show_uncategorized, selected_categories, graph_type, conditions, conditions_op, metadata, interval, color_scheme, tombstone, include_current, sort_by, trim_intervals, show_trend_lines) VALUES
 ('report-1', 'Spending by category', '2026-08-01', '2026-10-31', 0, 'Last 3 months', 'total', 'Category', 'Payment', 0, 0, 1, 0, NULL, 'BarGraph',
  '[{"field":"category","op":"oneOf","value":["6a2a7900-aeac-482b-a592-ce709c1f4b4b","3eb3739d-9384-490c-8e5f-73a396bf02e4"],"type":"id"}]', 'and', '{}', 'Monthly', NULL, 0, 0, 'desc', 0, 0);
INSERT INTO dashboard (id, type, width, height, x, y, meta, tombstone, dashboard_page_id) VALUES
 ('widget-report', 'custom-report', 4, 2, 0, 20, '{"id":"report-1"}', 0, '48f79e8a-0a29-4f3c-a7a3-9c04110766ad'),
 ('widget-formula', 'formula-card', 4, 2, 4, 20, '{"name":"Savings rate","formula":"=ROUND(QUERY(\"g\")/3,2)","queries":{"g":{"timeFrame":{"start":"2026-08","end":"2026-10","mode":"static"}}}}', 0, '48f79e8a-0a29-4f3c-a7a3-9c04110766ad');
"#;

/// Every column of the tables the loader reads, as small.zip's schema has them.
const COLUMNS: &[(&str, &[&str])] = &[
	(
		"transactions",
		&[
			"id",
			"isParent",
			"isChild",
			"acct",
			"category",
			"amount",
			"description",
			"notes",
			"date",
			"financial_id",
			"type",
			"location",
			"error",
			"imported_description",
			"starting_balance_flag",
			"transferred_id",
			"sort_order",
			"tombstone",
			"cleared",
			"pending",
			"parent_id",
			"schedule",
			"reconciled",
			"raw_synced_data",
		],
	),
	(
		"accounts",
		&[
			"id",
			"name",
			"offbudget",
			"closed",
			"tombstone",
			"sort_order",
			"type",
			"last_reconciled",
			"account_group_id",
			"balance_current",
		],
	),
	(
		"categories",
		&[
			"id",
			"name",
			"is_income",
			"cat_group",
			"sort_order",
			"tombstone",
			"hidden",
			"goal_def",
			"template_settings",
			"cleanup_def",
		],
	),
	(
		"category_groups",
		&[
			"id",
			"name",
			"is_income",
			"sort_order",
			"tombstone",
			"hidden",
		],
	),
	(
		"payees",
		&[
			"id",
			"name",
			"category",
			"tombstone",
			"transfer_acct",
			"favorite",
			"learn_categories",
		],
	),
	("payee_mapping", &["id", "targetId"]),
	("category_mapping", &["id", "transferId"]),
	(
		"zero_budgets",
		&[
			"id",
			"month",
			"category",
			"amount",
			"carryover",
			"goal",
			"long_goal",
		],
	),
	("preferences", &["id", "value"]),
	(
		"dashboard",
		&[
			"id",
			"type",
			"width",
			"height",
			"x",
			"y",
			"meta",
			"tombstone",
			"dashboard_page_id",
		],
	),
	("dashboard_pages", &["id", "name", "tombstone"]),
	(
		"rules",
		&[
			"id",
			"stage",
			"conditions",
			"actions",
			"tombstone",
			"conditions_op",
		],
	),
	(
		"schedules",
		&[
			"id",
			"rule",
			"active",
			"completed",
			"posts_transaction",
			"tombstone",
			"name",
			"custom_upcoming_length",
			"sort_order",
		],
	),
	(
		"schedules_next_date",
		&[
			"id",
			"schedule_id",
			"local_next_date",
			"local_next_date_ts",
			"base_next_date",
			"base_next_date_ts",
			"tombstone",
		],
	),
	(
		"schedules_json_paths",
		&["schedule_id", "payee", "account", "amount", "date"],
	),
	(
		"custom_reports",
		&[
			"id",
			"name",
			"start_date",
			"end_date",
			"date_static",
			"date_range",
			"mode",
			"group_by",
			"balance_type",
			"show_empty",
			"show_offbudget",
			"show_hidden",
			"show_uncategorized",
			"selected_categories",
			"graph_type",
			"conditions",
			"conditions_op",
			"metadata",
			"interval",
			"color_scheme",
			"tombstone",
			"include_current",
			"sort_by",
			"trim_intervals",
			"show_trend_lines",
		],
	),
];

/// SQL values Actual never writes where it writes a typed value. SQLite
/// keeps whatever it is given, so a file can hold any of them anywhere.
const SCALARS: &[&str] = &[
	"NULL",
	"''",
	"'not a number'",
	"X'00FF'",
	"0",
	"-1",
	"1.5",
	"9007199254740993",
	"9223372036854775807",
	"-9223372036854775808",
	"9e999",
	"99999999",
	"20261340",
	"'2026-11-20'",
	"'🙂'",
];

/// Rule, schedule, widget and report JSON that is truncated, nested, or not
/// the shape Actual writes.
fn hostile_json() -> Vec<String> {
	vec![
		"[{\"op\":\"is\",\"field\":\"description\",\"val".to_owned(),
		"{\"op\":\"is\"}".to_owned(),
		"\"a string\"".to_owned(),
		"42".to_owned(),
		"null".to_owned(),
		"[]".to_owned(),
		"[null, 1, \"x\", []]".to_owned(),
		format!("{}{}", "[".repeat(300), "]".repeat(300)),
		"[{\"op\":\"isapprox\",\"field\":\"date\",\"value\":{\"start\":\"0\",\"frequency\":\"daily\",\"endMode\":\"after_n_occurrences\",\"endOccurrences\":1e300}}]".to_owned(),
		"[{\"op\":\"isapprox\",\"field\":\"date\",\"value\":{\"start\":\"99999999\",\"frequency\":\"yearly\"}}]".to_owned(),
		"[{\"op\":\"is\",\"field\":\"amount\",\"value\":9007199254740993}]".to_owned(),
		"[{\"op\":\"set-split-amount\",\"value\":1e308,\"options\":{\"splitIndex\":4294967295,\"method\":\"fixed-amount\"}}]".to_owned(),
		"{\"name\":\"x\",\"timeFrame\":{\"start\":\"0000-00\",\"end\":\"262142-12\",\"mode\":\"static\"},\"conditions\":\"x\"}".to_owned(),
		"{\"formula\":\"=((((1\",\"queries\":[1]}".to_owned(),
		"\u{FEFF}{\"id\":\"report-1\"}".to_owned(),
		"{\"a\":1,\"a\":2}".to_owned(),
		"\"\\ud800\"".to_owned(),
	]
}

const JSON_COLUMNS: &[(&str, &str)] = &[
	("rules", "conditions"),
	("rules", "actions"),
	("dashboard", "meta"),
	("custom_reports", "conditions"),
	("custom_reports", "selected_categories"),
	("categories", "goal_def"),
	("categories", "template_settings"),
	("transactions", "error"),
	("schedules_json_paths", "date"),
];

fn sql_text(text: &str) -> String {
	format!("'{}'", text.replace('\'', "''"))
}

/// Edits applied to the furnished database for one case.
struct Case {
	label: String,
	sql: String,
}

/// Every column with four of the scalar values (rotating, so each value
/// meets columns of every kind), every JSON column with every hostile JSON
/// text, then the whole-file cases.
fn cases() -> Vec<Case> {
	let mut cases = Vec::new();
	let mut turn = 0;
	for (table, columns) in COLUMNS {
		for column in *columns {
			for _ in 0..4 {
				let value = SCALARS[turn % SCALARS.len()];
				turn += 1;
				cases.push(Case {
					label: format!("{table}.{column} = {value}"),
					sql: format!(
						"UPDATE {table} SET \"{column}\" = {value} WHERE rowid IN (SELECT rowid FROM {table} ORDER BY rowid LIMIT 2)"
					),
				});
			}
		}
	}
	for (table, column) in JSON_COLUMNS {
		for (n, text) in hostile_json().iter().enumerate() {
			cases.push(Case {
				label: format!("{table}.{column} = hostile JSON {n}"),
				sql: format!("UPDATE {table} SET \"{column}\" = {}", sql_text(text)),
			});
		}
	}
	for (label, sql) in [
		(
			"rule JSON nested 100,000 deep",
			format!(
				"UPDATE rules SET conditions = {deep}, actions = {deep}",
				deep = sql_text(&format!("{}{}", "[".repeat(100_000), "]".repeat(100_000)))
			),
		),
		(
			"every amount past 2^53",
			"UPDATE transactions SET amount = 9007199254740993 + rowid".to_owned(),
		),
		(
			"amounts that overflow when summed",
			"UPDATE transactions SET amount = 9223372036854775807".to_owned(),
		),
		(
			"every amount one above i64::MIN",
			"UPDATE transactions SET amount = -9223372036854775807".to_owned(),
		),
		(
			"budgeted amounts past 2^53",
			"UPDATE zero_budgets SET amount = 9223372036854775807".to_owned(),
		),
		(
			"transaction dates 0, 99999999 and 20261340",
			"UPDATE transactions SET date = CASE rowid % 3 WHEN 0 THEN 0 WHEN 1 THEN 99999999 ELSE 20261340 END".to_owned(),
		),
		(
			"budget months 0 and 999999",
			"UPDATE zero_budgets SET month = CASE rowid % 2 WHEN 0 THEN 0 ELSE 999999 END".to_owned(),
		),
		(
			"a next date of 0",
			"UPDATE schedules_next_date SET local_next_date = 0, base_next_date = 0".to_owned(),
		),
		(
			"a next date in year 9999",
			"UPDATE schedules_next_date SET local_next_date = 99991231, base_next_date = 99991231, local_next_date_ts = 253402300799000".to_owned(),
		),
		(
			"every id NULL",
			"UPDATE transactions SET id = NULL; UPDATE accounts SET id = NULL; UPDATE payees SET id = NULL".to_owned(),
		),
		(
			"a split parent whose children point at themselves",
			"UPDATE transactions SET isParent = 1, isChild = 1, parent_id = id".to_owned(),
		),
		(
			"a transfer pointing at itself",
			"UPDATE transactions SET transferred_id = id".to_owned(),
		),
		(
			"a payee mapping cycle",
			"UPDATE payee_mapping SET targetId = (SELECT id FROM payees ORDER BY id DESC LIMIT 1)".to_owned(),
		),
		(
			"a category mapped to a missing one",
			"UPDATE category_mapping SET transferId = 'gone'".to_owned(),
		),
		(
			"a schedule whose rule is missing",
			"DELETE FROM rules WHERE id = 'rule-bill'".to_owned(),
		),
		(
			"the budget type is something else",
			"INSERT INTO preferences (id, value) VALUES ('budgetType', 'tracking'), ('firstDayOfWeekIdx', '99'), ('numberFormat', 'hex')".to_owned(),
		),
		(
			"a widget at absurd coordinates",
			"UPDATE dashboard SET x = 9223372036854775807, y = -9223372036854775808, width = 0, height = -5".to_owned(),
		),
		(
			"a widget on a page that doesn't exist",
			"UPDATE dashboard SET dashboard_page_id = 'gone'".to_owned(),
		),
		(
			"an upcoming length past every calendar",
			"UPDATE schedules SET custom_upcoming_length = '99999999999999999999-year'".to_owned(),
		),
	] {
		cases.push(Case {
			label: label.to_owned(),
			sql,
		});
	}
	cases
}

/// Every file and folder under `dir`, so "nothing left behind" also catches
/// a hidden staging folder.
fn tree(dir: &Path) -> Vec<String> {
	let mut found = Vec::new();
	let mut pending = vec![dir.to_path_buf()];
	while let Some(next) = pending.pop() {
		for entry in std::fs::read_dir(&next).unwrap() {
			let path = entry.unwrap().path();
			found.push(path.strip_prefix(dir).unwrap().display().to_string());
			if path.is_dir() {
				pending.push(path);
			}
		}
	}
	found.sort();
	found
}

/// Runs `work` on its own thread and fails, naming `label`, if it doesn't
/// finish in time, so a hang is a red test instead of a stuck run.
fn within<T: Send + 'static>(label: &str, work: impl FnOnce() -> T + Send + 'static) -> T {
	let (done, finished) = mpsc::channel();
	let worker = std::thread::spawn(move || {
		let result = work();
		let _ = done.send(());
		result
	});
	match finished.recv_timeout(Duration::from_secs(30)) {
		Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => match worker.join() {
			Ok(result) => result,
			Err(panic) => {
				eprintln!("panicked on: {label}");
				std::panic::resume_unwind(panic)
			}
		},
		Err(mpsc::RecvTimeoutError::Timeout) => panic!("still running after 30 s on: {label}"),
	}
}

fn typed(label: &str, error: &CoreError) {
	assert!(
		!matches!(error, CoreError::Internal(_)),
		"{label}: the core hit a bug: {error}"
	);
}

/// What the app does with a budget it just opened.
fn use_budget(data: &BudgetData) {
	let today = today();
	let civil = CivilDate::from(today);
	let classification = classify(data);
	let _ = account_balances(data, &classification, civil);
	let sheet = EnvelopeSheet::compute(data, civil);

	let book = RuleBook::new(data);
	for transaction in data
		.transactions
		.values()
		.filter(|row| !row.tombstone)
		.take(40)
	{
		let input = RuleTransaction {
			account: transaction.account.clone(),
			date: transaction.date,
			amount: transaction.amount,
			payee: transaction.payee.clone(),
			category: transaction.category.clone(),
			notes: transaction.notes.clone(),
			imported_payee: transaction.imported_payee.clone(),
			cleared: Some(transaction.cleared),
			reconciled: Some(transaction.reconciled),
			schedule: transaction.schedule.clone(),
			deleted: false,
			splits: Vec::new(),
			is_child: false,
		};
		let mut payees = BudgetPayees::new(data, || PayeeId::new("made"));
		let _ = book.run(&input, data, &mut payees);
	}
	let mut payees = BudgetPayees::new(data, || PayeeId::new("made"));
	let _ = advance_schedules(&book, data, &mut payees, civil);
	let year_on = CivilDate::from(today.checked_add_days(chrono::Days::new(366)).unwrap());
	let ids: Vec<ScheduleId> = data.schedules.keys().cloned().collect();
	for id in &ids {
		let schedule = &data.schedules[id];
		let _ = read_bill(schedule, data.rules.get(&schedule.rule), data);
		let _ = bill_status(data, id, 7, civil);
		let _ = bill_dates(data, id, civil, year_on);
	}

	let _ = dashboards(data);
	for widget in data.widgets.values().filter(|widget| !widget.tombstone) {
		let Ok(meta) = parse_meta(widget.meta.as_deref()) else {
			continue;
		};
		let meta = meta.as_ref();
		let Some(kind) = widget.kind.as_deref().and_then(WidgetType::parse) else {
			continue;
		};
		let _ = match (kind, sheet.as_ref()) {
			(WidgetType::NetWorth, _) => net_worth_card(data, meta, today, 1).map(drop),
			(WidgetType::CashFlow, _) => cash_flow_card(data, meta, today).map(drop),
			(WidgetType::Spending, _) => spending(data, meta, today).map(drop),
			(WidgetType::Crossover, _) => crossover(data, meta, today).map(drop),
			(WidgetType::Summary, _) => summary(data, meta, today).map(drop),
			(WidgetType::Calendar, _) => calendar(data, meta, today, 1).map(drop),
			(WidgetType::AgeOfMoney, _) => age_of_money(data, meta, today).map(drop),
			(WidgetType::MonteCarlo, _) => monte_carlo_card(data, meta).map(drop),
			(WidgetType::BudgetAnalysis, Ok(sheet)) => {
				budget_analysis(data, sheet, meta, today).map(drop)
			}
			(WidgetType::Formula, Ok(sheet)) => formula_card(data, sheet, meta, today).map(drop),
			(WidgetType::Sankey, Ok(sheet)) => {
				sankey_card(data, sheet, meta, today, Some(8)).map(drop)
			}
			(WidgetType::BalanceForecast, Ok(sheet)) => {
				balance_forecast_card(data, sheet, meta, today).map(drop)
			}
			_ => Ok(()),
		};
	}
	for row in data.custom_reports.values().filter(|row| !row.tombstone) {
		if let Ok(report) = CustomReport::from_row(row) {
			let _ = evaluate_report(data, &report, today, 1);
		}
	}
}

/// How one hostile import ended.
#[derive(Debug, Default)]
struct Tally {
	not_applicable: usize,
	refused: usize,
	opened: usize,
	read_only: usize,
	open_failed: usize,
}

fn import_and_use(case: &Case, db: &[u8], metadata: &[u8], tally: &mut Tally) {
	let scratch = Scratch::new("case");
	let db_path = scratch.0.join("db.sqlite");
	std::fs::write(&db_path, db).unwrap();
	{
		let conn = rusqlite::Connection::open(&db_path).unwrap();
		if conn.execute_batch(&case.sql).is_err() {
			tally.not_applicable += 1;
			return;
		}
	}
	let edited = std::fs::read(&db_path).unwrap();
	std::fs::remove_file(&db_path).unwrap();
	let zip = scratch.0.join("hostile.zip");
	write_zip(&zip, &edited, metadata);
	let before = tree(&scratch.0);
	let budget_dir = scratch.0.join("Imported");

	let label = case.label.clone();
	let into = budget_dir.clone();
	let outcome = within(&label.clone(), move || {
		let imported = match import_actual_zip(&zip, &into) {
			Ok(ImportOutcome::Imported(imported)) => imported,
			Ok(ImportOutcome::Refused(_)) => return "refused",
			Err(error) => {
				typed(&label, &error);
				return "refused";
			}
		};
		let mut store = match open_budget(&imported.db_path) {
			Ok(store) => store,
			Err(error) => {
				typed(&label, &error);
				return "open failed";
			}
		};
		let writable = matches!(store.access(), Access::ReadWrite);
		if writable && let Err(error) = store.adopt() {
			typed(&label, &error);
		}
		if let Err(error) = store.open_audit() {
			typed(&label, &error);
		}
		match store.load() {
			Ok(loaded) => {
				use_budget(&loaded.data);
				if writable { "opened" } else { "read-only" }
			}
			Err(error) => {
				typed(&label, &error);
				"open failed"
			}
		}
	});
	match outcome {
		"refused" => {
			tally.refused += 1;
			assert_eq!(
				tree(&scratch.0),
				before,
				"{}: a refused import left something behind",
				case.label
			);
		}
		other => {
			assert!(budget_dir.join("db.sqlite").is_file(), "{}", case.label);
			match other {
				"opened" => tally.opened += 1,
				"read-only" => tally.read_only += 1,
				_ => tally.open_failed += 1,
			}
		}
	}
}

#[test]
fn a_hostile_actual_file_imports_to_a_typed_error_or_a_budget_that_opens_and_works() {
	let (db, metadata) = small();
	let furnished = {
		let scratch = Scratch::new("furnish");
		let path = scratch.0.join("db.sqlite");
		std::fs::write(&path, &db).unwrap();
		rusqlite::Connection::open(&path)
			.unwrap()
			.execute_batch(FURNISH)
			.unwrap();
		std::fs::read(&path).unwrap()
	};

	// The furnished file itself opens and works, so every refusal below
	// comes from the hostile edit.
	let mut baseline = Tally::default();
	let unedited = Case {
		label: "unedited".to_owned(),
		sql: String::new(),
	};
	import_and_use(&unedited, &furnished, &metadata, &mut baseline);
	assert_eq!(baseline.opened, 1, "{baseline:?}");

	let cases = cases();
	let mut tally = Tally::default();
	for case in &cases {
		import_and_use(case, &furnished, &metadata, &mut tally);
	}
	eprintln!("{} hostile files: {tally:?}", cases.len());
	assert!(
		tally.opened + tally.read_only > cases.len() / 2,
		"most hostile files still open: {tally:?}"
	);
}

#[test]
fn a_hostile_metadata_file_is_refused_and_leaves_nothing_behind() {
	let (db, _) = small();
	for (label, metadata) in [
		("empty", Vec::new()),
		("truncated", b"{\"id\":\"Veritas-Small".to_vec()),
		("an array", b"[1,2,3]".to_vec()),
		("not UTF-8", vec![0xFF, 0xFE, 0x00, 0x7B]),
		(
			"nested deeper than any parser allows",
			format!("{}{}", "[".repeat(100_000), "]".repeat(100_000)).into_bytes(),
		),
		(
			"a budget name that is a number",
			b"{\"id\":\"x\",\"budgetName\":42,\"resetClock\":\"yes\"}".to_vec(),
		),
	] {
		let scratch = Scratch::new("metadata");
		let zip = scratch.0.join("hostile.zip");
		write_zip(&zip, &db, &metadata);
		let before = tree(&scratch.0);
		let into = scratch.0.join("Imported");
		let result = within(label, move || import_actual_zip(&zip, &into));
		match result {
			Ok(ImportOutcome::Imported(_)) => {
				assert!(scratch.0.join("Imported/db.sqlite").is_file(), "{label}");
			}
			Ok(ImportOutcome::Refused(_)) => assert_eq!(tree(&scratch.0), before, "{label}"),
			Err(error) => {
				typed(label, &error);
				assert_eq!(tree(&scratch.0), before, "{label}");
			}
		}
	}
}

/// One expense at the smallest amount SQLite's integer column holds, which
/// the loader takes as it is (Actual itself never writes past 2^51).
fn spent_the_minimum() -> support::Book {
	let mut book = support::Book::new();
	book.account("checking", false);
	book.row("income", "checking", "2026-11-02", 250_000);
	book.row("expense", "checking", "2026-11-03", i64::MIN);
	book
}

#[test]
fn the_calendar_card_refuses_a_day_spent_at_the_smallest_storable_amount() {
	// Found by this harness and fixed in ca2a23f: the calendar card took
	// `.abs()` of a day's total, which overflows on i64::MIN (a debug build
	// panicked; a release build wrapped and showed a negative expense).
	let book = spent_the_minimum();
	let result = within("calendar, one expense at i64::MIN", move || {
		calendar(&book.budget, None, today(), 0).map(drop)
	});
	assert!(
		matches!(
			result,
			Ok(()) | Err(CoreError::Overflow | CoreError::Invalid(_))
		),
		"{result:?}"
	);
}

#[test]
fn the_age_of_money_card_refuses_a_day_spent_at_the_smallest_storable_amount() {
	// Found by this harness and fixed in ca2a23f: age of money took `.abs()`
	// of each expense, the same overflow as the calendar card's.
	let book = spent_the_minimum();
	let result = within("age of money, one expense at i64::MIN", move || {
		age_of_money(&book.budget, None, today()).map(drop)
	});
	assert!(
		matches!(
			result,
			Ok(()) | Err(CoreError::Overflow | CoreError::Invalid(_))
		),
		"{result:?}"
	);
}

/// Two months that each balance, earning, budgeting and spending a little
/// over 2^62, so each month is sound but the two together pass i64::MAX.
fn two_months_past_the_limit() -> support::Book {
	let half = (1_i64 << 62) + 1;
	let mut book = support::Book::new();
	book.account("checking", false);
	book.group("income", true);
	book.category("salary", "income", None);
	book.group("bills", false);
	book.category("rent", "bills", None);
	for (month, day) in [("2026-10", "03"), ("2026-11", "03")] {
		book.row(
			&format!("pay-{month}"),
			"checking",
			&format!("{month}-02"),
			half,
		)
		.category = Some(support::cat("salary"));
		book.row(
			&format!("rent-{month}"),
			"checking",
			&format!("{month}-{day}"),
			-half,
		)
		.category = Some(support::cat("rent"));
		let ym = veritas_domain::YearMonth::parse(month).unwrap();
		book.budget.budget_cells.insert(
			(ym, support::cat("rent")),
			veritas_domain::BudgetCell {
				month: ym,
				category: support::cat("rent"),
				budgeted: veritas_domain::Money::from_minor(half),
				carryover: false,
				goal: None,
				long_goal: false,
			},
		);
	}
	book
}

/// The app's path: the envelope sheet first, then the card. Data past the
/// integer limit is refused with a typed error there, never drawn as a wrong
/// number. (The engine's sheet refuses these books itself, so the cards'
/// own checked arithmetic from fc70664 is a second line behind it.)
fn sheet_then(
	budget: &BudgetData,
	card: impl FnOnce(&BudgetData, &EnvelopeSheet) -> Result<(), CoreError>,
) -> Result<(), CoreError> {
	let sheet = EnvelopeSheet::compute(budget, CivilDate::new(2026, 11, 20)?)?;
	card(budget, &sheet)
}

#[test]
fn a_range_past_the_integer_limit_is_refused_before_the_sankey_card_draws_it() {
	let book = two_months_past_the_limit();
	let result = within("sankey card, two months past i64::MAX", move || {
		sheet_then(&book.budget, |budget, sheet| {
			sankey_card(budget, sheet, None, today(), Some(8)).map(drop)
		})
	});
	assert!(matches!(result, Err(CoreError::Overflow)), "{result:?}");
}

#[test]
fn a_range_past_the_integer_limit_is_refused_before_budget_analysis_draws_it() {
	let book = two_months_past_the_limit();
	let result = within("budget analysis, two months past i64::MAX", move || {
		sheet_then(&book.budget, |budget, sheet| {
			budget_analysis(budget, sheet, None, today()).map(drop)
		})
	});
	assert!(matches!(result, Err(CoreError::Overflow)), "{result:?}");
}

/// Imports small.zip after `sql` and loads it the way the app does.
fn import_edited(sql: &str) -> veritas_store::Loaded {
	let (db, metadata) = small();
	let scratch = Scratch::new("edited");
	let db_path = scratch.0.join("db.sqlite");
	std::fs::write(&db_path, db).unwrap();
	rusqlite::Connection::open(&db_path)
		.unwrap()
		.execute_batch(sql)
		.unwrap();
	let zip = scratch.0.join("edited.zip");
	write_zip(&zip, &std::fs::read(&db_path).unwrap(), &metadata);
	let ImportOutcome::Imported(imported) =
		import_actual_zip(&zip, &scratch.0.join("Imported")).unwrap()
	else {
		panic!("{sql}: refused");
	};
	open_budget(&imported.db_path).unwrap().load().unwrap()
}

#[test]
fn a_transaction_the_loader_cant_read_is_reported_and_left_out_never_read_as_zero() {
	// Silent wrongness would be a row read with a made-up amount or date: a
	// balance off by a transaction nobody can see. The row must be named in
	// the load report and kept out of the budget instead.
	let id: String = {
		let (db, _) = small();
		let scratch = Scratch::new("id");
		let path = scratch.0.join("db.sqlite");
		std::fs::write(&path, db).unwrap();
		rusqlite::Connection::open(&path)
			.unwrap()
			.query_row(
				"SELECT id FROM transactions ORDER BY rowid LIMIT 1",
				[],
				|row| row.get(0),
			)
			.unwrap()
	};
	for (column, value) in [
		("amount", "'12.34'"),
		("amount", "1.5"),
		("amount", "X'00FF'"),
		("date", "20261340"),
		("date", "99999999"),
		("date", "0"),
		("date", "'2026-11-20'"),
		("acct", "NULL"),
	] {
		let sql = format!("UPDATE transactions SET {column} = {value} WHERE id = '{id}'");
		let loaded = import_edited(&sql);
		assert!(
			loaded
				.report
				.skipped
				.iter()
				.any(|row| row.table == "transactions" && row.id.as_deref() == Some(id.as_str())),
			"{column} = {value}: not in the load report {:?}",
			loaded.report
		);
		assert!(
			!loaded
				.data
				.transactions
				.contains_key(&veritas_domain::TransactionId::new(id.as_str())),
			"{column} = {value}: the row was loaded anyway"
		);
	}
}
