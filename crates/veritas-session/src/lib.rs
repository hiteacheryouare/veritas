//! Open and close, the single-writer lock, the command bus with undo and redo, change events, snapshots and backups.

mod background;
mod backup;
mod bus;
mod fsio;
mod lock;
mod retention;
mod session;
mod shared;
mod snapshot;
mod store;

pub use backup::write_backup;
pub use bus::{ChangeListener, ChangeSet, Command, CommandBus, Listeners, Permit, Subscription};
pub use lock::{Access, LOCK_FILE, WriterLock, acquire};
pub use retention::{RetentionPolicy, expired};
pub use session::{Backend, BackupResult, Clock, DB_FILE, METADATA_FILE, Session, SessionOptions};
pub use shared::SharedSession;
pub use snapshot::{
	SNAPSHOTS_DIR, Snapshot, SnapshotReason, list_snapshots, prune_snapshots, restore_snapshot,
	take_snapshot,
};
