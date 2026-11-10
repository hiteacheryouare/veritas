use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Mutex, MutexGuard};

use veritas_domain::CoreError;

use crate::session::{Backend, Session};

/// A session shared across threads, as the FFI holds it. A bug that panics
/// mid-call can leave memory half-changed while storage rolled back, so the
/// session reloads from disk before it serves anything else.
pub struct SharedSession<B: Backend> {
	inner: Mutex<Session<B>>,
}

impl<B: Backend> SharedSession<B> {
	pub fn new(session: Session<B>) -> Self {
		SharedSession {
			inner: Mutex::new(session),
		}
	}

	pub fn with<T>(
		&self,
		call: impl FnOnce(&mut Session<B>) -> Result<T, CoreError>,
	) -> Result<T, CoreError> {
		let mut session = self.lock()?;
		match catch_unwind(AssertUnwindSafe(|| call(&mut session))) {
			Ok(result) => result,
			Err(payload) => {
				let detail = panic_detail(payload.as_ref());
				Err(match session.reload() {
					Ok(_) => CoreError::Internal(format!(
						"the core hit a bug: {detail}. The budget was reloaded from disk"
					)),
					Err(error) => CoreError::Internal(format!(
						"the core hit a bug: {detail}, and reloading the budget failed: {error}"
					)),
				})
			}
		}
	}

	fn lock(&self) -> Result<MutexGuard<'_, Session<B>>, CoreError> {
		match self.inner.lock() {
			Ok(session) => Ok(session),
			Err(poisoned) => {
				self.inner.clear_poison();
				let mut session = poisoned.into_inner();
				session.reload()?;
				Ok(session)
			}
		}
	}
}

fn panic_detail(payload: &(dyn Any + Send)) -> String {
	payload
		.downcast_ref::<&str>()
		.map(|text| (*text).to_owned())
		.or_else(|| payload.downcast_ref::<String>().cloned())
		.unwrap_or_else(|| "unknown panic".to_owned())
}
