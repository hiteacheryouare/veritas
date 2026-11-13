//! R-RULE-1: rules that name payees or categories which were later merged or
//! deleted. Actual never rewrites the rules; it maps their ids through
//! `payee_mapping` and `category_mapping` when it loads them (`migrateIds`).
//! Payees are merged through the oracle's `mergePayees`, twice in a chain,
//! and a category is deleted into another, then the same transactions run
//! through Actual and Veritas.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{
	apply_and_dump, budget_from_dump, expected_row, oracle_dir, repo_root, rule_transaction,
	scratch_dir, uuid,
};
use serde_json::{Value, json};
use veritas_domain::PayeeId;
use veritas_rules::{BudgetPayees, RuleBook};

const TODAY: &str = "2026-11-09";

fn payee(n: usize) -> String {
	uuid(4, n)
}

fn category(n: usize) -> String {
	uuid(3, n)
}

#[test]
fn rules_follow_merged_payees_and_deleted_categories_like_actual() {
	let Some(oracle) = oracle_dir() else {
		eprintln!(
			"skipping the merge oracle test: tools/actual-oracle/node_modules is missing, run `cargo xtask oracle setup`"
		);
		return;
	};
	let account = uuid(1, 1);
	let mut ops = vec![
		json!({"op": "createAccount", "id": account, "name": "Checking"}),
		json!({"op": "createCategoryGroup", "id": uuid(2, 1), "name": "Merge test"}),
	];
	// 0 Amazon, 1 "AMZN Mktp" (a duplicate), 2 Coffee, 3 "Amazon.com" (the final target).
	for (n, name) in ["Amazon", "AMZN Mktp", "Coffee", "Amazon.com"]
		.iter()
		.enumerate()
	{
		ops.push(json!({"op": "createPayee", "id": payee(n), "name": name}));
	}
	for n in 0..3 {
		ops.push(json!({"op": "createCategory", "id": category(n), "name": format!("Merge category {n}"), "group": uuid(2, 1)}));
	}
	let rules = [
		(
			json!([{"op": "is", "field": "payee", "value": payee(1)}]),
			json!([{"op": "set", "field": "category", "value": category(0)}]),
		),
		(
			json!([{"op": "oneOf", "field": "payee", "value": [payee(1), payee(0), payee(2)]}]),
			json!([{"op": "append-notes", "value": " #oneof"}]),
		),
		(
			json!([{"op": "isNot", "field": "payee", "value": payee(0)}]),
			json!([{"op": "append-notes", "value": " #not-amazon"}]),
		),
		(
			json!([{"op": "notOneOf", "field": "payee", "value": [payee(1)]}]),
			json!([{"op": "prepend-notes", "value": "N: "}]),
		),
		(
			json!([{"op": "contains", "field": "imported_payee", "value": "shop"}]),
			json!([{"op": "set", "field": "payee", "value": payee(1)}]),
		),
		(
			json!([{"op": "contains", "field": "imported_payee", "value": "deli"}]),
			json!([{"op": "set", "field": "category", "value": category(1)}]),
		),
		(
			json!([{"op": "is", "field": "category", "value": category(1)}]),
			json!([{"op": "set", "field": "cleared", "value": true}]),
		),
		(
			json!([{"op": "is", "field": "category", "value": category(2)}]),
			json!([{"op": "append-notes", "value": " #cat2"}]),
		),
	];
	for (n, (conditions, actions)) in rules.iter().enumerate() {
		ops.push(
			json!({"op": "createRule", "id": uuid(8, n), "conditions": conditions, "actions": actions}),
		);
	}
	// The duplicate merges into Amazon, then Amazon into Amazon.com, which
	// re-points the first mapping too. Category 1 is deleted into category 2.
	ops.push(json!({"op": "mergePayees", "target": payee(0), "merge": [payee(1)]}));
	ops.push(json!({"op": "mergePayees", "target": payee(3), "merge": [payee(0)]}));
	ops.push(json!({"op": "deleteCategory", "id": category(1), "transferCategory": category(2)}));

	let mut inputs = Vec::new();
	for (n, (imported, payee_n, category_n)) in [
		("AMAZON MKTP", Some(3), None),
		("AMAZON MKTP", Some(0), None),
		("AMZN", Some(1), None),
		("Coffee", Some(2), None),
		("Corner shop", None, None),
		("Deli on 5th", None, None),
		("Deli on 5th", Some(2), Some(1)),
		("Unknown", None, Some(2)),
		("Unknown", None, None),
	]
	.into_iter()
	.enumerate()
	{
		let mut t = json!({"id": uuid(9, n), "date": "2026-11-01", "amount": -1000 - n as i64, "cleared": false, "imported_payee": imported});
		if let Some(p) = payee_n {
			t["payee"] = json!(payee(p));
		}
		if let Some(c) = category_n {
			t["category"] = json!(category(c));
		}
		inputs.push(t);
	}
	ops.push(json!({"op": "addTransactions", "account": account, "transactions": inputs, "runTransfers": false}));

	let scratch = scratch_dir("merges");
	let dump = apply_and_dump(
		&oracle,
		&scratch,
		"merges",
		&repo_root().join("fixtures/synthetic/small.zip"),
		ops,
		TODAY,
	);
	let _ = std::fs::remove_dir_all(&scratch);

	let budget = budget_from_dump(&dump);
	assert_eq!(
		budget.payee_mapping[&PayeeId::new(payee(1))],
		PayeeId::new(payee(3)),
		"the second merge re-points the first"
	);
	let book = RuleBook::new(&budget);
	let rows: std::collections::BTreeMap<&str, &Value> = dump["tables"]["transactions"]["rows"]
		.as_array()
		.unwrap()
		.iter()
		.map(|row| (row["id"].as_str().unwrap(), row))
		.collect();
	let mut mismatches = Vec::new();
	let mut applied = 0;
	for input in &inputs {
		let outcome = book.run(
			&rule_transaction(input, &account),
			&budget,
			&mut BudgetPayees::new(&budget, || PayeeId::new("made")),
		);
		assert!(outcome.undetermined.is_empty());
		applied += outcome.applied.len();
		let veritas = expected_row(&outcome.partial, &account);
		let row = rows[input["id"].as_str().unwrap()];
		let actual: serde_json::Map<String, Value> = veritas
			.as_object()
			.unwrap()
			.keys()
			.map(|key| (key.clone(), row[key.as_str()].clone()))
			.collect();
		if Value::Object(actual.clone()) != veritas {
			mismatches.push(format!(
				"{input}\n  actual:  {}\n  veritas: {veritas}",
				Value::Object(actual)
			));
		}
	}
	eprintln!(
		"{} transactions, {applied} rule applications through merged ids, {} differ",
		inputs.len(),
		mismatches.len()
	);
	assert!(applied >= inputs.len(), "too few rules fired: {applied}");
	assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}
