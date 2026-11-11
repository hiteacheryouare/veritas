use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Waker};

use veritas_session::CancelToken;

use crate::FfiError;

struct Slot<T> {
	result: Option<Result<T, FfiError>>,
	waker: Option<Waker>,
}

/// Blocking core work on its own thread, as a future Swift can await. When
/// Swift cancels its task, UniFFI drops this future, and dropping it cancels
/// the work's token.
pub(crate) struct Blocking<T> {
	slot: Arc<Mutex<Slot<T>>>,
	cancel: CancelToken,
}

pub(crate) fn run<T: Send + 'static>(
	work: impl FnOnce(&CancelToken) -> Result<T, FfiError> + Send + 'static,
) -> Blocking<T> {
	let slot = Arc::new(Mutex::new(Slot {
		result: None,
		waker: None,
	}));
	let cancel = CancelToken::default();
	let (thread_slot, thread_cancel) = (slot.clone(), cancel.clone());
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
	Blocking { slot, cancel }
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

impl<T> Drop for Blocking<T> {
	fn drop(&mut self) {
		self.cancel.cancel();
	}
}

#[cfg(test)]
mod tests {
	use std::sync::atomic::{AtomicBool, Ordering};
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
		assert_eq!(block_on(run(|_| Ok(42))).unwrap(), 42);
		assert!(matches!(
			block_on(run(|_| -> Result<(), FfiError> { panic!("a bug") })),
			Err(FfiError::Internal { .. })
		));
	}

	#[test]
	fn dropping_the_future_cancels_the_work() {
		let saw_cancel = Arc::new(AtomicBool::new(false));
		let seen = saw_cancel.clone();
		let future = run(move |cancel| {
			let deadline = Instant::now() + Duration::from_secs(5);
			while !cancel.is_cancelled() && Instant::now() < deadline {
				std::thread::sleep(Duration::from_millis(1));
			}
			seen.store(cancel.is_cancelled(), Ordering::SeqCst);
			Ok(())
		});
		drop(future);
		let deadline = Instant::now() + Duration::from_secs(5);
		while !saw_cancel.load(Ordering::SeqCst) && Instant::now() < deadline {
			std::thread::sleep(Duration::from_millis(1));
		}
		assert!(saw_cancel.load(Ordering::SeqCst));
	}
}
