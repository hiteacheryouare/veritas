#![allow(
	clippy::unwrap_used,
	clippy::expect_used,
	clippy::panic,
	reason = "integration tests fail by panicking"
)]

//! The crash harness (PLAN §6.6): a child process edits a real budget while
//! the parent SIGKILLs it at random moments, then reopens the budget. Every
//! reopen must find the file sound, the lock free and the last committed
//! action present. A second child creates budgets until it is killed, and
//! every budget folder it leaves must be whole. `cargo test` runs 25 kills
//! of each; set VERITAS_CRASH_KILLS=1000 for the full run.

mod support;

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use support::TempDir;
use veritas_domain::{
	AccountId, BudgetCell, BudgetData, CategoryId, CoreError, Money, NewAccount, NewAccountKind,
	NewBudget, NumberFormat, NumberStyle, ReadOnlyReason, Row, YearMonth,
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
	run_and_kill_with(dir, delay, "edit_until_killed", || {})
}

/// Starts `helper` on `dir`, runs `meanwhile` once it is ready, then kills it
/// `delay` later.
fn run_and_kill_with(dir: &Path, delay: Duration, helper: &str, meanwhile: impl FnOnce()) -> Heard {
	let mut child = Command::new(std::env::current_exe().unwrap())
		.args([
			"--exact",
			helper,
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
	meanwhile();
	std::thread::sleep(delay);
	child.kill().unwrap();
	child.wait().unwrap();
	reader.join().unwrap().unwrap()
}

/// A small xorshift, so each run kills at the same moments.
struct Moments(u64);

impl Moments {
	fn next_delay(&mut self) -> Duration {
		self.next_delay_up_to(Duration::from_millis(40))
	}

	fn next_delay_up_to(&mut self, longest: Duration) -> Duration {
		self.0 ^= self.0 << 13;
		self.0 ^= self.0 >> 7;
		self.0 ^= self.0 << 17;
		let longest = u64::try_from(longest.as_micros())
			.unwrap_or(u64::MAX)
			.max(1);
		Duration::from_micros(self.0 % longest)
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

#[test]
#[ignore = "helper process, spawned and killed by the create crash harness"]
fn create_until_killed() {
	let Some(budgets) = std::env::var_os(CRASH_DIR) else {
		return;
	};
	let budgets = PathBuf::from(budgets);
	let budget = NewBudget {
		name: "Household".to_owned(),
		format: NumberFormat {
			currency: Some("USD".to_owned()),
			style: NumberStyle::CommaDot,
			hide_decimals: false,
		},
		retirement_401k: true,
		hsa: false,
		accounts: vec![NewAccount {
			name: "Checking".to_owned(),
			kind: NewAccountKind::Checking,
			starting_balance: Money::from_minor(120_000),
		}],
	};
	say("ready 0");
	let mut created: i64 = 0;
	loop {
		let dir = budgets.join(format!("budget-{created}"));
		CrashSession::create_budget(&dir, &budget, SessionOptions::default()).unwrap();
		created += 1;
		say(&format!("committed {created}"));
	}
}

#[test]
fn a_killed_create_never_leaves_a_half_made_budget() {
	// PLAN §6.6, R-IO-0: create builds aside and renames into place, so every
	// budget folder a crash leaves behind is whole, and only hidden build
	// folders can be partial. The child builds real budgets from the pinned
	// template through veritas-store.
	let kills: usize = std::env::var("VERITAS_CRASH_KILLS")
		.ok()
		.and_then(|kills| kills.parse().ok())
		.unwrap_or(25);
	let root = TempDir::new("crash-create");
	let budgets = root.path().join("budgets");
	std::fs::create_dir(&budgets).unwrap();
	let mut moments = Moments(0xD1B5_4A32_D192_ED03);
	let (mut created, mut caught_mid_build) = (0, 0);

	for kill in 0..kills {
		let batch = budgets.join(format!("run-{kill}"));
		std::fs::create_dir(&batch).unwrap();
		// A create flushes every file to the drive before its rename, which takes
		// longer than an edit, so kills spread wider to land on either side.
		let delay = moments.next_delay_up_to(Duration::from_millis(400));
		let heard = run_and_kill_with(&batch, delay, "create_until_killed", || {});
		let visible: Vec<PathBuf> = std::fs::read_dir(&batch)
			.unwrap()
			.map(|entry| entry.unwrap().path())
			.filter(|path| !path.file_name().unwrap().to_string_lossy().starts_with('.'))
			.collect();
		let count = visible.len() as i64;
		created += count;
		let staging = std::fs::read_dir(&batch).unwrap().count() as i64 - count;
		caught_mid_build += i64::from(staging > 0);
		assert!(
			count == heard.committed || count == heard.committed + 1,
			"kill {kill}: {count} budgets, {} reported created",
			heard.committed
		);
		for dir in visible {
			let mut session =
				CrashSession::open(&dir, SessionOptions::default()).unwrap_or_else(|error| {
					panic!("kill {kill}: {} won't open: {error}", dir.display())
				});
			assert_eq!(session.read_only(), None, "kill {kill}");
			session.wait_for_open_check();
			assert!(!session.audit_report().unwrap().is_corrupt(), "kill {kill}");
		}
		std::fs::remove_dir_all(&batch).unwrap();
	}
	assert!(created > 0, "no create finished before a kill");
	eprintln!(
		"create crash harness: {kills} kills, {created} budgets created, {caught_mid_build} kills mid-build"
	);
}

#[test]
fn a_second_window_never_writes_through_a_writer_crash() {
	// R-REL-6, R-REL-1: another window opens while the writer is busy,
	// refuses edits, and stays read-only after the writer is killed. The next
	// window to open takes over writing even though the read-only one still
	// has the file open.
	let rounds: usize = std::env::var("VERITAS_CRASH_KILLS")
		.ok()
		.and_then(|kills| kills.parse().ok())
		.unwrap_or(25);
	let root = TempDir::new("crash-windows");
	let dir = root.path().join("budget");
	veritas_interop::import_actual_zip(&fixture("small"), &dir).unwrap();
	let mut moments = Moments(0xA076_1D64_78BD_642F);

	for round in 0..rounds {
		let mut window: Option<CrashSession> = None;
		let heard = run_and_kill_with(&dir, moments.next_delay(), "edit_until_killed", || {
			let mut opened = CrashSession::open(&dir, SessionOptions::default()).unwrap();
			assert_eq!(
				opened.read_only(),
				Some(ReadOnlyReason::OpenElsewhere),
				"round {round}"
			);
			assert!(
				matches!(
					opened.perform(&set_to(-1)),
					Err(CoreError::ReadOnly(ReadOnlyReason::OpenElsewhere))
				),
				"round {round}"
			);
			window = Some(opened);
		});
		let mut window = window.unwrap();

		let mut next = CrashSession::open(&dir, SessionOptions::default()).unwrap();
		assert_eq!(
			next.read_only(),
			None,
			"round {round}: the dead writer's lock held"
		);
		let found = value(next.data().unwrap())
			.unwrap_or_else(|| panic!("round {round}: a transaction landed in half"));
		assert!(
			found == heard.committed || found == heard.in_flight,
			"round {round}: found {found}, but {} was committed and {} in flight",
			heard.committed,
			heard.in_flight
		);
		next.perform(&set_to(found + 1)).unwrap();
		assert_eq!(
			window.read_only(),
			Some(ReadOnlyReason::OpenElsewhere),
			"round {round}: the read-only window started writing"
		);
		assert!(
			matches!(
				window.perform(&set_to(-1)),
				Err(CoreError::ReadOnly(ReadOnlyReason::OpenElsewhere))
			),
			"round {round}"
		);
		next.wait_for_open_check();
		assert!(!next.audit_report().unwrap().is_corrupt(), "round {round}");
		drop(next);
		drop(window);

		let reopened = CrashSession::open(&dir, SessionOptions::default()).unwrap();
		assert_eq!(
			value(reopened.data().unwrap()),
			Some(found + 1),
			"round {round}: the takeover's edit was lost"
		);
	}
}

#[test]
fn a_window_opened_while_another_writes_loads_only_whole_actions() {
	// R-REL-1, R-REL-6: every action is one transaction, so a second window
	// must see each one entirely or not at all. It gets no change events from
	// the other process, so a torn load stays on screen.
	let root = TempDir::new("crash-torn");
	let dir = root.path().join("budget");
	veritas_interop::import_actual_zip(&fixture("small"), &dir).unwrap();
	let mut torn = 0;
	run_and_kill_with(&dir, Duration::ZERO, "edit_until_killed", || {
		for _ in 0..200 {
			let window = CrashSession::open(&dir, SessionOptions::default()).unwrap();
			assert_eq!(window.read_only(), Some(ReadOnlyReason::OpenElsewhere));
			torn += usize::from(value(window.data().unwrap()).is_none());
		}
	});
	assert_eq!(torn, 0, "{torn} of 200 windows loaded half an action");
}
