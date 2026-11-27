#![allow(
	clippy::unwrap_used,
	clippy::expect_used,
	clippy::panic,
	reason = "integration tests fail by panicking"
)]

//! The Reports screens through the exported API, the way Swift calls them:
//! never a stale number after any kind of commit, the same numbers
//! veritas-reports gives on a fresh load, and saved reports and widget moves
//! that survive undo, reopen and Actual.

#[path = "support/children.rs"]
mod children;

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::Thread;

use veritas_domain::{BudgetData, CivilDate, ReadOnlyReason, YearMonth};
use veritas_ffi::{
	BalanceSheet, CancelToken, CashFlowColumn, Command, DashboardPage, DrillCategory, Month,
	NetProfitColumn, NetProfitLine, NetProfitReport, NetWorthMonth, NewAccount, NewAccountKind,
	OwnersEquity, ReportDrill, ReportTransaction, SankeyNodeKey, SankeyNodeKind, SankeyReport,
	Session, TaxSummary, TransactionDraft,
};
use veritas_reports::{NodeKind, Period};

const TODAY: &str = "2026-11-13";
const FIRST: Month = Month {
	year: 2025,
	month: 11,
};
const LAST: Month = Month {
	year: 2026,
	month: 11,
};
const PERIOD: (&str, &str) = ("2025-11-01", "2026-11-30");

const CHECKING: &str = "0a1c000f-c328-42b8-8966-a4be59a2504b";
const SAVINGS: &str = "66260e81-beca-4d3d-8ab4-00c8dc2c2ded";
const VISA: &str = "2cf26d00-3bc3-4cd9-a05c-5d9552dd0e63";
const WALLET: &str = "278e3e4d-b445-4df6-ace8-baa01bfd1e0e";
const INCOME: &str = "3c1699a5-522a-435e-86dc-93d900a14f0e";
const GROCERIES: &str = "fc093a41-2d11-4d98-a211-e5db92205f23";
const DINING_OUT: &str = "fffcd498-80e7-4984-b378-87cee6b5625c";
const HOUSEHOLD: &str = "e2dcc389-9fe5-4b1f-93d6-0099cd592c17";
const ACME: &str = "5e14fce3-f9bf-4b33-acda-d252992d9b26";
const BEAN_THERE: &str = "75aba692-9ea4-443c-876d-0b29acecd8db";
const TRATTORIA: &str = "595fbfc6-8cb7-4e42-b56a-a74acd76be9b";
const SPENDING_REPORT: &str = "c7eaad37-4a0f-4f44-8c5e-ca30ff232dd2";
const NET_WORTH_CARD: &str = "a8828813-ceda-452a-8de4-4a24a78c531e";

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
			"ffi-reports-{label}-{}-{}",
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

fn import(scratch: &Scratch, name: &str) -> Arc<Session> {
	let session = block_on(Session::import_actual_zip(
		fixture(name),
		scratch.budget(),
		CancelToken::new(),
	))
	.unwrap();
	pin_today(&session);
	session
}

fn open(scratch: &Scratch) -> Arc<Session> {
	let session = children::while_no_child_starts(|| {
		block_on(Session::open(scratch.budget(), CancelToken::new())).unwrap()
	});
	pin_today(&session);
	session
}

fn pin_today(session: &Session) {
	session.budget_month(LAST, TODAY.to_owned()).unwrap();
}

fn perform(session: &Session, command: Command) {
	session.perform(command).unwrap();
}

/// Everything the Reports screens and the dashboards show for the fixture's
/// whole history, plus the rows behind the expenses line.
#[derive(Debug, PartialEq)]
struct Screens {
	net_profit: NetProfitReport,
	net_worth: Vec<NetWorthMonth>,
	cash_flow: Vec<CashFlowColumn>,
	sankey: SankeyReport,
	balance_sheet: BalanceSheet,
	owners_equity: OwnersEquity,
	tax: TaxSummary,
	dashboards: Vec<DashboardPage>,
	expenses: Vec<ReportTransaction>,
}

fn screens(session: &Session) -> Screens {
	let token = CancelToken::new;
	let (first, last) = (PERIOD.0.to_owned(), PERIOD.1.to_owned());
	Screens {
		net_profit: block_on(session.net_profit(FIRST, LAST, token())).unwrap(),
		net_worth: block_on(session.net_worth(FIRST, LAST, token())).unwrap(),
		cash_flow: block_on(session.cash_flow(FIRST, LAST, token())).unwrap(),
		sankey: block_on(session.sankey(first.clone(), last.clone(), token())).unwrap(),
		balance_sheet: block_on(session.balance_sheet(last.clone(), token())).unwrap(),
		owners_equity: block_on(session.owners_equity(first.clone(), last.clone(), token()))
			.unwrap(),
		tax: block_on(session.tax_summary(2026, token())).unwrap(),
		dashboards: block_on(session.dashboards(TODAY.to_owned(), token())).unwrap(),
		expenses: block_on(session.report_transactions(
			ReportDrill::NetProfit {
				first,
				last,
				line: NetProfitLine::Expenses,
				category: None,
			},
			token(),
		))
		.unwrap(),
	}
}

/// What a second window opened on the budget right now shows. It loads
/// from disk into a session of its own, so nothing it shows is cached.
fn second_window(scratch: &Scratch) -> Screens {
	let window = open(scratch);
	assert!(window.state().unwrap().read_only.is_some());
	let shown = screens(&window);
	window.close().unwrap();
	shown
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
		cleared: true,
		splits: Vec::new(),
	}
}

#[test]
fn every_report_is_fresh_after_each_kind_of_commit_undo_redo_restore_and_reopen() {
	// The classification cache (R-PERF-4) against a second window that
	// loads from disk: after every commit kind the cache must drop for
	// (transactions, accounts, categories and their mappings, payees and
	// their mappings, meta rows) and the ones it keeps (budget cells, notes,
	// custom reports, widgets), the live session shows exactly what a fresh
	// load shows. Steps that move a number are checked to move it.
	let scratch = Scratch::new("fresh");
	let session = import(&scratch, "all-features");
	let mut shown = screens(&session);
	assert_eq!(shown, second_window(&scratch), "on open");

	let mut bonus = entry("bonus", CHECKING, "2026-06-15", 125_000);
	bonus.payee_id = Some(ACME.to_owned());
	bonus.category_id = Some(INCOME.to_owned());
	let mut lunch = entry("lunch", VISA, "2026-07-02", -2_350);
	lunch.payee_id = Some(TRATTORIA.to_owned());
	lunch.category_id = Some(DINING_OUT.to_owned());
	let mut lunch_as_groceries = lunch.clone();
	lunch_as_groceries.category_id = Some(GROCERIES.to_owned());
	let mut to_savings = entry("to-savings", CHECKING, "2026-08-01", -40_000);
	to_savings.payee_id = Some(
		session
			.payees()
			.unwrap()
			.into_iter()
			.find(|p| p.transfer_account_id.as_deref() == Some(SAVINGS))
			.unwrap()
			.id,
	);
	let mut cafe = entry("cafe", WALLET, "2026-09-09", -650);
	cafe.payee_id = Some(BEAN_THERE.to_owned());
	cafe.category_id = Some(DINING_OUT.to_owned());

	let steps: Vec<(&str, Command, bool)> = vec![
		(
			"income added",
			Command::AddTransaction { transaction: bonus },
			true,
		),
		(
			"spending added",
			Command::AddTransaction {
				transaction: lunch.clone(),
			},
			true,
		),
		(
			"spending recategorized",
			Command::EditTransaction {
				transaction_id: "lunch".to_owned(),
				transaction: lunch_as_groceries,
			},
			true,
		),
		(
			"savings transfer added",
			Command::AddTransaction {
				transaction: to_savings,
			},
			true,
		),
		(
			"savings moved off budget",
			Command::SetAccountOffBudget {
				account_id: SAVINGS.to_owned(),
				off_budget: true,
			},
			true,
		),
		(
			"budget cell edited",
			Command::SetBudget {
				month: LAST,
				category_id: GROCERIES.to_owned(),
				amount: 77_700,
			},
			true,
		),
		(
			"dining out deleted into household",
			Command::DeleteCategoryMovingHistory {
				category_id: DINING_OUT.to_owned(),
				to_category_id: HOUSEHOLD.to_owned(),
			},
			true,
		),
		(
			"wallet spending added",
			Command::AddTransaction { transaction: cafe },
			true,
		),
		(
			"payees merged",
			Command::MergePayees {
				target_payee_id: TRATTORIA.to_owned(),
				payee_ids: vec![BEAN_THERE.to_owned()],
			},
			true,
		),
		(
			"reconcile adjustment",
			Command::AddReconcileAdjustment {
				account_id: WALLET.to_owned(),
				statement_balance: 0,
			},
			true,
		),
		(
			"category archived",
			Command::ArchiveCategory {
				category_id: GROCERIES.to_owned(),
			},
			true,
		),
		(
			"account opened",
			Command::CreateAccount {
				account: NewAccount {
					name: "Credit union".to_owned(),
					kind: NewAccountKind::Savings,
					starting_balance: 310_000,
					loan: None,
				},
			},
			true,
		),
		(
			"category note",
			Command::SetCategoryNote {
				category_id: HOUSEHOLD.to_owned(),
				text: "Cleaning and repairs".to_owned(),
			},
			false,
		),
		(
			"transaction deleted",
			Command::DeleteTransaction {
				transaction_id: "bonus".to_owned(),
			},
			true,
		),
	];
	let mut history = vec![shown];
	for (what, command, moves) in &steps {
		let before = history.last().unwrap();
		perform(&session, command.clone());
		shown = screens(&session);
		assert_eq!(shown, second_window(&scratch), "after {what}");
		if *moves {
			assert_ne!(&shown, before, "{what} should change a report");
		} else {
			assert_eq!(&shown, before, "{what} changes no report");
		}
		history.push(shown);
	}

	for (n, (what, _, _)) in steps.iter().enumerate().rev() {
		session.undo().unwrap();
		let undone = screens(&session);
		assert_eq!(undone, history[n], "undoing {what}");
		assert_eq!(undone, second_window(&scratch), "undoing {what}, fresh");
	}
	for (n, (what, _, _)) in steps.iter().enumerate() {
		session.redo().unwrap();
		let redone = screens(&session);
		assert_eq!(redone, history[n + 1], "redoing {what}");
	}
	assert_eq!(screens(&session), second_window(&scratch), "after redo");

	let merge = steps
		.iter()
		.position(|(what, _, _)| *what == "payees merged")
		.unwrap();
	let snapshot = session
		.snapshots()
		.unwrap()
		.into_iter()
		.find(|s| s.reason == veritas_ffi::SnapshotKind::BulkEdit)
		.expect("the merge snapshotted the budget first");
	session.restore_snapshot(snapshot.id).unwrap();
	let restored = screens(&session);
	assert_eq!(
		restored, history[merge],
		"restored to just before the merge"
	);
	assert_eq!(restored, second_window(&scratch), "after restoring, fresh");

	let mut late = entry("late", CHECKING, "2026-10-20", -9_900);
	late.payee_id = Some(TRATTORIA.to_owned());
	late.category_id = Some(GROCERIES.to_owned());
	perform(&session, Command::AddTransaction { transaction: late });
	let added = screens(&session);
	assert_ne!(added, restored);
	assert_eq!(added, second_window(&scratch), "an edit after a restore");
	session.close().unwrap();
	let reopened = open(&scratch);
	assert_eq!(screens(&reopened), added, "after a reopen");
}

#[test]
fn with_a_goal_open_every_edit_still_reaches_the_reports() {
	// While a goal is active, a commit takes the session's goal path, which
	// refreshes the envelope sheet itself. Reports must still follow every
	// edit, undo and redo on that path.
	let scratch = Scratch::new("goal-path");
	let session = import(&scratch, "all-features");
	let created = session
		.perform(Command::CreateGoal {
			name: "New bike".to_owned(),
			target: 90_000,
			target_date: Some("2027-06-30".to_owned()),
			icon: None,
			color: None,
		})
		.unwrap();
	let goal_category = created
		.entities
		.iter()
		.find(|e| e.kind == veritas_ffi::EntityKind::Category && e.id.is_some())
		.and_then(|e| e.id.clone())
		.unwrap();
	assert!(!session.goals(TODAY.to_owned()).unwrap().goals.is_empty());
	let before = screens(&session);
	assert_eq!(before, second_window(&scratch));

	let mut deposit = entry("bike-shop", CHECKING, "2026-10-04", -45_000);
	deposit.payee_id = Some(TRATTORIA.to_owned());
	deposit.category_id = Some(goal_category);
	perform(
		&session,
		Command::AddTransaction {
			transaction: deposit.clone(),
		},
	);
	let added = screens(&session);
	assert_ne!(added, before);
	assert_eq!(
		added,
		second_window(&scratch),
		"after spending from the goal"
	);
	deposit.amount = -30_000;
	perform(
		&session,
		Command::EditTransaction {
			transaction_id: "bike-shop".to_owned(),
			transaction: deposit,
		},
	);
	let edited = screens(&session);
	assert_ne!(edited, added);
	assert_eq!(edited, second_window(&scratch), "after editing it");
	session.undo().unwrap();
	assert_eq!(screens(&session), added, "undo");
	session.undo().unwrap();
	assert_eq!(screens(&session), before, "undo again");
	session.redo().unwrap();
	assert_eq!(screens(&session), added, "redo");
	assert_eq!(screens(&session), second_window(&scratch));
}

/// The budget as it is on disk, loaded by the store with no session.
fn on_disk(scratch: &Scratch) -> BudgetData {
	let db = Path::new(&scratch.budget()).join(veritas_session::DB_FILE);
	let store = veritas_store::Store::open(
		&db,
		veritas_store::Access::ReadOnly(ReadOnlyReason::OpenElsewhere),
	)
	.unwrap();
	store.load().unwrap().data
}

fn date(text: &str) -> CivilDate {
	let parts: Vec<u32> = text.split('-').map(|p| p.parse().unwrap()).collect();
	CivilDate::new(parts[0] as i32, parts[1], parts[2]).unwrap()
}

fn ids(rows: &[veritas_domain::TransactionId]) -> Vec<String> {
	rows.iter().map(ToString::to_string).collect()
}

fn same_column(ours: &NetProfitColumn, theirs: &veritas_reports::NetProfit, label: &str) {
	let line = |line: &veritas_ffi::ReportLine| (line.total, line.count);
	let direct = |line: &veritas_reports::Line| (line.total.minor(), line.rows.len() as u64);
	let section = |section: &veritas_ffi::ReportSection| {
		let categories: Vec<_> = section
			.categories
			.iter()
			.map(|c| (c.category_id.clone(), c.total, c.count))
			.collect();
		(section.total, section.count, categories)
	};
	let direct_section = |section: &veritas_reports::Section| {
		let categories: Vec<_> = section
			.categories
			.iter()
			.map(|c| {
				(
					c.category.as_ref().map(ToString::to_string),
					c.total.minor(),
					c.rows.len() as u64,
				)
			})
			.collect();
		(section.total.minor(), section.rows.len() as u64, categories)
	};
	assert_eq!(
		section(&ours.income),
		direct_section(&theirs.income),
		"{label} income"
	);
	assert_eq!(
		section(&ours.expenses),
		direct_section(&theirs.expenses),
		"{label} expenses"
	);
	assert_eq!(
		section(&ours.debt_paydown),
		direct_section(&theirs.debt_paydown),
		"{label} debt paydown"
	);
	assert_eq!(
		section(&ours.savings),
		direct_section(&theirs.savings),
		"{label} savings"
	);
	assert_eq!(
		line(&ours.net_profit),
		direct(&theirs.net_profit),
		"{label} net profit"
	);
	assert_eq!(
		line(&ours.net_cash_flow),
		direct(&theirs.net_cash_flow),
		"{label} net cash flow"
	);
	assert_eq!(
		line(&ours.unreviewed),
		direct(&theirs.unreviewed),
		"{label} unreviewed"
	);
}

fn node_key(kind: &NodeKind) -> SankeyNodeKey {
	let key = |kind, id: Option<String>| SankeyNodeKey { kind, id };
	match kind {
		NodeKind::IncomeCategory(id) => key(SankeyNodeKind::IncomeCategory, Some(id.to_string())),
		NodeKind::RefundsAndWithdrawals => key(SankeyNodeKind::RefundsAndWithdrawals, None),
		NodeKind::UnreviewedTransfersIn => key(SankeyNodeKind::UnreviewedTransfersIn, None),
		NodeKind::Shortfall => key(SankeyNodeKind::Shortfall, None),
		NodeKind::Income => key(SankeyNodeKind::Income, None),
		NodeKind::CategoryGroup(id) => key(
			SankeyNodeKind::CategoryGroup,
			id.as_ref().map(ToString::to_string),
		),
		NodeKind::Category(id) => key(
			SankeyNodeKind::Category,
			id.as_ref().map(ToString::to_string),
		),
		NodeKind::Savings => key(SankeyNodeKind::Savings, None),
		NodeKind::DebtPaydown => key(SankeyNodeKind::DebtPaydown, None),
		NodeKind::IncomeReversals => key(SankeyNodeKind::IncomeReversals, None),
		NodeKind::UnreviewedTransfers => key(SankeyNodeKind::UnreviewedTransfers, None),
		NodeKind::Unallocated => key(SankeyNodeKind::Unallocated, None),
	}
}

fn drill(session: &Session, drill: ReportDrill) -> Vec<String> {
	block_on(session.report_transactions(drill, CancelToken::new()))
		.unwrap()
		.into_iter()
		.map(|row| row.id)
		.collect()
}

#[test]
fn reports_through_the_ffi_match_veritas_reports_on_a_fresh_load_and_drill_to_their_rows() {
	// After a few edits, so the numbers aren't only the fixture's: Net
	// Profit by month and in total, the Sankey, the balance sheet and
	// owner's equity read the same as veritas-reports called on the budget
	// loaded straight from disk, and every number drills to exactly its rows.
	let scratch = Scratch::new("direct");
	let session = import(&scratch, "all-features");
	let mut bonus = entry("bonus", CHECKING, "2026-06-15", 125_000);
	bonus.payee_id = Some(ACME.to_owned());
	bonus.category_id = Some(INCOME.to_owned());
	perform(&session, Command::AddTransaction { transaction: bonus });
	perform(
		&session,
		Command::SetAccountOffBudget {
			account_id: SAVINGS.to_owned(),
			off_budget: true,
		},
	);
	perform(
		&session,
		Command::DeleteCategoryMovingHistory {
			category_id: DINING_OUT.to_owned(),
			to_category_id: HOUSEHOLD.to_owned(),
		},
	);
	let ours = screens(&session);
	let budget = on_disk(&scratch);
	let classification = veritas_reports::classify(&budget);
	let period = Period::new(date(PERIOD.0), date(PERIOD.1)).unwrap();

	let months = veritas_reports::months(
		YearMonth::new(FIRST.year, FIRST.month).unwrap(),
		YearMonth::new(LAST.year, LAST.month).unwrap(),
	)
	.unwrap();
	assert_eq!(ours.net_profit.months.len(), months.len());
	for (column, month) in ours.net_profit.months.iter().zip(&months) {
		let direct =
			veritas_reports::net_profit(&budget, &classification, Period::month(*month)).unwrap();
		same_column(column, &direct, &format!("{month:?}"));
	}
	let total = veritas_reports::net_profit(&budget, &classification, period).unwrap();
	same_column(&ours.net_profit.total, &total, "total");
	assert_ne!(total.expenses.total.minor(), 0);

	let sankey = veritas_reports::sankey(&budget, &classification, period).unwrap();
	let nodes: Vec<_> = ours
		.sankey
		.nodes
		.iter()
		.map(|n| (n.key.clone(), n.value, n.count))
		.collect();
	let direct_nodes: Vec<_> = sankey
		.nodes
		.iter()
		.map(|n| (node_key(&n.kind), n.value.minor(), n.rows.len() as u64))
		.collect();
	assert_eq!(nodes, direct_nodes, "Sankey nodes");
	let flows: Vec<_> = ours
		.sankey
		.flows
		.iter()
		.map(|f| (f.from as usize, f.to as usize, f.value))
		.collect();
	let direct_flows: Vec<_> = sankey
		.flows
		.iter()
		.map(|f| (f.from, f.to, f.value.minor()))
		.collect();
	assert_eq!(flows, direct_flows, "Sankey flows");

	let sheet = veritas_reports::balance_sheet(&budget, &classification, period.last()).unwrap();
	assert_eq!(
		(
			ours.balance_sheet.total_assets,
			ours.balance_sheet.total_liabilities,
			ours.balance_sheet.net_worth
		),
		(
			sheet.total_assets.minor(),
			sheet.total_liabilities.minor(),
			sheet.net_worth.minor()
		)
	);
	let accounts = |sections: &[veritas_ffi::SheetSection]| -> Vec<(String, i64)> {
		sections
			.iter()
			.flat_map(|s| &s.accounts)
			.map(|a| (a.account_id.clone(), a.amount))
			.collect()
	};
	let direct_accounts =
		|sections: &[veritas_reports::AccountGroupSection]| -> Vec<(String, i64)> {
			sections
				.iter()
				.flat_map(|s| &s.accounts)
				.map(|a| (a.account.to_string(), a.amount.minor()))
				.collect()
		};
	assert_eq!(
		accounts(&ours.balance_sheet.assets),
		direct_accounts(&sheet.assets)
	);
	assert_eq!(
		accounts(&ours.balance_sheet.liabilities),
		direct_accounts(&sheet.liabilities)
	);

	let equity = veritas_reports::owners_equity(&budget, &classification, period).unwrap();
	let line = |l: &veritas_ffi::ReportLine| (l.total, l.count);
	let direct = |l: &veritas_reports::Line| (l.total.minor(), l.rows.len() as u64);
	let ffi = &ours.owners_equity;
	assert_eq!(
		(ffi.opening, ffi.closing),
		(equity.opening.minor(), equity.closing.minor())
	);
	assert_eq!(
		[
			line(&ffi.net_income),
			line(&ffi.capitalized_transfers),
			line(&ffi.starting_balances),
			line(&ffi.reconciliation_adjustments),
			line(&ffi.revaluations),
			line(&ffi.unpaired_transfers),
			line(&ffi.in_transit),
		],
		equity.lines().map(direct)
	);
	assert_eq!(ffi.unexplained, 0);
	assert_eq!(ffi.net_income.total, ours.net_profit.total.net_profit.total);

	for category in &total.expenses.categories {
		let shown = drill(
			&session,
			ReportDrill::NetProfit {
				first: PERIOD.0.to_owned(),
				last: PERIOD.1.to_owned(),
				line: NetProfitLine::Expenses,
				category: Some(match &category.category {
					Some(id) => DrillCategory::Category { id: id.to_string() },
					None => DrillCategory::Uncategorized,
				}),
			},
		);
		assert_eq!(shown, ids(&category.rows), "{:?}", category.category);
	}
	for (lines, section) in [
		(NetProfitLine::Income, &total.income.rows),
		(NetProfitLine::Savings, &total.savings.rows),
		(NetProfitLine::DebtPaydown, &total.debt_paydown.rows),
		(NetProfitLine::Unreviewed, &total.unreviewed.rows),
	] {
		let shown = drill(
			&session,
			ReportDrill::NetProfit {
				first: PERIOD.0.to_owned(),
				last: PERIOD.1.to_owned(),
				line: lines,
				category: None,
			},
		);
		assert_eq!(&shown, &ids(section), "{lines:?}");
	}
	for node in sankey
		.nodes
		.iter()
		.filter(|n| n.kind != NodeKind::Shortfall)
	{
		let shown = drill(
			&session,
			ReportDrill::SankeyNode {
				first: PERIOD.0.to_owned(),
				last: PERIOD.1.to_owned(),
				node: node_key(&node.kind),
			},
		);
		assert_eq!(shown, ids(&node.rows), "{:?}", node.kind);
	}
}

#[test]
fn a_drilled_transfer_names_its_other_account_as_the_register_does() {
	// v18 (C1): every row behind a Net Profit line carries the account on
	// the other side of its transfer, the same account the register shows,
	// so the drill-down can draw "To X" or "From X"; other rows carry none.
	let scratch = Scratch::new("drill-transfers");
	let session = import(&scratch, "all-features");
	let account_name = |id: &str| {
		session
			.accounts()
			.unwrap()
			.accounts
			.into_iter()
			.find(|account| account.id == id)
			.map(|account| account.name)
	};
	let mut transfers = 0;
	let mut others = 0;
	for line in [
		NetProfitLine::Income,
		NetProfitLine::Expenses,
		NetProfitLine::Savings,
		NetProfitLine::DebtPaydown,
		NetProfitLine::Unreviewed,
	] {
		let rows: Vec<ReportTransaction> = block_on(session.report_transactions(
			ReportDrill::NetProfit {
				first: PERIOD.0.to_owned(),
				last: PERIOD.1.to_owned(),
				line,
				category: None,
			},
			CancelToken::new(),
		))
		.unwrap();
		for row in rows {
			let register = match &row.parent_id {
				Some(parent) => {
					session
						.register_row(parent.clone())
						.unwrap()
						.lines
						.into_iter()
						.find(|split| split.id == row.id)
						.unwrap()
						.transfer_account_id
				}
				None => {
					session
						.register_row(row.id.clone())
						.unwrap()
						.transfer_account_id
				}
			};
			assert_eq!(row.transfer_account_id, register, "{line:?} {}", row.id);
			match &row.transfer_account_id {
				Some(other) => {
					transfers += 1;
					assert_ne!(*other, row.account_id, "{}", row.id);
					assert_eq!(
						row.payee,
						account_name(other),
						"the payee names that account"
					);
				}
				None => others += 1,
			}
		}
	}
	assert!(
		transfers > 0 && others > 0,
		"{transfers} transfers, {others} others"
	);
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

/// Actual imports and loads `zip`, then dumps its tables.
fn actual_reads(oracle: &Path, zip: &str) -> serde_json::Value {
	let output = children::output(
		std::process::Command::new("node")
			.current_dir(oracle)
			.arg("src/main.ts")
			.arg("dump")
			.arg(zip)
			.args(["--today", TODAY]),
	)
	.expect("node runs");
	assert!(
		output.status.success(),
		"Actual couldn't read {zip}: {}",
		String::from_utf8_lossy(&output.stderr)
	);
	serde_json::from_slice(&output.stdout).unwrap()
}

fn table(dump: &serde_json::Value, name: &str) -> Vec<serde_json::Map<String, serde_json::Value>> {
	dump["tables"][name]["rows"]
		.as_array()
		.unwrap()
		.iter()
		.map(|row| row.as_object().unwrap().clone())
		.collect()
}

fn frame(session: &Session, id: &str) -> (i64, i64, i64, i64) {
	let pages = block_on(session.dashboards(TODAY.to_owned(), CancelToken::new())).unwrap();
	let widget = pages
		.iter()
		.flat_map(|p| &p.widgets)
		.find(|w| w.id == id)
		.unwrap();
	(widget.x, widget.y, widget.width, widget.height)
}

fn widget_name(session: &Session, id: &str) -> Option<String> {
	block_on(session.dashboards(TODAY.to_owned(), CancelToken::new()))
		.unwrap()
		.into_iter()
		.flat_map(|p| p.widgets)
		.find(|w| w.id == id)
		.map(|w| match w.view {
			veritas_ffi::WidgetView::CustomReport { report } => Some(report.settings.name),
			_ => w.config.name,
		})
		.unwrap()
}

const SPENDING_WIDGET: &str = "8de603a2-58a0-411b-8e1c-cca26d049f0c";

#[test]
fn a_saved_report_and_a_moved_widget_survive_undo_reopen_and_actual() {
	// R-REP-4: the editor saves a report and the dashboard moves a card.
	// Each is one undo step, both stay after a reopen, every view shows the
	// new settings straight away, and Actual reads back the fixture's rows
	// with only the edited columns changed.
	let scratch = Scratch::new("saved");
	let session = import(&scratch, "all-features");
	let view = |session: &Session| {
		block_on(session.custom_report(
			SPENDING_REPORT.to_owned(),
			TODAY.to_owned(),
			CancelToken::new(),
		))
		.unwrap()
	};
	let original = view(&session);
	let original_frame = frame(&session, NET_WORTH_CARD);
	assert_eq!(original_frame, (0, 2, 6, 2));

	let mut edited = original.settings.clone();
	edited.name = "  Where it went ".to_owned();
	edited.graph_type = Some("DonutGraph".to_owned());
	edited.interval = veritas_ffi::ReportIntervalUnit::Yearly;
	edited.show_empty = true;
	edited.total_mode = false;
	let preview = block_on(session.preview_custom_report(
		edited.clone(),
		TODAY.to_owned(),
		CancelToken::new(),
	))
	.unwrap();
	perform(
		&session,
		Command::SaveCustomReport {
			settings: edited.clone(),
		},
	);
	let saved = view(&session);
	assert_eq!(saved.settings.name, "Where it went");
	assert_eq!(
		saved, preview,
		"the saved report shows what the editor previewed"
	);
	assert_ne!(saved.data, original.data);
	assert_eq!(
		widget_name(&session, SPENDING_WIDGET).as_deref(),
		Some("Where it went"),
		"the dashboard card follows the rename"
	);
	perform(
		&session,
		Command::PlaceWidget {
			widget_id: NET_WORTH_CARD.to_owned(),
			x: 11,
			y: 14,
			width: 1,
			height: 3,
		},
	);
	assert_eq!(
		frame(&session, NET_WORTH_CARD),
		(9, 14, 3, 3),
		"clamped to the minimum width, inside the 12-column grid"
	);

	session.undo().unwrap();
	assert_eq!(frame(&session, NET_WORTH_CARD), original_frame);
	session.undo().unwrap();
	assert_eq!(view(&session), original);
	assert_eq!(
		widget_name(&session, SPENDING_WIDGET).as_deref(),
		Some("Spending by category")
	);
	session.redo().unwrap();
	session.redo().unwrap();
	assert_eq!(view(&session), saved);
	assert_eq!(frame(&session, NET_WORTH_CARD), (9, 14, 3, 3));
	session.close().unwrap();

	let session = open(&scratch);
	assert_eq!(view(&session), saved, "after a reopen");
	assert_eq!(frame(&session, NET_WORTH_CARD), (9, 14, 3, 3));
	let zip = scratch.0.join("saved.zip").display().to_string();
	block_on(session.export_actual_zip(zip.clone(), CancelToken::new())).unwrap();
	let Some(oracle) = oracle() else {
		return;
	};
	let before = actual_reads(&oracle, &fixture("all-features"));
	let after = actual_reads(&oracle, &zip);
	let changed = |table_name: &str, id: &str| -> Vec<(String, serde_json::Value)> {
		let old = table(&before, table_name);
		let new = table(&after, table_name);
		assert_eq!(old.len(), new.len(), "{table_name} rows");
		for (old_row, new_row) in old.iter().zip(&new) {
			if old_row["id"] != id {
				assert_eq!(old_row, new_row, "{table_name} {}", old_row["id"]);
			}
		}
		let old_row = old.iter().find(|r| r["id"] == id).unwrap();
		let new_row = new.iter().find(|r| r["id"] == id).unwrap();
		new_row
			.iter()
			.filter(|(column, value)| old_row.get(*column) != Some(value))
			.map(|(column, value)| (column.clone(), value.clone()))
			.collect()
	};
	assert_eq!(
		changed("custom_reports", SPENDING_REPORT),
		[
			("graph_type".to_owned(), serde_json::json!("DonutGraph")),
			("interval".to_owned(), serde_json::json!("Yearly")),
			("mode".to_owned(), serde_json::json!("time")),
			("name".to_owned(), serde_json::json!("Where it went")),
			("show_empty".to_owned(), serde_json::json!(1)),
		]
	);
	assert_eq!(
		changed("dashboard", NET_WORTH_CARD),
		[
			("height".to_owned(), serde_json::json!(3)),
			("width".to_owned(), serde_json::json!(3)),
			("x".to_owned(), serde_json::json!(9)),
			("y".to_owned(), serde_json::json!(14)),
		]
	);
}

const NET_WORTH_REPORT: &str = "e920280b-3c05-4a11-87a1-c036aac6bb68";

#[test]
fn names_that_differ_only_by_case_both_save_and_a_widget_dragged_off_the_grid_lands_on_it() {
	// Actual's `reportNameExists` compares with SQLite's `=`, which is
	// case-sensitive, so "net worth over time" can sit beside "Net worth
	// over time" but an exact repeat can't. A widget dragged past the
	// top-left corner lands at 0,0, and past the right edge at the last
	// column it fits. Actual reads all of it back.
	let scratch = Scratch::new("names-and-edges");
	let session = import(&scratch, "all-features");
	let settings = |id: &str| {
		block_on(session.custom_report(id.to_owned(), TODAY.to_owned(), CancelToken::new()))
			.unwrap()
			.settings
	};
	let save = |settings: veritas_ffi::CustomReportSettings| {
		session.perform(Command::SaveCustomReport { settings })
	};

	let mut spending = settings(SPENDING_REPORT);
	spending.name = "net worth over time".to_owned();
	save(spending.clone()).unwrap();
	let mut net_worth = settings(NET_WORTH_REPORT);
	assert_eq!(net_worth.name, "Net worth over time");
	net_worth.name = "NET WORTH OVER TIME".to_owned();
	save(net_worth.clone()).unwrap();
	net_worth.name = "net worth over time".to_owned();
	assert!(matches!(
		save(net_worth.clone()),
		Err(veritas_ffi::FfiError::Invalid { .. })
	));
	net_worth.name = " net worth over time ".to_owned();
	assert!(
		matches!(
			save(net_worth.clone()),
			Err(veritas_ffi::FfiError::Invalid { .. })
		),
		"saved trimmed, it would repeat the other name exactly"
	);
	spending.name = "  net worth over time  ".to_owned();
	save(spending.clone()).expect("a report keeps its own name");
	let mut names: Vec<String> = session
		.custom_reports()
		.unwrap()
		.into_iter()
		.map(|r| r.name)
		.collect();
	names.sort();
	assert_eq!(names, ["NET WORTH OVER TIME", "net worth over time"]);

	let place = |x, y, width, height| {
		perform(
			&session,
			Command::PlaceWidget {
				widget_id: NET_WORTH_CARD.to_owned(),
				x,
				y,
				width,
				height,
			},
		);
		frame(&session, NET_WORTH_CARD)
	};
	assert_eq!(
		place(-4, -2, 6, 2),
		(0, 0, 6, 2),
		"dragged past the top-left"
	);
	assert_eq!(
		place(i64::MIN, i64::MIN, 0, 0),
		(0, 0, 3, 2),
		"the extremes"
	);
	assert_eq!(place(40, 3, 20, 2), (0, 3, 12, 2), "wider than the grid");
	assert_eq!(place(10, 3, 4, 2), (8, 3, 4, 2), "past the right edge");
	session.undo().unwrap();
	assert_eq!(frame(&session, NET_WORTH_CARD), (0, 3, 12, 2));

	let zip = scratch.0.join("edges.zip").display().to_string();
	block_on(session.export_actual_zip(zip.clone(), CancelToken::new())).unwrap();
	let Some(oracle) = oracle() else {
		return;
	};
	let actual = actual_reads(&oracle, &zip);
	let mut theirs: Vec<String> = table(&actual, "custom_reports")
		.iter()
		.filter(|r| r["tombstone"] == 0)
		.map(|r| r["name"].as_str().unwrap().to_owned())
		.collect();
	theirs.sort();
	assert_eq!(theirs, names);
	let card = table(&actual, "dashboard")
		.into_iter()
		.find(|w| w["id"] == NET_WORTH_CARD)
		.unwrap();
	assert_eq!(
		(&card["x"], &card["y"], &card["width"], &card["height"]),
		(
			&serde_json::json!(0),
			&serde_json::json!(3),
			&serde_json::json!(12),
			&serde_json::json!(2)
		)
	);
}
