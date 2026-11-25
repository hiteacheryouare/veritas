use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{PoisonError, RwLock};

use veritas_session::CancelToken;

use crate::session::Core;

pub(crate) struct Scratch(PathBuf);

impl Scratch {
	pub(crate) fn new(label: &str) -> Self {
		static NEXT: AtomicUsize = AtomicUsize::new(0);
		let path = Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("../../target/tmp")
			.join(format!(
				"ffi-{label}-{}-{}",
				std::process::id(),
				NEXT.fetch_add(1, Ordering::Relaxed)
			));
		let _ = std::fs::remove_dir_all(&path);
		std::fs::create_dir_all(&path).unwrap();
		Scratch(path)
	}

	pub(crate) fn budget(&self) -> String {
		self.0.join("budget").display().to_string()
	}
}

impl Drop for Scratch {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.0);
	}
}

/// A fresh copy of `fixtures/synthetic/{fixture}.zip`, open.
pub(crate) fn imported(fixture: &str) -> (Scratch, Core<veritas_store::Store>) {
	let scratch = Scratch::new(fixture);
	let zip = Path::new(env!("CARGO_MANIFEST_DIR"))
		.join(format!("../../fixtures/synthetic/{fixture}.zip"))
		.display()
		.to_string();
	let core = Core::<veritas_store::Store>::import_actual_zip(
		&zip,
		&scratch.budget(),
		&CancelToken::default(),
	)
	.unwrap();
	(scratch, core)
}

/// From fork to exec a child process holds a copy of every file this process
/// has open, and a budget's lock isn't released until every copy closes. So
/// children start under the read side, and a test that closes a budget and
/// needs its lock back holds the write side across both.
static STARTING_CHILDREN: RwLock<()> = RwLock::new(());

/// `command.output()`, started where `while_no_child_starts` waits for it.
pub(crate) fn output(command: &mut Command) -> std::io::Result<Output> {
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
/// holds a lock that `work` releases and takes again.
pub(crate) fn while_no_child_starts<T>(work: impl FnOnce() -> T) -> T {
	let _quiet = STARTING_CHILDREN
		.write()
		.unwrap_or_else(PoisonError::into_inner);
	work()
}

/// Closes `core` and opens its budget again, as the only writer.
pub(crate) fn reopened(
	core: Core<veritas_store::Store>,
	scratch: &Scratch,
) -> Core<veritas_store::Store> {
	while_no_child_starts(|| {
		drop(core);
		Core::<veritas_store::Store>::open(&scratch.budget(), &CancelToken::default()).unwrap()
	})
}
