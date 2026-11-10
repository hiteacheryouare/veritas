use std::fmt;

macro_rules! id {
	($($name:ident),* $(,)?) => {$(
		#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
		pub struct $name(String);

		impl $name {
			pub fn new(id: impl Into<String>) -> Self {
				$name(id.into())
			}

			pub fn as_str(&self) -> &str {
				&self.0
			}
		}

		impl fmt::Display for $name {
			fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
				f.write_str(&self.0)
			}
		}
	)*};
}

// Actual ids are opaque TEXT: usually UUIDv4, but seeded rows and old files
// carry other shapes, so nothing here assumes a format.
id!(
	AccountId,
	AccountGroupId,
	CategoryId,
	CategoryGroupId,
	PayeeId,
	TransactionId,
	RuleId,
	ScheduleId,
	TagId,
	GoalId,
	ScenarioId,
);
