//! R-RULE-1 and R-RULE-3 on a budget shaped like a real household's: bank
//! import cleanup in the pre stage, category rules keyed on the cleaned-up
//! payee, estimate-bill amounts, tags, and post-stage rules that read the
//! category an earlier rule chose. Actual runs them through `addTransactions`;
//! Veritas runs the same transactions over the rule rows Actual stored.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;

use common::{
	apply_and_dump, expected_row, oracle_dir, repo_root, rule_from_row, rule_transaction,
	scratch_dir, uuid,
};
use serde_json::{Value, json};
use veritas_domain::{
	Account, AccountId, BudgetData, BudgetType, Category, CategoryGroupId, CategoryId, Payee,
	PayeeId, RuleId,
};
use veritas_rules::{BudgetPayees, ParsedRule, RuleBook, RuleSupport};

const TODAY: &str = "2026-12-15";

const ACCOUNTS: [(&str, bool); 3] = [
	("Joint checking", false),
	("Visa", false),
	("Brokerage", true),
];
const GROUPS: [&str; 3] = ["Monthly bills", "Day to day", "Earnings"];
const CATEGORIES: [(&str, &str); 10] = [
	("Rent", "Monthly bills"),
	("Utilities", "Monthly bills"),
	("Phone", "Monthly bills"),
	("Subscriptions", "Monthly bills"),
	("Groceries", "Day to day"),
	("Dining", "Day to day"),
	("Shopping", "Day to day"),
	("Gas", "Day to day"),
	("Work expenses", "Day to day"),
	("Paycheck", "Earnings"),
];
const PAYEES: [&str; 12] = [
	"Amazon",
	"Oak Street Properties",
	"Kroger",
	"Trader Joe's",
	"Starbucks",
	"Comcast",
	"Verizon",
	"Acme Inc",
	"Netflix",
	"Café Olé",
	"Costco",
	"Venmo",
];

fn position<T>(items: &[T], name: &str, key: impl Fn(&T) -> &str) -> usize {
	items
		.iter()
		.position(|item| key(item) == name)
		.unwrap_or_else(|| panic!("no {name:?} in the household"))
}

fn account(name: &str) -> String {
	uuid(1, position(&ACCOUNTS, name, |a| a.0))
}

fn group(name: &str) -> String {
	uuid(2, position(&GROUPS, name, |g| g))
}

fn category(name: &str) -> String {
	uuid(3, position(&CATEGORIES, name, |c| c.0))
}

fn payee(name: &str) -> String {
	uuid(4, position(&PAYEES, name, |p| p))
}

fn setup_ops() -> Vec<Value> {
	let mut ops = Vec::new();
	for (name, off_budget) in ACCOUNTS {
		ops.push(
			json!({"op": "createAccount", "id": account(name), "name": name, "offBudget": off_budget}),
		);
	}
	for name in GROUPS {
		ops.push(json!({"op": "createCategoryGroup", "id": group(name), "name": name}));
	}
	for (name, in_group) in CATEGORIES {
		ops.push(
			json!({"op": "createCategory", "id": category(name), "name": name,
			"group": group(in_group), "isIncome": in_group == "Earnings"}),
		);
	}
	for name in PAYEES {
		ops.push(json!({"op": "createPayee", "id": payee(name), "name": name}));
	}
	ops
}

fn budget_from(dump: &Value) -> BudgetData {
	let mut budget = BudgetData::empty(BudgetType::Envelope);
	for (name, off_budget) in ACCOUNTS {
		let id = AccountId::new(account(name));
		budget.accounts.insert(
			id.clone(),
			Account {
				id,
				name: name.into(),
				off_budget,
				closed: false,
				group: None,
				sort_order: 0.0,
				tombstone: false,
			},
		);
	}
	for (name, in_group) in CATEGORIES {
		let id = CategoryId::new(category(name));
		budget.categories.insert(
			id.clone(),
			Category {
				id,
				name: name.into(),
				group: CategoryGroupId::new(group(in_group)),
				is_income: in_group == "Earnings",
				hidden: false,
				sort_order: 0.0,
				tombstone: false,
			},
		);
	}
	for name in PAYEES {
		let id = PayeeId::new(payee(name));
		budget.payees.insert(
			id.clone(),
			Payee {
				id,
				name: name.into(),
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

fn condition(field: &str, op: &str, value: Value) -> Value {
	let kind = match field {
		"payee" | "category" | "category_group" | "account" => "id",
		"amount" => "number",
		"date" => "date",
		"cleared" => "boolean",
		_ => "string",
	};
	json!({"op": op, "field": field, "value": value, "type": kind})
}

fn set(field: &str, value: Value) -> Value {
	let kind = match field {
		"payee" | "category" | "account" => "id",
		"amount" => "number",
		"cleared" => "boolean",
		_ => "string",
	};
	json!({"op": "set", "field": field, "value": value, "type": kind})
}

struct HouseholdRule {
	name: &'static str,
	stage: Value,
	conditions_op: &'static str,
	conditions: Vec<Value>,
	actions: Vec<Value>,
}

fn rule(
	name: &'static str,
	stage: Option<&str>,
	conditions: Vec<Value>,
	actions: Vec<Value>,
) -> HouseholdRule {
	HouseholdRule {
		name,
		stage: stage.map_or(Value::Null, |stage| json!(stage)),
		conditions_op: "and",
		conditions,
		actions,
	}
}

fn any_of(mut rule: HouseholdRule) -> HouseholdRule {
	rule.conditions_op = "or";
	rule
}

/// The rules a person accumulates over a couple of years of importing from
/// their bank, in the order they'd have written them.
fn household_rules() -> Vec<HouseholdRule> {
	let rent_on_the_first = json!({"start": "2026-01-01", "frequency": "monthly", "interval": 1,
		"patterns": [], "skipWeekend": false, "weekendSolveMode": "after", "endMode": "never"});
	vec![
		any_of(rule(
			"amazon cleanup",
			Some("pre"),
			vec![
				condition("imported_payee", "contains", json!("AMZN")),
				condition("imported_payee", "contains", json!("amazon")),
			],
			vec![
				set("payee", json!(payee("Amazon"))),
				set("category", json!(category("Shopping"))),
			],
		)),
		rule(
			"kroger cleanup",
			Some("pre"),
			vec![condition("imported_payee", "contains", json!("KROGER"))],
			vec![set("payee", json!(payee("Kroger")))],
		),
		rule(
			"trader joe's cleanup",
			Some("pre"),
			vec![condition("imported_payee", "contains", json!("trader joe"))],
			vec![set("payee", json!(payee("Trader Joe's")))],
		),
		rule(
			"starbucks cleanup",
			Some("pre"),
			vec![condition("imported_payee", "contains", json!("starbucks"))],
			vec![set("payee", json!(payee("Starbucks")))],
		),
		rule(
			"café cleanup",
			Some("pre"),
			vec![condition("imported_payee", "is", json!("TST* CAFE OLE"))],
			vec![set("payee", json!(payee("Café Olé")))],
		),
		rule(
			"landlord cleanup",
			Some("pre"),
			vec![condition(
				"imported_payee",
				"contains",
				json!("landlord llc"),
			)],
			vec![set("payee", json!(payee("Oak Street Properties")))],
		),
		rule(
			"paycheck",
			Some("pre"),
			vec![condition(
				"imported_payee",
				"is",
				json!("ACME CORP PAYROLL"),
			)],
			vec![
				set("payee", json!(payee("Acme Inc"))),
				set("category", json!(category("Paycheck"))),
			],
		),
		rule(
			"netflix",
			Some("pre"),
			vec![condition(
				"imported_payee",
				"oneOf",
				json!(["NETFLIX.COM", "Netflix 866-579-7172"]),
			)],
			vec![
				set("payee", json!(payee("Netflix"))),
				set("category", json!(category("Subscriptions"))),
			],
		),
		rule(
			"drop card authorization holds",
			Some("pre"),
			vec![condition("imported_payee", "contains", json!("AUTH HOLD"))],
			vec![json!({"op": "delete-transaction", "value": null})],
		),
		rule(
			"amazon is shopping",
			None,
			vec![condition("payee", "is", json!(payee("Amazon")))],
			vec![set("category", json!(category("Shopping")))],
		),
		rule(
			"prime membership",
			None,
			vec![
				condition("payee", "is", json!(payee("Amazon"))),
				condition("amount", "isapprox", json!(-1499)),
			],
			vec![set("category", json!(category("Subscriptions")))],
		),
		rule(
			"december amazon",
			None,
			vec![
				condition("payee", "is", json!(payee("Amazon"))),
				condition("date", "is", json!("2026-12")),
			],
			vec![json!({"op": "append-notes", "value": " #holiday"})],
		),
		rule(
			"groceries",
			None,
			vec![condition(
				"payee",
				"oneOf",
				json!([payee("Kroger"), payee("Trader Joe's")]),
			)],
			vec![set("category", json!(category("Groceries")))],
		),
		rule(
			"coffee",
			None,
			vec![condition("payee", "is", json!(payee("Starbucks")))],
			vec![set("category", json!(category("Dining")))],
		),
		rule(
			"work tag",
			None,
			vec![condition("notes", "hasTags", json!("#work"))],
			vec![set("category", json!(category("Work expenses")))],
		),
		rule(
			"comcast estimate",
			None,
			vec![
				condition("imported_payee", "contains", json!("comcast")),
				condition("amount", "isapprox", json!(-8999)),
			],
			vec![
				set("payee", json!(payee("Comcast"))),
				set("category", json!(category("Utilities"))),
			],
		),
		{
			let mut outflow = condition("amount", "isapprox", json!(6500));
			outflow["options"] = json!({"outflow": true});
			rule(
				"verizon estimate",
				None,
				vec![
					condition("imported_payee", "contains", json!("VZWRLSS")),
					outflow,
				],
				vec![
					set("payee", json!(payee("Verizon"))),
					set("category", json!(category("Phone"))),
				],
			)
		},
		any_of(rule(
			"gas stations",
			None,
			vec![
				condition("imported_payee", "contains", json!("shell")),
				condition("imported_payee", "contains", json!("exxon")),
			],
			vec![set("category", json!(category("Gas")))],
		)),
		rule(
			"rent around the first",
			None,
			vec![
				condition("payee", "is", json!(payee("Oak Street Properties"))),
				condition("date", "isapprox", rent_on_the_first),
			],
			vec![set("category", json!(category("Rent")))],
		),
		rule(
			"dividends",
			None,
			vec![
				json!({"op": "offBudget", "field": "account", "value": null, "type": "id"}),
				condition("imported_payee", "contains", json!("dividend")),
			],
			vec![set("notes", json!("Dividend reinvested"))],
		),
		rule(
			"payee contains amzn",
			None,
			vec![condition("payee", "contains", json!("AMZN"))],
			vec![set("notes", json!("matched a payee id by text"))],
		),
		rule(
			"tag the bills",
			Some("post"),
			vec![condition(
				"category_group",
				"is",
				json!(group("Monthly bills")),
			)],
			vec![json!({"op": "append-notes", "value": " #bills"})],
		),
		rule(
			"big purchases",
			Some("post"),
			vec![
				condition("category", "is", json!(category("Shopping"))),
				condition("amount", "lte", json!(-10000)),
			],
			vec![json!({"op": "prepend-notes", "value": "Big purchase: "})],
		),
		rule(
			"review uncategorized card charges",
			Some("post"),
			vec![
				condition("account", "is", json!(account("Visa"))),
				condition("category", "is", Value::Null),
			],
			vec![json!({"op": "append-notes", "value": " #review"})],
		),
	]
}

struct Purchase {
	label: &'static str,
	account: &'static str,
	date: &'static str,
	amount: i64,
	imported: Option<&'static str>,
	payee: Option<&'static str>,
	notes: Option<&'static str>,
}

const fn bank(
	label: &'static str,
	account: &'static str,
	date: &'static str,
	amount: i64,
	imported: &'static str,
) -> Purchase {
	Purchase {
		label,
		account,
		date,
		amount,
		imported: Some(imported),
		payee: None,
		notes: None,
	}
}

const fn noted(mut purchase: Purchase, notes: &'static str) -> Purchase {
	purchase.notes = Some(notes);
	purchase
}

const fn by_hand(
	label: &'static str,
	account: &'static str,
	date: &'static str,
	amount: i64,
	payee: &'static str,
) -> Purchase {
	Purchase {
		label,
		account,
		date,
		amount,
		imported: None,
		payee: Some(payee),
		notes: None,
	}
}

/// What two months of statements look like, including the awkward rows:
/// a bill that came in higher than estimated, a refund, a late rent payment,
/// an authorization hold, a gift shop that happens to contain "shell".
fn statements() -> Vec<Purchase> {
	vec![
		bank(
			"amazon order",
			"Visa",
			"2026-11-02",
			-4599,
			"AMZN Mktp US*2K4L19RT2",
		),
		bank(
			"prime renewal",
			"Visa",
			"2026-11-05",
			-1499,
			"Amazon Prime*RT5J21",
		),
		bank(
			"amazon near prime price",
			"Visa",
			"2026-11-12",
			-1600,
			"AMZN Mktp US*7Y1XQ03L",
		),
		bank(
			"holiday tv",
			"Visa",
			"2026-12-10",
			-18999,
			"AMAZON.COM*M12AB3 AMZN.COM/BILL",
		),
		bank(
			"holiday socks",
			"Visa",
			"2026-12-11",
			-1250,
			"AMZN Mktp US*ZZ09",
		),
		bank(
			"kroger",
			"Joint checking",
			"2026-11-03",
			-8734,
			"KROGER #432",
		),
		bank(
			"trader joe's",
			"Joint checking",
			"2026-11-09",
			-5612,
			"TRADER JOE'S #552 QPS",
		),
		noted(
			bank(
				"client coffee",
				"Visa",
				"2026-11-04",
				-575,
				"STARBUCKS STORE 04512",
			),
			"client mtg #work",
		),
		noted(
			bank("work lunch", "Visa", "2026-11-06", -2840, "TST* CAFE OLE"),
			"#work lunch",
		),
		noted(
			bank(
				"hashtag-free lunch",
				"Visa",
				"2026-11-07",
				-2100,
				"TST* CAFE OLE",
			),
			"#workshop snacks",
		),
		bank(
			"comcast as estimated",
			"Joint checking",
			"2026-11-15",
			-8999,
			"COMCAST CABLE COMM",
		),
		bank(
			"comcast rate hike",
			"Joint checking",
			"2026-12-15",
			-11250,
			"COMCAST CABLE COMM",
		),
		bank(
			"verizon",
			"Joint checking",
			"2026-11-20",
			-6712,
			"VZWRLSS*APOCC VISB",
		),
		bank(
			"verizon credit",
			"Joint checking",
			"2026-11-22",
			6500,
			"VZWRLSS*APOCC VISB",
		),
		bank(
			"paycheck",
			"Joint checking",
			"2026-11-14",
			250000,
			"Acme Corp Payroll",
		),
		bank(
			"paycheck with memo",
			"Joint checking",
			"2026-11-28",
			250000,
			"ACME CORP PAYROLL PPD",
		),
		bank("netflix", "Visa", "2026-11-18", -1549, "NETFLIX.COM"),
		bank("shell", "Visa", "2026-11-08", -4100, "SHELL OIL 57444"),
		bank("exxon", "Visa", "2026-11-16", -3800, "EXXONMOBIL 4421"),
		bank(
			"gift shop",
			"Visa",
			"2026-11-21",
			-2200,
			"SEASHELLS GIFT SHOP",
		),
		bank(
			"gas hold",
			"Visa",
			"2026-11-23",
			-100,
			"AUTH HOLD SHELL OIL",
		),
		bank(
			"rent on the first",
			"Joint checking",
			"2026-11-01",
			-150000,
			"LANDLORD LLC RENT",
		),
		bank(
			"rent two days early",
			"Joint checking",
			"2026-11-29",
			-150000,
			"LANDLORD LLC RENT",
		),
		bank(
			"rent late",
			"Joint checking",
			"2026-10-05",
			-150000,
			"LANDLORD LLC RENT",
		),
		bank(
			"takeout",
			"Visa",
			"2026-11-19",
			-3150,
			"DOORDASH*THAI HOUSE",
		),
		bank(
			"takeout refund",
			"Joint checking",
			"2026-11-24",
			3150,
			"DOORDASH*THAI HOUSE",
		),
		bank(
			"dividend",
			"Brokerage",
			"2026-12-01",
			1234,
			"VANGUARD DIVIDEND REINV",
		),
		bank("blank description", "Joint checking", "2026-11-30", 0, ""),
		by_hand("amazon by hand", "Visa", "2026-11-25", -2599, "Amazon"),
		by_hand(
			"venmo by hand",
			"Joint checking",
			"2026-11-26",
			-4000,
			"Venmo",
		),
	]
}

fn as_input(purchase: &Purchase, n: usize) -> Value {
	let mut input = json!({"id": uuid(9, n), "date": purchase.date, "amount": purchase.amount});
	if let Some(imported) = purchase.imported {
		input["imported_payee"] = json!(imported);
	}
	if let Some(name) = purchase.payee {
		input["payee"] = json!(payee(name));
	}
	if let Some(notes) = purchase.notes {
		input["notes"] = json!(notes);
	}
	input
}

fn name_of<'a>(
	id: &Value,
	names: impl IntoIterator<Item = &'a str>,
	to_id: fn(&str) -> String,
) -> Option<&'a str> {
	let id = id.as_str()?;
	names.into_iter().find(|name| to_id(name) == id)
}

/// Payee name, category name, notes and whether it was deleted.
type RegisterLine<'a> = (Option<&'a str>, Option<&'a str>, Option<&'a str>, bool);

/// What Actual wrote, in the terms a person reads off the register.
fn register_line(row: &Value) -> RegisterLine<'_> {
	(
		name_of(&row["description"], PAYEES, payee),
		name_of(&row["category"], CATEGORIES.iter().map(|c| c.0), category),
		row["notes"].as_str(),
		row["tombstone"] == 1,
	)
}

/// Creates the household and `rules` in Actual, adds `purchases` through
/// `addTransactions` account by account, and returns the canonical dump.
fn run_in_actual(
	oracle: &std::path::Path,
	label: &str,
	rules: &[HouseholdRule],
	purchases: &[Purchase],
) -> Value {
	let mut ops = setup_ops();
	for (n, rule) in rules.iter().enumerate() {
		ops.push(
			json!({"op": "createRule", "id": uuid(8, n), "stage": rule.stage,
			"conditionsOp": rule.conditions_op, "conditions": rule.conditions, "actions": rule.actions}),
		);
	}
	let mut by_account: BTreeMap<&str, Vec<Value>> = BTreeMap::new();
	for (n, purchase) in purchases.iter().enumerate() {
		by_account
			.entry(purchase.account)
			.or_default()
			.push(as_input(purchase, n));
	}
	for (name, transactions) in &by_account {
		ops.push(json!({"op": "addTransactions", "account": account(name),
			"transactions": transactions, "runTransfers": false}));
	}
	let scratch = scratch_dir(label);
	let dump = apply_and_dump(
		oracle,
		&scratch,
		label,
		&repo_root().join("fixtures/synthetic/small.zip"),
		ops,
		TODAY,
	);
	let _ = std::fs::remove_dir_all(&scratch);
	dump
}

/// The transaction rows Actual wrote, by id.
fn transaction_rows(dump: &Value) -> BTreeMap<&str, &Value> {
	dump["tables"]["transactions"]["rows"]
		.as_array()
		.into_iter()
		.flatten()
		.filter_map(|row| Some((row["id"].as_str()?, row)))
		.collect()
}

/// Veritas's result in the columns `addTransactions` wrote, next to Actual's.
fn compare(outcome_row: &Value, actual_row: &Value) -> Option<(Value, Value)> {
	let actual: serde_json::Map<String, Value> = outcome_row
		.as_object()
		.unwrap()
		.keys()
		.map(|key| (key.clone(), actual_row[key.as_str()].clone()))
		.collect();
	let actual = Value::Object(actual);
	(actual != *outcome_row).then(|| (actual, outcome_row.clone()))
}

#[test]
fn a_households_rules_categorize_two_months_of_statements_like_actual() {
	let Some(oracle) = oracle_dir() else {
		eprintln!(
			"skipping the household rules test: tools/actual-oracle/node_modules is missing, run `cargo xtask oracle setup`"
		);
		return;
	};
	let rules = household_rules();
	let purchases = statements();
	let dump = run_in_actual(&oracle, "household", &rules, &purchases);

	let budget = budget_from(&dump);
	assert_eq!(budget.rules.len(), rules.len());
	for stored in budget.rules.values() {
		assert_eq!(
			ParsedRule::parse(stored).support(),
			RuleSupport::Full,
			"{} as Actual stored it: {}",
			stored.id,
			stored.conditions_json
		);
	}
	let book = RuleBook::new(&budget);
	let rows = transaction_rows(&dump);
	let rule_name = |id: &RuleId| {
		(0..rules.len())
			.find(|n| uuid(8, *n) == id.as_str())
			.map_or("?", |n| rules[n].name)
	};

	let mut mismatches = Vec::new();
	let mut lines = BTreeMap::new();
	let mut fired = BTreeMap::<&str, usize>::new();
	for (n, purchase) in purchases.iter().enumerate() {
		let input = as_input(purchase, n);
		let account_id = account(purchase.account);
		let outcome = book.run(
			&rule_transaction(&input, &account_id),
			&budget,
			&mut BudgetPayees::new(&budget, || PayeeId::new("made")),
		);
		assert!(outcome.undetermined.is_empty(), "{}", purchase.label);
		for id in &outcome.applied {
			*fired.entry(rule_name(id)).or_default() += 1;
		}
		let expected = expected_row(&outcome.transaction, &account_id);
		let row = rows
			.get(uuid(9, n).as_str())
			.unwrap_or_else(|| panic!("{} wasn't added", purchase.label));
		if let Some((actual, veritas)) = compare(&expected, row) {
			mismatches.push(format!(
				"{}\n  actual:  {actual}\n  veritas: {veritas}\n  rules applied: {:?}",
				purchase.label,
				outcome.applied.iter().map(rule_name).collect::<Vec<_>>()
			));
		}
		lines.insert(purchase.label, register_line(row));
	}
	assert!(
		mismatches.is_empty(),
		"{} of {} statement lines differ from Actual:\n{}",
		mismatches.len(),
		purchases.len(),
		mismatches.join("\n")
	);

	let expected_register: [(&str, RegisterLine); 30] = [
		(
			"amazon order",
			(Some("Amazon"), Some("Shopping"), None, false),
		),
		(
			"prime renewal",
			(
				Some("Amazon"),
				Some("Subscriptions"),
				Some(" #bills"),
				false,
			),
		),
		// Actual's ±7.5% estimate catches an ordinary order priced near Prime.
		(
			"amazon near prime price",
			(
				Some("Amazon"),
				Some("Subscriptions"),
				Some(" #bills"),
				false,
			),
		),
		(
			"holiday tv",
			(
				Some("Amazon"),
				Some("Shopping"),
				Some("Big purchase:  #holiday"),
				false,
			),
		),
		(
			"holiday socks",
			(Some("Amazon"), Some("Shopping"), Some(" #holiday"), false),
		),
		("kroger", (Some("Kroger"), Some("Groceries"), None, false)),
		(
			"trader joe's",
			(Some("Trader Joe's"), Some("Groceries"), None, false),
		),
		// The tag rule scores lowest, so the payee's rule runs after it and wins.
		(
			"client coffee",
			(
				Some("Starbucks"),
				Some("Dining"),
				Some("client mtg #work"),
				false,
			),
		),
		(
			"work lunch",
			(
				Some("Café Olé"),
				Some("Work expenses"),
				Some("#work lunch"),
				false,
			),
		),
		(
			"hashtag-free lunch",
			(
				Some("Café Olé"),
				None,
				Some("#workshop snacks #review"),
				false,
			),
		),
		(
			"comcast as estimated",
			(Some("Comcast"), Some("Utilities"), Some(" #bills"), false),
		),
		("comcast rate hike", (None, None, None, false)),
		(
			"verizon",
			(Some("Verizon"), Some("Phone"), Some(" #bills"), false),
		),
		("verizon credit", (None, None, None, false)),
		(
			"paycheck",
			(Some("Acme Inc"), Some("Paycheck"), None, false),
		),
		("paycheck with memo", (None, None, None, false)),
		(
			"netflix",
			(
				Some("Netflix"),
				Some("Subscriptions"),
				Some(" #bills"),
				false,
			),
		),
		("shell", (None, Some("Gas"), None, false)),
		("exxon", (None, Some("Gas"), None, false)),
		("gift shop", (None, Some("Gas"), None, false)),
		("gas hold", (None, Some("Gas"), None, true)),
		(
			"rent on the first",
			(
				Some("Oak Street Properties"),
				Some("Rent"),
				Some(" #bills"),
				false,
			),
		),
		(
			"rent two days early",
			(
				Some("Oak Street Properties"),
				Some("Rent"),
				Some(" #bills"),
				false,
			),
		),
		(
			"rent late",
			(Some("Oak Street Properties"), None, None, false),
		),
		("takeout", (None, None, Some(" #review"), false)),
		("takeout refund", (None, None, None, false)),
		("dividend", (None, None, Some("Dividend reinvested"), false)),
		("blank description", (None, None, None, false)),
		(
			"amazon by hand",
			(Some("Amazon"), Some("Shopping"), None, false),
		),
		("venmo by hand", (Some("Venmo"), None, None, false)),
	];
	assert_eq!(expected_register.len(), purchases.len());
	for (label, expected) in expected_register {
		assert_eq!(lines[label], expected, "{label}");
	}

	// In memory a payee condition compares the payee's id, never its name.
	assert_eq!(fired.get("payee contains amzn"), None);
	let silent: Vec<&str> = rules
		.iter()
		.map(|rule| rule.name)
		.filter(|name| *name != "payee contains amzn" && !fired.contains_key(name))
		.collect();
	assert!(
		silent.is_empty(),
		"rules no statement line exercised: {silent:?}"
	);
}

/// Rules made in Actual with features Veritas doesn't run: a Handlebars
/// template, a formula, a split, a regular expression, and a rule that mixes
/// a plain category with a template.
fn rules_beyond_veritas() -> Vec<HouseholdRule> {
	let mut template = set("notes", json!(""));
	template["options"] = json!({"template": "Venmo to {{payee_name}} on {{date}}"});
	let mut formula = set("amount", json!(0));
	formula["options"] = json!({"formula": "=amount * 1.03"});
	let mut order_note = set("notes", json!(""));
	order_note["options"] = json!({"template": "Order {{date}}"});
	let split_category = |index: u32, name: &str| {
		let mut action = set("category", json!(category(name)));
		action["options"] = json!({"splitIndex": index});
		action
	};
	vec![
		rule(
			"costco cleanup",
			Some("pre"),
			vec![condition("imported_payee", "contains", json!("costco"))],
			vec![set("payee", json!(payee("Costco")))],
		),
		rule(
			"groceries",
			None,
			vec![condition("payee", "is", json!(payee("Kroger")))],
			vec![set("category", json!(category("Groceries")))],
		),
		rule(
			"venmo template",
			None,
			vec![condition("payee", "is", json!(payee("Venmo")))],
			vec![template],
		),
		rule(
			"rent increase formula",
			None,
			vec![
				condition("payee", "is", json!(payee("Oak Street Properties"))),
				condition("amount", "lt", json!(-100_000)),
			],
			vec![formula],
		),
		rule(
			"costco split",
			None,
			vec![condition("payee", "is", json!(payee("Costco")))],
			vec![
				json!({"op": "set-split-amount", "value": 50, "options": {"splitIndex": 1, "method": "fixed-percent"}}),
				split_category(1, "Groceries"),
				json!({"op": "set-split-amount", "value": null, "options": {"splitIndex": 2, "method": "remainder"}}),
				split_category(2, "Shopping"),
			],
		),
		rule(
			"square regex",
			None,
			vec![condition("imported_payee", "matches", json!("^sq \\*"))],
			vec![set("category", json!(category("Dining")))],
		),
		rule(
			"amazon with order note",
			None,
			vec![condition("payee", "is", json!(payee("Amazon")))],
			vec![set("category", json!(category("Shopping"))), order_note],
		),
	]
}

/// R-RULE-3: what Actual stored for rules Veritas can't run comes back out of
/// Veritas unchanged, those rules are read-only, and Veritas never applies
/// half of one. Where none of them is in play, the outcome matches Actual's.
#[test]
fn rules_veritas_cannot_run_are_read_only_untouched_and_never_half_applied() {
	let Some(oracle) = oracle_dir() else {
		eprintln!(
			"skipping the unsupported rules test: tools/actual-oracle/node_modules is missing, run `cargo xtask oracle setup`"
		);
		return;
	};
	let rules = rules_beyond_veritas();
	let purchases = vec![
		by_hand("venmo", "Joint checking", "2026-11-26", -4000, "Venmo"),
		by_hand(
			"rent",
			"Joint checking",
			"2026-12-01",
			-150_000,
			"Oak Street Properties",
		),
		bank(
			"costco run",
			"Joint checking",
			"2026-11-14",
			-21_480,
			"COSTCO WHSE #1234",
		),
		bank(
			"square coffee",
			"Visa",
			"2026-11-09",
			-650,
			"SQ *BLUE BOTTLE",
		),
		bank(
			"not square",
			"Visa",
			"2026-11-10",
			-1825,
			"SQUARE ONE DINER",
		),
		by_hand("amazon", "Visa", "2026-11-25", -2599, "Amazon"),
		by_hand("kroger", "Joint checking", "2026-11-03", -8734, "Kroger"),
	];
	let dump = run_in_actual(&oracle, "beyond", &rules, &purchases);
	let stored_rows: Vec<&Value> = dump["tables"]["rules"]["rows"]
		.as_array()
		.into_iter()
		.flatten()
		.collect();
	assert_eq!(stored_rows.len(), rules.len());
	let budget = budget_from(&dump);
	let rule_name = |id: &RuleId| {
		(0..rules.len())
			.find(|n| uuid(8, *n) == id.as_str())
			.map_or("?", |n| rules[n].name)
	};

	let read_only: Vec<&str> = budget
		.rules
		.values()
		.filter(|stored| {
			matches!(
				ParsedRule::parse(stored).support(),
				RuleSupport::ReadOnly(_)
			)
		})
		.map(|stored| rule_name(&stored.id))
		.collect();
	assert_eq!(
		read_only
			.iter()
			.copied()
			.collect::<std::collections::BTreeSet<_>>(),
		[
			"venmo template",
			"rent increase formula",
			"costco split",
			"amazon with order note"
		]
		.into_iter()
		.collect()
	);

	let book = RuleBook::new(&budget);
	let rows = transaction_rows(&dump);
	let mut undetermined = BTreeMap::new();
	for (n, purchase) in purchases.iter().enumerate() {
		let input = as_input(purchase, n);
		let account_id = account(purchase.account);
		let outcome = book.run(
			&rule_transaction(&input, &account_id),
			&budget,
			&mut BudgetPayees::new(&budget, || PayeeId::new("made")),
		);
		let names: Vec<&str> = outcome.undetermined.iter().map(rule_name).collect();
		for name in &names {
			assert!(read_only.contains(name), "{}: {name}", purchase.label);
		}
		for applied in outcome.applied.iter().map(rule_name) {
			assert!(
				!read_only.contains(&applied),
				"{}: Veritas applied the read-only rule {applied}",
				purchase.label
			);
		}
		// Where no read-only rule is in play, everything Veritas did must
		// agree with Actual, the Square regex included (D47).
		if names.is_empty() {
			let row = rows[uuid(9, n).as_str()];
			if let Some((actual, veritas)) =
				compare(&expected_row(&outcome.transaction, &account_id), row)
			{
				panic!(
					"{}\n  actual:  {actual}\n  veritas: {veritas}",
					purchase.label
				);
			}
		}
		undetermined.insert(purchase.label, (names, outcome.transaction));
	}

	for (label, rule) in [
		("venmo", "venmo template"),
		("rent", "rent increase formula"),
		("costco run", "costco split"),
		("amazon", "amazon with order note"),
	] {
		assert!(
			undetermined[label].0.contains(&rule),
			"{label}: Veritas didn't flag {rule}, got {:?}",
			undetermined[label].0
		);
	}
	assert_eq!(
		undetermined["square coffee"].1.category,
		Some(CategoryId::new(category("Dining"))),
		"the regex rule runs in Veritas now"
	);
	assert_eq!(undetermined["not square"].1.category, None);
	assert_eq!(
		undetermined["amazon"].1.category, None,
		"the plain half of a rule with a template must not be applied on its own"
	);
	assert_eq!(
		undetermined["costco run"]
			.1
			.payee
			.as_ref()
			.map(PayeeId::as_str),
		Some(payee("Costco").as_str()),
		"the supported cleanup before a split rule still runs"
	);

	let actual_row = |label: &str| {
		let n = purchases.iter().position(|p| p.label == label).unwrap();
		rows[uuid(9, n).as_str()]
	};
	assert_eq!(actual_row("venmo")["notes"], "Venmo to Venmo on 2026-11-26");
	assert_eq!(actual_row("rent")["amount"], -154_500);
	assert_eq!(actual_row("square coffee")["category"], category("Dining"));
	assert_eq!(actual_row("not square")["category"], Value::Null);
	assert_eq!(actual_row("kroger")["category"], category("Groceries"));
	assert_eq!(actual_row("amazon")["category"], category("Shopping"));
	assert_eq!(actual_row("amazon")["notes"], "Order 2026-11-25");
	let costco_id = uuid(9, 2);
	let mut split: Vec<(i64, Value)> = rows
		.values()
		.filter(|row| row["parent_id"] == costco_id.as_str() && row["tombstone"] == 0)
		.map(|row| (row["amount"].as_i64().unwrap(), row["category"].clone()))
		.collect();
	split.sort_by_key(|(amount, _)| *amount);
	assert_eq!(
		split,
		[
			(-10_740, json!(category("Groceries"))),
			(-10_740, json!(category("Shopping")))
		]
	);

	for (row, stored) in stored_rows.iter().zip(budget.rules.values()) {
		assert_eq!(row["id"], stored.id.as_str());
		assert_eq!(
			row["conditions"].as_str(),
			Some(stored.conditions_json.as_str()),
			"{}",
			rule_name(&stored.id)
		);
		assert_eq!(
			row["actions"].as_str(),
			Some(stored.actions_json.as_str()),
			"{}",
			rule_name(&stored.id)
		);
	}
	let after_running = budget_from(&dump);
	assert_eq!(budget.rules, after_running.rules);
}
