//! What the oracle differential tests share: finding the oracle, running it
//! and a seeded generator.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

pub fn repo_root() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `None` when `cargo xtask oracle setup` hasn't been run, so the caller skips.
pub fn oracle_dir() -> Option<PathBuf> {
	let dir = repo_root().join("tools/actual-oracle");
	dir.join("node_modules").exists().then_some(dir)
}

pub fn scratch_dir(name: &str) -> PathBuf {
	let dir = std::env::temp_dir().join(format!("veritas-rules-{name}-{}", std::process::id()));
	std::fs::create_dir_all(&dir).unwrap();
	dir
}

fn node(oracle: &Path, args: &[&str]) -> String {
	let mut command = Command::new("node");
	command.current_dir(oracle).arg("src/main.ts").args(args);
	let output = command
		.output()
		.unwrap_or_else(|error| panic!("running {command:?}: {error}"));
	assert!(
		output.status.success(),
		"{command:?} failed:\n{}",
		String::from_utf8_lossy(&output.stderr)
	);
	String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Applies `ops` to `base` with Actual and returns the canonical dump of the
/// result, both at the pinned `today`.
pub fn apply_and_dump(
	oracle: &Path,
	scratch: &Path,
	label: &str,
	base: &Path,
	ops: Vec<Value>,
	today: &str,
) -> Value {
	let ops_path = scratch.join(format!("{label}.json"));
	let zip_path = scratch.join(format!("{label}.zip"));
	std::fs::write(
		&ops_path,
		serde_json::json!({"version": 1, "ops": ops}).to_string(),
	)
	.unwrap();
	let (base, ops_path, zip_path) = (
		base.to_string_lossy().into_owned(),
		ops_path.to_string_lossy().into_owned(),
		zip_path.to_string_lossy().into_owned(),
	);
	node(
		oracle,
		&[
			"apply", &base, &ops_path, "--out", &zip_path, "--today", today,
		],
	);
	dump(oracle, &zip_path, today)
}

pub fn dump(oracle: &Path, zip: &str, today: &str) -> Value {
	serde_json::from_str(&node(oracle, &["dump", zip, "--today", today])).unwrap()
}

/// xorshift64*: deterministic, so a failure names a case that reproduces.
pub struct Rng(pub u64);

impl Rng {
	pub fn next(&mut self) -> u64 {
		self.0 ^= self.0 >> 12;
		self.0 ^= self.0 << 25;
		self.0 ^= self.0 >> 27;
		self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
	}

	pub fn below(&mut self, n: u64) -> u64 {
		self.next() % n
	}

	pub fn chance(&mut self, percent: u64) -> bool {
		self.below(100) < percent
	}

	pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
		&items[self.below(items.len() as u64) as usize]
	}
}

pub fn uuid(kind: u32, n: usize) -> String {
	format!("{kind:08x}-0000-4000-8000-{n:012x}")
}
