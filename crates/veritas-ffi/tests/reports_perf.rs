#![allow(
	clippy::unwrap_used,
	clippy::expect_used,
	clippy::panic,
	reason = "integration tests fail by panicking"
)]

//! R-PERF-4 through the exported API on perf-100k: a typical report within
//! 100 ms and a year-long Sankey within 500 ms, timed the way the app meets
//! them, right after an edit made the session classify again. A budget edit
//! must not cost a reclassification.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread::Thread;
use std::time::{Duration, Instant};

use veritas_ffi::{CancelToken, Command, Month, Session, TransactionDraft};

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

impl Drop for Scratch {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.0);
	}
}

const TYPICAL: Duration = Duration::from_millis(100);
const YEAR_SANKEY: Duration = Duration::from_millis(500);

fn month(year: i32, month: u32) -> Month {
	Month { year, month }
}

/// The fastest of five runs of `report`, each right after `before`.
fn fastest(mut before: impl FnMut(), mut report: impl FnMut()) -> Duration {
	(0..5)
		.map(|_| {
			before();
			let started = Instant::now();
			report();
			started.elapsed()
		})
		.min()
		.unwrap()
}

#[test]
#[cfg_attr(
	debug_assertions,
	ignore = "R-PERF-4 budgets are for release builds: cargo test --release -p veritas-ffi --test reports_perf"
)]
fn reports_on_perf_100k_meet_r_perf_4_right_after_an_edit() {
	let scratch = Scratch(
		Path::new(env!("CARGO_TARGET_TMPDIR"))
			.join(format!("ffi-reports-perf-{}", std::process::id())),
	);
	let _ = std::fs::remove_dir_all(&scratch.0);
	std::fs::create_dir_all(&scratch.0).unwrap();
	let zip = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/perf-100k.zip");
	let session = block_on(Session::import_actual_zip(
		zip.display().to_string(),
		scratch.0.join("budget").display().to_string(),
		CancelToken::new(),
	))
	.unwrap();
	let account = session.accounts().unwrap().accounts[0].id.clone();
	let latest = session
		.register(
			veritas_ffi::RegisterScope::All,
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
	let year: i32 = latest[..4].parse::<i32>().unwrap() - 1;
	let today = format!("{}-12-31", year + 1);
	session
		.budget_month(month(year, 12), today.clone())
		.unwrap();
	let category = session
		.budget_month(month(year, 12), today.clone())
		.unwrap()
		.groups
		.into_iter()
		.find(|g| !g.is_income)
		.unwrap()
		.envelopes[0]
		.id
		.clone();

	let mut n = 0;
	let mut edit = || {
		n += 1;
		let draft = TransactionDraft {
			id: Some(format!("perf-{n}")),
			account_id: account.clone(),
			date: format!("{year}-06-15"),
			amount: -1_000 - n,
			payee_id: None,
			payee_name: None,
			category_id: Some(category.clone()),
			notes: None,
			cleared: false,
			splits: Vec::new(),
		};
		session
			.perform(Command::AddTransaction { transaction: draft })
			.unwrap();
	};
	let first = format!("{year}-01-01");
	let last = format!("{year}-12-31");
	let token = CancelToken::new;
	let mut timings = Vec::new();
	let mut time = |name: &str, budget: Duration, took: Duration| {
		eprintln!("{name}: {took:?}");
		timings.push((name.to_owned(), budget, took));
	};

	time(
		"net profit, 12 months",
		TYPICAL,
		fastest(&mut edit, || {
			block_on(session.net_profit(month(year, 1), month(year, 12), token())).unwrap();
		}),
	);
	time(
		"net worth, 12 months",
		TYPICAL,
		fastest(&mut edit, || {
			block_on(session.net_worth(month(year, 1), month(year, 12), token())).unwrap();
		}),
	);
	time(
		"cash flow, 12 months",
		TYPICAL,
		fastest(&mut edit, || {
			block_on(session.cash_flow(month(year, 1), month(year, 12), token())).unwrap();
		}),
	);
	time(
		"balance sheet",
		TYPICAL,
		fastest(&mut edit, || {
			block_on(session.balance_sheet(last.clone(), token())).unwrap();
		}),
	);
	time(
		"owner's equity, a year",
		TYPICAL,
		fastest(&mut edit, || {
			block_on(session.owners_equity(first.clone(), last.clone(), token())).unwrap();
		}),
	);
	time(
		"tax summary, a year",
		TYPICAL,
		fastest(&mut edit, || {
			block_on(session.tax_summary(year, token())).unwrap();
		}),
	);
	time(
		"dashboards",
		TYPICAL,
		fastest(&mut edit, || {
			block_on(session.dashboards(today.clone(), token())).unwrap();
		}),
	);
	time(
		"Sankey, a year",
		YEAR_SANKEY,
		fastest(&mut edit, || {
			block_on(session.sankey(first.clone(), last.clone(), token())).unwrap();
		}),
	);

	let after_edit = fastest(&mut edit, || {
		block_on(session.net_profit(month(year, 6), month(year, 6), token())).unwrap();
	});
	let mut amount = 0;
	let after_budget_edit = fastest(
		|| {
			amount += 100;
			session
				.perform(Command::SetBudget {
					month: month(year, 12),
					category_id: category.clone(),
					amount,
				})
				.unwrap();
		},
		|| {
			block_on(session.net_profit(month(year, 6), month(year, 6), token())).unwrap();
		},
	);
	eprintln!(
		"one month after an edit: {after_edit:?}, after a budget edit: {after_budget_edit:?}"
	);
	for (name, budget, took) in &timings {
		assert!(took <= budget, "{name} took {took:?}, over {budget:?}");
	}
	assert!(
		after_budget_edit * 3 < after_edit,
		"a budget edit kept the classification, so the next report skips classifying"
	);
	session.close().unwrap();
}
