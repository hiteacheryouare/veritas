use std::path::Path;

use veritas_domain::{CivilDate, CoreError};
use veritas_interop::ExportOutcome;
use veritas_store::{Access, AuditReport, Loaded, Store};

use veritas_domain::NewBudget;

use crate::session::Backend;

/// veritas-store's working copy is the real backend. Its writes come from
/// veritas-store's own `veritas_domain::Store` impl.
impl Backend for Store {
	fn open(db: &Path, access: Access) -> Result<Self, CoreError> {
		Store::open(db, access)
	}

	fn migration_ids(&self) -> Result<Vec<i64>, CoreError> {
		Store::migration_ids(self)
	}

	fn quick_problems(&self) -> Result<Vec<String>, CoreError> {
		let verdict = self.pragma_text("quick_check")?;
		Ok(if verdict == "ok" {
			Vec::new()
		} else {
			vec![verdict]
		})
	}

	fn verify(db: &Path) -> Result<AuditReport, CoreError> {
		let (_, report) = veritas_store::audit_connection(&veritas_store::open_read_only(db)?)?;
		Ok(report)
	}

	fn adopt(&mut self) -> Result<(), CoreError> {
		Store::adopt(self)
	}

	fn load(&self) -> Result<Loaded, CoreError> {
		Store::load(self)
	}

	fn export_actual_zip(&self, metadata: &Path, to: &Path) -> Result<ExportOutcome, CoreError> {
		veritas_interop::export_actual_zip(self, metadata, to)
	}

	fn create(dir: &Path, budget: &NewBudget, today: CivilDate) -> Result<(), CoreError> {
		veritas_interop::create_budget(dir, budget, today).map(|_| ())
	}

	fn recover_after_crash(db: &Path) -> Result<(), CoreError> {
		veritas_store::recover_after_crash(db)
	}
}
