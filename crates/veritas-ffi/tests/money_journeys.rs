#![allow(
	clippy::unwrap_used,
	clippy::expect_used,
	clippy::panic,
	reason = "integration tests fail by panicking"
)]

//! Moving money, loans and category kinds (R-XFER-1/3/4, R-DEBT-1/2,
//! docs/finance/debt.md DEBT-1..6, classification.md CLS-3.3, CLS-4.1)
//! driven through the exported API the way Swift drives it, with Actual's
//! own reading of the export as the verdict wherever the oracle is installed.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::Thread;

use serde_json::Value;
use veritas_ffi::{
	CancelToken, Change, Command, EntityKind, Month, NewAccount, NewAccountKind, RegisterRow,
	Session, SplitLineDraft, TransactionDraft,
};

const TODAY: &str = "2026-11-15";
const NOVEMBER: Month = Month {
	year: 2026,
	month: 11,
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
			"ffi-money-{label}-{}-{}",
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

fn text(path: &Path) -> String {
	path.display().to_string()
}

fn import(scratch: &Scratch) -> Arc<Session> {
	let zip = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/small.zip");
	let session = block_on(Session::import_actual_zip(
		text(&zip),
		text(&scratch.join("budget")),
		CancelToken::new(),
	))
	.unwrap();
	session.budget_month(NOVEMBER, TODAY.to_owned()).unwrap();
	session
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

fn new_account(session: &Session, name: &str, kind: NewAccountKind, starting: i64) -> String {
	let change = perform(
		session,
		Command::CreateAccount {
			account: NewAccount {
				name: name.to_owned(),
				kind,
				starting_balance: starting,
				loan: None,
			},
		},
	);
	created(&change, EntityKind::Account)
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

fn shown(session: &Session, id: &str) -> RegisterRow {
	session.register_row(id.to_owned()).unwrap()
}

fn gone(session: &Session, id: &str) -> bool {
	session.register_row(id.to_owned()).is_err()
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

fn by_id<'a>(dump: &'a Value, table: &str) -> BTreeMap<String, &'a Value> {
	dump["tables"][table]["rows"]
		.as_array()
		.unwrap()
		.iter()
		.map(|row| (row["id"].as_str().unwrap().to_owned(), row))
		.collect()
}

#[test]
fn editing_either_side_of_a_transfer_moves_both_dates_and_keeps_each_cleared_flag() {
	// R-XFER-3: Veritas syncs the date, unlike Actual's server. `cleared` stays
	// each side's own, as Actual's `updateTransfer` leaves it
	// (transfer.ts:120-138) and R-XFER-3 doesn't list it.
	use NewAccountKind::{Checking, Savings};
	let scratch = Scratch::new("dates");
	let session = import(&scratch);
	let everyday = new_account(&session, "Joint checking", Checking, 100_000);
	let rainy = new_account(&session, "Rainy day", Savings, 0);

	let mut save = entry("save", &everyday, "2026-11-03", -20_000);
	save.payee_id = Some(transfer_payee(&session, &rainy));
	save.cleared = true;
	add(&session, save.clone());
	let deposit = shown(&session, "save").counterpart_id.unwrap();
	assert_eq!(shown(&session, &deposit).date, "2026-11-03");

	save.date = "2026-11-05".to_owned();
	edit(&session, "save", save);
	let other = shown(&session, &deposit);
	assert_eq!(other.date, "2026-11-05", "the source drives the date");
	assert!(!other.cleared, "the other side keeps its own cleared flag");

	let mut from_savings = entry(&deposit, &rainy, "2026-11-08", 20_000);
	from_savings.payee_id = Some(transfer_payee(&session, &everyday));
	edit(&session, &deposit, from_savings);
	let source = shown(&session, "save");
	assert_eq!(source.date, "2026-11-08", "the other side drives it too");
	assert!(source.cleared);

	session.undo().unwrap();
	assert_eq!(shown(&session, "save").date, "2026-11-05");
}

/// A $400 split payment from checking: the principal line transfers to the
/// loan, the interest line is an expense (DEBT-6, rows for L-1).
fn split_payment(session: &Session, checking: &str, loan: &str, date: &str) -> String {
	let principal = category(session, "Groceries");
	let interest = category(session, "Utilities");
	let mut payment = entry("payment", checking, date, -40_000);
	payment.payee_name = Some("Credit union".to_owned());
	let mut to_loan = line(-32_000, Some(&principal));
	to_loan.payee_id = Some(transfer_payee(session, loan));
	payment.splits = vec![to_loan, line(-8_000, Some(&interest))];
	add(session, payment)
}

#[test]
fn moving_the_loan_side_of_a_split_payment_moves_the_whole_payment() {
	// R-XFER-3 date sync when the other side is a split line: the line, its
	// parent and its sibling lines move together, as Actual's client moves
	// the line and its parent (TransactionsTable.tsx:1286-1327), so the
	// payment never straddles two dates.
	use NewAccountKind::{Checking, Loan};
	let scratch = Scratch::new("split-date");
	let session = import(&scratch);
	let checking = new_account(&session, "Joint checking", Checking, 500_000);
	let loan = new_account(&session, "Car loan", Loan, 980_000);
	let payment = split_payment(&session, &checking, &loan, "2026-11-03");
	let split = shown(&session, &payment);
	let loan_side = split.lines[0].counterpart_id.clone().unwrap();

	let mut moved = entry(&loan_side, &loan, "2026-11-09", 32_000);
	moved.payee_id = Some(transfer_payee(&session, &checking));
	edit(&session, &loan_side, moved);
	let split = shown(&session, &payment);
	assert_eq!(split.date, "2026-11-09");
	assert_eq!(split.split_difference, None, "the payment still adds up");
	let history = session
		.register(
			veritas_ffi::RegisterScope::Account {
				account_id: checking.clone(),
			},
			veritas_ffi::RegisterFilter {
				from: Some("2026-11-09".to_owned()),
				to: Some("2026-11-09".to_owned()),
				..Default::default()
			},
			veritas_ffi::RegisterSort {
				key: veritas_ffi::RegisterSortKey::Date,
				ascending: false,
			},
			0,
			10,
		)
		.unwrap();
	assert_eq!(history.rows.len(), 1);
	assert_eq!(history.rows[0].lines.len(), 2);
}

#[test]
fn deleting_the_loan_side_of_a_payment_deletes_the_whole_payment() {
	// DEBT-6 A.5's recommendation, after the shell confirms: the loan-side
	// row's `counterpartParentId` tells the shell the delete takes the whole
	// payment, and the delete tombstones the parent, every line and the
	// counterpart in one step. From the checking side it already does (A.2).
	use NewAccountKind::{Checking, Loan};
	let scratch = Scratch::new("delete-loan-side");
	let session = import(&scratch);
	let checking = new_account(&session, "Joint checking", Checking, 500_000);
	let loan = new_account(&session, "Car loan", Loan, 980_000);
	let payment = split_payment(&session, &checking, &loan, "2026-11-03");
	let split = shown(&session, &payment);
	let lines: Vec<String> = split.lines.iter().map(|l| l.id.clone()).collect();
	let loan_side = split.lines[0].counterpart_id.clone().unwrap();
	let loan_row = shown(&session, &loan_side);
	assert_eq!(loan_row.counterpart_id.as_deref(), Some(lines[0].as_str()));
	assert_eq!(
		loan_row.counterpart_parent_id.as_deref(),
		Some(payment.as_str())
	);
	assert_eq!(split.counterpart_parent_id, None);

	perform(
		&session,
		Command::DeleteTransaction {
			transaction_id: loan_side.clone(),
		},
	);
	assert!(gone(&session, &payment) && gone(&session, &loan_side));
	for line in &lines {
		assert!(gone(&session, line));
	}
	let total = |id: &str| {
		session
			.accounts()
			.unwrap()
			.accounts
			.into_iter()
			.find(|a| a.id == id)
			.unwrap()
			.total
	};
	assert_eq!((total(&checking), total(&loan)), (500_000, -980_000));

	session.undo().unwrap();
	assert_eq!(shown(&session, &payment).lines.len(), 2);
	assert_eq!((total(&checking), total(&loan)), (460_000, -948_000));

	let zip = scratch.join("deleted.zip");
	session.undo().unwrap();
	session.redo().unwrap();
	perform(
		&session,
		Command::DeleteTransaction {
			transaction_id: loan_side,
		},
	);
	block_on(session.export_actual_zip(text(&zip), CancelToken::new())).unwrap();
	let Some(oracle) = oracle() else {
		return;
	};
	let actual = actual_reads(&oracle, &zip);
	let transactions = by_id(&actual, "transactions");
	assert_eq!(transactions[&payment]["tombstone"], 1);
	for line in &lines {
		assert_eq!(transactions[line]["tombstone"], 1);
	}
	assert_eq!(
		actual["computed"]["accounts"][&loan]["balance"].as_i64(),
		Some(-980_000)
	);
}

fn total(session: &Session, id: &str) -> i64 {
	session
		.accounts()
		.unwrap()
		.accounts
		.into_iter()
		.find(|a| a.id == id)
		.unwrap()
		.total
}

fn refused(result: Result<Change, veritas_ffi::FfiError>) -> bool {
	matches!(result, Err(veritas_ffi::FfiError::Invalid { .. }))
}

#[test]
fn move_money_shows_both_balances_and_asks_only_when_money_leaves_the_budget() {
	// R-XFER-1, R-XFER-4 (off → on doesn't ask), ST-9.1 for the loan.
	use NewAccountKind::{Checking, Investment, Loan, Savings};
	use veritas_ffi::TransferPurpose::{Ask, None as NoAsk};
	let scratch = Scratch::new("preview");
	let session = import(&scratch);
	let checking = new_account(&session, "Joint checking", Checking, 250_000);
	let rainy = new_account(&session, "Rainy day", Savings, 10_000);
	let brokerage = new_account(&session, "Brokerage", Investment, 1_000_000);
	let loan = new_account(&session, "Car loan", Loan, 980_000);

	let to_savings = session
		.transfer_preview(checking.clone(), rainy.clone(), 20_000)
		.unwrap();
	assert_eq!(
		(to_savings.from.now, to_savings.from.after),
		(total(&session, &checking), 230_000)
	);
	assert_eq!((to_savings.to.now, to_savings.to.after), (10_000, 30_000));
	assert_eq!((to_savings.purpose, to_savings.to_loan), (NoAsk, false));

	let invest = session
		.transfer_preview(checking.clone(), brokerage.clone(), 5_000)
		.unwrap();
	assert_eq!((invest.purpose, invest.to_loan), (Ask, false));
	let pay_loan = session
		.transfer_preview(checking.clone(), loan.clone(), 40_000)
		.unwrap();
	assert_eq!((pay_loan.purpose, pay_loan.to_loan), (Ask, true));
	assert_eq!((pay_loan.to.now, pay_loan.to.after), (-980_000, -940_000));
	let cash_out = session
		.transfer_preview(brokerage.clone(), checking.clone(), 5_000)
		.unwrap();
	assert_eq!((cash_out.purpose, cash_out.to_loan), (NoAsk, false));

	for amount in [0, -5] {
		assert!(
			session
				.transfer_preview(checking.clone(), rainy.clone(), amount)
				.is_err()
		);
	}
	assert!(
		session
			.transfer_preview(checking.clone(), checking.clone(), 5)
			.is_err()
	);
}

#[test]
fn a_kind_change_is_counted_before_it_reclassifies_history() {
	// CLS-3.3's example in cents: a -20000 transfer to an off-budget account
	// and a -5000 purchase, both in one expense category, set to savings:
	// "2 rows will change". CLS-4.1: kindTarget, then createKindCategory.
	use NewAccountKind::{Checking, Investment};
	use veritas_ffi::{CategoryKind, KindTarget};
	let scratch = Scratch::new("kinds");
	let session = import(&scratch);
	let checking = new_account(&session, "Joint checking", Checking, 250_000);
	let brokerage = new_account(&session, "Brokerage", Investment, 0);
	let expenses = session
		.budget_month(NOVEMBER, TODAY.to_owned())
		.unwrap()
		.groups
		.into_iter()
		.find(|g| !g.is_income)
		.unwrap()
		.id;
	let vacation = created(
		&perform(
			&session,
			Command::CreateCategory {
				group_id: expenses,
				name: "Vacation".to_owned(),
			},
		),
		EntityKind::Category,
	);

	let mut moved = entry("moved", &checking, "2026-11-02", -20_000);
	moved.payee_id = Some(transfer_payee(&session, &brokerage));
	moved.category_id = Some(vacation.clone());
	add(&session, moved);
	let mut bought = entry("bought", &checking, "2026-11-04", -5_000);
	bought.payee_name = Some("Airline".to_owned());
	bought.category_id = Some(vacation.clone());
	add(&session, bought);

	assert_eq!(
		session
			.kind_change_preview(vacation.clone(), CategoryKind::Savings)
			.unwrap(),
		2
	);
	assert_eq!(
		session
			.kind_change_preview(vacation.clone(), CategoryKind::Expense)
			.unwrap(),
		0
	);
	assert!(
		session
			.kind_change_preview(vacation.clone(), CategoryKind::Income)
			.is_err(),
		"CLS-3.1: a kind never crosses Actual's income flag"
	);
	perform(
		&session,
		Command::SetCategoryKind {
			category_id: vacation.clone(),
			kind: CategoryKind::Savings,
		},
	);
	assert_eq!(
		session.kind_target(CategoryKind::Savings).unwrap(),
		KindTarget::One {
			category_id: vacation.clone()
		}
	);
	assert_eq!(
		session
			.kind_change_preview(vacation.clone(), CategoryKind::Expense)
			.unwrap(),
		2
	);
	assert!(session.kind_target(CategoryKind::Expense).is_err());

	assert_eq!(
		session.kind_target(CategoryKind::DebtPrincipal).unwrap(),
		KindTarget::None
	);
	let groups_before = session
		.budget_month(NOVEMBER, TODAY.to_owned())
		.unwrap()
		.groups
		.len();
	let change = perform(
		&session,
		Command::CreateKindCategory {
			kind: CategoryKind::DebtPrincipal,
			name: "Loan Principal".to_owned(),
			unhide_group: false,
		},
	);
	let principal = created(&change, EntityKind::Category);
	assert_eq!(
		session.kind_target(CategoryKind::DebtPrincipal).unwrap(),
		KindTarget::One {
			category_id: principal.clone()
		}
	);
	let month = session.budget_month(NOVEMBER, TODAY.to_owned()).unwrap();
	let debt = month
		.groups
		.iter()
		.find(|g| g.envelopes.iter().any(|e| e.id == principal))
		.unwrap();
	assert_eq!(debt.name, "Debt");
	assert_eq!(month.groups.len(), groups_before + 1);
	assert!(refused(session.perform(Command::CreateKindCategory {
		kind: CategoryKind::DebtPrincipal,
		name: "Another".to_owned(),
		unhide_group: false,
	})));
	session.undo().unwrap();
	assert_eq!(
		session
			.budget_month(NOVEMBER, TODAY.to_owned())
			.unwrap()
			.groups
			.len(),
		groups_before,
		"one undo step takes the group and the category"
	);
}

#[test]
fn rates_read_as_basis_points() {
	use veritas_ffi::{NumberStyle, evaluate_rate};
	for typed in ["9.8", "9.80", "9.80%", " 9.80 % "] {
		assert_eq!(
			evaluate_rate(typed.to_owned(), NumberStyle::CommaDot).unwrap(),
			980
		);
	}
	assert_eq!(
		evaluate_rate("9,8".to_owned(), NumberStyle::DotComma).unwrap(),
		980
	);
	assert_eq!(
		evaluate_rate("0".to_owned(), NumberStyle::CommaDot).unwrap(),
		0
	);
	assert!(evaluate_rate("-1".to_owned(), NumberStyle::CommaDot).is_err());
	assert!(evaluate_rate("lots".to_owned(), NumberStyle::CommaDot).is_err());
}

fn terms(
	rate: Option<i64>,
	payment: Option<i64>,
	day: Option<u8>,
	lender: Option<&str>,
) -> veritas_ffi::LoanTerms {
	veritas_ffi::LoanTerms {
		rate_bps: rate,
		payment,
		payment_day: day,
		lender: lender.map(str::to_owned),
	}
}

fn new_loan(session: &Session, name: &str, owed: i64, loan: veritas_ffi::LoanTerms) -> String {
	let change = perform(
		session,
		Command::CreateAccount {
			account: NewAccount {
				name: name.to_owned(),
				kind: NewAccountKind::Loan,
				starting_balance: owed,
				loan: Some(loan),
			},
		},
	);
	created(&change, EntityKind::Account)
}

#[test]
fn a_loan_keeps_its_terms_and_only_loans_have_them() {
	// R-DEBT-1, DEBT-1, ST-9.1.
	let scratch = Scratch::new("terms");
	let session = import(&scratch);
	let checking = new_account(&session, "Joint checking", NewAccountKind::Checking, 0);
	let loan = new_loan(
		&session,
		"Car loan",
		980_000,
		terms(Some(980), Some(40_000), Some(15), Some(" Credit union ")),
	);
	assert_eq!(
		session.loan_terms(loan.clone()).unwrap(),
		Some(terms(
			Some(980),
			Some(40_000),
			Some(15),
			Some("Credit union")
		))
	);
	assert_eq!(session.loan_terms(checking.clone()).unwrap(), None);
	assert_eq!(
		session
			.overview()
			.unwrap()
			.groups
			.iter()
			.find(|g| g.group == veritas_ffi::AccountGroup::Loans)
			.unwrap()
			.accounts
			.iter()
			.filter(|a| a.id == loan)
			.count(),
		1
	);

	perform(
		&session,
		Command::SetLoanTerms {
			account_id: loan.clone(),
			terms: terms(Some(925), None, Some(1), Some("Credit union")),
		},
	);
	assert_eq!(
		session.loan_terms(loan.clone()).unwrap(),
		Some(terms(Some(925), None, Some(1), Some("Credit union"))),
		"a nil field clears it"
	);
	session.undo().unwrap();
	assert_eq!(
		session.loan_terms(loan.clone()).unwrap().unwrap().payment,
		Some(40_000)
	);

	assert!(refused(session.perform(Command::SetLoanTerms {
		account_id: checking.clone(),
		terms: terms(Some(980), None, None, None),
	})));
	for bad in [
		terms(Some(-1), None, None, None),
		terms(None, Some(-1), None, None),
		terms(None, None, Some(0), None),
		terms(None, None, Some(32), None),
	] {
		assert!(refused(session.perform(Command::SetLoanTerms {
			account_id: loan.clone(),
			terms: bad,
		})));
	}
	assert!(refused(session.perform(Command::CreateAccount {
		account: NewAccount {
			name: "Not a loan".to_owned(),
			kind: NewAccountKind::Checking,
			starting_balance: 0,
			loan: Some(terms(Some(980), None, None, None)),
		},
	})));
}

struct LoanSetup {
	_scratch: Scratch,
	session: Arc<Session>,
	checking: String,
	loan: String,
	principal: String,
	interest: String,
	escrow: String,
}

fn loan_setup(label: &str, owed: i64) -> LoanSetup {
	use veritas_ffi::CategoryKind;
	let scratch = Scratch::new(label);
	let session = import(&scratch);
	let checking = new_account(
		&session,
		"Joint checking",
		NewAccountKind::Checking,
		500_000,
	);
	let loan = new_loan(
		&session,
		"Car loan",
		owed,
		terms(Some(980), Some(40_000), Some(3), Some("Credit union")),
	);
	let principal = created(
		&perform(
			&session,
			Command::CreateKindCategory {
				kind: CategoryKind::DebtPrincipal,
				name: "Loan Principal".to_owned(),
				unhide_group: false,
			},
		),
		EntityKind::Category,
	);
	LoanSetup {
		_scratch: scratch,
		interest: category(&session, "Utilities"),
		escrow: category(&session, "Groceries"),
		session,
		checking,
		loan,
		principal,
	}
}

fn payment(
	setup: &LoanSetup,
	id: &str,
	date: &str,
	total: i64,
	interest: i64,
) -> veritas_ffi::LoanPaymentDraft {
	veritas_ffi::LoanPaymentDraft {
		id: Some(id.to_owned()),
		from_account_id: setup.checking.clone(),
		loan_account_id: setup.loan.clone(),
		date: date.to_owned(),
		total,
		interest,
		escrow: 0,
		fees: 0,
		principal_category_id: setup.principal.clone(),
		interest_category_id: Some(setup.interest.clone()),
		escrow_category_id: None,
		fees_category_id: None,
		notes: None,
		cleared: true,
		fund_principal: false,
	}
}

#[test]
fn the_wizard_previews_the_split_and_refuses_a_payment_below_its_interest() {
	// DEBT-2.1 to 2.3, DEBT-4, DEBT-5 L-1, L-2, L-8 and L-9.
	use veritas_ffi::LoanPaymentProblem::{PaymentTooSmall, PaysOff};
	let setup = loan_setup("preview", 980_000);
	let session = &setup.session;
	let preview = |date: &str, total, interest| {
		session
			.loan_payment_preview(setup.loan.clone(), date.to_owned(), total, interest, 0, 0)
			.unwrap()
	};
	let l1 = preview("2026-11-20", 40_000, 8_000);
	assert_eq!(
		(
			l1.principal,
			l1.outstanding,
			l1.estimate,
			l1.rate_bps,
			l1.problem
		),
		(32_000, 980_000, Some(8_003), Some(980), None)
	);
	assert_eq!(
		preview("2026-11-20", 5_000, 8_000).problem,
		Some(PaymentTooSmall { short_by: 3_000 })
	);
	assert_eq!(
		preview("2026-11-20", 990_000, 0).problem,
		Some(PaysOff { credit: 10_000 })
	);
	let starting_day = session
		.register(
			veritas_ffi::RegisterScope::Account {
				account_id: setup.loan.clone(),
			},
			veritas_ffi::RegisterFilter::default(),
			veritas_ffi::RegisterSort {
				key: veritas_ffi::RegisterSortKey::Date,
				ascending: false,
			},
			0,
			1,
		)
		.unwrap()
		.rows[0]
		.date
		.clone();
	assert_eq!(
		preview(&starting_day, 40_000, 8_000).outstanding,
		0,
		"the starting balance is dated that day, so the day before owes nothing"
	);
	assert!(
		session
			.loan_payment_preview(
				setup.loan.clone(),
				"2026-11-20".to_owned(),
				40_000,
				-1,
				0,
				0
			)
			.is_err()
	);
	assert!(
		session
			.loan_payment_preview(
				setup.checking.clone(),
				"2026-11-20".to_owned(),
				40_000,
				0,
				0,
				0
			)
			.is_err(),
		"checking isn't a loan"
	);

	let mut too_small = payment(&setup, "too-small", "2026-11-20", 5_000, 3_000);
	too_small.escrow = 4_000;
	too_small.escrow_category_id = Some(setup.escrow.clone());
	match session.perform(Command::RecordLoanPayment { payment: too_small }) {
		Err(veritas_ffi::FfiError::Invalid { message }) => assert!(
			message.starts_with("This payment is 20.00 less than its interest, escrow and fees."),
			"DEBT-2.2 names the shortfall in the budget's format: {message}"
		),
		other => panic!("DEBT-2.2 refuses it: {other:?}"),
	}
	assert!(session.register_row("too-small".to_owned()).is_err());
}

#[test]
fn a_recorded_payment_is_the_split_actual_reads_and_prefills_the_next_one() {
	// DEBT-6 rows for L-1, then L-3's pre-fill and hint, L-4's escrow line,
	// L-5b's fee read as escrow, L-7's cap and A.2's replace. Actual imports
	// the export and agrees on every row and balance.
	let setup = loan_setup("record", 980_000);
	let session = &setup.session;
	perform(
		session,
		Command::RecordLoanPayment {
			payment: payment(&setup, "l1", "2026-11-20", 40_000, 8_000),
		},
	);
	let split = shown(session, "l1");
	assert_eq!(split.amount, -40_000);
	assert_eq!(split.payee_name.as_deref(), Some("Credit union"));
	assert_eq!(split.category_id, None);
	assert_eq!(split.lines.len(), 2);
	let (to_loan, interest) = (&split.lines[0], &split.lines[1]);
	assert_eq!(
		(to_loan.amount, to_loan.category_id.as_deref()),
		(-32_000, Some(setup.principal.as_str()))
	);
	assert_eq!(
		to_loan.transfer_account_id.as_deref(),
		Some(setup.loan.as_str())
	);
	assert_eq!(
		(interest.amount, interest.category_id.as_deref()),
		(-8_000, Some(setup.interest.as_str()))
	);
	assert_eq!(interest.payee_name.as_deref(), Some("Credit union"));
	assert_eq!(total(session, &setup.loan), -948_000);
	assert_eq!(total(session, &setup.checking), 460_000);

	let next = session
		.loan_payment_defaults(setup.loan.clone(), "2026-12-01".to_owned())
		.unwrap();
	assert_eq!(
		next,
		veritas_ffi::LoanPaymentDefaults {
			from_account_id: Some(setup.checking.clone()),
			total: Some(40_000),
			interest: Some(8_000),
			escrow: Some(0),
			principal_category_id: Some(setup.principal.clone()),
			interest_category_id: Some(setup.interest.clone()),
			escrow_category_id: None,
		}
	);
	let hint = session
		.loan_payment_preview(
			setup.loan.clone(),
			"2026-12-03".to_owned(),
			40_000,
			8_000,
			0,
			0,
		)
		.unwrap();
	assert_eq!(hint.estimate, Some(7_742));

	let mut with_fee = payment(&setup, "l5", "2026-12-03", 42_500, 8_000);
	with_fee.fees = 2_500;
	with_fee.fees_category_id = Some(setup.escrow.clone());
	assert!(
		refused(session.perform(Command::RecordLoanPayment {
			payment: veritas_ffi::LoanPaymentDraft {
				fees_category_id: None,
				..with_fee.clone()
			},
		})),
		"a fee needs a category"
	);
	perform(session, Command::RecordLoanPayment { payment: with_fee });
	let l5b = session
		.loan_payment_defaults(setup.loan.clone(), "2027-01-01".to_owned())
		.unwrap();
	assert_eq!(
		(l5b.total, l5b.interest, l5b.escrow),
		(Some(42_500), Some(8_000), Some(2_500)),
		"L-5b: the fee reads as escrow and never lands in principal"
	);

	let mut escrowed = payment(&setup, "l1", "2026-11-20", 200_000, 100_000);
	escrowed.escrow = 40_000;
	escrowed.escrow_category_id = Some(setup.escrow.clone());
	let lines_before: Vec<String> = shown(session, "l1")
		.lines
		.iter()
		.map(|l| l.id.clone())
		.collect();
	perform(session, Command::RecordLoanPayment { payment: escrowed });
	let replaced = shown(session, "l1");
	assert_eq!(
		replaced.lines.iter().map(|l| l.amount).collect::<Vec<_>>(),
		[-60_000, -100_000, -40_000],
		"L-4: principal, interest, escrow"
	);
	for old in &lines_before {
		assert!(gone(session, old), "A.2: the old lines are replaced");
	}
	assert_eq!(total(session, &setup.loan), -980_000 + 60_000 + 32_000);

	let zip = setup._scratch.join("payments.zip");
	block_on(session.export_actual_zip(text(&zip), CancelToken::new())).unwrap();
	if let Some(oracle) = oracle() {
		let actual = actual_reads(&oracle, &zip);
		let transactions = by_id(&actual, "transactions");
		let payees = by_id(&actual, "payees");
		for id in ["l1", "l5"] {
			let parent = transactions[id];
			assert_eq!(parent["isParent"], 1);
			assert_eq!(parent["category"], Value::Null);
			assert_eq!(
				payees[parent["description"].as_str().unwrap()]["name"],
				"Credit union"
			);
			let lines: Vec<&&Value> = transactions
				.values()
				.filter(|t| t["parent_id"] == id && t["tombstone"] != 1)
				.collect();
			let sum: i64 = lines.iter().map(|t| t["amount"].as_i64().unwrap()).sum();
			assert_eq!(Some(sum), parent["amount"].as_i64(), "A.1 for {id}");
			let principal = lines
				.iter()
				.find(|t| t["transferred_id"].is_string())
				.unwrap();
			assert_eq!(principal["category"], setup.principal.as_str());
			let counterpart = transactions[principal["transferred_id"].as_str().unwrap()];
			assert_eq!(counterpart["acct"], setup.loan.as_str());
			assert_eq!(counterpart["transferred_id"], principal["id"]);
			assert_eq!(counterpart["category"], Value::Null);
			assert_eq!(
				counterpart["amount"].as_i64(),
				principal["amount"].as_i64().map(|a| -a)
			);
		}
		for account in [&setup.loan, &setup.checking] {
			let theirs: i64 = transactions
				.values()
				.filter(|t| t["acct"] == account.as_str() && t["isParent"] != 1)
				.filter(|t| {
					t["tombstone"] != 1
						&& t["parent_id"]
							.as_str()
							.is_none_or(|parent| transactions[parent]["tombstone"] != 1)
				})
				.map(|t| t["amount"].as_i64().unwrap())
				.sum();
			assert_eq!(theirs, total(session, account));
		}
		let november = &actual["computed"]["months"]["2026-11"]["categories"];
		assert_eq!(
			november[&setup.principal]["spent"].as_i64(),
			Some(-60_000),
			"A.6: Actual counts the principal line as spending in its envelope"
		);
	}

	let capped = loan_setup("cap", 20_000);
	let first = capped
		.session
		.loan_payment_defaults(capped.loan.clone(), "2026-12-01".to_owned())
		.unwrap();
	assert_eq!(
		(first.total, first.interest, first.escrow),
		(Some(20_000), None, Some(0)),
		"DEBT-3 source 2: the rate never pre-fills interest, and a blank interest caps as 0"
	);
	let hint = capped
		.session
		.loan_payment_preview(
			capped.loan.clone(),
			"2026-12-01".to_owned(),
			20_000,
			0,
			0,
			0,
		)
		.unwrap()
		.estimate;
	assert_eq!(hint, Some(163), "DEBT-4's hint is still offered: 20000 * 980 / 120000");
	perform(
		&capped.session,
		Command::SetLoanTerms {
			account_id: capped.loan.clone(),
			terms: terms(None, Some(40_000), None, Some("Credit union")),
		},
	);
	let l7 = capped
		.session
		.loan_payment_defaults(capped.loan.clone(), "2026-12-01".to_owned())
		.unwrap();
	assert_eq!(
		(l7.total, l7.interest, l7.escrow),
		(Some(20_000), None, Some(0)),
		"L-7 without a rate: principal capped at what's owed"
	);
}
