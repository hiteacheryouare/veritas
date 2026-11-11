use crate::{AccountKind, CoreError, Money};

/// Actual's number format preferences: `numberFormat`, `hideFraction` and
/// `defaultCurrencyCode` (loot-core/src/types/prefs.ts:26-31).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NumberFormat {
	/// An ISO 4217 code, or `None` for no currency symbol, Actual's default.
	pub currency: Option<String>,
	pub style: NumberStyle,
	pub hide_decimals: bool,
}

/// Actual's `numberFormat` values (loot-core/src/shared/util.ts:265-271).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumberStyle {
	/// `1,000.33`, Actual's default.
	CommaDot,
	/// `1.000,33`
	DotComma,
	/// `1 000,33`
	SpaceComma,
	/// `1'000.33`
	ApostropheDot,
	/// `1,00,000.33`, Indian grouping.
	CommaDotIn,
}

impl NumberStyle {
	/// The value Actual stores in `preferences.numberFormat`.
	pub fn actual_value(self) -> &'static str {
		match self {
			NumberStyle::CommaDot => "comma-dot",
			NumberStyle::DotComma => "dot-comma",
			NumberStyle::SpaceComma => "space-comma",
			NumberStyle::ApostropheDot => "apostrophe-dot",
			NumberStyle::CommaDotIn => "comma-dot-in",
		}
	}
}

/// What new-budget setup asks for (R-IO-0).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewBudget {
	pub name: String,
	pub format: NumberFormat,
	/// Off-budget accounts for a 401(k) and an HSA, so gross-paycheck
	/// contributions classify as savings (D56, CLS-10).
	pub retirement_401k: bool,
	pub hsa: bool,
	pub accounts: Vec<NewAccount>,
}

/// An account setup creates, with what it holds today.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewAccount {
	pub name: String,
	pub kind: NewAccountKind,
	/// As the user typed it: for a card, a line of credit, a loan or a
	/// mortgage, what is owed, as a positive amount. `opening_balance` gives
	/// the signed amount the account stores.
	pub starting_balance: Money,
}

/// What setup offers. Each kind decides the budget flag and the account's
/// side (D8: cards stay on budget, loans off; docs/finance/statements.md
/// ST-9.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewAccountKind {
	Checking,
	Savings,
	Cash,
	CreditCard,
	LineOfCredit,
	Investment,
	Retirement,
	Property,
	Loan,
	Mortgage,
}

impl NewAccountKind {
	pub fn off_budget(self) -> bool {
		match self {
			NewAccountKind::Checking
			| NewAccountKind::Savings
			| NewAccountKind::Cash
			| NewAccountKind::CreditCard
			| NewAccountKind::LineOfCredit => false,
			NewAccountKind::Investment
			| NewAccountKind::Retirement
			| NewAccountKind::Property
			| NewAccountKind::Loan
			| NewAccountKind::Mortgage => true,
		}
	}

	pub fn account_kind(self) -> AccountKind {
		match self {
			NewAccountKind::CreditCard
			| NewAccountKind::LineOfCredit
			| NewAccountKind::Loan
			| NewAccountKind::Mortgage => AccountKind::Liability,
			NewAccountKind::Checking
			| NewAccountKind::Savings
			| NewAccountKind::Cash
			| NewAccountKind::Investment
			| NewAccountKind::Retirement
			| NewAccountKind::Property => AccountKind::Asset,
		}
	}
}

impl NewAccount {
	/// The starting balance the account stores, in Actual's sign: what a
	/// liability owes becomes negative.
	pub fn opening_balance(&self) -> Result<Money, CoreError> {
		match self.kind.account_kind() {
			AccountKind::Asset => Ok(self.starting_balance),
			AccountKind::Liability => self.starting_balance.checked_neg(),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn account(kind: NewAccountKind, typed: i64) -> NewAccount {
		NewAccount {
			name: "Account".to_owned(),
			kind,
			starting_balance: Money::from_minor(typed),
		}
	}

	#[test]
	fn each_kind_lands_on_its_side_of_the_budget() {
		use NewAccountKind::*;
		let placed: Vec<_> = [
			Checking,
			Savings,
			Cash,
			CreditCard,
			LineOfCredit,
			Investment,
			Retirement,
			Property,
			Loan,
			Mortgage,
		]
		.into_iter()
		.map(|kind| (kind.off_budget(), kind.account_kind()))
		.collect();
		use AccountKind::{Asset, Liability};
		assert_eq!(
			placed,
			[
				(false, Asset),
				(false, Asset),
				(false, Asset),
				(false, Liability),
				(false, Liability),
				(true, Asset),
				(true, Asset),
				(true, Asset),
				(true, Liability),
				(true, Liability),
			]
		);
	}

	#[test]
	fn what_a_liability_owes_is_stored_negative() {
		assert_eq!(
			account(NewAccountKind::CreditCard, 45_000)
				.opening_balance()
				.unwrap(),
			Money::from_minor(-45_000)
		);
		assert_eq!(
			account(NewAccountKind::Mortgage, 25_000_000)
				.opening_balance()
				.unwrap(),
			Money::from_minor(-25_000_000)
		);
		assert_eq!(
			account(NewAccountKind::Checking, 120_000)
				.opening_balance()
				.unwrap(),
			Money::from_minor(120_000)
		);
		assert!(matches!(
			account(NewAccountKind::Loan, i64::MIN).opening_balance(),
			Err(CoreError::Overflow)
		));
	}
}
