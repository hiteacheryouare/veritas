use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use veritas_domain::{Change, CivilDate, CoreError, Row, StoreTransaction};
use veritas_store::{Access, NewAccount, StarterGroup, Store};

use crate::canonical::json::{self, Str, Value};
use crate::import::ImportedBudget;

/// What a new budget starts with. `categories` replaces the template's seed
/// categories (see `veritas_store::veritas_starter_set`).
#[derive(Clone, Debug)]
pub struct NewBudget<'a> {
	pub name: &'a str,
	pub currency_code: Option<&'a str>,
	pub number_format: Option<&'a str>,
	pub categories: &'a [StarterGroup],
	pub accounts: &'a [NewAccount],
	pub today: CivilDate,
}

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

/// Makes a new budget in `budget_dir`, which must not exist yet, from the
/// pinned template: an Actual-format file from its first write. It is built
/// in a hidden sibling folder and renamed into place, so a crash leaves
/// either no budget or a whole one.
pub fn create_budget(
	budget_dir: &Path,
	budget: &NewBudget<'_>,
) -> Result<ImportedBudget, CoreError> {
	if budget.name.trim().is_empty() {
		return Err(CoreError::Invalid("A budget needs a name.".to_owned()));
	}
	if budget_dir.exists() {
		return Err(CoreError::Io(format!(
			"{} already exists",
			budget_dir.display()
		)));
	}
	let id = budget_dir
		.file_name()
		.map(|name| name.to_string_lossy().into_owned())
		.ok_or_else(|| CoreError::Io(format!("{} has no folder name", budget_dir.display())))?;
	let building = budget_dir.with_file_name(format!(".{id}.creating"));
	if building.exists() {
		std::fs::remove_dir_all(&building).map_err(|error| io_error(&building, &error))?;
	}
	std::fs::create_dir(&building).map_err(|error| io_error(&building, &error))?;

	let built = build(&building, &id, budget).and_then(|audit| {
		std::fs::rename(&building, budget_dir).map_err(|error| io_error(budget_dir, &error))?;
		Ok(audit)
	});
	match built {
		Ok(audit) => Ok(ImportedBudget {
			dir: budget_dir.to_path_buf(),
			db_path: budget_dir.join("db.sqlite"),
			metadata_path: budget_dir.join("metadata.json"),
			access: Access::ReadWrite,
			audit,
		}),
		Err(error) => {
			let _ = std::fs::remove_dir_all(&building);
			Err(error)
		}
	}
}

fn build(
	dir: &Path,
	id: &str,
	budget: &NewBudget<'_>,
) -> Result<veritas_store::AuditReport, CoreError> {
	let db_path: PathBuf = dir.join("db.sqlite");
	std::fs::write(&db_path, veritas_store::TEMPLATE_DB)
		.map_err(|error| io_error(&db_path, &error))?;
	let metadata_path = dir.join("metadata.json");
	std::fs::write(&metadata_path, metadata(id, budget.name)?)
		.map_err(|error| io_error(&metadata_path, &error))?;

	let mut store = Store::open(&db_path, Access::ReadWrite)?;
	store.adopt()?;
	let mut planned = store.load()?.data;
	let mut writer = store.writer()?;
	if let Some(code) = budget.currency_code {
		writer.put_preference("defaultCurrencyCode", code)?;
	}
	if let Some(format) = budget.number_format {
		writer.put_preference("numberFormat", format)?;
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
	let categories = veritas_store::plan_starter_categories(&planned, budget.categories)?;
	write(&mut planned, categories)?;
	let now = now_ms();
	for account in budget.accounts {
		let rows = veritas_store::plan_new_account(&planned, account, budget.today, now)?;
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
