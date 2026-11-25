#![allow(
	clippy::unwrap_used,
	clippy::expect_used,
	clippy::panic,
	reason = "integration tests fail by panicking"
)]

//! R-AI-1's period tools on the real core: `net_profit_period` is additive.
//! Any day splits a month into two ranges whose sections add up, to the cent
//! and row for row, to the whole month, which is the month report itself.

use std::future::Future;
use std::path::Path;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread::Thread;

use veritas_ffi::{CancelToken, Month, NetProfitColumn, Session};

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

fn sections(column: &NetProfitColumn) -> [(i64, u64); 4] {
	[
		(column.income.total, column.income.count),
		(column.expenses.total, column.expenses.count),
		(column.debt_paydown.total, column.debt_paydown.count),
		(column.savings.total, column.savings.count),
	]
}

#[test]
fn any_day_splits_a_months_net_profit_into_two_ranges_that_add_up_to_it() {
	let scratch = Path::new(env!("CARGO_TARGET_TMPDIR"))
		.join(format!("period-additivity-{}", std::process::id()));
	let _ = std::fs::remove_dir_all(&scratch);
	std::fs::create_dir_all(&scratch).unwrap();
	let session = block_on(Session::import_actual_zip(
		Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("../../fixtures/synthetic/all-features.zip")
			.display()
			.to_string(),
		scratch.join("budget").display().to_string(),
		CancelToken::new(),
	))
	.unwrap();
	let range = |first: &str, last: &str| {
		block_on(session.net_profit_period(first.to_owned(), last.to_owned(), CancelToken::new()))
			.unwrap()
	};
	let october = Month {
		year: 2026,
		month: 10,
	};
	let report = block_on(session.net_profit(october, october, CancelToken::new())).unwrap();
	let whole = range("2026-10-01", "2026-10-31");
	assert_eq!(
		whole.column, report.total,
		"the whole month is the month report"
	);
	assert!(
		whole.column.expenses.count > 3,
		"October has spending to split"
	);

	for day in 1..31 {
		let left = range("2026-10-01", &format!("2026-10-{day:02}"));
		let right = range(&format!("2026-10-{:02}", day + 1), "2026-10-31");
		let summed: Vec<(i64, u64)> = sections(&left.column)
			.iter()
			.zip(sections(&right.column))
			.map(|(l, r)| (l.0 + r.0, l.1 + r.1))
			.collect();
		assert_eq!(summed, sections(&whole.column), "split after October {day}");
		assert_eq!(
			left.column.net_profit.total + right.column.net_profit.total,
			whole.column.net_profit.total,
			"Net Profit after October {day}"
		);
		for (whole_groups, left_groups, right_groups) in [
			(
				&whole.expense_groups,
				&left.expense_groups,
				&right.expense_groups,
			),
			(
				&whole.income_groups,
				&left.income_groups,
				&right.income_groups,
			),
		] {
			for group in whole_groups {
				let left_part = left_groups
					.iter()
					.find(|g| g.group_id == group.group_id)
					.map_or(0, |g| g.total);
				let right_part = right_groups
					.iter()
					.find(|g| g.group_id == group.group_id)
					.map_or(0, |g| g.total);
				assert_eq!(
					left_part + right_part,
					group.total,
					"{} after October {day}",
					group.name
				);
			}
		}
	}
	session.close().unwrap();
	let _ = std::fs::remove_dir_all(&scratch);
}
