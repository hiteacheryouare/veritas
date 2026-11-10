/// Every failure the core can report (R-REL-7, contracts/errors.md). The FFI
/// layer maps each variant one to one, and the apps turn them into messages.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
	#[error("{entity} {id} doesn't exist")]
	NotFound { entity: &'static str, id: String },

	#[error("{0}")]
	Invalid(String),

	#[error("this budget is read-only: {0}")]
	ReadOnly(ReadOnlyReason),

	#[error(
		"this budget was made by an older Actual. Open it in Actual {required} or newer, then export it again"
	)]
	UpgradeInActualFirst { required: String },

	#[error("the budget failed its integrity audit ({violations} problems found)")]
	AuditFailed { violations: usize },

	#[error("the file isn't a valid Actual budget: {0}")]
	NotAnActualBudget(String),

	#[error("storage error: {0}")]
	Storage(String),

	#[error("file error: {0}")]
	Io(String),

	#[error("the amount is too large")]
	Overflow,

	#[error("cancelled")]
	Cancelled,

	#[error("internal error: {0}")]
	Internal(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadOnlyReason {
	NewerActualVersion,
	TrackingBudget,
	OpenElsewhere,
	AuditFailed,
}

impl std::fmt::Display for ReadOnlyReason {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(match self {
			ReadOnlyReason::NewerActualVersion => {
				"it was saved by a newer Actual than this version of Veritas supports"
			}
			ReadOnlyReason::TrackingBudget => {
				"it's a tracking budget, and Veritas edits envelope budgets only"
			}
			ReadOnlyReason::OpenElsewhere => "it's open in another Veritas window",
			ReadOnlyReason::AuditFailed => "its integrity audit found problems",
		})
	}
}
