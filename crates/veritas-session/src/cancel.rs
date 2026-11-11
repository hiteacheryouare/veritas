use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use veritas_domain::CoreError;

/// Asks long work (import, create, export) to stop. The work checks it just
/// before its one visible step, the rename into place, so a cancelled run
/// leaves nothing behind. After that step it finishes regardless.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
	pub fn cancel(&self) {
		self.0.store(true, Ordering::SeqCst);
	}

	pub fn is_cancelled(&self) -> bool {
		self.0.load(Ordering::SeqCst)
	}

	pub(crate) fn check(&self) -> Result<(), CoreError> {
		if self.is_cancelled() {
			Err(CoreError::Cancelled)
		} else {
			Ok(())
		}
	}
}
