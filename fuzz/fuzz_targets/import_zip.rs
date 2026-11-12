#![no_main]

use std::path::PathBuf;
use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;

fn scratch() -> &'static PathBuf {
	static DIR: OnceLock<PathBuf> = OnceLock::new();
	DIR.get_or_init(|| {
		let dir = std::env::temp_dir().join(format!("veritas-fuzz-{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).expect("a scratch folder for the fuzzer");
		dir
	})
}

// Hostile bytes as an Actual export: the importer and the canonical dump must
// return an error, never panic, hang or leave a half-made budget behind.
fuzz_target!(|bytes: &[u8]| {
	let dir = scratch();
	let zip = dir.join("input.zip");
	let budget = dir.join("budget");
	std::fs::write(&zip, bytes).expect("the fuzzer can write its input");
	let imported = veritas_interop::import_actual_zip(&zip, &budget);
	if !matches!(imported, Ok(veritas_interop::ImportOutcome::Imported(_))) {
		assert!(!budget.exists(), "a failed import left its folder behind");
	}
	let _ = std::fs::remove_dir_all(&budget);
	let _ = veritas_interop::canonical_dump_to_vec(&zip);
});
