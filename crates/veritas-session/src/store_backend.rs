use std::path::Path;

use veritas_domain::{Change, CoreError, Store, StoreTransaction};
use veritas_store::Loaded;

use crate::session::Backend;

/// veritas-store's working copy behind a session. Opening, checking and
/// loading are real. Saving waits on the store's write path, so until it
/// lands every command fails with a storage error and nothing changes.
pub struct StoreBackend {
	store: veritas_store::Store,
}

pub struct PendingWrites;

impl Store for StoreBackend {
	type Transaction<'a> = PendingWrites;

	fn begin(&mut self) -> Result<PendingWrites, CoreError> {
		Ok(PendingWrites)
	}
}

impl StoreTransaction for PendingWrites {
	fn apply(&mut self, _: &Change) -> Result<(), CoreError> {
		Err(CoreError::Storage(
			"this build of Veritas can't save changes yet".to_owned(),
		))
	}

	fn commit(self) -> Result<(), CoreError> {
		Ok(())
	}

	fn rollback(self) -> Result<(), CoreError> {
		Ok(())
	}
}

impl Backend for StoreBackend {
	fn open(db: &Path) -> Result<Self, CoreError> {
		veritas_store::Store::open(db).map(|store| StoreBackend { store })
	}

	fn migration_ids(&self) -> Result<Vec<i64>, CoreError> {
		self.store.migration_ids()
	}

	fn quick_problems(&self) -> Result<Vec<String>, CoreError> {
		let verdict = self.store.pragma_text("quick_check")?;
		Ok(if verdict == "ok" {
			Vec::new()
		} else {
			vec![verdict]
		})
	}

	fn verify(db: &Path) -> Result<Vec<String>, CoreError> {
		veritas_store::integrity_check(&veritas_store::open_read_only(db)?)
	}

	fn adopt(&mut self) -> Result<(), CoreError> {
		self.store.adopt()
	}

	fn load(&self) -> Result<Loaded, CoreError> {
		self.store.load()
	}
}
