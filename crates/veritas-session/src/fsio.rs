use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use veritas_domain::CoreError;

pub(crate) fn io_error(action: &str, path: &Path) -> impl FnOnce(io::Error) -> CoreError {
	let context = format!("couldn't {action} {}", path.display());
	move |error| CoreError::Io(format!("{context}: {error}"))
}

pub(crate) fn storage_error(error: rusqlite::Error) -> CoreError {
	CoreError::Storage(error.to_string())
}

pub(crate) fn sync_file(path: &Path) -> Result<(), CoreError> {
	File::open(path)
		.and_then(|file| file.sync_all())
		.map_err(io_error("flush", path))
}

/// Flushes every file directly in `dir`, then `dir` itself, so a folder that
/// is about to be renamed into place holds whole files after a power cut.
pub(crate) fn sync_folder(dir: &Path) -> Result<(), CoreError> {
	for entry in fs::read_dir(dir).map_err(io_error("read", dir))? {
		let path = entry.map_err(io_error("read", dir))?.path();
		if path.is_file() {
			sync_file(&path)?;
		}
	}
	sync_dir(dir);
	Ok(())
}

/// Makes a rename durable. Some filesystems refuse to fsync a directory, and
/// by then the rename has already happened, so a refusal isn't a failure.
pub(crate) fn sync_dir(dir: &Path) {
	if let Ok(handle) = File::open(dir) {
		let _ = handle.sync_all();
	}
}

pub(crate) fn utf8(path: &Path) -> Result<&str, CoreError> {
	path.to_str()
		.ok_or_else(|| CoreError::Io(format!("{} isn't a UTF-8 path", path.display())))
}

const PARTIAL: &str = ".partial";

/// A half-written file this old belongs to a writer that crashed; anything
/// newer may still be in progress.
const STALE_AFTER: Duration = Duration::from_secs(60 * 60);

/// Creates a fresh, empty temp file for `name` in `dir`. The name is unique
/// to this process and call, and `create_new` makes sure no other writer
/// already has it.
pub(crate) fn reserve_partial(dir: &Path, name: &str) -> Result<(PathBuf, File), CoreError> {
	static NEXT: AtomicU64 = AtomicU64::new(0);
	loop {
		let path = dir.join(format!(
			".{name}.{}-{}{PARTIAL}",
			std::process::id(),
			NEXT.fetch_add(1, Ordering::Relaxed)
		));
		match OpenOptions::new().write(true).create_new(true).open(&path) {
			Ok(file) => return Ok((path, file)),
			Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
			Err(error) => return Err(io_error("write in", dir)(error)),
		}
	}
}

/// Deletes temp files for `name` that a crashed writer left in `dir`. Ones
/// another writer may still be filling are left alone.
pub(crate) fn sweep_stale_partials(dir: &Path, name: Option<&str>) {
	let Ok(entries) = fs::read_dir(dir) else {
		return;
	};
	let prefix = name.map(|name| format!(".{name}."));
	for entry in entries.flatten() {
		let file_name = entry.file_name();
		let file_name = file_name.to_string_lossy();
		let ours = file_name.starts_with('.')
			&& file_name.ends_with(PARTIAL)
			&& prefix
				.as_deref()
				.is_none_or(|prefix| file_name.starts_with(prefix));
		let stale = entry
			.metadata()
			.and_then(|metadata| metadata.modified())
			.is_ok_and(|modified| {
				SystemTime::now()
					.duration_since(modified)
					.is_ok_and(|age| age >= STALE_AFTER)
			});
		if ours && stale {
			let _ = fs::remove_file(entry.path());
		}
	}
}

/// Deletes folders starting with `prefix` in `dir` that a crashed create
/// left behind. A folder still being filled is younger than an hour and kept.
pub(crate) fn sweep_stale_folders(dir: &Path, prefix: &str) {
	let Ok(entries) = fs::read_dir(dir) else {
		return;
	};
	for entry in entries.flatten() {
		let ours = entry.file_name().to_string_lossy().starts_with(prefix);
		let stale = entry
			.metadata()
			.and_then(|metadata| metadata.modified())
			.is_ok_and(|modified| {
				SystemTime::now()
					.duration_since(modified)
					.is_ok_and(|age| age >= STALE_AFTER)
			});
		if ours && stale {
			let _ = fs::remove_dir_all(entry.path());
		}
	}
}
