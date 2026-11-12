//! Family 3 of contracts/canonical-dump.md: Actual itself imports every
//! Veritas export without errors, computes the same budget from it, and
//! keeps every table, `veritas_*` included (R-IO-2, R-IO-6).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Scratch, fixture, oracle, round_trip};
use veritas_interop::canonical_dump_to_vec;

fn computed_section(dump: &[u8]) -> String {
	let text = String::from_utf8(dump.to_vec()).unwrap();
	let start = text
		.find("\t\"computed\": ")
		.expect("a full dump has computed values");
	let end = text.find("\n\t\"format\": ").unwrap();
	text[start..end].to_owned()
}

fn assert_actual_accepts(name: &str) {
	let scratch = Scratch::new(&format!("acceptance-{name}"));
	let exported = round_trip(&fixture(name), &scratch);

	let Some(original) = oracle(&["dump".as_ref(), fixture(name).as_os_str()]) else {
		return;
	};
	let ours = oracle(&["dump".as_ref(), exported.as_os_str()]).unwrap();
	assert_eq!(
		computed_section(&ours),
		computed_section(&original),
		"{name}: Actual computes a different budget"
	);

	let no_ops = scratch.join("no-ops.json");
	std::fs::write(&no_ops, br#"{"version":1,"ops":[]}"#).unwrap();
	let reexported = scratch.join("reexported.zip");
	oracle(&[
		"apply".as_ref(),
		exported.as_os_str(),
		no_ops.as_os_str(),
		"--out".as_ref(),
		reexported.as_os_str(),
	])
	.unwrap();
	let before = String::from_utf8(canonical_dump_to_vec(&exported).unwrap()).unwrap();
	let after = String::from_utf8(canonical_dump_to_vec(&reexported).unwrap()).unwrap();
	let before: Vec<&str> = before.lines().collect();
	let after: Vec<&str> = after.lines().collect();
	assert_eq!(before.len(), after.len(), "{name}");
	let differing: Vec<(&str, &str)> = before
		.iter()
		.zip(&after)
		.filter(|(a, b)| a != b)
		.map(|(a, b)| (*a, *b))
		.collect();
	// Actual's resetClock gives the file a new CRDT node id, and nothing else changes.
	assert_eq!(differing.len(), 1, "{name}: {differing:#?}");
	assert!(differing[0].0.contains("\"clock\":"), "{differing:?}");
	assert!(before.iter().any(|line| line.contains("\"veritas_meta\"")));
}

#[test]
fn actual_accepts_the_small_export() {
	assert_actual_accepts("small");
}

#[test]
fn actual_accepts_the_all_features_export() {
	assert_actual_accepts("all-features");
}

#[test]
fn actual_accepts_the_perf_100k_export() {
	assert_actual_accepts("perf-100k");
}
