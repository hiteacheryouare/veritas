#![allow(
	clippy::unwrap_used,
	clippy::expect_used,
	clippy::panic,
	reason = "integration tests fail by panicking"
)]

//! R-PERF-4 for the dashboard through the exported API on perf-100k: the
//! default dashboard within a typical report's 100 ms once warm, and each
//! native card D73 adds, filtered the way a typical card is, costing the
//! page no more than a typical report.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread::Thread;
use std::time::{Duration, Instant};

use veritas_ffi::{
	CancelToken, Command, ReportCondition, Session, TransactionDraft, WidgetConfig,
	WidgetTimeFrame, WidgetView,
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

impl Drop for Scratch {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.0);
	}
}

const TYPICAL: Duration = Duration::from_millis(100);

fn fastest(mut before: impl FnMut(), mut run: impl FnMut()) -> Duration {
	(0..5)
		.map(|_| {
			before();
			let started = Instant::now();
			run();
			started.elapsed()
		})
		.min()
		.unwrap()
}

#[test]
#[cfg_attr(
	debug_assertions,
	ignore = "R-PERF-4 budgets are for release builds: cargo test --release -p veritas-ffi --test dashboard_perf"
)]
fn the_dashboard_with_every_native_card_meets_r_perf_4_on_perf_100k() {
	let scratch = Scratch(
		Path::new(env!("CARGO_TARGET_TMPDIR"))
			.join(format!("ffi-dashboard-perf-{}", std::process::id())),
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
	let today = "2026-10-31".to_owned();
	let pages = block_on(session.dashboards(today.clone(), CancelToken::new())).unwrap();
	let page = pages[0].id.clone();
	let month = session
		.budget_month(
			veritas_ffi::Month {
				year: 2026,
				month: 10,
			},
			today.clone(),
		)
		.unwrap();
	let category = month
		.groups
		.iter()
		.find(|g| !g.is_income)
		.unwrap()
		.envelopes[0]
		.id
		.clone();
	let account = session.accounts().unwrap().accounts[0].id.clone();
	let year = Some(WidgetTimeFrame {
		mode: "static".to_owned(),
		start: "2025-11".to_owned(),
		end: "2026-10".to_owned(),
	});
	let filters = vec![
		ReportCondition {
			field: "category".to_owned(),
			op: "isNot".to_owned(),
			value: format!("\"{category}\""),
			options: None,
		},
		ReportCondition {
			field: "notes".to_owned(),
			op: "doesNotContain".to_owned(),
			value: "\"zzz\"".to_owned(),
			options: None,
		},
	];
	let draw = || {
		block_on(session.dashboards(today.clone(), CancelToken::new())).unwrap();
	};
	draw();
	let mut previous = fastest(|| {}, draw);
	let default_warm = previous;
	let mut marginal = Vec::new();
	for (n, (kind, summary)) in [
		("summary-card", Some("avgPerMonth")),
		("calendar-card", None),
		("age-of-money-card", None),
		("budget-analysis-card", None),
		("crossover-card", None),
	]
	.into_iter()
	.enumerate()
	{
		session
			.perform(Command::AddWidget {
				page_id: page.clone(),
				kind: kind.to_owned(),
				x: 0,
				y: 100 + 3 * i64::try_from(n).unwrap(),
				width: 4,
				height: 3,
				config: WidgetConfig {
					name: None,
					time_frame: year.clone(),
					conditions: if kind == "crossover-card" {
						Vec::new()
					} else {
						filters.clone()
					},
					conditions_or: false,
					mode: None,
					summary: summary.map(str::to_owned),
					content: None,
					text_align: None,
					formula: None,
					report_id: None,
				},
			})
			.unwrap();
		draw();
		let now = fastest(|| {}, draw);
		marginal.push((kind, now.saturating_sub(previous)));
		previous = now;
	}
	let mut n = 0;
	let mut edit = || {
		n += 1;
		session
			.perform(Command::AddTransaction {
				transaction: TransactionDraft {
					id: Some(format!("perf-{n}")),
					account_id: account.clone(),
					date: "2026-10-15".to_owned(),
					amount: -1_000 - n,
					payee_id: None,
					payee_name: None,
					category_id: Some(category.clone()),
					notes: None,
					cleared: false,
					splits: Vec::new(),
				},
			})
			.unwrap();
	};
	let show = || {
		let pages = block_on(session.dashboards(today.clone(), CancelToken::new())).unwrap();
		let drawn = pages
			.iter()
			.flat_map(|p| &p.widgets)
			.filter(|w| {
				matches!(
					w.view,
					WidgetView::Summary { .. }
						| WidgetView::Calendar { .. }
						| WidgetView::AgeOfMoney { .. }
						| WidgetView::BudgetAnalysis { .. }
						| WidgetView::Crossover { .. }
				)
			})
			.count();
		assert!(drawn >= 5, "every native card drew");
	};
	show();
	let warm = fastest(|| {}, show);
	let after_edit = fastest(&mut edit, show);
	let cards: usize = block_on(session.dashboards(today.clone(), CancelToken::new()))
		.unwrap()
		.iter()
		.map(|p| p.widgets.len())
		.sum();
	eprintln!("default dashboard warm: {default_warm:?}");
	for (kind, cost) in &marginal {
		eprintln!("{kind} adds {cost:?}");
	}
	eprintln!("dashboards() with {cards} cards: warm {warm:?}, right after an edit {after_edit:?}");
	assert!(
		default_warm <= TYPICAL,
		"the default dashboard took {default_warm:?}"
	);
	for (kind, cost) in &marginal {
		assert!(*cost <= TYPICAL, "{kind} adds {cost:?} to the dashboard");
	}
	session.close().unwrap();
}
