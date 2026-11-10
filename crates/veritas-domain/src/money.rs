use std::fmt;

use crate::CoreError;

/// An amount in minor units (hundredths), the same integer Actual stores.
/// Negative is money leaving an account.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Money(i64);

impl Money {
	pub const ZERO: Money = Money(0);

	pub const fn from_minor(minor: i64) -> Self {
		Money(minor)
	}

	pub const fn minor(self) -> i64 {
		self.0
	}

	pub fn checked_add(self, other: Money) -> Result<Money, CoreError> {
		self.0
			.checked_add(other.0)
			.map(Money)
			.ok_or(CoreError::Overflow)
	}

	pub fn checked_sub(self, other: Money) -> Result<Money, CoreError> {
		self.0
			.checked_sub(other.0)
			.map(Money)
			.ok_or(CoreError::Overflow)
	}

	pub fn checked_neg(self) -> Result<Money, CoreError> {
		self.0.checked_neg().map(Money).ok_or(CoreError::Overflow)
	}

	pub fn checked_sum(amounts: impl IntoIterator<Item = Money>) -> Result<Money, CoreError> {
		amounts
			.into_iter()
			.try_fold(Money::ZERO, |total, amount| total.checked_add(amount))
	}

	pub const fn is_negative(self) -> bool {
		self.0 < 0
	}
}

impl fmt::Display for Money {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let sign = if self.0 < 0 { "-" } else { "" };
		let magnitude = self.0.unsigned_abs();
		write!(f, "{sign}{}.{:02}", magnitude / 100, magnitude % 100)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn displays_whole_cents_without_float_error() {
		assert_eq!(Money::from_minor(11234).to_string(), "112.34");
		assert_eq!(Money::from_minor(-3475).to_string(), "-34.75");
		assert_eq!(Money::from_minor(5).to_string(), "0.05");
		assert_eq!(
			Money::from_minor(i64::MIN).to_string(),
			"-92233720368547758.08"
		);
	}

	#[test]
	fn overflow_is_an_error_not_a_panic() {
		let max = Money::from_minor(i64::MAX);
		assert!(matches!(
			max.checked_add(Money::from_minor(1)),
			Err(CoreError::Overflow)
		));
		assert!(matches!(
			Money::from_minor(i64::MIN).checked_neg(),
			Err(CoreError::Overflow)
		));
	}

	#[test]
	fn sums_a_month_of_transactions() {
		let month = [-4_512, -165_000, 250_000, -8_999].map(Money::from_minor);
		assert_eq!(
			Money::checked_sum(month).map(Money::minor).ok(),
			Some(71_489)
		);
	}
}
