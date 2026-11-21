//! The shadow run on a real Actual export (PLAN §8 Phase 3, §10 A3 and C5),
//! started by `cargo xtask shadow <export.zip>`, which hands it a fresh
//! scratch folder in `VERITAS_SHADOW_DIR` and points `TMPDIR` inside it.
//!
//! Real money: every line it prints is a count, a check's name, or a
//! position and a kind ("widget #7 (sankey-card)"). It never prints an
//! amount, a name, a note, an id or an error's text, and its panic hook is
//! silent, because any of those could quote the budget.

use std::collections::BTreeSet;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::process::{Command, ExitCode, Stdio};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread::Thread;

use chrono::NaiveDate;
use serde_json::{Value, json};
use veritas_ffi::{CancelToken, Session};
use veritas_interop::{
	ExportOutcome, ImportOutcome, canonical_dump_of_actual_tables, export_actual_zip,
	import_actual_zip,
};
use veritas_reports::{CustomReport, QueryRow, report_range, report_rows};
use veritas_store::{Access, Store};

#[allow(
	clippy::unwrap_used,
	clippy::expect_used,
	clippy::panic,
	reason = "shared with the widget journeys; a panic here is caught and reported without its message"
)]
#[path = "../../../crates/veritas-ffi/tests/support/cards.rs"]
mod cards;

/// A check that couldn't finish. Only its name is ever shown.
struct Stopped(&'static str);

type Checked<T> = Result<T, Stopped>;

fn say(line: &str) {
	println!("shadow: {line}");
}

fn main() -> ExitCode {
	std::panic::set_hook(Box::new(|_| {}));
	let (Some(zip), Some(scratch)) = (
		std::env::args_os().nth(1).map(PathBuf::from),
		std::env::var_os("VERITAS_SHADOW_DIR").map(PathBuf::from),
	) else {
		say("run me through `cargo xtask shadow <export.zip>`");
		return ExitCode::from(2);
	};
	if !zip.is_file() {
		say("the export isn't a file");
		return ExitCode::from(2);
	}
	let run = std::panic::catch_unwind(|| shadow(&zip, &scratch));
	match run {
		Ok(Ok(true)) => {
			say("all checks pass");
			ExitCode::SUCCESS
		}
		Ok(Ok(false)) => {
			say("some checks differ");
			ExitCode::FAILURE
		}
		Ok(Err(Stopped(check))) => {
			say(&format!("{check}: could not finish (details withheld)"));
			ExitCode::FAILURE
		}
		Err(_) => {
			say("stopped early (details withheld)");
			ExitCode::FAILURE
		}
	}
}

fn shadow(zip: &Path, scratch: &Path) -> Checked<bool> {
	let oracle = Oracle::new()?;
	let today = oracle.today.clone();
	let round_trip = round_trip(&oracle, zip, scratch)?;
	let (session, exported) = open_in_veritas(zip, scratch, &today)?;
	let dump = oracle.json(&["dump", &text(&exported)])?;
	let months = months(&session, &dump, &today)?;
	let widgets = widgets(&oracle, &session, &exported, &dump, scratch)?;
	let reports = custom_reports(&oracle, zip, scratch)?;
	Ok(round_trip && months && widgets && reports)
}

fn text(path: &Path) -> String {
	path.display().to_string()
}

struct Oracle {
	dir: PathBuf,
	today: String,
}

impl Oracle {
	fn new() -> Checked<Oracle> {
		let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../actual-oracle");
		if !dir.join("node_modules").is_dir() {
			return Err(Stopped(
				"the oracle isn't installed (cargo xtask oracle setup)",
			));
		}
		let reference = std::env::var_os("ACTUAL_REFERENCE_DIR")
			.map_or_else(|| dir.join("../../reference/actual"), PathBuf::from);
		if !reference.is_dir() {
			return Err(Stopped(
				"Actual's source isn't checked out (cargo xtask reference)",
			));
		}
		let output = Command::new("date")
			.arg("+%Y-%m-%d")
			.stderr(Stdio::null())
			.output()
			.map_err(|_| Stopped("today's date"))?;
		let today = String::from_utf8_lossy(&output.stdout).trim().to_owned();
		Ok(Oracle { dir, today })
	}

	/// The oracle's stdout. Its stderr is dropped unread: Actual's logs can
	/// quote rows.
	fn run(&self, args: &[&str]) -> Checked<Vec<u8>> {
		let output = Command::new("node")
			.current_dir(&self.dir)
			.arg("src/main.ts")
			.args(args)
			.args(["--today", &self.today, "--seed", "1"])
			.stdin(Stdio::null())
			.stderr(Stdio::null())
			.output()
			.map_err(|_| Stopped("starting the oracle"))?;
		if !output.status.success() {
			return Err(Stopped("the oracle"));
		}
		Ok(output.stdout)
	}

	fn json(&self, args: &[&str]) -> Checked<Value> {
		serde_json::from_slice(&self.run(args)?).map_err(|_| Stopped("reading the oracle's answer"))
	}
}

/// A3: imported and immediately exported, the canonical dump is unchanged,
/// by Veritas's own dumper and by Actual's.
fn round_trip(oracle: &Oracle, zip: &Path, scratch: &Path) -> Checked<bool> {
	const CHECK: &str = "round trip";
	let budget = match import_actual_zip(zip, &scratch.join("round-trip")) {
		Ok(ImportOutcome::Imported(budget)) => budget,
		Ok(ImportOutcome::Refused(report)) => {
			say(&format!(
				"{CHECK}: import refused, {} audit finding(s)",
				report.violations.len()
			));
			return Ok(false);
		}
		Err(_) => return Err(Stopped(CHECK)),
	};
	let mut store = Store::open(&budget.db_path, Access::ReadWrite).map_err(|_| Stopped(CHECK))?;
	store.adopt().map_err(|_| Stopped(CHECK))?;
	let exported = scratch.join("round-trip.zip");
	match export_actual_zip(&store, &budget.metadata_path, &exported) {
		Ok(ExportOutcome::Exported(audit)) if audit.is_clean() => {}
		Ok(ExportOutcome::Exported(audit)) => {
			say(&format!(
				"{CHECK}: export audit found {} finding(s)",
				audit.violations.len()
			));
			return Ok(false);
		}
		Ok(_) => {
			say(&format!("{CHECK}: export refused"));
			return Ok(false);
		}
		Err(_) => return Err(Stopped(CHECK)),
	}
	drop(store);

	let before = canonical_dump_of_actual_tables(zip).map_err(|_| Stopped(CHECK))?;
	let after = canonical_dump_of_actual_tables(&exported).map_err(|_| Stopped(CHECK))?;
	let lines = before.split(|&b| b == b'\n').count();
	let ours = differing_lines(&before, &after);

	let theirs_before = oracle.json(&["dump", &text(zip), "--raw-only"])?;
	let theirs_after = oracle.json(&["dump", &text(&exported), "--raw-only"])?;
	let mut sections = Vec::new();
	walk(
		"",
		&without_veritas_tables(theirs_before),
		&without_veritas_tables(theirs_after),
		&mut sections,
	);
	say(&format!(
		"{CHECK}: Veritas's dump {lines} lines, {ours} differ; Actual's dump {} section(s) differ",
		sections.len()
	));
	Ok(ours == 0 && sections.is_empty())
}

fn differing_lines(a: &[u8], b: &[u8]) -> usize {
	let left: BTreeSet<&[u8]> = a.split(|&byte| byte == b'\n').collect();
	let right: BTreeSet<&[u8]> = b.split(|&byte| byte == b'\n').collect();
	left.symmetric_difference(&right).count()
}

/// contracts/canonical-dump.md: R-IO-3 compares Actual's tables, so the
/// `veritas_*` tables adopting adds, and their schema rows, are left out.
fn without_veritas_tables(mut dump: Value) -> Value {
	let veritas = |name: Option<&str>| name.is_some_and(|name| name.starts_with("veritas_"));
	if let Some(tables) = dump.get_mut("tables").and_then(Value::as_object_mut) {
		tables.retain(|name, _| !veritas(Some(name)));
	}
	if let Some(schema) = dump.get_mut("schema").and_then(Value::as_array_mut) {
		schema.retain(|entry| {
			!veritas(entry.get("tbl_name").and_then(Value::as_str))
				&& !veritas(entry.get("name").and_then(Value::as_str))
		});
	}
	dump
}

/// Collects where two dumps differ. The paths are kept only to be counted.
fn walk(path: &str, a: &Value, b: &Value, out: &mut Vec<String>) {
	match (a, b) {
		(Value::Object(left), Value::Object(right)) => {
			let keys: BTreeSet<&String> = left.keys().chain(right.keys()).collect();
			for key in keys {
				let child = format!("{path}.{key}");
				match (left.get(key), right.get(key)) {
					(Some(x), Some(y)) => walk(&child, x, y, out),
					_ => out.push(child),
				}
			}
		}
		(Value::Array(left), Value::Array(right)) => {
			let left: BTreeSet<String> = left.iter().map(Value::to_string).collect();
			let right: BTreeSet<String> = right.iter().map(Value::to_string).collect();
			if left != right {
				out.push(path.to_owned());
			}
		}
		(x, y) if x != y => out.push(path.to_owned()),
		_ => {}
	}
}

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

/// The export opened the way the app opens it, then exported again, so
/// Actual reads exactly what Veritas holds.
fn open_in_veritas(zip: &Path, scratch: &Path, today: &str) -> Checked<(Arc<Session>, PathBuf)> {
	const CHECK: &str = "opening in Veritas";
	let session = block_on(Session::import_actual_zip(
		text(zip),
		text(&scratch.join("session")),
		CancelToken::new(),
	))
	.map_err(|_| Stopped(CHECK))?;
	session
		.budget_month(month(today)?, today.to_owned())
		.map_err(|_| Stopped(CHECK))?;
	let exported = scratch.join("session.zip");
	block_on(session.export_actual_zip(text(&exported), CancelToken::new()))
		.map_err(|_| Stopped(CHECK))?;
	Ok((session, exported))
}

fn month(text: &str) -> Checked<veritas_ffi::Month> {
	let year = text.get(..4).and_then(|y| y.parse().ok());
	let month = text.get(5..7).and_then(|m| m.parse().ok());
	match (year, month) {
		(Some(year), Some(month)) => Ok(veritas_ffi::Month { year, month }),
		_ => Err(Stopped("reading a month")),
	}
}

/// The shadow run's budget check: every month Actual computes, its to-budget
/// and each envelope's budgeted, spent and available against the Budget
/// screen, and every open account's balance against the account list.
fn months(session: &Session, dump: &Value, today: &str) -> Checked<bool> {
	const CHECK: &str = "months";
	let months = dump["computed"]["months"]
		.as_object()
		.ok_or(Stopped(CHECK))?;
	let (mut cells, mut differ, mut bad_months) = (0usize, 0usize, Vec::new());
	let (mut only_actual, mut only_veritas) = (0usize, 0usize);
	for (index, (label, theirs)) in months.iter().enumerate() {
		let shown = session
			.budget_month(month(label)?, today.to_owned())
			.map_err(|_| Stopped(CHECK))?;
		let before = (differ, only_actual, only_veritas);
		let mut check = |ours: i64, theirs: &Value| {
			cells += 1;
			if *theirs != ours {
				differ += 1;
			}
		};
		check(shown.to_budget, &theirs["toBudget"]);
		let mut ours = BTreeSet::new();
		for group in &shown.groups {
			for row in &group.envelopes {
				ours.insert(row.id.as_str());
				let category = &theirs["categories"][&row.id];
				if category.is_null() {
					only_veritas += 1;
					continue;
				}
				if group.is_income {
					check(row.spent, &category["received"]);
				} else {
					check(row.budgeted, &category["budgeted"]);
					check(row.spent, &category["spent"]);
					check(row.available, &category["balance"]);
				}
			}
		}
		if let Some(categories) = theirs["categories"].as_object() {
			only_actual += categories
				.keys()
				.filter(|id| !ours.contains(id.as_str()))
				.count();
		}
		if (differ, only_actual, only_veritas) != before {
			bad_months.push(format!("#{}", index + 1));
		}
	}
	say(&format!(
		"{CHECK}: {} compared, {} mismatched{}; {cells} cells, {differ} differ; \
		 categories only in Actual: {only_actual}, only in Veritas: {only_veritas}",
		months.len(),
		bad_months.len(),
		listed(&bad_months)
	));

	let accounts = session
		.accounts()
		.map_err(|_| Stopped("accounts"))?
		.accounts;
	let theirs = dump["computed"]["accounts"]
		.as_object()
		.ok_or(Stopped("accounts"))?;
	let mut differing = Vec::new();
	let mut missing = 0usize;
	for (index, account) in accounts.iter().enumerate() {
		match theirs.get(&account.id) {
			Some(actual) if actual["balance"] == account.total => {}
			Some(_) => differing.push(format!("#{}", index + 1)),
			None => missing += 1,
		}
	}
	let only_actual_accounts = theirs
		.keys()
		.filter(|id| !accounts.iter().any(|a| &a.id == *id))
		.count();
	let closed = accounts.iter().filter(|a| a.closed).count();
	say(&format!(
		"accounts: {} compared ({closed} closed), {} differ{}; only in Actual: {only_actual_accounts}, only in Veritas: {missing}",
		accounts.len(),
		differing.len(),
		listed(&differing)
	));
	Ok(differ == 0
		&& only_actual == 0
		&& only_veritas == 0
		&& differing.is_empty()
		&& missing == 0
		&& only_actual_accounts == 0)
}

fn listed(items: &[String]) -> String {
	if items.is_empty() {
		String::new()
	} else {
		format!(": {}", items.join(", "))
	}
}

const NATIVE: [&str; 12] = [
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
	"sankey-card",
	"balance-forecast-card",
];

/// C5, the dashboard: every card Veritas draws natively, run through Actual's
/// own card code with the settings Veritas holds, as the widget journeys do.
/// Cards that show a saved report are the custom reports' check.
fn widgets(
	oracle: &Oracle,
	session: &Session,
	exported: &Path,
	dump: &Value,
	scratch: &Path,
) -> Checked<bool> {
	const CHECK: &str = "widgets";
	let today = &oracle.today;
	let pages = block_on(session.dashboards(today.clone(), CancelToken::new()))
		.map_err(|_| Stopped(CHECK))?;
	let metas: std::collections::BTreeMap<String, Value> = dump["tables"]["dashboard"]["rows"]
		.as_array()
		.map(|rows| {
			rows.iter()
				.filter_map(|row| {
					let id = row["id"].as_str()?.to_owned();
					let meta = row["meta"]
						.as_str()
						.and_then(|text| serde_json::from_str(text).ok())
						.unwrap_or(Value::Null);
					Some((id, meta))
				})
				.collect()
		})
		.unwrap_or_default();
	// D74: a budget whose currency has no decimal places keeps its cells on the
	// budget scale, so Actual's card code reads it after the register-scale ops.
	let currency = session
		.overview()
		.ok()
		.and_then(|overview| overview.number_format)
		.and_then(|format| format.currency);
	let zero_places = currency.is_some_and(|code| {
		veritas_ffi::currencies()
			.unwrap_or_default()
			.iter()
			.any(|known| known.code == code && known.decimal_places == 0)
	});
	let actuals_zip = if zero_places {
		let ops = scratch.join("register-scale.ops.json");
		let scaled = scratch.join("register-scale.zip");
		std::fs::write(
			&ops,
			json!({"version": 1, "ops": cards::register_scale_ops(dump)}).to_string(),
		)
		.map_err(|_| Stopped(CHECK))?;
		oracle.run(&[
			"apply",
			&text(exported),
			&text(&ops),
			"--out",
			&text(&scaled),
		])?;
		scaled
	} else {
		exported.to_path_buf()
	};

	let mut requests = Vec::new();
	let mut expected = Vec::new();
	let mut undrawn = Vec::new();
	let mut other_kinds = 0usize;
	for (index, widget) in pages.iter().flat_map(|p| &p.widgets).enumerate() {
		let position = index + 1;
		let Some(shown) = cards::as_actual(&widget.view) else {
			if NATIVE.contains(&widget.kind.as_str()) {
				undrawn.push(format!("widget #{position} ({})", safe_kind(&widget.kind)));
			} else {
				other_kinds += 1;
			}
			continue;
		};
		let mut meta = metas.get(&widget.id).cloned().unwrap_or(Value::Null);
		if zero_places && widget.kind == "monte-carlo-card" {
			cards::register_scale_monte_carlo(&mut meta, false);
		}
		let card = if widget.kind == "balance-forecast-card" {
			"forecast-card"
		} else {
			widget.kind.as_str()
		};
		requests.push(
			json!({"id": position.to_string(), "type": card, "meta": meta, "firstDayOfWeekIdx": "0"}),
		);
		expected.push((position, widget.kind.clone(), shown));
	}
	let file = scratch.join("widgets.json");
	std::fs::write(&file, Value::Array(requests).to_string()).map_err(|_| Stopped(CHECK))?;
	let theirs = oracle.json(&["widgets", &text(&actuals_zip), &text(&file)])?;
	let mismatched: Vec<String> = expected
		.iter()
		.filter(|(position, kind, shown)| {
			cards::theirs_comparable(kind, &theirs[position.to_string()], today) != *shown
		})
		.map(|(position, kind, _)| format!("widget #{position} ({})", safe_kind(kind)))
		.collect();
	let compared = expected.len() + undrawn.len();
	let matched = expected.len() - mismatched.len();
	let differ: Vec<String> = undrawn.into_iter().chain(mismatched).collect();
	say(&format!(
		"{CHECK}: {compared} compared, {matched} matched, {} differ{}; {other_kinds} show a saved report",
		differ.len(),
		listed(&differ)
	));
	Ok(differ.is_empty())
}

/// A widget kind as Actual names it, or "other": a kind is the app's word,
/// never the user's, but an unknown one is shown generically all the same.
fn safe_kind(kind: &str) -> &str {
	if NATIVE.contains(&kind) || kind == "custom-report" {
		kind
	} else {
		"other"
	}
}

/// C5, saved reports: each report's rows for its own range and filters
/// against Actual's query engine (`report-rows`), as R-REP-4's tests do.
fn custom_reports(oracle: &Oracle, zip: &Path, scratch: &Path) -> Checked<bool> {
	const CHECK: &str = "custom reports";
	let budget = match import_actual_zip(zip, &scratch.join("reports")) {
		Ok(ImportOutcome::Imported(budget)) => budget,
		_ => return Err(Stopped(CHECK)),
	};
	let mut store = Store::open(&budget.db_path, Access::ReadWrite).map_err(|_| Stopped(CHECK))?;
	store.adopt().map_err(|_| Stopped(CHECK))?;
	let data = store.load().map_err(|_| Stopped(CHECK))?.data;
	let today = NaiveDate::parse_from_str(&oracle.today, "%Y-%m-%d").map_err(|_| Stopped(CHECK))?;

	let mut rows: Vec<_> = data
		.custom_reports
		.values()
		.filter(|row| !row.tombstone)
		.collect();
	rows.sort_by(|a, b| a.id.cmp(&b.id));
	let mut unreadable = Vec::new();
	let mut requests = Vec::new();
	let mut ours = Vec::new();
	for (index, row) in rows.iter().enumerate() {
		let position = index + 1;
		let Ok(report) = CustomReport::from_row(row) else {
			unreadable.push(format!("report #{position}"));
			continue;
		};
		let Ok((start, end)) = report_range(&report, &data, today, 0) else {
			unreadable.push(format!("report #{position}"));
			continue;
		};
		let conditions: Value =
			serde_json::from_str(report.row.conditions.as_deref().unwrap_or("[]"))
				.unwrap_or(Value::Array(Vec::new()));
		let Ok((assets, debts)) = report_rows(&data, &report, &start, &end) else {
			unreadable.push(format!("report #{position}"));
			continue;
		};
		requests.push(json!({
			"id": position.to_string(),
			"startDate": start,
			"endDate": end,
			"interval": format!("{:?}", report.interval),
			"conditions": conditions,
			"conditionsOp": if report.conditions_or { "or" } else { "and" },
		}));
		ours.push((position, sorted(&assets), sorted(&debts)));
	}
	let mut differ = Vec::new();
	if !requests.is_empty() {
		let file = scratch.join("report-rows.json");
		std::fs::write(&file, Value::Array(requests).to_string()).map_err(|_| Stopped(CHECK))?;
		let theirs = oracle.json(&["report-rows", &text(zip), &text(&file)])?;
		for (position, assets, debts) in &ours {
			let expected = &theirs[position.to_string()];
			let side = |name: &str| {
				let mut keys: Vec<String> = expected[name]
					.as_array()
					.map(|rows| rows.iter().map(Value::to_string).collect())
					.unwrap_or_default();
				keys.sort();
				keys
			};
			if side("assets") != *assets || side("debts") != *debts {
				differ.push(format!("report #{position}"));
			}
		}
	}
	say(&format!(
		"{CHECK}: {} compared, {} matched, {} differ{}; {} not readable here{}",
		ours.len(),
		ours.len() - differ.len(),
		differ.len(),
		listed(&differ),
		unreadable.len(),
		listed(&unreadable)
	));
	Ok(differ.is_empty())
}

fn sorted(rows: &[QueryRow]) -> Vec<String> {
	let id = |value: Option<String>| value.map_or(Value::Null, Value::String);
	let mut keys: Vec<String> = rows
		.iter()
		.map(|row| {
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
			.to_string()
		})
		.collect();
	keys.sort();
	keys
}
