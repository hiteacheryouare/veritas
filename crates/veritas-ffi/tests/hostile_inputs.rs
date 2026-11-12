#![allow(
	clippy::unwrap_used,
	clippy::expect_used,
	clippy::panic,
	reason = "integration tests fail by panicking"
)]

//! R-REL-7 through the exported API, exactly as Swift calls it: whatever the
//! app hands the core comes back as a typed error, never as `Internal` (a
//! caught bug), and a refused open, import or create leaves the user's
//! folders as they were.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::Thread;

use veritas_ffi::{
	FfiError, NewAccount, NewAccountKind, NewBudgetOptions, NumberFormat, NumberStyle, Session,
	evaluate_amount, list_budgets, parse_month,
};

/// Actual's `MAX_SAFE_NUMBER` (loot-core/src/shared/util.ts), the largest
/// amount its budget math accepts.
const ACTUAL_MAX_AMOUNT: i64 = (1 << 51) - 1;

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

impl Scratch {
	fn new(label: &str) -> Self {
		static NEXT: AtomicUsize = AtomicUsize::new(0);
		let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
			"ffi-hostile-{label}-{}-{}",
			std::process::id(),
			NEXT.fetch_add(1, Ordering::Relaxed)
		));
		let _ = std::fs::remove_dir_all(&path);
		std::fs::create_dir_all(&path).unwrap();
		Scratch(path)
	}

	fn path(&self) -> &Path {
		&self.0
	}

	fn join(&self, name: &str) -> String {
		self.0.join(name).display().to_string()
	}
}

impl Drop for Scratch {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.0);
	}
}

/// Every file and folder under `dir`, hidden ones included, with file sizes,
/// so "nothing changed" also catches a half-built hidden staging folder.
fn tree(dir: &Path) -> Vec<(String, Option<u64>)> {
	let mut found = Vec::new();
	let mut pending = vec![dir.to_path_buf()];
	while let Some(next) = pending.pop() {
		for entry in std::fs::read_dir(&next).unwrap() {
			let entry = entry.unwrap();
			let path = entry.path();
			let relative = path.strip_prefix(dir).unwrap().display().to_string();
			if entry.file_type().unwrap().is_dir() {
				found.push((relative, None));
				pending.push(path);
			} else {
				found.push((relative, Some(entry.metadata().unwrap().len())));
			}
		}
	}
	found.sort();
	found
}

fn small_zip() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/small.zip")
}

fn assert_typed<T>(what: &str, result: Result<T, FfiError>) -> FfiError {
	match result {
		Ok(_) => panic!("{what}: expected a typed error, but it succeeded"),
		Err(FfiError::Internal { message }) => panic!("{what}: the core hit a bug: {message}"),
		Err(error) => error,
	}
}

fn usd() -> NumberFormat {
	NumberFormat {
		currency: Some("USD".to_owned()),
		style: NumberStyle::CommaDot,
		hide_decimals: false,
	}
}

fn with_accounts(accounts: Vec<NewAccount>) -> NewBudgetOptions {
	NewBudgetOptions {
		retirement_401k: false,
		hsa: false,
		accounts,
	}
}

fn account(name: &str, kind: NewAccountKind, starting_balance: i64) -> NewAccount {
	NewAccount {
		name: name.to_owned(),
		kind,
		starting_balance,
	}
}

fn create(
	name: &str,
	directory: String,
	options: NewBudgetOptions,
) -> Result<Arc<Session>, FfiError> {
	block_on(Session::create_budget(
		name.to_owned(),
		directory,
		usd(),
		options,
	))
}

#[test]
fn opening_a_folder_that_holds_no_budget_is_a_typed_error_that_changes_nothing() {
	let scratch = Scratch::new("open");
	let file = scratch.path().join("notes.txt");
	std::fs::write(&file, "groceries 42.00").unwrap();
	let empty = scratch.path().join("Empty");
	std::fs::create_dir(&empty).unwrap();
	let garbage = scratch.path().join("Garbage");
	std::fs::create_dir(&garbage).unwrap();
	std::fs::write(garbage.join("db.sqlite"), vec![0xA5; 8192]).unwrap();
	std::fs::write(garbage.join("metadata.json"), "{\"id\": \"x\"").unwrap();
	let truncated = scratch.path().join("Truncated");
	std::fs::create_dir(&truncated).unwrap();
	std::fs::write(truncated.join("db.sqlite"), b"").unwrap();
	std::fs::write(truncated.join("metadata.json"), "{}").unwrap();
	let before = tree(scratch.path());

	for (what, directory) in [
		("a missing folder", scratch.join("Gone")),
		("an empty path", String::new()),
		("a file", file.display().to_string()),
		("an empty folder", empty.display().to_string()),
		(
			"a NUL in the path",
			format!("{}\0budget", scratch.path().display()),
		),
		("random bytes for db.sqlite", garbage.display().to_string()),
		("an empty db.sqlite", truncated.display().to_string()),
	] {
		assert_typed(what, block_on(Session::open(directory)));
	}

	let after: Vec<_> = tree(scratch.path())
		.into_iter()
		.filter(|(path, _)| !path.ends_with("veritas.lock"))
		.collect();
	assert_eq!(after, before);
}

#[test]
fn importing_something_that_is_not_an_actual_export_leaves_no_budget_behind() {
	let scratch = Scratch::new("import");
	let garbage = scratch.path().join("garbage.zip");
	std::fs::write(
		&garbage,
		(0..50_000u32)
			.map(|i| (i * 31 % 251) as u8)
			.collect::<Vec<_>>(),
	)
	.unwrap();
	let empty = scratch.path().join("empty.zip");
	std::fs::write(&empty, b"").unwrap();
	let real = std::fs::read(small_zip()).unwrap();
	let truncated = scratch.path().join("truncated.zip");
	std::fs::write(&truncated, &real[..real.len() / 2]).unwrap();
	let tail_cut = scratch.path().join("tail-cut.zip");
	std::fs::write(&tail_cut, &real[..real.len() - 22]).unwrap();
	let folder_as_zip = scratch.path().join("folder.zip");
	std::fs::create_dir(&folder_as_zip).unwrap();
	let taken = scratch.path().join("Taken");
	std::fs::create_dir(&taken).unwrap();
	std::fs::write(taken.join("keep.txt"), "mine").unwrap();
	let before = tree(scratch.path());

	let path = |p: &Path| p.display().to_string();
	for (what, zip, into) in [
		("random bytes", path(&garbage), scratch.join("A")),
		("an empty file", path(&empty), scratch.join("B")),
		("half of a real export", path(&truncated), scratch.join("C")),
		(
			"an export missing its central directory",
			path(&tail_cut),
			scratch.join("D"),
		),
		(
			"a folder named .zip",
			path(&folder_as_zip),
			scratch.join("E"),
		),
		(
			"a zip that doesn't exist",
			scratch.join("missing.zip"),
			scratch.join("F"),
		),
		(
			"a destination that exists",
			path(&small_zip()),
			path(&taken),
		),
		(
			"a destination whose parent doesn't exist",
			path(&small_zip()),
			scratch.join("no/such/G"),
		),
		("an empty destination", path(&small_zip()), String::new()),
		(
			"a destination ending in ..",
			path(&small_zip()),
			format!("{}/..", scratch.path().display()),
		),
	] {
		assert_typed(what, block_on(Session::import_actual_zip(zip, into)));
		assert_eq!(tree(scratch.path()), before, "{what} left something behind");
	}
}

#[test]
fn setup_refuses_a_bad_name_or_destination_and_builds_nothing() {
	let scratch = Scratch::new("create-bad");
	let taken = scratch.path().join("Taken");
	std::fs::create_dir(&taken).unwrap();
	let before = tree(scratch.path());
	let checking = || with_accounts(vec![account("Checking", NewAccountKind::Checking, 120_000)]);

	for (what, name, directory, options) in [
		("a blank budget name", "   ", scratch.join("A"), checking()),
		(
			"a blank account name",
			"Household",
			scratch.join("B"),
			with_accounts(vec![account(" \t", NewAccountKind::Savings, 0)]),
		),
		(
			"a folder that exists",
			"Household",
			taken.display().to_string(),
			checking(),
		),
		(
			"a parent that doesn't exist",
			"Household",
			scratch.join("no/such/C"),
			checking(),
		),
		(
			"an empty destination",
			"Household",
			String::new(),
			checking(),
		),
	] {
		assert_typed(what, create(name, directory, options));
		assert_eq!(tree(scratch.path()), before, "{what} left something behind");
	}
}

#[test]
fn setup_refuses_a_starting_balance_actual_cannot_hold() {
	let scratch = Scratch::new("create-overflow");
	for (kind, typed) in [
		(NewAccountKind::Checking, ACTUAL_MAX_AMOUNT + 1),
		(NewAccountKind::Checking, i64::MAX),
		(NewAccountKind::Checking, -(ACTUAL_MAX_AMOUNT + 1)),
		(NewAccountKind::Checking, i64::MIN),
		(NewAccountKind::Investment, ACTUAL_MAX_AMOUNT + 1),
		(NewAccountKind::CreditCard, ACTUAL_MAX_AMOUNT + 1),
		(NewAccountKind::Mortgage, i64::MAX),
		(NewAccountKind::Mortgage, i64::MIN),
	] {
		let refused = create(
			"Household",
			scratch.join("Household"),
			with_accounts(vec![account("Big", kind, typed)]),
		);
		assert!(
			matches!(refused, Err(FfiError::Overflow)),
			"{kind:?} starting at {typed}: {:?}",
			refused.err()
		);
		assert_eq!(tree(scratch.path()), [], "{kind:?} starting at {typed}");
	}
}

/// What Actual's own API computes for a zip, with its clock pinned to today.
/// `None` without the oracle, unless CI requires it.
fn actual_reads(zip: &Path) -> Option<serde_json::Value> {
	let oracle = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/actual-oracle");
	if !oracle.join("node_modules").exists() {
		assert!(
			std::env::var_os("VERITAS_REQUIRE_ORACLE").is_none(),
			"VERITAS_REQUIRE_ORACLE is set but tools/actual-oracle/node_modules is missing"
		);
		eprintln!("skipping the Actual side: run `cargo xtask oracle setup`");
		return None;
	}
	let today = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
		.format("%Y-%m-%d")
		.to_string();
	let output = std::process::Command::new("node")
		.current_dir(&oracle)
		.args(["src/main.ts", "dump"])
		.arg(zip)
		.args(["--today", &today])
		.output()
		.expect("node runs");
	assert!(
		output.status.success(),
		"Actual couldn't read the export: {}",
		String::from_utf8_lossy(&output.stderr)
	);
	Some(serde_json::from_slice(&output.stdout).unwrap())
}

#[test]
fn a_budget_set_up_at_actuals_largest_amounts_exports_with_those_balances() {
	// The amount field caps what setup can type at Actual's limit, so these
	// are the largest balances a real user can start with. The one on-budget
	// account puts To Budget exactly at the limit.
	let scratch = Scratch::new("create-limit");
	let session = create(
		"Household",
		scratch.join("Household"),
		with_accounts(vec![
			account("Windfall", NewAccountKind::Checking, ACTUAL_MAX_AMOUNT),
			account("Estate", NewAccountKind::Investment, ACTUAL_MAX_AMOUNT),
			account("Underwater", NewAccountKind::Mortgage, ACTUAL_MAX_AMOUNT),
		]),
	)
	.unwrap();
	let balances: std::collections::BTreeMap<String, (String, i64)> = session
		.overview()
		.unwrap()
		.groups
		.iter()
		.flat_map(|group| group.accounts.iter())
		.map(|account| (account.id.clone(), (account.name.clone(), account.balance)))
		.collect();
	let by_name = |name: &str| balances.values().find(|(n, _)| n == name).unwrap().1;
	assert_eq!(by_name("Windfall"), ACTUAL_MAX_AMOUNT);
	assert_eq!(by_name("Estate"), ACTUAL_MAX_AMOUNT);
	assert_eq!(by_name("Underwater"), -ACTUAL_MAX_AMOUNT);

	let zip = scratch.path().join("Household.zip");
	let findings = block_on(session.export_actual_zip(zip.display().to_string())).unwrap();
	assert!(findings.is_empty(), "{findings:?}");
	session.close().unwrap();
	let Some(actual) = actual_reads(&zip) else {
		return;
	};
	let computed = &actual["computed"]["accounts"];
	for (id, (name, balance)) in &balances {
		assert_eq!(
			computed[id]["balance"].as_i64(),
			Some(*balance),
			"{name}: Actual reads {}",
			computed[id]
		);
	}
}

/// A small xorshift, so every run types the same strings.
struct Typist(u64);

impl Typist {
	fn below(&mut self, n: usize) -> usize {
		self.0 ^= self.0 << 13;
		self.0 ^= self.0 >> 7;
		self.0 ^= self.0 << 17;
		(self.0 % n as u64) as usize
	}

	fn text(&mut self, keys: &[&str], longest: usize) -> String {
		let len = self.below(longest + 1);
		(0..len).map(|_| keys[self.below(keys.len())]).collect()
	}
}

#[test]
fn whatever_is_typed_in_an_amount_field_comes_back_as_an_amount_or_a_typed_refusal() {
	// R-REL-7, R-BUD-3: the field hands the core every keystroke the user
	// commits, pasted text included.
	const KEYS: &[&str] = &[
		"0", "1", "5", "9", "00", ".", ",", " ", "'", "+", "-", "*", "/", "^", "(", ")", "e", "E",
		"$", "€", "\u{a0}", "\u{2212}", "٣", "１", "∞", "NaN", "Infinity", "0x", "%", "\t",
	];
	let styles = [
		NumberStyle::CommaDot,
		NumberStyle::DotComma,
		NumberStyle::SpaceComma,
		NumberStyle::ApostropheDot,
		NumberStyle::CommaDotIn,
	];
	let mut typist = Typist(0x2545_F491_4F6C_DD1D);
	let mut amounts = 0;
	for attempt in 0..20_000 {
		let text = typist.text(KEYS, 24);
		let style = styles[attempt % styles.len()];
		match evaluate_amount(text.clone(), style) {
			Ok(minor) => {
				assert!(minor.abs() <= ACTUAL_MAX_AMOUNT, "{text:?} gave {minor}");
				amounts += 1;
			}
			Err(FfiError::Invalid { .. } | FfiError::Overflow) => {}
			Err(other) => panic!("{text:?} in {style:?}: {other:?}"),
		}
	}
	assert!(amounts > 1_000, "only {amounts} of 20000 were amounts");

	for text in [
		"9".repeat(100_000),
		format!("{}1", "(".repeat(10_000)),
		"1+".repeat(50_000),
		"9^9^9^9".to_owned(),
		"1e308*1e308".to_owned(),
		"-".repeat(1000) + "5",
	] {
		match evaluate_amount(text.clone(), NumberStyle::CommaDot) {
			Ok(minor) => assert!(minor.abs() <= ACTUAL_MAX_AMOUNT),
			Err(FfiError::Invalid { .. } | FfiError::Overflow) => {}
			Err(other) => panic!("{}…: {other:?}", &text[..20.min(text.len())]),
		}
	}
}

#[test]
fn a_month_or_a_budgets_folder_from_anywhere_never_reaches_a_bug() {
	let mut typist = Typist(0x9E37_79B9_7F4A_7C15);
	const KEYS: &[&str] = &[
		"2", "0", "7", "1", "9", "-", "--", " ", "１", "+", "13", "00",
	];
	for _ in 0..5_000 {
		let text = typist.text(KEYS, 12);
		if let Err(FfiError::Internal { message }) = parse_month(text.clone()) {
			panic!("{text:?}: {message}");
		}
	}

	let scratch = Scratch::new("list");
	let junk = scratch.path().join("Junk");
	std::fs::create_dir(&junk).unwrap();
	std::fs::write(junk.join("db.sqlite"), b"not sqlite").unwrap();
	std::fs::write(junk.join("metadata.json"), [0xFF, 0xFE, 0x00]).unwrap();
	std::fs::write(scratch.path().join("loose.txt"), "x").unwrap();
	assert_typed(
		"a file as the budgets folder",
		list_budgets(scratch.join("loose.txt")),
	);
	assert_eq!(list_budgets(scratch.join("missing")).unwrap(), []);
	assert_eq!(
		list_budgets(scratch.path().display().to_string()).unwrap(),
		[]
	);
}
