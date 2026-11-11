#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::Connection;

pub struct Scratch(pub PathBuf);

static NEXT: AtomicU64 = AtomicU64::new(0);

impl Scratch {
	pub fn new(label: &str) -> Scratch {
		let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
			"store-{label}-{}-{}",
			std::process::id(),
			NEXT.fetch_add(1, Ordering::Relaxed)
		));
		let _ = std::fs::remove_dir_all(&path);
		std::fs::create_dir_all(&path).unwrap();
		Scratch(path)
	}
}

impl Drop for Scratch {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.0);
	}
}

pub fn fixture_db(name: &str, scratch: &Scratch) -> PathBuf {
	let zip_path = Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("../../fixtures/synthetic")
		.join(format!("{name}.zip"));
	let mut archive = zip::ZipArchive::new(std::fs::File::open(zip_path).unwrap()).unwrap();
	let db_path = scratch.0.join("db.sqlite");
	let mut out = std::fs::File::create(&db_path).unwrap();
	std::io::copy(&mut archive.by_name("db.sqlite").unwrap(), &mut out).unwrap();
	db_path
}

pub fn count(conn: &Connection, sql: &str) -> usize {
	conn.query_row(sql, [], |row| row.get::<_, i64>(0)).unwrap() as usize
}
