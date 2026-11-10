use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::path::Path;

use veritas_domain::CoreError;
use zip::ZipArchive;

/// Actual's `safeUnzip` cap, applied to the archive, each entry and the total.
const MAX_ZIP_SIZE: u64 = 500 * 1024 * 1024;

fn not_a_budget(message: impl Into<String>) -> CoreError {
	CoreError::NotAnActualBudget(message.into())
}

fn io_error(path: &Path, error: &io::Error) -> CoreError {
	CoreError::Io(format!("{}: {error}", path.display()))
}

/// The two files of an Actual export, located the way Actual's `importBuffer`
/// locates them.
pub(crate) struct BudgetArchive {
	archive: ZipArchive<BufReader<File>>,
	db_index: usize,
	metadata_index: usize,
}

fn is_unsafe_name(name: &str) -> bool {
	let bytes = name.as_bytes();
	let drive_letter = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
	name.contains('\0')
		|| name.contains('\\')
		|| drive_letter
		|| name.starts_with('/')
		|| name.split('/').any(|segment| segment == "..")
}

/// The folder holding both files: the root if it qualifies, otherwise the one
/// folder that does. `None` when there is no such folder or more than one.
pub(crate) fn shared_folder<'a>(
	names: impl IntoIterator<Item = &'a str> + Clone,
) -> Option<String> {
	let folders_of = |file: &str| -> Vec<String> {
		let nested = format!("/{file}");
		names
			.clone()
			.into_iter()
			.filter(|name| *name == file || name.ends_with(&nested))
			.map(|name| name[..name.len() - file.len()].to_owned())
			.collect()
	};
	let metadata_folders = folders_of("metadata.json");
	let shared: Vec<String> = folders_of("db.sqlite")
		.into_iter()
		.filter(|folder| metadata_folders.contains(folder))
		.collect();
	if shared.iter().any(String::is_empty) {
		Some(String::new())
	} else if shared.len() == 1 {
		shared.into_iter().next()
	} else {
		None
	}
}

impl BudgetArchive {
	pub(crate) fn open(zip_path: &Path) -> Result<BudgetArchive, CoreError> {
		let file = File::open(zip_path).map_err(|error| io_error(zip_path, &error))?;
		let length = file
			.metadata()
			.map_err(|error| io_error(zip_path, &error))?
			.len();
		if length > MAX_ZIP_SIZE {
			return Err(not_a_budget(
				"the zip is larger than Actual's 500 MB import limit",
			));
		}
		let mut archive = ZipArchive::new(BufReader::new(file))
			.map_err(|_| not_a_budget("the file isn't a zip archive"))?;
		if archive.has_duplicate_names() {
			return Err(not_a_budget("the zip lists the same file twice"));
		}

		let mut names = Vec::with_capacity(archive.len());
		let mut lowercase = std::collections::HashSet::new();
		let mut total: u64 = 0;
		for index in 0..archive.len() {
			let entry = archive
				.by_index_raw(index)
				.map_err(|_| not_a_budget("the zip has an entry that can't be read"))?;
			let name = entry
				.name()
				.map_err(|_| not_a_budget("the zip has an entry with an unreadable name"))?
				.into_owned();
			if is_unsafe_name(&name) {
				return Err(not_a_budget(
					"the zip has an entry with an unsafe path, like an absolute path or `..`",
				));
			}
			if entry.size() > MAX_ZIP_SIZE {
				return Err(not_a_budget(
					"a file in the zip is larger than Actual's 500 MB import limit",
				));
			}
			total = total.saturating_add(entry.size());
			if total > MAX_ZIP_SIZE {
				return Err(not_a_budget(
					"the zip's contents are larger than Actual's 500 MB import limit",
				));
			}
			if !lowercase.insert(name.to_lowercase()) {
				return Err(not_a_budget("the zip lists the same file twice"));
			}
			names.push(name);
		}

		let folder = shared_folder(names.iter().map(String::as_str)).ok_or_else(|| {
			not_a_budget(
				"the zip needs db.sqlite and metadata.json together, at its root or in exactly one folder",
			)
		})?;
		let index_of = |file: &str| {
			let wanted = format!("{folder}{file}");
			names.iter().position(|name| *name == wanted)
		};
		match (index_of("db.sqlite"), index_of("metadata.json")) {
			(Some(db_index), Some(metadata_index)) => Ok(BudgetArchive {
				archive,
				db_index,
				metadata_index,
			}),
			_ => Err(not_a_budget("the zip's budget files couldn't be found")),
		}
	}

	pub(crate) fn metadata_bytes(&mut self) -> Result<Vec<u8>, CoreError> {
		let mut bytes = Vec::new();
		self.copy_entry(self.metadata_index, &mut bytes)?;
		Ok(bytes)
	}

	pub(crate) fn extract_db(&mut self, to: &Path) -> Result<(), CoreError> {
		let mut file = File::create_new(to).map_err(|error| io_error(to, &error))?;
		self.copy_entry(self.db_index, &mut file)?;
		file.sync_all().map_err(|error| io_error(to, &error))
	}

	fn copy_entry(&mut self, index: usize, into: &mut impl Write) -> Result<(), CoreError> {
		let entry = self
			.archive
			.by_index(index)
			.map_err(|_| not_a_budget("a budget file in the zip can't be decompressed"))?;
		// The declared size can lie, so the cap also applies to what comes out.
		let mut limited = entry.take(MAX_ZIP_SIZE + 1);
		let copied = io::copy(&mut limited, into)
			.map_err(|_| not_a_budget("a budget file in the zip is damaged"))?;
		if copied > MAX_ZIP_SIZE {
			return Err(not_a_budget(
				"a file in the zip is larger than Actual's 500 MB import limit",
			));
		}
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_root_wins_over_a_folder_r_io_1() {
		let names = [
			"nested/db.sqlite",
			"nested/metadata.json",
			"db.sqlite",
			"metadata.json",
		];
		assert_eq!(shared_folder(names), Some(String::new()));
	}

	#[test]
	fn exactly_one_shared_folder_is_accepted_r_io_1() {
		let names = [
			"My Budget/db.sqlite",
			"My Budget/metadata.json",
			"readme.txt",
		];
		assert_eq!(shared_folder(names), Some("My Budget/".to_owned()));
		let split = ["a/db.sqlite", "b/metadata.json"];
		assert_eq!(shared_folder(split), None);
		let two = [
			"a/db.sqlite",
			"a/metadata.json",
			"b/db.sqlite",
			"b/metadata.json",
		];
		assert_eq!(shared_folder(two), None);
		let lonely = ["db.sqlite"];
		assert_eq!(shared_folder(lonely), None);
	}

	#[test]
	fn a_suffix_match_needs_a_slash() {
		let names = ["notdb.sqlite", "metadata.json"];
		assert_eq!(shared_folder(names), None);
	}

	#[test]
	fn unsafe_names_match_actuals_rules() {
		for name in [
			"/db.sqlite",
			"../db.sqlite",
			"a/../b",
			"C:db.sqlite",
			"a\\b",
			"a\0b",
		] {
			assert!(is_unsafe_name(name), "{name:?}");
		}
		for name in ["db.sqlite", "a/b/metadata.json", "..a/b", "a/.../b"] {
			assert!(!is_unsafe_name(name), "{name:?}");
		}
	}
}
