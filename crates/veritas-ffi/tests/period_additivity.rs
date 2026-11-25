#![allow(
	clippy::unwrap_used,
	clippy::expect_used,
	clippy::panic,
	reason = "integration tests fail by panicking"
)]

//! R-AI-1's period tools on the real core: `net_profit_period` is additive.
//! Any day splits a range into two whose sections and groups add up, to the
//! cent and row for row, to the whole range. A whole month is the month
//! report itself.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::path::{Path, PathBuf};
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

/// A folder removed when the test ends, passed or failed.
struct Scratch(PathBuf);

impl Scratch {
	fn new(label: &str) -> Self {
		let path =
			Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{label}-{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&path);
		std::fs::create_dir_all(&path).unwrap();
		Scratch(path)
	}
}

impl Drop for Scratch {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.0);
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

/// What a split must preserve: a range's Net Profit column and each
/// section's totals by group id (income, expenses, debt paydown, savings).
struct Totals {
	column: NetProfitColumn,
	groups: [BTreeMap<String, i64>; 4],
}

/// `left` and `right` split `whole` after `split`: every section, Net
/// Profit and every group of every section add up to it. A group missing
/// from any of the three counts as 0, so a group in only one half is
/// checked too.
fn assert_adds_up(whole: &Totals, left: &Totals, right: &Totals, split: &str) {
	let summed: Vec<(i64, u64)> = sections(&left.column)
		.iter()
		.zip(sections(&right.column))
		.map(|(l, r)| (l.0 + r.0, l.1 + r.1))
		.collect();
	assert_eq!(
		summed,
		sections(&whole.column),
		"sections split after {split}"
	);
	assert_eq!(
		left.column.net_profit.total + right.column.net_profit.total,
		whole.column.net_profit.total,
		"Net Profit split after {split}"
	);
	for ((whole, left), right) in whole.groups.iter().zip(&left.groups).zip(&right.groups) {
		let ids: BTreeSet<&String> = whole
			.keys()
			.chain(left.keys())
			.chain(right.keys())
			.collect();
		for id in ids {
			let total = |groups: &BTreeMap<String, i64>| groups.get(id).copied().unwrap_or(0);
			assert_eq!(
				total(left) + total(right),
				total(whole),
				"group {id} split after {split}"
			);
		}
	}
}

#[test]
fn any_day_splits_a_net_profit_range_into_two_that_add_up_to_it() {
	let scratch = Scratch::new("period-additivity");
	let session = block_on(Session::import_actual_zip(
		Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("../../fixtures/synthetic/all-features.zip")
			.display()
			.to_string(),
		scratch.0.join("budget").display().to_string(),
		CancelToken::new(),
	))
	.unwrap();
	let range = |first: &str, last: &str| {
		let period = block_on(session.net_profit_period(
			first.to_owned(),
			last.to_owned(),
			CancelToken::new(),
		))
		.unwrap();
		macro_rules! by_id {
			($groups:expr) => {
				$groups
					.iter()
					.map(|group| (group.group_id.clone(), group.total))
					.collect::<BTreeMap<String, i64>>()
			};
		}
		Totals {
			groups: [
				by_id!(period.income_groups),
				by_id!(period.expense_groups),
				by_id!(period.debt_paydown_groups),
				by_id!(period.savings_groups),
			],
			column: period.column,
		}
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
		assert_adds_up(&whole, &left, &right, &format!("October {day}"));
	}

	for (first, last_of_left, first_of_right, last) in [
		("2026-09-15", "2026-09-30", "2026-10-01", "2026-10-15"),
		("2026-02-15", "2026-02-28", "2026-03-01", "2026-03-15"),
	] {
		let (left, right) = (range(first, last_of_left), range(first_of_right, last));
		for half in [&left, &right] {
			assert!(
				half.column.income.count + half.column.expenses.count > 0,
				"{first} to {last} has rows on both sides of the month boundary"
			);
		}
		assert_adds_up(&range(first, last), &left, &right, last_of_left);
	}
	session.close().unwrap();
}
