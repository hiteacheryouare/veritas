//! R-RULE-1 and R-RULE-3 against the oracle. Actual creates generated rules
//! and runs them on transactions added through `addTransactions`; Veritas
//! reads the rule rows Actual stored and runs the same transactions through
//! its own engine. Every resulting field must agree.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;

use common::{
	Rng, apply_and_dump, dump, expected_row, oracle_dir, repo_root, rule_from_row,
	rule_transaction, scratch_dir, uuid,
};
use serde_json::{Value, json};
use veritas_domain::{
	Account, AccountId, BudgetData, BudgetType, Category, CategoryGroupId, CategoryId, Payee,
	PayeeId, RuleId,
};
use veritas_rules::{BudgetPayees, ParsedRule, RuleBook, RuleSupport};

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
	match rng.below(10) {
		9 => {
			json!({"op": "set", "field": "payee_name", "value": rng.pick(&["Amazon", "amazon", "New Shop", "NEW SHOP", "", "Coffee Shop"]), "type": "string"})
		}
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
				last_reconciled: None,
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
	// Pre-stage rules that score 0 and sort first, so they run before any
	// generated rule can rewrite the imported payee they look at.
	for (n, (text, name)) in [
		("zelle to", "Zelle Friend"),
		("shell oil", ""),
		("starbucks", "amazon"),
		("amzn digital", "New Shop"),
	]
	.iter()
	.enumerate()
	{
		ops.push(json!({"op": "createRule", "id": uuid(0, n), "stage": "pre",
			"conditions": [{"op": "contains", "field": "imported_payee", "value": text}],
			"actions": [{"op": "set", "field": "payee_name", "value": name}]}));
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
	let actual_payees: BTreeMap<&str, &str> = dump["tables"]["payees"]["rows"]
		.as_array()
		.into_iter()
		.flatten()
		.filter_map(|row| Some((row["id"].as_str()?, row["name"].as_str()?)))
		.collect();
	let mut made = 0;
	let mut payees = BudgetPayees::new(&budget, || {
		made += 1;
		PayeeId::new(format!("made-{made}"))
	});
	let known = |id: &str| world.payees.iter().any(|(payee, _)| payee == id);

	let mut mismatches = Vec::new();
	let mut rules_fired = 0;
	let mut made_payees = 0;
	for (input, account) in &added {
		let outcome = book.run(&rule_transaction(input, account), &budget, &mut payees);
		assert!(outcome.undetermined.is_empty());
		rules_fired += outcome.applied.len();
		let mut expected = expected_row(&outcome.partial, account);
		let row = rows
			.get(input["id"].as_str().unwrap())
			.unwrap_or_else(|| panic!("{} wasn't added", input["id"]));
		let mut actual: serde_json::Map<String, Value> = expected
			.as_object()
			.unwrap()
			.keys()
			.map(|key| (key.clone(), row[key.as_str()].clone()))
			.collect();
		// Payees a rule made get fresh ids on each side, so compare their names.
		if let Some(id) = actual["description"].as_str().filter(|id| !known(id)) {
			let name = actual_payees[id];
			actual.insert("description".into(), json!(format!("made: {name}")));
			made_payees += 1;
		}
		if let Some(id) = expected["description"].as_str().filter(|id| !known(id)) {
			let name = &payees
				.created
				.iter()
				.find(|(made, _)| made.as_str() == id)
				.unwrap()
				.1;
			expected["description"] = json!(format!("made: {name}"));
		}
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
	let base = common::dump(
		&oracle,
		&repo_root()
			.join("fixtures/synthetic/small.zip")
			.to_string_lossy(),
		TODAY,
	);
	let before: Vec<&str> = base["tables"]["payees"]["rows"]
		.as_array()
		.into_iter()
		.flatten()
		.filter_map(|row| row["id"].as_str())
		.collect();
	let mut actual_made: Vec<&str> = actual_payees
		.iter()
		.filter(|(id, name)| !known(id) && !before.contains(id) && !name.is_empty())
		.map(|(_, name)| *name)
		.collect();
	let mut veritas_made: Vec<&str> = payees
		.created
		.iter()
		.map(|(_, name)| name.as_str())
		.collect();
	actual_made.sort_unstable();
	veritas_made.sort_unstable();
	assert!(
		!veritas_made.is_empty(),
		"no rule made a payee, so set payee_name went unproven"
	);
	assert_eq!(
		veritas_made, actual_made,
		"set payee_name made different payees"
	);
	eprintln!("both made payees {veritas_made:?}; {made_payees} transactions kept one");
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

const PATTERNS: [&str; 40] = [
	"^amzn",
	"amzn|amazon",
	"^(starbucks|peet'?s) #?\\d+",
	"\\d{3,}",
	"^\\D+$",
	"\\bshell\\b",
	"oil$",
	"^zelle (to|from) \\w+$",
	"[aeiou]{2}",
	"^[a-z ]+$",
	"\\.com$",
	"s\\.",
	"\\s{2,}",
	"^$",
	"école",
	"\\u00e9",
	"\\x41",
	"[^a-z0-9 ]",
	"(?:mk|dig)",
	"a{2}",
	"\\W",
	"^.{5}$",
	"#\\d",
	"llc\\b",
	"[A-Z]",
	"\\S+\\s\\S+",
	"x{",
	"a]",
	"(oil|gas)?$",
	"(?<n>\\d)",
	"^[\\w.]+$",
	"[-.]",
	"\\Bll",
	"^a.*b$",
	"mktp\\s+us",
	"[\\d-]",
	"b+?",
	"^.+@.+$",
	"(?=a)",
	"fo**",
];

const TEXTS: [&str; 24] = [
	"AMZN Mktp US",
	"Amazon.com",
	"Starbucks #123",
	"Peets 42",
	"Shell Oil",
	"SHELL",
	"Zelle to Bob",
	"zelle from Ann",
	"École du ski",
	"12345",
	"Coffee  Shop",
	"A1",
	"Gas",
	"LANDLORD LLC",
	"llcx",
	"x{",
	"a]b",
	"aa",
	"ab",
	"me@example.com",
	"1-2",
	"Mr. Smith",
	"amzn digital",
	"",
];

/// D47: `matches` conditions go through the `regex` crate, read the JS way.
/// Each rule appends its own marker, so the notes show exactly which
/// patterns Actual matched for each text.
#[test]
fn regex_conditions_match_the_oracle() {
	let Some(oracle) = oracle_dir() else {
		eprintln!(
			"skipping the regex oracle test: tools/actual-oracle/node_modules is missing, run `cargo xtask oracle setup`"
		);
		return;
	};
	let world = world();
	let mut ops = setup_ops(&world);
	let mut refused = Vec::new();
	for (n, pattern) in PATTERNS.iter().enumerate() {
		let condition =
			json!({"op": "matches", "field": "imported_payee", "value": pattern, "type": "string"});
		let probe = rule_from_row(
			&json!({"id": "probe", "conditions": json!([condition]).to_string(), "actions": "[]"}),
		);
		if ParsedRule::parse(&probe).support() != RuleSupport::Full {
			refused.push(*pattern);
			continue;
		}
		ops.push(
			json!({"op": "createRule", "id": uuid(8, n), "conditions": [condition],
			"actions": [{"op": "append-notes", "value": format!(" [{n}]")}]}),
		);
	}
	assert_eq!(
		refused,
		["(?=a)", "fo**"],
		"only lookaround and patterns regex can't read are refused"
	);
	let account = &world.accounts[0].0;
	let transactions: Vec<Value> = TEXTS
		.iter()
		.enumerate()
		.map(|(n, text)| json!({"id": uuid(9, n), "date": "2026-11-01", "amount": -100, "cleared": true, "imported_payee": text}))
		.collect();
	ops.push(json!({"op": "addTransactions", "account": account, "transactions": transactions, "runTransfers": false}));

	let scratch = scratch_dir("regex");
	let dump = apply_and_dump(
		&oracle,
		&scratch,
		"regex",
		&repo_root().join("fixtures/synthetic/small.zip"),
		ops,
		TODAY,
	);
	let _ = std::fs::remove_dir_all(&scratch);
	let budget = budget_from(&world, &dump);
	let book = RuleBook::new(&budget);
	let rows: BTreeMap<&str, &Value> = dump["tables"]["transactions"]["rows"]
		.as_array()
		.into_iter()
		.flatten()
		.filter_map(|row| Some((row["id"].as_str()?, row)))
		.collect();
	let mut mismatches = Vec::new();
	let mut matched = 0;
	for input in &transactions {
		let outcome = book.run(
			&rule_transaction(input, account),
			&budget,
			&mut BudgetPayees::new(&budget, || PayeeId::new("made")),
		);
		assert!(
			outcome.undetermined.is_empty(),
			"{input}: {:?}",
			outcome.undetermined
		);
		matched += outcome.applied.len();
		let actual = &rows[input["id"].as_str().unwrap()]["notes"];
		let veritas = json!(outcome.partial.notes);
		if *actual != veritas {
			mismatches.push(format!(
				"{:?}: actual {actual}, veritas {veritas}",
				input["imported_payee"]
			));
		}
	}
	eprintln!(
		"{} patterns × {} texts, {matched} matches, {} differ",
		PATTERNS.len() - refused.len(),
		TEXTS.len(),
		mismatches.len()
	);
	assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

/// Two live payees can share a name: `createPayee` doesn't stop it. Actual's
/// `getPayeeByName` then takes whichever SQLite returns first, so Veritas
/// reports a `set payee_name` to that name as undetermined instead of
/// guessing, and still runs a rename to a name that is unique.
#[test]
fn a_payee_name_shared_by_two_payees_is_left_to_actual() {
	let Some(oracle) = oracle_dir() else {
		eprintln!(
			"skipping the duplicate payee oracle test: tools/actual-oracle/node_modules is missing, run `cargo xtask oracle setup`"
		);
		return;
	};
	let mut world = world();
	world.payees.push((uuid(4, 9), "AMAZON"));
	let mut ops = setup_ops(&world);
	let rename = |n: usize, text: &str, name: &str| {
		json!({"op": "createRule", "id": uuid(8, n), "stage": "pre",
			"conditions": [{"op": "contains", "field": "imported_payee", "value": text}],
			"actions": [{"op": "set", "field": "payee_name", "value": name}]})
	};
	ops.push(rename(0, "amzn", "amazon"));
	// The fixture has its own "Landlord", so the unique name is the coffee shop.
	ops.push(rename(1, "zelle", "coffee shop"));
	let account = world.accounts[0].0.clone();
	let transactions = vec![
		json!({"id": uuid(9, 0), "date": "2026-11-01", "amount": -100, "cleared": true, "imported_payee": "AMZN Mktp"}),
		json!({"id": uuid(9, 1), "date": "2026-11-01", "amount": -200, "cleared": true, "imported_payee": "Zelle to Bob"}),
	];
	ops.push(json!({"op": "addTransactions", "account": account, "transactions": transactions, "runTransfers": false}));
	let scratch = scratch_dir("duplicate-payees");
	let dump = apply_and_dump(
		&oracle,
		&scratch,
		"duplicates",
		&repo_root().join("fixtures/synthetic/small.zip"),
		ops,
		TODAY,
	);
	let _ = std::fs::remove_dir_all(&scratch);
	let budget = budget_from(&world, &dump);
	let book = RuleBook::new(&budget);
	let row = |n: usize| {
		dump["tables"]["transactions"]["rows"]
			.as_array()
			.unwrap()
			.iter()
			.find(|row| row["id"] == uuid(9, n))
			.unwrap()["description"]
			.clone()
	};
	let run = |n: usize| {
		book.run(
			&rule_transaction(&transactions[n], &account),
			&budget,
			&mut BudgetPayees::new(&budget, || PayeeId::new("made")),
		)
	};

	let amazon = run(0);
	assert_eq!(amazon.undetermined, [RuleId::new(uuid(8, 0))]);
	assert_eq!(
		amazon.partial.payee, None,
		"the ambiguous rename isn't half applied"
	);
	assert!(
		[json!(uuid(4, 0)), json!(uuid(4, 9))].contains(&row(0)),
		"Actual picked one of the two: {}",
		row(0)
	);

	let coffee = run(1);
	assert!(coffee.undetermined.is_empty());
	assert_eq!(coffee.partial.payee, Some(PayeeId::new(uuid(4, 2))));
	assert_eq!(row(1), json!(uuid(4, 2)));
}
