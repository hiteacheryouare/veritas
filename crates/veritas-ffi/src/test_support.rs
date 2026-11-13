use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

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

pub(crate) fn reopened(scratch: &Scratch) -> Core<veritas_store::Store> {
	Core::<veritas_store::Store>::open(&scratch.budget(), &CancelToken::default()).unwrap()
}
