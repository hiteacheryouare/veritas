//! Drives `tools/actual-oracle` and loads Actual files through veritas-store,
//! for the differential tests. Without the oracle these tests skip, unless
//! `VERITAS_REQUIRE_ORACLE` is set (CI), where a missing oracle is a failure.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;
use veritas_domain::{BudgetData, YearMonth};
use veritas_engine::EnvelopeSheet;
use veritas_interop::{Access, ImportOutcome, import_actual_zip};
use veritas_store::Store;

pub fn repo_root() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn fixture(name: &str) -> PathBuf {
	repo_root()
		.join("fixtures/synthetic")
		.join(format!("{name}.zip"))
}

fn oracle_dir() -> PathBuf {
	repo_root().join("tools/actual-oracle")
}

/// True when the oracle can run. Panics instead of skipping under CI's
/// `VERITAS_REQUIRE_ORACLE`, so a green run always means parity was checked.
pub fn available() -> bool {
	if oracle_dir().join("node_modules").exists() {
		return true;
	}
	let fix = "tools/actual-oracle/node_modules is missing: run `cargo xtask oracle setup`";
	if std::env::var_os("VERITAS_REQUIRE_ORACLE").is_some() {
		panic!("VERITAS_REQUIRE_ORACLE is set but {fix}");
	}
	eprintln!("skipping the oracle comparison: {fix}");
	false
}

fn run(args: &[&std::ffi::OsStr]) -> Vec<u8> {
	let output = Command::new("node")
		.current_dir(oracle_dir())
		.arg("src/main.ts")
		.args(args)
		.output()
		.expect("node runs");
	assert!(
		output.status.success(),
		"the oracle failed: {}",
		String::from_utf8_lossy(&output.stderr)
	);
	output.stdout
}

/// The oracle's full dump of a zip, with `computed` as Actual's API reports it.
pub fn dump(zip: &Path) -> Value {
	serde_json::from_slice(&run(&["dump".as_ref(), zip.as_os_str()])).unwrap()
}

/// Runs each op list on a fresh import of `zip` inside one Actual process.
pub fn replay(zip: &Path, sessions: &[Value]) -> Vec<Value> {
	let scratch = Scratch::new("replay");
	let input = scratch.join("sessions.json");
	let output = scratch.join("results.json");
	let body = serde_json::json!({ "version": 1, "sessions": sessions });
	std::fs::write(&input, serde_json::to_vec(&body).unwrap()).unwrap();
	run(&[
		"replay".as_ref(),
		zip.as_os_str(),
		input.as_os_str(),
		"--out".as_ref(),
		output.as_os_str(),
	]);
	serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap()
}

/// Imports a zip the way the app does and loads it into the domain model.
pub fn load(zip: &Path) -> BudgetData {
	let scratch = Scratch::new("load");
	let ImportOutcome::Imported(imported) =
		import_actual_zip(zip, &scratch.join("budget")).unwrap()
	else {
		panic!("{} failed its audit on import", zip.display());
	};
	let mut store = Store::open(&imported.db_path, Access::ReadWrite).unwrap();
	store.adopt().unwrap();
	store.load().unwrap().data
}

pub struct Scratch(PathBuf);

static NEXT: AtomicU64 = AtomicU64::new(0);

impl Scratch {
	pub fn new(label: &str) -> Scratch {
		let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
			"engine-{label}-{}-{}",
			std::process::id(),
			NEXT.fetch_add(1, Ordering::Relaxed)
		));
		let _ = std::fs::remove_dir_all(&path);
		std::fs::create_dir_all(&path).unwrap();
		Scratch(path)
	}

	pub fn join(&self, name: &str) -> PathBuf {
		self.0.join(name)
	}
}

impl Drop for Scratch {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.0);
	}
}

/// Every difference between the sheet and the months Actual's API reported
/// (`api/budget-month`, `reference/actual/packages/loot-core/src/server/api.ts:454`).
pub fn compare_months(sheet: &EnvelopeSheet, budget: &BudgetData, months: &Value) -> Vec<String> {
	let mut differences = Vec::new();
	let months = months.as_object().expect("months is an object");
	let api_months: Vec<YearMonth> = months
		.keys()
		.map(|m| YearMonth::parse(m).unwrap())
		.collect();
	let ours = &sheet.months()[..sheet.months().len().saturating_sub(1)];
	if ours != api_months.as_slice() {
		differences.push(format!("months: ours {ours:?}, Actual's {api_months:?}"));
	}
	for (label, actual) in months {
		let month = YearMonth::parse(label).unwrap();
		let Some(ours) = sheet.month(month) else {
			differences.push(format!("{label}: missing from the sheet"));
			continue;
		};
		let mut check = |what: String, ours: Value, theirs: &Value| {
			if &ours != theirs {
				differences.push(format!("{label} {what}: ours {ours}, Actual's {theirs}"));
			}
		};
		let s = ours.summary;
		let money = |m: veritas_domain::Money| Value::from(m.minor());
		check(
			"incomeAvailable".into(),
			money(s.available_funds),
			&actual["incomeAvailable"],
		);
		check(
			"lastMonthOverspent".into(),
			money(s.last_month_overspent),
			&actual["lastMonthOverspent"],
		);
		check(
			"totalBudgeted".into(),
			money(s.total_budgeted),
			&actual["totalBudgeted"],
		);
		check("toBudget".into(), money(s.to_budget), &actual["toBudget"]);
		check(
			"fromLastMonth".into(),
			money(s.from_last_month),
			&actual["fromLastMonth"],
		);
		check(
			"totalIncome".into(),
			money(s.total_income),
			&actual["totalIncome"],
		);
		check(
			"totalSpent".into(),
			money(s.total_spent),
			&actual["totalSpent"],
		);
		check(
			"totalBalance".into(),
			money(s.total_balance),
			&actual["totalBalance"],
		);
		let hold = match budget.holds.get(&month) {
			None => Value::from(0),
			Some(None) => Value::Null,
			Some(Some(hold)) => money(*hold),
		};
		check("forNextMonth".into(), hold, &actual["forNextMonth"]);

		let groups = actual["groups"].as_object().expect("groups");
		if groups.len() != ours.groups.len() {
			check(
				"group count".into(),
				Value::from(ours.groups.len()),
				&Value::from(groups.len()),
			);
		}
		for (id, theirs) in groups {
			let Some(group) = ours
				.groups
				.get(&veritas_domain::CategoryGroupId::new(id.clone()))
			else {
				check(
					format!("group {id}"),
					Value::from("missing"),
					&Value::from("present"),
				);
				continue;
			};
			if theirs.get("received").is_some() {
				check(
					format!("group {id} received"),
					money(s.total_income),
					&theirs["received"],
				);
			} else {
				check(
					format!("group {id} budgeted"),
					money(group.budgeted),
					&theirs["budgeted"],
				);
				check(
					format!("group {id} spent"),
					money(group.spent),
					&theirs["spent"],
				);
				check(
					format!("group {id} balance"),
					money(group.balance),
					&theirs["balance"],
				);
			}
		}
		let categories = actual["categories"].as_object().expect("categories");
		if categories.len() != ours.categories.len() {
			check(
				"category count".into(),
				Value::from(ours.categories.len()),
				&Value::from(categories.len()),
			);
		}
		for (id, theirs) in categories {
			let Some(category) = ours
				.categories
				.get(&veritas_domain::CategoryId::new(id.clone()))
			else {
				check(
					format!("category {id}"),
					Value::from("missing"),
					&Value::from("present"),
				);
				continue;
			};
			if theirs.get("received").is_some() {
				check(
					format!("category {id} received"),
					money(category.spent),
					&theirs["received"],
				);
			} else {
				check(
					format!("category {id} budgeted"),
					money(category.budgeted),
					&theirs["budgeted"],
				);
				check(
					format!("category {id} spent"),
					money(category.spent),
					&theirs["spent"],
				);
				check(
					format!("category {id} balance"),
					money(category.balance),
					&theirs["balance"],
				);
				check(
					format!("category {id} carryover"),
					Value::from(category.carryover),
					&theirs["carryover"],
				);
			}
		}
	}
	differences
}

/// The sheet's `today` is the oracle's pinned clock.
pub fn today(dump: &Value) -> veritas_domain::CivilDate {
	let text = dump["computed"]["today"].as_str().unwrap();
	let parts: Vec<u32> = text.split('-').map(|p| p.parse().unwrap()).collect();
	veritas_domain::CivilDate::new(parts[0] as i32, parts[1], parts[2]).unwrap()
}
