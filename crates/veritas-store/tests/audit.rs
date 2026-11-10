//! The invariant audit (R-REL-2) against the fixtures and hand-broken copies.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Scratch, fixture_db};
use rusqlite::Connection;
use veritas_store::{Access, Check, Store, Violation};

fn violations(db: &std::path::Path) -> Vec<Violation> {
	let (loaded, report) = Store::open(db, Access::ReadWrite)
		.unwrap()
		.open_audit()
		.unwrap();
	assert!(loaded.is_some());
	report.violations
}

#[test]
fn the_fixtures_pass_the_audit_r_rel_2() {
	for name in ["small", "all-features", "perf-100k"] {
		let scratch = Scratch::new(&format!("audit-{name}"));
		let found = violations(&fixture_db(name, &scratch));
		assert_eq!(found, vec![], "{name}");
	}
}

fn pick(conn: &Connection, sql: &str) -> Vec<String> {
	let mut statement = conn.prepare(sql).unwrap();
	statement
		.query_map([], |row| row.get::<_, String>(0))
		.unwrap()
		.map(Result::unwrap)
		.collect()
}

#[test]
fn every_broken_invariant_is_reported_and_nothing_is_fixed_r_rel_2() {
	let scratch = Scratch::new("audit-broken");
	let db_path = fixture_db("all-features", &scratch);
	let conn = Connection::open(&db_path).unwrap();
	let pairs = pick(
		&conn,
		"SELECT a.id FROM transactions a JOIN transactions b ON b.id = a.transferred_id \
		 WHERE b.transferred_id = a.id AND a.id < b.id AND a.tombstone = 0 AND b.tombstone = 0 \
		 AND a.isChild = 0 AND b.isChild = 0 ORDER BY a.id",
	);
	let (one_way, mismatched) = (&pairs[0], &pairs[1]);
	let one_way_partner = pick(
		&conn,
		&format!("SELECT transferred_id FROM transactions WHERE id = '{one_way}'"),
	)
	.remove(0);
	let mismatched_partner = pick(
		&conn,
		&format!("SELECT transferred_id FROM transactions WHERE id = '{mismatched}'"),
	)
	.remove(0);
	let child = pick(
		&conn,
		"SELECT c.id FROM transactions c JOIN transactions p ON p.id = c.parent_id \
		 WHERE c.isChild = 1 AND c.tombstone = 0 AND p.tombstone = 0 AND c.transferred_id IS NULL ORDER BY c.id LIMIT 1",
	)
	.remove(0);
	let parent = pick(
		&conn,
		&format!("SELECT parent_id FROM transactions WHERE id = '{child}'"),
	)
	.remove(0);
	let plain = pick(
		&conn,
		"SELECT id FROM transactions WHERE isParent = 0 AND isChild = 0 AND transferred_id IS NULL \
		 AND tombstone = 0 ORDER BY id LIMIT 1",
	)
	.remove(0);
	let account = pick(
		&conn,
		"SELECT acct FROM transactions WHERE tombstone = 0 ORDER BY id LIMIT 1",
	)
	.remove(0);
	let category = pick(
		&conn,
		"SELECT id FROM categories WHERE tombstone = 0 ORDER BY id LIMIT 1",
	)
	.remove(0);
	let payees = pick(
		&conn,
		"SELECT id FROM payees WHERE transfer_acct IS NULL AND tombstone = 0 ORDER BY id LIMIT 3",
	);
	conn.execute_batch(&format!(
		"UPDATE transactions SET transferred_id = NULL WHERE id = '{one_way_partner}';
		 UPDATE transactions SET amount = amount + 1 WHERE id = '{mismatched}';
		 UPDATE transactions SET amount = amount + 1 WHERE id = '{child}';
		 UPDATE transactions SET transferred_id = '{plain}' WHERE id = '{plain}';
		 INSERT INTO transactions (id, acct, amount, date, isParent) VALUES ('empty-split', '{account}', 0, 20261001, 1);
		 INSERT INTO transactions (id, acct, amount, date, isChild, parent_id) VALUES ('stepchild', '{account}', 5, 20261001, 1, '{plain}');
		 INSERT INTO transactions (id, acct, amount, date) VALUES ('ghost-account', 'ghost', 5, 20261001);
		 INSERT INTO transactions (id, acct, amount, date, category) VALUES ('unmapped', '{account}', 5, 20261001, 'nowhere');
		 INSERT INTO category_mapping VALUES ('loop-a', 'loop-b'), ('loop-b', 'loop-a');
		 UPDATE payee_mapping SET targetId = '{p1}' WHERE id = '{p0}';
		 UPDATE payee_mapping SET targetId = '{p2}' WHERE id = '{p1}';
		 INSERT INTO zero_budgets (id, month, category, amount) VALUES ('2026-10-{category}', 202610, '{category}', 1);
		 INSERT INTO zero_budgets (id, month, category, amount) VALUES ('202610-gone', 202610, 'gone', 1);
		 INSERT INTO transactions (id, acct, amount, date) VALUES ('no-date', '{account}', 5, NULL);",
		p0 = payees[0],
		p1 = payees[1],
		p2 = payees[2],
	))
	.unwrap();
	Store::open(&db_path, Access::ReadWrite)
		.unwrap()
		.adopt()
		.unwrap();
	conn.execute_batch(
		"INSERT INTO veritas_goal (id, category_id, target_cents, status, created_at) \
		 VALUES ('orphan-goal', 'deleted-in-actual', 100, 'active', 0);",
	)
	.unwrap();
	let before = std::fs::read(&db_path).unwrap();

	let found: Vec<(Check, String, Option<String>)> = violations(&db_path)
		.into_iter()
		.map(|violation| {
			(
				violation.check,
				violation.id.unwrap_or_default(),
				violation.related,
			)
		})
		.collect();
	let some = |id: &str| Some(id.to_owned());
	let expected = vec![
		(
			Check::UnpairedTransfer,
			one_way.clone(),
			some(&one_way_partner),
		),
		(Check::UnpairedTransfer, one_way_partner.clone(), None),
		(
			Check::TransferAmountMismatch,
			mismatched.clone().min(mismatched_partner.clone()),
			some(&mismatched.clone().max(mismatched_partner)),
		),
		(Check::SplitSumMismatch, parent, None),
		(Check::StrayTransferLink, plain.clone(), some(&plain)),
		(Check::EmptySplit, "empty-split".to_owned(), None),
		(
			Check::ChildOfNonParent,
			"stepchild".to_owned(),
			some(&plain),
		),
		(
			Check::MissingReference("account"),
			"ghost-account".to_owned(),
			some("ghost"),
		),
		(
			Check::MissingReference("category mapping"),
			"unmapped".to_owned(),
			some("nowhere"),
		),
		(Check::MappingCycle, "loop-a".to_owned(), some("loop-b")),
		(Check::MappingCycle, "loop-b".to_owned(), some("loop-a")),
		(Check::MappingChain, payees[0].clone(), some(&payees[1])),
		(
			Check::DuplicateBudgetCell,
			format!("2026-10-{category}"),
			None,
		),
		(
			Check::MissingReference("category"),
			"2026-10/gone".to_owned(),
			some("gone"),
		),
		(
			Check::Unrepresentable("the transaction has no date"),
			"no-date".to_owned(),
			None,
		),
		(
			Check::MissingReference("category"),
			"orphan-goal".to_owned(),
			some("deleted-in-actual"),
		),
	];
	for item in &expected {
		assert!(found.contains(item), "missing {item:?}\nfound {found:#?}");
	}
	assert_eq!(found.len(), expected.len(), "{found:#?}");
	assert_eq!(
		std::fs::read(&db_path).unwrap(),
		before,
		"the audit wrote to the file"
	);
}

#[test]
fn a_damaged_file_reports_corruption_and_is_not_loaded_r_rel_2() {
	let scratch = Scratch::new("audit-damaged");
	let db_path = fixture_db("all-features", &scratch);
	let conn = Connection::open(&db_path).unwrap();
	let root: i64 = conn
		.query_row(
			"SELECT rootpage FROM sqlite_master WHERE name = 'trans_date'",
			[],
			|row| row.get(0),
		)
		.unwrap();
	let page_size: i64 = conn
		.query_row("PRAGMA page_size", [], |row| row.get(0))
		.unwrap();
	drop(conn);
	let mut bytes = std::fs::read(&db_path).unwrap();
	let start = ((root - 1) * page_size) as usize;
	for byte in &mut bytes[start + 100..start + page_size as usize - 8] {
		*byte = byte.wrapping_add(0x55);
	}
	std::fs::write(&db_path, bytes).unwrap();

	let (loaded, report) = Store::open(&db_path, Access::ReadWrite)
		.unwrap()
		.open_audit()
		.unwrap();
	assert!(loaded.is_none());
	assert!(report.is_corrupt());
	assert!(
		report
			.violations
			.iter()
			.all(|violation| matches!(violation.check, Check::Integrity(_)))
	);
}

#[test]
fn unknown_tax_bucket_ids_are_reported_tax_3() {
	let scratch = Scratch::new("audit-tax");
	let db_path = fixture_db("small", &scratch);
	let mut store = Store::open(&db_path, Access::ReadWrite).unwrap();
	store.adopt().unwrap();
	let conn = Connection::open(&db_path).unwrap();
	let category: String = conn
		.query_row(
			"SELECT id FROM categories WHERE tombstone = 0 ORDER BY id LIMIT 1",
			[],
			|row| row.get(0),
		)
		.unwrap();
	// Another tool could write these: the schema's CHECKs guard only SQLite
	// connections that enforce them.
	conn.execute_batch(&format!(
		"PRAGMA ignore_check_constraints = ON;
		 INSERT INTO veritas_category_meta (category_id, tax_bucket) VALUES ('{category}', 'schedule-c');
		 INSERT INTO veritas_tax_tag VALUES ('rx', 'medical'), ('notax', 'none'), ('crypto', 'capital_gains');"
	))
	.unwrap();
	let found: Vec<(Check, String, Option<String>)> = violations(&db_path)
		.into_iter()
		.map(|violation| {
			(
				violation.check,
				violation.id.unwrap_or_default(),
				violation.related,
			)
		})
		.collect();
	assert_eq!(
		found,
		vec![
			(
				Check::UnknownTaxBucket,
				category,
				Some("schedule-c".to_owned())
			),
			(
				Check::UnknownTaxBucket,
				"crypto".to_owned(),
				Some("capital_gains".to_owned())
			),
		]
	);
}
