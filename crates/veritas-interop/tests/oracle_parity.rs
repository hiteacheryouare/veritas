//! The Rust canonical dump against `tools/actual-oracle dump --raw-only`
//! (PLAN §6.4, contracts/canonical-dump.md). Byte equality is the contract.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Scratch, assert_same_dump, fixture, oracle_raw_dump, pack, unpack};
use veritas_interop::canonical_dump_to_vec;

fn assert_parity(zip: &std::path::Path, label: &str) -> Option<Vec<u8>> {
	let ours = canonical_dump_to_vec(zip).unwrap();
	let theirs = oracle_raw_dump(zip)?;
	assert_same_dump(&ours, &theirs, label);
	Some(ours)
}

#[test]
fn small_dumps_byte_identical_to_the_oracle_r_io_3() {
	assert_parity(&fixture("small"), "small");
}

#[test]
fn all_features_dumps_byte_identical_to_the_oracle_r_io_3() {
	assert_parity(&fixture("all-features"), "all-features");
}

#[test]
fn perf_100k_dumps_byte_identical_to_the_oracle_r_io_3() {
	assert_parity(&fixture("perf-100k"), "perf-100k");
}

#[test]
fn every_sqlite_storage_class_and_odd_name_dumps_like_the_oracle() {
	let scratch = Scratch::new("odd-cells");
	let (db_path, _) = unpack(&fixture("small"), scratch.path());
	let conn = rusqlite::Connection::open(&db_path).unwrap();
	conn.execute_batch(
		r#"
		CREATE TABLE "odd ""quoted"" table" (a, b);
		INSERT INTO "odd ""quoted"" table" VALUES
			(9e999, -9e999), (-0.0, 1e21), (1e-7, 0.1 + 0.2), (16384.0, 5e-324),
			(9223372036854775807, -9223372036854775808), (X'00ff10', X''),
			(CAST(X'F08080' AS TEXT), CAST(X'E28241' AS TEXT)), (CAST(X'EDA080' AS TEXT), NULL),
			('tab	quote" back\ nul' || char(0) || ' bell' || char(7) || char(127) || char(8232), 'é😀');
		CREATE TABLE composite (z TEXT, "Ünïcode" INTEGER, a REAL, PRIMARY KEY ("Ünïcode", z));
		INSERT INTO composite VALUES ('b', 2, 1.5), ('a', 2, NULL), ('c', 1, 3.0), (NULL, 1, 2.0);
		CREATE INDEX odd_index ON composite (a);
		CREATE VIEW odd_view AS SELECT * FROM composite;
		CREATE TRIGGER odd_trigger AFTER INSERT ON composite BEGIN SELECT 1; END;
		"#,
	)
	.unwrap();
	drop(conn);
	let mut metadata = br#"{"budgetName":"Caf\u00e9 \ud83d\ude00 \ud800","id":"x","n":[1.0,1e3,12345678901234567890,-0,0.000001,1e-7,1.5],"dup":1,"dup":{"b":true,"a":null},"nested":{"deep":[[],{}]},"userId":"u","groupId":null,"keep":"\u2028/\u007f","raw":""#.to_vec();
	metadata.extend_from_slice("é😀\u{2028}".as_bytes());
	metadata.extend_from_slice(b"\"}");
	let zip = scratch.join("odd.zip");
	pack(&db_path, &metadata, &zip);
	let ours = canonical_dump_to_vec(&zip).unwrap();
	let text = String::from_utf8(ours.clone()).unwrap();
	assert!(text.contains("{\"a\":{\"$real\":\"Infinity\"},\"b\":{\"$real\":\"-Infinity\"}}"));
	assert!(text.contains("\"a\":\"\u{FFFD}\u{FFFD}\u{FFFD}\",\"b\":\"\u{FFFD}A\""));
	assert!(!text.contains("\"id\": \"x\""));
	assert_parity(&zip, "odd cells");
}
