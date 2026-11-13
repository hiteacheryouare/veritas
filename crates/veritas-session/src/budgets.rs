use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use veritas_domain::CoreError;

use crate::fsio::io_error;
use crate::lock::LOCK_FILE;
use crate::session::{DB_FILE, METADATA_FILE, budget_name};

/// One budget folder in the budgets directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BudgetEntry {
	/// Actual's budget id from metadata.json, or the folder name without one.
	pub id: String,
	pub name: Option<String>,
	pub folder: PathBuf,
	/// When a window last opened it for writing.
	pub last_opened: Option<DateTime<Utc>>,
}

/// Every budget folder directly inside `budgets_dir`: a folder holding
/// `db.sqlite` and `metadata.json`. Most recently opened first. Anything
/// else in the directory is skipped, and a missing directory has no budgets.
pub fn list_budgets(budgets_dir: &Path) -> Result<Vec<BudgetEntry>, CoreError> {
	let entries = match fs::read_dir(budgets_dir) {
		Ok(entries) => entries,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
		Err(error) => return Err(io_error("read", budgets_dir)(error)),
	};
	let mut budgets = Vec::new();
	for entry in entries {
		let folder = entry.map_err(io_error("read", budgets_dir))?.path();
		let hidden = folder
			.file_name()
			.is_some_and(|name| name.to_string_lossy().starts_with('.'));
		if hidden || !folder.join(DB_FILE).is_file() || !folder.join(METADATA_FILE).is_file() {
			continue;
		}
		let Ok(name) = budget_name(&folder) else {
			continue;
		};
		budgets.push(BudgetEntry {
			id: budget_id(&folder),
			name,
			last_opened: fs::metadata(folder.join(LOCK_FILE))
				.and_then(|metadata| metadata.modified())
				.ok()
				.map(DateTime::<Utc>::from),
			folder,
		});
	}
	budgets.sort_by(|a, b| {
		b.last_opened
			.cmp(&a.last_opened)
			.then_with(|| a.name.cmp(&b.name))
			.then_with(|| a.folder.cmp(&b.folder))
	});
	Ok(budgets)
}

fn budget_id(folder: &Path) -> String {
	fs::read_to_string(folder.join(METADATA_FILE))
		.ok()
		.and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
		.and_then(|metadata| metadata.get("id")?.as_str().map(str::to_owned))
		.unwrap_or_else(|| {
			folder
				.file_name()
				.map(|name| name.to_string_lossy().into_owned())
				.unwrap_or_default()
		})
}
