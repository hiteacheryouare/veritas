#![allow(dead_code)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

pub fn repo_root() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn fixture(name: &str) -> PathBuf {
	repo_root()
		.join("fixtures/synthetic")
		.join(format!("{name}.zip"))
}

pub struct Scratch(PathBuf);

static NEXT: AtomicU64 = AtomicU64::new(0);

impl Scratch {
	pub fn new(label: &str) -> Scratch {
		let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
			"{label}-{}-{}",
			std::process::id(),
			NEXT.fetch_add(1, Ordering::Relaxed)
		));
		let _ = std::fs::remove_dir_all(&path);
		std::fs::create_dir_all(&path).unwrap();
		Scratch(path)
	}

	pub fn path(&self) -> &Path {
		&self.0
	}

	pub fn join(&self, name: &str) -> PathBuf {
		self.0.join(name)
	}
}

impl Drop for Scratch {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.0);
	}
}

pub fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
	let file = std::fs::File::create(path).unwrap();
	let mut zip = zip::ZipWriter::new(file);
	let options = zip::write::SimpleFileOptions::default()
		.compression_method(zip::CompressionMethod::Deflated);
	for (name, bytes) in entries {
		zip.start_file(*name, options).unwrap();
		zip.write_all(bytes).unwrap();
	}
	zip.finish().unwrap();
}

/// The two files of an Actual zip, unpacked by hand so tests can edit them.
pub fn unpack(zip_path: &Path, into: &Path) -> (PathBuf, Vec<u8>) {
	let mut archive = zip::ZipArchive::new(std::fs::File::open(zip_path).unwrap()).unwrap();
	let mut db = Vec::new();
	std::io::copy(&mut archive.by_name("db.sqlite").unwrap(), &mut db).unwrap();
	let mut metadata = Vec::new();
	std::io::copy(
		&mut archive.by_name("metadata.json").unwrap(),
		&mut metadata,
	)
	.unwrap();
	let db_path = into.join("db.sqlite");
	std::fs::write(&db_path, db).unwrap();
	(db_path, metadata)
}

pub fn pack(db_path: &Path, metadata: &[u8], zip_path: &Path) {
	let db = std::fs::read(db_path).unwrap();
	write_zip(zip_path, &[("db.sqlite", &db), ("metadata.json", metadata)]);
}

pub fn oracle_dir() -> PathBuf {
	repo_root().join("tools/actual-oracle")
}

/// `tools/actual-oracle dump --raw-only`, or `None` (with a note on stderr)
/// when the oracle isn't installed.
pub fn oracle_raw_dump(zip_path: &Path) -> Option<Vec<u8>> {
	oracle(&["dump".as_ref(), zip_path.as_os_str(), "--raw-only".as_ref()])
}

/// Runs `tools/actual-oracle` with these arguments and returns its stdout, or
/// `None` (with a note on stderr) when the oracle isn't installed.
pub fn oracle(args: &[&std::ffi::OsStr]) -> Option<Vec<u8>> {
	if !oracle_dir().join("node_modules").exists() {
		eprintln!(
			"skipping the oracle comparison: tools/actual-oracle/node_modules is missing, run `cargo xtask oracle setup`"
		);
		return None;
	}
	let output = Command::new("node")
		.current_dir(oracle_dir())
		.arg("src/main.ts")
		.args(args)
		.output()
		.expect("node runs");
	assert!(
		output.status.success(),
		"the oracle failed: {}",
		String::from_utf8_lossy(&output.stderr)
	);
	Some(output.stdout)
}

fn oracle_installed() -> bool {
	if oracle_dir().join("node_modules").exists() {
		return true;
	}
	eprintln!(
		"skipping the Actual side: tools/actual-oracle/node_modules is missing, run `cargo xtask oracle setup`"
	);
	false
}

/// What `@actual-app/api` itself does with a zip: the oracle's full dump runs
/// `importBudget` and loads the budget. `Err` carries Actual's stderr.
pub fn actual_opens(zip_path: &Path) -> Option<Result<(), String>> {
	if !oracle_installed() {
		return None;
	}
	let output = Command::new("node")
		.current_dir(oracle_dir())
		.args(["src/main.ts", "dump"])
		.arg(zip_path)
		.output()
		.expect("node runs");
	Some(if output.status.success() {
		Ok(())
	} else {
		Err(String::from_utf8_lossy(&output.stderr).into_owned())
	})
}

/// Runs `oracle apply` with no ops: Actual imports the zip, then exports it.
pub fn actual_reexport(zip_path: &Path, scratch: &Scratch) -> Option<PathBuf> {
	if !oracle_installed() {
		return None;
	}
	let ops = scratch.join("no-ops.json");
	std::fs::write(&ops, br#"{"version": 1, "ops": []}"#).unwrap();
	let out = scratch.join("reexported.zip");
	let output = Command::new("node")
		.current_dir(oracle_dir())
		.args(["src/main.ts", "apply"])
		.arg(zip_path)
		.arg(&ops)
		.arg("--out")
		.arg(&out)
		.output()
		.expect("node runs");
	assert!(
		output.status.success(),
		"the oracle failed: {}",
		String::from_utf8_lossy(&output.stderr)
	);
	Some(out)
}

/// Fails with the first differing line instead of two 75 MB strings.
pub fn assert_same_dump(veritas: &[u8], oracle: &[u8], label: &str) {
	if veritas == oracle {
		return;
	}
	let ours: Vec<&[u8]> = veritas.split(|&b| b == b'\n').collect();
	let theirs: Vec<&[u8]> = oracle.split(|&b| b == b'\n').collect();
	let line = ours
		.iter()
		.zip(&theirs)
		.position(|(a, b)| a != b)
		.unwrap_or(ours.len().min(theirs.len()));
	let show = |lines: &[&[u8]]| {
		lines
			.get(line)
			.map(|l| String::from_utf8_lossy(l).into_owned())
			.unwrap_or_else(|| "<end of dump>".to_owned())
	};
	panic!(
		"{label}: dumps differ at line {} ({} vs {} lines)\n  veritas: {}\n  oracle:  {}",
		line + 1,
		ours.len(),
		theirs.len(),
		show(&ours),
		show(&theirs)
	);
}

pub fn imported(
	result: Result<veritas_interop::ImportOutcome, veritas_domain::CoreError>,
) -> veritas_interop::ImportedBudget {
	match result {
		Ok(veritas_interop::ImportOutcome::Imported(budget)) => budget,
		other => panic!("expected an import, got {other:?}"),
	}
}

/// Imports a fixture-shaped zip, adopts it and exports it again, the way the
/// app would with no edits in between.
pub fn round_trip(zip_path: &Path, scratch: &Scratch) -> PathBuf {
	let budget = imported(veritas_interop::import_actual_zip(
		zip_path,
		&scratch.join("budget"),
	));
	let mut store =
		veritas_store::Store::open(&budget.db_path, veritas_store::Access::ReadWrite).unwrap();
	store.adopt().unwrap();
	let out = scratch.join("exported.zip");
	match veritas_interop::export_actual_zip(&store, &budget.metadata_path, &out).unwrap() {
		veritas_interop::ExportOutcome::Exported(audit) => assert!(audit.is_clean(), "{audit:?}"),
		refused => panic!("export refused: {refused:?}"),
	}
	out
}
