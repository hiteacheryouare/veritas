use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use veritas_domain::{Change, CivilDate, CoreError, NewBudget, Row, StoreTransaction};
use veritas_store::{Access, Store};

use crate::canonical::json::{self, Str, Value};
use crate::import::ImportedBudget;

fn io_error(path: &Path, error: &std::io::Error) -> CoreError {
	CoreError::Io(format!("{}: {error}", path.display()))
}

fn now_ms() -> f64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|elapsed| elapsed.as_millis() as f64)
		.unwrap_or_default()
}

/// Actual's createBudget writes `{id, budgetName}`, with the folder name as id.
fn metadata(id: &str, name: &str) -> Result<Vec<u8>, CoreError> {
	let value = Value::Object(vec![
		(Str::from("id"), Value::String(Str::from(id))),
		(Str::from("budgetName"), Value::String(Str::from(name))),
	]);
	let mut out = Vec::new();
	json::stringify(&mut out, &value).map_err(|error| CoreError::Internal(error.to_string()))?;
	Ok(out)
}

/// Builds a complete new budget, db.sqlite and metadata.json, into `dir`,
/// which must exist and be empty: the pinned template, an Actual-format file
/// from its first write, with Veritas's starter categories, setup's accounts
/// and then the 401(k) and HSA accounts it asked for. Starting balances are
/// dated `today`. This isn't atomic on its own; the caller builds in
/// a hidden folder and renames it into place, and discards the folder if
/// this fails.
pub fn create_budget(
	dir: &Path,
	budget: &NewBudget,
	today: CivilDate,
) -> Result<ImportedBudget, CoreError> {
	if budget.name.trim().is_empty() {
		return Err(CoreError::Invalid("A budget needs a name.".to_owned()));
	}
	let mut entries = std::fs::read_dir(dir).map_err(|error| io_error(dir, &error))?;
	if entries.next().is_some() {
		return Err(CoreError::Io(format!("{} isn't empty", dir.display())));
	}
	let id = dir
		.file_name()
		.map(|name| name.to_string_lossy().into_owned())
		.ok_or_else(|| CoreError::Io(format!("{} has no folder name", dir.display())))?;
	let audit = build(dir, &id, budget, today)?;
	Ok(ImportedBudget {
		dir: dir.to_path_buf(),
		db_path: dir.join("db.sqlite"),
		metadata_path: dir.join("metadata.json"),
		access: Access::ReadWrite,
		audit,
	})
}

fn build(
	dir: &Path,
	id: &str,
	budget: &NewBudget,
	today: CivilDate,
) -> Result<veritas_store::AuditReport, CoreError> {
	let db_path: PathBuf = dir.join("db.sqlite");
	std::fs::write(&db_path, veritas_store::TEMPLATE_DB)
		.map_err(|error| io_error(&db_path, &error))?;
	let metadata_path = dir.join("metadata.json");
	std::fs::write(&metadata_path, metadata(id, &budget.name)?)
		.map_err(|error| io_error(&metadata_path, &error))?;

	let mut store = Store::open(&db_path, Access::ReadWrite)?;
	store.adopt()?;
	let mut planned = store.load()?.data;
	let mut writer = store.writer()?;
	writer.put_preference("numberFormat", budget.format.style.actual_value())?;
	if budget.format.hide_decimals {
		writer.put_preference("hideFraction", "true")?;
	}
	if let Some(code) = &budget.format.currency {
		writer.put_preference("defaultCurrencyCode", code)?;
	}
	let mut write =
		|planned: &mut veritas_domain::BudgetData, rows: Vec<Row>| -> Result<(), CoreError> {
			for row in rows {
				let change = Change::put(planned, row);
				writer.apply(&change)?;
				change.apply(planned);
			}
			Ok(())
		};
	let categories =
		veritas_store::plan_starter_categories(&planned, &veritas_store::veritas_starter_set())?;
	write(&mut planned, categories)?;
	let now = now_ms();
	let paycheck = veritas_store::paycheck_accounts(budget.retirement_401k, budget.hsa);
	for account in budget.accounts.iter().chain(&paycheck) {
		let rows = veritas_store::plan_new_account(&planned, account, today, now)?;
		write(&mut planned, rows)?;
	}
	writer.commit()?;

	let (_, audit) = store.open_audit()?;
	if !audit.is_clean() {
		return Err(CoreError::Internal(format!(
			"a new budget failed its own audit: {:?}",
			audit.violations
		)));
	}
	drop(store);
	Ok(audit)
}
