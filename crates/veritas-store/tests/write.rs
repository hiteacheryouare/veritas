//! The write path: partial updates, inserts with Actual's defaults,
//! tombstones and the side rows Actual keeps (PLAN §6.2).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::Path;

use common::{Scratch, fixture_db};
use rusqlite::Connection;
use rusqlite::types::Value;
use veritas_domain::{
	Account, AccountId, BudgetCell, BudgetData, Category, CategoryId, CivilDate, CoreError, Money,
	Payee, PayeeId, SplitRole, Transaction, TransactionId, YearMonth,
};
use veritas_store::{Access, Store};

fn row(conn: &Connection, table: &str, id: &str) -> Vec<(String, Value)> {
	let mut statement = conn
		.prepare(&format!("SELECT * FROM {table} WHERE id = ?1"))
		.unwrap();
	let names: Vec<String> = statement.column_names().map(str::to_owned).collect();
	statement
		.query_row([id], |row| {
			Ok(names
				.iter()
				.enumerate()
				.map(|(index, name)| (name.clone(), row.get::<_, Value>(index).unwrap()))
				.collect())
		})
		.unwrap()
}

fn changed_columns(before: &[(String, Value)], after: &[(String, Value)]) -> Vec<String> {
	before
		.iter()
		.zip(after)
		.filter(|(a, b)| a.1 != b.1)
		.map(|(a, _)| a.0.clone())
		.collect()
}

fn loaded(db: &Path) -> BudgetData {
	Store::open(db, Access::ReadWrite)
		.unwrap()
		.load()
		.unwrap()
		.data
}

fn first_plain_transaction(data: &BudgetData) -> Transaction {
	data.transactions
		.values()
		.find(|t| t.split == SplitRole::None && t.transfer.is_none() && !t.tombstone)
		.unwrap()
		.clone()
}

#[test]
fn an_edit_writes_only_the_changed_column_and_an_unknown_column_survives_edge_case_1() {
	let scratch = Scratch::new("write-partial");
	let db = fixture_db("small", &scratch);
	let data = loaded(&db);
	let before = first_plain_transaction(&data);
	let raw = Connection::open(&db).unwrap();
	raw.execute_batch("ALTER TABLE transactions ADD COLUMN future_flag INTEGER DEFAULT 7;")
		.unwrap();
	raw.execute(
		"UPDATE transactions SET future_flag = 42 WHERE id = ?1",
		[before.id.as_str()],
	)
	.unwrap();
	let snapshot = row(&raw, "transactions", before.id.as_str());

	let after = Transaction {
		amount: Money::from_minor(before.amount.minor() - 1250),
		..before.clone()
	};
	let mut store = Store::open(&db, Access::ReadWrite).unwrap();
	let mut writer = store.writer().unwrap();
	writer.put_transaction(Some(&before), Some(&after)).unwrap();
	writer.commit().unwrap();

	let edited = row(&raw, "transactions", before.id.as_str());
	assert_eq!(changed_columns(&snapshot, &edited), ["amount"]);
	assert!(edited.contains(&("future_flag".to_owned(), Value::Integer(42))));
	assert_eq!(loaded(&db).transactions[&before.id], after);
}

#[test]
fn writing_a_row_unchanged_touches_nothing() {
	let scratch = Scratch::new("write-noop");
	let db = fixture_db("all-features", &scratch);
	let data = loaded(&db);
	let before = std::fs::read(&db).unwrap();
	let mut store = Store::open(&db, Access::ReadWrite).unwrap();
	let mut writer = store.writer().unwrap();
	for transaction in data.transactions.values() {
		writer
			.put_transaction(Some(transaction), Some(transaction))
			.unwrap();
	}
	for category in data.categories.values() {
		writer.put_category(Some(category), Some(category)).unwrap();
	}
	for cell in data.budget_cells.values() {
		writer.put_budget_cell(Some(cell), Some(cell)).unwrap();
	}
	for schedule in data.schedules.values() {
		writer.put_schedule(Some(schedule), Some(schedule)).unwrap();
	}
	writer.commit().unwrap();
	drop(store);
	assert_eq!(loaded(&db), data);
	assert_eq!(std::fs::read(&db).unwrap(), before);
}

#[test]
fn a_delete_is_a_tombstone_and_actual_rows_are_never_removed() {
	let scratch = Scratch::new("write-tombstone");
	let db = fixture_db("small", &scratch);
	let before = first_plain_transaction(&loaded(&db));
	let mut store = Store::open(&db, Access::ReadWrite).unwrap();
	let mut writer = store.writer().unwrap();
	let deleted = Transaction {
		tombstone: true,
		..before.clone()
	};
	writer
		.put_transaction(Some(&before), Some(&deleted))
		.unwrap();
	assert!(matches!(
		writer.put_transaction(Some(&deleted), None),
		Err(CoreError::Invalid(_))
	));
	writer.commit().unwrap();
	let after = loaded(&db);
	assert!(after.transactions[&before.id].tombstone);
}

#[test]
fn a_new_transaction_gets_actuals_defaults_for_everything_veritas_leaves_out() {
	let scratch = Scratch::new("write-insert");
	let db = fixture_db("small", &scratch);
	let data = loaded(&db);
	let template = first_plain_transaction(&data);
	let new = Transaction {
		id: TransactionId::new("veritas-new-1"),
		date: CivilDate::new(2026, 11, 2).unwrap(),
		amount: Money::from_minor(-4512),
		notes: None,
		imported_payee: None,
		schedule: None,
		cleared: false,
		reconciled: false,
		sort_order: 1_794_225_600_000.0,
		..template
	};
	let mut store = Store::open(&db, Access::ReadWrite).unwrap();
	let mut writer = store.writer().unwrap();
	writer.put_transaction(None, Some(&new)).unwrap();
	writer.commit().unwrap();

	let raw = Connection::open(&db).unwrap();
	let stored = row(&raw, "transactions", "veritas-new-1");
	let value = |name: &str| {
		stored
			.iter()
			.find(|(column, _)| column == name)
			.unwrap()
			.1
			.clone()
	};
	assert_eq!(value("pending"), Value::Integer(0));
	assert_eq!(value("type"), Value::Null);
	assert_eq!(value("error"), Value::Null);
	assert_eq!(value("date"), Value::Integer(20_261_102));
	assert_eq!(value("cleared"), Value::Integer(0));
	assert_eq!(loaded(&db).transactions[&new.id], new);
}

#[test]
fn a_new_category_needs_its_mapping_row_in_the_same_change() {
	let scratch = Scratch::new("write-mapping");
	let db = fixture_db("small", &scratch);
	let data = loaded(&db);
	let group = data
		.categories
		.values()
		.find(|c| !c.is_income)
		.unwrap()
		.group
		.clone();
	let category = Category {
		id: CategoryId::new("veritas-cat"),
		name: "Pets".to_owned(),
		group,
		is_income: false,
		hidden: false,
		sort_order: 99_999.0,
		tombstone: false,
	};
	let mut store = Store::open(&db, Access::ReadWrite).unwrap();
	let mut writer = store.writer().unwrap();
	writer.put_category(None, Some(&category)).unwrap();
	assert!(matches!(writer.commit(), Err(CoreError::Invalid(_))));
	assert!(
		!loaded(&db).categories.contains_key(&category.id),
		"the refused change rolled back"
	);

	let mut writer = store.writer().unwrap();
	writer.put_category(None, Some(&category)).unwrap();
	writer
		.put_category_mapping(&category.id, None, Some(&category.id))
		.unwrap();
	writer.commit().unwrap();
	let after = loaded(&db);
	assert_eq!(after.categories[&category.id], category);
	assert_eq!(after.resolve_category(&category.id), Some(&category.id));
	let raw = Connection::open(&db).unwrap();
	let settings: String = raw
		.query_row(
			"SELECT template_settings FROM categories WHERE id = 'veritas-cat'",
			[],
			|row| row.get(0),
		)
		.unwrap();
	assert_eq!(settings, "{\"source\":\"notes\"}");
}

#[test]
fn a_new_account_needs_its_transfer_payee_and_mapping() {
	let scratch = Scratch::new("write-account");
	let db = fixture_db("small", &scratch);
	let account = Account {
		id: AccountId::new("veritas-acct"),
		name: "Credit union".to_owned(),
		off_budget: false,
		closed: false,
		group: None,
		sort_order: 5000.0,
		tombstone: false,
	};
	let payee = Payee {
		id: PayeeId::new("veritas-acct-payee"),
		name: String::new(),
		transfer_account: Some(account.id.clone()),
		favorite: false,
		learn_categories: true,
		tombstone: false,
	};
	let mut store = Store::open(&db, Access::ReadWrite).unwrap();
	let mut writer = store.writer().unwrap();
	writer.put_account(None, Some(&account)).unwrap();
	assert!(matches!(writer.commit(), Err(CoreError::Invalid(_))));

	let mut writer = store.writer().unwrap();
	writer.put_account(None, Some(&account)).unwrap();
	writer.put_payee(None, Some(&payee)).unwrap();
	assert!(
		matches!(writer.commit(), Err(CoreError::Invalid(_))),
		"the payee needs its mapping too"
	);

	let mut writer = store.writer().unwrap();
	writer.put_account(None, Some(&account)).unwrap();
	writer.put_payee(None, Some(&payee)).unwrap();
	writer
		.put_payee_mapping(&payee.id, None, Some(&payee.id))
		.unwrap();
	writer.commit().unwrap();

	let renamed = Account {
		name: "Credit union checking".to_owned(),
		..account.clone()
	};
	let mut writer = store.writer().unwrap();
	writer.put_account(Some(&account), Some(&renamed)).unwrap();
	writer.commit().unwrap();
	let after = loaded(&db);
	assert_eq!(after.accounts[&account.id], renamed);
	assert_eq!(
		after.payees[&payee.id], payee,
		"a rename never touches the transfer payee"
	);
	let (loaded, report) = Store::open(&db, Access::ReadWrite)
		.unwrap()
		.open_audit()
		.unwrap();
	assert!(loaded.is_some());
	assert!(report.is_clean(), "{report:?}");
}

#[test]
fn budget_cells_are_found_by_month_and_category_in_either_id_format() {
	let scratch = Scratch::new("write-budget");
	let db = fixture_db("small", &scratch);
	let data = loaded(&db);
	let category = data
		.categories
		.values()
		.find(|c| !c.is_income)
		.unwrap()
		.id
		.clone();
	let raw = Connection::open(&db).unwrap();
	raw.execute(
		"INSERT INTO zero_budgets (id, month, category, amount) VALUES (?1, 201903, ?2, 4200)",
		[format!("2019-03-{category}"), category.as_str().to_owned()],
	)
	.unwrap();
	let march = YearMonth::parse("2019-03").unwrap();
	let before = loaded(&db).budget_cells[&(march, category.clone())].clone();
	let after = BudgetCell {
		budgeted: Money::from_minor(5000),
		carryover: true,
		..before.clone()
	};
	let fresh = BudgetCell {
		month: YearMonth::parse("2019-04").unwrap(),
		category: category.clone(),
		budgeted: Money::from_minor(700),
		carryover: false,
		goal: None,
		long_goal: false,
	};
	let mut store = Store::open(&db, Access::ReadWrite).unwrap();
	let mut writer = store.writer().unwrap();
	writer.put_budget_cell(Some(&before), Some(&after)).unwrap();
	writer.put_budget_cell(None, Some(&fresh)).unwrap();
	writer.commit().unwrap();

	let ids: Vec<String> = {
		let mut statement = raw
			.prepare("SELECT id FROM zero_budgets WHERE month IN (201903, 201904) ORDER BY month")
			.unwrap();
		statement
			.query_map([], |row| row.get(0))
			.unwrap()
			.map(Result::unwrap)
			.collect()
	};
	assert_eq!(
		ids,
		[format!("2019-03-{category}"), format!("201904-{category}")]
	);
	let reloaded = loaded(&db);
	assert_eq!(reloaded.budget_cells[&(march, category.clone())], after);
	assert_eq!(reloaded.budget_cells[&(fresh.month, category)], fresh);
}

#[test]
fn a_cleared_note_is_written_as_null_and_can_be_set_again() {
	let scratch = Scratch::new("write-note");
	let db = fixture_db("small", &scratch);
	let mut store = Store::open(&db, Access::ReadWrite).unwrap();
	let mut writer = store.writer().unwrap();
	writer
		.put_note("budget-2026-10", None, Some(Some("Rent went up")))
		.unwrap();
	writer.commit().unwrap();
	let mut writer = store.writer().unwrap();
	writer
		.put_note("budget-2026-10", Some(Some("Rent went up")), Some(None))
		.unwrap();
	writer.commit().unwrap();
	let raw = Connection::open(&db).unwrap();
	let note: Option<String> = raw
		.query_row(
			"SELECT note FROM notes WHERE id = 'budget-2026-10'",
			[],
			|row| row.get(0),
		)
		.unwrap();
	assert_eq!(note, None);
	assert_eq!(loaded(&db).notes["budget-2026-10"], None);
	assert!(matches!(
		store
			.writer()
			.unwrap()
			.put_note("budget-2026-10", Some(None), None),
		Err(CoreError::Invalid(_))
	));
	let mut writer = store.writer().unwrap();
	writer
		.put_note("budget-2026-10", Some(None), Some(Some("Back to normal")))
		.unwrap();
	writer.commit().unwrap();
	assert_eq!(
		loaded(&db).notes["budget-2026-10"].as_deref(),
		Some("Back to normal")
	);
}

#[test]
fn a_dropped_writer_rolls_back_r_rel_1() {
	let scratch = Scratch::new("write-rollback");
	let db = fixture_db("small", &scratch);
	let data = loaded(&db);
	let before = first_plain_transaction(&data);
	let after = Transaction {
		amount: Money::from_minor(1),
		..before.clone()
	};
	let mut store = Store::open(&db, Access::ReadWrite).unwrap();
	{
		let mut writer = store.writer().unwrap();
		writer.put_transaction(Some(&before), Some(&after)).unwrap();
	}
	let mut writer = store.writer().unwrap();
	writer.put_transaction(Some(&before), Some(&after)).unwrap();
	writer.rollback().unwrap();
	assert_eq!(loaded(&db), data);
}

#[test]
fn a_schedule_date_change_lands_where_v_schedules_reads_it() {
	let scratch = Scratch::new("write-schedule");
	let db = fixture_db("all-features", &scratch);
	let data = loaded(&db);
	let before = data
		.schedules
		.values()
		.find(|s| s.next_date.is_some())
		.unwrap()
		.clone();
	let after = veritas_domain::Schedule {
		next_date: Some(CivilDate::new(2027, 1, 15).unwrap()),
		..before.clone()
	};
	let mut store = Store::open(&db, Access::ReadWrite).unwrap();
	let mut writer = store.writer().unwrap();
	writer.put_schedule(Some(&before), Some(&after)).unwrap();
	writer.commit().unwrap();
	let raw = Connection::open(&db).unwrap();
	let next: i64 = raw
		.query_row(
			"SELECT CASE WHEN local_next_date_ts = base_next_date_ts THEN local_next_date ELSE base_next_date END \
			 FROM schedules_next_date WHERE schedule_id = ?1",
			[before.id.as_str()],
			|row| row.get(0),
		)
		.unwrap();
	assert_eq!(next, 20_270_115);
	assert_eq!(loaded(&db).schedules[&before.id], after);
}

#[test]
fn the_store_sets_wal_even_on_a_file_restored_in_delete_mode() {
	let scratch = Scratch::new("write-wal");
	let db = fixture_db("small", &scratch);
	let raw = Connection::open(&db).unwrap();
	let mode: String = raw
		.query_row("PRAGMA journal_mode = DELETE", [], |row| row.get(0))
		.unwrap();
	assert_eq!(mode, "delete");
	drop(raw);
	let store = Store::open(&db, Access::ReadWrite).unwrap();
	assert_eq!(store.pragma_text("journal_mode").unwrap(), "wal");
}

#[test]
fn a_write_protected_file_is_never_opened_for_writing() {
	let scratch = Scratch::new("write-protected");
	let db = fixture_db("small", &scratch);
	let writable = std::fs::metadata(&db).unwrap().permissions();
	let mut protected = writable.clone();
	protected.set_readonly(true);
	std::fs::set_permissions(&db, protected).unwrap();
	assert!(matches!(
		Store::open(&db, Access::ReadWrite),
		Err(CoreError::Io(_))
	));
	let store = Store::open(
		&db,
		Access::ReadOnly(veritas_domain::ReadOnlyReason::OpenElsewhere),
	)
	.unwrap();
	assert!(store.load().is_ok());
	drop(store);
	std::fs::set_permissions(&db, writable).unwrap();
}

#[test]
fn an_edit_to_a_duplicated_budget_cell_lands_in_the_row_the_loader_reads() {
	let scratch = Scratch::new("write-duplicate-cell");
	let db = fixture_db("small", &scratch);
	let category = loaded(&db)
		.categories
		.values()
		.find(|c| !c.is_income)
		.unwrap()
		.id
		.clone();
	Connection::open(&db)
		.unwrap()
		.execute_batch(&format!(
			"INSERT INTO zero_budgets (id, month, category, amount) VALUES
			   ('2019-05-{category}', 201905, '{category}', 100),
			   ('201905-{category}', 201905, '{category}', 200);"
		))
		.unwrap();
	let may = YearMonth::parse("2019-05").unwrap();
	let before = loaded(&db).budget_cells[&(may, category.clone())].clone();
	assert_eq!(
		before.budgeted,
		Money::from_minor(200),
		"the later row is the one Actual's sheet shows"
	);
	let after = BudgetCell {
		budgeted: Money::from_minor(300),
		..before.clone()
	};
	let mut store = Store::open(&db, Access::ReadWrite).unwrap();
	let mut writer = store.writer().unwrap();
	writer.put_budget_cell(Some(&before), Some(&after)).unwrap();
	writer.commit().unwrap();
	assert_eq!(loaded(&db).budget_cells[&(may, category)], after);
}

#[test]
fn a_schedule_date_edit_lands_in_the_next_date_row_the_loader_reads() {
	let scratch = Scratch::new("write-duplicate-next-date");
	let db = fixture_db("all-features", &scratch);
	let data = loaded(&db);
	let before = data
		.schedules
		.values()
		.find(|s| s.next_date.is_some())
		.unwrap()
		.clone();
	// A tombstoned row ahead of the live one, as a sync can leave behind.
	Connection::open(&db)
		.unwrap()
		.execute_batch(&format!(
			"CREATE TEMP TABLE live AS SELECT * FROM schedules_next_date WHERE schedule_id = '{id}';
			 DELETE FROM schedules_next_date WHERE schedule_id = '{id}';
			 INSERT INTO schedules_next_date (id, schedule_id, local_next_date, local_next_date_ts, base_next_date, base_next_date_ts, tombstone)
			   VALUES ('dead-row', '{id}', 20200101, 1, 20200101, 1, 1);
			 INSERT INTO schedules_next_date SELECT * FROM live;",
			id = before.id
		))
		.unwrap();
	assert_eq!(loaded(&db).schedules[&before.id], before);
	let after = veritas_domain::Schedule {
		next_date: Some(CivilDate::new(2027, 2, 1).unwrap()),
		..before.clone()
	};
	let mut store = Store::open(&db, Access::ReadWrite).unwrap();
	let mut writer = store.writer().unwrap();
	writer.put_schedule(Some(&before), Some(&after)).unwrap();
	writer.commit().unwrap();
	assert_eq!(loaded(&db).schedules[&before.id], after);
}

#[test]
fn a_new_budget_row_leaves_goal_columns_null_like_actuals_set_budget() {
	let scratch = Scratch::new("write-cell-defaults");
	let db = fixture_db("small", &scratch);
	let category = loaded(&db)
		.categories
		.values()
		.find(|c| !c.is_income)
		.unwrap()
		.id
		.clone();
	let cell = BudgetCell {
		month: YearMonth::parse("2019-06").unwrap(),
		category: category.clone(),
		budgeted: Money::from_minor(1_000),
		carryover: false,
		goal: None,
		long_goal: false,
	};
	let mut store = Store::open(&db, Access::ReadWrite).unwrap();
	let mut writer = store.writer().unwrap();
	writer.put_budget_cell(None, Some(&cell)).unwrap();
	writer.commit().unwrap();
	let stored = row(
		&Connection::open(&db).unwrap(),
		"zero_budgets",
		&format!("201906-{category}"),
	);
	let value = |name: &str| {
		stored
			.iter()
			.find(|(column, _)| column == name)
			.unwrap()
			.1
			.clone()
	};
	assert_eq!(value("goal"), Value::Null);
	assert_eq!(value("long_goal"), Value::Null);
	assert_eq!(value("carryover"), Value::Integer(0));
	assert_eq!(loaded(&db).budget_cells[&(cell.month, category)], cell);
}

#[test]
fn tax_tags_are_added_changed_and_removed() {
	let scratch = Scratch::new("write-tax-tags");
	let db = fixture_db("small", &scratch);
	let mut store = Store::open(&db, Access::ReadWrite).unwrap();
	store.adopt().unwrap();
	let mut writer = store.writer().unwrap();
	writer.put_tax_tag("rx", None, Some("medical")).unwrap();
	writer.put_tax_tag("notax", None, Some("none")).unwrap();
	writer.commit().unwrap();
	let mut writer = store.writer().unwrap();
	writer
		.put_tax_tag("rx", Some("medical"), Some("hsa"))
		.unwrap();
	writer.put_tax_tag("notax", Some("none"), None).unwrap();
	writer.commit().unwrap();
	let tags = loaded(&db).tax_tags;
	assert_eq!(tags.len(), 1);
	assert_eq!(tags["rx"], "hsa");
}
