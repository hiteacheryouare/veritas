//! The Actual-schema SQLite adapter, the veritas_* extension tables and the invariant audit. The only crate that knows Actual's physical table and column names.

mod audit;
mod cells;
mod connection;
mod create;
mod load;
mod schema;
mod starter;
mod store_trait;
mod write;

pub use audit::{AuditReport, Check, Violation, audit};
pub use connection::{
	Access, Store, audit_connection, integrity_check, migration_ids, open_read_only,
	recover_after_crash,
};
pub use create::{
	NewAccount, TEMPLATE_DB, new_id, paycheck_accounts, plan_new_account, plan_starter_categories,
};
pub use load::{LoadReport, Loaded, SkippedRow, load};
pub use schema::VERITAS_SCHEMA_VERSION;
pub use starter::{StarterCategory, StarterGroup, veritas_starter_set};
pub use write::Writer;
