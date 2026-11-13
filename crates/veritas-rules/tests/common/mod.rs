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

/// `None` when `cargo xtask oracle setup` hasn't been run, so the caller
/// skips. With `VERITAS_REQUIRE_ORACLE` set (CI), a missing oracle fails the
/// test instead, so differential tests can't pass by not running.
pub fn oracle_dir() -> Option<PathBuf> {
	let dir = repo_root().join("tools/actual-oracle");
	if dir.join("node_modules").exists() {
		return Some(dir);
	}
	assert!(
		std::env::var_os("VERITAS_REQUIRE_ORACLE").is_none(),
		"VERITAS_REQUIRE_ORACLE is set but tools/actual-oracle/node_modules is missing: run `cargo xtask oracle setup`"
	);
	None
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
		splits: Vec::new(),
		is_child: false,
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

/// Like `apply_and_dump`, for an op file written by hand so its JSON keeps
/// a chosen key order.
pub fn apply_text_and_dump(
	oracle: &Path,
	scratch: &Path,
	label: &str,
	base: &Path,
	ops_text: &str,
	today: &str,
) -> Value {
	let ops_path = scratch.join(format!("{label}.json"));
	let zip_path = scratch.join(format!("{label}.zip"));
	std::fs::write(&ops_path, ops_text).unwrap();
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

pub fn apply_only(oracle: &Path, base: &Path, ops: Vec<Value>, out: &Path, today: &str) {
	let ops_path = out.with_extension("json");
	std::fs::write(
		&ops_path,
		serde_json::json!({"version": 1, "ops": ops}).to_string(),
	)
	.unwrap();
	node(
		oracle,
		&[
			"apply",
			&base.to_string_lossy(),
			&ops_path.to_string_lossy(),
			"--out",
			&out.to_string_lossy(),
			"--today",
			today,
		],
	);
}

fn rows<'a>(dump: &'a Value, table: &str) -> impl Iterator<Item = &'a Value> {
	dump["tables"][table]["rows"]
		.as_array()
		.into_iter()
		.flatten()
}

fn date_int(value: &Value) -> Option<veritas_domain::CivilDate> {
	let n = value.as_i64()?;
	veritas_domain::CivilDate::new(
		i32::try_from(n / 10_000).ok()?,
		u32::try_from(n / 100 % 100).ok()?,
		u32::try_from(n % 100).ok()?,
	)
	.ok()
}

/// The budget the store would load from a dump: what rules and schedules
/// read. Next dates come from what Actual computed (`v_schedules`).
pub fn budget_from_dump(dump: &Value) -> veritas_domain::BudgetData {
	use veritas_domain::*;
	let text = |row: &Value, key: &str| row[key].as_str().map(str::to_owned);
	let flag = |row: &Value, key: &str| row[key] == 1;
	let mut budget = BudgetData::empty(BudgetType::Envelope);
	for row in rows(dump, "accounts") {
		let id = AccountId::new(row["id"].as_str().unwrap());
		budget.accounts.insert(
			id.clone(),
			Account {
				last_reconciled: None,
				id,
				name: text(row, "name").unwrap_or_default(),
				off_budget: flag(row, "offbudget"),
				closed: flag(row, "closed"),
				group: None,
				sort_order: row["sort_order"].as_f64().unwrap_or_default(),
				tombstone: flag(row, "tombstone"),
			},
		);
	}
	for row in rows(dump, "payees") {
		let id = PayeeId::new(row["id"].as_str().unwrap());
		budget.payees.insert(
			id.clone(),
			Payee {
				id,
				name: text(row, "name").unwrap_or_default(),
				transfer_account: text(row, "transfer_acct").map(AccountId::new),
				favorite: flag(row, "favorite"),
				learn_categories: flag(row, "learn_categories"),
				tombstone: flag(row, "tombstone"),
			},
		);
	}
	for row in rows(dump, "payee_mapping") {
		budget.payee_mapping.insert(
			PayeeId::new(row["id"].as_str().unwrap()),
			PayeeId::new(row["targetId"].as_str().unwrap()),
		);
	}
	for row in rows(dump, "category_mapping") {
		budget.category_mapping.insert(
			CategoryId::new(row["id"].as_str().unwrap()),
			CategoryId::new(row["transferId"].as_str().unwrap()),
		);
	}
	for row in rows(dump, "categories") {
		let id = CategoryId::new(row["id"].as_str().unwrap());
		budget.categories.insert(
			id.clone(),
			Category {
				id,
				name: text(row, "name").unwrap_or_default(),
				group: CategoryGroupId::new(row["cat_group"].as_str().unwrap_or_default()),
				is_income: flag(row, "is_income"),
				hidden: flag(row, "hidden"),
				sort_order: row["sort_order"].as_f64().unwrap_or_default(),
				tombstone: flag(row, "tombstone"),
			},
		);
	}
	for row in rows(dump, "rules") {
		let id = RuleId::new(row["id"].as_str().unwrap());
		budget.rules.insert(
			id.clone(),
			Rule {
				id,
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
				conditions_json: text(row, "conditions").unwrap_or_default(),
				actions_json: text(row, "actions").unwrap_or_default(),
				tombstone: flag(row, "tombstone"),
			},
		);
	}
	for row in rows(dump, "schedules") {
		let id = ScheduleId::new(row["id"].as_str().unwrap());
		let computed = &dump["computed"]["schedules"][id.as_str()];
		budget.schedules.insert(
			id.clone(),
			Schedule {
				id,
				name: text(row, "name"),
				rule: RuleId::new(row["rule"].as_str().unwrap_or_default()),
				posts_transaction: flag(row, "posts_transaction"),
				completed: flag(row, "completed"),
				next_date: computed["nextDate"]
					.as_str()
					.and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
					.map(CivilDate::from),
				tombstone: flag(row, "tombstone"),
			},
		);
	}
	for row in rows(dump, "transactions") {
		let id = TransactionId::new(row["id"].as_str().unwrap());
		let Some(date) = date_int(&row["date"]) else {
			continue;
		};
		budget.transactions.insert(
			id.clone(),
			Transaction {
				id,
				account: AccountId::new(row["acct"].as_str().unwrap_or_default()),
				date,
				amount: Money::from_minor(row["amount"].as_i64().unwrap_or_default()),
				payee: text(row, "description").map(PayeeId::new),
				imported_payee: text(row, "imported_description"),
				category: text(row, "category").map(CategoryId::new),
				notes: text(row, "notes"),
				cleared: flag(row, "cleared"),
				reconciled: flag(row, "reconciled"),
				split: if flag(row, "isParent") {
					SplitRole::Parent
				} else if flag(row, "isChild") {
					SplitRole::Child {
						parent: TransactionId::new(row["parent_id"].as_str().unwrap_or_default()),
					}
				} else {
					SplitRole::None
				},
				transfer: text(row, "transferred_id").map(TransactionId::new),
				schedule: text(row, "schedule").map(ScheduleId::new),
				starting_balance: flag(row, "starting_balance_flag"),
				sort_order: row["sort_order"].as_f64().unwrap_or_default(),
				tombstone: flag(row, "tombstone"),
			},
		);
	}
	budget
}
