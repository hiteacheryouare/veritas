use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;

use veritas_domain::{CoreError, ReadOnlyReason};

use crate::fsio::io_error;

pub const LOCK_FILE: &str = "veritas.lock";

/// Held for as long as this process may write the budget. The OS drops the
/// lock when the file closes, including when the process is killed.
#[derive(Debug)]
pub struct WriterLock {
	_file: File,
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
	let path = budget_dir.join(LOCK_FILE);
	let file = OpenOptions::new()
		.read(true)
		.write(true)
		.create(true)
		.truncate(false)
		.open(&path)
		.map_err(io_error("open", &path))?;
	match file.try_lock() {
		Ok(()) => Ok(Access::Writer(WriterLock { _file: file })),
		Err(TryLockError::WouldBlock) => Ok(Access::ReadOnly(ReadOnlyReason::OpenElsewhere)),
		Err(TryLockError::Error(error)) => Err(io_error("lock", &path)(error)),
	}
}
