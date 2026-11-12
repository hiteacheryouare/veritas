//! PLAN §6.4 family 2 and WS-ENGINE's done-when (R-BUD-1, R-BUD-4): random
//! op sequences run in Veritas and, through `oracle replay`, in Actual, each
//! from a synthetic fixture loaded by veritas-store. Afterwards every month of the
//! sheet, every `zero_budgets` and `zero_budget_months` row and every budget
//! month note must match.
//!
//! `VERITAS_ORACLE_SESSIONS` sets how many sequences run (default 100 per
//! fixture; the done-when run is 10000) and `VERITAS_ORACLE_SEED` picks a different set.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod oracle;
mod session;

use std::collections::BTreeMap;

use proptest::strategy::{Strategy, ValueTree};
use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
use serde_json::{Value, json};
use session::{Op, Session, Universe, op_strategy};
use veritas_domain::{AccountId, BudgetData};

const CHUNK: usize = 250;

fn setting(name: &str, default: u64) -> u64 {
	std::env::var(name)
		.ok()
		.and_then(|v| v.parse().ok())
		.unwrap_or(default)
}

/// The live expense categories, the first income category and the first
/// on-budget account of the loaded file.
fn universe(budget: &BudgetData) -> Universe {
	let live = |c: &&veritas_domain::Category| !c.tombstone;
	let expenses = budget
		.categories
		.values()
		.filter(live)
		.filter(|c| !c.is_income)
		.map(|c| c.id.clone())
		.collect();
	let income = budget
		.categories
		.values()
		.filter(live)
		.find(|c| c.is_income)
		.unwrap()
		.id
		.clone();
	let account: AccountId = budget
		.accounts
		.values()
		.find(|a| !a.off_budget && !a.closed && !a.tombstone)
		.unwrap()
		.id
		.clone();
	Universe {
		expenses,
		income,
		account,
	}
}

/// Every difference between Actual's rows after replay and the engine's
/// budget, keyed the way Actual looks rows up.
fn compare_rows(budget: &BudgetData, actual: &Value) -> Vec<String> {
	let mut differences = Vec::new();
	let theirs: BTreeMap<(i64, String), (i64, i64)> = actual["zeroBudgets"]
		.as_array()
		.unwrap()
		.iter()
		.map(|row| {
			let key = (
				row["month"].as_i64().unwrap(),
				row["category"].as_str().unwrap().to_owned(),
			);
			(
				key,
				(
					row["amount"].as_i64().unwrap_or(0),
					row["carryover"].as_i64().unwrap_or(0),
				),
			)
		})
		.collect();
	let ours: BTreeMap<(i64, String), (i64, i64)> = budget
		.budget_cells
		.values()
		.map(|cell| {
			let month = i64::from(cell.month.year()) * 100 + i64::from(cell.month.month());
			(
				(month, cell.category.as_str().to_owned()),
				(cell.budgeted.minor(), i64::from(cell.carryover)),
			)
		})
		.collect();
	if theirs != ours {
		for key in theirs
			.keys()
			.chain(ours.keys())
			.collect::<std::collections::BTreeSet<_>>()
		{
			if theirs.get(key) != ours.get(key) {
				differences.push(format!(
					"zero_budgets {key:?}: ours {:?}, Actual's {:?}",
					ours.get(key),
					theirs.get(key)
				));
			}
		}
	}
	let theirs: BTreeMap<String, Option<i64>> = actual["zeroBudgetMonths"]
		.as_array()
		.unwrap()
		.iter()
		.map(|row| {
			(
				row["id"].as_str().unwrap().to_owned(),
				row["buffered"].as_i64(),
			)
		})
		.collect();
	let ours: BTreeMap<String, Option<i64>> = budget
		.holds
		.iter()
		.map(|(month, hold)| (month.to_string(), hold.map(|h| h.minor())))
		.collect();
	if theirs != ours {
		differences.push(format!(
			"zero_budget_months: ours {ours:?}, Actual's {theirs:?}"
		));
	}
	let theirs: BTreeMap<String, Option<String>> = actual["budgetNotes"]
		.as_array()
		.unwrap()
		.iter()
		.map(|row| {
			(
				row["id"].as_str().unwrap().to_owned(),
				row["note"].as_str().map(str::to_owned),
			)
		})
		.collect();
	let ours: BTreeMap<String, Option<String>> = budget
		.notes
		.iter()
		.filter(|(id, _)| id.starts_with("budget-"))
		.map(|(id, n)| (id.clone(), n.clone()))
		.collect();
	if theirs != ours {
		differences.push(format!("budget notes: ours {ours:?}, Actual's {theirs:?}"));
	}
	differences
}

struct Case {
	ops: Vec<Op>,
	session: Session,
}

#[test]
fn random_sessions_from_small_match_actual() {
	random_sessions_match_actual("small");
}

/// Hidden groups, a merged category, splits, transfers and off-budget accounts.
#[test]
fn random_sessions_from_all_features_match_actual() {
	random_sessions_match_actual("all-features");
}

fn random_sessions_match_actual(fixture: &str) {
	if !oracle::available() {
		return;
	}
	let zip = oracle::fixture(fixture);
	let dump = oracle::dump(&zip);
	let today = oracle::today(&dump);
	let loaded = oracle::load(&zip);
	let universe = universe(&loaded);
	let months = Session::new(loaded.clone(), today, universe.clone()).month_count();
	let strategy = proptest::collection::vec(op_strategy(months, universe.expenses.len()), 1..30);

	let count = setting("VERITAS_ORACLE_SESSIONS", 100) as usize;
	let seed = setting("VERITAS_ORACLE_SEED", 1);
	let mut seed_bytes = [0u8; 32];
	seed_bytes[..8].copy_from_slice(&seed.to_le_bytes());
	let mut runner = TestRunner::new_with_rng(
		Config::default(),
		TestRng::from_seed(RngAlgorithm::ChaCha, &seed_bytes),
	);

	let cases: Vec<Case> = (0..count)
		.map(|_| {
			let ops = strategy.new_tree(&mut runner).unwrap().current();
			let mut session = Session::new(loaded.clone(), today, universe.clone());
			for op in &ops {
				session.run(op);
			}
			Case { ops, session }
		})
		.collect();

	let parallel = std::thread::available_parallelism()
		.map_or(4, |n| n.get())
		.min(8);
	let chunks: Vec<&[Case]> = cases.chunks(CHUNK).collect();
	let mut results: Vec<Vec<Value>> = vec![Vec::new(); chunks.len()];
	for (batch, slots) in chunks.chunks(parallel).zip(results.chunks_mut(parallel)) {
		std::thread::scope(|scope| {
			for (chunk, slot) in batch.iter().zip(slots.iter_mut()) {
				scope.spawn(|| {
					let sessions: Vec<Value> = chunk
						.iter()
						.map(|c| json!(c.session.oracle_ops()))
						.collect();
					*slot = oracle::replay(&zip, &sessions);
				});
			}
		});
	}

	let mut failures = Vec::new();
	for (index, (case, actual)) in cases.iter().zip(results.into_iter().flatten()).enumerate() {
		let mut differences = Vec::new();
		if let Some(error) = actual["error"].as_str() {
			differences.push(format!("Actual refused an op: {error}"));
		}
		differences.extend(oracle::compare_months(
			&case.session.sheet,
			&case.session.budget,
			&actual["months"],
		));
		differences.extend(compare_rows(&case.session.budget, &actual));
		if !differences.is_empty() {
			failures.push(format!(
				"session {index} ({} ops {:?})\n  oracle ops: {}\n  {}",
				case.ops.len(),
				case.ops,
				json!(case.session.oracle_ops()),
				differences
					.iter()
					.take(8)
					.cloned()
					.collect::<Vec<_>>()
					.join("\n  ")
			));
		}
	}
	assert!(
		failures.is_empty(),
		"{fixture}: {} of {count} sessions differ from Actual (seed {seed}). First:\n{}",
		failures.len(),
		failures
			.iter()
			.take(3)
			.cloned()
			.collect::<Vec<_>>()
			.join("\n\n")
	);
	eprintln!("{fixture}: {count} random sessions match Actual (seed {seed})");
}
