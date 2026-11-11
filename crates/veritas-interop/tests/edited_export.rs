//! Exports after real edits through the writer. The same edits made in Actual
//! must give Actual the same budget (R-IO-2), Actual's own re-export must keep
//! the veritas_* rows (R-IO-6), and a category deleted in Actual leaves a
//! dangling veritas reference the audit reports instead of crashing.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::{Path, PathBuf};

use common::{Scratch, fixture, imported, oracle};
use serde_json::{Value, json};
use veritas_domain::{
	AccountId, AccountKind, AccountMeta, BudgetCell, BudgetData, CategoryId, CategoryKind,
	CategoryMeta, CivilDate, Goal, GoalId, GoalStatus, Money, PayeeId, SplitRole, Transaction,
	TransactionId, TransactionMeta, YearMonth,
};
use veritas_interop::{ExportOutcome, canonical_dump_to_vec, export_actual_zip, import_actual_zip};
use veritas_store::{Access, Check, Store};

struct Picks {
	from: AccountId,
	to: AccountId,
	category: CategoryId,
	deleted: Transaction,
}

fn transfer_payee(data: &BudgetData, account: &AccountId) -> PayeeId {
	data.payees
		.values()
		.find(|payee| payee.transfer_account.as_ref() == Some(account) && !payee.tombstone)
		.unwrap()
		.id
		.clone()
}

fn pick(data: &BudgetData) -> Picks {
	let mut accounts = data
		.accounts
		.values()
		.filter(|account| !account.off_budget && !account.closed && !account.tombstone)
		.map(|account| account.id.clone());
	let from = accounts.next().unwrap();
	let to = accounts.next().unwrap();
	let category = data
		.categories
		.values()
		.find(|category| {
			!category.is_income
				&& !category.hidden
				&& !category.tombstone
				&& !data.category_groups[&category.group].tombstone
		})
		.unwrap()
		.id
		.clone();
	let deleted =
		data.transactions
			.values()
			.find(|t| {
				t.split == SplitRole::None
					&& t.transfer.is_none()
					&& t.schedule.is_none()
					&& !t.starting_balance
					&& !t.tombstone && t.category.is_some()
			})
			.unwrap()
			.clone();
	Picks {
		from,
		to,
		category,
		deleted,
	}
}

const MONTH: &str = "2026-10";

fn plain(id: &str, account: &AccountId, day: u32, amount: i64) -> Transaction {
	Transaction {
		id: TransactionId::new(id),
		account: account.clone(),
		date: CivilDate::new(2026, 10, day).unwrap(),
		amount: Money::from_minor(amount),
		payee: None,
		imported_payee: None,
		category: None,
		notes: None,
		cleared: false,
		reconciled: false,
		split: SplitRole::None,
		transfer: None,
		schedule: None,
		starting_balance: false,
		sort_order: 1_794_000_000_000.0 + f64::from(day),
		tombstone: false,
	}
}

/// A budget edit, a new transaction, a transfer between two on-budget
/// accounts and a deletion, plus Veritas-only rows on the same category.
fn edit_in_veritas(store: &mut Store, data: &BudgetData, picks: &Picks) {
	let month = YearMonth::parse(MONTH).unwrap();
	let cell_before = data.budget_cells.get(&(month, picks.category.clone()));
	let cell_after = BudgetCell {
		month,
		category: picks.category.clone(),
		budgeted: Money::from_minor(12345),
		carryover: cell_before.is_some_and(|cell| cell.carryover),
		goal: cell_before.and_then(|cell| cell.goal),
		long_goal: cell_before.is_some_and(|cell| cell.long_goal),
	};
	let groceries = Transaction {
		category: Some(picks.category.clone()),
		..plain("veritas-groceries", &picks.from, 14, -4512)
	};
	let out_leg = Transaction {
		payee: Some(transfer_payee(data, &picks.to)),
		transfer: Some(TransactionId::new("veritas-transfer-in")),
		..plain("veritas-transfer-out", &picks.from, 20, -25000)
	};
	let in_leg = Transaction {
		payee: Some(transfer_payee(data, &picks.from)),
		transfer: Some(TransactionId::new("veritas-transfer-out")),
		..plain("veritas-transfer-in", &picks.to, 20, 25000)
	};
	let tombstoned = Transaction {
		tombstone: true,
		..picks.deleted.clone()
	};

	let mut writer = store.writer().unwrap();
	writer
		.put_budget_cell(cell_before, Some(&cell_after))
		.unwrap();
	writer.put_transaction(None, Some(&groceries)).unwrap();
	writer.put_transaction(None, Some(&out_leg)).unwrap();
	writer.put_transaction(None, Some(&in_leg)).unwrap();
	writer
		.put_transaction(Some(&picks.deleted), Some(&tombstoned))
		.unwrap();
	writer
		.put_category_meta(
			&picks.category,
			data.category_meta.get(&picks.category),
			Some(&CategoryMeta {
				kind: Some(CategoryKind::Savings),
				tax_bucket: Some("hsa".to_owned()),
				archived_on: None,
			}),
		)
		.unwrap();
	writer
		.put_account_meta(
			&picks.from,
			data.account_meta.get(&picks.from),
			Some(&AccountMeta {
				kind: Some(AccountKind::Asset),
				loan_rate_bps: None,
				loan_payment: None,
				loan_payment_day: None,
				lender: None,
			}),
		)
		.unwrap();
	writer
		.put_transaction_meta(
			&groceries.id,
			None,
			Some(&TransactionMeta { adjustment: false }),
		)
		.unwrap();
	writer
		.put_goal(
			None,
			Some(&Goal {
				id: GoalId::new("veritas-goal"),
				category: picks.category.clone(),
				target: Money::from_minor(500000),
				target_date: Some(CivilDate::new(2027, 6, 1).unwrap()),
				icon: None,
				color: None,
				status: GoalStatus::Active,
				reached_month: None,
				closed_month: None,
				released_to: None,
				released: None,
			}),
		)
		.unwrap();
	writer.commit().unwrap();
}

/// The same edits as Actual's API makes them, for `oracle apply`.
fn same_edits_in_actual(picks: &Picks) -> Value {
	json!({
		"version": 1,
		"ops": [
			{"op": "setBudgetAmount", "month": MONTH, "category": picks.category.as_str(), "amount": 12345},
			{"op": "addTransactions", "account": picks.from.as_str(), "transactions": [
				{"id": "veritas-groceries", "date": "2026-10-14", "amount": -4512,
				 "category": picks.category.as_str(), "cleared": false},
				{"id": "veritas-transfer-out", "date": "2026-10-20", "amount": -25000,
				 "payee": {"transferTo": picks.to.as_str()}, "cleared": false}
			]},
			{"op": "deleteTransaction", "id": picks.deleted.id.as_str()}
		]
	})
}

fn apply_in_actual(zip: &Path, ops: &Value, out: &Path, scratch: &Scratch) -> Option<()> {
	let ops_path = scratch.join(&format!(
		"{}.ops.json",
		out.file_stem().unwrap().to_string_lossy()
	));
	std::fs::write(&ops_path, serde_json::to_vec(ops).unwrap()).unwrap();
	oracle(&[
		"apply".as_ref(),
		zip.as_os_str(),
		ops_path.as_os_str(),
		"--out".as_ref(),
		out.as_os_str(),
	])
	.map(|_| ())
}

/// Actual's full dump of a zip: it imports and loads the file, so this fails
/// if Actual refuses it.
fn actual_computed(zip: &Path) -> Option<Value> {
	let dump = oracle(&["dump".as_ref(), zip.as_os_str()])?;
	let mut parsed: Value = serde_json::from_slice(&dump).unwrap();
	Some(parsed["computed"].take())
}

fn export(store: &Store, metadata: &Path, to: &Path) {
	match export_actual_zip(store, metadata, to).unwrap() {
		ExportOutcome::Exported(audit) => assert!(audit.is_clean(), "{audit:?}"),
		refused => panic!("export refused: {refused:?}"),
	}
}

fn edited_export(scratch: &Scratch) -> (PathBuf, Picks, BudgetData) {
	let budget = imported(import_actual_zip(
		&fixture("all-features"),
		&scratch.join("budget"),
	));
	let mut store = Store::open(&budget.db_path, Access::ReadWrite).unwrap();
	store.adopt().unwrap();
	let data = store.load().unwrap().data;
	let picks = pick(&data);
	edit_in_veritas(&mut store, &data, &picks);
	let (_, audit) = store.open_audit().unwrap();
	assert!(
		audit.is_clean(),
		"the writer's own rows trip the audit: {audit:?}"
	);
	let exported = scratch.join("veritas.zip");
	export(&store, &budget.metadata_path, &exported);
	(exported, picks, data)
}

#[test]
fn actual_computes_the_same_budget_from_veritas_edits_as_from_its_own_r_io_2() {
	let scratch = Scratch::new("edited-export");
	let (exported, picks, _) = edited_export(&scratch);

	let Some(ours) = actual_computed(&exported) else {
		return;
	};
	let actual_edited = scratch.join("actual.zip");
	apply_in_actual(
		&fixture("all-features"),
		&same_edits_in_actual(&picks),
		&actual_edited,
		&scratch,
	)
	.unwrap();
	let theirs = actual_computed(&actual_edited).unwrap();
	let untouched = actual_computed(&fixture("all-features")).unwrap();

	let cell =
		|computed: &Value| computed["months"][MONTH]["categories"][picks.category.as_str()].clone();
	assert_eq!(cell(&ours)["budgeted"], json!(12345));
	assert_ne!(
		ours["accounts"], untouched["accounts"],
		"the edits should move account balances"
	);
	assert_eq!(cell(&ours), cell(&theirs));
	assert_eq!(ours["accounts"], theirs["accounts"]);
	assert_eq!(ours, theirs, "Actual computes a different budget");
}

fn table_sections(dump: &[u8], prefix: &str) -> Vec<String> {
	let text = String::from_utf8(dump.to_vec()).unwrap();
	let mut sections = Vec::new();
	let mut current: Option<String> = None;
	for line in text.lines() {
		if let Some(name) = line
			.strip_prefix("\t\t\"")
			.and_then(|rest| rest.strip_suffix("\": {"))
		{
			current = name.starts_with(prefix).then(String::new);
		}
		if let Some(section) = current.as_mut() {
			section.push_str(line);
			section.push('\n');
			if line == "\t\t}" || line == "\t\t}," {
				sections.push(current.take().unwrap());
			}
		}
	}
	sections
}

#[test]
fn veritas_rows_survive_actuals_reexport_of_an_edited_budget_r_io_6() {
	let scratch = Scratch::new("veritas-rows-survive");
	let (exported, _, _) = edited_export(&scratch);
	let reexported = scratch.join("reexported.zip");
	if apply_in_actual(
		&exported,
		&json!({"version": 1, "ops": []}),
		&reexported,
		&scratch,
	)
	.is_none()
	{
		return;
	}
	let before_dump = canonical_dump_to_vec(&exported).unwrap();
	let after_dump = canonical_dump_to_vec(&reexported).unwrap();
	let before = table_sections(&before_dump, "veritas_");
	let after = table_sections(&after_dump, "veritas_");
	assert_eq!(before.len(), 8, "{before:#?}");
	for needle in ["veritas-goal", "\"hsa\"", "veritas-groceries", "\"asset\""] {
		assert!(
			before.iter().any(|section| section.contains(needle)),
			"{needle} isn't in the export"
		);
	}
	assert_eq!(after, before);

	// Loading the edited file doesn't make Actual rewrite any row Veritas
	// wrote, the transfer pair included. Only the new CRDT node id differs.
	let before_text = String::from_utf8(before_dump).unwrap();
	let after_text = String::from_utf8(after_dump).unwrap();
	let differing: Vec<(&str, &str)> = before_text
		.lines()
		.zip(after_text.lines())
		.filter(|(a, b)| a != b)
		.collect();
	assert_eq!(before_text.lines().count(), after_text.lines().count());
	assert_eq!(differing.len(), 1, "{differing:#?}");
	assert!(differing[0].0.contains("\"clock\":"), "{differing:?}");
}

#[test]
fn a_category_deleted_in_actual_leaves_a_reported_dangling_goal_r_io_6() {
	let scratch = Scratch::new("dangling-goal");
	let (exported, picks, data) = edited_export(&scratch);
	let other =
		data.categories
			.values()
			.find(|c| {
				c.id != picks.category
					&& !c.is_income && !c.tombstone
					&& !data.category_groups[&c.group].tombstone
			})
			.unwrap()
			.id
			.clone();

	for (label, target) in [("deleted", None), ("merged", Some(&other))] {
		let mut op = json!({"op": "deleteCategory", "id": picks.category.as_str()});
		if let Some(target) = target {
			op["transferCategory"] = json!(target.as_str());
		}
		let after_actual = scratch.join(&format!("{label}.zip"));
		if apply_in_actual(
			&exported,
			&json!({"version": 1, "ops": [op]}),
			&after_actual,
			&scratch,
		)
		.is_none()
		{
			return;
		}

		let budget = imported(import_actual_zip(&after_actual, &scratch.join(label)));
		assert_eq!(budget.access, veritas_interop::Access::ReadWrite);
		let dangling: Vec<_> = budget
			.audit
			.violations
			.iter()
			.filter(|violation| violation.related.as_deref() == Some(picks.category.as_str()))
			.filter(|violation| violation.table.starts_with("veritas_"))
			.map(|violation| (violation.table, violation.check.clone()))
			.collect();
		assert_eq!(
			dangling,
			[("veritas_goal", Check::DeletedReference("category"))],
			"{label}: {:?}",
			budget.audit
		);
		let store = Store::open(&budget.db_path, Access::ReadWrite).unwrap();
		let data = store.load().unwrap().data;
		assert_eq!(
			data.goals[&GoalId::new("veritas-goal")].category,
			picks.category
		);
		if let Some(target) = target {
			assert_eq!(data.resolve_category(&picks.category), Some(target));
		}
	}
}

#[test]
fn an_unknown_table_and_column_survive_an_edit_of_the_same_row_r_io_5() {
	let scratch = Scratch::new("unknown-parts-edited");
	let (db_path, metadata) = common::unpack(&fixture("small"), scratch.path());
	let conn = rusqlite::Connection::open(&db_path).unwrap();
	let id: String = conn
		.query_row(
			"SELECT id FROM transactions WHERE tombstone = 0 AND isParent = 0 AND isChild = 0 \
			 AND transferred_id IS NULL ORDER BY id LIMIT 1",
			[],
			|row| row.get(0),
		)
		.unwrap();
	conn.execute_batch(
		"CREATE TABLE future_feature (id TEXT PRIMARY KEY, payload BLOB, weight REAL);
		 INSERT INTO future_feature VALUES ('f1', X'cafe00', 0.1), ('f2', NULL, 1e300);
		 ALTER TABLE transactions ADD COLUMN future_flag INTEGER DEFAULT 7;
		 ALTER TABLE transactions ADD COLUMN future_note TEXT;",
	)
	.unwrap();
	conn.execute(
		"UPDATE transactions SET future_flag = 42, future_note = 'kept' WHERE id = ?1",
		[&id],
	)
	.unwrap();
	drop(conn);
	let source = scratch.join("future.zip");
	common::pack(&db_path, &metadata, &source);

	let budget = imported(import_actual_zip(&source, &scratch.join("budget")));
	let mut store = Store::open(&budget.db_path, Access::ReadWrite).unwrap();
	store.adopt().unwrap();
	let before = store.load().unwrap().data.transactions[&TransactionId::new(&id)].clone();
	let after = Transaction {
		amount: Money::from_minor(before.amount.minor() - 100),
		notes: Some("edited in Veritas".to_owned()),
		cleared: !before.cleared,
		..before.clone()
	};
	let mut writer = store.writer().unwrap();
	writer.put_transaction(Some(&before), Some(&after)).unwrap();
	writer.commit().unwrap();
	let exported = scratch.join("exported.zip");
	export(&store, &budget.metadata_path, &exported);

	// The same edit made directly in SQL, so the only expected difference
	// from the source is the three columns the user changed.
	let expected_dir = scratch.join("expected");
	std::fs::create_dir(&expected_dir).unwrap();
	let (expected_db, _) = common::unpack(&source, &expected_dir);
	rusqlite::Connection::open(&expected_db)
		.unwrap()
		.execute(
			"UPDATE transactions SET amount = ?1, notes = ?2, cleared = ?3 WHERE id = ?4",
			rusqlite::params![
				after.amount.minor(),
				"edited in Veritas",
				i64::from(after.cleared),
				id
			],
		)
		.unwrap();
	let expected = scratch.join("expected.zip");
	common::pack(&expected_db, &metadata, &expected);

	let ours = veritas_interop::canonical_dump_of_actual_tables(&exported).unwrap();
	common::assert_same_dump(
		&ours,
		&veritas_interop::canonical_dump_of_actual_tables(&expected).unwrap(),
		"edited row with unknown parts",
	);
	let text = String::from_utf8(ours).unwrap();
	assert!(text.contains("\"future_flag\":42,\"future_note\":\"kept\""));
	assert!(text.contains("{\"$blob\":\"cafe00\"}"));

	if let Some(actual) = common::actual_opens(&exported) {
		actual.unwrap();
	}
}
