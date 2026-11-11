//! R-IO-0: Veritas makes a new budget on its own, from a template Actual made,
//! and Actual opens the result as one of its own.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::io::Read;

use common::{Scratch, oracle, write_zip};
use rusqlite::Connection;
use serde_json::{Value, json};
use veritas_domain::{
	BudgetCell, BudgetData, CategoryKind, CivilDate, CoreError, FormatPrefs, Money, NewAccount,
	NewAccountKind, NewBudget, NumberFormat, NumberStyle, SplitRole, Transaction, TransactionId,
	YearMonth,
};
use veritas_interop::{
	Access, ExportOutcome, Manifest, canonical_dump_to_vec, create_budget, export_actual_zip,
};
use veritas_store::{Store, TEMPLATE_DB, paycheck_accounts};

fn today() -> CivilDate {
	CivilDate::new(2026, 11, 9).unwrap()
}

fn account(name: &str, kind: NewAccountKind, cents: i64) -> NewAccount {
	NewAccount {
		name: name.to_owned(),
		kind,
		starting_balance: Money::from_minor(cents),
	}
}

fn accounts() -> Vec<NewAccount> {
	vec![
		account("Checking", NewAccountKind::Checking, 150_000),
		account("Savings", NewAccountKind::Savings, 50_000),
		account("Brokerage", NewAccountKind::Investment, 1_000_000),
		account("Wallet", NewAccountKind::Cash, 0),
		account("Visa", NewAccountKind::CreditCard, 25_000),
		account("Mortgage", NewAccountKind::Mortgage, 30_000_000),
	]
}

/// What setup's accounts plus the 401(k) and HSA boxes add up to.
fn all_accounts() -> Vec<NewAccount> {
	let mut all = accounts();
	all.extend(paycheck_accounts(true, true));
	all
}

fn format() -> NumberFormat {
	NumberFormat {
		currency: Some("USD".to_owned()),
		style: NumberStyle::CommaDot,
		hide_decimals: true,
	}
}

fn setup(name: &str) -> NewBudget {
	NewBudget {
		name: name.to_owned(),
		format: format(),
		retirement_401k: true,
		hsa: true,
		accounts: accounts(),
	}
}

fn new_budget(scratch: &Scratch) -> veritas_interop::ImportedBudget {
	let dir = scratch.join("Household-a1b2c3d");
	std::fs::create_dir(&dir).unwrap();
	create_budget(&dir, &setup("Household"), today()).unwrap()
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

	assert_eq!(data.accounts.len(), 8);
	for new in all_accounts() {
		let made = data.accounts.values().find(|a| a.name == new.name).unwrap();
		assert_eq!(made.off_budget, new.kind.off_budget(), "{}", new.name);
		assert_eq!(
			data.account_meta[&made.id].kind,
			Some(new.kind.account_kind()),
			"{}",
			new.name
		);
		let opening: i64 = data
			.transactions
			.values()
			.filter(|t| t.starting_balance && t.account == made.id)
			.map(|t| t.amount.minor())
			.sum();
		let expected = new.opening_balance().unwrap().minor();
		assert_eq!(
			opening, expected,
			"a liability's balance owed is stored negative: {}",
			new.name
		);
	}
	let balances: Vec<&Transaction> = data
		.transactions
		.values()
		.filter(|t| t.starting_balance)
		.collect();
	assert_eq!(balances.len(), 5, "a zero balance writes no transaction");
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
		8
	);
	assert_eq!(
		data.format,
		FormatPrefs {
			number_format: Some("comma-dot".to_owned()),
			hide_fraction: true,
			currency_code: Some("USD".to_owned()),
		}
	);
	assert!(
		data.accounts
			.values()
			.any(|a| a.name == "401(k)" && a.off_budget)
	);
	assert!(
		data.accounts
			.values()
			.any(|a| a.name == "HSA" && a.off_budget)
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
fn create_refuses_a_nameless_budget_or_a_folder_that_isnt_empty() {
	let scratch = Scratch::new("create-refused");
	let empty = scratch.join("Nameless");
	std::fs::create_dir(&empty).unwrap();
	let result = create_budget(&empty, &setup("  "), today());
	assert!(matches!(result, Err(CoreError::Invalid(_))), "{result:?}");

	let used = scratch.join("Used");
	std::fs::create_dir(&used).unwrap();
	std::fs::write(used.join("notes.txt"), b"mine").unwrap();
	let result = create_budget(&used, &setup("Used"), today());
	assert!(
		matches!(result, Err(CoreError::Io(_))),
		"a folder with files in it is never built into"
	);
	assert_eq!(std::fs::read(used.join("notes.txt")).unwrap(), b"mine");
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
	assert_eq!(balance("Visa"), json!(-25_000));
	assert_eq!(balance("Mortgage"), json!(-30_000_000));
	assert_eq!(balance("401(k)"), json!(0));
	let month = &ours["months"]["2026-11"];
	let cell = &month["categories"][groceries.category.unwrap().as_str()];
	assert_eq!(cell["budgeted"], json!(10_000));
	assert_eq!(cell["spent"], json!(-4_512));
	assert_eq!(cell["balance"], json!(5_488));
	// On-budget starting balances are income in budget math (docs/finance CLS-4 note).
	// The card's opening balance is on budget too, so it counts against income.
	assert_eq!(month["totalIncome"], json!(175_000));
	assert_eq!(month["toBudget"], json!(165_000));

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
		.map(|a| {
			json!({
				"op": "createAccount",
				"id": a.name,
				"name": a.name,
				"offBudget": a.kind.off_budget(),
				"initialBalance": a.opening_balance().unwrap().minor()
			})
		})
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
	assert_eq!(
		starting_rows(&canonical_dump_to_vec(&exported).unwrap()),
		starting_rows(&canonical_dump_to_vec(&actual_made).unwrap())
	);
}
