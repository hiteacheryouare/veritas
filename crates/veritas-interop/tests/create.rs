//! R-IO-0: Veritas makes a new budget on its own, from a template Actual made,
//! and Actual opens the result as one of its own.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::io::Read;
use std::path::Path;

use common::{Scratch, oracle, write_zip};
use rusqlite::Connection;
use serde_json::{Value, json};
use veritas_domain::{
	BudgetCell, BudgetData, CategoryKind, CivilDate, CoreError, Money, SplitRole, Transaction,
	TransactionId, YearMonth,
};
use veritas_interop::{
	Access, ExportOutcome, Manifest, NewBudget, canonical_dump_to_vec, create_budget,
	export_actual_zip,
};
use veritas_store::{NewAccount, Store, TEMPLATE_DB, veritas_starter_set};

fn today() -> CivilDate {
	CivilDate::new(2026, 11, 9).unwrap()
}

fn accounts() -> Vec<NewAccount> {
	vec![
		NewAccount {
			name: "Checking".to_owned(),
			off_budget: false,
			starting_balance: Money::from_minor(150_000),
		},
		NewAccount {
			name: "Savings".to_owned(),
			off_budget: false,
			starting_balance: Money::from_minor(50_000),
		},
		NewAccount {
			name: "Brokerage".to_owned(),
			off_budget: true,
			starting_balance: Money::from_minor(1_000_000),
		},
		NewAccount {
			name: "Wallet".to_owned(),
			off_budget: false,
			starting_balance: Money::ZERO,
		},
	]
}

fn new_budget(scratch: &Scratch) -> veritas_interop::ImportedBudget {
	let starter = veritas_starter_set();
	let accounts = accounts();
	create_budget(
		&scratch.join("Household-a1b2c3d"),
		&NewBudget {
			name: "Household",
			currency_code: Some("USD"),
			number_format: Some("comma-dot"),
			categories: &starter,
			accounts: &accounts,
			today: today(),
		},
	)
	.unwrap()
}

#[test]
fn the_template_is_a_current_actual_budget_r_io_0() {
	let scratch = Scratch::new("template-manifest");
	let db = scratch.join("db.sqlite");
	std::fs::write(&db, TEMPLATE_DB).unwrap();
	let conn = veritas_store::open_read_only(&db).unwrap();
	let ids = veritas_store::migration_ids(&conn).unwrap();
	assert!(matches!(
		Manifest::supported().unwrap().check(&ids),
		Ok(Access::ReadWrite)
	));
	let (loaded, audit) = veritas_store::audit_connection(&conn).unwrap();
	assert!(audit.is_clean(), "{audit:?}");
	let data = loaded.unwrap().data;
	assert_eq!(data.categories.len(), 7, "Actual's seed categories");
	assert_eq!(data.category_groups.len(), 3);
	assert!(data.transactions.is_empty() && data.accounts.is_empty());
}

#[test]
fn the_committed_template_is_what_the_oracle_makes_today() {
	let scratch = Scratch::new("template-regenerate");
	let zip = scratch.join("template.zip");
	if oracle(&[
		"generate".as_ref(),
		"template".as_ref(),
		"--out".as_ref(),
		zip.as_os_str(),
	])
	.is_none()
	{
		return;
	}
	let mut archive = zip::ZipArchive::new(std::fs::File::open(&zip).unwrap()).unwrap();
	let mut db = Vec::new();
	archive
		.by_name("db.sqlite")
		.unwrap()
		.read_to_end(&mut db)
		.unwrap();
	assert!(
		db == TEMPLATE_DB,
		"run `cargo xtask template` and commit the result"
	);
}

#[test]
fn a_new_budget_has_the_starter_set_accounts_and_starting_balances_r_io_0() {
	let scratch = Scratch::new("create");
	let budget = new_budget(&scratch);
	assert_eq!(budget.access, Access::ReadWrite);
	assert!(budget.audit.is_clean());
	let names: Vec<String> = std::fs::read_dir(scratch.path())
		.unwrap()
		.map(|entry| entry.unwrap().file_name().into_string().unwrap())
		.collect();
	assert_eq!(
		names,
		["Household-a1b2c3d"],
		"no building folder is left behind"
	);
	let metadata: Value =
		serde_json::from_slice(&std::fs::read(&budget.metadata_path).unwrap()).unwrap();
	assert_eq!(
		metadata,
		json!({"id": "Household-a1b2c3d", "budgetName": "Household"})
	);

	let store = Store::open(&budget.db_path, Access::ReadWrite).unwrap();
	let data = store.load().unwrap().data;
	let live: Vec<_> = data.categories.values().filter(|c| !c.tombstone).collect();
	assert_eq!(live.len(), 35);
	assert_eq!(
		data.category_groups
			.values()
			.filter(|g| !g.tombstone)
			.count(),
		9
	);
	for category in &live {
		assert_eq!(
			data.resolve_category(&category.id),
			Some(&category.id),
			"{}",
			category.name
		);
		let meta = &data.category_meta[&category.id];
		assert_eq!(meta.kind == Some(CategoryKind::Income), category.is_income);
	}
	let mut orders: Vec<f64> = live.iter().map(|c| c.sort_order).collect();
	orders.sort_by(f64::total_cmp);
	orders.dedup();
	assert_eq!(orders.len(), 35, "SC-3: no sort_order ties");
	let starting = live.iter().find(|c| c.name == "Starting Balances").unwrap();
	assert_eq!(starting.id.as_str(), "506e8d9d-7ed0-4397-84e4-07a9185dc6b2");

	assert_eq!(data.accounts.len(), 4);
	let balances: Vec<&Transaction> = data
		.transactions
		.values()
		.filter(|t| t.starting_balance)
		.collect();
	assert_eq!(balances.len(), 3, "a zero balance writes no transaction");
	for row in &balances {
		let account = &data.accounts[&row.account];
		assert_eq!(row.date, today());
		assert!(row.cleared);
		let expected = if account.off_budget {
			None
		} else {
			Some(starting.id.clone())
		};
		assert_eq!(row.category, expected, "{}", account.name);
	}
	let starting_payees = data
		.payees
		.values()
		.filter(|p| p.name == "Starting Balance")
		.count();
	assert_eq!(starting_payees, 1, "one Starting Balance payee, reused");
	assert_eq!(
		data.payees
			.values()
			.filter(|p| p.transfer_account.is_some())
			.count(),
		4
	);

	let conn = Connection::open(&budget.db_path).unwrap();
	let currency: String = conn
		.query_row(
			"SELECT value FROM preferences WHERE id = 'defaultCurrencyCode'",
			[],
			|row| row.get(0),
		)
		.unwrap();
	assert_eq!(currency, "USD");
}

#[test]
fn a_failed_create_leaves_nothing_behind() {
	let scratch = Scratch::new("create-failed");
	let mut starter = veritas_starter_set();
	starter[0].categories[0].tax_bucket = Some("capital_gains".to_owned());
	let target = scratch.join("Broken");
	let result = create_budget(
		&target,
		&NewBudget {
			name: "Broken",
			currency_code: None,
			number_format: None,
			categories: &starter,
			accounts: &[],
			today: today(),
		},
	);
	assert!(matches!(result, Err(CoreError::Invalid(_))), "{result:?}");
	assert_eq!(std::fs::read_dir(scratch.path()).unwrap().count(), 0);

	std::fs::create_dir(&target).unwrap();
	let starter = veritas_starter_set();
	let result = create_budget(
		&target,
		&NewBudget {
			name: "Twice",
			currency_code: None,
			number_format: None,
			categories: &starter,
			accounts: &[],
			today: today(),
		},
	);
	assert!(
		matches!(result, Err(CoreError::Io(_))),
		"an existing folder is never overwritten"
	);
}

fn computed(dump: &[u8]) -> Value {
	let mut parsed: Value = serde_json::from_slice(dump).unwrap();
	parsed["computed"].take()
}

fn starting_rows(dump: &[u8]) -> Vec<Value> {
	let parsed: Value = serde_json::from_slice(dump).unwrap();
	let mut rows: Vec<Value> = parsed["tables"]["transactions"]["rows"]
		.as_array()
		.unwrap()
		.iter()
		.filter(|row| row["starting_balance_flag"] == json!(1))
		.map(|row| {
			let mut row = row.clone();
			for column in ["id", "acct", "description", "sort_order"] {
				row.as_object_mut().unwrap().remove(column);
			}
			row
		})
		.collect();
	rows.sort_by_key(|row| row["amount"].as_i64());
	rows
}

fn pay(data: &BudgetData) -> Transaction {
	let checking = data
		.accounts
		.values()
		.find(|a| a.name == "Checking")
		.unwrap()
		.id
		.clone();
	let groceries = data
		.categories
		.values()
		.find(|c| c.name == "Groceries")
		.unwrap()
		.id
		.clone();
	Transaction {
		id: TransactionId::new("first-groceries"),
		account: checking,
		date: today(),
		amount: Money::from_minor(-4_512),
		payee: None,
		imported_payee: None,
		category: Some(groceries),
		notes: None,
		cleared: false,
		reconciled: false,
		split: SplitRole::None,
		transfer: None,
		schedule: None,
		starting_balance: false,
		sort_order: 1.0,
		tombstone: false,
	}
}

#[test]
fn actual_opens_a_new_budget_with_the_same_balances_and_budget_r_io_0() {
	let scratch = Scratch::new("create-actual");
	let budget = new_budget(&scratch);
	let mut store = Store::open(&budget.db_path, Access::ReadWrite).unwrap();
	let data = store.load().unwrap().data;
	let groceries = pay(&data);
	let november = YearMonth::parse("2026-11").unwrap();
	let mut writer = store.writer().unwrap();
	writer.put_transaction(None, Some(&groceries)).unwrap();
	writer
		.put_budget_cell(
			None,
			Some(&BudgetCell {
				month: november,
				category: groceries.category.clone().unwrap(),
				budgeted: Money::from_minor(10_000),
				carryover: false,
				goal: None,
				long_goal: false,
			}),
		)
		.unwrap();
	writer.commit().unwrap();
	let exported = scratch.join("household.zip");
	assert!(matches!(
		export_actual_zip(&store, &budget.metadata_path, &exported).unwrap(),
		ExportOutcome::Exported(_)
	));

	let Some(dump) = oracle(&["dump".as_ref(), exported.as_os_str()]) else {
		return;
	};
	let ours = computed(&dump);
	let data = store.load().unwrap().data;
	let balance = |name: &str| {
		let id = data
			.accounts
			.values()
			.find(|a| a.name == name)
			.unwrap()
			.id
			.as_str()
			.to_owned();
		ours["accounts"][id]["balance"].clone()
	};
	assert_eq!(balance("Checking"), json!(145_488));
	assert_eq!(balance("Savings"), json!(50_000));
	assert_eq!(balance("Brokerage"), json!(1_000_000));
	assert_eq!(balance("Wallet"), json!(0));
	let month = &ours["months"]["2026-11"];
	let cell = &month["categories"][groceries.category.unwrap().as_str()];
	assert_eq!(cell["budgeted"], json!(10_000));
	assert_eq!(cell["spent"], json!(-4_512));
	assert_eq!(cell["balance"], json!(5_488));
	// On-budget starting balances are income in budget math (docs/finance CLS-4 note).
	assert_eq!(month["totalIncome"], json!(200_000));
	assert_eq!(month["toBudget"], json!(190_000));

	// Actual's own createAccount, starting from the same template, writes the
	// same starting-balance rows, apart from the ids it generates.
	let template = scratch.join("template.zip");
	write_zip(
		&template,
		&[
			("db.sqlite", TEMPLATE_DB),
			("metadata.json", br#"{"budgetName":"Household"}"#),
		],
	);
	let ops = scratch.join("accounts.json");
	let account_ops: Vec<Value> = accounts()
		.iter()
		.map(|a| json!({"op": "createAccount", "id": a.name, "name": a.name, "offBudget": a.off_budget, "initialBalance": a.starting_balance.minor()}))
		.collect();
	std::fs::write(
		&ops,
		serde_json::to_vec(&json!({"version": 1, "ops": account_ops})).unwrap(),
	)
	.unwrap();
	let actual_made = scratch.join("actual.zip");
	oracle(&[
		"apply".as_ref(),
		template.as_os_str(),
		ops.as_os_str(),
		"--out".as_ref(),
		actual_made.as_os_str(),
	])
	.unwrap();
	let veritas_only = scratch.join("veritas-accounts.zip");
	export_only_accounts(&scratch, &veritas_only);
	assert_eq!(
		starting_rows(&canonical_dump_to_vec(&veritas_only).unwrap()),
		starting_rows(&canonical_dump_to_vec(&actual_made).unwrap())
	);
}

fn export_only_accounts(scratch: &Scratch, to: &Path) {
	let accounts = accounts();
	let budget = create_budget(
		&scratch.join("Accounts-only"),
		&NewBudget {
			name: "Household",
			currency_code: None,
			number_format: None,
			categories: &[],
			accounts: &accounts,
			today: today(),
		},
	)
	.unwrap();
	let store = Store::open(&budget.db_path, Access::ReadWrite).unwrap();
	assert!(matches!(
		export_actual_zip(&store, &budget.metadata_path, to).unwrap(),
		ExportOutcome::Exported(_)
	));
}
