#![allow(
	clippy::unwrap_used,
	clippy::expect_used,
	clippy::panic,
	reason = "integration tests fail by panicking"
)]

//! PLAN §8 Phase 3's end-to-end journeys through the exported API, as the
//! app drives it, each ending with Actual reading the exported file: a budget
//! started from scratch and used for a month, a goal closed and looked back
//! on a year later, and a scripted session of every kind of edit. Actual's
//! own budget months, account balances, schedules and report rows are the
//! judge (PLAN §10, docs/acceptance.md).

#[path = "support/children.rs"]
mod children;

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
	BillAmount, BillDate, BillDraft, BillFrequency, BillRepeat, BillRepeatEnd, BudgetMonth,
	CancelToken, CategoryKind, Command, EntityKind, FfiError, GoalState, LoanPaymentDraft, Month,
	NewAccount, NewAccountKind, NewBudgetOptions, NumberFormat, NumberStyle, ReadOnlyKind,
	RegisterFilter, RegisterRow, RegisterScope, RegisterSort, Session, SplitLineDraft,
	TransactionDraft,
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
			"ffi-plan-{label}-{}-{}",
			std::process::id(),
			NEXT.fetch_add(1, Ordering::Relaxed)
		));
		let _ = std::fs::remove_dir_all(&path);
		std::fs::create_dir_all(&path).unwrap();
		Scratch(path)
	}

	fn budget(&self) -> String {
		self.0.join("budget").display().to_string()
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

fn fixture(name: &str) -> String {
	Path::new(env!("CARGO_MANIFEST_DIR"))
		.join(format!("../../fixtures/synthetic/{name}.zip"))
		.display()
		.to_string()
}

fn month(text: &str) -> Month {
	let (year, month) = text.split_once('-').unwrap();
	Month {
		year: year.parse().unwrap(),
		month: month[..2].parse().unwrap(),
	}
}

fn label(month: Month) -> String {
	format!("{:04}-{:02}", month.year, month.month)
}

fn next(month: Month) -> Month {
	if month.month == 12 {
		Month {
			year: month.year + 1,
			month: 1,
		}
	} else {
		Month {
			year: month.year,
			month: month.month + 1,
		}
	}
}

fn usd() -> NumberFormat {
	NumberFormat {
		currency: Some("USD".to_owned()),
		style: NumberStyle::CommaDot,
		hide_decimals: false,
		symbol_first: None,
		symbol_space: None,
	}
}

fn account(name: &str, kind: NewAccountKind, starting_balance: i64) -> NewAccount {
	NewAccount {
		name: name.to_owned(),
		kind,
		starting_balance,
		loan: None,
	}
}

/// A budget made in the app. Its starting balances are dated by the core's
/// own clock, so the journey's "today" is that date: whatever day the test
/// runs on, every entry lands on or before it.
fn create(scratch: &Scratch, accounts: Vec<NewAccount>) -> (Arc<Session>, String) {
	let session = block_on(Session::create_budget(
		"Household".to_owned(),
		scratch.budget(),
		usd(),
		NewBudgetOptions {
			retirement_401k: false,
			hsa: false,
			accounts,
		},
		CancelToken::new(),
	))
	.unwrap();
	let today = every_row(&session)
		.into_iter()
		.map(|row| row.date)
		.min()
		.expect("starting balances");
	session.budget_month(month(&today), today.clone()).unwrap();
	(session, today)
}

/// Day `day` of `today`'s month, or `today` itself if that is earlier.
fn on(today: &str, day: u32) -> String {
	let current: u32 = today[8..10].parse().unwrap();
	format!("{}-{:02}", &today[..7], day.min(current))
}

fn months_after(start: Month, count: u32) -> Month {
	(0..count).fold(start, |m, _| next(m))
}

fn perform(session: &Session, command: Command) -> Option<String> {
	session
		.perform(command)
		.unwrap()
		.entities
		.into_iter()
		.find_map(|e| {
			matches!(
				e.kind,
				EntityKind::Account | EntityKind::Category | EntityKind::Transaction
			)
			.then_some(e.id)
			.flatten()
		})
}

fn created(session: &Session, command: Command, kind: EntityKind) -> String {
	session
		.perform(command)
		.unwrap()
		.entities
		.into_iter()
		.find(|e| e.kind == kind && e.id.is_some())
		.and_then(|e| e.id)
		.unwrap_or_else(|| panic!("no {kind:?} was created"))
}

fn envelope(session: &Session, today: &str, name: &str) -> String {
	session
		.budget_month(month(today), today.to_owned())
		.unwrap()
		.groups
		.into_iter()
		.flat_map(|g| g.envelopes)
		.find(|e| e.name == name)
		.unwrap_or_else(|| panic!("no envelope named {name}"))
		.id
}

fn account_id(session: &Session, name: &str) -> String {
	session
		.accounts()
		.unwrap()
		.accounts
		.into_iter()
		.find(|a| a.name == name)
		.unwrap_or_else(|| panic!("no account named {name}"))
		.id
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

fn draft(
	account: &str,
	date: &str,
	amount: i64,
	payee: &str,
	category: Option<&str>,
) -> TransactionDraft {
	TransactionDraft {
		id: None,
		account_id: account.to_owned(),
		date: date.to_owned(),
		amount,
		payee_id: None,
		payee_name: Some(payee.to_owned()),
		category_id: category.map(str::to_owned),
		notes: None,
		cleared: true,
		splits: Vec::new(),
	}
}

fn add(session: &Session, transaction: TransactionDraft) -> String {
	created(
		session,
		Command::AddTransaction { transaction },
		EntityKind::Transaction,
	)
}

fn transfer(session: &Session, from: &str, to: &str, date: &str, amount: i64) -> String {
	let mut transaction = draft(from, date, -amount, "", None);
	transaction.payee_name = None;
	transaction.payee_id = Some(transfer_payee(session, to));
	add(session, transaction)
}

fn every_row(session: &Session) -> Vec<RegisterRow> {
	let page = |offset| {
		session
			.register(
				RegisterScope::All,
				RegisterFilter::default(),
				RegisterSort {
					key: veritas_ffi::RegisterSortKey::Date,
					ascending: true,
				},
				offset,
				500,
			)
			.unwrap()
	};
	let first = page(0);
	let mut rows = first.rows;
	while (rows.len() as u64) < first.total {
		rows.extend(page(rows.len() as u64).rows);
	}
	rows.sort_by(|a, b| a.id.cmp(&b.id));
	rows
}

fn export(session: &Session, zip: &Path) {
	block_on(session.export_actual_zip(zip.display().to_string(), CancelToken::new())).unwrap();
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

/// Actual importing `zip` with its own `importBudget`, then its computed
/// months, balances and schedules beside the raw tables.
fn actual_reads(oracle: &Path, zip: &Path, today: &str) -> Value {
	let output = children::output(
		std::process::Command::new("node")
			.current_dir(oracle)
			.arg("src/main.ts")
			.arg("dump")
			.arg(zip)
			.args(["--today", today]),
	)
	.expect("node runs");
	assert!(
		output.status.success(),
		"Actual couldn't import {}: {}",
		zip.display(),
		String::from_utf8_lossy(&output.stderr)
	);
	serde_json::from_slice(&output.stdout).unwrap()
}

fn rows_by_id<'a>(dump: &'a Value, table: &str) -> BTreeMap<String, &'a Value> {
	dump["tables"][table]["rows"]
		.as_array()
		.unwrap()
		.iter()
		.map(|row| (row["id"].as_str().unwrap().to_owned(), row))
		.collect()
}

/// Every month Actual computes from the file against the Budget screen for
/// that month, and every account's balance against the account list: what
/// the user saw in Veritas is what Actual shows.
fn assert_actual_shows_what_veritas_showed(session: &Session, dump: &Value, today: &str) {
	let computed = &dump["computed"];
	let months = computed["months"].as_object().unwrap();
	assert!(!months.is_empty());
	let first: BudgetMonth = session
		.budget_month(month(months.keys().next().unwrap()), today.to_owned())
		.unwrap();
	let mut ours = Vec::new();
	let mut cursor = first.first_month;
	while (cursor.year, cursor.month) <= (first.last_month.year, first.last_month.month) {
		ours.push(label(cursor));
		cursor = next(cursor);
	}
	let theirs: Vec<String> = months.keys().cloned().collect();
	assert!(
		theirs.iter().all(|m| ours.contains(m)),
		"Actual has months Veritas doesn't: ours {ours:?}, Actual's {theirs:?}"
	);

	let mut differences = Vec::new();
	let (mut envelopes, mut accounts) = (0, 0);
	for (name, actual) in months {
		let shown = session.budget_month(month(name), today.to_owned()).unwrap();
		if actual["toBudget"] != shown.to_budget {
			differences.push(format!(
				"{name} to budget: Veritas {}, Actual {}",
				shown.to_budget, actual["toBudget"]
			));
		}
		for group in shown.groups.iter().filter(|g| !g.is_income) {
			for row in &group.envelopes {
				envelopes += 1;
				let theirs = &actual["categories"][&row.id];
				for (what, ours, key) in [
					("budgeted", row.budgeted, "budgeted"),
					("spent", row.spent, "spent"),
					("available", row.available, "balance"),
				] {
					if theirs[key] != ours {
						differences.push(format!(
							"{name} {} {what}: Veritas {ours}, Actual {}",
							row.name, theirs[key]
						));
					}
				}
			}
		}
	}
	for account in session
		.accounts()
		.unwrap()
		.accounts
		.iter()
		.filter(|a| !a.closed)
	{
		accounts += 1;
		let theirs = &computed["accounts"][&account.id]["balance"];
		if *theirs != account.total {
			differences.push(format!(
				"{} balance: Veritas {}, Actual {theirs}",
				account.name, account.total
			));
		}
	}
	assert!(differences.is_empty(), "{differences:#?}");
	assert!(
		envelopes > 0 && accounts > 0,
		"compared {envelopes} envelopes and {accounts} accounts"
	);
}

#[test]
fn a_budget_started_in_veritas_and_used_for_a_month_reads_the_same_in_actual() {
	// PLAN §10 (Interop): a new budget with starting balances, the starter
	// categories, a month of budgeting and everyday entry, exported and
	// imported by Actual with no errors and the same balances and budgets.
	let scratch = Scratch::new("from-scratch");
	let (session, today) = create(
		&scratch,
		vec![
			account("Checking", NewAccountKind::Checking, 182_450),
			account("Rewards Visa", NewAccountKind::CreditCard, 31_277),
			account("Emergency Fund", NewAccountKind::Savings, 500_000),
		],
	);
	let today = today.as_str();
	let checking = account_id(&session, "Checking");
	let visa = account_id(&session, "Rewards Visa");
	let savings = account_id(&session, "Emergency Fund");
	let current = month(today);
	let wages = envelope(&session, today, "Wages");
	let groceries = envelope(&session, today, "Groceries");
	let dining = envelope(&session, today, "Dining Out");
	let power = envelope(&session, today, "Electricity and Gas");
	let household = envelope(&session, today, "Miscellaneous");
	let savings_envelope = envelope(&session, today, "Savings");

	for (category, amount) in [
		(&groceries, 60_000),
		(&dining, 15_000),
		(&power, 11_234),
		(&household, 25_000),
		(&savings_envelope, 50_000),
	] {
		perform(
			&session,
			Command::SetBudget {
				month: current,
				category_id: category.clone(),
				amount,
			},
		);
	}
	perform(
		&session,
		Command::MoveBudget {
			month: current,
			amount: 5_000,
			from_category_id: Some(household.clone()),
			to_category_id: Some(dining.clone()),
		},
	);

	add(
		&session,
		draft(
			&checking,
			&on(today, 6),
			312_500,
			"Acme Payroll",
			Some(&wages),
		),
	);
	add(
		&session,
		draft(
			&checking,
			&on(today, 20),
			312_500,
			"Acme Payroll",
			Some(&wages),
		),
	);
	add(
		&session,
		draft(
			&visa,
			&on(today, 3),
			-5_000,
			"Corner Grocer",
			Some(&groceries),
		),
	);
	add(
		&session,
		draft(
			&visa,
			&on(today, 14),
			-8_766,
			"Corner Grocer",
			Some(&groceries),
		),
	);
	add(
		&session,
		draft(&visa, &on(today, 22), -4_319, "Thai Basil", Some(&dining)),
	);
	add(
		&session,
		draft(
			&checking,
			&on(today, 9),
			-10_982,
			"City Power",
			Some(&power),
		),
	);
	let mut target = draft(&visa, &on(today, 16), -12_000, "Target", None);
	target.splits = vec![
		SplitLineDraft {
			id: None,
			amount: -7_000,
			category_id: Some(groceries.clone()),
			payee_id: None,
			payee_name: None,
			notes: None,
		},
		SplitLineDraft {
			id: None,
			amount: -5_000,
			category_id: Some(household.clone()),
			payee_id: None,
			payee_name: Some("Target".to_owned()),
			notes: Some("Towels".to_owned()),
		},
	];
	add(&session, target);
	transfer(&session, &checking, &visa, &on(today, 25), 31_277);
	transfer(&session, &checking, &savings, &on(today, 21), 50_000);

	let shown = session.budget_month(current, today.to_owned()).unwrap();
	let row = |id: &str| {
		shown
			.groups
			.iter()
			.flat_map(|g| &g.envelopes)
			.find(|e| e.id == id)
			.unwrap()
			.clone()
	};
	assert_eq!(
		row(&groceries).spent,
		-(5_000 + 8_766 + 7_000),
		"a card swipe spends in its month"
	);
	assert_eq!(row(&household).spent, -5_000);
	let purchases = 5_000 + 8_766 + 7_000 + 4_319 + 10_982 + 5_000;
	let spent: i64 = shown
		.groups
		.iter()
		.filter(|g| !g.is_income)
		.map(|g| g.spent)
		.sum();
	assert_eq!(
		spent, -purchases,
		"the card payment and the savings transfer spend nothing; only purchases do"
	);
	let net_profit = block_on(session.net_profit(current, current, CancelToken::new()))
		.unwrap()
		.total;
	assert_eq!(
		net_profit.expenses.total, purchases,
		"Net Profit counts the swipes as expenses and never the card payment"
	);
	assert_eq!(row(&dining).budgeted, 20_000);
	assert_eq!(
		row(&savings_envelope).spent,
		0,
		"moving money to an on-budget account spends nothing"
	);
	let balances: BTreeMap<String, i64> = session
		.accounts()
		.unwrap()
		.accounts
		.into_iter()
		.map(|a| (a.name, a.total))
		.collect();
	assert_eq!(
		balances["Checking"],
		182_450 + 625_000 - 10_982 - 31_277 - 50_000
	);
	assert_eq!(
		balances["Rewards Visa"],
		-31_277 - 5_000 - 8_766 - 4_319 - 12_000 + 31_277
	);
	assert_eq!(balances["Emergency Fund"], 550_000);
	assert_eq!(
		session.audit_report().unwrap().unwrap_or_default(),
		Vec::new()
	);

	let Some(oracle) = oracle() else { return };
	let zip = scratch.join("household.zip");
	export(&session, &zip);
	let dump = actual_reads(&oracle, &zip, today);
	assert_actual_shows_what_veritas_showed(&session, &dump, today);
	let transactions = rows_by_id(&dump, "transactions");
	let children: Vec<i64> = transactions
		.values()
		.filter(|t| t["isChild"] == 1 && t["tombstone"] != 1)
		.map(|t| t["amount"].as_i64().unwrap())
		.collect();
	assert_eq!(
		children.iter().sum::<i64>(),
		-12_000,
		"the split reads as one purchase in Actual"
	);
}

#[test]
fn a_goal_closed_this_year_keeps_its_transactions_and_still_shows_in_last_years_reports() {
	// PLAN §10 (Budget & goals): $1,500 due in 6 months suggests $250 a month;
	// the purchase makes it reached/spent; closing releases the leftover by a
	// budget move, touches no transaction and hides the envelope; and next
	// year, last year's Net Profit and Wrapped still show it.
	let scratch = Scratch::new("goal");
	let (session, today) = create(
		&scratch,
		vec![account("Checking", NewAccountKind::Checking, 400_000)],
	);
	let this_month = month(&today);
	let due = format!("{}-15", label(months_after(this_month, 5)));
	let year = this_month.year;
	let next_year = format!("{}-01-15", year + 1);
	let (today, next_year) = (today.as_str(), next_year.as_str());
	let checking = account_id(&session, "Checking");
	perform(
		&session,
		Command::CreateGoal {
			name: "New laptop".to_owned(),
			target: 150_000,
			target_date: Some(due.clone()),
			icon: Some("laptopcomputer".to_owned()),
			color: None,
		},
	);
	let goal = |today: &str| {
		session
			.goals(today.to_owned())
			.unwrap()
			.goals
			.into_iter()
			.find(|g| g.name == "New laptop")
			.unwrap()
	};
	let laptop = goal(today);
	assert_eq!(laptop.suggested_monthly, Some(25_000));
	assert_eq!(laptop.months_left, Some(6));

	perform(
		&session,
		Command::SetBudget {
			month: this_month,
			category_id: laptop.category_id.clone(),
			amount: 150_000,
		},
	);
	assert_eq!(goal(today).state, GoalState::Reached);
	let purchase = add(
		&session,
		draft(
			&checking,
			today,
			-148_000,
			"Apple Store",
			Some(&laptop.category_id),
		),
	);
	assert_eq!(goal(today).state, GoalState::ReachedSpent);
	assert_eq!(goal(today).available, 2_000);

	let before_close = every_row(&session);
	let to_budget = session
		.budget_month(this_month, today.to_owned())
		.unwrap()
		.to_budget;
	perform(
		&session,
		Command::CloseGoal {
			goal_id: laptop.id.clone(),
			release_to_category_id: None,
			also_move_later: false,
		},
	);
	assert_eq!(
		every_row(&session),
		before_close,
		"closing a goal never touches a transaction"
	);
	let shown = session.budget_month(this_month, today.to_owned()).unwrap();
	assert_eq!(
		shown.to_budget,
		to_budget + 2_000,
		"the leftover goes back by a budget move"
	);
	let envelope = shown
		.groups
		.iter()
		.flat_map(|g| &g.envelopes)
		.find(|e| e.id == laptop.category_id)
		.unwrap();
	assert!(envelope.hidden);
	assert_eq!((envelope.budgeted, envelope.available), (148_000, 0));

	session
		.budget_month(month(next_year), next_year.to_owned())
		.unwrap();
	let last_year = block_on(session.net_profit(
		Month { year, month: 1 },
		Month { year, month: 12 },
		CancelToken::new(),
	))
	.unwrap()
	.total;
	let line = last_year
		.expenses
		.categories
		.iter()
		.find(|c| c.category_id.as_deref() == Some(laptop.category_id.as_str()))
		.expect("last year's Net Profit lists the closed goal");
	assert_eq!(line.total, 148_000);
	assert!(line.labels.closed_goal);
	let wrapped =
		block_on(session.wrapped(year, next_year.to_owned(), CancelToken::new())).unwrap();
	assert!(wrapped.completed_goal_ids.contains(&laptop.id));
	let top = wrapped
		.top_categories
		.iter()
		.find(|c| c.category_id == laptop.category_id)
		.expect("last year's Wrapped counts the closed goal's spending");
	assert_eq!(top.total, 148_000);
	assert!(top.labels.closed_goal);

	let Some(oracle) = oracle() else { return };
	let zip = scratch.join("goal.zip");
	export(&session, &zip);
	let dump = actual_reads(&oracle, &zip, next_year);
	assert_actual_shows_what_veritas_showed(&session, &dump, next_year);
	assert_eq!(
		rows_by_id(&dump, "transactions")[&purchase]["category"],
		laptop.category_id.as_str()
	);
	assert_eq!(
		rows_by_id(&dump, "categories")[&laptop.category_id]["hidden"],
		1
	);
}

const SPENDING_REPORT: &str = "c7eaad37-4a0f-4f44-8c5e-ca30ff232dd2";

#[test]
fn a_scripted_session_of_every_kind_of_edit_reads_back_in_actual() {
	// PLAN §10 (Interop): budget edits, a transfer, a split loan payment, a new
	// bill, a closed goal and an edited custom report in one session; Actual
	// imports the export with no errors and shows the same balances, budgets,
	// schedule and report.
	const TODAY: &str = "2026-11-13";
	let scratch = Scratch::new("scripted");
	let session = block_on(Session::import_actual_zip(
		fixture("all-features"),
		scratch.budget(),
		CancelToken::new(),
	))
	.unwrap();
	let november = month(TODAY);
	session.budget_month(november, TODAY.to_owned()).unwrap();
	let checking = "0a1c000f-c328-42b8-8966-a4be59a2504b";
	let groceries = "fc093a41-2d11-4d98-a211-e5db92205f23";
	let household = "e2dcc389-9fe5-4b1f-93d6-0099cd592c17";

	perform(
		&session,
		Command::SetBudget {
			month: november,
			category_id: groceries.to_owned(),
			amount: 45_000,
		},
	);
	perform(
		&session,
		Command::MoveBudget {
			month: november,
			amount: 5_000,
			from_category_id: Some(groceries.to_owned()),
			to_category_id: Some(household.to_owned()),
		},
	);

	let fund = created(
		&session,
		Command::CreateAccount {
			account: account("Rainy Day", NewAccountKind::Savings, 0),
		},
		EntityKind::Account,
	);
	transfer(&session, checking, &fund, "2026-11-10", 20_000);

	let loan = created(
		&session,
		Command::CreateAccount {
			account: NewAccount {
				loan: Some(veritas_ffi::LoanTerms {
					rate_bps: Some(650),
					payment: Some(40_000),
					payment_day: Some(5),
					lender: Some("Credit union".to_owned()),
				}),
				..account("Car loan", NewAccountKind::Loan, 1_200_000)
			},
		},
		EntityKind::Account,
	);
	let principal = created(
		&session,
		Command::CreateKindCategory {
			kind: CategoryKind::DebtPrincipal,
			name: "Loan Principal".to_owned(),
			unhide_group: false,
		},
		EntityKind::Category,
	);
	let payment: Vec<String> = session
		.perform(Command::RecordLoanPayment {
			payment: LoanPaymentDraft {
				id: None,
				from_account_id: checking.to_owned(),
				loan_account_id: loan.clone(),
				date: "2026-11-05".to_owned(),
				total: 40_000,
				interest: 8_000,
				escrow: 0,
				fees: 0,
				principal_category_id: principal.clone(),
				interest_category_id: Some(household.to_owned()),
				escrow_category_id: None,
				fees_category_id: None,
				notes: None,
				cleared: true,
				fund_principal: false,
			},
		})
		.unwrap()
		.entities
		.into_iter()
		.filter(|e| e.kind == EntityKind::Transaction)
		.filter_map(|e| e.id)
		.collect();

	let power = session
		.payees()
		.unwrap()
		.into_iter()
		.find(|p| p.transfer_account_id.is_none())
		.unwrap()
		.id;
	perform(
		&session,
		Command::CreateBill {
			bill_id: Some("phone-bill".to_owned()),
			bill: BillDraft {
				name: Some("Phone".to_owned()),
				payee_id: Some(power),
				account_id: Some(checking.to_owned()),
				amount: BillAmount::Exact { amount: -6_500 },
				date: BillDate::Repeat {
					repeat: BillRepeat {
						start: "2026-12-03".to_owned(),
						interval: 1,
						frequency: BillFrequency::Monthly,
						patterns: Vec::new(),
						weekend: None,
						end: BillRepeatEnd::Never,
					},
				},
				category_id: Some(household.to_owned()),
				auto_post: false,
			},
		},
	);

	perform(
		&session,
		Command::CreateGoal {
			name: "Bike".to_owned(),
			target: 30_000,
			target_date: None,
			icon: None,
			color: None,
		},
	);
	let bike = session
		.goals(TODAY.to_owned())
		.unwrap()
		.goals
		.into_iter()
		.find(|g| g.name == "Bike")
		.unwrap();
	perform(
		&session,
		Command::SetBudget {
			month: november,
			category_id: bike.category_id.clone(),
			amount: 30_000,
		},
	);
	let bike_purchase = add(
		&session,
		draft(
			checking,
			"2026-11-12",
			-29_000,
			"Bike Shop",
			Some(&bike.category_id),
		),
	);
	perform(
		&session,
		Command::CloseGoal {
			goal_id: bike.id.clone(),
			release_to_category_id: Some(groceries.to_owned()),
			also_move_later: false,
		},
	);

	let mut report = block_on(session.custom_report(
		SPENDING_REPORT.to_owned(),
		TODAY.to_owned(),
		CancelToken::new(),
	))
	.unwrap()
	.settings;
	report.name = "Where it went".to_owned();
	report.interval = veritas_ffi::ReportIntervalUnit::Yearly;
	perform(&session, Command::SaveCustomReport { settings: report });

	let interest = block_on(session.net_profit(november, november, CancelToken::new()))
		.unwrap()
		.total;
	assert_eq!(
		interest
			.debt_paydown
			.categories
			.iter()
			.find(|c| c.category_id.as_deref() == Some(principal.as_str()))
			.map(|c| c.total),
		Some(32_000),
		"the principal is debt paydown"
	);
	let bill_next = session
		.bills(TODAY.to_owned())
		.unwrap()
		.into_iter()
		.find(|b| b.id == "phone-bill")
		.unwrap()
		.next_date;
	assert_eq!(bill_next.as_deref(), Some("2026-12-03"));
	assert_eq!(
		session.audit_report().unwrap().unwrap_or_default(),
		Vec::new()
	);

	let Some(oracle) = oracle() else { return };
	let zip = scratch.join("session.zip");
	export(&session, &zip);
	let dump = actual_reads(&oracle, &zip, TODAY);
	assert_actual_shows_what_veritas_showed(&session, &dump, TODAY);

	let transactions = rows_by_id(&dump, "transactions");
	let parent = transactions
		.values()
		.find(|t| t["isChild"] == 1 && t["category"] == principal.as_str())
		.and_then(|t| t["parent_id"].as_str())
		.expect("Actual finds the principal line")
		.to_owned();
	assert!(
		payment.contains(&parent),
		"the split is the payment the wizard recorded"
	);
	assert_eq!(transactions[&parent]["acct"], checking);
	assert_eq!(transactions[&parent]["amount"], -40_000);
	let lines: Vec<(Option<&str>, i64)> = transactions
		.values()
		.filter(|t| t["parent_id"] == parent.as_str() && t["tombstone"] != 1)
		.map(|t| (t["category"].as_str(), t["amount"].as_i64().unwrap()))
		.collect();
	let mut lines = lines;
	lines.sort();
	let mut expected = vec![
		(Some(principal.as_str()), -32_000),
		(Some(household), -8_000),
	];
	expected.sort();
	assert_eq!(
		lines, expected,
		"Actual reads the payment as a categorized split"
	);
	assert_eq!(transactions[&parent]["isParent"], 1);
	assert_eq!(
		transactions[&bike_purchase]["category"],
		bike.category_id.as_str()
	);
	assert_eq!(
		rows_by_id(&dump, "categories")[&bike.category_id]["hidden"],
		1
	);

	assert_eq!(
		dump["computed"]["schedules"]["phone-bill"]["nextDate"], "2026-12-03",
		"Actual schedules the bill where Veritas does"
	);
	let schedules = rows_by_id(&dump, "schedules");
	assert_eq!(schedules["phone-bill"]["posts_transaction"], 0);
	let saved = rows_by_id(&dump, "custom_reports")[SPENDING_REPORT];
	assert_eq!(saved["name"], "Where it went");
	assert_eq!(saved["interval"], "Yearly");
}

#[test]
fn a_newer_actual_file_opened_through_the_api_refuses_every_edit_and_is_never_written() {
	// PLAN §10 (Interop), R-IO-7: the app sees why it is read-only, and no
	// command, undo or close writes a byte.
	const TODAY: &str = "2026-11-13";
	let scratch = Scratch::new("newer");
	let session = block_on(Session::import_actual_zip(
		fixture("small"),
		scratch.budget(),
		CancelToken::new(),
	))
	.unwrap();
	session.close().unwrap();
	let db = Path::new(&scratch.budget()).join("db.sqlite");
	rusqlite::Connection::open(&db)
		.unwrap()
		.execute_batch("INSERT INTO __migrations__ (id) VALUES (1795000000000);")
		.unwrap();
	let before = std::fs::read(&db).unwrap();

	let session = children::while_no_child_starts(|| {
		block_on(Session::open(scratch.budget(), CancelToken::new())).unwrap()
	});
	assert_eq!(
		session.state().unwrap().read_only,
		Some(ReadOnlyKind::NewerActualVersion)
	);
	let shown = session
		.budget_month(month(TODAY), TODAY.to_owned())
		.unwrap();
	let category = shown
		.groups
		.iter()
		.find(|g| !g.is_income)
		.and_then(|g| g.envelopes.first())
		.unwrap()
		.id
		.clone();
	let account = session.accounts().unwrap().accounts[0].id.clone();
	for command in [
		Command::SetBudget {
			month: shown.month,
			category_id: category.clone(),
			amount: 1_234,
		},
		Command::AddTransaction {
			transaction: draft(&account, TODAY, -100, "Anyone", Some(&category)),
		},
		Command::CreateAccount {
			account: account_named("Sneaky"),
		},
		Command::RenameCategory {
			category_id: category.clone(),
			name: "Renamed".to_owned(),
		},
	] {
		let result = session.perform(command);
		assert!(
			matches!(
				result,
				Err(FfiError::ReadOnly {
					reason: ReadOnlyKind::NewerActualVersion
				})
			),
			"{result:?}"
		);
	}
	assert!(!session.state().unwrap().can_undo);
	session.close().unwrap();
	assert_eq!(
		std::fs::read(&db).unwrap(),
		before,
		"the file is byte for byte as Actual left it"
	);
	let wal = Path::new(&scratch.budget()).join("db.sqlite-wal");
	assert!(
		std::fs::metadata(&wal).map_or(true, |m| m.len() == 0),
		"nothing was written ahead either"
	);
}

fn account_named(name: &str) -> NewAccount {
	account(name, NewAccountKind::Checking, 0)
}

#[test]
fn a_backup_is_a_file_actual_opens_and_a_removed_folder_never_blocks_editing() {
	// PLAN §10 (Interop), R-REL-4: the backup is a valid Actual export of the
	// budget as it stands; with its folder gone, backing up fails with a typed
	// error, editing carries on and nothing is lost.
	let scratch = Scratch::new("backup");
	let (session, today) = create(
		&scratch,
		vec![account("Checking", NewAccountKind::Checking, 120_000)],
	);
	let today = today.as_str();
	let checking = account_id(&session, "Checking");
	let groceries = envelope(&session, today, "Groceries");
	perform(
		&session,
		Command::SetBudget {
			month: month(today),
			category_id: groceries.clone(),
			amount: 40_000,
		},
	);
	add(
		&session,
		draft(
			&checking,
			&on(today, 4),
			-6_123,
			"Corner Grocer",
			Some(&groceries),
		),
	);

	let folder = scratch.join("OneDrive Backups");
	std::fs::create_dir_all(&folder).unwrap();
	session
		.set_backup_folder(Some(folder.display().to_string()))
		.unwrap();
	let backup = PathBuf::from(session.back_up_now().unwrap());
	assert_eq!(backup.parent(), Some(folder.as_path()));
	assert_eq!(
		std::fs::read_dir(&folder).unwrap().count(),
		1,
		"no temporary file is left beside the backup"
	);

	std::fs::remove_dir_all(&folder).unwrap();
	let failed = session.back_up_now();
	assert!(
		matches!(failed, Err(FfiError::Io { .. } | FfiError::Storage { .. })),
		"{failed:?}"
	);
	add(
		&session,
		draft(
			&checking,
			&on(today, 5),
			-2_000,
			"Corner Grocer",
			Some(&groceries),
		),
	);
	let spent = |session: &Session| {
		session
			.budget_month(month(today), today.to_owned())
			.unwrap()
			.groups
			.into_iter()
			.flat_map(|g| g.envelopes)
			.find(|e| e.id == groceries)
			.unwrap()
			.spent
	};
	assert_eq!(spent(&session), -8_123);
	session.close().unwrap();
	let reopened = children::while_no_child_starts(|| {
		block_on(Session::open(scratch.budget(), CancelToken::new())).unwrap()
	});
	reopened
		.budget_month(month(today), today.to_owned())
		.unwrap();
	assert_eq!(
		spent(&reopened),
		-8_123,
		"the edit made after the failed backup is still there"
	);

	let Some(oracle) = oracle() else { return };
	let first = scratch.join("first-backup.zip");
	std::fs::create_dir_all(&folder).unwrap();
	reopened
		.set_backup_folder(Some(folder.display().to_string()))
		.unwrap();
	std::fs::copy(reopened.back_up_now().unwrap(), &first).unwrap();
	let dump = actual_reads(&oracle, &first, today);
	assert_actual_shows_what_veritas_showed(&reopened, &dump, today);
}

fn import_all_features(scratch: &Scratch, today: &str) -> Arc<Session> {
	let session = block_on(Session::import_actual_zip(
		fixture("all-features"),
		scratch.budget(),
		CancelToken::new(),
	))
	.unwrap();
	session
		.budget_month(month(today), today.to_owned())
		.unwrap();
	session
}

fn bulk_snapshots(session: &Session) -> Vec<String> {
	session
		.snapshots()
		.unwrap()
		.into_iter()
		.filter(|s| s.reason == veritas_ffi::SnapshotKind::BulkEdit)
		.map(|s| s.id)
		.collect()
}

/// R-REL-3: a bulk edit snapshots the budget first, and restoring that
/// snapshot brings back the state before it.
fn assert_snapshotted_first(session: &Session, before: &[String]) -> String {
	let after = bulk_snapshots(session);
	let taken: Vec<&String> = after.iter().filter(|id| !before.contains(id)).collect();
	assert_eq!(
		taken.len(),
		1,
		"a bulk edit snapshots the budget first (R-REL-3)"
	);
	taken[0].clone()
}

#[test]
fn moving_a_categorys_history_snapshots_the_budget_first() {
	const TODAY: &str = "2026-11-13";
	let scratch = Scratch::new("move-history");
	let session = import_all_features(&scratch, TODAY);
	let dining = "fffcd498-80e7-4984-b378-87cee6b5625c";
	let groceries = "fc093a41-2d11-4d98-a211-e5db92205f23";
	let rows = every_row(&session);
	let moved = rows
		.iter()
		.filter(|r| r.category_id.as_deref() == Some(dining))
		.count();
	assert!(moved > 1, "Dining Out has history to move");
	let before = bulk_snapshots(&session);

	perform(
		&session,
		Command::DeleteCategoryMovingHistory {
			category_id: dining.to_owned(),
			to_category_id: groceries.to_owned(),
		},
	);
	assert!(
		every_row(&session)
			.iter()
			.all(|r| r.category_id.as_deref() != Some(dining)),
		"every row moved"
	);
	let snapshot = assert_snapshotted_first(&session, &before);
	session.restore_snapshot(snapshot).unwrap();
	assert_eq!(every_row(&session), rows);
}

#[test]
fn applying_next_months_suggestions_snapshots_the_budget_first() {
	const TODAY: &str = "2026-11-13";
	let scratch = Scratch::new("apply-suggestions");
	let session = import_all_features(&scratch, TODAY);
	let cells = |session: &Session| -> Vec<(String, i64)> {
		(0..3)
			.map(|ahead| months_after(month(TODAY), ahead))
			.flat_map(|m| session.budget_month(m, TODAY.to_owned()).unwrap().groups)
			.flat_map(|g| g.envelopes)
			.map(|e| (e.id, e.budgeted))
			.collect()
	};
	let budgeted = cells(&session);
	let before = bulk_snapshots(&session);

	perform(
		&session,
		Command::ApplySuggestions {
			today: TODAY.to_owned(),
		},
	);
	assert_ne!(
		cells(&session),
		budgeted,
		"the suggestions changed the budget"
	);
	let snapshot = assert_snapshotted_first(&session, &before);
	session.restore_snapshot(snapshot).unwrap();
	assert_eq!(cells(&session), budgeted);
}

#[test]
fn merging_payees_snapshots_the_budget_first() {
	const TODAY: &str = "2026-11-13";
	let scratch = Scratch::new("merge-payees");
	let session = import_all_features(&scratch, TODAY);
	let rows = every_row(&session);
	let payees: Vec<String> = session
		.payees()
		.unwrap()
		.into_iter()
		.filter(|p| p.transfer_account_id.is_none() && p.transactions > 0)
		.map(|p| p.id)
		.take(2)
		.collect();
	let before = bulk_snapshots(&session);
	perform(
		&session,
		Command::MergePayees {
			target_payee_id: payees[0].clone(),
			payee_ids: vec![payees[1].clone()],
		},
	);
	assert_ne!(every_row(&session), rows);
	let snapshot = assert_snapshotted_first(&session, &before);
	session.restore_snapshot(snapshot).unwrap();
	assert_eq!(every_row(&session), rows);
}

#[test]
fn finishing_a_reconcile_snapshots_the_budget_first() {
	const TODAY: &str = "2026-11-13";
	let scratch = Scratch::new("finish-reconcile");
	let session = import_all_features(&scratch, TODAY);
	let checking = "0a1c000f-c328-42b8-8966-a4be59a2504b";
	let rows = every_row(&session);
	let cleared = session
		.accounts()
		.unwrap()
		.accounts
		.into_iter()
		.find(|a| a.id == checking)
		.unwrap()
		.cleared;
	let unlocked = rows
		.iter()
		.filter(|r| r.account_id == checking && r.cleared && !r.reconciled)
		.count();
	assert!(unlocked > 1, "cleared rows wait to be locked");
	let before = bulk_snapshots(&session);

	perform(
		&session,
		Command::FinishReconcile {
			account_id: checking.to_owned(),
			statement_balance: cleared,
		},
	);
	assert!(
		every_row(&session)
			.iter()
			.filter(|r| r.account_id == checking && r.cleared)
			.all(|r| r.reconciled),
		"finishing locks every cleared row"
	);
	let snapshot = assert_snapshotted_first(&session, &before);
	session.restore_snapshot(snapshot).unwrap();
	assert_eq!(every_row(&session), rows);
}

#[test]
fn deleting_a_dashboard_snapshots_the_budget_first() {
	const TODAY: &str = "2026-11-13";
	let scratch = Scratch::new("delete-dashboard");
	let session = import_all_features(&scratch, TODAY);
	let pages = |session: &Session| {
		block_on(session.dashboards(TODAY.to_owned(), CancelToken::new()))
			.unwrap()
			.into_iter()
			.map(|page| {
				(
					page.id,
					page.name,
					page.widgets
						.into_iter()
						.map(|w| (w.id, w.kind))
						.collect::<Vec<_>>(),
				)
			})
			.collect::<Vec<_>>()
	};
	let shown = pages(&session);
	assert!(
		shown.len() > 1,
		"all-features has more than one dashboard page"
	);
	let deleted = shown[1].0.clone();
	let before = bulk_snapshots(&session);

	perform(
		&session,
		Command::DeleteDashboard {
			page_id: deleted.clone(),
		},
	);
	assert!(!pages(&session).iter().any(|(id, _, _)| *id == deleted));
	let snapshot = assert_snapshotted_first(&session, &before);
	session.restore_snapshot(snapshot).unwrap();
	assert_eq!(pages(&session), shown, "every page and widget is back");
}

#[test]
fn closing_a_goal_and_moving_its_later_budget_snapshots_the_budget_first() {
	const TODAY: &str = "2026-11-13";
	let scratch = Scratch::new("close-goal-later");
	let session = import_all_features(&scratch, TODAY);
	perform(
		&session,
		Command::CreateGoal {
			name: "Kayak".to_owned(),
			target: 90_000,
			target_date: Some("2027-04-15".to_owned()),
			icon: None,
			color: None,
		},
	);
	let kayak = session
		.goals(TODAY.to_owned())
		.unwrap()
		.goals
		.into_iter()
		.find(|g| g.name == "Kayak")
		.unwrap();
	let months: Vec<Month> = (0..3)
		.map(|ahead| months_after(month(TODAY), ahead))
		.collect();
	for (index, month) in months.iter().enumerate() {
		perform(
			&session,
			Command::SetBudget {
				month: *month,
				category_id: kayak.category_id.clone(),
				amount: 10_000 * (index as i64 + 1),
			},
		);
	}
	let cells = |session: &Session| -> Vec<(String, i64, i64)> {
		months
			.iter()
			.flat_map(|m| session.budget_month(*m, TODAY.to_owned()).unwrap().groups)
			.flat_map(|g| g.envelopes)
			.map(|e| (e.id, e.budgeted, e.available))
			.collect()
	};
	let budgeted = cells(&session);
	let rows = every_row(&session);
	let before = bulk_snapshots(&session);

	perform(
		&session,
		Command::CloseGoal {
			goal_id: kayak.id.clone(),
			release_to_category_id: None,
			also_move_later: true,
		},
	);
	assert_ne!(cells(&session), budgeted, "the later months' budget moved");
	let snapshot = assert_snapshotted_first(&session, &before);
	session.restore_snapshot(snapshot).unwrap();
	assert_eq!(cells(&session), budgeted, "every month's budget is back");
	assert_eq!(every_row(&session), rows);
	assert!(
		session
			.goals(TODAY.to_owned())
			.unwrap()
			.goals
			.iter()
			.any(|g| g.id == kayak.id && g.state != GoalState::Closed),
		"and the goal is open again"
	);
}
