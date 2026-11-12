//! R-REL-1, R-REL-6: a window that opens while another process writes must
//! load one committed state, never part of an action.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use common::{Scratch, fixture_db};
use rusqlite::Connection;
use veritas_domain::{AccountId, BudgetData, YearMonth};

/// The account name and the budgeted amount the writer keeps equal.
fn pair(data: &BudgetData, account: &AccountId, month: YearMonth, category: &str) -> (i64, i64) {
	let named = data.accounts[account]
		.name
		.strip_prefix("Edit ")
		.and_then(|n| n.parse().ok())
		.unwrap_or(-1);
	let budgeted = data
		.budget_cells
		.iter()
		.find(|((m, c), _)| *m == month && c.as_str() == category)
		.map(|(_, cell)| cell.budgeted.minor())
		.unwrap_or(-2);
	(named, budgeted)
}

#[test]
fn a_load_sees_one_committed_state_while_another_connection_writes() {
	let scratch = Scratch::new("load-snapshot");
	let db = fixture_db("small", &scratch);
	let setup = Connection::open(&db).unwrap();
	setup.execute_batch("PRAGMA journal_mode = WAL").unwrap();
	let account: String = setup
		.query_row(
			"SELECT id FROM accounts WHERE tombstone = 0 LIMIT 1",
			[],
			|row| row.get(0),
		)
		.unwrap();
	let (cell, month, category): (String, i64, String) = setup
		.query_row(
			"SELECT id, month, category FROM zero_budgets LIMIT 1",
			[],
			|row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
		)
		.unwrap();
	setup
		.execute_batch(&format!(
			"UPDATE accounts SET name = 'Edit 0' WHERE id = '{account}';
			UPDATE zero_budgets SET amount = 0 WHERE id = '{cell}';"
		))
		.unwrap();
	drop(setup);
	let month = YearMonth::new((month / 100) as i32, (month % 100) as u32).unwrap();
	let account_id = AccountId::new(account.clone());

	let stop = Arc::new(AtomicBool::new(false));
	let writer = {
		let (db, stop) = (db.clone(), stop.clone());
		std::thread::spawn(move || {
			let conn = Connection::open(&db).unwrap();
			conn.busy_timeout(std::time::Duration::from_secs(5))
				.unwrap();
			let mut n = 0;
			while !stop.load(Ordering::Relaxed) {
				n += 1;
				conn.execute_batch(&format!(
					"BEGIN IMMEDIATE;
					UPDATE accounts SET name = 'Edit {n}' WHERE id = '{account}';
					UPDATE zero_budgets SET amount = {n} WHERE id = '{cell}';
					COMMIT;"
				))
				.unwrap();
			}
			n
		})
	};

	let reader = veritas_store::open_read_only(&db).unwrap();
	let mut torn = 0;
	for _ in 0..200 {
		let loaded = veritas_store::load(&reader).unwrap();
		let (named, budgeted) = pair(&loaded.data, &account_id, month, &category);
		torn += usize::from(named != budgeted);
	}
	stop.store(true, Ordering::Relaxed);
	let writes = writer.join().unwrap();

	assert!(writes > 0);
	assert_eq!(
		torn, 0,
		"{torn} of 200 loads mixed two states ({writes} writes)"
	);
}
