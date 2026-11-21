#![allow(
	clippy::unwrap_used,
	clippy::expect_used,
	clippy::panic,
	reason = "integration tests fail by panicking"
)]

//! Wrapped, the statements' header and income statement, and tax setup
//! through the exported API (R-WRAP-1, R-WRAP-2, R-REP-7, R-REP-8). Wrapped's
//! net-worth change and category totals are checked against Actual's own
//! summary card code; the header name and tax mappings against a reopen, undo
//! and Actual reading the exported file.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::Thread;

use serde_json::{Value, json};
use veritas_ffi::{
	CancelToken, Command, FfiError, Month, Session, TaxBucket, TaxSummary, TaxTagMeaning,
	TransactionDraft, WrappedFact, WrappedReport,
};

const TODAY: &str = "2026-11-13";
const NOVEMBER: Month = Month {
	year: 2026,
	month: 11,
};

const CHECKING: &str = "0a1c000f-c328-42b8-8966-a4be59a2504b";
const INCOME: &str = "3c1699a5-522a-435e-86dc-93d900a14f0e";
const GROCERIES: &str = "fc093a41-2d11-4d98-a211-e5db92205f23";
const HOUSEHOLD: &str = "e2dcc389-9fe5-4b1f-93d6-0099cd592c17";

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
			"ffi-insights-{label}-{}-{}",
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

	fn join(&self, name: &str) -> String {
		self.0.join(name).display().to_string()
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

fn import(scratch: &Scratch) -> Arc<Session> {
	let session = block_on(Session::import_actual_zip(
		fixture("all-features"),
		scratch.budget(),
		CancelToken::new(),
	))
	.unwrap();
	session.budget_month(NOVEMBER, TODAY.to_owned()).unwrap();
	session
}

fn reopen(scratch: &Scratch, session: Arc<Session>) -> Arc<Session> {
	session.close().unwrap();
	let session = block_on(Session::open(scratch.budget(), CancelToken::new())).unwrap();
	session.budget_month(NOVEMBER, TODAY.to_owned()).unwrap();
	session
}

fn export(session: &Session, zip: &str) {
	block_on(session.export_actual_zip(zip.to_owned(), CancelToken::new())).unwrap();
}

fn perform(session: &Session, command: Command) {
	session.perform(command).unwrap();
}

fn refused(session: &Session, command: Command) {
	assert!(
		matches!(session.perform(command), Err(FfiError::Invalid { .. })),
		"expected Invalid"
	);
}

fn entry(account: &str, date: &str, amount: i64, category: &str, notes: &str) -> Command {
	Command::AddTransaction {
		transaction: TransactionDraft {
			id: None,
			account_id: account.to_owned(),
			date: date.to_owned(),
			amount,
			payee_id: None,
			payee_name: None,
			category_id: Some(category.to_owned()),
			notes: (!notes.is_empty()).then(|| notes.to_owned()),
			cleared: true,
			splits: Vec::new(),
		},
	}
}

fn wrapped(session: &Session, year: i32) -> WrappedReport {
	block_on(session.wrapped(year, TODAY.to_owned(), CancelToken::new())).unwrap()
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

fn node(oracle: &Path, args: &[&str]) -> Value {
	let output = std::process::Command::new("node")
		.current_dir(oracle)
		.arg("src/main.ts")
		.args(args)
		.args(["--today", TODAY])
		.output()
		.expect("node runs");
	assert!(
		output.status.success(),
		"the oracle failed on {args:?}: {}",
		String::from_utf8_lossy(&output.stderr)
	);
	serde_json::from_slice(&output.stdout).unwrap()
}

fn veritas_rows(dump: &Value, table: &str, key: &str) -> Vec<(String, String)> {
	let mut rows: Vec<(String, String)> = dump["tables"][table]["rows"]
		.as_array()
		.unwrap()
		.iter()
		.map(|row| {
			let mut columns = row.as_object().unwrap().iter();
			let value = columns
				.find(|(name, _)| name.as_str() != key)
				.map(|(_, value)| value.as_str().unwrap_or_default().to_owned())
				.unwrap_or_default();
			(row[key].as_str().unwrap().to_owned(), value)
		})
		.collect();
	rows.sort();
	rows
}

/// A summary card summing every row from `first`'s month to `last`, narrowed
/// by `conditions`, as Actual's dashboard would compute it.
fn summary_request(id: &str, first: &str, last: &str, conditions: Value) -> Value {
	json!({
		"id": id,
		"type": "summary-card",
		"meta": {
			"timeFrame": {"mode": "static", "start": first, "end": last},
			"conditions": conditions,
		},
		"firstDayOfWeekIdx": "0",
	})
}

#[test]
fn wrapped_agrees_with_the_statements_and_with_actuals_own_sums() {
	let scratch = Scratch::new("wrapped");
	let session = import(&scratch);
	perform(&session, entry(CHECKING, "2026-03-05", 1500, GROCERIES, ""));
	perform(
		&session,
		entry(CHECKING, "2026-04-09", -2500, HOUSEHOLD, "lamp"),
	);

	let years = [(2025, "2025-12-31"), (2026, TODAY)];
	let mut requests = Vec::new();
	let mut expected = Vec::new();
	for (year, last) in years {
		let first = format!("{year}-01-01");
		let report = wrapped(&session, year);
		assert_eq!(
			(report.first.as_str(), report.last.as_str()),
			(first.as_str(), last)
		);
		assert!(report.has_data, "{year}");

		let statement =
			block_on(session.income_statement(first.clone(), last.to_owned(), CancelToken::new()))
				.unwrap();
		assert_eq!(statement.total_revenue.total, report.total_in, "{year}");
		assert_eq!(statement.total_expenses.total, report.total_out, "{year}");
		assert_eq!(statement.net_income.total, report.net, "{year}");
		for groups in [&statement.revenue, &statement.expenses] {
			for group in groups {
				let categories: i64 = group.categories.iter().map(|line| line.total).sum();
				assert_eq!(categories, group.total);
			}
		}
		assert!(
			statement
				.expenses
				.iter()
				.rev()
				.skip(1)
				.all(|group| group.group_id.is_some()),
			"uncategorized spending comes last"
		);

		let closing = block_on(session.balance_sheet(last.to_owned(), CancelToken::new())).unwrap();
		assert_eq!(closing.net_worth, report.net_worth_end, "{year}");
		let equity =
			block_on(session.owners_equity(first.clone(), last.to_owned(), CancelToken::new()))
				.unwrap();
		assert_eq!(equity.opening, report.net_worth_start, "{year}");
		assert_eq!(equity.closing - equity.opening, report.net_worth_change);
		assert_eq!(equity.unexplained, 0);

		let shares: i64 = report
			.top_categories
			.iter()
			.map(|category| category.share)
			.sum::<i64>()
			+ report.shares.other_categories.unwrap_or(0);
		assert_eq!(shares, 1000, "{year}: categories and Everything else");
		let group_shares: i64 = report
			.top_groups
			.iter()
			.map(|group| group.share)
			.sum::<i64>()
			+ report.shares.other_groups.unwrap_or(0);
		assert_eq!(group_shares, 1000, "{year}: groups and Everything else");
		assert_eq!(
			report
				.shares
				.out_vs_in
				.zip(report.shares.net)
				.map(|(out, net)| out + net),
			(report.total_in > 0).then_some(1000)
		);
		assert!(report.fun_facts.iter().all(|fact| match fact {
			WrappedFact::TopCategoryShare {
				category_id,
				tenths,
			} =>
				*category_id == report.top_categories[0].category_id
					&& *tenths == report.top_categories[0].share,
			WrappedFact::NoSpendShare {
				days,
				evaluated_days,
				..
			} => days <= evaluated_days,
			_ => true,
		}));

		requests.push(summary_request(
			&format!("worth-{year}"),
			&first,
			last,
			json!([]),
		));
		expected.push((format!("worth-{year}"), report.net_worth_change));
		for category in &report.top_categories {
			let id = format!("category-{year}-{}", category.category_id);
			requests.push(summary_request(
				&id,
				&first,
				last,
				json!([{"field": "category", "op": "is", "value": category.category_id, "type": "id"}]),
			));
			expected.push((id, -category.total));
		}
	}
	assert!(wrapped(&session, 2026).no_spend.current.is_some());
	assert!(matches!(
		block_on(session.wrapped(2027, TODAY.to_owned(), CancelToken::new())),
		Err(FfiError::Invalid { .. })
	));

	let Some(oracle) = oracle() else {
		return;
	};
	let zip = scratch.join("wrapped.zip");
	export(&session, &zip);
	let file = scratch.join("summaries.json");
	std::fs::write(&file, Value::Array(requests).to_string()).unwrap();
	let theirs = node(&oracle, &["widgets", &zip, &file]);
	for (id, ours) in expected {
		let result = &theirs[id.as_str()];
		assert!(result.get("error").is_none(), "{id}: {result}");
		assert_eq!(result["data"]["total"], json!(ours), "{id}");
	}
}

fn month_after(month: &Month) -> Month {
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

#[test]
fn wrapped_monthly_spending_is_each_months_expenses_and_its_share_of_out() {
	// v18, FC-8's monthly series: one bar per month of the period, each the
	// income statement's expenses for that month (CLS-6 expense_total), with
	// its share of total out rounded on its own.
	let scratch = Scratch::new("wrapped-months");
	let session = import(&scratch);
	perform(
		&session,
		entry(CHECKING, "2026-03-05", 1500, GROCERIES, "refund"),
	);
	for year in [2025, 2026] {
		let report = wrapped(&session, year);
		assert!(!report.months.is_empty(), "{year}");
		let last = &report.months.last().unwrap().month;
		assert_eq!(
			format!("{}-{:02}", last.year, last.month),
			report.last[..7],
			"{year} runs to the period's last month"
		);
		for pair in report.months.windows(2) {
			assert_eq!(
				pair[1].month,
				month_after(&pair[0].month),
				"{year}: no gaps"
			);
		}
		for bar in &report.months {
			let first = format!("{}-{:02}-01", bar.month.year, bar.month.month);
			let end = {
				let next = month_after(&bar.month);
				let next_first = chrono::NaiveDate::from_ymd_opt(next.year, next.month, 1).unwrap();
				let month_end = next_first
					.pred_opt()
					.unwrap()
					.format("%Y-%m-%d")
					.to_string();
				month_end.min(report.last.clone())
			};
			let statement =
				block_on(session.income_statement(first.clone(), end, CancelToken::new())).unwrap();
			assert_eq!(bar.total, statement.total_expenses.total, "{year} {first}");
			let expected_share = (report.total_out > 0).then(|| {
				let scaled = i128::from(bar.total) * 1000;
				let whole = i128::from(report.total_out);
				let rounded = (scaled.abs() * 2 + whole) / (whole * 2);
				i64::try_from(if scaled < 0 { -rounded } else { rounded }).unwrap()
			});
			assert_eq!(bar.share, expected_share, "{year} {first}");
		}
		if report.months.len() == 12 {
			let sum: i64 = report.months.iter().map(|bar| bar.total).sum();
			assert_eq!(sum, report.total_out, "{year}: a whole year adds up to out");
		}
		let biggest = report.biggest_month.as_ref().unwrap();
		assert!(
			report
				.months
				.iter()
				.any(|bar| bar.month == biggest.month && bar.total == biggest.total),
			"{year}: the biggest month is one of the bars"
		);
	}
}

#[test]
fn wrapped_years_lists_exactly_the_years_with_data_and_pins_today() {
	let (_scratch, session) = fresh("wrapped-years", "USD");
	let years = |today: &str| {
		block_on(session.wrapped_years(today.to_owned(), CancelToken::new())).unwrap()
	};
	assert_eq!(years(TODAY), Vec::<i32>::new(), "a new budget has no rows");
	let checking = new_account(
		&session,
		"Checking",
		veritas_ffi::NewAccountKind::Checking,
		10_000,
		None,
	);
	let category = session
		.budget_month(NOVEMBER, TODAY.to_owned())
		.unwrap()
		.groups
		.into_iter()
		.find(|group| !group.is_income)
		.and_then(|group| group.envelopes.into_iter().next())
		.unwrap()
		.id;
	perform(
		&session,
		entry(&checking, "2024-03-05", -2500, &category, ""),
	);
	perform(
		&session,
		entry(&checking, "2026-12-20", -2500, &category, "later"),
	);
	assert_eq!(years(TODAY), [2026, 2024]);
	for year in 2024..=2026 {
		assert_eq!(
			years(TODAY).contains(&year),
			wrapped(&session, year).has_data,
			"{year}: the picker and No data agree"
		);
	}

	assert_eq!(years("2027-06-01"), [2026, 2024], "2027 has nothing yet");
	let savings = new_account(
		&session,
		"Savings",
		veritas_ffi::NewAccountKind::Savings,
		5_000,
		None,
	);
	assert_eq!(
		years("2027-06-01"),
		[2027, 2026, 2024],
		"the new account's starting balance is dated the pinned day, in 2027"
	);
	assert!(!savings.is_empty());
}

fn actual_tables(dump: &Value) -> Value {
	let mut dump = dump.clone();
	if let Some(tables) = dump["tables"].as_object_mut() {
		tables.retain(|name, _| !name.starts_with("veritas_"));
	}
	dump
}

#[test]
fn a_goal_icon_is_one_emoji_one_undo_step_and_never_reaches_actuals_tables() {
	let (scratch, session) = fresh("goal-icon", "USD");
	let goal = created_id(
		&session,
		Command::CreateGoal {
			name: "Trip".to_owned(),
			target: 50_000,
			target_date: None,
			icon: None,
			color: None,
		},
		veritas_ffi::EntityKind::Goal,
	);
	let icon = |session: &Session| {
		session
			.goals(TODAY.to_owned())
			.unwrap()
			.goals
			.into_iter()
			.find(|summary| summary.id == goal)
			.unwrap()
			.icon
	};
	let set = |session: &Session, icon: Option<&str>| {
		session.perform(Command::SetGoalIcon {
			goal_id: goal.clone(),
			icon: icon.map(str::to_owned),
		})
	};
	let before_zip = scratch.join("before.zip");
	export(&session, &before_zip);
	let month_before = session.budget_month(NOVEMBER, TODAY.to_owned()).unwrap();

	let change = set(&session, Some("✈️")).unwrap();
	assert!(
		change
			.entities
			.iter()
			.any(|entity| entity.kind == veritas_ffi::EntityKind::Goal
				&& entity.id.as_deref() == Some(goal.as_str()))
	);
	assert_eq!(icon(&session).as_deref(), Some("✈️"));
	assert_eq!(
		session.budget_month(NOVEMBER, TODAY.to_owned()).unwrap(),
		month_before,
		"an icon moves no money"
	);
	for refused in ["ab", "🎯🎯", "✈️ ", "laptopcomputer", "👨‍👩‍👧", "🇯", "1"]
	{
		assert!(
			matches!(set(&session, Some(refused)), Err(FfiError::Invalid { .. })),
			"{refused:?}"
		);
	}
	assert_eq!(
		icon(&session).as_deref(),
		Some("✈️"),
		"a refusal changes nothing"
	);
	assert!(
		set(&session, Some("✈️")).unwrap().entities.is_empty(),
		"the same icon writes nothing"
	);

	set(&session, Some("👩🏽‍💻")).unwrap();
	session.undo().unwrap();
	assert_eq!(icon(&session).as_deref(), Some("✈️"), "one undo step");
	session.undo().unwrap();
	assert_eq!(icon(&session), None);
	session.redo().unwrap();
	assert_eq!(icon(&session).as_deref(), Some("✈️"));

	perform(
		&session,
		Command::CloseGoal {
			goal_id: goal.clone(),
			release_to_category_id: None,
			also_move_later: false,
		},
	);
	assert_eq!(icon(&session).as_deref(), Some("✈️"), "closing keeps it");
	set(&session, Some("🇯🇵")).unwrap();
	assert_eq!(
		icon(&session).as_deref(),
		Some("🇯🇵"),
		"a closed goal can change it"
	);
	let session = reopen(&scratch, session);
	assert_eq!(icon(&session).as_deref(), Some("🇯🇵"));
	set(&session, Some("")).unwrap();
	assert_eq!(icon(&session), None, "empty clears, like nil");
	set(&session, Some("🎯")).unwrap();
	set(&session, None).unwrap();
	assert_eq!(icon(&session), None);
	assert!(matches!(
		session.perform(Command::SetGoalIcon {
			goal_id: "no-such-goal".to_owned(),
			icon: Some("🎯".to_owned()),
		}),
		Err(FfiError::NotFound { .. })
	));

	let Some(oracle) = oracle() else {
		return;
	};
	session.undo().unwrap();
	assert_eq!(icon(&session).as_deref(), Some("🎯"));
	let after_zip = scratch.join("after.zip");
	export(&session, &after_zip);
	let before = node(&oracle, &["dump", &before_zip]);
	let after = node(&oracle, &["dump", &after_zip]);
	assert!(
		after["tables"]["veritas_goal"].to_string().contains("🎯"),
		"the icon is in veritas_goal"
	);
	let holds_icon = |dump: &Value| dump.to_string().contains("🎯");
	assert!(
		!holds_icon(&actual_tables(&after)),
		"no Actual table holds the icon"
	);
	assert!(!holds_icon(&before));
}

#[test]
fn the_entity_name_is_one_undo_step_survives_a_reopen_and_actual_reads_the_file() {
	let scratch = Scratch::new("entity");
	let session = import(&scratch);
	let budget_name = session.overview().unwrap().budget_name.unwrap_or_default();
	let entity = session.statement_entity().unwrap();
	assert_eq!(
		(entity.name, entity.custom_name),
		(budget_name.clone(), None)
	);

	perform(
		&session,
		Command::SetEntityName {
			name: "  Household, LLC ".to_owned(),
		},
	);
	let entity = session.statement_entity().unwrap();
	assert_eq!(entity.name, "Household, LLC");
	assert_eq!(entity.custom_name.as_deref(), Some("Household, LLC"));
	refused(
		&session,
		Command::SetEntityName {
			name: "x".repeat(101),
		},
	);
	refused(
		&session,
		Command::SetEntityName {
			name: "Household\nLLC".to_owned(),
		},
	);
	perform(
		&session,
		Command::SetEntityName {
			name: "é".repeat(100),
		},
	);
	session.undo().unwrap();
	assert_eq!(session.statement_entity().unwrap().name, "Household, LLC");
	session.undo().unwrap();
	assert_eq!(session.statement_entity().unwrap().name, budget_name);
	session.redo().unwrap();

	let session = reopen(&scratch, session);
	assert_eq!(session.statement_entity().unwrap().name, "Household, LLC");

	let Some(oracle) = oracle() else {
		return;
	};
	let zip = scratch.join("named.zip");
	export(&session, &zip);
	let dump = node(&oracle, &["dump", &zip]);
	assert_eq!(
		veritas_rows(&dump, "veritas_meta", "key"),
		[
			("entity_name".to_owned(), "Household, LLC".to_owned()),
			("schema_version".to_owned(), "1".to_owned()),
		]
	);

	perform(
		&session,
		Command::SetEntityName {
			name: "   ".to_owned(),
		},
	);
	let entity = session.statement_entity().unwrap();
	assert_eq!((entity.name, entity.custom_name), (budget_name, None));
	let cleared = scratch.join("cleared.zip");
	export(&session, &cleared);
	let dump = node(&oracle, &["dump", &cleared]);
	assert_eq!(
		veritas_rows(&dump, "veritas_meta", "key"),
		[("schema_version".to_owned(), "1".to_owned())]
	);
}

fn tax_summary(session: &Session) -> TaxSummary {
	block_on(session.tax_summary(2027, CancelToken::new())).unwrap()
}

fn total(summary: &TaxSummary, bucket: TaxBucket) -> Option<(i64, u64, bool)> {
	summary
		.buckets
		.iter()
		.find(|total| total.bucket == bucket)
		.map(|total| (total.total, total.count, total.negative))
}

#[test]
fn tax_setup_drives_the_tax_summary_and_survives_undo_a_reopen_and_actual() {
	let scratch = Scratch::new("tax");
	let session = import(&scratch);
	let bucket = |bucket| Some(TaxTagMeaning::Bucket { bucket });
	perform(
		&session,
		Command::SetCategoryTaxBucket {
			category_id: HOUSEHOLD.to_owned(),
			bucket: Some(TaxBucket::Charitable),
		},
	);
	for (tag, meaning) in [
		("#medical", bucket(TaxBucket::Medical)),
		("school", bucket(TaxBucket::Education)),
		("notax", Some(TaxTagMeaning::Exclude)),
	] {
		perform(
			&session,
			Command::SetTaxTag {
				tag: tag.to_owned(),
				meaning,
			},
		);
	}
	for tag in ["", "#", "##medical", "rx refill", "med#rx"] {
		refused(
			&session,
			Command::SetTaxTag {
				tag: tag.to_owned(),
				meaning: bucket(TaxBucket::Medical),
			},
		);
	}
	for (amount, category, notes) in [
		(-10000, HOUSEHOLD, ""),
		(-5000, HOUSEHOLD, ""),
		(2000, HOUSEHOLD, "refund"),
		(-3000, GROCERIES, "pharmacy #medical"),
		(-4000, HOUSEHOLD, "#notax"),
		(-2500, GROCERIES, "#medical #school"),
		(6500, GROCERIES, "#medical"),
	] {
		perform(
			&session,
			entry(CHECKING, "2027-01-10", amount, category, notes),
		);
	}

	let summary = tax_summary(&session);
	assert_eq!(
		total(&summary, TaxBucket::Charitable),
		Some((13000, 3, false)),
		"T-1, and T-3's #notax row is out"
	);
	assert_eq!(
		total(&summary, TaxBucket::Medical),
		Some((-3500, 2, true)),
		"T-2 and a bigger refund (TAX-4)"
	);
	assert_eq!(total(&summary, TaxBucket::Education), None);
	assert_eq!(summary.conflicts, 1);
	let conflict = summary
		.detail
		.iter()
		.find(|row| row.bucket.is_none())
		.unwrap();
	assert_eq!(
		conflict.conflicting,
		[TaxBucket::Education, TaxBucket::Medical]
	);
	assert_eq!(conflict.notes, "#medical #school");
	let checking = session
		.accounts()
		.unwrap()
		.accounts
		.into_iter()
		.find(|account| account.id == CHECKING)
		.unwrap()
		.name;
	assert!(
		summary
			.detail
			.iter()
			.all(|row| row.account_name == checking)
	);
	assert!(
		summary
			.detail
			.iter()
			.all(|row| row.category_labels.is_some())
	);
	for total in &summary.buckets {
		let counted: i64 = summary
			.detail
			.iter()
			.filter(|row| row.bucket == Some(total.bucket))
			.filter_map(|row| row.counted)
			.sum();
		assert_eq!(counted, total.total, "TAX-6.1 for {:?}", total.bucket);
	}

	perform(
		&session,
		Command::SetCategoryTaxBucket {
			category_id: INCOME.to_owned(),
			bucket: Some(TaxBucket::Charitable),
		},
	);
	let opposite = |session: &Session| {
		session
			.tax_setup()
			.unwrap()
			.categories
			.into_iter()
			.find(|line| line.category_id == INCOME)
			.map(|line| (line.bucket, line.opposite_direction))
	};
	assert_eq!(
		opposite(&session),
		Some((Some(TaxBucket::Charitable), true))
	);
	perform(
		&session,
		Command::SetCategoryTaxBucket {
			category_id: INCOME.to_owned(),
			bucket: Some(TaxBucket::W2Income),
		},
	);
	assert_eq!(opposite(&session), Some((Some(TaxBucket::W2Income), false)));
	perform(
		&session,
		Command::SetCategoryTaxBucket {
			category_id: INCOME.to_owned(),
			bucket: None,
		},
	);
	assert_eq!(opposite(&session), None);

	perform(
		&session,
		Command::SetTaxTag {
			tag: "medical".to_owned(),
			meaning: None,
		},
	);
	let unmapped = tax_summary(&session);
	assert_eq!(total(&unmapped, TaxBucket::Medical), None);
	assert_eq!(
		total(&unmapped, TaxBucket::Education),
		Some((2500, 1, false)),
		"with #medical unmapped, #school alone decides the row"
	);
	session.undo().unwrap();
	assert_eq!(tax_summary(&session), summary);

	let session = reopen(&scratch, session);
	assert_eq!(tax_summary(&session), summary);
	let tags: Vec<(String, Option<TaxTagMeaning>)> = session
		.tax_setup()
		.unwrap()
		.tags
		.into_iter()
		.map(|mapping| (mapping.tag, mapping.meaning))
		.collect();
	assert_eq!(
		tags,
		[
			("medical".to_owned(), bucket(TaxBucket::Medical)),
			("notax".to_owned(), Some(TaxTagMeaning::Exclude)),
			("school".to_owned(), bucket(TaxBucket::Education)),
		]
	);

	let Some(oracle) = oracle() else {
		return;
	};
	let zip = scratch.join("tax.zip");
	export(&session, &zip);
	let dump = node(&oracle, &["dump", &zip]);
	assert_eq!(
		veritas_rows(&dump, "veritas_tax_tag", "tag"),
		[
			("medical".to_owned(), "medical".to_owned()),
			("notax".to_owned(), "none".to_owned()),
			("school".to_owned(), "education".to_owned()),
		]
	);
}

fn fresh(label: &str, currency: &str) -> (Scratch, Arc<Session>) {
	let scratch = Scratch::new(label);
	let session = block_on(Session::create_budget(
		label.to_owned(),
		scratch.budget(),
		veritas_ffi::NumberFormat {
			currency: Some(currency.to_owned()),
			style: veritas_ffi::NumberStyle::CommaDot,
			hide_decimals: false,
			symbol_first: None,
			symbol_space: None,
		},
		veritas_ffi::NewBudgetOptions {
			retirement_401k: false,
			hsa: false,
			accounts: Vec::new(),
		},
		CancelToken::new(),
	))
	.unwrap();
	session.budget_month(NOVEMBER, TODAY.to_owned()).unwrap();
	(scratch, session)
}

fn created_id(session: &Session, command: Command, kind: veritas_ffi::EntityKind) -> String {
	session
		.perform(command)
		.unwrap()
		.entities
		.into_iter()
		.find(|e| e.kind == kind && e.id.is_some())
		.and_then(|e| e.id)
		.unwrap()
}

fn new_account(
	session: &Session,
	name: &str,
	kind: veritas_ffi::NewAccountKind,
	starting: i64,
	loan: Option<veritas_ffi::LoanTerms>,
) -> String {
	created_id(
		session,
		Command::CreateAccount {
			account: veritas_ffi::NewAccount {
				name: name.to_owned(),
				kind,
				starting_balance: starting,
				loan,
			},
		},
		veritas_ffi::EntityKind::Account,
	)
}

#[test]
fn d77_sg_8_a_yen_goal_with_spending_keeps_its_budget_and_a_dollar_goal_still_gets_a_suggestion() {
	// FC-5 SG-8 through the FFI on budgets made the way Ryan makes them: a
	// goal "New laptop" (target 150000 on the budget scale, due 2027-06-15)
	// budgeted 10000 this month and next, with one small purchase in it.
	// In yen the purchase (¥500, stored -50000) makes the goal's available
	// mix scales, so it gets no suggestion, its line says why in yen, Apply
	// suggestions leaves its cell alone, and its ETA and pace say "spending
	// recorded". The same goal in dollars ($5.00, stored -500) keeps all of
	// them. A purchase dated later this month, after today, counts too: D77
	// reads spending through the end of the month.
	use veritas_ffi::EntityKind;
	let december = Month {
		year: 2026,
		month: 12,
	};
	for (currency, purchase, date) in [
		("JPY", -50_000, "2026-11-10"),
		("JPY", -50_000, "2026-11-28"),
		("USD", -500, "2026-11-10"),
	] {
		let what = format!("{currency} purchase on {date}");
		let (_scratch, session) = fresh(&format!("sg8-{currency}-{date}"), currency);
		let account = new_account(
			&session,
			"Checking",
			veritas_ffi::NewAccountKind::Checking,
			50_000_000,
			None,
		);
		let goal = created_id(
			&session,
			Command::CreateGoal {
				name: "New laptop".to_owned(),
				target: 150_000,
				target_date: Some("2027-06-15".to_owned()),
				icon: None,
				color: None,
			},
			EntityKind::Goal,
		);
		let goals = || session.goals(TODAY.to_owned()).unwrap();
		let category = goals()
			.goals
			.into_iter()
			.find(|g| g.id == goal)
			.unwrap()
			.category_id;
		for month in [NOVEMBER, december] {
			perform(
				&session,
				Command::SetBudget {
					month,
					category_id: category.clone(),
					amount: 10_000,
				},
			);
		}
		perform(
			&session,
			entry(&account, date, purchase, &category, "laptop bag"),
		);

		let suggestions =
			block_on(session.budget_suggestions(TODAY.to_owned(), CancelToken::new())).unwrap();
		assert_eq!(suggestions.month, december, "{what}");
		let line = suggestions
			.items
			.iter()
			.find(|item| item.category_id == category)
			.unwrap_or_else(|| panic!("{what}: the goal is listed"))
			.clone();
		let summary = goals().goals.into_iter().find(|g| g.id == goal).unwrap();
		let forecast =
			block_on(session.forecast(TODAY.to_owned(), Vec::new(), CancelToken::new())).unwrap();
		let eta = forecast
			.baseline
			.goal_etas
			.iter()
			.find(|eta| eta.goal_id == goal)
			.unwrap()
			.clone();
		perform(
			&session,
			Command::ApplySuggestions {
				today: TODAY.to_owned(),
			},
		);
		let december_cell = session
			.budget_month(december, TODAY.to_owned())
			.unwrap()
			.groups
			.iter()
			.flat_map(|g| &g.envelopes)
			.find(|e| e.id == category)
			.unwrap()
			.budgeted;
		session.undo().unwrap();
		if currency == "JPY" {
			assert_eq!(line.amount, None, "{what}: no suggestion");
			assert_eq!(
				line.reason, "New laptop: spending recorded, keeping ¥10,000",
				"{what}"
			);
			assert_eq!(
				december_cell, 10_000,
				"{what}: Apply suggestions keeps the goal's cell"
			);
			assert_eq!(
				summary.pace,
				Some(veritas_ffi::GoalPace::SpendingRecorded),
				"{what}"
			);
			assert_eq!(summary.suggested_monthly, None, "{what}");
			assert_eq!(eta.text, "No ETA: spending recorded", "{what}");
			assert_eq!(eta.remaining, None, "{what}: no remaining figure");
		} else {
			let amount = line
				.amount
				.unwrap_or_else(|| panic!("{what}: dollars keep the suggestion: {}", line.reason));
			assert!(
				!line.reason.contains("spending recorded"),
				"{}",
				line.reason
			);
			assert_eq!(december_cell, amount, "{what}: Apply suggestions writes it");
			assert_ne!(summary.pace, Some(veritas_ffi::GoalPace::SpendingRecorded));
			assert!(summary.suggested_monthly.is_some());
			assert_ne!(eta.text, "No ETA: spending recorded");
		}
	}
}

#[test]
fn wrapped_shares_always_add_up_to_1000() {
	// R-WRAP-2: with amounts hidden, each top-five list and its "Everything
	// else" add up to exactly 1000 tenths, and total out with net does too,
	// on all-features' years and on a year built to land on awkward thirds,
	// with a refund-heavy category and more than five payees.
	fn check(report: &WrappedReport, what: &str) {
		if !report.has_data {
			return;
		}
		let lists = [
			(
				report
					.top_categories
					.iter()
					.map(|c| c.share)
					.collect::<Vec<_>>(),
				report.shares.other_categories,
				"categories",
			),
			(
				report
					.top_groups
					.iter()
					.map(|g| g.share)
					.collect::<Vec<_>>(),
				report.shares.other_groups,
				"groups",
			),
			(
				report
					.top_payees
					.iter()
					.map(|p| p.share)
					.collect::<Vec<_>>(),
				report.shares.other_payees,
				"payees",
			),
		];
		for (shares, other, list) in lists {
			if shares.is_empty() {
				continue;
			}
			let sum: i64 = shares.iter().sum::<i64>() + other.unwrap_or(0);
			assert_eq!(sum, 1000, "{what} {list}: {shares:?} + {other:?}");
		}
		if let (Some(out), Some(net)) = (report.shares.out_vs_in, report.shares.net) {
			assert_eq!(out + net, 1000, "{what}: total out and net");
		}
	}
	let scratch = Scratch::new("wrapped-shares");
	let session = import(&scratch);
	for year in [2025, 2026] {
		check(&wrapped(&session, year), &format!("all-features {year}"));
	}
	let (_fresh, fresh) = fresh("wrapped-thirds", "USD");
	let account = new_account(
		&fresh,
		"Checking",
		veritas_ffi::NewAccountKind::Checking,
		0,
		None,
	);
	let month = fresh.budget_month(NOVEMBER, TODAY.to_owned()).unwrap();
	let income = month.groups.iter().find(|g| g.is_income).unwrap().envelopes[0]
		.id
		.clone();
	let spending: Vec<String> = month
		.groups
		.iter()
		.filter(|g| !g.is_income)
		.flat_map(|g| g.envelopes.iter().map(|e| e.id.clone()))
		.take(8)
		.collect();
	perform(&fresh, entry(&account, "2025-01-05", 999_999, &income, ""));
	for (n, category) in spending.iter().take(3).enumerate() {
		let mut draft = TransactionDraft {
			id: None,
			account_id: account.clone(),
			date: format!("2025-0{}-10", n + 2),
			amount: -10_000,
			payee_id: None,
			payee_name: Some(format!("Shop {n}")),
			category_id: Some(category.clone()),
			notes: None,
			cleared: true,
			splits: Vec::new(),
		};
		perform(
			&fresh,
			Command::AddTransaction {
				transaction: draft.clone(),
			},
		);
		draft.payee_name = Some(format!("Kiosk {n}"));
		draft.amount = -1;
		perform(&fresh, Command::AddTransaction { transaction: draft });
	}
	perform(
		&fresh,
		entry(&account, "2025-06-01", -700, &spending[3], "refunded later"),
	);
	perform(
		&fresh,
		entry(&account, "2025-06-20", 2_100, &spending[3], "refund"),
	);
	for (n, category) in spending.iter().enumerate().skip(4) {
		perform(
			&fresh,
			entry(
				&account,
				&format!("2025-07-{:02}", 10 + n),
				-(333 + n as i64),
				category,
				"",
			),
		);
	}
	check(&wrapped(&fresh, 2025), "built year");
	assert!(
		wrapped(&fresh, 2025).top_payees.len() == 5,
		"more than five payees"
	);
}

#[test]
fn the_income_statement_is_net_profit_for_every_range() {
	// R-REP-7 ST-3: net income always equals Net Profit for the same range,
	// and revenue and expenses carry Net Profit's income and expense totals
	// and categories, for a month, a quarter, a year to date and twelve
	// months across a new year.
	let scratch = Scratch::new("income-statement");
	let session = import(&scratch);
	let month = |year: i32, month: u32| Month { year, month };
	for (first, last) in [
		(month(2026, 10), month(2026, 10)),
		(month(2026, 7), month(2026, 9)),
		(month(2026, 1), month(2026, 11)),
		(month(2025, 11), month(2026, 10)),
	] {
		let report = block_on(session.net_profit(first, last, CancelToken::new())).unwrap();
		let first_day = format!("{}-{:02}-01", first.year, first.month);
		let last_day = report.total.last.clone();
		let statement = block_on(session.income_statement(
			first_day.clone(),
			last_day.clone(),
			CancelToken::new(),
		))
		.unwrap();
		let what = format!("{first_day} to {last_day}");
		assert_eq!(
			statement.net_income.total, report.total.net_profit.total,
			"{what}"
		);
		assert_eq!(
			statement.total_revenue.total, report.total.income.total,
			"{what}"
		);
		assert_eq!(
			statement.total_expenses.total, report.total.expenses.total,
			"{what}"
		);
		let lines = |groups: &[veritas_ffi::StatementGroup]| -> std::collections::BTreeMap<Option<String>, i64> {
			groups
				.iter()
				.flat_map(|g| &g.categories)
				.filter(|c| c.total != 0)
				.map(|c| (c.category_id.clone(), c.total))
				.collect()
		};
		let net_profit_lines = |section: &veritas_ffi::ReportSection| -> std::collections::BTreeMap<Option<String>, i64> {
			section
				.categories
				.iter()
				.filter(|c| c.total != 0)
				.map(|c| (c.category_id.clone(), c.total))
				.collect()
		};
		assert_eq!(
			lines(&statement.revenue),
			net_profit_lines(&report.total.income),
			"{what} revenue"
		);
		assert_eq!(
			lines(&statement.expenses),
			net_profit_lines(&report.total.expenses),
			"{what} expenses"
		);
		for group in statement.revenue.iter().chain(&statement.expenses) {
			assert_eq!(
				group.total,
				group.categories.iter().map(|c| c.total).sum::<i64>(),
				"{what} {:?}",
				group.group_name
			);
		}
	}
}

#[test]
fn a_what_if_moves_its_horizons_by_its_amount_and_every_horizon_adds_up() {
	// R-FC-1 and R-FC-2 through the FFI on all-features: horizons at 3, 6,
	// 10 and 12 months whose account balances add up to their net worth
	// inside the band, and a one-time what-if (a $1,000.00 repair in March)
	// that lowers each horizon from March on by exactly its amount, leaves
	// the earlier ones alone, shifts the band without widening it, and is
	// one undo step.
	use veritas_ffi::{EntityKind, ScenarioChange};
	let scratch = Scratch::new("what-if-horizons");
	let session = import(&scratch);
	let scenario = created_id(
		&session,
		Command::CreateScenario {
			name: "Roof repair".to_owned(),
			first_event: None,
			today: None,
		},
		EntityKind::Scenario,
	);
	perform(
		&session,
		Command::AddScenarioEvent {
			scenario_id: scenario.clone(),
			change: ScenarioChange::OneTime {
				date: "2027-03-15".to_owned(),
				account_id: CHECKING.to_owned(),
				category_id: Some(HOUSEHOLD.to_owned()),
				amount: -100_000,
			},
			today: TODAY.to_owned(),
		},
	);
	let forecast =
		block_on(session.forecast(TODAY.to_owned(), vec![scenario.clone()], CancelToken::new()))
			.unwrap();
	let horizons = |projection: &veritas_ffi::ForecastProjection| -> Vec<(u32, Month, i64)> {
		projection
			.horizons
			.iter()
			.map(|h| (h.months, h.month, h.net_worth))
			.collect()
	};
	let base = horizons(&forecast.baseline);
	assert_eq!(base.iter().map(|h| h.0).collect::<Vec<_>>(), [3, 6, 10, 12]);
	for horizon in &forecast.baseline.horizons {
		assert_eq!(
			horizon.accounts.iter().map(|a| a.balance).sum::<i64>(),
			horizon.net_worth,
			"{} months: the accounts add up to net worth",
			horizon.months
		);
		if let (Some(low), Some(high)) = (horizon.band_low, horizon.band_high) {
			assert!(
				low <= horizon.net_worth && horizon.net_worth <= high,
				"{} months: inside the band",
				horizon.months
			);
		}
	}
	let what_if = &forecast.scenarios[0];
	for (base_horizon, scenario_horizon) in forecast.baseline.horizons.iter().zip(&what_if.horizons)
	{
		let shift = if (base_horizon.month.year, base_horizon.month.month) >= (2027, 3) {
			100_000
		} else {
			0
		};
		assert_eq!(
			scenario_horizon.net_worth,
			base_horizon.net_worth - shift,
			"{} months",
			base_horizon.months
		);
		assert_eq!(
			(scenario_horizon.band_low, scenario_horizon.band_high),
			(
				base_horizon.band_low.map(|l| l - shift),
				base_horizon.band_high.map(|h| h - shift)
			),
			"{} months: the band's width never changes",
			base_horizon.months
		);
	}
	// v18: each horizon carries the what-if's difference from the baseline,
	// in net worth and per account (FC-3); the baseline's is nil.
	for (base_horizon, scenario_horizon) in forecast.baseline.horizons.iter().zip(&what_if.horizons)
	{
		let shift = base_horizon.net_worth - scenario_horizon.net_worth;
		assert_eq!(scenario_horizon.net_worth_difference, Some(-shift));
		assert_eq!(base_horizon.net_worth_difference, None);
		assert!(
			base_horizon
				.accounts
				.iter()
				.all(|account| account.difference.is_none())
		);
		for account in &scenario_horizon.accounts {
			let expected = if account.account_id == CHECKING {
				-shift
			} else {
				0
			};
			assert_eq!(account.difference, Some(expected), "{}", account.name);
		}
	}
	assert!(
		forecast
			.accounts_today
			.iter()
			.all(|account| account.difference.is_none())
	);
	// v18: the band at every projected month (FC-4.2 step 5), none for the
	// current month's remainder, and the horizons are points of that series.
	for projection in [&forecast.baseline, what_if] {
		assert_eq!(
			(
				projection.months[0].band_low,
				projection.months[0].band_high
			),
			(None, None)
		);
		assert!(
			projection
				.months
				.iter()
				.skip(1)
				.all(|month| month.band_low.is_some())
		);
		for horizon in &projection.horizons {
			let month = &projection.months[horizon.months as usize];
			assert_eq!(month.month, horizon.month);
			assert_eq!(
				(month.band_low, month.band_high),
				(horizon.band_low, horizon.band_high)
			);
		}
		for (base_month, month) in forecast.baseline.months.iter().zip(&projection.months) {
			assert_eq!(
				month
					.band_high
					.zip(month.band_low)
					.map(|(high, low)| high - low),
				base_month
					.band_high
					.zip(base_month.band_low)
					.map(|(high, low)| high - low),
				"a what-if never widens the band"
			);
		}
	}
	// v18: net worth before today is the net worth report's, to the cent.
	assert_eq!(forecast.history.len(), 12);
	let report = block_on(session.net_worth(
		forecast.history[0].month,
		forecast.history[11].month,
		CancelToken::new(),
	))
	.unwrap();
	assert_eq!(forecast.history, report);
	assert_eq!(
		forecast.history[11].month,
		Month {
			year: 2026,
			month: 10
		},
		"the last complete month before today's"
	);

	session.undo().unwrap();
	let undone =
		block_on(session.forecast(TODAY.to_owned(), vec![scenario.clone()], CancelToken::new()))
			.unwrap();
	assert!(
		undone.scenarios[0]
			.horizons
			.iter()
			.all(|horizon| horizon.net_worth_difference == Some(0)),
		"an empty what-if differs by nothing"
	);
	assert_eq!(
		horizons(&undone.scenarios[0]),
		base,
		"undoing the event leaves the what-if at the baseline"
	);
}

#[test]
fn v17_add_what_if_with_its_first_event_is_one_undo_step_and_a_refusal_leaves_nothing() {
	// "Add what-if" makes the what-if and its first line in one command, so
	// one undo removes both, and a refused line refuses the what-if too:
	// nothing is written and the undo stack doesn't move.
	use veritas_ffi::{EntityKind, ScenarioChange};
	let scratch = Scratch::new("what-if-first-event");
	let session = import(&scratch);
	let repair = ScenarioChange::OneTime {
		date: "2027-03-15".to_owned(),
		account_id: CHECKING.to_owned(),
		category_id: Some(HOUSEHOLD.to_owned()),
		amount: -100_000,
	};
	let id = created_id(
		&session,
		Command::CreateScenario {
			name: "Roof repair".to_owned(),
			first_event: Some(repair.clone()),
			today: Some(TODAY.to_owned()),
		},
		EntityKind::Scenario,
	);
	let listed = session.scenarios(TODAY.to_owned()).unwrap();
	assert_eq!(listed.len(), 1);
	assert_eq!(listed[0].id, id);
	assert_eq!(listed[0].events.len(), 1);
	assert_eq!(listed[0].events[0].change, repair);
	let forecast =
		block_on(session.forecast(TODAY.to_owned(), vec![id.clone()], CancelToken::new())).unwrap();
	assert_ne!(
		forecast.scenarios[0].horizons.last().map(|h| h.net_worth),
		forecast.baseline.horizons.last().map(|h| h.net_worth),
		"the first line is in the projection"
	);

	session.undo().unwrap();
	assert!(
		session.scenarios(TODAY.to_owned()).unwrap().is_empty(),
		"one undo removes the what-if and its line together"
	);
	session.redo().unwrap();
	assert_eq!(session.scenarios(TODAY.to_owned()).unwrap().len(), 1);

	refused(
		&session,
		Command::CreateScenario {
			name: "Last month".to_owned(),
			first_event: Some(ScenarioChange::OneTime {
				date: "2026-10-01".to_owned(),
				account_id: CHECKING.to_owned(),
				category_id: None,
				amount: -100,
			}),
			today: Some(TODAY.to_owned()),
		},
	);
	refused(
		&session,
		Command::CreateScenario {
			name: "No date".to_owned(),
			first_event: Some(repair),
			today: None,
		},
	);
	assert!(
		matches!(
			session.perform(Command::CreateScenario {
				name: "Gone account".to_owned(),
				first_event: Some(ScenarioChange::OneTime {
					date: "2027-01-10".to_owned(),
					account_id: "no-such-account".to_owned(),
					category_id: None,
					amount: -100,
				}),
				today: Some(TODAY.to_owned()),
			}),
			Err(FfiError::NotFound { .. })
		),
		"an unknown account refuses the what-if too"
	);
	let names: Vec<String> = session
		.scenarios(TODAY.to_owned())
		.unwrap()
		.into_iter()
		.map(|scenario| scenario.name)
		.collect();
	assert_eq!(
		names,
		["Roof repair"],
		"a refused line leaves no what-if behind"
	);
	session.undo().unwrap();
	assert!(
		session.scenarios(TODAY.to_owned()).unwrap().is_empty(),
		"the refusals added no undo steps"
	);
}

#[test]
fn a_car_loan_built_in_the_app_is_forecast_payment_by_payment_as_fc_2_and_actual_say() {
	// The loan gap: a car loan made through the FFI with its terms (6.00%,
	// $300 a month), one recorded payment split by the wizard ($60 interest,
	// $20 escrow, a $5 late fee, $215 principal), and its monthly bill to
	// the loan from November. The forecast lists the loan with a known split
	// and FL-5d's "Not repeated" note for the fee, and each
	// payment splits FC-2's way: interest on the balance at the start of the
	// month at 6.00% / 12 rounded half up, the last escrow repeated, the rest
	// principal into the loan. Each payment adds back up to the bill, as
	// Actual's own forecast/generate has it on the same dates, and the loan's
	// balance at each horizon is today's less the principal paid by then.
	use veritas_ffi::{
		BillAmount, BillDate, BillDraft, BillFrequency, BillRepeat, BillRepeatEnd, CategoryKind,
		ForecastFlowKind, LoanPaymentDraft, LoanTerms,
	};
	let (scratch, session) = fresh("loan-forecast", "USD");
	let checking = new_account(
		&session,
		"Checking",
		veritas_ffi::NewAccountKind::Checking,
		5_000_000,
		None,
	);
	let loan = new_account(
		&session,
		"Car loan",
		veritas_ffi::NewAccountKind::Loan,
		1_200_000,
		Some(LoanTerms {
			rate_bps: Some(600),
			payment: Some(30_000),
			payment_day: Some(15),
			lender: Some("Credit union".to_owned()),
		}),
	);
	let veritas_ffi::KindTarget::One {
		category_id: principal,
	} = session.kind_target(CategoryKind::DebtPrincipal).unwrap()
	else {
		panic!("the starter set has one principal category");
	};
	let month = session.budget_month(NOVEMBER, TODAY.to_owned()).unwrap();
	let envelope = |name: &str| {
		month
			.groups
			.iter()
			.flat_map(|g| &g.envelopes)
			.find(|e| e.name == name)
			.unwrap_or_else(|| panic!("no starter envelope {name}"))
			.id
			.clone()
	};
	let (interest, escrow, fees) = (
		envelope("Miscellaneous"),
		envelope("Car Insurance"),
		envelope("Gifts"),
	);
	perform(
		&session,
		Command::RecordLoanPayment {
			payment: LoanPaymentDraft {
				id: Some("october".to_owned()),
				from_account_id: checking.clone(),
				loan_account_id: loan.clone(),
				date: "2026-10-15".to_owned(),
				total: 30_000,
				interest: 6_000,
				escrow: 2_000,
				fees: 500,
				principal_category_id: principal.clone(),
				interest_category_id: Some(interest.clone()),
				escrow_category_id: Some(escrow.clone()),
				fees_category_id: Some(fees.clone()),
				notes: None,
				cleared: true,
				fund_principal: false,
			},
		},
	);
	let loan_payee = session
		.payees()
		.unwrap()
		.into_iter()
		.find(|p| p.transfer_account_id.as_deref() == Some(loan.as_str()))
		.unwrap()
		.id;
	perform(
		&session,
		Command::CreateBill {
			bill_id: Some("car-payment".to_owned()),
			bill: BillDraft {
				name: Some("Car payment".to_owned()),
				payee_id: Some(loan_payee),
				account_id: Some(checking.clone()),
				amount: BillAmount::Exact { amount: -30_000 },
				date: BillDate::Repeat {
					repeat: BillRepeat {
						start: "2026-11-15".to_owned(),
						interval: 1,
						frequency: BillFrequency::Monthly,
						patterns: Vec::new(),
						weekend: None,
						end: BillRepeatEnd::Never,
					},
				},
				category_id: None,
				auto_post: false,
			},
		},
	);

	let forecast =
		block_on(session.forecast(TODAY.to_owned(), Vec::new(), CancelToken::new())).unwrap();
	let listed = forecast
		.loans
		.iter()
		.find(|l| l.account_id == loan)
		.unwrap_or_else(|| {
			panic!(
				"the car loan is forecast, so it is listed: {:?}",
				forecast.loans
			)
		});
	assert!(
		!listed.estimated_from_amounts && !listed.interest_assumed_zero,
		"a mapped split is known: {listed:?}"
	);
	assert_eq!(listed.notes.len(), 1, "{:?}", listed.notes);
	assert!(
		listed.notes[0].starts_with("Not repeated: Gifts"),
		"FL-5d: the mapped fee isn't repeated, and the screen says so: {:?}",
		listed.notes
	);

	let mut by_date: std::collections::BTreeMap<String, Vec<&veritas_ffi::ForecastFlow>> =
		std::collections::BTreeMap::new();
	for flow in forecast
		.known_flows
		.iter()
		.filter(|f| f.bill_id.as_deref() == Some("car-payment"))
	{
		by_date.entry(flow.date.clone()).or_default().push(flow);
	}
	assert!(by_date.len() >= 12, "a year of payments: {}", by_date.len());
	let mut owed = 1_200_000 - 21_500;
	let mut paid_principal = std::collections::BTreeMap::new();
	for (date, flows) in &by_date {
		let part = |kind: ForecastFlowKind, category: Option<&str>| -> i64 {
			flows
				.iter()
				.filter(|f| {
					f.account_id == checking
						&& f.kind == kind && f.category_id.as_deref() == category
				})
				.map(|f| f.amount)
				.sum()
		};
		let month_interest = (owed * 600 + 60_000) / 120_000;
		let expected_principal = (30_000 - month_interest - 2_000).min(owed);
		assert_eq!(
			part(ForecastFlowKind::Expense, Some(&interest)),
			-month_interest,
			"{date}: interest on {owed} at 6.00%"
		);
		assert_eq!(
			part(ForecastFlowKind::Expense, Some(&escrow)),
			-2_000,
			"{date}: the last escrow, repeated"
		);
		let into_loan: i64 = flows
			.iter()
			.filter(|f| {
				f.account_id == checking
					&& f.kind == ForecastFlowKind::Transfer
					&& f.counterpart_account_id.as_deref() == Some(loan.as_str())
			})
			.map(|f| f.amount)
			.sum();
		assert_eq!(
			into_loan, -expected_principal,
			"{date}: the rest is principal into the loan"
		);
		assert_eq!(
			flows
				.iter()
				.filter(|f| f.account_id == checking)
				.map(|f| f.amount)
				.sum::<i64>(),
			-(month_interest + 2_000 + expected_principal),
			"{date}: the parts add back up to the payment"
		);
		owed -= expected_principal;
		paid_principal.insert(date.clone(), expected_principal);
	}
	for horizon in &forecast.baseline.horizons {
		let end = format!("{}-{:02}-31", horizon.month.year, horizon.month.month);
		let principal_by_then: i64 = paid_principal
			.iter()
			.filter(|(date, _)| date.as_str() <= end.as_str())
			.map(|(_, p)| p)
			.sum();
		let balance = horizon
			.accounts
			.iter()
			.find(|a| a.account_id == loan)
			.unwrap()
			.balance;
		assert_eq!(
			balance,
			-(1_178_500 - principal_by_then),
			"{} months: the loan's balance",
			horizon.months
		);
	}

	let zip = scratch.0.join("loan.zip").display().to_string();
	export(&session, &zip);
	let Some(oracle) = oracle() else {
		return;
	};
	let last = by_date.keys().last().unwrap().clone();
	let actual = node(&oracle, &["forecast-occurrences", &zip, &last]);
	let theirs: std::collections::BTreeMap<String, i64> = actual
		.as_array()
		.unwrap()
		.iter()
		.filter(|o| o["schedule"] == "car-payment" && o["account"] == checking.as_str())
		.map(|o| {
			(
				o["date"].as_str().unwrap().to_owned(),
				o["amount"].as_i64().unwrap(),
			)
		})
		.collect();
	let ours: std::collections::BTreeMap<String, i64> = by_date
		.iter()
		.map(|(date, flows)| {
			(
				date.clone(),
				flows
					.iter()
					.filter(|f| f.account_id == checking)
					.map(|f| f.amount)
					.sum(),
			)
		})
		.collect();
	assert_eq!(
		ours, theirs,
		"the same payment dates and amounts as Actual's forecast"
	);
}

#[test]
fn every_forecast_loan_is_listed_and_a_clean_one_has_no_notes() {
	// Architect, #83: forecast.loans lists every loan the forecast splits,
	// in Actual's account order, so the screen can show which loans are
	// forecast. Two loans with terms and a last payment the wizard mapped
	// with interest and principal only, no escrow and no fee, carry no
	// notes and neither flag. A loan with no payment to read from stays in
	// unforecast.
	use veritas_ffi::{CategoryKind, LoanPaymentDraft, LoanTerms};
	let (_scratch, session) = fresh("clean-loans", "USD");
	let checking = new_account(
		&session,
		"Checking",
		veritas_ffi::NewAccountKind::Checking,
		5_000_000,
		None,
	);
	let terms = || {
		Some(LoanTerms {
			rate_bps: Some(600),
			payment: Some(30_000),
			payment_day: Some(15),
			lender: None,
		})
	};
	let car = new_account(
		&session,
		"Car loan",
		veritas_ffi::NewAccountKind::Loan,
		1_200_000,
		terms(),
	);
	let boat = new_account(
		&session,
		"Boat loan",
		veritas_ffi::NewAccountKind::Loan,
		1_200_000,
		terms(),
	);
	let unpaid = new_account(
		&session,
		"Unpaid loan",
		veritas_ffi::NewAccountKind::Loan,
		1_200_000,
		terms(),
	);
	let veritas_ffi::KindTarget::One {
		category_id: principal,
	} = session.kind_target(CategoryKind::DebtPrincipal).unwrap()
	else {
		panic!("the starter set has one principal category");
	};
	let interest = session
		.budget_month(NOVEMBER, TODAY.to_owned())
		.unwrap()
		.groups
		.iter()
		.flat_map(|g| &g.envelopes)
		.find(|e| e.name == "Miscellaneous")
		.unwrap()
		.id
		.clone();
	for loan in [&car, &boat] {
		perform(
			&session,
			Command::RecordLoanPayment {
				payment: LoanPaymentDraft {
					id: Some(format!("october-{loan}")),
					from_account_id: checking.clone(),
					loan_account_id: loan.clone(),
					date: "2026-10-15".to_owned(),
					total: 30_000,
					interest: 6_000,
					escrow: 0,
					fees: 0,
					principal_category_id: principal.clone(),
					interest_category_id: Some(interest.clone()),
					escrow_category_id: None,
					fees_category_id: None,
					notes: None,
					cleared: true,
					fund_principal: false,
				},
			},
		);
	}

	let forecast =
		block_on(session.forecast(TODAY.to_owned(), Vec::new(), CancelToken::new())).unwrap();
	let listed: Vec<(&str, &str, bool, bool, usize)> = forecast
		.loans
		.iter()
		.map(|l| {
			(
				l.account_id.as_str(),
				l.name.as_str(),
				l.estimated_from_amounts,
				l.interest_assumed_zero,
				l.notes.len(),
			)
		})
		.collect();
	assert_eq!(
		listed,
		[
			(car.as_str(), "Car loan", false, false, 0),
			(boat.as_str(), "Boat loan", false, false, 0),
		],
		"both clean loans, in account order, with no notes"
	);
	assert!(
		forecast
			.unforecast
			.iter()
			.any(|item| item.loan_account_id.as_deref() == Some(unpaid.as_str())),
		"a loan the forecast can't use stays in unforecast: {:?}",
		forecast.unforecast
	);
}
