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
		differences.extend(oracle::compare_rows(&case.session.budget, &actual));
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
