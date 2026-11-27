#![allow(
	dead_code,
	reason = "each test crate includes this file and uses only part of it"
)]

//! Child processes and budget locks, for the tests in one binary that run on
//! parallel threads. Include it with
//! `#[path = "support/children.rs"] mod children;`.

use std::io;
use std::process::{Command, Output, Stdio};
use std::sync::{PoisonError, RwLock};

/// From fork to exec a child process holds a copy of every file this process
/// has open, and a budget's lock isn't released until every copy closes. So
/// children start under the read side, and opening a budget this process
/// closed takes the write side, which waits for every child mid-spawn to exec.
/// Every test that reopens a budget it needs to write must open it through
/// `while_no_child_starts`, and every child must start through `output`.
static STARTING_CHILDREN: RwLock<()> = RwLock::new(());

/// `command.output()`, started where `while_no_child_starts` waits for it.
pub fn output(command: &mut Command) -> io::Result<Output> {
	let child = {
		let _starting = STARTING_CHILDREN
			.read()
			.unwrap_or_else(PoisonError::into_inner);
		command
			.stdin(Stdio::null())
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			.spawn()?
	};
	child.wait_with_output()
}

/// Runs `work` while no child process is starting, so no other test's child
/// holds a copy of a lock that `work` takes. Never start a child inside it:
/// the lock isn't reentrant, and the test would hang.
pub fn while_no_child_starts<T>(work: impl FnOnce() -> T) -> T {
	let _quiet = STARTING_CHILDREN
		.write()
		.unwrap_or_else(PoisonError::into_inner);
	work()
}
