//! R-RULE-1 and R-RULE-3 for split rules, bills left to Actual and rows
//! typed by hand, against the oracle. Split rules are generated (bounded and
//! seeded) on awkward amounts, combined with transfers to other accounts, and
//! run through `addTransactions` in Actual and through Veritas; a bill whose
//! rule can't run in Veritas is left whole, and Actual finishing it must end
//! where Actual alone would.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::Path;

use common::{
	Rng, apply_and_dump, budget_from_dump, dump, oracle_dir, repo_root, rule_transaction,
	scratch_dir, uuid,
};
use serde_json::{Value, json};
use veritas_domain::{AccountId, BudgetData, CategoryId, CivilDate, Money, PayeeId, ScheduleId};
use veritas_rules::{
	BudgetPayees, NextDateWrite, ParsedRule, RuleBook, RuleSupport, RuleTransaction, ScheduleWrite,
	added_transaction, advance_schedules, counterpart, manual_entry,
};

const TODAY: &str = "2026-11-09";
const GENERATED_RULES: usize = 70;

fn category(n: usize) -> String {
	uuid(3, n)
}

fn payee(n: usize) -> String {
	uuid(4, n)
}

fn day(text: &str) -> CivilDate {
	CivilDate::from(chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap())
}

fn line_amount(split: usize, method: &str, value: Value) -> Value {
	json!({"op": "set-split-amount", "value": value, "options": {"splitIndex": split, "method": method}})
}

fn on_line(split: usize, op: &str, field: &str, value: Value) -> Value {
	json!({"op": op, "field": field, "value": value, "options": {"splitIndex": split}})
}

/// Every split action Actual runs without crashing: lines are opened in
/// order (an action past the end adds one line), and actions may come back
/// to earlier lines. Percentages include fractions and values over 100.
fn random_split_actions(rng: &mut Rng) -> Vec<Value> {
	let mut actions = Vec::new();
	if rng.chance(20) {
		actions.push(
			json!({"op": "set", "field": "category", "value": category(rng.below(4) as usize)}),
		);
	}
	if rng.chance(10) {
		actions.push(line_amount(
			0,
			"fixed-amount",
			json!(*rng.pick(&[-5_000, -4_211, 9_999])),
		));
	}
	let lines = 1 + rng.below(4) as usize;
	for line in 1..=lines {
		actions.push(match rng.below(3) {
			0 => line_amount(
				line,
				"fixed-amount",
				json!(*rng.pick(&[-1, -333, -1_000, -2_499, 7, 0, -12_345])),
			),
			1 => line_amount(
				line,
				"fixed-percent",
				json!(*rng.pick(&[10.0, 12.5, 33.333, 50.0, 0.5, 99.0, 100.0, 150.0, 66.67])),
			),
			_ => line_amount(line, "remainder", Value::Null),
		});
		if rng.chance(40) {
			actions.push(on_line(
				line,
				"set",
				"category",
				json!(category(rng.below(4) as usize)),
			));
		}
		if rng.chance(25) {
			actions.push(on_line(
				line,
				"set",
				"payee",
				json!(payee(rng.below(4) as usize)),
			));
		}
		if rng.chance(20) {
			let op = *rng.pick(&["set", "append-notes", "prepend-notes"]);
			actions.push(on_line(line, op, "notes", json!(format!("line {line}"))));
		}
		if line > 1 && rng.chance(15) {
			let earlier = 1 + rng.below(line as u64 - 1) as usize;
			actions.push(on_line(earlier, "append-notes", "notes", json!(" again")));
		}
	}
	actions
}

const AMOUNTS: [i64; 14] = [
	-1, -2, -3, -5, 7, 0, -333, -10_001, -12_345, -99_999, 250_000, 310_001, -21_480, 1,
];

/// The parent and its lines as Veritas would store them, in the columns the
/// dump compares.
fn veritas_rows(outcome: &RuleTransaction) -> (Value, Vec<Value>) {
	let parent = json!({
		"amount": outcome.amount.minor(),
		"category": outcome.category.as_ref().map(CategoryId::as_str),
		"description": outcome.payee.as_ref().map(PayeeId::as_str),
		"isParent": i64::from(!outcome.splits.is_empty()),
		"notes": outcome.notes.as_deref(),
		"error": outcome.split_difference().unwrap().map(Money::minor),
	});
	let lines = outcome
		.splits
		.iter()
		.map(|line| {
			json!({
				"amount": line.amount.minor(),
				"category": line.category.as_ref().map(CategoryId::as_str),
				"description": line.payee.as_ref().map(PayeeId::as_str),
				"notes": line.notes.as_deref(),
			})
		})
		.collect();
	(parent, lines)
}

fn rows(dump: &Value) -> &Vec<Value> {
	dump["tables"]["transactions"]["rows"].as_array().unwrap()
}

fn pick(row: &Value, keys: &[&str]) -> Value {
	Value::Object(
		keys.iter()
			.map(|key| ((*key).to_owned(), row[*key].clone()))
			.collect(),
	)
}

/// The stored parent and its live lines in line order, with the parent's
/// split `error` reduced to its `difference`.
fn actual_rows<'a>(dump: &'a Value, id: &str) -> (Value, Vec<&'a Value>) {
	let parent_row = rows(dump).iter().find(|row| row["id"] == id).unwrap();
	let mut parent = pick(
		parent_row,
		&["amount", "category", "description", "isParent", "notes"],
	);
	parent["error"] = match parent_row["error"].as_str() {
		Some(text) => serde_json::from_str::<Value>(text).unwrap()["difference"].clone(),
		None => Value::Null,
	};
	let mut lines: Vec<&Value> = rows(dump)
		.iter()
		.filter(|row| row["parent_id"] == id && row["tombstone"] == 0)
		.collect();
	lines.sort_by(|a, b| {
		b["sort_order"]
			.as_f64()
			.partial_cmp(&a["sort_order"].as_f64())
			.unwrap()
	});
	(parent, lines)
}

fn line_keys(line: &Value) -> Value {
	pick(line, &["amount", "category", "description", "notes"])
}

fn base_ops(accounts: &[(String, &str)]) -> Vec<Value> {
	let mut ops: Vec<Value> = accounts
		.iter()
		.map(|(id, name)| json!({"op": "createAccount", "id": id, "name": name}))
		.collect();
	ops.push(json!({"op": "createCategoryGroup", "id": uuid(2, 1), "name": "Split journeys"}));
	for n in 0..4 {
		ops.push(json!({"op": "createCategory", "id": category(n), "name": format!("Split journey {n}"), "group": uuid(2, 1)}));
		ops.push(
			json!({"op": "createPayee", "id": payee(n), "name": format!("Split journey payee {n}")}),
		);
	}
	ops
}

/// Generated split rules, one per tag, some followed by a second split rule
/// in the post stage on the already split result, on odd amounts.
#[test]
fn generated_split_rules_match_the_oracle_to_the_cent() {
	let Some(oracle) = oracle_dir() else {
		eprintln!(
			"skipping the split journey oracle test: tools/actual-oracle/node_modules is missing, run `cargo xtask oracle setup`"
		);
		return;
	};
	let account = uuid(1, 1);
	let mut ops = base_ops(&[(account.clone(), "Joint checking")]);
	let mut rng = Rng(0x5eed_0005_9117_0001);
	let mut inputs = Vec::new();
	for n in 0..GENERATED_RULES {
		let tag = format!("tag{n:03}x");
		ops.push(json!({"op": "createRule", "id": uuid(8, n),
			"conditions": [{"op": "contains", "field": "imported_payee", "value": tag}],
			"actions": random_split_actions(&mut rng)}));
		let twice = rng.chance(25);
		if twice {
			ops.push(
				json!({"op": "createRule", "id": uuid(8, 1_000 + n), "stage": "post",
				"conditions": [{"op": "contains", "field": "imported_payee", "value": format!("{tag}2")}],
				"actions": random_split_actions(&mut rng)}),
			);
		}
		for k in 0..2 {
			let suffix = if twice && k == 1 { "2" } else { "" };
			let mut t = json!({"id": uuid(9, inputs.len()), "date": "2026-11-01", "amount": *rng.pick(&AMOUNTS),
				"cleared": false, "imported_payee": format!("{tag}{suffix} purchase")});
			if rng.chance(60) {
				t["payee"] = json!(payee(rng.below(4) as usize));
			}
			if rng.chance(40) {
				t["category"] = json!(category(rng.below(4) as usize));
			}
			if rng.chance(30) {
				t["notes"] = json!("receipt");
			}
			inputs.push(t);
		}
	}
	ops.push(json!({"op": "addTransactions", "account": account, "transactions": inputs, "runTransfers": false}));
	let scratch = scratch_dir("split-sweep");
	let dump = apply_and_dump(
		&oracle,
		&scratch,
		"split-sweep",
		&repo_root().join("fixtures/synthetic/small.zip"),
		ops,
		TODAY,
	);
	let _ = std::fs::remove_dir_all(&scratch);

	let budget = budget_from_dump(&dump);
	let book = RuleBook::new(&budget);
	let mut mismatches = Vec::new();
	let (mut split, mut lines_seen) = (0, 0);
	for input in &inputs {
		let outcome = book.run(
			&rule_transaction(input, &account),
			&budget,
			&mut BudgetPayees::new(&budget, || PayeeId::new("made")),
		);
		let id = input["id"].as_str().unwrap();
		let (actual_parent, actual_lines) = actual_rows(&dump, id);
		let Some(ruled) = outcome.transaction else {
			mismatches.push(format!("{input}: undetermined {:?}", outcome.reasons));
			continue;
		};
		let (parent, lines) = veritas_rows(&added_transaction(ruled, &AccountId::new(&account)));
		let actual_lines: Vec<Value> = actual_lines.iter().map(|line| line_keys(line)).collect();
		split += usize::from(!lines.is_empty());
		lines_seen += lines.len();
		if parent != actual_parent || lines != actual_lines {
			mismatches.push(format!(
				"{input}\n  actual:  {actual_parent} {}\n  veritas: {parent} {}",
				json!(actual_lines),
				json!(lines)
			));
		}
	}
	eprintln!(
		"{} transactions, {split} split into {lines_seen} lines, {} differ",
		inputs.len(),
		mismatches.len()
	);
	assert!(split > inputs.len() * 3 / 4, "too few splits: {split}");
	assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

/// Split lines that are transfers: Actual makes each such line a transfer
/// with its own leg in the other account, built and run through the rules as
/// `addTransfer` does (`transfer.ts:47-93`), and clears the line's category
/// when both accounts are on budget.
#[test]
fn split_lines_that_are_transfers_get_legs_like_actual() {
	let Some(oracle) = oracle_dir() else {
		return;
	};
	let (checking, savings, brokerage) = (uuid(1, 1), uuid(1, 2), uuid(1, 3));
	let scratch = scratch_dir("split-transfers");
	let base = base_ops(&[
		(checking.clone(), "Joint checking"),
		(savings.clone(), "Rainy day"),
		(brokerage.clone(), "Index funds"),
	]);
	let setup = apply_and_dump(
		&oracle,
		&scratch,
		"accounts",
		&repo_root().join("fixtures/synthetic/small.zip"),
		base,
		TODAY,
	);
	let transfer_payee = |account: &str| {
		rows_of(&setup, "payees")
			.find(|row| row["transfer_acct"] == account)
			.unwrap()["id"]
			.as_str()
			.unwrap()
			.to_owned()
	};
	let (to_savings, to_brokerage) = (transfer_payee(&savings), transfer_payee(&brokerage));
	let ops = vec![
		// Payday: a fixed amount to savings, 10% to investing, the rest as pay.
		json!({"op": "createRule", "id": uuid(8, 1),
		"conditions": [{"op": "contains", "field": "imported_payee", "value": "payroll"}],
		"actions": [
			line_amount(1, "fixed-amount", json!(50_000)),
			on_line(1, "set", "payee", json!(to_savings)),
			on_line(1, "set", "notes", json!("auto-save")),
			line_amount(2, "fixed-percent", json!(10)),
			on_line(2, "set", "payee", json!(to_brokerage)),
			line_amount(3, "remainder", Value::Null),
		]}),
		// A transfer that a rule splits: most to savings, the rest to investing.
		json!({"op": "createRule", "id": uuid(8, 2),
		"conditions": [{"op": "contains", "field": "imported_payee", "value": "move money"}],
		"actions": [
			line_amount(1, "fixed-percent", json!(60)),
			line_amount(2, "remainder", Value::Null),
			on_line(2, "set", "payee", json!(to_brokerage)),
		]}),
		// The rules Actual runs on each new leg.
		json!({"op": "createRule", "id": uuid(8, 3),
			"conditions": [{"op": "is", "field": "account", "value": savings}],
			"actions": [{"op": "append-notes", "value": " (saved)"}, {"op": "set", "field": "cleared", "value": true}]}),
		json!({"op": "addTransactions", "account": checking, "runTransfers": true, "transactions": [
			{"id": uuid(9, 0), "date": "2026-11-06", "amount": 312_345, "cleared": false, "imported_payee": "ACME PAYROLL", "payee": payee(2), "category": category(0)},
			{"id": uuid(9, 1), "date": "2026-11-07", "amount": -100_001, "cleared": false, "imported_payee": "move money", "payee": to_savings},
			{"id": uuid(9, 2), "date": "2026-11-08", "amount": 1, "cleared": false, "imported_payee": "ACME PAYROLL bonus", "payee": payee(2)},
		]}),
	];
	let dump = apply_and_dump(
		&oracle,
		&scratch,
		"transfers",
		&scratch.join("accounts.zip"),
		ops,
		TODAY,
	);
	let _ = std::fs::remove_dir_all(&scratch);

	let budget = budget_from_dump(&dump);
	let book = RuleBook::new(&budget);
	let from_checking = transfer_payee(&checking);
	let mut mismatches = Vec::new();
	let mut legs = 0;
	for n in 0..3 {
		let id = uuid(9, n);
		let stored = rows(&dump)
			.iter()
			.find(|row| row["id"] == id.as_str())
			.unwrap();
		let mut input = json!({"id": id, "date": "", "amount": stored["amount"], "imported_payee": stored["imported_description"], "cleared": false});
		input["date"] = json!(
			CivilDate::new(
				i32::try_from(stored["date"].as_i64().unwrap() / 10_000).unwrap(),
				u32::try_from(stored["date"].as_i64().unwrap() / 100 % 100).unwrap(),
				u32::try_from(stored["date"].as_i64().unwrap() % 100).unwrap(),
			)
			.unwrap()
			.to_string()
		);
		input["payee"] = json!([payee(2), to_savings.clone(), payee(2)][n]);
		if n == 0 {
			input["category"] = json!(category(0));
		}
		let outcome = book.run(
			&rule_transaction(&input, &checking),
			&budget,
			&mut BudgetPayees::new(&budget, || PayeeId::new("made")),
		);
		let ruled = added_transaction(outcome.transaction.unwrap(), &AccountId::new(&checking));
		let (_, actual_lines) = actual_rows(&dump, &id);
		assert_eq!(ruled.splits.len(), actual_lines.len(), "{input}");
		for (line, actual) in ruled.splits.iter().zip(&actual_lines) {
			let other = line
				.payee
				.as_ref()
				.and_then(|p| budget.payees.get(p))
				.and_then(|p| p.transfer_account.clone());
			let mut expected = json!({
				"amount": line.amount.minor(),
				"category": line.category.as_ref().map(CategoryId::as_str),
				"description": line.payee.as_ref().map(PayeeId::as_str),
				"notes": line.notes.as_deref(),
			});
			let Some(other) = other else {
				if expected != line_keys(actual) {
					mismatches.push(format!("{input}: line {} vs {expected}", line_keys(actual)));
				}
				continue;
			};
			// Both accounts are on budget, so Actual clears the line's category.
			expected["category"] = Value::Null;
			if expected != line_keys(actual) {
				mismatches.push(format!(
					"{input}: transfer line {} vs {expected}",
					line_keys(actual)
				));
			}
			let mirror = RuleTransaction {
				account: other.clone(),
				amount: Money::from_minor(-line.amount.minor()),
				payee: Some(PayeeId::new(&from_checking)),
				category: None,
				notes: line.notes.clone().filter(|notes| !notes.is_empty()),
				imported_payee: None,
				cleared: Some(false),
				splits: Vec::new(),
				is_child: false,
				..ruled.clone()
			};
			let leg_rules = book
				.run(
					&mirror,
					&budget,
					&mut BudgetPayees::new(&budget, || PayeeId::new("made")),
				)
				.transaction
				.unwrap();
			let leg = counterpart(&mirror, &leg_rules);
			let stored_leg = rows(&dump)
				.iter()
				.find(|row| row["transferred_id"] == actual["id"])
				.unwrap_or_else(|| panic!("Actual made no leg for {actual}"));
			let expected_leg = json!({
				"acct": other.as_str(),
				"amount": leg.amount.minor(),
				"description": leg.payee.as_ref().map(PayeeId::as_str),
				"notes": leg.notes.as_deref(),
				"cleared": i64::from(leg.cleared == Some(true)),
				"category": Value::Null,
				"isChild": 0,
			});
			let actual_leg = pick(
				stored_leg,
				&[
					"acct",
					"amount",
					"description",
					"notes",
					"cleared",
					"category",
					"isChild",
				],
			);
			if expected_leg != actual_leg {
				mismatches.push(format!("{input}: leg {actual_leg} vs {expected_leg}"));
			}
			legs += 1;
		}
	}
	assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
	// Payday's remainder line copies the investing line's transfer payee, so
	// both paydays make three legs, and the split transfer two.
	assert_eq!(legs, 8, "every transfer line gets one leg");
}

fn rows_of<'a>(dump: &'a Value, table: &str) -> impl Iterator<Item = &'a Value> {
	dump["tables"][table]["rows"]
		.as_array()
		.into_iter()
		.flatten()
}

/// Every oracle run seeds Actual's ids the same way by default, so chained
/// runs need their own seed or a new row reuses an earlier row's id.
fn apply_seeded(oracle: &Path, base: &Path, ops: Vec<Value>, out: &Path, today: &str, seed: u32) {
	let ops_path = out.with_extension("json");
	std::fs::write(&ops_path, json!({"version": 1, "ops": ops}).to_string()).unwrap();
	let output = std::process::Command::new("node")
		.current_dir(oracle)
		.arg("src/main.ts")
		.arg("apply")
		.arg(base)
		.arg(&ops_path)
		.arg("--out")
		.arg(out)
		.args(["--today", today, "--seed", &seed.to_string()])
		.output()
		.unwrap();
	assert!(
		output.status.success(),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
}

type Posted = (
	String,
	i64,
	i64,
	Option<String>,
	Option<String>,
	Option<String>,
);

fn posted(dump: &Value) -> Vec<Posted> {
	let mut out: Vec<Posted> = rows(dump)
		.iter()
		.filter(|row| row["tombstone"] == 0 && row["schedule"].is_string())
		.map(|row| {
			(
				row["schedule"].as_str().unwrap().to_owned(),
				row["date"].as_i64().unwrap(),
				row["amount"].as_i64().unwrap(),
				row["description"].as_str().map(str::to_owned),
				row["notes"].as_str().map(str::to_owned),
				row["category"].as_str().map(str::to_owned),
			)
		})
		.collect();
	out.sort();
	out
}

fn next_dates(dump: &Value) -> Value {
	dump["computed"]["schedules"].clone()
}

/// D60 on the daily run: a bill whose rule can't run in Veritas is left whole
/// (no posts, no next-date writes, no payees, and no `lastScheduleRun`), while
/// the other bill is posted. When Ryan then opens the budget in Actual, it
/// finishes the left bill and ends exactly where Actual alone would have.
#[test]
fn a_bill_left_to_actual_is_finished_by_actual_as_if_veritas_never_ran() {
	let Some(oracle) = oracle_dir() else {
		return;
	};
	let checking = uuid(1, 1);
	let (rent, gym) = (ScheduleId::new(uuid(10, 1)), ScheduleId::new(uuid(10, 2)));
	let monthly = |start: &str| {
		json!({"start": start, "interval": 1, "frequency": "monthly", "patterns": [], "skipWeekend": false,
			"weekendSolveMode": "before", "endMode": "never", "endOccurrences": 1, "endDate": start})
	};
	let mut ops = base_ops(&[(checking.clone(), "Joint checking")]);
	ops.extend([
		json!({"op": "createSchedule", "id": rent.as_str(), "name": "Rent", "postsTransaction": true,
			"payee": payee(0), "account": checking, "amount": -185_000, "amountOp": "is", "date": monthly("2026-09-01")}),
		json!({"op": "createSchedule", "id": gym.as_str(), "name": "Gym", "postsTransaction": true,
			"payee": payee(1), "account": checking, "amount": -4_250, "amountOp": "is", "date": monthly("2026-09-05")}),
		// The gym is renamed to a payee that doesn't exist yet. From October its
		// notes come from a template, which Veritas can't run, so September
		// posts fine in Veritas and October doesn't: the whole bill, its new
		// payee and September's post with it, goes back to Actual.
		json!({"op": "createRule", "id": uuid(8, 1), "stage": "pre", "conditions": [{"op": "is", "field": "payee", "value": payee(1)}],
			"actions": [{"op": "set", "field": "payee_name", "value": "Gym Partners"}]}),
		json!({"op": "createRule", "id": uuid(8, 2), "conditions": [{"op": "gte", "field": "date", "value": "2026-10-01"},
			{"op": "is", "field": "amount", "value": -4_250}],
			"actions": [{"op": "set", "field": "notes", "value": "", "options": {"template": "Gym {{date}}"}}]}),
	]);
	let scratch = scratch_dir("left-whole");
	let start = scratch.join("start.zip");
	apply_seeded(
		&oracle,
		&repo_root().join("fixtures/synthetic/small.zip"),
		ops,
		&start,
		"2026-08-20",
		11,
	);
	let today = "2026-11-10";
	let before = dump(&oracle, &start.to_string_lossy(), today);
	let budget: BudgetData = budget_from_dump(&before);
	let book = RuleBook::new(&budget);
	let mut payees = BudgetPayees::new(&budget, || PayeeId::new("made"));
	let plan = advance_schedules(&book, &budget, &mut payees, day(today));

	assert_eq!(plan.left.len(), 1);
	assert_eq!(plan.left[0].schedule, gym);
	assert!(
		plan.left[0].note().contains("a template action"),
		"{}",
		plan.left[0].note()
	);
	assert!(plan.posts.iter().all(|p| p.schedule == rent));
	assert_eq!(
		plan.posts.len(),
		3,
		"rent for September, October and November"
	);
	assert!(
		payees.created.is_empty(),
		"the gym's new payee is left to Actual too"
	);
	let mut skips = 0;
	for write in &plan.writes {
		match write {
			ScheduleWrite::NextDate {
				schedule,
				write: NextDateWrite::Local(_),
			} if *schedule == rent => skips += 1,
			other => panic!("only rent moves on, and today's run isn't marked done: {other:?}"),
		}
	}

	// Actual alone.
	let alone = scratch.join("alone.zip");
	apply_seeded(
		&oracle,
		&start,
		vec![json!({"op": "runSchedules"})],
		&alone,
		today,
		21,
	);
	// Veritas's plan, then Actual's next open. Rent has no rules but its own,
	// so adding the planned rows through `addTransactions` stores them as planned.
	let rows_json: Vec<Value> = plan
		.posts
		.iter()
		.enumerate()
		.map(|(n, p)| {
			let t = &p.transaction;
			json!({"id": uuid(9, n), "date": t.date.to_string(), "amount": t.amount.minor(),
				"payee": t.payee.as_ref().map(PayeeId::as_str), "schedule": rent.as_str(), "cleared": false})
		})
		.collect();
	let mut veritas_ops =
		vec![json!({"op": "addTransactions", "account": checking, "transactions": rows_json})];
	veritas_ops.extend((0..skips).map(|_| json!({"op": "skipSchedule", "id": rent.as_str()})));
	let veritas = scratch.join("veritas.zip");
	apply_seeded(&oracle, &start, veritas_ops, &veritas, today, 31);
	let finished = scratch.join("finished.zip");
	apply_seeded(
		&oracle,
		&veritas,
		vec![json!({"op": "runSchedules"})],
		&finished,
		today,
		41,
	);
	let (alone, finished) = (
		dump(&oracle, &alone.to_string_lossy(), today),
		dump(&oracle, &finished.to_string_lossy(), today),
	);
	let _ = std::fs::remove_dir_all(&scratch);

	let gym_posts = posted(&alone)
		.into_iter()
		.filter(|p| p.0 == gym.as_str())
		.count();
	assert_eq!(
		gym_posts, 3,
		"Actual posts the gym for September to November"
	);
	let without_payee = |posts: Vec<Posted>| -> Vec<Posted> {
		posts
			.into_iter()
			.map(|(s, d, a, _, n, c)| (s, d, a, None, n, c))
			.collect()
	};
	assert_eq!(
		without_payee(posted(&finished)),
		without_payee(posted(&alone)),
		"Actual finishing after Veritas posts what Actual alone posts"
	);
	let payee_names = |dump: &Value| -> Vec<String> {
		let mut names: Vec<String> = rows_of(dump, "payees")
			.filter(|row| row["tombstone"] == 0)
			.filter_map(|row| row["name"].as_str().map(str::to_owned))
			.collect();
		names.sort();
		names
	};
	assert_eq!(payee_names(&finished), payee_names(&alone));
	assert_eq!(next_dates(&finished), next_dates(&alone));
}

/// D64: a row typed by hand is saved exactly as typed when a rule can't run
/// on it, even though other rules, a split among them, would have filled it.
#[test]
fn a_typed_row_survives_an_undetermined_rule_untouched() {
	let Some(oracle) = oracle_dir() else {
		return;
	};
	let checking = uuid(1, 1);
	let mut ops = base_ops(&[(checking.clone(), "Joint checking")]);
	ops.extend([
		json!({"op": "createRule", "id": uuid(8, 1), "conditions": [{"op": "is", "field": "payee", "value": payee(3)}],
			"actions": [{"op": "set", "field": "category", "value": category(2)}]}),
		json!({"op": "createRule", "id": uuid(8, 2), "stage": "post", "conditions": [{"op": "is", "field": "payee", "value": payee(3)}],
			"actions": [line_amount(1, "fixed-percent", json!(80)), line_amount(2, "remainder", Value::Null),
				on_line(2, "set", "category", json!(category(1)))]}),
		json!({"op": "createRule", "id": uuid(8, 3), "conditions": [{"op": "contains", "field": "notes", "value": "tip"}],
			"actions": [{"op": "set", "field": "notes", "value": "", "options": {"template": "{{notes}} (tipped)"}}]}),
	]);
	let scratch = scratch_dir("typed-row");
	let dump = apply_and_dump(
		&oracle,
		&scratch,
		"typed-row",
		&repo_root().join("fixtures/synthetic/small.zip"),
		ops,
		TODAY,
	);
	let _ = std::fs::remove_dir_all(&scratch);
	let budget = budget_from_dump(&dump);
	let book = RuleBook::new(&budget);
	for rule in budget
		.rules
		.values()
		.filter(|rule| rule.id.as_str() != uuid(8, 3))
	{
		assert_eq!(ParsedRule::parse(rule).support(), RuleSupport::Full);
	}
	let typed = |notes: &str| RuleTransaction {
		account: AccountId::new(&checking),
		date: day("2026-11-08"),
		amount: Money::from_minor(-6_420),
		payee: Some(PayeeId::new(payee(3))),
		category: None,
		notes: Some(notes.to_owned()),
		imported_payee: None,
		cleared: Some(true),
		reconciled: None,
		schedule: None,
		deleted: false,
		splits: Vec::new(),
		is_child: false,
	};

	let with_tip = typed("dinner, big tip");
	let outcome = book.run(
		&with_tip,
		&budget,
		&mut BudgetPayees::new(&budget, || PayeeId::new("made")),
	);
	assert!(
		outcome.partial.category.is_some() || !outcome.partial.splits.is_empty(),
		"the other rules did run"
	);
	let saved = manual_entry(&with_tip, &outcome, &[]);
	assert_eq!(saved.transaction, with_tip, "saved exactly as typed");
	let note = saved.note.unwrap();
	assert!(
		note.contains("Run Rules") && note.contains("a template action"),
		"{note}"
	);

	let plain = typed("dinner");
	let outcome = book.run(
		&plain,
		&budget,
		&mut BudgetPayees::new(&budget, || PayeeId::new("made")),
	);
	let saved = manual_entry(&plain, &outcome, &[]);
	assert_eq!(saved.note, None);
	assert_eq!(
		saved.transaction.category,
		Some(CategoryId::new(category(2)))
	);
	assert_eq!(
		saved
			.transaction
			.splits
			.iter()
			.map(|l| l.amount.minor())
			.collect::<Vec<_>>(),
		[-5_136, -1_284],
		"80% and the rest of the typed amount"
	);
	assert_eq!(
		saved.transaction.payee,
		Some(PayeeId::new(payee(3))),
		"the typed payee stays"
	);
}
