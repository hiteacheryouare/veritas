use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

use veritas_domain::{CoreError, ReadOnlyReason};

use crate::fsio::io_error;

pub const LOCK_FILE: &str = "veritas.lock";

/// Held for as long as this process may write the budget. The OS drops the
/// lock when the file closes, including when the process is killed.
#[derive(Debug)]
pub struct WriterLock {
	_file: File,
	dir: PathBuf,
}

impl WriterLock {
	/// The budget directory this lock is for, with symlinks resolved.
	pub fn dir(&self) -> &Path {
		&self.dir
	}

	/// Fails unless `file` lives in the directory this lock is for.
	pub(crate) fn covers(&self, file: &Path) -> Result<(), CoreError> {
		let parent = file.parent().unwrap_or(Path::new("."));
		let parent = parent.canonicalize().map_err(io_error("open", parent))?;
		if parent == self.dir {
			Ok(())
		} else {
			Err(CoreError::Internal(format!(
				"the writer lock is for {}, not {}",
				self.dir.display(),
				parent.display()
			)))
		}
	}
}

#[derive(Debug)]
pub enum Access {
	Writer(WriterLock),
	ReadOnly(ReadOnlyReason),
}

impl Access {
	pub fn read_only_reason(&self) -> Option<ReadOnlyReason> {
		match self {
			Access::Writer(_) => None,
			Access::ReadOnly(reason) => Some(*reason),
		}
	}
}

/// Takes the single-writer lock on a budget directory, or reports that
/// another window or process already has it.
pub fn acquire(budget_dir: &Path) -> Result<Access, CoreError> {
	let dir = budget_dir
		.canonicalize()
		.map_err(io_error("open", budget_dir))?;
	let path = dir.join(LOCK_FILE);
	let file = OpenOptions::new()
		.read(true)
		.write(true)
		.create(true)
		.truncate(false)
		.open(&path)
		.map_err(io_error("open", &path))?;
	match file.try_lock() {
		Ok(()) => Ok(Access::Writer(WriterLock { _file: file, dir })),
		Err(TryLockError::WouldBlock) => Ok(Access::ReadOnly(ReadOnlyReason::OpenElsewhere)),
		Err(TryLockError::Error(error)) => Err(io_error("lock", &path)(error)),
	}
}
