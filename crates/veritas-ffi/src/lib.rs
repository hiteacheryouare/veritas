//! The only surface the apps see: commands, queries and the change-event stream, exported through UniFFI.

use std::panic::{AssertUnwindSafe, catch_unwind};

use veritas_domain::{CoreError, ReadOnlyReason, YearMonth};

uniffi::setup_scaffolding!();

/// contracts/errors.md maps each variant to what the apps show.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FfiError {
	#[error("{entity} {id} doesn't exist")]
	NotFound { entity: String, id: String },
	#[error("{message}")]
	Invalid { message: String },
	#[error("read-only: {reason:?}")]
	ReadOnly { reason: ReadOnlyKind },
	#[error("open it in Actual {required} or newer first")]
	UpgradeInActualFirst { required: String },
	#[error("integrity audit found {violations} problems")]
	AuditFailed { violations: u64 },
	#[error("{message}")]
	NotAnActualBudget { message: String },
	#[error("{message}")]
	Storage { message: String },
	#[error("{message}")]
	Io { message: String },
	#[error("the amount is too large")]
	Overflow,
	#[error("cancelled")]
	Cancelled,
	#[error("{message}")]
	Internal { message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ReadOnlyKind {
	NewerActualVersion,
	TrackingBudget,
	OpenElsewhere,
	AuditFailed,
}

impl From<CoreError> for FfiError {
	fn from(error: CoreError) -> Self {
		match error {
			CoreError::NotFound { entity, id } => FfiError::NotFound {
				entity: entity.to_owned(),
				id,
			},
			CoreError::Invalid(message) => FfiError::Invalid { message },
			CoreError::ReadOnly(reason) => FfiError::ReadOnly {
				reason: match reason {
					ReadOnlyReason::NewerActualVersion => ReadOnlyKind::NewerActualVersion,
					ReadOnlyReason::TrackingBudget => ReadOnlyKind::TrackingBudget,
					ReadOnlyReason::OpenElsewhere => ReadOnlyKind::OpenElsewhere,
					ReadOnlyReason::AuditFailed => ReadOnlyKind::AuditFailed,
				},
			},
			CoreError::UpgradeInActualFirst { required } => {
				FfiError::UpgradeInActualFirst { required }
			}
			CoreError::AuditFailed { violations } => FfiError::AuditFailed {
				violations: violations as u64,
			},
			CoreError::NotAnActualBudget(message) => FfiError::NotAnActualBudget { message },
			CoreError::Storage(message) => FfiError::Storage { message },
			CoreError::Io(message) => FfiError::Io { message },
			CoreError::Overflow => FfiError::Overflow,
			CoreError::Cancelled => FfiError::Cancelled,
			CoreError::Internal(message) => FfiError::Internal { message },
		}
	}
}

/// Every export runs through here, so a bug in the core becomes an
/// `Internal` error the app can show instead of a crash (R-REL-7).
fn guard<T>(call: impl FnOnce() -> Result<T, CoreError>) -> Result<T, FfiError> {
	match catch_unwind(AssertUnwindSafe(call)) {
		Ok(result) => result.map_err(FfiError::from),
		Err(payload) => {
			let detail = payload
				.downcast_ref::<&str>()
				.map(|text| (*text).to_owned())
				.or_else(|| payload.downcast_ref::<String>().cloned())
				.unwrap_or_else(|| "unknown panic".to_owned());
			Err(FfiError::Internal {
				message: format!("the core hit a bug: {detail}"),
			})
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct Month {
	pub year: i32,
	pub month: u32,
}

#[uniffi::export]
pub fn core_version() -> String {
	env!("CARGO_PKG_VERSION").to_owned()
}

#[uniffi::export]
pub fn parse_month(text: String) -> Result<Month, FfiError> {
	guard(|| {
		YearMonth::parse(&text).map(|month| Month {
			year: month.year(),
			month: month.month(),
		})
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_panic_inside_the_core_becomes_an_internal_error() {
		let result: Result<(), FfiError> = guard(|| panic!("index out of bounds"));
		match result {
			Err(FfiError::Internal { message }) => assert!(message.contains("index out of bounds")),
			other => panic!("expected an internal error, got {other:?}"),
		}
	}

	#[test]
	fn core_errors_cross_the_boundary_with_their_details() {
		assert!(matches!(
			parse_month("2027-13".to_owned()),
			Err(FfiError::Invalid { message }) if message.contains("2027-13")
		));
		assert_eq!(
			parse_month("2027-03".to_owned()).ok(),
			Some(Month {
				year: 2027,
				month: 3
			})
		);
	}
}
