#![allow(
	clippy::unwrap_used,
	clippy::expect_used,
	clippy::panic,
	reason = "integration tests fail by panicking"
)]

//! Loans, Move money and the register's extras over a year of real use,
//! through the exported API (R-XFER-1, R-XFER-3, R-XFER-4, R-DEBT-1,
//! R-DEBT-2), with Actual's own reading of the export as the verdict.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::Thread;

use serde_json::Value;
use veritas_ffi::{
	BudgetMonth, CancelToken, Change, Command, EntityKind, FfiError, LoanPaymentDraft, LoanTerms,
	Month, NetProfitLine, NewAccount, NewAccountKind, NewBudgetOptions, NumberFormat, NumberStyle,
	RegisterFilter, RegisterRow, RegisterScope, RegisterSort, RegisterSortKey, ReportDrill, Session,
	SplitLineDraft, TransactionDraft, TransferPurpose,
};

const TODAY: &str = "2026-12-31";

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
			"ffi-loan-year-{label}-{}-{}",
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

fn fixture(name: &str) -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../fixtures/synthetic/{name}.zip"))
}

fn import(scratch: &Scratch, fixture_name: &str) -> Arc<Session> {
	let session = block_on(Session::import_actual_zip(
		text(&fixture(fixture_name)),
		text(&scratch.join("budget")),
		CancelToken::new(),
	))
	.unwrap();
	session
		.budget_month(
			Month {
				year: 2026,
				month: 12,
			},
			TODAY.to_owned(),
		)
		.unwrap();
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

fn account(
	session: &Session,
	name: &str,
	kind: NewAccountKind,
	starting: i64,
	loan: Option<LoanTerms>,
) -> String {
	created(
		&perform(
			session,
			Command::CreateAccount {
				account: NewAccount {
					name: name.to_owned(),
					kind,
					starting_balance: starting,
					loan,
				},
			},
		),
		EntityKind::Account,
	)
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

fn transfer_payee(session: &Session, account: &str) -> String {
	session
		.payees()
		.unwrap()
		.into_iter()
		.find(|p| p.transfer_account_id.as_deref() == Some(account))
		.unwrap()
		.id
}

fn expense_group(session: &Session) -> String {
	session
		.budget_month(
			Month {
				year: 2026,
				month: 12,
			},
			TODAY.to_owned(),
		)
		.unwrap()
		.groups
		.into_iter()
		.find(|g| !g.is_income)
		.unwrap()
		.id
}

fn new_category(session: &Session, group: &str, name: &str) -> String {
	created(
		&perform(
			session,
			Command::CreateCategory {
				group_id: group.to_owned(),
				name: name.to_owned(),
			},
		),
		EntityKind::Category,
	)
}

fn shown(session: &Session, id: &str) -> RegisterRow {
	session.register_row(id.to_owned()).unwrap()
}

fn gone(session: &Session, id: &str) -> bool {
	session.register_row(id.to_owned()).is_err()
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

fn oracle() -> Option<PathBuf> {
	let oracle = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/actual-oracle");
	if oracle.join("node_modules").exists() {
		return Some(oracle);
	}
	assert!(
		std::env::var_os("VERITAS_REQUIRE_ORACLE").is_none(),
		"VERITAS_REQUIRE_ORACLE is set but tools/actual-oracle/node_modules is missing"
	);
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
		"Actual couldn't read the export: {}",
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

/// Every live row Actual counts, and none it would trip on: each split adds
/// up and carries no error, no live line hangs off a deleted parent, and
/// every transfer pairs both ways with its other side live. Returns the
/// live rows.
fn actual_finds_every_row_sound<'a>(transactions: &BTreeMap<String, &'a Value>) -> Vec<&'a Value> {
	let live = |row: &Value| {
		row["tombstone"] != 1
			&& row["parent_id"]
				.as_str()
				.filter(|_| row["isChild"] == 1)
				.is_none_or(|parent| transactions[parent]["tombstone"] != 1)
	};
	let mut rows = Vec::new();
	for row in transactions.values().copied() {
		if row["isChild"] == 1 && row["tombstone"] != 1 {
			let parent = transactions[row["parent_id"].as_str().unwrap()];
			assert_ne!(parent["tombstone"], 1, "{} outlives its split", row["id"]);
		}
		if !live(row) {
			continue;
		}
		rows.push(row);
		if row["isParent"] == 1 {
			let sum: i64 = transactions
				.values()
				.filter(|t| t["parent_id"] == row["id"] && t["isChild"] == 1 && t["tombstone"] != 1)
				.map(|t| t["amount"].as_i64().unwrap())
				.sum();
			assert_eq!(Some(sum), row["amount"].as_i64(), "split {}", row["id"]);
			assert_eq!(row["error"], Value::Null, "split {} error", row["id"]);
		}
		if let Some(other) = row["transferred_id"].as_str() {
			let other = transactions[other];
			assert!(live(other), "{}'s other side is gone", row["id"]);
			assert_eq!(other["transferred_id"], row["id"]);
			assert_eq!(other["amount"].as_i64(), row["amount"].as_i64().map(|a| -a));
		}
	}
	rows
}

fn month(n: u32) -> String {
	format!("2026-{n:02}-01")
}

#[test]
fn a_year_of_mortgage_payments_with_escrow_a_fee_edits_deletes_and_a_payoff_reads_back_in_actual() {
	// R-DEBT-1, R-DEBT-2, R-XFER-3. The wizard's own pre-fill and hint drive
	// each month, a late fee lands in March, payments are moved and deleted
	// from both sides and undone, and the last one pays the loan off. Actual
	// then imports the file and finds every payment a sound split, the loan
	// at zero and the same balances; Net Profit splits each payment into
	// expense and debt paydown.
	use veritas_ffi::CategoryKind;
	let scratch = Scratch::new("year");
	let session = import(&scratch, "small");
	session
		.budget_month(
			Month {
				year: 2025,
				month: 12,
			},
			"2025-12-15".to_owned(),
		)
		.unwrap();
	let checking = account(
		&session,
		"House checking",
		NewAccountKind::Checking,
		2_500_000,
		None,
	);
	let loan = account(
		&session,
		"Home loan",
		NewAccountKind::Mortgage,
		1_500_000,
		Some(LoanTerms {
			rate_bps: Some(590),
			payment: Some(130_000),
			payment_day: Some(1),
			lender: Some("Big Bank".to_owned()),
		}),
	);
	let group = expense_group(&session);
	let interest = new_category(&session, &group, "Mortgage interest");
	let escrow = new_category(&session, &group, "Escrow");
	let fees = new_category(&session, &group, "Bank fees");
	let principal = created(
		&perform(
			&session,
			Command::CreateKindCategory {
				kind: CategoryKind::DebtPrincipal,
				name: "Mortgage principal".to_owned(),
				unhide_group: false,
			},
		),
		EntityKind::Category,
	);

	let mut principals = BTreeMap::new();
	let record = |n: u32, id: &str, fee: i64| -> i64 {
		let date = month(n);
		let defaults = session
			.loan_payment_defaults(loan.clone(), date.clone())
			.unwrap();
		let escrow_part = 20_000;
		let mut total = defaults.total.unwrap();
		let hint = session
			.loan_payment_preview(loan.clone(), date.clone(), total, 0, escrow_part, fee)
			.unwrap()
			.estimate
			.unwrap();
		if n == 1 {
			assert_eq!(
				defaults.interest, None,
				"DEBT-3 source 2 leaves interest blank; DEBT-4 is only a hint"
			);
		} else {
			assert_eq!(
				defaults.escrow,
				Some(escrow_part),
				"the escrow carries over"
			);
		}
		total = total - defaults.interest.unwrap_or(0) - defaults.escrow.unwrap_or(0)
			+ hint + escrow_part
			+ fee;
		let preview = session
			.loan_payment_preview(loan.clone(), date.clone(), total, hint, escrow_part, fee)
			.unwrap();
		assert_eq!(preview.problem, None, "month {n}");
		perform(
			&session,
			Command::RecordLoanPayment {
				payment: LoanPaymentDraft {
					id: Some(id.to_owned()),
					from_account_id: checking.clone(),
					loan_account_id: loan.clone(),
					date,
					total,
					interest: hint,
					escrow: escrow_part,
					fees: fee,
					principal_category_id: principal.clone(),
					interest_category_id: Some(interest.clone()),
					escrow_category_id: Some(escrow.clone()),
					fees_category_id: (fee > 0).then(|| fees.clone()),
					notes: Some(format!("Payment {n}")),
					cleared: true,
					fund_principal: false,
				},
			},
		);
		let split = shown(&session, id);
		assert_eq!(split.split_difference, None);
		assert_eq!(
			split.lines.len(),
			if fee > 0 { 4 } else { 3 },
			"month {n}: {:#?}",
			split.lines
		);
		preview.principal
	};
	for n in 1..=11 {
		let fee = if n == 3 { 3_500 } else { 0 };
		let id = format!("pay-{n:02}");
		let paid = record(n, &id, fee);
		principals.insert(id, paid);
	}
	assert_eq!(
		session
			.loan_payment_defaults(loan.clone(), month(4))
			.unwrap()
			.escrow,
		Some(20_000),
		"after March's fee the escrow reads as escrow, not the fee"
	);

	let loan_side = |id: &str| shown(&session, id).lines[0].counterpart_id.clone().unwrap();
	let april = loan_side("pay-04");
	let mut moved = entry(&april, &loan, "2026-04-06", principals["pay-04"]);
	moved.payee_id = Some(transfer_payee(&session, &checking));
	perform(
		&session,
		Command::EditTransaction {
			transaction_id: april.clone(),
			transaction: moved,
		},
	);
	let payment = shown(&session, "pay-04");
	assert_eq!(
		payment.date, "2026-04-06",
		"the loan side moves the whole payment"
	);
	assert_eq!(payment.split_difference, None);

	let july = shown(&session, "pay-07");
	let mut later = entry("pay-07", &checking, "2026-07-03", july.amount);
	later.payee_id = july.payee_id.clone();
	later.notes = july.notes.clone();
	later.cleared = true;
	later.splits = july
		.lines
		.iter()
		.map(|l| SplitLineDraft {
			id: Some(l.id.clone()),
			amount: l.amount,
			payee_id: l.payee_id.clone(),
			payee_name: None,
			category_id: l.category_id.clone(),
			notes: l.notes.clone(),
		})
		.collect();
	perform(
		&session,
		Command::EditTransaction {
			transaction_id: "pay-07".to_owned(),
			transaction: later,
		},
	);
	assert_eq!(
		shown(&session, &loan_side("pay-07")).date,
		"2026-07-03",
		"the checking side moves the loan side"
	);

	let owed = total(&session, &loan);
	let september = loan_side("pay-09");
	assert_eq!(
		shown(&session, &september).counterpart_parent_id.as_deref(),
		Some("pay-09")
	);
	perform(
		&session,
		Command::DeleteTransaction {
			transaction_id: september.clone(),
		},
	);
	assert!(gone(&session, "pay-09") && gone(&session, &september));
	assert_eq!(total(&session, &loan), owed - principals["pay-09"]);
	session.undo().unwrap();
	assert_eq!(total(&session, &loan), owed);
	assert_eq!(shown(&session, "pay-09").lines.len(), 3);

	let october = loan_side("pay-10");
	perform(
		&session,
		Command::DeleteTransaction {
			transaction_id: "pay-10".to_owned(),
		},
	);
	assert!(
		gone(&session, &october),
		"the checking side takes the loan side"
	);
	session.undo().unwrap();
	assert!(!gone(&session, &october));
	session.redo().unwrap();
	assert!(gone(&session, "pay-10") && gone(&session, &october));
	principals.remove("pay-10");
	let paid = record(10, "pay-10b", 0);
	principals.insert("pay-10b".to_owned(), paid);

	let owed = -total(&session, &loan);
	let date = month(12);
	let hint = session
		.loan_payment_preview(loan.clone(), date.clone(), owed, 0, 0, 0)
		.unwrap()
		.estimate
		.unwrap();
	let payoff = owed + hint + 20_000;
	let preview = session
		.loan_payment_preview(loan.clone(), date.clone(), payoff, hint, 20_000, 0)
		.unwrap();
	assert_eq!((preview.principal, preview.problem), (owed, None));
	let over = session
		.loan_payment_preview(loan.clone(), date.clone(), payoff + 500, hint, 20_000, 0)
		.unwrap();
	assert_eq!(
		over.problem,
		Some(veritas_ffi::LoanPaymentProblem::PaysOff { credit: 500 })
	);
	perform(
		&session,
		Command::RecordLoanPayment {
			payment: LoanPaymentDraft {
				id: Some("payoff".to_owned()),
				from_account_id: checking.clone(),
				loan_account_id: loan.clone(),
				date,
				total: payoff,
				interest: hint,
				escrow: 20_000,
				fees: 0,
				principal_category_id: principal.clone(),
				interest_category_id: Some(interest.clone()),
				escrow_category_id: Some(escrow.clone()),
				fees_category_id: None,
				notes: Some("Payoff".to_owned()),
				cleared: true,
				fund_principal: false,
			},
		},
	);
	principals.insert("payoff".to_owned(), owed);
	assert_eq!(total(&session, &loan), 0, "paid off");
	assert_eq!(
		session
			.loan_terms(loan.clone())
			.unwrap()
			.unwrap()
			.lender
			.as_deref(),
		Some("Big Bank"),
		"a paid-off loan keeps its terms and its group (ST-9.3)"
	);

	let year = block_on(session.net_profit(
		Month {
			year: 2026,
			month: 1,
		},
		Month {
			year: 2026,
			month: 12,
		},
		CancelToken::new(),
	))
	.unwrap()
	.total;
	let paid_down: i64 = principals.values().sum();
	assert_eq!(paid_down, 1_500_000, "every cent of principal paid");
	assert_eq!(year.debt_paydown.total, paid_down);
	let spent = |category: &str| {
		year.expenses
			.categories
			.iter()
			.find(|c| c.category_id.as_deref() == Some(category))
			.map_or(0, |c| c.total)
	};
	assert_eq!(spent(&escrow), 12 * 20_000);
	assert_eq!(spent(&fees), 3_500);
	assert_eq!(spent(&principal), 0, "principal is never an expense");
	let interest_paid = spent(&interest);
	assert!(interest_paid > 0);
	let checking_out = 2_500_000 - total(&session, &checking);
	assert_eq!(
		checking_out,
		paid_down + interest_paid + 12 * 20_000 + 3_500,
		"each payment's cents land in exactly one place"
	);

	let zip = scratch.join("year.zip");
	block_on(session.export_actual_zip(text(&zip), CancelToken::new())).unwrap();
	let Some(oracle) = oracle() else {
		return;
	};
	let actual = actual_reads(&oracle, &zip);
	let transactions = by_id(&actual, "transactions");
	let live = actual_finds_every_row_sound(&transactions);
	for id in principals.keys() {
		let parent = transactions[id.as_str()];
		let lines: Vec<&Value> = live
			.iter()
			.copied()
			.filter(|t| t["parent_id"] == id.as_str())
			.collect();
		assert!(
			lines.iter().all(|l| l["date"] == parent["date"]),
			"{id} dates"
		);
		let to_loan = lines
			.iter()
			.find(|l| l["transferred_id"].is_string())
			.unwrap();
		assert_eq!(to_loan["category"], principal.as_str());
		let loan_row = transactions[to_loan["transferred_id"].as_str().unwrap()];
		assert_eq!(loan_row["date"], parent["date"], "{id} loan side date");
		assert_eq!(loan_row["acct"], loan.as_str());
	}
	assert_eq!(
		actual["computed"]["accounts"][&loan]["balance"].as_i64(),
		Some(0)
	);
	assert_eq!(
		actual["computed"]["accounts"][&checking]["balance"].as_i64(),
		Some(total(&session, &checking))
	);
	let gone_ids: BTreeSet<&str> = ["pay-10"].into();
	for id in gone_ids {
		assert_eq!(transactions[id]["tombstone"], 1);
	}
}

#[test]
fn move_money_previews_what_accounts_shows_after_every_move() {
	// R-XFER-1 and R-XFER-4: each preview's after balances are exactly what
	// the account list shows once the move is saved, and only on budget to
	// off budget asks what it was for. An account made as a loan stays one
	// at zero and overpaid (ST-9.3), so the wizard stays on offer.
	use NewAccountKind::{Checking, Investment, Loan, Savings};
	let scratch = Scratch::new("moves");
	let session = import(&scratch, "small");
	let checking = account(&session, "Joint checking", Checking, 400_000, None);
	let savings = account(&session, "Rainy day", Savings, 50_000, None);
	let brokerage = account(&session, "Brokerage", Investment, 300_000, None);
	let loan = account(&session, "Car loan", Loan, 60_000, None);
	let moves: [(&str, &str, i64, TransferPurpose, bool); 8] = [
		(&checking, &savings, 25_000, TransferPurpose::None, false),
		(&savings, &checking, 5_000, TransferPurpose::None, false),
		(&checking, &brokerage, 40_000, TransferPurpose::Ask, false),
		(&brokerage, &loan, 30_000, TransferPurpose::None, false),
		(&checking, &loan, 30_000, TransferPurpose::Ask, true),
		(&checking, &loan, 10_000, TransferPurpose::Ask, true),
		(&checking, &loan, 1, TransferPurpose::Ask, true),
		(&brokerage, &checking, 1, TransferPurpose::None, false),
	];
	for (n, (from, to, amount, purpose, to_loan)) in moves.into_iter().enumerate() {
		let preview = session
			.transfer_preview(from.to_owned(), to.to_owned(), amount)
			.unwrap();
		assert_eq!(
			(preview.purpose, preview.to_loan),
			(purpose, to_loan),
			"move {n}"
		);
		assert_eq!(
			(preview.from.now, preview.to.now),
			(total(&session, from), total(&session, to)),
			"move {n} before"
		);
		let mut draft = entry(&format!("move-{n}"), from, "2026-12-10", -amount);
		draft.payee_id = Some(transfer_payee(&session, to));
		perform(&session, Command::AddTransaction { transaction: draft });
		assert_eq!(
			(total(&session, from), total(&session, to)),
			(preview.from.after, preview.to.after),
			"move {n} after"
		);
	}
	assert_eq!(
		total(&session, &loan),
		10_001,
		"overpaid, still a loan (ST-9.3)"
	);

	let dated_later = |amount: i64| {
		let mut draft = entry("future", &checking, "2027-03-01", -amount);
		draft.payee_id = Some(transfer_payee(&session, &savings));
		draft
	};
	perform(
		&session,
		Command::AddTransaction {
			transaction: dated_later(7_000),
		},
	);
	let preview = session
		.transfer_preview(checking.clone(), savings.clone(), 1_000)
		.unwrap();
	assert_eq!(
		preview.from.now,
		total(&session, &checking),
		"the preview counts rows dated later, as the account list does"
	);
}

#[test]
fn amounts_actual_cannot_load_are_overflow_everywhere_money_is_typed() {
	// Actual's MAX_SAFE_NUMBER is 2^51 - 1 minor units. Past it, every
	// entry point refuses with Overflow and writes nothing.
	let scratch = Scratch::new("overflow");
	let session = import(&scratch, "small");
	let checking = account(
		&session,
		"Joint checking",
		NewAccountKind::Checking,
		0,
		None,
	);
	let savings = account(&session, "Rainy day", NewAccountKind::Savings, 0, None);
	let loan = account(
		&session,
		"Car loan",
		NewAccountKind::Loan,
		100_000,
		Some(LoanTerms {
			rate_bps: Some(500),
			payment: Some(10_000),
			payment_day: None,
			lender: None,
		}),
	);
	let max: i64 = (1 << 51) - 1;
	let overflow = |result: Result<Change, FfiError>| matches!(result, Err(FfiError::Overflow));
	let before = session.accounts().unwrap();

	assert!(overflow(session.perform(Command::AddTransaction {
		transaction: entry("big", &checking, "2026-12-01", max + 1),
	})));
	assert!(overflow(session.perform(Command::AddTransaction {
		transaction: entry("small", &checking, "2026-12-01", -(max + 1)),
	})));
	let mut split = entry("split", &checking, "2026-12-01", -max);
	split.splits = vec![
		SplitLineDraft {
			id: None,
			amount: -max,
			payee_id: None,
			payee_name: None,
			category_id: None,
			notes: None,
		},
		SplitLineDraft {
			id: None,
			amount: -1,
			payee_id: None,
			payee_name: None,
			category_id: None,
			notes: None,
		},
	];
	assert!(
		session
			.perform(Command::AddTransaction { transaction: split })
			.is_err(),
		"lines past the limit never save"
	);
	assert!(matches!(
		session.transfer_preview(checking.clone(), savings.clone(), max + 1),
		Err(FfiError::Overflow)
	));
	assert!(matches!(
		session.loan_payment_preview(loan.clone(), "2026-12-15".to_owned(), max + 1, 0, 0, 0),
		Err(FfiError::Overflow)
	));
	assert!(overflow(session.perform(Command::SetLoanTerms {
		account_id: loan.clone(),
		terms: LoanTerms {
			rate_bps: None,
			payment: Some(max + 1),
			payment_day: None,
			lender: None,
		},
	})));
	assert_eq!(session.accounts().unwrap(), before, "nothing was written");

	perform(
		&session,
		Command::AddTransaction {
			transaction: entry("at-limit", &checking, "2026-12-01", max),
		},
	);
	assert_eq!(total(&session, &checking), max, "the limit itself is fine");
}

fn drilled(session: &Session, line: NetProfitLine) -> BTreeSet<String> {
	block_on(session.report_transactions(
		ReportDrill::NetProfit {
			first: "2000-01-01".to_owned(),
			last: "2030-12-31".to_owned(),
			line,
			category: None,
		},
		CancelToken::new(),
	))
	.unwrap()
	.into_iter()
	.map(|row| row.id)
	.collect()
}

/// Which Net Profit line each row lands on, over all of history.
fn placement(session: &Session) -> BTreeMap<String, Vec<NetProfitLine>> {
	let mut placed: BTreeMap<String, Vec<NetProfitLine>> = BTreeMap::new();
	for line in [
		NetProfitLine::Income,
		NetProfitLine::Expenses,
		NetProfitLine::DebtPaydown,
		NetProfitLine::Savings,
		NetProfitLine::Unreviewed,
	] {
		for id in drilled(session, line) {
			placed.entry(id).or_default().push(line);
		}
	}
	placed
}

/// Each primary-leg row's kind and misfiled flag, classified from the
/// budget as it is on disk.
fn classified_on_disk(scratch: &Scratch) -> BTreeMap<String, (String, bool)> {
	let conn = rusqlite::Connection::open(scratch.join("budget").join("db.sqlite")).unwrap();
	let data = veritas_store::load(&conn).unwrap().data;
	veritas_reports::classify(&data)
		.rows()
		.iter()
		.filter(|row| row.leg == veritas_reports::Leg::Primary)
		.map(|row| {
			(
				row.id.to_string(),
				(format!("{:?}", row.decision.kind()), row.flags.misfiled),
			)
		})
		.collect()
}

#[test]
fn a_kind_changes_count_is_what_the_saved_change_reclassifies() {
	// CLS-3.3's confirm count against the change once saved: for every
	// expense category of all-features and each kind it could take, the
	// count `kindChangePreview` gives is the number of rows whose kind or
	// misfiled flag differs in the budget reread from disk. Rows that move
	// to another Net Profit line are among them, and every other counted
	// row is a purchase newly flagged misfiled, as CLS-3.3's example says.
	use veritas_ffi::CategoryKind;
	let scratch = Scratch::new("kinds");
	let session = import(&scratch, "all-features");
	let categories: Vec<String> = session
		.budget_month(
			Month {
				year: 2026,
				month: 12,
			},
			TODAY.to_owned(),
		)
		.unwrap()
		.groups
		.into_iter()
		.filter(|g| !g.is_income)
		.flat_map(|g| g.envelopes)
		.map(|e| e.id)
		.collect();
	let (mut moved_any, mut flagged_any) = (false, false);
	for category in &categories {
		for kind in [CategoryKind::Savings, CategoryKind::DebtPrincipal] {
			let lines_before = placement(&session);
			let rows_before = classified_on_disk(&scratch);
			let Ok(count) = session.kind_change_preview(category.clone(), kind) else {
				continue;
			};
			if session
				.perform(Command::SetCategoryKind {
					category_id: category.clone(),
					kind,
				})
				.is_err()
			{
				continue;
			}
			let rows_after = classified_on_disk(&scratch);
			let changed: BTreeSet<&String> = rows_after
				.iter()
				.filter(|(id, now)| rows_before.get(*id) != Some(now))
				.map(|(id, _)| id)
				.collect();
			assert_eq!(count as usize, changed.len(), "{category} to {kind:?}");
			let lines_after = placement(&session);
			for (id, lines) in &lines_after {
				if lines_before.get(id) != Some(lines) {
					assert!(changed.contains(id), "{id} moved line but wasn't counted");
					moved_any = true;
				}
			}
			for id in &changed {
				if lines_before.get(*id) == lines_after.get(*id) {
					assert!(rows_after[*id].1, "{id} counted without moving or a flag");
					flagged_any = true;
				}
			}
			session.undo().unwrap();
			assert_eq!(classified_on_disk(&scratch), rows_before, "undo");
		}
	}
	assert!(moved_any && flagged_any, "both kinds of change happened");
}

#[test]
fn a_new_principal_or_savings_category_goes_into_cls_4_1s_group() {
	// CLS-4.1: a visible "Savings and Debt" group wins, matched however it's
	// cased or spaced; otherwise a visible "Debt" or "Savings" group is
	// reused the same way, or created when missing.
	use veritas_ffi::{CategoryKind, KindTarget};
	let group_of = |session: &Session, category: &str| -> (String, String) {
		session
			.budget_month(
				Month {
					year: 2026,
					month: 12,
				},
				TODAY.to_owned(),
			)
			.unwrap()
			.groups
			.into_iter()
			.find(|g| g.envelopes.iter().any(|e| e.id == category))
			.map(|g| (g.id, g.name))
			.unwrap()
	};
	let make = |session: &Session, kind, name: &str| -> Result<String, FfiError> {
		session
			.perform(Command::CreateKindCategory {
				kind,
				name: name.to_owned(),
				unhide_group: false,
			})
			.map(|change| created(&change, EntityKind::Category))
	};
	let group = |session: &Session, name: &str| {
		created(
			&perform(
				session,
				Command::CreateCategoryGroup {
					name: name.to_owned(),
				},
			),
			EntityKind::CategoryGroup,
		)
	};

	let scratch = Scratch::new("cls41-shared");
	let session = import(&scratch, "small");
	let shared = group(&session, "  savings AND debt ");
	let principal = make(&session, CategoryKind::DebtPrincipal, "Car loan principal").unwrap();
	let saving = make(&session, CategoryKind::Savings, "House fund").unwrap();
	assert_eq!(group_of(&session, &principal).0, shared);
	assert_eq!(group_of(&session, &saving).0, shared);
	assert_eq!(
		session.kind_target(CategoryKind::DebtPrincipal).unwrap(),
		KindTarget::One {
			category_id: principal.clone()
		}
	);
	assert!(
		make(&session, CategoryKind::DebtPrincipal, "Another").is_err(),
		"one already exists: pick it"
	);

	let saving_group = group_of(&session, &saving).1;
	assert!(saving_group.eq_ignore_ascii_case("savings and debt"));

	let scratch = Scratch::new("cls41-own");
	let session = import(&scratch, "small");
	let debts = group(&session, " DEBT ");
	let principal = make(&session, CategoryKind::DebtPrincipal, "Loan principal").unwrap();
	assert_eq!(group_of(&session, &principal).0, debts, "reused");
	let saving = make(&session, CategoryKind::Savings, "Rainy day fund").unwrap();
	assert_eq!(
		group_of(&session, &saving).1,
		"Savings",
		"created when missing"
	);
	session.undo().unwrap();
	assert!(
		session
			.budget_month(
				Month {
					year: 2026,
					month: 12,
				},
				TODAY.to_owned(),
			)
			.unwrap()
			.groups
			.iter()
			.all(|g| g.name != "Savings"),
		"one undo takes back the category and the group it made"
	);
}

#[test]
fn a_hidden_debt_or_savings_group_does_not_stop_cls_4_1_creating_the_category() {
	// CLS-4.1: a hidden "Debt" or "Savings" group is reused and unhidden
	// after a confirm step, never shadowed by a second group. On "no" nothing
	// is written; on "yes" one undo step unhides the group and adds the
	// category, and the group's other categories keep their own flags.
	use veritas_ffi::{CategoryKind, EnvelopeGroup, KindCategoryPlan, KindTarget};
	let groups = |session: &Session| -> Vec<EnvelopeGroup> {
		session
			.budget_month(
				Month {
					year: 2026,
					month: 12,
				},
				TODAY.to_owned(),
			)
			.unwrap()
			.groups
	};
	let group_by_id = |session: &Session, id: &str| -> EnvelopeGroup {
		groups(session).into_iter().find(|g| g.id == id).unwrap()
	};
	let make = |session: &Session, kind, name: &str, unhide_group| {
		session
			.perform(Command::CreateKindCategory {
				kind,
				name: name.to_owned(),
				unhide_group,
			})
			.map(|change| created(&change, EntityKind::Category))
	};
	let hidden_group = |session: &Session, name: &str| {
		let id = created(
			&perform(
				session,
				Command::CreateCategoryGroup {
					name: name.to_owned(),
				},
			),
			EntityKind::CategoryGroup,
		);
		perform(
			session,
			Command::SetCategoryGroupHidden {
				group_id: id.clone(),
				hidden: true,
			},
		);
		id
	};

	let scratch = Scratch::new("cls41-hidden");
	let session = import(&scratch, "small");
	let debt = hidden_group(&session, "Debt");
	let old_loan = new_category(&session, &debt, "Old car loan");
	perform(
		&session,
		Command::SetCategoryHidden {
			category_id: old_loan.clone(),
			hidden: true,
		},
	);
	let student = new_category(&session, &debt, "Student loan");
	assert_eq!(
		session.kind_target(CategoryKind::DebtPrincipal).unwrap(),
		KindTarget::None
	);
	assert_eq!(
		session
			.kind_category_plan(CategoryKind::DebtPrincipal)
			.unwrap(),
		KindCategoryPlan {
			group_name: "Debt".to_owned(),
			group_is_hidden: true,
		}
	);

	let before = groups(&session);
	assert!(
		matches!(
			make(
				&session,
				CategoryKind::DebtPrincipal,
				"Loan Principal",
				false
			),
			Err(FfiError::Invalid { .. })
		),
		"without the user's yes the hidden group stays hidden"
	);
	assert_eq!(groups(&session), before, "cancel writes nothing");

	let principal = make(
		&session,
		CategoryKind::DebtPrincipal,
		"Loan Principal",
		true,
	)
	.unwrap();
	let home = group_by_id(&session, &debt);
	assert!(!home.hidden, "unhidden");
	assert_eq!(home.name, "Debt");
	let flags: BTreeMap<String, bool> = home
		.envelopes
		.iter()
		.map(|e| (e.id.clone(), e.hidden))
		.collect();
	assert_eq!(
		flags,
		BTreeMap::from([
			(principal.clone(), false),
			(old_loan.clone(), true),
			(student.clone(), false),
		]),
		"the new category lands in it, and the others keep their own flags"
	);
	assert_eq!(
		groups(&session)
			.iter()
			.filter(|g| g.name.eq_ignore_ascii_case("debt"))
			.count(),
		1,
		"never a second group"
	);
	assert_eq!(
		session.kind_target(CategoryKind::DebtPrincipal).unwrap(),
		KindTarget::One {
			category_id: principal.clone()
		}
	);

	session.undo().unwrap();
	let undone = group_by_id(&session, &debt);
	assert!(undone.hidden, "one undo hides the group again");
	assert!(undone.envelopes.iter().all(|e| e.id != principal));
	session.redo().unwrap();
	assert!(!group_by_id(&session, &debt).hidden);

	let savings = hidden_group(&session, "sAVINGS");
	assert_eq!(
		session.kind_category_plan(CategoryKind::Savings).unwrap(),
		KindCategoryPlan {
			group_name: "sAVINGS".to_owned(),
			group_is_hidden: true,
		},
		"matched ignoring case, as Actual matches group names"
	);
	assert!(make(&session, CategoryKind::Savings, "Rainy day fund", false).is_err());
	let saving = make(&session, CategoryKind::Savings, "Rainy day fund", true).unwrap();
	let home = group_by_id(&session, &savings);
	assert!(!home.hidden);
	assert!(home.envelopes.iter().any(|e| e.id == saving && !e.hidden));

	let scratch_income = Scratch::new("cls41-income");
	let renamed = import(&scratch_income, "small");
	let income = groups(&renamed).into_iter().find(|g| g.is_income).unwrap();
	perform(
		&renamed,
		Command::RenameCategoryGroup {
			group_id: income.id,
			name: "debt".to_owned(),
		},
	);
	assert!(
		matches!(
			renamed.kind_category_plan(CategoryKind::DebtPrincipal),
			Err(FfiError::Invalid { .. })
		),
		"an income group can't hold principal, and no second Debt group is made"
	);

	let zip = scratch.join("hidden.zip");
	block_on(session.export_actual_zip(text(&zip), CancelToken::new())).unwrap();
	let Some(oracle) = oracle() else {
		return;
	};
	let actual = actual_reads(&oracle, &zip);
	let group_rows = by_id(&actual, "category_groups");
	let category_rows = by_id(&actual, "categories");
	for group in [&debt, &savings] {
		assert_eq!(group_rows[group.as_str()]["hidden"], 0, "{group}");
		assert_eq!(group_rows[group.as_str()]["tombstone"], 0, "{group}");
	}
	assert_eq!(
		group_rows
			.values()
			.filter(|g| g["tombstone"] == 0)
			.filter(|g| g["name"]
				.as_str()
				.is_some_and(|n| n.eq_ignore_ascii_case("debt")))
			.count(),
		1
	);
	for (category, group, hidden) in [
		(&principal, &debt, 0),
		(&old_loan, &debt, 1),
		(&student, &debt, 0),
		(&saving, &savings, 0),
	] {
		let row = category_rows[category.as_str()];
		assert_eq!(row["cat_group"], group.as_str(), "{category}");
		assert_eq!(row["hidden"], hidden, "{category}");
		assert_eq!(row["tombstone"], 0, "{category}");
	}
	let december = &actual["computed"]["months"]["2026-12"];
	for group in [&debt, &savings] {
		assert!(december["groups"][group.as_str()].is_object(), "{group}");
	}
	for category in [&principal, &saving] {
		assert!(
			december["categories"][category.as_str()].is_object(),
			"{category}"
		);
	}
}

#[test]
fn balance_history_tags_and_the_exact_tag_filter_follow_the_register() {
	let scratch = Scratch::new("register");
	let session = import(&scratch, "small");
	let checking = account(
		&session,
		"Joint checking",
		NewAccountKind::Checking,
		100_000,
		None,
	);
	let notes = [
		("t1", "2026-03-01", -1_000, "Flights #trip"),
		("t2", "2026-03-05", -2_000, "#trips planning"),
		("t3", "2026-03-05", -3_000, "Hotel #trip#work"),
		("t4", "2026-03-31", 500, "Refund ##trip"),
		("t5", "2026-04-02", -4_000, "Taxi #Trip"),
		("t6", "2026-04-15", -6_000, "Dinner"),
	];
	for (id, date, amount, note) in notes {
		let mut draft = entry(id, &checking, date, amount);
		draft.notes = Some(note.to_owned());
		draft.cleared = true;
		session
			.perform(Command::AddTransaction { transaction: draft })
			.unwrap();
	}
	let mut split = entry("t7", &checking, "2026-04-20", -5_000);
	split.splits = vec![
		SplitLineDraft {
			id: None,
			amount: -3_000,
			payee_id: None,
			payee_name: Some("Hotel".to_owned()),
			category_id: None,
			notes: Some("room #trip".to_owned()),
		},
		SplitLineDraft {
			id: None,
			amount: -2_000,
			payee_id: None,
			payee_name: Some("Hotel".to_owned()),
			category_id: None,
			notes: Some("minibar".to_owned()),
		},
	];
	perform(&session, Command::AddTransaction { transaction: split });

	let scope = RegisterScope::Account {
		account_id: checking.clone(),
	};
	let sort = RegisterSort {
		key: RegisterSortKey::Date,
		ascending: false,
	};
	let ids = |filter: RegisterFilter| -> Vec<String> {
		let mut ids: Vec<String> = session
			.register(scope.clone(), filter, sort, 0, 100)
			.unwrap()
			.rows
			.into_iter()
			.map(|r| r.id)
			.collect();
		ids.sort();
		ids
	};
	let tagged = |tag: &str| {
		ids(RegisterFilter {
			tag: Some(tag.to_owned()),
			..Default::default()
		})
	};
	assert_eq!(
		tagged("trip"),
		["t1", "t3", "t7"],
		"exact, case-sensitive, lines too"
	);
	assert_eq!(tagged("trips"), ["t2"]);
	assert_eq!(tagged("Trip"), ["t5"]);
	assert_eq!(tagged("work"), ["t3"]);
	assert!(tagged("tri").is_empty());
	let counts: BTreeMap<String, u64> = session
		.tags()
		.unwrap()
		.into_iter()
		.map(|t| (t.name, t.count))
		.collect();
	assert_eq!(counts.get("trip"), Some(&3));
	assert_eq!(counts.get("trips"), Some(&1));
	assert_eq!(counts.get("Trip"), Some(&1));

	let all = session
		.register(scope.clone(), RegisterFilter::default(), sort, 0, 100)
		.unwrap()
		.rows;
	let april = session
		.register(
			scope.clone(),
			RegisterFilter {
				from: Some("2026-04-01".to_owned()),
				to: Some("2026-04-30".to_owned()),
				..Default::default()
			},
			sort,
			0,
			100,
		)
		.unwrap()
		.rows;
	assert_eq!(april.len(), 3);
	for row in &april {
		let full = all.iter().find(|r| r.id == row.id).unwrap();
		assert_eq!(
			row.running_balance, full.running_balance,
			"a date range keeps the account's real running balance"
		);
	}

	let history = session
		.balance_history(
			checking.clone(),
			"2026-03-01".to_owned(),
			"2026-04-30".to_owned(),
		)
		.unwrap();
	let end_of = |date: &str| -> i64 {
		all.iter()
			.filter(|r| r.date.as_str() <= date)
			.map(|r| r.amount)
			.sum()
	};
	assert_eq!(history.points.first().unwrap().date, "2026-03-01");
	assert_eq!(history.points.last().unwrap().date, "2026-04-30");
	for point in &history.points {
		assert_eq!(point.balance, end_of(&point.date), "{}", point.date);
	}
	let first = history.points.first().unwrap().balance as f64;
	let last = history.points.last().unwrap().balance as f64;
	assert_eq!(history.change, Some((last - first) / first.abs()));
	let empty = session
		.balance_history(
			checking.clone(),
			"2001-01-01".to_owned(),
			"2001-02-01".to_owned(),
		)
		.unwrap();
	assert_eq!(empty.change, None, "a zero start has no percent change");
	assert!(
		session
			.balance_history(checking, "2026-05-01".to_owned(), "2026-04-01".to_owned())
			.is_err()
	);
}

#[test]
fn cls_4_1_picks_the_right_group_around_hidden_and_income_groups() {
	// A hidden "Savings and Debt" group is never the home, so "Debt" is
	// created without asking and the hidden group stays as it was. A
	// visible "Savings and Debt" group wins over a hidden "Debt" group, so
	// nothing is unhidden. An income group named "Savings" can't hold a
	// savings category, and saying so writes nothing.
	use veritas_ffi::{CategoryKind, EnvelopeGroup, KindCategoryPlan};
	let groups = |session: &Session| -> Vec<EnvelopeGroup> {
		session
			.budget_month(
				Month {
					year: 2026,
					month: 12,
				},
				TODAY.to_owned(),
			)
			.unwrap()
			.groups
	};
	let make = |session: &Session, kind, name: &str, unhide_group| {
		session
			.perform(Command::CreateKindCategory {
				kind,
				name: name.to_owned(),
				unhide_group,
			})
			.map(|change| created(&change, EntityKind::Category))
	};
	let group = |session: &Session, name: &str, hidden: bool| {
		let id = created(
			&perform(
				session,
				Command::CreateCategoryGroup {
					name: name.to_owned(),
				},
			),
			EntityKind::CategoryGroup,
		);
		if hidden {
			perform(
				session,
				Command::SetCategoryGroupHidden {
					group_id: id.clone(),
					hidden: true,
				},
			);
		}
		id
	};
	let home_of = |session: &Session, category: &str| -> EnvelopeGroup {
		groups(session)
			.into_iter()
			.find(|g| g.envelopes.iter().any(|e| e.id == category))
			.unwrap()
	};

	let scratch = Scratch::new("cls41-hidden-shared");
	let session = import(&scratch, "small");
	let shared = group(&session, "Savings and Debt", true);
	assert_eq!(
		session
			.kind_category_plan(CategoryKind::DebtPrincipal)
			.unwrap(),
		KindCategoryPlan {
			group_name: "Debt".to_owned(),
			group_is_hidden: false,
		}
	);
	let principal = make(
		&session,
		CategoryKind::DebtPrincipal,
		"Loan principal",
		false,
	)
	.unwrap();
	let home = home_of(&session, &principal);
	assert_eq!((home.name.as_str(), home.hidden), ("Debt", false));
	assert_ne!(home.id, shared);
	assert!(
		groups(&session)
			.iter()
			.find(|g| g.id == shared)
			.unwrap()
			.hidden,
		"the hidden shared group is left alone"
	);

	let scratch = Scratch::new("cls41-visible-shared");
	let session = import(&scratch, "small");
	let hidden_debt = group(&session, "debt", true);
	let shared = group(&session, "SAVINGS and debt", false);
	assert!(
		!session
			.kind_category_plan(CategoryKind::DebtPrincipal)
			.unwrap()
			.group_is_hidden
	);
	let principal = make(
		&session,
		CategoryKind::DebtPrincipal,
		"Loan principal",
		false,
	)
	.unwrap();
	assert_eq!(home_of(&session, &principal).id, shared);
	assert!(
		groups(&session)
			.iter()
			.find(|g| g.id == hidden_debt)
			.unwrap()
			.hidden
	);

	let scratch = Scratch::new("cls41-income");
	let session = import(&scratch, "small");
	let income = session
		.budget_month(
			Month {
				year: 2026,
				month: 12,
			},
			TODAY.to_owned(),
		)
		.unwrap()
		.groups
		.into_iter()
		.find(|g| g.is_income)
		.unwrap()
		.id;
	perform(
		&session,
		Command::RenameCategoryGroup {
			group_id: income,
			name: "savings".to_owned(),
		},
	);
	assert!(matches!(
		session.kind_category_plan(CategoryKind::Savings),
		Err(FfiError::Invalid { .. })
	));
	let before = groups(&session);
	for unhide in [false, true] {
		assert!(matches!(
			make(&session, CategoryKind::Savings, "Rainy day fund", unhide),
			Err(FfiError::Invalid { .. })
		));
	}
	assert_eq!(groups(&session), before, "refusing writes nothing");
	let principal = make(
		&session,
		CategoryKind::DebtPrincipal,
		"Loan principal",
		false,
	)
	.unwrap();
	assert_eq!(
		home_of(&session, &principal).name,
		"Debt",
		"debt is unaffected"
	);
}

fn envelope_budgeted(month: &BudgetMonth, category: &str) -> i64 {
	month
		.groups
		.iter()
		.flat_map(|g| &g.envelopes)
		.find(|e| e.id == category)
		.unwrap()
		.budgeted
}

#[test]
fn the_wizard_funds_the_principal_from_to_budget_on_the_budget_scale_and_actual_agrees() {
	// DEBT-6 A.6 with D63's two scales. A USD budget funds 32017 cents of
	// principal as 32017; a yen budget's principal of 3,200,000 (register
	// scale, 32,000 yen) is budgeted as 32000 (budget scale). The payment
	// and the move are one undo step, and a payment with no interest needs
	// no interest category (DEBT-2).
	use veritas_ffi::CategoryKind;
	let december = Month {
		year: 2026,
		month: 12,
	};
	for (currency, principal, budgeted) in [("USD", 32_017, 32_017), ("JPY", 3_200_000, 32_000)] {
		let scratch = Scratch::new(&format!("fund-{currency}"));
		let session = block_on(Session::create_budget(
			currency.to_owned(),
			text(&scratch.join("budget")),
			NumberFormat {
				currency: Some(currency.to_owned()),
				style: NumberStyle::CommaDot,
				hide_decimals: false,
				symbol_first: None,
				symbol_space: None,
			},
			NewBudgetOptions {
				retirement_401k: false,
				hsa: false,
				accounts: Vec::new(),
			},
			CancelToken::new(),
		))
		.unwrap();
		let checking = account(&session, "Checking", NewAccountKind::Checking, 10_000_000, None);
		let loan = account(&session, "Car loan", NewAccountKind::Loan, 9_000_000, None);
		let veritas_ffi::KindTarget::One {
			category_id: category,
		} = session.kind_target(CategoryKind::DebtPrincipal).unwrap()
		else {
			panic!("{currency}: the starter set has one principal category");
		};
		let month = || session.budget_month(december, TODAY.to_owned()).unwrap();
		let before = month();
		let draft = LoanPaymentDraft {
			id: Some("pay".to_owned()),
			from_account_id: checking.clone(),
			loan_account_id: loan.clone(),
			date: "2026-12-05".to_owned(),
			total: principal,
			interest: 0,
			escrow: 0,
			fees: 0,
			principal_category_id: category.clone(),
			interest_category_id: None,
			escrow_category_id: None,
			fees_category_id: None,
			notes: None,
			cleared: false,
			fund_principal: true,
		};
		assert!(
			matches!(
				session.perform(Command::RecordLoanPayment {
					payment: LoanPaymentDraft {
						interest: 100,
						total: principal + 100,
						..draft.clone()
					},
				}),
				Err(FfiError::Invalid { .. })
			),
			"{currency}: interest that isn't 0 needs its category"
		);
		perform(&session, Command::RecordLoanPayment { payment: draft });
		assert_eq!(shown(&session, "pay").lines.len(), 1, "{currency}");
		let after = month();
		assert_eq!(
			envelope_budgeted(&after, &category),
			envelope_budgeted(&before, &category) + budgeted,
			"{currency}"
		);
		assert_eq!(after.to_budget, before.to_budget - budgeted, "{currency}");
		session.undo().unwrap();
		let undone = month();
		assert_eq!(envelope_budgeted(&undone, &category), 0, "{currency}");
		assert!(session.register_row("pay".to_owned()).is_err(), "one undo step");
		session.redo().unwrap();

		let zip = scratch.join("funded.zip");
		block_on(session.export_actual_zip(text(&zip), CancelToken::new())).unwrap();
		let Some(oracle) = oracle() else {
			continue;
		};
		let actual = actual_reads(&oracle, &zip);
		let theirs = &actual["computed"]["months"]["2026-12"];
		assert_eq!(
			theirs["categories"][&category]["budgeted"].as_i64(),
			Some(budgeted),
			"{currency}: Actual reads the cell on the budget scale"
		);
		assert_eq!(
			theirs["categories"][&category]["spent"].as_i64(),
			Some(-principal),
			"{currency}"
		);
		assert_eq!(
			theirs["toBudget"].as_i64(),
			Some(month().to_budget),
			"{currency}: To Budget agrees"
		);
	}
}
