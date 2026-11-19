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

	/// A register-scale amount (2 implied places) on the budget scale of a
	/// currency with `places` (CLS-12.9, CLS-12.11). With fewer than 2 places
	/// it is rounded half up to whole units first, so 50000 is 500 in a yen
	/// budget and 50050 is 501. A 2-place currency keeps the integer.
	pub fn register_to_budget_scale(self, places: u8) -> Result<Money, CoreError> {
		match places {
			2 => Ok(self),
			0 | 1 => {
				let unit = 10_i64.pow(u32::from(2 - places));
				let rounded = self
					.0
					.checked_add(unit / 2)
					.ok_or(CoreError::Overflow)?
					.div_euclid(unit);
				Ok(Money(rounded))
			}
			_ => 10_i64
				.checked_pow(u32::from(places - 2))
				.and_then(|scale| self.0.checked_mul(scale))
				.map(Money)
				.ok_or(CoreError::Overflow),
		}
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
	fn a_register_amount_moves_to_the_budget_scale_by_the_currencys_places() {
		let scaled = |minor, places| {
			Money::from_minor(minor)
				.register_to_budget_scale(places)
				.unwrap()
				.minor()
		};
		assert_eq!(scaled(32_017, 2), 32_017, "2 places: one scale");
		assert_eq!(scaled(50_000, 0), 500, "CLS-12.9: 50000 is 500 yen");
		assert_eq!(scaled(50_050, 0), 501, "half up");
		assert_eq!(scaled(50_049, 0), 500);
		assert_eq!(scaled(1_234, 3), 12_340);
	}

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
