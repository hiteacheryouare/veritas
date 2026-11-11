#![allow(
	clippy::unwrap_used,
	clippy::expect_used,
	clippy::panic,
	reason = "integration tests fail by panicking"
)]

//! The crash harness (PLAN §6.6): a child process edits a real budget while
//! the parent SIGKILLs it at random moments, then reopens the budget. Every
//! reopen must find the file sound, the lock free and the last committed
//! action present. `cargo test` runs 25 kills; set VERITAS_CRASH_KILLS=1000
//! for the full run.

mod support;

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use support::TempDir;
use veritas_domain::{
	AccountId, BudgetCell, BudgetData, CategoryId, CoreError, Money, Row, YearMonth,
};
use veritas_session::{Session, SessionOptions, SnapshotReason};

type CrashSession = Session<veritas_store::Store>;

const CRASH_DIR: &str = "VERITAS_CRASH_DIR";

fn fixture(name: &str) -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("../../fixtures/synthetic")
		.join(format!("{name}.zip"))
}

/// The account and envelope every edit touches, chosen the same way in
/// both processes.
fn targets(data: &BudgetData) -> (AccountId, CategoryId) {
	let account = data
		.accounts
		.values()
		.find(|account| !account.tombstone)
		.map(|account| account.id.clone())
		.expect("a live account");
	let category = data
		.categories
		.values()
		.find(|category| !category.tombstone && !category.is_income)
		.map(|category| category.id.clone())
		.expect("a live expense category");
	(account, category)
}

fn month() -> YearMonth {
	YearMonth::new(2027, 3).unwrap()
}

/// The value both rows hold, or `None` if they disagree, which would mean
/// a transaction landed in half.
fn value(data: &BudgetData) -> Option<i64> {
	let (account, category) = targets(data);
	let named = data.accounts[&account]
		.name
		.strip_prefix("Edit ")
		.and_then(|n| n.parse::<i64>().ok())
		.unwrap_or(0);
	let budgeted = data
		.budget_cells
		.get(&(month(), category))
		.map(|cell| cell.budgeted.minor())
		.unwrap_or(0);
	(named == budgeted).then_some(named)
}

fn set_to(n: i64) -> impl Fn(&BudgetData) -> Result<Vec<Row>, CoreError> {
	move |data: &BudgetData| {
		let (account, category) = targets(data);
		let mut renamed = data.accounts[&account].clone();
		renamed.name = format!("Edit {n}");
		Ok(vec![
			Row::Account(renamed),
			Row::BudgetCell(BudgetCell {
				month: month(),
				category,
				budgeted: Money::from_minor(n),
				carryover: false,
				goal: None,
				long_goal: false,
			}),
		])
	}
}

fn say(line: &str) {
	println!("\ncrash-{line}");
	std::io::stdout().flush().unwrap();
}

#[test]
#[ignore = "helper process, spawned and killed by the crash harness"]
fn edit_until_killed() {
	let Some(dir) = std::env::var_os(CRASH_DIR) else {
		return;
	};
	let mut session = CrashSession::open(Path::new(&dir), SessionOptions::default()).unwrap();
	assert_eq!(session.read_only(), None);
	let mut current = value(session.data().unwrap()).expect("the two rows agree");
	say(&format!("ready {current}"));
	let mut step: u64 = 0;
	loop {
		step += 1;
		match step % 9 {
			3 => {
				say(&format!("next {}", current - 1));
				session.undo().unwrap();
				say(&format!("committed {}", current - 1));
				say(&format!("next {current}"));
				session.redo().unwrap();
				say(&format!("committed {current}"));
			}
			7 => {
				session.snapshot(SnapshotReason::Hourly).unwrap();
			}
			_ => {
				let next = current + 1;
				say(&format!("next {next}"));
				session.perform(&set_to(next)).unwrap();
				current = next;
				say(&format!("committed {current}"));
			}
		}
	}
}

/// What the child said before it died.
struct Heard {
	committed: i64,
	in_flight: i64,
}

fn run_and_kill(dir: &Path, delay: Duration) -> Heard {
	let mut child = Command::new(std::env::current_exe().unwrap())
		.args([
			"--exact",
			"edit_until_killed",
			"--ignored",
			"--nocapture",
			"--test-threads=1",
		])
		.env(CRASH_DIR, dir)
		.stdin(Stdio::null())
		.stdout(Stdio::piped())
		.stderr(Stdio::null())
		.spawn()
		.unwrap();
	let stdout = child.stdout.take().unwrap();
	let (ready, started) = mpsc::channel();
	let reader = std::thread::spawn(move || {
		let mut heard: Option<Heard> = None;
		for line in BufReader::new(stdout).lines().map_while(Result::ok) {
			let Some(event) = line.strip_prefix("crash-") else {
				continue;
			};
			let (kind, n) = event.split_once(' ').unwrap();
			let n: i64 = n.parse().unwrap();
			match kind {
				"ready" => {
					heard = Some(Heard {
						committed: n,
						in_flight: n,
					});
					let _ = ready.send(());
				}
				"next" => heard.as_mut().unwrap().in_flight = n,
				"committed" => {
					let heard = heard.as_mut().unwrap();
					heard.committed = n;
					heard.in_flight = n;
				}
				other => panic!("unexpected event {other}"),
			}
		}
		heard
	});
	if started.recv_timeout(Duration::from_secs(60)).is_err() {
		let _ = child.kill();
		panic!("the editing process never opened the budget");
	}
	std::thread::sleep(delay);
	child.kill().unwrap();
	child.wait().unwrap();
	reader.join().unwrap().unwrap()
}

/// A small xorshift, so each run kills at the same moments.
struct Moments(u64);

impl Moments {
	fn next_delay(&mut self) -> Duration {
		self.0 ^= self.0 << 13;
		self.0 ^= self.0 >> 7;
		self.0 ^= self.0 << 17;
		Duration::from_micros(self.0 % 40_000)
	}
}

#[test]
fn a_killed_writer_never_damages_the_budget_or_loses_a_commit() {
	// R-REL-1, R-REL-6, PLAN §6.6 crash tests.
	let kills: usize = std::env::var("VERITAS_CRASH_KILLS")
		.ok()
		.and_then(|kills| kills.parse().ok())
		.unwrap_or(25);
	let root = TempDir::new("crash");
	let dir = root.path().join("budget");
	veritas_interop::import_actual_zip(&fixture("small"), &dir).unwrap();
	let baseline = {
		let mut session = CrashSession::open(&dir, SessionOptions::default()).unwrap();
		session.wait_for_open_check();
		session.audit_report().unwrap().clone()
	};
	let mut moments = Moments(0x9E37_79B9_7F4A_7C15);
	let mut highest = 0;

	for kill in 0..kills {
		let heard = run_and_kill(&dir, moments.next_delay());

		let mut session = CrashSession::open(&dir, SessionOptions::default()).unwrap();
		assert_eq!(
			session.read_only(),
			None,
			"kill {kill}: the lock stayed held"
		);
		let found = value(session.data().unwrap())
			.unwrap_or_else(|| panic!("kill {kill}: a transaction landed in half"));
		assert!(
			found == heard.committed || found == heard.in_flight,
			"kill {kill}: found {found}, but {} was committed and {} in flight",
			heard.committed,
			heard.in_flight
		);
		session.wait_for_open_check();
		let report = session.audit_report().unwrap();
		assert!(!report.is_corrupt(), "kill {kill}: {report:?}");
		assert_eq!(
			report, &baseline,
			"kill {kill}: the audit found something new"
		);
		highest = highest.max(found);
	}
	eprintln!("crash harness: {kills} kills, the budget reached edit {highest}");
}
