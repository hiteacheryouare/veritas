//! The Actual-schema SQLite adapter, the veritas_* extension tables and the invariant audit. The only crate that knows Actual's physical table and column names.

mod audit;
mod cells;
mod connection;
mod load;
mod schema;
mod write;

pub use audit::{AuditReport, Check, Violation, audit};
pub use connection::{Store, audit_connection, integrity_check, migration_ids, open_read_only};
pub use load::{LoadReport, Loaded, SkippedRow, load};
pub use schema::VERITAS_SCHEMA_VERSION;
pub use write::Writer;
