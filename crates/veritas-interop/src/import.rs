use std::path::{Path, PathBuf};

use veritas_domain::CoreError;

use crate::archive::BudgetArchive;
use crate::canonical::json;
use crate::manifest::{Access, Manifest};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportedBudget {
	pub dir: PathBuf,
	pub db_path: PathBuf,
	pub metadata_path: PathBuf,
	pub access: Access,
}

fn io_error(path: &Path, error: &std::io::Error) -> CoreError {
	CoreError::Io(format!("{}: {error}", path.display()))
}

/// R-IO-1, R-IO-8: copies an Actual export into `budget_dir`, which must not
/// exist yet, then checks the copy. The zip is only ever read. On any failure
/// the new folder is removed, so nothing is left behind.
pub fn import_actual_zip(zip_path: &Path, budget_dir: &Path) -> Result<ImportedBudget, CoreError> {
	let mut archive = BudgetArchive::open(zip_path)?;
	let metadata = archive.metadata_bytes()?;
	json::parse(&String::from_utf8_lossy(&metadata))
		.map_err(|reason| CoreError::NotAnActualBudget(reason.to_owned()))?;

	std::fs::create_dir(budget_dir).map_err(|error| io_error(budget_dir, &error))?;
	let result = populate(&mut archive, &metadata, budget_dir);
	if result.is_err() {
		let _ = std::fs::remove_dir_all(budget_dir);
	}
	result
}

fn populate(
	archive: &mut BudgetArchive,
	metadata: &[u8],
	budget_dir: &Path,
) -> Result<ImportedBudget, CoreError> {
	let db_path = budget_dir.join("db.sqlite");
	let metadata_path = budget_dir.join("metadata.json");
	archive.extract_db(&db_path)?;
	std::fs::write(&metadata_path, metadata).map_err(|error| io_error(&metadata_path, &error))?;

	let conn = veritas_store::open_read_only(&db_path)?;
	let problems = veritas_store::integrity_check(&conn)?;
	if !problems.is_empty() {
		return Err(CoreError::AuditFailed {
			violations: problems.len(),
		});
	}
	let access = Manifest::supported()?.check(&veritas_store::migration_ids(&conn)?)?;
	drop(conn);

	Ok(ImportedBudget {
		dir: budget_dir.to_path_buf(),
		db_path,
		metadata_path,
		access,
	})
}
