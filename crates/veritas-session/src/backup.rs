use std::fs::{self, File};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use veritas_domain::CoreError;

use crate::fsio::{io_error, reserve_partial, sweep_stale_partials, sync_dir};

/// Writes an exported budget into `folder` so that `file_name` is either the
/// previous complete file or the new complete file, never a partial one. A
/// failure leaves nothing behind and is reported, so editing can carry on.
pub fn write_backup(folder: &Path, file_name: &str, bytes: &[u8]) -> Result<PathBuf, CoreError> {
	if file_name.is_empty() || file_name.starts_with('.') || file_name.contains(['/', '\\']) {
		return Err(CoreError::Invalid(format!(
			"{file_name:?} can't be used as a backup file name"
		)));
	}
	match fs::metadata(folder) {
		Ok(metadata) if metadata.is_dir() => {}
		Ok(_) => {
			return Err(CoreError::Io(format!(
				"the backup location {} isn't a folder. Choose a folder for backups",
				folder.display()
			)));
		}
		Err(error) if error.kind() == ErrorKind::NotFound => {
			return Err(CoreError::Io(format!(
				"the backup folder {} isn't available. Check that it's connected, or choose another folder",
				folder.display()
			)));
		}
		Err(error) => return Err(io_error("open the backup folder", folder)(error)),
	}

	let target = folder.join(file_name);
	let (partial, file) = reserve_partial(folder, file_name)?;
	if let Err(error) = write_then_rename(file, &partial, &target, bytes) {
		let _ = fs::remove_file(&partial);
		return Err(error);
	}
	sync_dir(folder);
	sweep_stale_partials(folder, Some(file_name));
	Ok(target)
}

fn write_then_rename(
	mut file: File,
	partial: &Path,
	target: &Path,
	bytes: &[u8],
) -> Result<(), CoreError> {
	file.write_all(bytes)
		.and_then(|()| file.sync_all())
		.map_err(io_error("write", partial))?;
	drop(file);
	fs::rename(partial, target).map_err(io_error("save the backup as", target))
}
