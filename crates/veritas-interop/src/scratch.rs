use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use veritas_domain::CoreError;

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A private folder under the system temp dir, removed on drop.
pub(crate) struct ScratchDir {
	path: PathBuf,
}

impl ScratchDir {
	pub(crate) fn new(purpose: &str) -> Result<ScratchDir, CoreError> {
		let nanos = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.map(|elapsed| elapsed.as_nanos())
			.unwrap_or_default();
		let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
		let path = std::env::temp_dir().join(format!(
			"veritas-{purpose}-{}-{nanos}-{unique}",
			std::process::id()
		));
		std::fs::create_dir(&path)
			.map_err(|error| CoreError::Io(format!("{}: {error}", path.display())))?;
		Ok(ScratchDir { path })
	}

	pub(crate) fn path(&self) -> &Path {
		&self.path
	}
}

impl Drop for ScratchDir {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.path);
	}
}
