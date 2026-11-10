use crate::{Change, CoreError};

/// Durable storage for the loaded budget. One command is one transaction.
pub trait Store {
	type Transaction<'a>: StoreTransaction
	where
		Self: 'a;

	fn begin(&mut self) -> Result<Self::Transaction<'_>, CoreError>;
}

/// If `apply` or `commit` fails, nothing from this transaction may persist.
pub trait StoreTransaction {
	fn apply(&mut self, change: &Change) -> Result<(), CoreError>;
	fn commit(self) -> Result<(), CoreError>;
	fn rollback(self) -> Result<(), CoreError>;
}
