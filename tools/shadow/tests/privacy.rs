#![allow(
	clippy::unwrap_used,
	clippy::expect_used,
	clippy::panic,
	reason = "integration tests fail by panicking"
)]

//! The shadow run's privacy contract on budgets we can read: every line it
//! prints is a `shadow:` line, and none carries a name, note, id or amount
//! from the budget, whether every check passes or one fails.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

const CANARY: &str = "Qx7canary";
const CANARY_AMOUNT: i64 = 987_654_321;

fn root() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn ready() -> bool {
	let oracle = root().join("tools/actual-oracle/node_modules").is_dir();
	let reference = std::env::var_os("ACTUAL_REFERENCE_DIR")
		.map_or_else(|| root().join("reference/actual"), PathBuf::from)
		.is_dir();
	if !(oracle && reference) {
		assert!(
			std::env::var_os("VERITAS_REQUIRE_ORACLE").is_none(),
			"VERITAS_REQUIRE_ORACLE is set but the oracle or Actual's source is missing"
		);
		eprintln!("skipping: the shadow run needs the oracle and Actual's source");
	}
	oracle && reference
}

struct Scratch(PathBuf);

impl Scratch {
	fn new(label: &str) -> Scratch {
		let path = Path::new(env!("CARGO_TARGET_TMPDIR"))
			.join(format!("shadow-{label}-{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&path);
		std::fs::create_dir_all(path.join("run/tmp")).unwrap();
		Scratch(path)
	}
}

impl Drop for Scratch {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.0);
	}
}

fn fixture(name: &str) -> PathBuf {
	root().join(format!("fixtures/synthetic/{name}.zip"))
}

/// The shadow binary on `zip`, as `cargo xtask shadow` runs it: stdout, both
/// streams together, and whether it passed.
fn shadow(zip: &Path, scratch: &Scratch) -> (String, bool) {
	let run = scratch.0.join("run");
	let output = Command::new(env!("CARGO_BIN_EXE_veritas-shadow"))
		.arg(zip)
		.env("VERITAS_SHADOW_DIR", &run)
		.env("TMPDIR", run.join("tmp"))
		.output()
		.unwrap();
	let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
	text.push_str(&String::from_utf8_lossy(&output.stderr));
	(text, output.status.success())
}

fn unpack(zip: &Path, into: &Path) -> (PathBuf, Vec<u8>) {
	let mut archive = zip::ZipArchive::new(std::fs::File::open(zip).unwrap()).unwrap();
	let mut read = |name: &str| {
		let mut bytes = Vec::new();
		archive
			.by_name(name)
			.unwrap()
			.read_to_end(&mut bytes)
			.unwrap();
		bytes
	};
	let db = read("db.sqlite");
	let metadata = read("metadata.json");
	let db_path = into.join("db.sqlite");
	std::fs::write(&db_path, db).unwrap();
	(db_path, metadata)
}

fn pack(db_path: &Path, metadata: &[u8], zip: &Path) {
	let mut writer = zip::ZipWriter::new(std::fs::File::create(zip).unwrap());
	let options = zip::write::SimpleFileOptions::default();
	writer.start_file("db.sqlite", options).unwrap();
	writer.write_all(&std::fs::read(db_path).unwrap()).unwrap();
	writer.start_file("metadata.json", options).unwrap();
	writer.write_all(metadata).unwrap();
	writer.finish().unwrap();
}

/// A copy of small.zip with canary names, a note and an amount in it, then
/// `edit` applied.
fn edited_copy(scratch: &Scratch, label: &str, edit: &str) -> PathBuf {
	let dir = scratch.0.join(label);
	std::fs::create_dir_all(&dir).unwrap();
	let (db, metadata) = unpack(&fixture("small"), &dir);
	let connection = rusqlite::Connection::open(&db).unwrap();
	connection
		.execute_batch(&format!(
			"UPDATE accounts SET name = 'Checking {CANARY}' WHERE id = (SELECT id FROM accounts WHERE tombstone = 0 ORDER BY sort_order LIMIT 1);
			UPDATE categories SET name = 'Groceries {CANARY}' WHERE id = (SELECT id FROM categories WHERE tombstone = 0 AND is_income = 0 LIMIT 1);
			UPDATE payees SET name = 'Payee {CANARY}' WHERE id = (SELECT id FROM payees WHERE tombstone = 0 AND transfer_acct IS NULL LIMIT 1);
			UPDATE transactions SET notes = 'note {CANARY}', amount = -{CANARY_AMOUNT}
				WHERE id = (SELECT id FROM transactions WHERE tombstone = 0 AND isParent = 0 AND isChild = 0 AND transferred_id IS NULL LIMIT 1);
			{edit}"
		))
		.unwrap();
	drop(connection);
	let zip = dir.join(format!("{label}.zip"));
	pack(&db, &metadata, &zip);
	zip
}

/// Every name, note, id and amount the budget holds, in the forms a leak
/// could take.
fn secrets(zip: &Path, scratch: &Scratch) -> (Vec<String>, Vec<i64>) {
	let dir = scratch.0.join("secrets");
	std::fs::create_dir_all(&dir).unwrap();
	let (db, _) = unpack(zip, &dir);
	let connection = rusqlite::Connection::open(&db).unwrap();
	let mut words = Vec::new();
	for sql in [
		"SELECT name FROM accounts",
		"SELECT id FROM accounts",
		"SELECT name FROM categories",
		"SELECT id FROM categories",
		"SELECT name FROM category_groups",
		"SELECT name FROM payees",
		"SELECT id FROM payees",
		"SELECT notes FROM transactions",
		"SELECT name FROM custom_reports",
		"SELECT id FROM custom_reports",
		"SELECT id FROM dashboard",
		"SELECT name FROM dashboard_pages",
	] {
		let Ok(mut statement) = connection.prepare(sql) else {
			continue;
		};
		let rows = statement
			.query_map([], |row| row.get::<_, Option<String>>(0))
			.unwrap();
		words.extend(
			rows.flatten()
				.flatten()
				.filter(|text| text.chars().count() >= 4),
		);
	}
	let mut amounts = Vec::new();
	for sql in [
		"SELECT amount FROM transactions",
		"SELECT amount FROM zero_budgets",
	] {
		let Ok(mut statement) = connection.prepare(sql) else {
			continue;
		};
		let rows = statement
			.query_map([], |row| row.get::<_, Option<i64>>(0))
			.unwrap();
		amounts.extend(
			rows.flatten()
				.flatten()
				.map(i64::abs)
				.filter(|amount| *amount >= 1_000),
		);
	}
	std::fs::remove_dir_all(&dir).unwrap();
	(words, amounts)
}

/// `figure` in `text` with no digit, separator or point touching it.
fn stands(figure: &str, text: &str) -> bool {
	text.match_indices(figure).any(|(at, _)| {
		let touching =
			|c: Option<char>| c.is_some_and(|c| c.is_ascii_digit() || c == ',' || c == '.');
		!touching(text[..at].chars().next_back())
			&& !touching(text[at + figure.len()..].chars().next())
	})
}

fn grouped(units: i64) -> String {
	let digits = units.to_string();
	let mut out = String::new();
	for (index, digit) in digits.chars().enumerate() {
		if index > 0 && (digits.len() - index).is_multiple_of(3) {
			out.push(',');
		}
		out.push(digit);
	}
	out
}

fn assert_says_nothing_about(output: &str, zip: &Path, scratch: &Scratch) {
	for line in output.lines().filter(|line| !line.trim().is_empty()) {
		assert!(
			line.starts_with("shadow: "),
			"only shadow lines, got: {line}"
		);
	}
	assert!(
		!output.contains(CANARY),
		"a canary name or note leaked:\n{output}"
	);
	let (words, amounts) = secrets(zip, scratch);
	assert!(
		words.len() > 10 && amounts.len() > 10,
		"the budget has names and amounts to look for"
	);
	for word in &words {
		assert!(
			!output.contains(word.as_str()),
			"\"{word}\" leaked:\n{output}"
		);
	}
	for amount in amounts.iter().copied().chain([CANARY_AMOUNT]) {
		let cents = format!("{}.{:02}", amount / 100, amount % 100);
		let grouped_cents = format!("{}.{:02}", grouped(amount / 100), amount % 100);
		for form in [
			amount.to_string(),
			cents,
			grouped_cents,
			grouped(amount / 100),
		] {
			if form.len() >= 4 {
				assert!(
					!stands(&form, output),
					"the amount {form} leaked:\n{output}"
				);
			}
		}
	}
}

#[test]
fn the_fixtures_pass_every_check_and_print_only_counts() {
	if !ready() {
		return;
	}
	for name in ["small", "all-features"] {
		let scratch = Scratch::new(name);
		let (output, passed) = shadow(&fixture(name), &scratch);
		assert!(passed, "{name}:\n{output}");
		for check in [
			"round trip:",
			"months:",
			"accounts:",
			"widgets:",
			"custom reports:",
			"all checks pass",
		] {
			assert!(output.contains(check), "{name} reports {check}\n{output}");
		}
		assert_says_nothing_about(&output, &fixture(name), &scratch);
	}
}

#[test]
fn a_budget_full_of_canaries_passes_without_printing_one() {
	if !ready() {
		return;
	}
	let scratch = Scratch::new("canaries");
	let zip = edited_copy(&scratch, "canaries", "");
	let (output, passed) = shadow(&zip, &scratch);
	assert!(passed, "{output}");
	assert_says_nothing_about(&output, &zip, &scratch);
}

#[test]
fn a_broken_copy_fails_and_still_prints_nothing_from_the_budget() {
	// A transaction whose account isn't there: Actual still counts it, Veritas's
	// loader leaves it out (and reports it), so the two disagree.
	if !ready() {
		return;
	}
	let scratch = Scratch::new("broken");
	let zip = edited_copy(
		&scratch,
		"broken",
		&format!("UPDATE transactions SET acct = 'gone-{CANARY}' WHERE notes = 'note {CANARY}';"),
	);
	let (output, passed) = shadow(&zip, &scratch);
	assert!(!passed, "a broken budget fails:\n{output}");
	assert!(
		output.contains(" differ")
			|| output.contains("could not finish")
			|| output.contains("refused"),
		"it says which check failed:\n{output}"
	);
	assert_says_nothing_about(&output, &zip, &scratch);
}
