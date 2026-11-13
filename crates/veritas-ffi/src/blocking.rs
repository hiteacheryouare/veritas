use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Waker};

use veritas_session::CancelToken as CoreCancel;

use crate::FfiError;

/// Cancels long work: open, import, create and export. Swift makes one per
/// call, passes it in, and calls `cancel()` from `withTaskCancellationHandler`.
/// UniFFI never cancels the Rust side of an `async` call by itself, so this
/// token is the only way work stops early.
#[derive(Debug, Default, uniffi::Object)]
pub struct CancelToken {
	token: CoreCancel,
}

#[uniffi::export]
impl CancelToken {
	#[uniffi::constructor]
	pub fn new() -> Arc<CancelToken> {
		Arc::new(CancelToken::default())
	}

	/// Stops the work before its one visible step. Calling it after the work
	/// finished, or twice, does nothing.
	pub fn cancel(&self) {
		self.token.cancel();
	}

	pub fn is_cancelled(&self) -> bool {
		self.token.is_cancelled()
	}
}

impl CancelToken {
	pub(crate) fn core(&self) -> CoreCancel {
		self.token.clone()
	}
}

struct Slot<T> {
	result: Option<Result<T, FfiError>>,
	waker: Option<Waker>,
}

/// Blocking core work on its own thread, as a future Swift can await.
pub(crate) struct Blocking<T> {
	slot: Arc<Mutex<Slot<T>>>,
}

pub(crate) fn run<T: Send + 'static>(
	cancel: &CancelToken,
	work: impl FnOnce(&CoreCancel) -> Result<T, FfiError> + Send + 'static,
) -> Blocking<T> {
	let slot = Arc::new(Mutex::new(Slot {
		result: None,
		waker: None,
	}));
	let (thread_slot, thread_cancel) = (slot.clone(), cancel.core());
	let spawned = std::thread::Builder::new()
		.name("veritas-core".to_owned())
		.spawn(move || {
			let result =
				catch_unwind(AssertUnwindSafe(|| work(&thread_cancel))).unwrap_or_else(|_| {
					Err(FfiError::Internal {
						message: "the core hit a bug in background work".to_owned(),
					})
				});
			let mut slot = thread_slot.lock().unwrap_or_else(PoisonError::into_inner);
			slot.result = Some(result);
			if let Some(waker) = slot.waker.take() {
				waker.wake();
			}
		});
	if let Err(error) = spawned {
		slot.lock().unwrap_or_else(PoisonError::into_inner).result =
			Some(Err(FfiError::Internal {
				message: format!("couldn't start background work: {error}"),
			}));
	}
	Blocking { slot }
}

impl<T> Future for Blocking<T> {
	type Output = Result<T, FfiError>;

	fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
		let mut slot = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
		match slot.result.take() {
			Some(result) => Poll::Ready(result),
			None => {
				slot.waker = Some(context.waker().clone());
				Poll::Pending
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use std::task::Wake;
	use std::thread::Thread;
	use std::time::{Duration, Instant};

	use super::*;

	struct Unpark(Thread);

	impl Wake for Unpark {
		fn wake(self: Arc<Self>) {
			self.0.unpark();
		}
	}

	fn block_on<F: Future + Unpin>(mut future: F) -> F::Output {
		let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
		let mut context = Context::from_waker(&waker);
		loop {
			if let Poll::Ready(output) = Pin::new(&mut future).poll(&mut context) {
				return output;
			}
			std::thread::park();
		}
	}

	#[test]
	fn awaiting_blocking_work_returns_its_result() {
		let token = CancelToken::new();
		assert_eq!(block_on(run(&token, |_| Ok(42))).unwrap(), 42);
		assert!(matches!(
			block_on(run(&token, |_| -> Result<(), FfiError> { panic!("a bug") })),
			Err(FfiError::Internal { .. })
		));
	}

	#[test]
	fn cancelling_the_token_reaches_work_already_running() {
		let token = CancelToken::new();
		let future = run(&token, |cancel| {
			let deadline = Instant::now() + Duration::from_secs(5);
			while !cancel.is_cancelled() && Instant::now() < deadline {
				std::thread::sleep(Duration::from_millis(1));
			}
			Ok(cancel.is_cancelled())
		});
		std::thread::sleep(Duration::from_millis(20));
		token.cancel();
		assert!(block_on(future).unwrap());
		assert!(token.is_cancelled());
	}

	#[test]
	fn finished_work_leaves_the_token_alone() {
		let token = CancelToken::new();
		block_on(run(&token, |_| Ok(()))).unwrap();
		assert!(!token.is_cancelled());
	}
}
