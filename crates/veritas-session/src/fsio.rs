use std::fs::File;
use std::io;
use std::path::Path;

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
