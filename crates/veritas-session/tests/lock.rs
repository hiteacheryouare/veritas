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
