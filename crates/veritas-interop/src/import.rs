use std::path::{Path, PathBuf};

use veritas_domain::CoreError;
use veritas_store::{AuditReport, Store};

use crate::archive::BudgetArchive;
use crate::canonical::json;
use crate::manifest::{Access, Manifest};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportedBudget {
	pub dir: PathBuf,
	pub db_path: PathBuf,
	pub metadata_path: PathBuf,
	pub access: Access,
	/// What the invariant audit found. Never corruption: that refuses the import.
	pub audit: AuditReport,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportOutcome {
	Imported(ImportedBudget),
	/// SQLite found the copy damaged, so Veritas didn't keep it (PLAN §6.6).
	/// The report is all that remains.
	Refused(AuditReport),
}

fn io_error(path: &Path, error: &std::io::Error) -> CoreError {
	CoreError::Io(format!("{}: {error}", path.display()))
}

/// R-IO-1, R-IO-8: copies an Actual export into `budget_dir`, which must not
/// exist yet, then checks and audits the copy (R-IO-7, R-REL-2). The zip is
/// only ever read. Unless the budget is imported, the new folder is removed,
/// so nothing is left behind.
pub fn import_actual_zip(zip_path: &Path, budget_dir: &Path) -> Result<ImportOutcome, CoreError> {
	let mut archive = BudgetArchive::open(zip_path)?;
	let metadata = archive.metadata_bytes()?;
	json::parse(&String::from_utf8_lossy(&metadata))
		.map_err(|reason| CoreError::NotAnActualBudget(reason.to_owned()))?;

	std::fs::create_dir(budget_dir).map_err(|error| io_error(budget_dir, &error))?;
	let result = populate(&mut archive, &metadata, budget_dir);
	if !matches!(result, Ok(ImportOutcome::Imported(_))) {
		let _ = std::fs::remove_dir_all(budget_dir);
	}
	result
}

fn populate(
	archive: &mut BudgetArchive,
	metadata: &[u8],
	budget_dir: &Path,
) -> Result<ImportOutcome, CoreError> {
	let db_path = budget_dir.join("db.sqlite");
	let metadata_path = budget_dir.join("metadata.json");
	archive.extract_db(&db_path)?;
	std::fs::write(&metadata_path, metadata).map_err(|error| io_error(&metadata_path, &error))?;

	let conn = veritas_store::open_read_only(&db_path)?;
	let problems = veritas_store::integrity_check(&conn)?;
	if !problems.is_empty() {
		return Ok(ImportOutcome::Refused(AuditReport::from_integrity(
			problems,
		)));
	}
	let access = Manifest::supported()?.check(&veritas_store::migration_ids(&conn)?)?;
	let audit = veritas_store::audit(&veritas_store::load(&conn)?);
	drop(conn);

	Ok(ImportOutcome::Imported(ImportedBudget {
		dir: budget_dir.to_path_buf(),
		db_path,
		metadata_path,
		access,
		audit,
	}))
}

/// Opens an imported budget's working copy with the access its migration
/// history allows: a file from a newer Actual opens read-only, and one from an
/// older Actual doesn't open.
pub fn open_budget(db_path: &Path) -> Result<Store, CoreError> {
	let access = {
		let conn = veritas_store::open_read_only(db_path)?;
		Manifest::supported()?.check(&veritas_store::migration_ids(&conn)?)?
	};
	Store::open(db_path, access)
}
