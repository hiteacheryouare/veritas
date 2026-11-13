//! R-REL-1: a committed action survives a power cut, not only a crash.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Scratch, fixture_db};
use veritas_domain::ReadOnlyReason;
use veritas_store::{Access, Store};

#[test]
fn a_writable_store_flushes_the_drive_cache_on_every_commit_and_checkpoint() {
	let scratch = Scratch::new("durability");
	let db = fixture_db("small", &scratch);

	let writer = Store::open(&db, Access::ReadWrite).unwrap();
	assert_eq!(writer.pragma_text("fullfsync").unwrap(), "1");
	assert_eq!(writer.pragma_text("checkpoint_fullfsync").unwrap(), "1");
	assert_eq!(writer.pragma_text("synchronous").unwrap(), "2");
	drop(writer);

	// A read-only open writes nothing, so it has nothing to flush.
	let reader = Store::open(&db, Access::ReadOnly(ReadOnlyReason::OpenElsewhere)).unwrap();
	assert_eq!(reader.pragma_text("fullfsync").unwrap(), "0");
}
