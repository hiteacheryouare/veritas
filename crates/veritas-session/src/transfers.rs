//! Move money's preview (R-XFER-1, R-XFER-4): both balances before and after,
//! and whether the move leaves the budget, so the sheet asks what it was for.

use veritas_domain::{AccountId, BudgetData, CivilDate, CoreError, Money};

use crate::accounts::{balances, live_account};
use crate::loans::in_loans;
use crate::transactions::storable;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BalanceAfter {
	pub account: AccountId,
	/// The account's total, as `accounts()` shows it.
	pub now: Money,
	pub after: Money,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferPurpose {
	None,
	/// On budget to off budget: R-XFER-4's question (CLS-4.1).
	Ask,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferPreview {
	pub from: BalanceAfter,
	pub to: BalanceAfter,
	pub purpose: TransferPurpose,
	/// On budget into a loan (docs/finance/statements.md ST-9.1): the sheet
	/// offers the loan payment wizard (R-DEBT-2).
	pub to_loan: bool,
}

pub fn transfer_preview(
	data: &BudgetData,
	from: &AccountId,
	to: &AccountId,
	amount: Money,
	today: CivilDate,
) -> Result<TransferPreview, CoreError> {
	if amount <= Money::ZERO {
		return Err(CoreError::Invalid(
			"Enter how much to move, more than zero.".to_owned(),
		));
	}
	storable(amount)?;
	if from == to {
		return Err(CoreError::Invalid(
			"Choose two different accounts.".to_owned(),
		));
	}
	let source = live_account(data, from)?;
	let target = live_account(data, to)?;
	let leaves_budget = !source.off_budget && target.off_budget;
	let from_now = balances(data, from)?.total;
	let to_now = balances(data, to)?.total;
	Ok(TransferPreview {
		from: BalanceAfter {
			account: from.clone(),
			now: from_now,
			after: from_now.checked_sub(amount)?,
		},
		to: BalanceAfter {
			account: to.clone(),
			now: to_now,
			after: to_now.checked_add(amount)?,
		},
		purpose: if leaves_budget {
			TransferPurpose::Ask
		} else {
			TransferPurpose::None
		},
		to_loan: leaves_budget && in_loans(data, to, today)?,
	})
}
