//! R-RULE-1 for split actions. Rules that split a transaction by fixed
//! amounts, percentages and remainders run in Actual through
//! `addTransactions` and in Veritas; the parent and every line must agree,
//! including the rounding of odd cents and Actual's quirks when a second
//! rule splits a transaction that is already split.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{
	apply_and_dump, budget_from_dump, oracle_dir, repo_root, rule_transaction, scratch_dir, uuid,
};
use serde_json::{Value, json};
use veritas_domain::{CategoryId, PayeeId};
use veritas_rules::{
	BudgetPayees, ParsedRule, RuleBook, RuleSupport, RuleTransaction, added_transaction,
};

const TODAY: &str = "2026-11-09";

fn category(n: usize) -> String {
	uuid(3, n)
}

fn payee(n: usize) -> String {
	uuid(4, n)
}

fn line_amount(split: u32, method: &str, value: Value) -> Value {
	json!({"op": "set-split-amount", "value": value, "options": {"splitIndex": split, "method": method}})
}

fn on_line(split: u32, field: &str, value: Value) -> Value {
	json!({"op": "set", "field": field, "value": value, "options": {"splitIndex": split}})
}

fn contains(text: &str) -> Value {
	json!({"op": "contains", "field": "imported_payee", "value": text})
}

fn rules() -> Vec<(Option<&'static str>, Vec<Value>, Vec<Value>)> {
	vec![
		// Half groceries, half household at Costco.
		(
			None,
			vec![contains("costco")],
			vec![
				line_amount(1, "fixed-percent", json!(50)),
				on_line(1, "category", json!(category(0))),
				line_amount(2, "remainder", Value::Null),
				on_line(2, "category", json!(category(1))),
			],
		),
		// A paycheck: a fixed amount to savings, 10% to retirement, the rest as pay.
		(
			None,
			vec![contains("payroll")],
			vec![
				line_amount(1, "fixed-amount", json!(50_000)),
				on_line(1, "category", json!(category(2))),
				line_amount(2, "fixed-percent", json!(10)),
				on_line(2, "category", json!(category(3))),
				line_amount(3, "remainder", Value::Null),
			],
		),
		// Dinner split three ways, each line with its own payee and note.
		(
			None,
			vec![json!({"op": "contains", "field": "notes", "value": "dinner"})],
			vec![
				line_amount(1, "remainder", Value::Null),
				on_line(1, "payee", json!(payee(1))),
				on_line(1, "notes", json!("Alice")),
				line_amount(2, "remainder", Value::Null),
				on_line(2, "payee", json!(payee(2))),
				line_amount(3, "remainder", Value::Null),
				json!({"op": "append-notes", "value": " (Carol)", "options": {"splitIndex": 3}}),
			],
		),
		// A category for the whole purchase, then a 30/70 split.
		(
			None,
			vec![contains("target")],
			vec![
				json!({"op": "set", "field": "category", "value": category(3)}),
				line_amount(1, "fixed-percent", json!(30)),
				line_amount(2, "remainder", Value::Null),
				on_line(2, "category", json!(category(1))),
			],
		),
		// Runs after the splits and sees the split parent, whose payee is now gone.
		(
			Some("post"),
			vec![json!({"op": "lt", "field": "amount", "value": 0})],
			vec![json!({"op": "append-notes", "value": " #checked"})],
		),
		(
			Some("post"),
			vec![json!({"op": "is", "field": "payee", "value": null})],
			vec![json!({"op": "set", "field": "cleared", "value": true})],
		),
		// A second split rule on an already split Costco run.
		(
			Some("post"),
			vec![contains("costco wholesale")],
			vec![
				line_amount(1, "fixed-amount", json!(-100)),
				on_line(1, "category", json!(category(2))),
			],
		),
	]
}

fn transactions() -> Vec<Value> {
	let mut out = Vec::new();
	let mut add = |imported: &str, amount: i64, payee_n: Option<usize>, notes: Option<&str>| {
		let n = out.len();
		let mut t = json!({"id": uuid(9, n), "date": "2026-11-01", "amount": amount, "cleared": false, "imported_payee": imported});
		if let Some(p) = payee_n {
			t["payee"] = json!(payee(p));
		}
		if let Some(notes) = notes {
			t["notes"] = json!(notes);
		}
		out.push(t);
	};
	for amount in [-21_480, -10_001, -333, -1, 0, -99_999, 7] {
		add("COSTCO #123", amount, Some(0), None);
		add("Costco Wholesale 9", amount, None, None);
		add("TARGET T-100", amount, Some(3), Some("school"));
		add("Bistro", amount, None, Some("dinner with friends"));
	}
	for amount in [250_000, 310_001, 49_999, 50_000, 3] {
		add("ACME PAYROLL", amount, Some(2), None);
	}
	add("Corner shop", -1250, Some(1), None);
	out
}

fn setup_ops() -> Vec<Value> {
	let mut ops = vec![
		json!({"op": "createAccount", "id": uuid(1, 1), "name": "Checking"}),
		json!({"op": "createCategoryGroup", "id": uuid(2, 1), "name": "Split test"}),
	];
	for n in 0..4 {
		ops.push(json!({"op": "createCategory", "id": category(n), "name": format!("Split category {n}"), "group": uuid(2, 1)}));
		ops.push(json!({"op": "createPayee", "id": payee(n), "name": format!("Split payee {n}")}));
	}
	ops
}

fn veritas_rows(outcome: &RuleTransaction, account: &str) -> (Value, Vec<Value>) {
	let parent = json!({
		"amount": outcome.amount.minor(),
		"category": outcome.category.as_ref().map(CategoryId::as_str),
		"cleared": i64::from(outcome.cleared == Some(true)),
		"description": outcome.payee.as_ref().map(PayeeId::as_str),
		"isParent": i64::from(!outcome.splits.is_empty()),
		"notes": outcome.notes.as_deref(),
		"error": outcome.split_difference().map(|difference| difference.minor()),
	});
	let lines = outcome
		.splits
		.iter()
		.enumerate()
		.map(|(n, line)| {
			json!({
				"acct": account,
				"amount": line.amount.minor(),
				"category": line.category.as_ref().map(CategoryId::as_str),
				"cleared": i64::from(outcome.cleared == Some(true)),
				"description": line.payee.as_ref().map(PayeeId::as_str),
				"isChild": 1,
				"notes": line.notes.as_deref(),
				"sort_order": 0.0 - n as f64,
			})
		})
		.collect();
	(parent, lines)
}

/// The stored parent and lines, with the parent's `error` reduced to its
/// `difference` so both sides read the same.
fn actual_rows(
	dump: &Value,
	id: &str,
	parent_keys: &Value,
	line_keys: &Value,
) -> (Value, Vec<Value>) {
	let rows: Vec<&Value> = dump["tables"]["transactions"]["rows"]
		.as_array()
		.unwrap()
		.iter()
		.collect();
	let pick = |row: &Value, keys: &Value| -> Value {
		Value::Object(
			keys.as_object()
				.unwrap()
				.keys()
				.map(|key| (key.clone(), row[key.as_str()].clone()))
				.collect(),
		)
	};
	let parent_row = rows.iter().find(|row| row["id"] == id).unwrap();
	let mut parent = pick(parent_row, parent_keys);
	parent["error"] = match parent_row["error"].as_str() {
		Some(text) => serde_json::from_str::<Value>(text).unwrap()["difference"].clone(),
		None => Value::Null,
	};
	let mut lines: Vec<&&Value> = rows
		.iter()
		.filter(|row| row["parent_id"] == id && row["tombstone"] == 0)
		.collect();
	lines.sort_by_key(|row| -row["sort_order"].as_f64().unwrap() as i64);
	let lines = lines.iter().map(|row| pick(row, line_keys)).collect();
	(parent, lines)
}

#[test]
fn split_rules_match_the_oracle() {
	let Some(oracle) = oracle_dir() else {
		eprintln!(
			"skipping the split oracle test: tools/actual-oracle/node_modules is missing, run `cargo xtask oracle setup`"
		);
		return;
	};
	let mut ops = setup_ops();
	for (n, (stage, conditions, actions)) in rules().into_iter().enumerate() {
		ops.push(json!({"op": "createRule", "id": uuid(8, n), "stage": stage, "conditions": conditions, "actions": actions}));
	}
	let inputs = transactions();
	ops.push(json!({"op": "addTransactions", "account": uuid(1, 1), "transactions": inputs, "runTransfers": false}));
	let scratch = scratch_dir("splits");
	let dump = apply_and_dump(
		&oracle,
		&scratch,
		"splits",
		&repo_root().join("fixtures/synthetic/small.zip"),
		ops,
		TODAY,
	);
	let _ = std::fs::remove_dir_all(&scratch);

	let budget = budget_from_dump(&dump);
	for rule in budget.rules.values() {
		assert_eq!(
			ParsedRule::parse(rule).support(),
			RuleSupport::Full,
			"{}",
			rule.actions_json
		);
	}
	let book = RuleBook::new(&budget);
	let account = uuid(1, 1);
	let mut mismatches = Vec::new();
	let mut split = 0;
	for input in &inputs {
		let outcome = book.run(
			&rule_transaction(input, &account),
			&budget,
			&mut BudgetPayees::new(&budget, || PayeeId::new("made")),
		);
		assert!(
			outcome.undetermined.is_empty(),
			"{input}: {:?}",
			outcome.undetermined
		);
		let ruled = added_transaction(
			outcome
				.transaction
				.expect("every split rule here runs in Veritas"),
			&veritas_domain::AccountId::new(&account),
		);
		let (parent, lines) = veritas_rows(&ruled, &account);
		split += usize::from(!lines.is_empty());
		let line_keys = lines.first().cloned().unwrap_or(json!({}));
		let (actual_parent, actual_lines) =
			actual_rows(&dump, input["id"].as_str().unwrap(), &parent, &line_keys);
		if parent != actual_parent || lines != actual_lines {
			mismatches.push(format!(
				"{input}\n  actual:  {actual_parent} {}\n  veritas: {parent} {}",
				json!(actual_lines),
				json!(lines)
			));
		}
	}
	eprintln!(
		"{} transactions, {split} split by rules, {} differ",
		inputs.len(),
		mismatches.len()
	);
	assert!(
		split > inputs.len() / 2,
		"too few splits to prove much: {split}"
	);
	assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}
