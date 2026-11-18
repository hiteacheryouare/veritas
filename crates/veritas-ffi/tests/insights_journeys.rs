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
