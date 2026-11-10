//! Loading Actual budgets into BudgetData, against the committed fixtures
//! and hand-made copies that carry Actual's quirks.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::Path;

use common::{Scratch, count, fixture_db};
use rusqlite::Connection;
use veritas_domain::{
	BudgetType, CategoryId, CategoryKind, CoreError, GoalStatus, Money, PayeeId, ReadOnlyReason,
	ScheduleId, SplitRole, TransactionId, YearMonth,
};
use veritas_store::{Access, Store, VERITAS_SCHEMA_VERSION};

#[test]
fn all_features_loads_every_modeled_row_including_tombstones() {
	let scratch = Scratch::new("all-features");
	let db_path = fixture_db("all-features", &scratch);
	let raw = Connection::open(&db_path).unwrap();
	let loaded = Store::open(&db_path, Access::ReadWrite)
		.unwrap()
		.load()
		.unwrap();
	let data = &loaded.data;

	assert_eq!(loaded.report.skipped, vec![]);
	assert_eq!(data.budget_type, BudgetType::Envelope);
	for (table, len) in [
		("accounts", data.accounts.len()),
		("account_groups", data.account_groups.len()),
		("category_groups", data.category_groups.len()),
		("categories", data.categories.len()),
		("payees", data.payees.len()),
		("transactions", data.transactions.len()),
		("zero_budgets", data.budget_cells.len()),
		("zero_budget_months", data.holds.len()),
		("rules", data.rules.len()),
		("schedules", data.schedules.len()),
		("tags", data.tags.len()),
		("notes", data.notes.len()),
		("category_mapping", data.category_mapping.len()),
		("payee_mapping", data.payee_mapping.len()),
	] {
		assert_eq!(
			len,
			count(&raw, &format!("SELECT count(*) FROM {table}")),
			"{table}"
		);
	}

	let tombstoned = data.transactions.values().filter(|t| t.tombstone).count();
	assert_eq!(
		tombstoned,
		count(
			&raw,
			"SELECT count(*) FROM transactions WHERE tombstone = 1"
		)
	);
	assert!(tombstoned > 0);
	let parents = data
		.transactions
		.values()
		.filter(|t| t.split == SplitRole::Parent)
		.collect::<Vec<_>>();
	assert_eq!(
		parents.len(),
		count(&raw, "SELECT count(*) FROM transactions WHERE isParent = 1")
	);
	assert!(parents.iter().all(|parent| parent.category.is_none()));
	let children = data
		.transactions
		.values()
		.filter(|t| matches!(t.split, SplitRole::Child { .. }))
		.count();
	assert_eq!(
		children,
		count(&raw, "SELECT count(*) FROM transactions WHERE isChild = 1")
	);
	let transfers = data
		.transactions
		.values()
		.filter(|t| t.transfer.is_some())
		.count();
	assert_eq!(
		transfers,
		count(
			&raw,
			"SELECT count(*) FROM transactions WHERE transferred_id IS NOT NULL"
		)
	);

	let merged = data
		.category_mapping
		.iter()
		.find(|(from, to)| from != to)
		.map(|(from, _)| from.clone())
		.unwrap();
	assert_ne!(data.resolve_category(&merged), Some(&merged));
	let self_mapped = data
		.category_mapping
		.iter()
		.filter(|(from, to)| from == to)
		.count();
	assert_eq!(
		self_mapped,
		count(
			&raw,
			"SELECT count(*) FROM category_mapping WHERE id = transferId"
		)
	);
	assert!(self_mapped > 0);
	assert!(data.categories[&merged].tombstone);
	assert!(data.payee_mapping.iter().any(|(from, to)| from != to));

	let (payee, amount, date): (String, i64, i64) = raw
		.query_row(
			"SELECT description, amount, date FROM transactions WHERE isParent = 0 AND tombstone = 0 \
			 AND description IS NOT NULL ORDER BY id LIMIT 1",
			[],
			|row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
		)
		.unwrap();
	let sample = data
		.transactions
		.values()
		.filter(|t| t.split != SplitRole::Parent && !t.tombstone && t.payee.is_some())
		.min_by(|a, b| a.id.cmp(&b.id))
		.unwrap();
	assert_eq!(sample.payee, Some(PayeeId::new(payee)));
	assert_eq!(sample.amount, Money::from_minor(amount));
	assert_eq!(
		i64::from(sample.date.year()) * 10_000
			+ i64::from(sample.date.month()) * 100
			+ i64::from(sample.date.day()),
		date
	);

	let hold: i64 = raw
		.query_row(
			"SELECT buffered FROM zero_budget_months WHERE id = '2026-07'",
			[],
			|row| row.get(0),
		)
		.unwrap();
	assert_eq!(
		data.holds[&YearMonth::parse("2026-07").unwrap()],
		Some(Money::from_minor(hold))
	);

	let (schedule, next): (String, i64) = raw
		.query_row(
			"SELECT schedule_id, base_next_date FROM schedules_next_date ORDER BY id LIMIT 1",
			[],
			|row| Ok((row.get(0)?, row.get(1)?)),
		)
		.unwrap();
	let next_date = data.schedules[&ScheduleId::new(schedule)]
		.next_date
		.unwrap();
	assert_eq!(next_date.to_string().replace('-', ""), next.to_string());
	assert!(
		data.rules
			.values()
			.any(|rule| rule.conditions_json.contains("description"))
	);
}

#[test]
fn the_working_copy_runs_in_wal_with_full_sync_and_no_foreign_keys() {
	let scratch = Scratch::new("pragmas");
	let store = Store::open(&fixture_db("small", &scratch), Access::ReadWrite).unwrap();
	assert_eq!(store.pragma_text("journal_mode").unwrap(), "wal");
	assert_eq!(store.pragma_text("synchronous").unwrap(), "2");
	assert_eq!(store.pragma_text("foreign_keys").unwrap(), "0");
}

#[test]
fn adopting_adds_the_veritas_tables_and_leaves_migrations_alone() {
	let scratch = Scratch::new("adopt");
	let db_path = fixture_db("small", &scratch);
	let migrations = |path: &Path| -> String {
		Connection::open(path)
			.unwrap()
			.query_row(
				"SELECT group_concat(id) FROM (SELECT id FROM __migrations__ ORDER BY id)",
				[],
				|row| row.get(0),
			)
			.unwrap()
	};
	let before = migrations(&db_path);
	let mut store = Store::open(&db_path, Access::ReadWrite).unwrap();
	store.adopt().unwrap();
	store.adopt().unwrap();
	assert_eq!(store.migration_ids().unwrap().len(), 60);
	drop(store);
	assert_eq!(migrations(&db_path), before);

	let raw = Connection::open(&db_path).unwrap();
	let version: String = raw
		.query_row(
			"SELECT value FROM veritas_meta WHERE key = 'schema_version'",
			[],
			|row| row.get(0),
		)
		.unwrap();
	assert_eq!(version, VERITAS_SCHEMA_VERSION.to_string());
	assert_eq!(
		count(
			&raw,
			"SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name LIKE 'veritas\\_%' ESCAPE '\\'"
		),
		8
	);
	assert_eq!(
		count(
			&raw,
			"SELECT count(*) FROM sqlite_master WHERE name LIKE 'veritas%' AND instr(sql, char(13)) > 0"
		),
		0
	);
}

#[test]
fn a_budget_from_a_newer_veritas_schema_is_read_only() {
	let scratch = Scratch::new("schema-version");
	let db_path = fixture_db("small", &scratch);
	let mut store = Store::open(&db_path, Access::ReadWrite).unwrap();
	store.adopt().unwrap();
	Connection::open(&db_path)
		.unwrap()
		.execute(
			"UPDATE veritas_meta SET value = '2' WHERE key = 'schema_version'",
			[],
		)
		.unwrap();
	assert!(matches!(
		store.adopt(),
		Err(CoreError::ReadOnly(ReadOnlyReason::NewerVeritasSchema))
	));
	Connection::open(&db_path)
		.unwrap()
		.execute(
			"UPDATE veritas_meta SET value = 'banana' WHERE key = 'schema_version'",
			[],
		)
		.unwrap();
	assert!(matches!(store.adopt(), Err(CoreError::Storage(_))));
}

#[test]
fn veritas_metadata_loads_when_present() {
	let scratch = Scratch::new("veritas-meta");
	let db_path = fixture_db("small", &scratch);
	let mut store = Store::open(&db_path, Access::ReadWrite).unwrap();
	assert!(store.load().unwrap().data.goals.is_empty());
	store.adopt().unwrap();
	let raw = Connection::open(&db_path).unwrap();
	let (category, account): (String, String) = raw
		.query_row(
			"SELECT (SELECT id FROM categories WHERE is_income = 0 ORDER BY id LIMIT 1), \
			 (SELECT id FROM accounts ORDER BY id LIMIT 1)",
			[],
			|row| Ok((row.get(0)?, row.get(1)?)),
		)
		.unwrap();
	raw.execute_batch(&format!(
		"INSERT INTO veritas_category_meta VALUES ('{category}', 'savings', 'hsa', 20270115);
		 INSERT INTO veritas_account_meta VALUES ('{account}', 'liability', 649, 31250, 15, 'Credit union');
		 INSERT INTO veritas_category_meta VALUES ('kindless', NULL, NULL, NULL);
		 INSERT INTO veritas_tax_tag VALUES ('medical', 'medical'), ('gift', 'none');
		 INSERT INTO veritas_transaction_meta VALUES ('t-adjust', 1), ('t-plain', 0);
		 INSERT INTO veritas_goal (id, category_id, target_cents, target_date, status, reached_month, created_at)
		   VALUES ('g1', '{category}', 500000, 20271201, 'reached', '2027-06', 0);"
	))
	.unwrap();
	let data = store.load().unwrap().data;
	let meta = &data.category_meta[&CategoryId::new(&category)];
	assert_eq!(meta.kind, Some(CategoryKind::Savings));
	assert_eq!(meta.tax_bucket.as_deref(), Some("hsa"));
	assert_eq!(meta.archived_on.unwrap().to_string(), "2027-01-15");
	assert_eq!(data.category_meta[&CategoryId::new("kindless")].kind, None);
	assert_eq!(data.tax_tags["medical"], "medical");
	assert_eq!(data.tax_tags["gift"], "none");
	assert!(data.transaction_meta[&TransactionId::new("t-adjust")].adjustment);
	assert!(!data.transaction_meta[&TransactionId::new("t-plain")].adjustment);
	let loan = &data.account_meta[&veritas_domain::AccountId::new(&account)];
	assert_eq!(loan.loan_payment, Some(Money::from_minor(31250)));
	assert_eq!(loan.loan_payment_day, Some(15));
	let goal = data.goals.values().next().unwrap();
	assert_eq!(goal.status, GoalStatus::Reached);
	assert_eq!(
		goal.reached_month,
		Some(YearMonth::parse("2027-06").unwrap())
	);
	assert_eq!(goal.target, Money::from_minor(500_000));
}

#[test]
fn actuals_quirks_load_and_unrepresentable_rows_are_counted_not_fatal() {
	let scratch = Scratch::new("quirks");
	let db_path = fixture_db("small", &scratch);
	let raw = Connection::open(&db_path).unwrap();
	let (account, category, group): (String, String, String) = raw
		.query_row(
			"SELECT (SELECT id FROM accounts ORDER BY id LIMIT 1), \
			 (SELECT id FROM categories WHERE is_income = 0 ORDER BY id LIMIT 1), \
			 (SELECT cat_group FROM categories ORDER BY id LIMIT 1)",
			[],
			|row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
		)
		.unwrap();
	raw.execute_batch(&format!(
		"INSERT INTO zero_budgets (id, month, category, amount) VALUES ('2019-03-{category}', 201903, '{category}', 4200);
		 INSERT INTO zero_budgets (id, month, category, amount, carryover) VALUES ('201904-{category}', 201904, '{category}', NULL, 1);
		 INSERT INTO zero_budgets (id, month, category, amount) VALUES ('2019-04-{category}', 201904, '{category}', 99);
		 INSERT INTO transactions (id, acct, amount, date, description) VALUES ('null-amount', '{account}', NULL, 20261101, NULL);
		 INSERT INTO transactions (id, acct, amount, date) VALUES ('no-date', '{account}', 100, NULL);
		 INSERT INTO transactions (id, acct, amount, date, isChild, parent_id) VALUES ('child-flag-2', '{account}', 1, 20261101, 2, 'parent');
		 INSERT INTO transactions (id, acct, amount, date, isChild) VALUES ('child-flag-text', '{account}', 1, 20261101, 'yes');
		 INSERT INTO transactions (id, acct, amount, date, tombstone) VALUES ('null-tombstone', '{account}', 1, 20261101, NULL);
		 INSERT INTO categories (id, name, cat_group, tombstone) VALUES ('null-tombstone', 'Gone', '{group}', NULL);
		 INSERT INTO transactions (id, acct, amount, date, imported_description) VALUES ('imported', '{account}', -100, 20261101, 'ACME*STORE 123');
		 INSERT INTO transactions (id, acct, amount, date) VALUES ('bad-date', '{account}', 100, 20270230);
		 INSERT INTO transactions (id, acct, amount, date) VALUES ('fraction', '{account}', 12.5, 20261101);
		 INSERT INTO transactions (id, acct, amount, date) VALUES ('no-account', NULL, 100, 20261101);
		 INSERT INTO transactions (id, acct, amount, date, isChild) VALUES ('orphan-child', '{account}', 100, 20261101, 1);
		 INSERT INTO transactions (id, acct, amount, date, isParent, category) VALUES ('parent', '{account}', 100, 20261101, 1, '{category}');
		 INSERT INTO transactions (id, acct, amount, date, isChild, parent_id) VALUES ('parent/child', '{account}', 100, 20261101, 1, 'parent');
		 INSERT INTO transactions (id, acct, amount, date, tombstone, cleared) VALUES ('odd-flags', '{account}', 1, 20261101, 2, NULL);
		 INSERT INTO categories (id, name, cat_group) VALUES ('no-group', 'Orphan', NULL);
		 INSERT INTO categories (id, name, cat_group, hidden) VALUES ('nameless', NULL, '{group}', 1);
		 INSERT INTO payees (id, name) VALUES ('nameless-payee', NULL);
		 INSERT INTO schedules (id, rule) VALUES ('no-rule', NULL);
		 INSERT INTO rules (id, stage, conditions, actions) VALUES ('odd-stage', 'later', '[]', '[]');
		 INSERT INTO rules (id, conditions, actions) VALUES ('no-actions', '[]', NULL);
		 INSERT INTO category_mapping VALUES ('dangling', NULL);
		 INSERT INTO notes VALUES ('empty-note', NULL);
		 INSERT INTO zero_budget_months VALUES ('2019-03', NULL);
		 INSERT INTO accounts (id, name, sort_order) VALUES (NULL, 'No id', 'first');"
	))
	.unwrap();
	drop(raw);

	let loaded = Store::open(&db_path, Access::ReadWrite)
		.unwrap()
		.load()
		.unwrap();
	let data = &loaded.data;
	let march = YearMonth::parse("2019-03").unwrap();
	let april = YearMonth::parse("2019-04").unwrap();
	assert_eq!(
		data.budget_cells[&(march, CategoryId::new(&category))].budgeted,
		Money::from_minor(4200)
	);
	let april_cell = &data.budget_cells[&(april, CategoryId::new(&category))];
	assert_eq!(april_cell.budgeted, Money::from_minor(99));
	assert!(!april_cell.carryover);

	let transaction = |id: &str| data.transactions.get(&TransactionId::new(id));
	assert_eq!(
		transaction("imported").unwrap().imported_payee.as_deref(),
		Some("ACME*STORE 123")
	);
	assert_eq!(transaction("null-amount").unwrap().imported_payee, None);
	assert_eq!(transaction("null-amount").unwrap().amount, Money::ZERO);
	assert_eq!(transaction("null-amount").unwrap().payee, None);
	assert_eq!(transaction("parent").unwrap().category, None);
	assert_eq!(
		transaction("parent/child").unwrap().split,
		SplitRole::Child {
			parent: TransactionId::new("parent")
		}
	);
	let odd = transaction("odd-flags").unwrap();
	assert!(odd.tombstone);
	assert!(!odd.cleared);
	assert_eq!(data.categories[&CategoryId::new("nameless")].name, "");
	assert!(data.categories[&CategoryId::new("nameless")].hidden);
	assert_eq!(data.payees[&PayeeId::new("nameless-payee")].name, "");
	assert_eq!(
		data.rules[&veritas_domain::RuleId::new("odd-stage")].stage,
		veritas_domain::RuleStage::Default
	);
	assert!(!data.notes.contains_key("empty-note"));
	// Actual's transaction views use IFNULL(tombstone, 0); every other read is tombstone = 0.
	assert!(!transaction("null-tombstone").unwrap().tombstone);
	assert!(data.categories[&CategoryId::new("null-tombstone")].tombstone);
	assert_eq!(data.holds[&march], None);

	let mut skipped: Vec<(&str, Option<String>)> = loaded
		.report
		.skipped
		.iter()
		.map(|row| (row.table, row.id.clone()))
		.collect();
	skipped.sort();
	let some = |id: &str| Some(id.to_owned());
	assert_eq!(
		skipped,
		vec![
			("accounts", None),
			("categories", some("no-group")),
			("category_mapping", some("dangling")),
			("rules", some("no-actions")),
			("schedules", some("no-rule")),
			("transactions", some("bad-date")),
			("transactions", some("child-flag-2")),
			("transactions", some("child-flag-text")),
			("transactions", some("fraction")),
			("transactions", some("no-account")),
			("transactions", some("no-date")),
			("transactions", some("orphan-child")),
			("zero_budgets", Some(format!("2019-04-{category}"))),
		]
	);
}

#[test]
fn a_tracking_budget_reads_reflect_budgets() {
	let scratch = Scratch::new("tracking");
	let db_path = fixture_db("all-features", &scratch);
	let raw = Connection::open(&db_path).unwrap();
	raw.execute(
		"UPDATE preferences SET value = 'tracking' WHERE id = 'budgetType'",
		[],
	)
	.unwrap();
	let reflect = count(&raw, "SELECT count(*) FROM reflect_budgets");
	drop(raw);
	let data = Store::open(&db_path, Access::ReadWrite)
		.unwrap()
		.load()
		.unwrap()
		.data;
	assert_eq!(data.budget_type, BudgetType::Tracking);
	assert_eq!(data.budget_cells.len(), reflect);
	assert!(data.holds.is_empty(), "tracking budgets have no holds");
}

#[test]
fn a_missing_or_legacy_budget_type_is_envelope() {
	let scratch = Scratch::new("budget-type");
	let db_path = fixture_db("small", &scratch);
	let raw = Connection::open(&db_path).unwrap();
	raw.execute(
		"UPDATE preferences SET value = 'report' WHERE id = 'budgetType'",
		[],
	)
	.unwrap();
	let store = Store::open(&db_path, Access::ReadWrite).unwrap();
	assert_eq!(store.load().unwrap().data.budget_type, BudgetType::Envelope);
	raw.execute("DELETE FROM preferences WHERE id = 'budgetType'", [])
		.unwrap();
	assert_eq!(store.load().unwrap().data.budget_type, BudgetType::Envelope);
}

fn loaded_counts(loaded: &veritas_store::Loaded) -> Vec<(&'static str, usize)> {
	let data = &loaded.data;
	vec![
		("accounts", data.accounts.len()),
		("account_groups", data.account_groups.len()),
		("category_groups", data.category_groups.len()),
		("categories", data.categories.len()),
		("payees", data.payees.len()),
		("transactions", data.transactions.len()),
		("zero_budgets", data.budget_cells.len()),
		("zero_budget_months", data.holds.len()),
		("rules", data.rules.len()),
		("schedules", data.schedules.len()),
		("tags", data.tags.len()),
		("notes", data.notes.len()),
		("category_mapping", data.category_mapping.len()),
		("payee_mapping", data.payee_mapping.len()),
	]
}

/// Every column of every modeled table, set to each kind of value a damaged
/// or hand-edited file can hold. The load never fails, every row is either
/// loaded or reported, and no other table loses a row.
#[test]
fn no_odd_value_in_any_modeled_column_aborts_the_load_or_drops_a_row_silently() {
	let scratch = Scratch::new("odd-values");
	let pristine = fixture_db("all-features", &scratch);
	let baseline = Store::open(&pristine, Access::ReadWrite)
		.unwrap()
		.load()
		.unwrap();
	assert_eq!(baseline.report.skipped, vec![]);
	let baseline_counts = loaded_counts(&baseline);
	let mut tables: Vec<&str> = baseline_counts.iter().map(|(table, _)| *table).collect();
	tables.push("schedules_next_date");

	let odd_values = ["NULL", "X'00ff'", "'not a number'", "1e300", "-1", "2.5"];
	let mut exercised = 0;
	for table in tables {
		let columns: Vec<String> = {
			let conn = Connection::open(&pristine).unwrap();
			let mut statement = conn
				.prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
				.unwrap();
			statement
				.query_map([], |row| row.get(0))
				.unwrap()
				.map(Result::unwrap)
				.collect()
		};
		assert!(!columns.is_empty(), "{table}");
		for column in &columns {
			for value in odd_values {
				let case = format!("{table}.{column} = {value}");
				let db_path = scratch.0.join(format!("case-{exercised}.sqlite"));
				std::fs::copy(&pristine, &db_path).unwrap();
				let raw = Connection::open(&db_path).unwrap();
				// A value the schema's own constraints refuse can't be in a file.
				if raw
					.execute(&format!("UPDATE {table} SET \"{column}\" = {value}"), [])
					.is_err()
				{
					continue;
				}
				// A NULL note is how Actual clears one, so it is no row at all.
				let present = if table == "notes" {
					" WHERE note IS NOT NULL"
				} else {
					""
				};
				let rows = count(&raw, &format!("SELECT count(*) FROM {table}{present}"));
				drop(raw);

				let loaded = Store::open(&db_path, Access::ReadWrite)
					.unwrap()
					.load()
					.unwrap_or_else(|error| panic!("{case}: the load failed: {error}"));
				let skipped_here = loaded
					.report
					.skipped
					.iter()
					.filter(|row| row.table == table)
					.count();
				for ((name, now), (_, before)) in
					loaded_counts(&loaded).iter().zip(&baseline_counts)
				{
					if *name == table {
						assert_eq!(now + skipped_here, rows, "{case}: rows went missing");
					} else if !(table == "schedules_next_date" && *name == "schedules") {
						assert_eq!(now, before, "{case}: {name} lost rows");
					}
				}
				exercised += 1;
				drop(loaded);
				let _ = std::fs::remove_file(&db_path);
				let _ = std::fs::remove_file(db_path.with_extension("sqlite-wal"));
				let _ = std::fs::remove_file(db_path.with_extension("sqlite-shm"));
			}
		}
	}
	assert!(exercised > 400, "only {exercised} cases ran");
}
