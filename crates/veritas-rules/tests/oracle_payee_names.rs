//! R-RULE-1 for `set payee_name` against the oracle, on a payee list as it
//! looks after a few bank imports: case-only duplicates, one pair already
//! merged, names whose lowercase changes their length, and renames to payees
//! that don't exist yet. Whenever Veritas runs every rule, the payee and
//! notes it ends with must be the ones Actual stored.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;

use common::{
	apply_and_dump, oracle_dir, repo_root, rule_from_row, rule_transaction, scratch_dir, uuid,
};
use serde_json::{Value, json};
use veritas_domain::{Account, AccountId, BudgetData, BudgetType, Payee, PayeeId, RuleId};
use veritas_rules::{BudgetPayees, RuleBook};

const TODAY: &str = "2026-11-09";

/// Every payee, mapping and rule row as Actual stored it.
fn budget_from(account: &str, dump: &Value) -> BudgetData {
	let mut budget = BudgetData::empty(BudgetType::Envelope);
	budget.accounts.insert(
		AccountId::new(account),
		Account {
			id: AccountId::new(account),
			name: account.to_owned(),
			off_budget: false,
			closed: false,
			group: None,
			sort_order: 0.0,
			tombstone: false,
		},
	);
	let rows = |table: &str| {
		dump["tables"][table]["rows"]
			.as_array()
			.cloned()
			.unwrap_or_default()
	};
	for row in rows("payees") {
		let id = PayeeId::new(row["id"].as_str().unwrap());
		budget.payees.insert(
			id.clone(),
			Payee {
				id,
				name: row["name"].as_str().unwrap_or_default().to_owned(),
				transfer_account: row["transfer_acct"].as_str().map(AccountId::new),
				favorite: row["favorite"] == 1,
				learn_categories: row["learn_categories"] == 1,
				tombstone: row["tombstone"] == 1,
			},
		);
	}
	for row in rows("payee_mapping") {
		budget.payee_mapping.insert(
			PayeeId::new(row["id"].as_str().unwrap()),
			PayeeId::new(row["targetId"].as_str().unwrap()),
		);
	}
	for row in rows("rules") {
		let rule = rule_from_row(&row);
		budget.rules.insert(rule.id.clone(), rule);
	}
	budget
}

#[test]
fn renames_agree_with_actual_unless_the_name_is_ambiguous() {
	let Some(oracle) = oracle_dir() else {
		eprintln!(
			"skipping the payee name oracle test: tools/actual-oracle/node_modules is missing, run `cargo xtask oracle setup`"
		);
		return;
	};
	let account = uuid(1, 1);
	let payee = |n: usize| uuid(4, n);
	let mut ops = vec![
		json!({"op": "createAccount", "id": account, "name": "Joint checking", "offBudget": false}),
	];
	for (n, name) in [
		"Amazon",
		"AMAZON",
		"École du ski",
		"Straße Bäckerei",
		"İstanbul Grill",
		"Joe's Pizza",
		"JOE'S PIZZA",
	]
	.iter()
	.enumerate()
	{
		ops.push(json!({"op": "createPayee", "id": payee(n), "name": name}));
	}
	// The user already merged the pizza duplicate, so only one is live.
	ops.push(json!({"op": "mergePayees", "target": payee(5), "merge": [payee(6)]}));

	let rename = |n: usize, text: &str, name: &str| {
		json!({"op": "createRule", "id": uuid(8, n), "stage": "pre",
			"conditions": [{"op": "contains", "field": "imported_payee", "value": text, "type": "string"}],
			"actions": [{"op": "set", "field": "payee_name", "value": name}]})
	};
	ops.extend([
		rename(0, "amzn", "amazon"),
		rename(1, "ecole", "ÉCOLE DU SKI"),
		rename(2, "baeckerei", "straße bäckerei"),
		rename(3, "strasse", "STRASSE BÄCKEREI"),
		rename(4, "istanbul", "i\u{307}stanbul grill"),
		rename(5, "joes", "joe's pizza"),
		rename(6, "corner", "Corner Store"),
		rename(7, "the store", "CORNER STORE"),
		rename(8, "clear me", ""),
	]);
	// Default-stage rules see the payee the pre rules picked, by id and name.
	ops.push(json!({"op": "createRule", "id": uuid(8, 9),
		"conditions": [{"op": "is", "field": "payee", "value": payee(2), "type": "id"}],
		"actions": [{"op": "append-notes", "value": " [ski id]"}]}));
	ops.push(json!({"op": "createRule", "id": uuid(8, 10),
		"conditions": [{"op": "matches", "field": "payee_name", "value": "^corner", "type": "string"}],
		"actions": [{"op": "append-notes", "value": " [corner name]"}]}));

	let transactions: Vec<Value> = [
		("AMZN Mktp US", None),
		("ECOLE DU SKI 42", None),
		("Baeckerei Munich", None),
		("STRASSE 9", None),
		("ISTANBUL GRILL", Some(payee(0))),
		("JOES PIZZA #7", None),
		("Corner shop", None),
		("THE STORE 2", None),
		("CLEAR ME", Some(payee(2))),
		("corner again", None),
	]
	.iter()
	.enumerate()
	.map(|(n, (text, starting_payee))| {
		let mut transaction = json!({"id": uuid(9, n), "date": "2026-11-01", "amount": -100,
			"cleared": true, "imported_payee": text});
		if let Some(id) = starting_payee {
			transaction["payee"] = json!(id);
		}
		transaction
	})
	.collect();

	// Veritas reads the budget as it was before the import, so it can't
	// find the payees Actual's rules created during it.
	let scratch = scratch_dir("payee-names");
	let before = apply_and_dump(
		&oracle,
		&scratch,
		"before",
		&repo_root().join("fixtures/synthetic/small.zip"),
		ops,
		TODAY,
	);
	let dump = apply_and_dump(
		&oracle,
		&scratch,
		"after",
		&scratch.join("before.zip"),
		vec![
			json!({"op": "addTransactions", "account": account, "transactions": transactions, "runTransfers": false}),
		],
		TODAY,
	);
	let _ = std::fs::remove_dir_all(&scratch);
	let budget = budget_from(&account, &before);
	assert!(budget.payees[&PayeeId::new(payee(6))].tombstone);
	let book = RuleBook::new(&budget);
	let rows: BTreeMap<&str, &Value> = dump["tables"]["transactions"]["rows"]
		.as_array()
		.unwrap()
		.iter()
		.filter_map(|row| Some((row["id"].as_str()?, row)))
		.collect();
	let actual_payee_name = |id: &Value| {
		dump["tables"]["payees"]["rows"]
			.as_array()
			.unwrap()
			.iter()
			.find(|row| row["id"] == *id)
			.map(|row| row["name"].as_str().unwrap().to_owned())
	};

	// One directory for the whole batch, as `addTransactions` sees the
	// payees each earlier transaction's rules created.
	let mut made = 0;
	let mut payees = BudgetPayees::new(&budget, || {
		made += 1;
		PayeeId::new(format!("made-{made}"))
	});
	let mut wrong = Vec::new();
	for input in &transactions {
		let outcome = book.run(&rule_transaction(input, &account), &budget, &mut payees);
		let row = rows[input["id"].as_str().unwrap()];
		let text = &input["imported_payee"];
		if text == "AMZN Mktp US" {
			assert_eq!(outcome.undetermined, [RuleId::new(uuid(8, 0))]);
			assert!(
				outcome.reasons[&RuleId::new(uuid(8, 0))].contains("more than one payee"),
				"{:?}",
				outcome.reasons
			);
			assert!(
				[json!(payee(0)), json!(payee(1))].contains(&row["description"]),
				"Actual picked one of the two Amazons: {}",
				row["description"]
			);
			continue;
		}
		if !outcome.undetermined.is_empty() {
			wrong.push(format!("{text}: undetermined {:?}", outcome.reasons));
			continue;
		}
		let veritas_payee = outcome.partial.payee.as_ref().map(|id| {
			let created = payees.created.iter().find(|(made, _)| made == id);
			match (budget.payees.get(id), created) {
				(Some(payee), _) => payee.name.clone(),
				(None, Some((_, name))) => name.clone(),
				(None, None) => format!("unknown id {id:?}"),
			}
		});
		let actual_payee = row["description"]
			.as_str()
			.map(|_| actual_payee_name(&row["description"]).unwrap_or_default());
		if veritas_payee != actual_payee {
			wrong.push(format!(
				"{text}: Actual's payee {actual_payee:?}, Veritas {veritas_payee:?}"
			));
		}
		if json!(outcome.partial.notes) != row["notes"] {
			wrong.push(format!(
				"{text}: Actual's notes {}, Veritas {:?}",
				row["notes"], outcome.partial.notes
			));
		}
	}
	assert!(wrong.is_empty(), "{}", wrong.join("\n"));
	assert_eq!(
		payees
			.created
			.iter()
			.map(|(_, name)| name.as_str())
			.collect::<Vec<_>>(),
		["STRASSE BÄCKEREI", "Corner Store"],
		"ß doesn't fold to ss, and later renames find the payee an earlier one made"
	);
}
