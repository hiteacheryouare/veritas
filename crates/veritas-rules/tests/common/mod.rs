//! What the oracle differential tests share: finding the oracle, running it,
//! a seeded generator, and reading rule and transaction rows the way
//! `addTransactions` wrote them.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};
use veritas_domain::{
	AccountId, CategoryId, CivilDate, ConditionsOp, Money, PayeeId, Rule, RuleId, RuleStage,
	ScheduleId,
};
use veritas_rules::RuleTransaction;

pub fn repo_root() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `None` when `cargo xtask oracle setup` hasn't been run, so the caller skips.
pub fn oracle_dir() -> Option<PathBuf> {
	let dir = repo_root().join("tools/actual-oracle");
	dir.join("node_modules").exists().then_some(dir)
}

pub fn scratch_dir(name: &str) -> PathBuf {
	let dir = std::env::temp_dir().join(format!("veritas-rules-{name}-{}", std::process::id()));
	std::fs::create_dir_all(&dir).unwrap();
	dir
}

fn node(oracle: &Path, args: &[&str]) -> String {
	let mut command = Command::new("node");
	command.current_dir(oracle).arg("src/main.ts").args(args);
	let output = command
		.output()
		.unwrap_or_else(|error| panic!("running {command:?}: {error}"));
	assert!(
		output.status.success(),
		"{command:?} failed:\n{}",
		String::from_utf8_lossy(&output.stderr)
	);
	String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Applies `ops` to `base` with Actual and returns the canonical dump of the
/// result, both at the pinned `today`.
pub fn apply_and_dump(
	oracle: &Path,
	scratch: &Path,
	label: &str,
	base: &Path,
	ops: Vec<Value>,
	today: &str,
) -> Value {
	let ops_path = scratch.join(format!("{label}.json"));
	let zip_path = scratch.join(format!("{label}.zip"));
	std::fs::write(
		&ops_path,
		serde_json::json!({"version": 1, "ops": ops}).to_string(),
	)
	.unwrap();
	let (base, ops_path, zip_path) = (
		base.to_string_lossy().into_owned(),
		ops_path.to_string_lossy().into_owned(),
		zip_path.to_string_lossy().into_owned(),
	);
	node(
		oracle,
		&[
			"apply", &base, &ops_path, "--out", &zip_path, "--today", today,
		],
	);
	dump(oracle, &zip_path, today)
}

pub fn dump(oracle: &Path, zip: &str, today: &str) -> Value {
	serde_json::from_str(&node(oracle, &["dump", zip, "--today", today])).unwrap()
}

/// xorshift64*: deterministic, so a failure names a case that reproduces.
pub struct Rng(pub u64);

impl Rng {
	pub fn next(&mut self) -> u64 {
		self.0 ^= self.0 >> 12;
		self.0 ^= self.0 << 25;
		self.0 ^= self.0 >> 27;
		self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
	}

	pub fn below(&mut self, n: u64) -> u64 {
		self.next() % n
	}

	pub fn chance(&mut self, percent: u64) -> bool {
		self.below(100) < percent
	}

	pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
		&items[self.below(items.len() as u64) as usize]
	}
}

pub fn uuid(kind: u32, n: usize) -> String {
	format!("{kind:08x}-0000-4000-8000-{n:012x}")
}

pub fn rule_from_row(row: &Value) -> Rule {
	Rule {
		id: RuleId::new(row["id"].as_str().unwrap()),
		stage: match row["stage"].as_str() {
			Some("pre" | "cleanup" | "modify") => RuleStage::Pre,
			Some("post") => RuleStage::Post,
			_ => RuleStage::Default,
		},
		conditions_op: if row["conditions_op"] == "or" {
			ConditionsOp::Or
		} else {
			ConditionsOp::And
		},
		conditions_json: row["conditions"].as_str().unwrap_or_default().to_owned(),
		actions_json: row["actions"].as_str().unwrap_or_default().to_owned(),
		tombstone: row["tombstone"] == 1,
	}
}

/// What `addTransactions` hands `runRules`: the account it was added to,
/// a trimmed imported payee, and nothing for keys that weren't given.
pub fn rule_transaction(input: &Value, account: &str) -> RuleTransaction {
	let text = |key: &str| input[key].as_str().map(str::to_owned);
	RuleTransaction {
		account: AccountId::new(account),
		date: CivilDate::from(
			chrono::NaiveDate::parse_from_str(input["date"].as_str().unwrap(), "%Y-%m-%d").unwrap(),
		),
		amount: Money::from_minor(input["amount"].as_i64().unwrap()),
		payee: text("payee").map(PayeeId::new),
		category: text("category").map(CategoryId::new),
		notes: text("notes"),
		imported_payee: text("imported_payee")
			.map(|payee| payee.trim().to_owned())
			.filter(|payee| !payee.is_empty()),
		cleared: input["cleared"].as_bool(),
		reconciled: None,
		schedule: None,
		deleted: false,
	}
}

/// The columns `addTransactions` writes after the rules ran: the account
/// is forced back to the one being added to, and `cleared` defaults to true.
pub fn expected_row(outcome: &RuleTransaction, account: &str) -> Value {
	json!({
		"acct": account,
		"amount": outcome.amount.minor(),
		"category": outcome.category.as_ref().map(CategoryId::as_str),
		"cleared": i64::from(outcome.cleared.unwrap_or(true)),
		"date": outcome.date.to_string().replace('-', "").parse::<i64>().unwrap(),
		"description": outcome.payee.as_ref().map(PayeeId::as_str),
		"imported_description": outcome.imported_payee.as_deref(),
		"notes": outcome.notes.as_deref(),
		"schedule": outcome.schedule.as_ref().map(ScheduleId::as_str),
		"tombstone": i64::from(outcome.deleted),
	})
}
