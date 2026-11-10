#![allow(
	clippy::unwrap_used,
	clippy::expect_used,
	clippy::panic,
	reason = "integration tests fail by panicking"
)]

mod support;

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard, mpsc};
use std::time::Duration;

use support::TempDir;
use veritas_domain::{CoreError, ReadOnlyReason};
use veritas_session::{Access, acquire};

const HOLDER_DIR: &str = "VERITAS_LOCK_HOLDER_DIR";

/// A spawned child shares every open file, and so every lock, with this
/// process until it execs. Tests that spawn or that expect a lock to come
/// free the moment it drops take turns.
fn one_at_a_time() -> MutexGuard<'static, ()> {
	static TURN: Mutex<()> = Mutex::new(());
	TURN.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
#[ignore = "helper process, spawned by the two-process lock tests"]
fn hold_the_lock_until_killed() {
	let Some(dir) = std::env::var_os(HOLDER_DIR) else {
		return;
	};
	let access = acquire(Path::new(&dir)).unwrap();
	let state = match access {
		Access::Writer(_) => "writer",
		Access::ReadOnly(_) => "read-only",
	};
	println!("\nlock-state {state}");
	std::io::stdout().flush().unwrap();
	let _ = std::io::stdin().read_to_end(&mut Vec::new());
	drop(access);
}

/// Runs `hold_the_lock_until_killed` in a second process and waits until it
/// reports whether it got the lock.
fn second_process(dir: &Path) -> (Child, String) {
	let (child, reported) = spawn_holder(dir);
	wait_for_report(child, &reported)
}

fn spawn_holder(dir: &Path) -> (Child, mpsc::Receiver<String>) {
	let mut child = Command::new(std::env::current_exe().unwrap())
		.args([
			"--exact",
			"hold_the_lock_until_killed",
			"--ignored",
			"--nocapture",
			"--test-threads=1",
		])
		.env(HOLDER_DIR, dir)
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.stderr(Stdio::null())
		.spawn()
		.unwrap();
	let stdout = child.stdout.take().unwrap();
	let (report, reported) = mpsc::channel();
	std::thread::spawn(move || {
		for line in BufReader::new(stdout).lines().map_while(Result::ok) {
			if let Some(state) = line.strip_prefix("lock-state ") {
				let _ = report.send(state.to_owned());
			}
		}
	});
	(child, reported)
}

fn wait_for_report(mut child: Child, reported: &mpsc::Receiver<String>) -> (Child, String) {
	match reported.recv_timeout(Duration::from_secs(30)) {
		Ok(state) => (child, state),
		Err(error) => {
			let _ = child.kill();
			panic!("the helper process never reported its lock: {error}");
		}
	}
}

#[test]
fn a_second_process_gets_read_only_and_a_killed_writer_frees_the_lock() {
	// R-REL-6
	let _turn = one_at_a_time();
	let dir = TempDir::new("lock-crash");
	let (mut holder, state) = second_process(dir.path());
	assert_eq!(state, "writer");

	let second = acquire(dir.path()).unwrap();
	assert_eq!(
		second.read_only_reason(),
		Some(ReadOnlyReason::OpenElsewhere)
	);

	holder.kill().unwrap();
	holder.wait().unwrap();
	assert!(matches!(acquire(dir.path()).unwrap(), Access::Writer(_)));
}

#[test]
fn a_process_opening_a_budget_this_one_writes_gets_read_only() {
	// R-REL-6
	let _turn = one_at_a_time();
	let dir = TempDir::new("lock-other");
	let mine = acquire(dir.path()).unwrap();
	assert!(matches!(mine, Access::Writer(_)));

	let (mut other, state) = second_process(dir.path());
	assert_eq!(state, "read-only");
	drop(other.stdin.take());
	assert!(other.wait().unwrap().success());
}

#[test]
fn a_second_window_in_the_same_process_gets_read_only_until_the_first_closes() {
	// R-REL-6
	let _turn = one_at_a_time();
	let dir = TempDir::new("lock-window");
	let first = acquire(dir.path()).unwrap();
	assert!(matches!(first, Access::Writer(_)));
	assert_eq!(
		acquire(dir.path()).unwrap().read_only_reason(),
		Some(ReadOnlyReason::OpenElsewhere)
	);
	drop(first);
	assert!(matches!(acquire(dir.path()).unwrap(), Access::Writer(_)));
}

#[test]
fn a_missing_budget_directory_is_a_file_error() {
	let dir = TempDir::new("lock-missing");
	let result = acquire(&dir.path().join("gone"));
	assert!(matches!(result, Err(CoreError::Io(_))), "{result:?}");
}

#[test]
fn budgets_opened_at_the_same_moment_by_several_processes_get_exactly_one_writer() {
	// R-REL-6: a double-clicked budget and a restored window race to open it.
	let _turn = one_at_a_time();
	let dir = TempDir::new("lock-race");
	let spawned: Vec<_> = (0..6).map(|_| spawn_holder(dir.path())).collect();
	let holders: Vec<(Child, String)> = spawned
		.into_iter()
		.map(|(child, reported)| wait_for_report(child, &reported))
		.collect();

	let writers = holders
		.iter()
		.filter(|(_, state)| state == "writer")
		.count();
	assert_eq!(
		writers,
		1,
		"{:?}",
		holders.iter().map(|(_, s)| s).collect::<Vec<_>>()
	);
	assert_eq!(
		acquire(dir.path()).unwrap().read_only_reason(),
		Some(ReadOnlyReason::OpenElsewhere)
	);

	for (mut holder, _) in holders {
		drop(holder.stdin.take());
		assert!(holder.wait().unwrap().success());
	}
	assert!(matches!(acquire(dir.path()).unwrap(), Access::Writer(_)));
}

#[test]
fn each_budget_has_its_own_lock() {
	// R-REL-6: the lock is per budget, so a second budget opens for writing
	// while the first is open.
	let _turn = one_at_a_time();
	let household = TempDir::new("lock-household");
	let business = TempDir::new("lock-business");
	let first = acquire(household.path()).unwrap();
	let second = acquire(business.path()).unwrap();
	assert!(matches!(first, Access::Writer(_)));
	assert!(matches!(second, Access::Writer(_)));
}

#[test]
fn a_budget_reached_through_a_symlink_is_still_the_same_budget() {
	// R-REL-6: macOS hands out /var and /private/var for the same folder, and
	// a user may open a budget through an alias.
	let _turn = one_at_a_time();
	let parent = TempDir::new("lock-symlink");
	let budget = parent.path().join("My Budget");
	std::fs::create_dir(&budget).unwrap();
	let alias = parent.path().join("Budget alias");
	std::os::unix::fs::symlink(&budget, &alias).unwrap();

	let (mut holder, state) = second_process(&alias);
	assert_eq!(state, "writer");
	assert_eq!(
		acquire(&budget).unwrap().read_only_reason(),
		Some(ReadOnlyReason::OpenElsewhere)
	);
	holder.kill().unwrap();
	holder.wait().unwrap();
	assert!(matches!(acquire(&budget).unwrap(), Access::Writer(_)));
}
