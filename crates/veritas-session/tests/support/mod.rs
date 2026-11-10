#![allow(
	dead_code,
	clippy::unwrap_used,
	reason = "each test crate uses a part of these helpers, and they fail by panicking"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A fresh directory under Cargo's per-target temp folder, removed on drop.
pub struct TempDir(PathBuf);

impl TempDir {
	pub fn new(label: &str) -> Self {
		static NEXT: AtomicUsize = AtomicUsize::new(0);
		let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
			"session-{label}-{}-{}",
			std::process::id(),
			NEXT.fetch_add(1, Ordering::Relaxed)
		));
		let _ = fs::remove_dir_all(&path);
		fs::create_dir_all(&path).unwrap();
		TempDir(path)
	}

	pub fn path(&self) -> &Path {
		&self.0
	}

	pub fn entries(&self) -> Vec<String> {
		entries(&self.0)
	}
}

impl Drop for TempDir {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

pub fn entries(dir: &Path) -> Vec<String> {
	let mut names: Vec<String> = fs::read_dir(dir)
		.unwrap()
		.map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
		.collect();
	names.sort();
	names
}
