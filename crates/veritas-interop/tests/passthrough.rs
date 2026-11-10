//! Data Veritas doesn't model survives into the dump unchanged (prep for R-IO-5).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Scratch, assert_same_dump, fixture, oracle_raw_dump, pack, unpack};
use veritas_interop::canonical_dump_to_vec;

#[test]
fn an_unknown_table_and_an_unknown_column_appear_in_the_dump_r_io_5() {
	let scratch = Scratch::new("passthrough");
	let (db_path, metadata) = unpack(&fixture("small"), scratch.path());
	let conn = rusqlite::Connection::open(&db_path).unwrap();
	let transaction: String = conn
		.query_row(
			"SELECT id FROM transactions ORDER BY id LIMIT 1",
			[],
			|row| row.get(0),
		)
		.unwrap();
	conn.execute_batch(
		"CREATE TABLE future_feature (id TEXT PRIMARY KEY, payload BLOB, weight REAL);
		 INSERT INTO future_feature VALUES ('f1', X'cafe', 2.0), ('f0', NULL, 0.25);
		 ALTER TABLE transactions ADD COLUMN future_flag INTEGER DEFAULT 7;",
	)
	.unwrap();
	conn.execute(
		"UPDATE transactions SET future_flag = 42 WHERE id = ?1",
		[&transaction],
	)
	.unwrap();
	drop(conn);
	let zip = scratch.join("future.zip");
	pack(&db_path, &metadata, &zip);

	let before = String::from_utf8(canonical_dump_to_vec(&fixture("small")).unwrap()).unwrap();
	let after = String::from_utf8(canonical_dump_to_vec(&zip).unwrap()).unwrap();
	assert!(!before.contains("future_feature"));
	assert!(after.contains(
		"\t\t\"future_feature\": {\n\t\t\t\"primaryKey\": [\n\t\t\t\t\"id\"\n\t\t\t],\n\t\t\t\"rows\": [\n\t\t\t\t{\"id\":\"f0\",\"payload\":null,\"weight\":0.25},\n\t\t\t\t{\"id\":\"f1\",\"payload\":{\"$blob\":\"cafe\"},\"weight\":2.0}\n\t\t\t]\n\t\t}"
	));
	assert!(after.contains(&format!("\"future_flag\":42,\"id\":\"{transaction}\"")));
	let defaulted = after.matches("\"future_flag\":7,").count();
	let transactions = before.matches("\"financial_id\":").count();
	assert_eq!(defaulted, transactions - 1);
	assert!(after.contains("raw_synced_data text, future_flag INTEGER DEFAULT 7)"));

	if let Some(oracle) = oracle_raw_dump(&zip) {
		assert_same_dump(
			after.as_bytes(),
			&oracle,
			"small with unknown table and column",
		);
	}
}
