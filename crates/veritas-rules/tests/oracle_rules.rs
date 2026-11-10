//! R-RULE-1 and R-RULE-3 against the oracle. Actual creates generated rules
//! and runs them on transactions added through `addTransactions`; Veritas
//! reads the rule rows Actual stored and runs the same transactions through
//! its own engine. Every resulting field must agree.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;

use common::{Rng, apply_and_dump, dump, oracle_dir, repo_root, scratch_dir, uuid};
use serde_json::{Value, json};
use veritas_domain::{
	Account, AccountId, BudgetData, BudgetType, Category, CategoryGroupId, CategoryId, CivilDate,
	ConditionsOp, Money, Payee, PayeeId, Rule, RuleId, RuleStage,
};
use veritas_rules::{ParsedRule, RuleBook, RuleSupport, RuleTransaction};

const TODAY: &str = "2026-11-09";
const RULES: usize = 70;
const TRANSACTIONS: usize = 400;

struct World {
	accounts: Vec<(String, bool)>,
	groups: Vec<String>,
	categories: Vec<(String, String)>,
	payees: Vec<(String, &'static str)>,
}

fn world() -> World {
	World {
		accounts: vec![(uuid(1, 1), false), (uuid(1, 2), true)],
		groups: vec![uuid(2, 1), uuid(2, 2)],
		categories: (0..4).map(|i| (uuid(3, i), uuid(2, 1 + i % 2))).collect(),
		payees: vec![
			(uuid(4, 0), "Amazon"),
			(uuid(4, 1), "Landlord"),
			(uuid(4, 2), "Coffee Shop"),
			(uuid(4, 3), "École"),
		],
	}
}

const IMPORTED: [&str; 9] = [
	"AMZN Mktp US",
	"amazon.com",
	"Starbucks #123",
	"LANDLORD LLC",
	"  École du ski ",
	"Zelle to Bob",
	"amzn digital",
	"Shell Oil",
	"",
];
const FRAGMENTS: [&str; 8] = [
	"amzn",
	"AMAZON",
	"star",
	"landlord llc",
	"école",
	"zelle",
	"oil",
	"s",
];
const NOTES: [&str; 6] = [
	"lunch #food",
	"#Food",
	"rent",
	"Coffee ##escaped #tag",
	"#food#work",
	"paid",
];
const AMOUNTS: [i64; 10] = [-4210, -100, 0, 100, 5000, -1535, -1650, 108, -108, -2500];

fn random_condition(rng: &mut Rng, world: &World) -> Value {
	let payee = |rng: &mut Rng| json!(rng.pick(&world.payees).0);
	let category = |rng: &mut Rng| json!(rng.pick(&world.categories).0);
	let account = |rng: &mut Rng| json!(rng.pick(&world.accounts).0);
	match rng.below(11) {
		0 | 1 => {
			let op = *rng.pick(&[
				"is",
				"isNot",
				"contains",
				"doesNotContain",
				"oneOf",
				"notOneOf",
			]);
			let value = match op {
				"is" | "isNot" => json!(rng.pick(&IMPORTED).trim()),
				"oneOf" | "notOneOf" => json!([rng.pick(&IMPORTED).trim(), rng.pick(&FRAGMENTS)]),
				_ => json!(rng.pick(&FRAGMENTS)),
			};
			json!({"op": op, "field": "imported_payee", "value": value, "type": "string"})
		}
		2 => {
			let op = *rng.pick(&["is", "isNot", "oneOf", "notOneOf"]);
			let value = match op {
				"oneOf" | "notOneOf" => json!([payee(rng), payee(rng)]),
				_ if rng.chance(15) => Value::Null,
				_ => payee(rng),
			};
			json!({"op": op, "field": "payee", "value": value, "type": "id"})
		}
		3 => {
			let op = *rng.pick(&["is", "contains", "doesNotContain", "isNot"]);
			let name = rng.pick(&world.payees).1;
			let value = if op == "is" || op == "isNot" {
				name.to_uppercase()
			} else {
				name[..3].to_lowercase()
			};
			json!({"op": op, "field": "payee_name", "value": value, "type": "string"})
		}
		4 => {
			let op = *rng.pick(&["contains", "is", "doesNotContain", "hasTags", "hasAnyTag"]);
			let value = match op {
				"hasTags" | "hasAnyTag" => {
					json!(rng.pick(&["#food", "#FOOD #work", "#tag", "##escaped", "#food #nope"]))
				}
				_ => json!(rng.pick(&["food", "LUNCH #food", "rent", "#", "paid"])),
			};
			json!({"op": op, "field": "notes", "value": value, "type": "string"})
		}
		5 | 6 => {
			let op = *rng.pick(&["is", "isapprox", "gt", "gte", "lt", "lte", "isbetween"]);
			let value = if op == "isbetween" {
				json!({"num1": rng.pick(&AMOUNTS), "num2": rng.pick(&AMOUNTS)})
			} else {
				json!(rng.pick(&AMOUNTS))
			};
			let mut condition =
				json!({"op": op, "field": "amount", "value": value, "type": "number"});
			if op != "isbetween" && rng.chance(35) {
				let sign = if rng.chance(50) { "inflow" } else { "outflow" };
				condition["options"] = json!({sign: true});
				condition["value"] = json!(rng.pick(&AMOUNTS).abs());
			}
			condition
		}
		7 => {
			let op = *rng.pick(&["is", "isapprox", "gt", "lte"]);
			let value = match op {
				"is" if rng.chance(50) => json!(rng.pick(&["2026-10", "2026", "2025"])),
				_ => json!(format!(
					"2026-{:02}-{:02}",
					10 + rng.below(2),
					1 + rng.below(28)
				)),
			};
			json!({"op": op, "field": "date", "value": value, "type": "date"})
		}
		8 => match rng.below(3) {
			0 => json!({"op": "is", "field": "account", "value": account(rng), "type": "id"}),
			1 => json!({"op": "onBudget", "field": "account", "value": null, "type": "id"}),
			_ => json!({"op": "offBudget", "field": "account", "value": null, "type": "id"}),
		},
		9 => {
			if rng.chance(30) {
				json!({"op": "is", "field": "category_group", "value": rng.pick(&world.groups), "type": "id"})
			} else {
				let op = *rng.pick(&["is", "isNot", "oneOf"]);
				let value = match op {
					"oneOf" => json!([category(rng), category(rng)]),
					_ if rng.chance(25) => Value::Null,
					_ => category(rng),
				};
				json!({"op": op, "field": "category", "value": value, "type": "id"})
			}
		}
		_ => json!({"op": "is", "field": "cleared", "value": rng.chance(50), "type": "boolean"}),
	}
}

fn random_action(rng: &mut Rng, world: &World) -> Value {
	match rng.below(9) {
		0 | 1 => {
			let value = if rng.chance(15) {
				Value::Null
			} else {
				json!(rng.pick(&world.categories).0)
			};
			json!({"op": "set", "field": "category", "value": value, "type": "id"})
		}
		2 => {
			json!({"op": "set", "field": "payee", "value": rng.pick(&world.payees).0, "type": "id"})
		}
		3 => json!({"op": "set", "field": "notes", "value": rng.pick(&NOTES), "type": "string"}),
		4 => json!({"op": "set", "field": "cleared", "value": rng.chance(50), "type": "boolean"}),
		5 => json!({"op": "append-notes", "value": " #auto"}),
		6 => json!({"op": "prepend-notes", "value": "Auto: "}),
		7 => json!({"op": "set", "field": "amount", "value": rng.pick(&AMOUNTS), "type": "number"}),
		_ => {
			json!({"op": "set", "field": "imported_payee", "value": rng.pick(&IMPORTED).trim(), "type": "string"})
		}
	}
}

fn random_transaction(rng: &mut Rng, world: &World, n: usize) -> Value {
	let mut transaction = json!({
		"id": uuid(9, n),
		"date": format!("2026-{:02}-{:02}", 10 + rng.below(2), 1 + rng.below(28)),
		"amount": if rng.chance(60) { *rng.pick(&AMOUNTS) } else { rng.below(20_000) as i64 - 10_000 },
		"cleared": rng.chance(50),
	});
	if rng.chance(85) {
		transaction["imported_payee"] = json!(rng.pick(&IMPORTED));
	}
	if rng.chance(45) {
		transaction["payee"] = json!(rng.pick(&world.payees).0);
	}
	if rng.chance(40) {
		transaction["notes"] = json!(rng.pick(&NOTES));
	}
	if rng.chance(25) {
		transaction["category"] = json!(rng.pick(&world.categories).0);
	}
	transaction
}

fn setup_ops(world: &World) -> Vec<Value> {
	let mut ops = Vec::new();
	for (i, (id, off_budget)) in world.accounts.iter().enumerate() {
		ops.push(json!({"op": "createAccount", "id": id, "name": format!("Account {i}"), "offBudget": off_budget}));
	}
	for (i, id) in world.groups.iter().enumerate() {
		ops.push(json!({"op": "createCategoryGroup", "id": id, "name": format!("Group {i}")}));
	}
	for (i, (id, group)) in world.categories.iter().enumerate() {
		ops.push(
			json!({"op": "createCategory", "id": id, "name": format!("Category {i}"), "group": group}),
		);
	}
	for (id, name) in &world.payees {
		ops.push(json!({"op": "createPayee", "id": id, "name": name}));
	}
	ops
}

/// The budget Veritas sees: the generated entities plus every rule row the
/// dump holds, read exactly as stored.
fn budget_from(world: &World, dump: &Value) -> BudgetData {
	let mut budget = BudgetData::empty(BudgetType::Envelope);
	for (id, off_budget) in &world.accounts {
		budget.accounts.insert(
			AccountId::new(id),
			Account {
				id: AccountId::new(id),
				name: id.clone(),
				off_budget: *off_budget,
				closed: false,
				group: None,
				sort_order: 0.0,
				tombstone: false,
			},
		);
	}
	for (id, group) in &world.categories {
		budget.categories.insert(
			CategoryId::new(id),
			Category {
				id: CategoryId::new(id),
				name: id.clone(),
				group: CategoryGroupId::new(group),
				is_income: false,
				hidden: false,
				sort_order: 0.0,
				tombstone: false,
			},
		);
	}
	for (id, name) in &world.payees {
		budget.payees.insert(
			PayeeId::new(id),
			Payee {
				id: PayeeId::new(id),
				name: (*name).into(),
				transfer_account: None,
				favorite: false,
				learn_categories: true,
				tombstone: false,
			},
		);
	}
	for row in dump["tables"]["rules"]["rows"]
		.as_array()
		.into_iter()
		.flatten()
	{
		let rule = rule_from_row(row);
		budget.rules.insert(rule.id.clone(), rule);
	}
	budget
}

fn rule_from_row(row: &Value) -> Rule {
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
fn rule_transaction(input: &Value, account: &str) -> RuleTransaction {
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
fn expected_row(outcome: &RuleTransaction, account: &str) -> Value {
	json!({
		"acct": account,
		"amount": outcome.amount.minor(),
		"category": outcome.category.as_ref().map(CategoryId::as_str),
		"cleared": i64::from(outcome.cleared.unwrap_or(true)),
		"date": outcome.date.to_string().replace('-', "").parse::<i64>().unwrap(),
		"description": outcome.payee.as_ref().map(PayeeId::as_str),
		"imported_description": outcome.imported_payee.as_deref(),
		"notes": outcome.notes.as_deref(),
		"tombstone": i64::from(outcome.deleted),
	})
}

#[test]
fn rule_outcomes_match_the_oracle() {
	let Some(oracle) = oracle_dir() else {
		eprintln!(
			"skipping the rules oracle test: tools/actual-oracle/node_modules is missing, run `cargo xtask oracle setup`"
		);
		return;
	};
	let world = world();
	let mut rng = Rng(0x0123_4567_89ab_cdef);
	let mut ops = setup_ops(&world);
	for n in 0..RULES {
		let conditions: Vec<Value> = (0..1 + rng.below(3))
			.map(|_| random_condition(&mut rng, &world))
			.collect();
		let actions: Vec<Value> = (0..1 + rng.below(2))
			.map(|_| random_action(&mut rng, &world))
			.collect();
		let stage = rng
			.pick(&[json!("pre"), Value::Null, Value::Null, json!("post")])
			.clone();
		let conditions_op = if rng.chance(25) { "or" } else { "and" };
		ops.push(
			json!({"op": "createRule", "id": uuid(8, n), "stage": stage, "conditionsOp": conditions_op,
			"conditions": conditions, "actions": actions}),
		);
	}
	let mut added = Vec::new();
	for (a, (account, _)) in world.accounts.iter().enumerate() {
		let transactions: Vec<Value> = (0..TRANSACTIONS / 2)
			.map(|i| random_transaction(&mut rng, &world, a * TRANSACTIONS + i))
			.collect();
		added.extend(transactions.iter().map(|t| (t.clone(), account.clone())));
		ops.push(json!({"op": "addTransactions", "account": account, "transactions": transactions, "runTransfers": false}));
	}

	let scratch = scratch_dir("rules");
	let dump = apply_and_dump(
		&oracle,
		&scratch,
		"rules",
		&repo_root().join("fixtures/synthetic/small.zip"),
		ops,
		TODAY,
	);
	let _ = std::fs::remove_dir_all(&scratch);

	let budget = budget_from(&world, &dump);
	for rule in budget.rules.values() {
		assert_eq!(
			ParsedRule::parse(rule).support(),
			RuleSupport::Full,
			"rule {} as Actual stored it",
			rule.id
		);
	}
	let book = RuleBook::new(&budget);
	let rows: BTreeMap<&str, &Value> = dump["tables"]["transactions"]["rows"]
		.as_array()
		.into_iter()
		.flatten()
		.filter_map(|row| Some((row["id"].as_str()?, row)))
		.collect();

	let mut mismatches = Vec::new();
	let mut rules_fired = 0;
	for (input, account) in &added {
		let outcome = book.run(&rule_transaction(input, account), &budget);
		assert!(outcome.undetermined.is_empty());
		rules_fired += outcome.applied.len();
		let expected = expected_row(&outcome.transaction, account);
		let row = rows
			.get(input["id"].as_str().unwrap())
			.unwrap_or_else(|| panic!("{} wasn't added", input["id"]));
		let actual: serde_json::Map<String, Value> = expected
			.as_object()
			.unwrap()
			.keys()
			.map(|key| (key.clone(), row[key.as_str()].clone()))
			.collect();
		if Value::Object(actual.clone()) != expected {
			mismatches.push(format!(
				"{input}\n  actual:  {}\n  veritas: {expected}\n  rules applied: {:?}",
				Value::Object(actual),
				outcome.applied
			));
		}
	}
	eprintln!(
		"compared {} transactions against the oracle ({rules_fired} rule applications), {} differ",
		added.len(),
		mismatches.len()
	);
	assert!(
		rules_fired > added.len() / 2,
		"too few rules fired to prove much: {rules_fired}"
	);
	assert!(
		mismatches.is_empty(),
		"{} of {} transactions differ from Actual:\n{}",
		mismatches.len(),
		added.len(),
		mismatches
			.iter()
			.take(10)
			.cloned()
			.collect::<Vec<_>>()
			.join("\n")
	);
}

/// R-RULE-3: the all-features fixture's templated, formula and append rules
/// are read-only in Veritas, everything else is fully supported, and parsing
/// leaves the stored text untouched.
#[test]
fn unsupported_rules_in_the_fixture_are_read_only_and_untouched() {
	let Some(oracle) = oracle_dir() else {
		eprintln!(
			"skipping the rules fixture test: tools/actual-oracle/node_modules is missing, run `cargo xtask oracle setup`"
		);
		return;
	};
	let zip = repo_root().join("fixtures/synthetic/all-features.zip");
	let dump = dump(&oracle, &zip.to_string_lossy(), TODAY);
	let mut read_only = Vec::new();
	for row in dump["tables"]["rules"]["rows"]
		.as_array()
		.into_iter()
		.flatten()
	{
		let rule = rule_from_row(row);
		let before = rule.clone();
		let parsed = ParsedRule::parse(&rule);
		assert_eq!(rule, before);
		let raw: Vec<Value> = serde_json::from_str(&rule.actions_json).unwrap();
		assert_eq!(
			parsed
				.actions
				.iter()
				.map(|item| item.raw.clone())
				.collect::<Vec<_>>(),
			raw
		);
		match parsed.support() {
			RuleSupport::Full => {}
			RuleSupport::ReadOnly(reasons) => read_only.push((rule.actions_json.clone(), reasons)),
			RuleSupport::IgnoredByActual(reason) => panic!(
				"Actual loads rule {}, but Veritas says it doesn't: {reason}",
				rule.id
			),
		}
	}
	assert_eq!(read_only.len(), 2, "{read_only:#?}");
	assert!(
		read_only
			.iter()
			.any(|(actions, _)| actions.contains("template"))
	);
	assert!(
		read_only
			.iter()
			.any(|(actions, _)| actions.contains("formula"))
	);
}
