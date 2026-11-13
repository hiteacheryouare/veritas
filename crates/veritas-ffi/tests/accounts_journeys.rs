#![allow(
	clippy::unwrap_used,
	clippy::expect_used,
	clippy::panic,
	reason = "integration tests fail by panicking"
)]

//! The Accounts screen (R-ACC-1 to R-ACC-5) driven through the exported API
//! the way Swift drives it, over whole sessions, with Actual's own reading of
//! the export as the verdict wherever the oracle is installed.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::Thread;

use serde_json::{Value, json};
use veritas_ffi::{
	AccountDetail, CancelToken, Change, Command, EntityKind, Month, NewAccount, NewAccountKind,
	PayeeDetail, RegisterFilter, RegisterRow, RegisterScope, RegisterSort, RegisterSortKey,
	Session, SplitLineDraft, TransactionDraft,
};

const TODAY: &str = "2026-11-15";
const NOVEMBER: Month = Month {
	year: 2026,
	month: 11,
};
const NEWEST_FIRST: RegisterSort = RegisterSort {
	key: RegisterSortKey::Date,
	ascending: false,
};

struct Unpark(Thread);

impl Wake for Unpark {
	fn wake(self: Arc<Self>) {
		self.0.unpark();
	}
}

fn block_on<F: Future>(future: F) -> F::Output {
	let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
	let mut context = Context::from_waker(&waker);
	let mut future = pin!(future);
	loop {
		if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
			return output;
		}
		std::thread::park();
	}
}

struct Scratch(PathBuf);

impl Scratch {
	fn new(label: &str) -> Self {
		static NEXT: AtomicUsize = AtomicUsize::new(0);
		let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
			"ffi-accounts-{label}-{}-{}",
			std::process::id(),
			NEXT.fetch_add(1, Ordering::Relaxed)
		));
		let _ = std::fs::remove_dir_all(&path);
		std::fs::create_dir_all(&path).unwrap();
		Scratch(path)
	}

	fn join(&self, name: &str) -> PathBuf {
		self.0.join(name)
	}
}

impl Drop for Scratch {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.0);
	}
}

fn small_zip() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/small.zip")
}

fn text(path: &Path) -> String {
	path.display().to_string()
}

/// Imports `zip` into `scratch/<name>` and pins the session's today.
fn import(scratch: &Scratch, zip: &Path, name: &str) -> Arc<Session> {
	let session = block_on(Session::import_actual_zip(
		text(zip),
		text(&scratch.join(name)),
		CancelToken::new(),
	))
	.unwrap();
	pin_today(&session);
	session
}

fn reopen(scratch: &Scratch, name: &str) -> Arc<Session> {
	let session = block_on(Session::open(text(&scratch.join(name)), CancelToken::new())).unwrap();
	pin_today(&session);
	session
}

/// Commands date closing transfers and adjustments with the day the shell
/// last read the budget as of.
fn pin_today(session: &Session) {
	session.budget_month(NOVEMBER, TODAY.to_owned()).unwrap();
}

fn export(session: &Session, zip: &Path) {
	block_on(session.export_actual_zip(text(zip), CancelToken::new())).unwrap();
}

fn perform(session: &Session, command: Command) -> Change {
	session.perform(command).unwrap()
}

fn created(change: &Change, kind: EntityKind) -> String {
	change
		.entities
		.iter()
		.find(|e| e.kind == kind && e.id.is_some())
		.and_then(|e| e.id.clone())
		.unwrap()
}

fn new_account(
	session: &Session,
	name: &str,
	kind: NewAccountKind,
	starting_balance: i64,
) -> String {
	let change = perform(
		session,
		Command::CreateAccount {
			account: NewAccount {
				name: name.to_owned(),
				kind,
				starting_balance,
			},
		},
	);
	created(&change, EntityKind::Account)
}

fn account(session: &Session, id: &str) -> AccountDetail {
	session
		.accounts()
		.unwrap()
		.accounts
		.into_iter()
		.find(|a| a.id == id)
		.unwrap()
}

fn transfer_payee(session: &Session, account: &str) -> String {
	session
		.payees()
		.unwrap()
		.into_iter()
		.find(|p| p.transfer_account_id.as_deref() == Some(account))
		.unwrap()
		.id
}

fn category(session: &Session, name: &str) -> String {
	session
		.budget_month(NOVEMBER, TODAY.to_owned())
		.unwrap()
		.groups
		.into_iter()
		.flat_map(|g| g.envelopes)
		.find(|e| e.name == name)
		.unwrap_or_else(|| panic!("small.zip has no category named {name}"))
		.id
}

fn entry(id: &str, account: &str, date: &str, amount: i64) -> TransactionDraft {
	TransactionDraft {
		id: Some(id.to_owned()),
		account_id: account.to_owned(),
		date: date.to_owned(),
		amount,
		payee_id: None,
		payee_name: None,
		category_id: None,
		notes: None,
		cleared: false,
		splits: Vec::new(),
	}
}

fn line(amount: i64, category: Option<&str>) -> SplitLineDraft {
	SplitLineDraft {
		id: None,
		amount,
		payee_id: None,
		payee_name: None,
		category_id: category.map(str::to_owned),
		notes: None,
	}
}

fn add(session: &Session, draft: TransactionDraft) -> String {
	let id = draft.id.clone().unwrap();
	perform(session, Command::AddTransaction { transaction: draft });
	id
}

fn edit(session: &Session, id: &str, draft: TransactionDraft) {
	perform(
		session,
		Command::EditTransaction {
			transaction_id: id.to_owned(),
			transaction: draft,
		},
	);
}

fn register(session: &Session, scope: RegisterScope) -> Vec<RegisterRow> {
	session
		.register(scope, RegisterFilter::default(), NEWEST_FIRST, 0, 100_000)
		.unwrap()
		.rows
}

fn in_account(account: &str) -> RegisterScope {
	RegisterScope::Account {
		account_id: account.to_owned(),
	}
}

fn sqlite(scratch: &Scratch, name: &str) -> rusqlite::Connection {
	rusqlite::Connection::open(scratch.join(name).join("db.sqlite")).unwrap()
}

/// Puts rows into a closed budget as an older Actual file would hold them.
fn inject(scratch: &Scratch, name: &str, sql: &str) {
	sqlite(scratch, name).execute_batch(sql).unwrap();
}

fn oracle() -> Option<PathBuf> {
	let oracle = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/actual-oracle");
	if oracle.join("node_modules").exists() {
		return Some(oracle);
	}
	assert!(
		std::env::var_os("VERITAS_REQUIRE_ORACLE").is_none(),
		"VERITAS_REQUIRE_ORACLE is set but tools/actual-oracle/node_modules is missing"
	);
	eprintln!("skipping the Actual side: run `cargo xtask oracle setup`");
	None
}

/// Actual imports and loads `zip`, and dumps its tables and its own
/// computed balances and budget months.
fn actual_reads(oracle: &Path, zip: &Path) -> Value {
	let output = std::process::Command::new("node")
		.current_dir(oracle)
		.arg("src/main.ts")
		.arg("dump")
		.arg(zip)
		.args(["--today", TODAY])
		.output()
		.expect("node runs");
	assert!(
		output.status.success(),
		"Actual couldn't read {}: {}",
		zip.display(),
		String::from_utf8_lossy(&output.stderr)
	);
	serde_json::from_slice(&output.stdout).unwrap()
}

/// Runs Actual's own handlers for `ops` on `zip` into `out`. Each run on a
/// zip another run made needs its own `seed`, or Actual hands out the same
/// ids again and overwrites rows.
fn actual_apply(oracle: &Path, zip: &Path, ops: Value, out: &Path, seed: u32) {
	let file = out.with_extension("ops.json");
	std::fs::write(&file, json!({"version": 1, "ops": ops}).to_string()).unwrap();
	let output = std::process::Command::new("node")
		.current_dir(oracle)
		.arg("src/main.ts")
		.arg("apply")
		.arg(zip)
		.arg(&file)
		.arg("--out")
		.arg(out)
		.args(["--today", TODAY, "--seed", &seed.to_string()])
		.output()
		.expect("node runs");
	assert!(
		output.status.success(),
		"Actual couldn't apply the ops: {}",
		String::from_utf8_lossy(&output.stderr)
	);
}

fn rows<'a>(dump: &'a Value, table: &str) -> &'a Vec<Value> {
	dump["tables"][table]["rows"].as_array().unwrap()
}

fn by_id<'a>(dump: &'a Value, table: &str) -> BTreeMap<String, &'a Value> {
	rows(dump, table)
		.iter()
		.map(|row| (row["id"].as_str().unwrap().to_owned(), row))
		.collect()
}

/// A transaction Actual counts: not deleted, and for a split line, its
/// parent isn't deleted either.
fn counted(transactions: &BTreeMap<String, &Value>, row: &Value) -> bool {
	if row["tombstone"] != 1 && row["isChild"] == 1 {
		let parent = row["parent_id"].as_str().unwrap();
		return transactions[parent]["tombstone"] != 1;
	}
	row["tombstone"] != 1
}

#[test]
fn a_month_of_account_work_reads_back_in_actual_with_the_same_numbers() {
	// R-ACC-1..4 as one session: open accounts, quick entry, a transfer edited
	// from its other side, a split with a line that pays an off-budget
	// account, a reconcile with an adjustment, an edit to a locked row, a
	// closed card and a duplicate. Then Actual imports the export.
	use NewAccountKind::{Checking, CreditCard, Investment, Savings};
	let scratch = Scratch::new("month");
	let session = import(&scratch, &small_zip(), "budget");
	let everyday = new_account(&session, "Joint checking", Checking, 250_000);
	let rainy = new_account(&session, "Rainy day", Savings, 0);
	let brokerage = new_account(&session, "Brokerage", Investment, 1_000_000);
	let card = new_account(&session, "Old card", CreditCard, 30_000);
	let groceries = category(&session, "Groceries");
	let utilities = category(&session, "Utilities");
	assert!(account(&session, &brokerage).off_budget);

	let mut shop = entry("shop", &everyday, "2026-11-02", -6_420);
	shop.payee_name = Some("corner grocer".to_owned());
	shop.category_id = Some(groceries.clone());
	shop.cleared = true;
	add(&session, shop.clone());

	let mut save = entry("save", &everyday, "2026-11-03", -20_000);
	save.payee_id = Some(transfer_payee(&session, &rainy));
	save.cleared = true;
	add(&session, save);
	let deposit = session
		.register_row("save".to_owned())
		.unwrap()
		.counterpart_id
		.unwrap();
	let mut from_savings = entry(&deposit, &rainy, "2026-11-03", 25_000);
	from_savings.payee_id = Some(transfer_payee(&session, &everyday));
	from_savings.notes = Some("november savings".to_owned());
	edit(&session, &deposit, from_savings);
	let save_row = session.register_row("save".to_owned()).unwrap();
	assert_eq!(
		(save_row.amount, save_row.notes.as_deref(), save_row.cleared),
		(-25_000, Some("november savings"), true),
		"editing the savings side drives the checking side and keeps its own cleared flag"
	);

	let mut receipt = entry("receipt", &everyday, "2026-11-06", -15_000);
	receipt.payee_name = Some("Warehouse".to_owned());
	let mut to_brokerage = line(-6_000, Some(&utilities));
	to_brokerage.payee_id = Some(transfer_payee(&session, &brokerage));
	receipt.splits = vec![line(-9_000, Some(&groceries)), to_brokerage];
	add(&session, receipt);
	let split = session.register_row("receipt".to_owned()).unwrap();
	assert_eq!(split.lines.len(), 2);
	assert_eq!(
		split.lines[1].transfer_account_id.as_deref(),
		Some(brokerage.as_str())
	);
	assert_eq!(
		split.lines[1].category_id.as_deref(),
		Some(utilities.as_str()),
		"a line that leaves the budget keeps its category"
	);

	let state = session.reconciliation(everyday.clone(), 218_480).unwrap();
	assert_eq!((state.cleared, state.difference), (218_580, -100));
	perform(
		&session,
		Command::AddReconcileAdjustment {
			account_id: everyday.clone(),
			statement_balance: 218_480,
		},
	);
	perform(
		&session,
		Command::FinishReconcile {
			account_id: everyday.clone(),
			statement_balance: 218_480,
		},
	);
	shop.notes = Some("milk and eggs".to_owned());
	edit(&session, "shop", shop);
	let shop_row = session.register_row("shop".to_owned()).unwrap();
	assert!(
		shop_row.reconciled && shop_row.cleared,
		"a notes edit leaves a locked row locked"
	);

	perform(
		&session,
		Command::CloseAccount {
			account_id: card.clone(),
			transfer_to_account_id: Some(everyday.clone()),
			category_id: None,
		},
	);
	perform(
		&session,
		Command::DuplicateTransaction {
			transaction_id: "shop".to_owned(),
			new_id: Some("shop-again".to_owned()),
		},
	);

	let ours = session.accounts().unwrap();
	let total_of = |id: &str| ours.accounts.iter().find(|a| a.id == id).unwrap().total;
	assert_eq!(
		total_of(&everyday),
		250_000 - 6_420 - 25_000 - 15_000 - 100 - 30_000 - 6_420
	);
	assert_eq!(total_of(&rainy), 25_000);
	assert_eq!(total_of(&brokerage), 1_006_000);
	assert_eq!(total_of(&card), 0);
	let reconciled: BTreeSet<String> = register(&session, in_account(&everyday))
		.into_iter()
		.filter(|row| row.reconciled)
		.map(|row| row.id)
		.collect();
	assert_eq!(
		reconciled.len(),
		4,
		"starting balance, groceries, the transfer and the adjustment"
	);
	let budget = session.budget_month(NOVEMBER, TODAY.to_owned()).unwrap();

	let zip = scratch.join("month.zip");
	export(&session, &zip);
	let Some(oracle) = oracle() else {
		return;
	};
	let actual = actual_reads(&oracle, &zip);
	let transactions = by_id(&actual, "transactions");
	for account in &ours.accounts {
		assert_eq!(
			actual["computed"]["accounts"][&account.id]["balance"].as_i64(),
			Some(account.total),
			"{} total",
			account.name
		);
		let cleared: i64 = transactions
			.values()
			.filter(|t| t["acct"] == account.id.as_str() && t["isParent"] != 1)
			.filter(|t| counted(&transactions, t) && t["cleared"] == 1)
			.map(|t| t["amount"].as_i64().unwrap())
			.sum();
		assert_eq!(cleared, account.cleared, "{} cleared", account.name);
	}
	let theirs: BTreeSet<String> = transactions
		.values()
		.filter(|t| t["acct"] == everyday.as_str() && t["isChild"] != 1)
		.filter(|t| counted(&transactions, t) && t["reconciled"] == 1)
		.map(|t| t["id"].as_str().unwrap().to_owned())
		.collect();
	assert_eq!(theirs, reconciled);

	let november = &actual["computed"]["months"]["2026-11"];
	assert_eq!(november["toBudget"].as_i64(), Some(budget.to_budget));
	let expenses = budget.groups.iter().filter(|g| !g.is_income);
	for envelope in expenses.flat_map(|g| &g.envelopes) {
		let theirs = &november["categories"][&envelope.id];
		assert_eq!(
			(theirs["spent"].as_i64(), theirs["balance"].as_i64()),
			(Some(envelope.spent), Some(envelope.available)),
			"{}",
			envelope.name
		);
	}

	let off_budget: BTreeSet<&str> = rows(&actual, "accounts")
		.iter()
		.filter(|a| a["offbudget"] == 1)
		.map(|a| a["id"].as_str().unwrap())
		.collect();
	let payees = by_id(&actual, "payees");
	for row in transactions.values().filter(|t| counted(&transactions, t)) {
		if off_budget.contains(row["acct"].as_str().unwrap()) {
			assert_eq!(row["category"], Value::Null, "off-budget row {}", row["id"]);
		}
		if row["isParent"] == 1 {
			let lines: i64 = transactions
				.values()
				.filter(|t| t["parent_id"] == row["id"] && t["tombstone"] != 1)
				.map(|t| t["amount"].as_i64().unwrap())
				.sum();
			assert_eq!(Some(lines), row["amount"].as_i64(), "split {}", row["id"]);
		}
		let Some(other) = row["transferred_id"].as_str() else {
			continue;
		};
		let other = transactions[other];
		assert!(
			counted(&transactions, other),
			"{} pairs with a live row",
			row["id"]
		);
		assert_eq!(other["transferred_id"], row["id"]);
		assert_eq!(other["amount"].as_i64(), row["amount"].as_i64().map(|a| -a));
		let payee = payees[row["description"].as_str().unwrap()];
		assert_eq!(payee["transfer_acct"], other["acct"], "{} payee", row["id"]);
	}
}

/// Each pair's two rows as Actual stores them, found by the source account
/// and the notes the source was given.
fn pair_view(dump: &Value, source_account: &str, notes: &str) -> (String, Value, Value) {
	let transactions = by_id(dump, "transactions");
	let source = transactions
		.values()
		.find(|t| t["acct"] == source_account && t["notes"] == notes && t["tombstone"] != 1)
		.unwrap_or_else(|| panic!("no live row in {source_account} with notes {notes}"));
	let other = transactions[source["transferred_id"].as_str().unwrap()];
	assert_eq!(other["transferred_id"], source["id"]);
	let view = |row: &Value| {
		json!({
			"acct": row["acct"], "amount": row["amount"], "description": row["description"],
			"notes": row["notes"], "cleared": row["cleared"], "reconciled": row["reconciled"],
			"category": row["category"], "isParent": row["isParent"], "isChild": row["isChild"],
			"tombstone": row["tombstone"], "schedule": row["schedule"],
		})
	};
	(
		source["id"].as_str().unwrap().to_owned(),
		view(source),
		view(other),
	)
}

#[test]
fn transfers_across_the_budget_line_are_written_as_actual_writes_them() {
	// §4.2's category rules, edge case 5's ground: on→off keeps the typed
	// category, off→off and off→on rows never hold one. Both sides start
	// from the same export, add the same three transfers, then edit them.
	// A fourth runs off→on, the user categorizes its on-budget side, then
	// moves the side typed in onto the budget: on→on clears both categories.
	let scratch = Scratch::new("budget-line");
	let session = import(&scratch, &small_zip(), "base");
	let everyday = new_account(&session, "Joint checking", NewAccountKind::Checking, 0);
	let brokerage = new_account(&session, "Brokerage", NewAccountKind::Investment, 0);
	let loan = new_account(&session, "Car loan", NewAccountKind::Loan, 0);
	let rainy = new_account(&session, "Rainy day", NewAccountKind::Savings, 0);
	let groceries = category(&session, "Groceries");
	let base = scratch.join("base.zip");
	export(&session, &base);
	session.close().unwrap();
	let pairs = [
		(everyday.clone(), brokerage.clone()),
		(brokerage.clone(), loan.clone()),
		(loan.clone(), everyday.clone()),
	];

	let veritas = import(&scratch, &base, "veritas");
	for (n, (from, to)) in pairs.iter().enumerate() {
		let mut draft = entry(&format!("pair-{n}"), from, "2026-11-10", -10_001 - n as i64);
		draft.payee_id = Some(transfer_payee(&veritas, to));
		draft.category_id = Some(groceries.clone());
		draft.notes = Some(format!("pair {n}"));
		draft.cleared = true;
		add(&veritas, draft.clone());
		draft.amount = -12_001 - n as i64;
		draft.notes = Some(format!("pair {n} edited"));
		edit(&veritas, &format!("pair-{n}"), draft);
	}
	let mut moving = entry("pair-3", &brokerage, "2026-11-10", -10_004);
	moving.payee_id = Some(transfer_payee(&veritas, &everyday));
	moving.notes = Some("pair 3".to_owned());
	moving.cleared = true;
	add(&veritas, moving.clone());
	let landed = veritas
		.register_row("pair-3".to_owned())
		.unwrap()
		.counterpart_id
		.unwrap();
	let mut categorized = entry(&landed, &everyday, "2026-11-10", 10_004);
	categorized.payee_id = Some(transfer_payee(&veritas, &brokerage));
	categorized.category_id = Some(groceries.clone());
	categorized.notes = Some("pair 3".to_owned());
	edit(&veritas, &landed, categorized);
	assert_eq!(
		veritas
			.register_row(landed.clone())
			.unwrap()
			.category_id
			.as_deref(),
		Some(groceries.as_str()),
		"off→on: the on-budget side takes a category"
	);
	moving.account_id = rainy.clone();
	moving.amount = -12_004;
	moving.notes = Some("pair 3 edited".to_owned());
	edit(&veritas, "pair-3", moving);
	assert_eq!(
		veritas
			.register_row("pair-0".to_owned())
			.unwrap()
			.category_id
			.as_deref(),
		Some(groceries.as_str())
	);
	let ours_zip = scratch.join("veritas.zip");
	export(&veritas, &ours_zip);
	let Some(oracle) = oracle() else {
		return;
	};
	let ours = actual_reads(&oracle, &ours_zip);

	let added = scratch.join("actual-added.zip");
	let adds: Vec<Value> = pairs
		.iter()
		.enumerate()
		.map(|(n, (from, to))| {
			json!({"op": "addTransactions", "account": from, "transactions": [{
				"date": "2026-11-10", "amount": -10_001 - n as i64, "payee": {"transferTo": to},
				"category": groceries, "notes": format!("pair {n}"), "cleared": true,
			}]})
		})
		.chain([
			json!({"op": "addTransactions", "account": brokerage, "transactions": [{
				"date": "2026-11-10", "amount": -10_004, "payee": {"transferTo": everyday},
				"notes": "pair 3",
			}]}),
		])
		.collect();
	actual_apply(&oracle, &base, Value::Array(adds), &added, 1);
	let after_add = actual_reads(&oracle, &added);
	let (moving, _, _) = pair_view(&after_add, &brokerage, "pair 3");
	let edits: Vec<Value> = pairs
		.iter()
		.enumerate()
		.map(|(n, (from, _))| {
			let (id, _, _) = pair_view(&after_add, from, &format!("pair {n}"));
			json!({"op": "updateTransaction", "id": id,
				"fields": {"amount": -12_001 - n as i64, "notes": format!("pair {n} edited")}})
		})
		.chain([
			json!({"op": "updateTransaction", "id": {"transferOf": moving},
				"fields": {"category": groceries}}),
			json!({"op": "updateTransaction", "id": moving,
				"fields": {"account": rainy, "amount": -12_004, "notes": "pair 3 edited"}}),
		])
		.collect();
	let edited = scratch.join("actual-edited.zip");
	actual_apply(&oracle, &added, Value::Array(edits), &edited, 2);
	let theirs = actual_reads(&oracle, &edited);

	let typed_in = pairs
		.iter()
		.map(|(from, _)| from)
		.chain([&rainy])
		.enumerate();
	for (n, from) in typed_in {
		let notes = format!("pair {n} edited");
		let (_, our_source, our_other) = pair_view(&ours, from, &notes);
		let (_, their_source, their_other) = pair_view(&theirs, from, &notes);
		assert_eq!(our_source, their_source, "pair {n}, the side typed in");
		assert_eq!(our_other, their_other, "pair {n}, the other side");
	}
	let (_, moved, moved_other) = pair_view(&ours, &rainy, "pair 3 edited");
	assert_eq!(
		(&moved["category"], &moved_other["category"]),
		(&Value::Null, &Value::Null),
		"on→on clears both sides"
	);
	for account in [&everyday, &brokerage, &loan, &rainy] {
		assert_eq!(
			ours["computed"]["accounts"][account],
			theirs["computed"]["accounts"][account]
		);
	}
	assert_eq!(
		ours["computed"]["months"]["2026-11"],
		theirs["computed"]["months"]["2026-11"]
	);
}

#[test]
fn an_imported_split_with_a_deleted_line_keeps_that_line_deleted() {
	// A split Actual wrote after the user removed one of its lines: the
	// removed line is a tombstoned child. It must not count, show, come back
	// on an edit or a duplicate, or change what Actual reads.
	let scratch = Scratch::new("deleted-line");
	let session = import(&scratch, &small_zip(), "budget");
	let everyday = new_account(&session, "Joint checking", NewAccountKind::Checking, 10_000);
	let groceries = category(&session, "Groceries");
	let utilities = category(&session, "Utilities");
	session.close().unwrap();
	inject(
		&scratch,
		"budget",
		&format!(
			"INSERT INTO transactions (id, acct, date, amount, isParent, isChild, cleared, sort_order, tombstone)
			 VALUES ('bill', '{everyday}', 20261104, -3000, 1, 0, 0, 5, 0);
			 INSERT INTO transactions (id, acct, date, amount, isParent, isChild, parent_id, category, cleared, sort_order, tombstone)
			 VALUES ('bill-1', '{everyday}', 20261104, -2000, 0, 1, 'bill', '{groceries}', 0, -1, 0),
			        ('bill-2', '{everyday}', 20261104, -1000, 0, 1, 'bill', '{utilities}', 0, -2, 0),
			        ('bill-gone', '{everyday}', 20261104, -700, 0, 1, 'bill', '{utilities}', 0, -3, 1);"
		),
	);
	let session = reopen(&scratch, "budget");
	let shown = session.register_row("bill".to_owned()).unwrap();
	assert_eq!(
		shown
			.lines
			.iter()
			.map(|l| l.id.as_str())
			.collect::<Vec<_>>(),
		["bill-1", "bill-2"]
	);
	assert_eq!(shown.split_difference, None);
	assert_eq!(account(&session, &everyday).total, 7_000);

	let mut edited = entry("bill", &everyday, "2026-11-04", -3000);
	edited.splits = vec![
		SplitLineDraft {
			id: Some("bill-1".to_owned()),
			..line(-1800, Some(&groceries))
		},
		SplitLineDraft {
			id: Some("bill-2".to_owned()),
			..line(-1000, Some(&utilities))
		},
		line(-200, Some(&utilities)),
	];
	let mut reviving = edited.clone();
	reviving.splits[2].id = Some("bill-gone".to_owned());
	assert!(
		session
			.perform(Command::EditTransaction {
				transaction_id: "bill".to_owned(),
				transaction: reviving,
			})
			.is_err(),
		"a deleted line can't be edited back to life"
	);
	edit(&session, "bill", edited);
	perform(
		&session,
		Command::DuplicateTransaction {
			transaction_id: "bill".to_owned(),
			new_id: Some("bill-copy".to_owned()),
		},
	);
	let copy = session.register_row("bill-copy".to_owned()).unwrap();
	assert_eq!(
		copy.lines.iter().map(|l| l.amount).collect::<Vec<_>>(),
		[-1800, -1000, -200]
	);
	assert_eq!(account(&session, &everyday).total, 4_000);
	let budget = session.budget_month(NOVEMBER, TODAY.to_owned()).unwrap();
	let spent = |id: &str| {
		budget
			.groups
			.iter()
			.flat_map(|g| &g.envelopes)
			.find(|e| e.id == id)
			.unwrap()
			.spent
	};
	let zip = scratch.join("bill.zip");
	export(&session, &zip);
	session.close().unwrap();
	let gone: (i64, Option<String>) = sqlite(&scratch, "budget")
		.query_row(
			"SELECT tombstone, parent_id FROM transactions WHERE id = 'bill-gone'",
			[],
			|r| Ok((r.get(0)?, r.get(1)?)),
		)
		.unwrap();
	assert_eq!(gone, (1, Some("bill".to_owned())));

	let Some(oracle) = oracle() else {
		return;
	};
	let actual = actual_reads(&oracle, &zip);
	assert_eq!(
		actual["computed"]["accounts"][&everyday]["balance"].as_i64(),
		Some(4_000)
	);
	let november = &actual["computed"]["months"]["2026-11"]["categories"];
	for id in [&groceries, &utilities] {
		assert_eq!(november[id]["spent"].as_i64(), Some(spent(id)));
	}
}

/// Everything the Accounts screens show: the account list, every register
/// row and every payee.
fn screens(session: &Session) -> (veritas_ffi::AccountList, Vec<RegisterRow>, Vec<PayeeDetail>) {
	(
		session.accounts().unwrap(),
		register(session, RegisterScope::All),
		session.payees().unwrap(),
	)
}

#[test]
fn undo_walks_a_session_of_account_work_back_step_by_step_and_redo_replays_it() {
	// R-REL-5 over the Accounts commands: each one is a single undo step, so
	// undoing all of them shows the screens exactly as they opened, and redo
	// brings back exactly what the session made.
	use NewAccountKind::{Checking, CreditCard, Savings};
	let scratch = Scratch::new("undo");
	let session = import(&scratch, &small_zip(), "budget");
	let groceries = category(&session, "Groceries");
	let start = screens(&session);

	let mut steps = 0;
	let mut step = |command: Command| {
		perform(&session, command);
		steps += 1;
	};
	let create = |name: &str, kind, starting_balance| Command::CreateAccount {
		account: NewAccount {
			name: name.to_owned(),
			kind,
			starting_balance,
		},
	};
	step(create("Joint checking", Checking, 120_000));
	step(create("Rainy day", Savings, 0));
	step(create("Old card", CreditCard, 4_000));
	let ids: BTreeMap<String, String> = session
		.accounts()
		.unwrap()
		.accounts
		.into_iter()
		.map(|a| (a.name, a.id))
		.collect();
	let (everyday, rainy, card) = (
		ids["Joint checking"].clone(),
		ids["Rainy day"].clone(),
		ids["Old card"].clone(),
	);
	let mut save = entry("save", &everyday, "2026-11-03", -20_000);
	save.payee_id = Some(transfer_payee(&session, &rainy));
	step(Command::AddTransaction { transaction: save });
	let mut shop = entry("shop", &everyday, "2026-11-04", -3_300);
	shop.payee_name = Some("Corner Cafe".to_owned());
	shop.splits = vec![line(-3_000, Some(&groceries)), line(-300, None)];
	step(Command::AddTransaction {
		transaction: shop.clone(),
	});
	shop.amount = -3_500;
	shop.splits = vec![line(-3_500, Some(&groceries))];
	step(Command::EditTransaction {
		transaction_id: "shop".to_owned(),
		transaction: shop,
	});
	step(Command::SetCleared {
		transaction_ids: vec!["save".to_owned(), "shop".to_owned()],
		cleared: true,
	});
	step(Command::AddReconcileAdjustment {
		account_id: everyday.clone(),
		statement_balance: 96_400,
	});
	step(Command::FinishReconcile {
		account_id: everyday.clone(),
		statement_balance: 96_400,
	});
	let cafe = session
		.register_row("shop".to_owned())
		.unwrap()
		.payee_id
		.unwrap();
	let grocer = session
		.payees()
		.unwrap()
		.into_iter()
		.find(|p| p.name == "Corner Grocer")
		.unwrap()
		.id;
	step(Command::RenamePayee {
		payee_id: cafe.clone(),
		name: "The Corner Cafe".to_owned(),
	});
	step(Command::SetPayeeFavorite {
		payee_id: grocer.clone(),
		favorite: true,
	});
	step(Command::MergePayees {
		target_payee_id: grocer,
		payee_ids: vec![cafe],
	});
	step(Command::CreateAccountGroup {
		name: "Cards".to_owned(),
	});
	step(Command::SetAccountNote {
		account_id: card.clone(),
		text: "Cancelled in October".to_owned(),
	});
	step(Command::MoveAccount {
		account_id: rainy.clone(),
		before_account_id: Some(everyday.clone()),
	});
	step(Command::CloseAccount {
		account_id: card,
		transfer_to_account_id: Some(everyday.clone()),
		category_id: None,
	});
	step(Command::DuplicateTransaction {
		transaction_id: "save".to_owned(),
		new_id: Some("save-again".to_owned()),
	});
	step(Command::DeleteTransaction {
		transaction_id: "save".to_owned(),
	});
	let end = screens(&session);
	assert_ne!(end, start);
	assert_eq!(
		account(&session, &rainy).total,
		20_000,
		"the deleted transfer took its pair with it, the duplicate made its own"
	);

	for _ in 0..steps {
		session.undo().unwrap();
	}
	assert_eq!(screens(&session), start, "after undoing all {steps} steps");
	for _ in 0..steps {
		session.redo().unwrap();
	}
	assert_eq!(screens(&session), end, "after redoing them");
	session.close().unwrap();
	let reopened = reopen(&scratch, "budget");
	assert_eq!(screens(&reopened), end, "and on disk");
}

#[test]
fn register_pages_add_up_to_the_whole_register_and_follow_every_commit() {
	// R-PERF-3's paging: the screen asks for one page at a time, so pages
	// must tile the register with no row lost or repeated, carry the right
	// running balance, and never serve a page from before the last commit,
	// undo or redo.
	let scratch = Scratch::new("paging");
	let session = import(&scratch, &small_zip(), "budget");
	let everyday = new_account(&session, "Joint checking", NewAccountKind::Checking, 50_000);
	for n in 0..23 {
		let day = 1 + n % 6;
		let mut draft = entry(
			&format!("row-{n}"),
			&everyday,
			&format!("2026-11-{day:02}"),
			-100 - n,
		);
		draft.cleared = n % 2 == 0;
		add(&session, draft);
	}
	let scope = in_account(&everyday);
	let page = |offset: u64, limit: u64| {
		session
			.register(
				scope.clone(),
				RegisterFilter::default(),
				NEWEST_FIRST,
				offset,
				limit,
			)
			.unwrap()
	};
	let whole = page(0, 1_000);
	assert_eq!(whole.total, 24);
	let mut tiled = Vec::new();
	for offset in (0..30).step_by(5) {
		let next = page(offset, 5);
		assert_eq!(next.total, 24);
		tiled.extend(next.rows);
	}
	assert_eq!(tiled, whole.rows);
	assert_eq!(
		whole.rows[0].running_balance,
		Some(account(&session, &everyday).total)
	);
	for pair in whole.rows.windows(2) {
		assert_eq!(
			pair[0].running_balance.unwrap() - pair[0].amount,
			pair[1].running_balance.unwrap()
		);
	}

	let top_before = page(0, 3).rows;
	let mut late = entry("late", &everyday, "2026-11-30", -7_777);
	late.notes = Some("late fee".to_owned());
	add(&session, late);
	let after_add = page(0, 3);
	assert_eq!(after_add.total, 25);
	assert_eq!(after_add.rows[0].id, "late");
	assert_eq!(
		after_add.rows[0].running_balance,
		Some(account(&session, &everyday).total)
	);
	session.undo().unwrap();
	assert_eq!(page(0, 3).rows, top_before, "undo is seen by the next page");
	session.redo().unwrap();
	assert_eq!(page(0, 3).rows[0].id, "late", "and so is redo");

	let mut cheaper = entry("row-0", &everyday, "2026-11-01", -50);
	cheaper.cleared = true;
	edit(&session, "row-0", cheaper);
	let edited = page(0, 1_000);
	assert_eq!(
		edited.rows[0].running_balance,
		Some(account(&session, &everyday).total)
	);
	let row = edited.rows.iter().find(|r| r.id == "row-0").unwrap();
	assert_eq!(row.amount, -50);
}

#[test]
fn closing_an_account_a_bill_pays_into_leaves_the_bill_as_actual_does() {
	// §4.9: a close writes `closed` and the closing transfer, nothing else.
	// A monthly savings transfer scheduled into the account stays exactly
	// as Actual keeps it when Actual closes the same account.
	let scratch = Scratch::new("close-scheduled");
	let session = import(&scratch, &small_zip(), "base");
	let everyday = new_account(&session, "Joint checking", NewAccountKind::Checking, 50_000);
	let rainy = new_account(&session, "Rainy day", NewAccountKind::Savings, 8_000);
	let into_rainy = transfer_payee(&session, &rainy);
	let base = scratch.join("base.zip");
	export(&session, &base);
	session.close().unwrap();
	let Some(oracle) = oracle() else {
		return;
	};
	let scheduled = scratch.join("scheduled.zip");
	actual_apply(
		&oracle,
		&base,
		json!([{"op": "createSchedule", "id": "monthly-save", "name": "Monthly savings",
			"payee": into_rainy, "account": everyday, "amount": -10_000, "amountOp": "is",
			"date": {"start": "2026-12-01", "frequency": "monthly", "interval": 1, "patterns": [],
				"skipWeekend": false, "weekendSolveMode": "after", "endMode": "never"}}]),
		&scheduled,
		21,
	);

	let veritas = import(&scratch, &scheduled, "veritas");
	perform(
		&veritas,
		Command::CloseAccount {
			account_id: rainy.clone(),
			transfer_to_account_id: Some(everyday.clone()),
			category_id: None,
		},
	);
	assert!(account(&veritas, &rainy).closed);
	let ours_zip = scratch.join("veritas.zip");
	export(&veritas, &ours_zip);
	let ours = actual_reads(&oracle, &ours_zip);

	let closed = scratch.join("actual-closed.zip");
	actual_apply(
		&oracle,
		&scheduled,
		json!([{"op": "closeAccount", "id": rainy, "transferAccount": everyday}]),
		&closed,
		22,
	);
	let theirs = actual_reads(&oracle, &closed);
	assert_eq!(ours["tables"]["schedules"], theirs["tables"]["schedules"]);
	assert_eq!(ours["tables"]["rules"], theirs["tables"]["rules"]);
	assert_eq!(
		ours["computed"]["schedules"],
		theirs["computed"]["schedules"]
	);
	assert_eq!(ours["computed"]["accounts"], theirs["computed"]["accounts"]);
	let (_, our_close, our_deposit) = pair_view(&ours, &rainy, "Closing account");
	let (_, their_close, their_deposit) = pair_view(&theirs, &rainy, "Closing account");
	assert_eq!(our_close, their_close, "the closing transfer");
	assert_eq!(our_deposit, their_deposit, "its other side");
	let closed_flag = |dump: &Value| by_id(dump, "accounts")[&rainy]["closed"].clone();
	assert_eq!(closed_flag(&ours), json!(1));
	assert_eq!(closed_flag(&ours), closed_flag(&theirs));
}
