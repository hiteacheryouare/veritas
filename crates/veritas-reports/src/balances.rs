use std::cmp::Ordering;
use std::collections::BTreeMap;

use veritas_domain::{
	Account, AccountGroupId, AccountId, AccountKind, BudgetData, CivilDate, CoreError, Money,
};

use crate::{Classification, ClassifiedRow};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
	Asset,
	Liability,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AccountBalance {
	pub account: AccountId,
	pub group: Option<AccountGroupId>,
	/// Actual's sign: a liability's balance is negative.
	pub balance: Money,
	pub side: Side,
	pub on_budget: bool,
	pub closed: bool,
	pub deleted: bool,
}

/// Every account's balance at the end of `on`, in the budget's account order.
/// Open, closed, on- and off-budget accounts are all included.
pub fn account_balances(
	budget: &BudgetData,
	classification: &Classification,
	on: CivilDate,
) -> Result<Vec<AccountBalance>, CoreError> {
	let sums = sum_by_account(classification.through(on))?;
	Ok(describe(budget, &sums))
}

/// Assets minus liabilities: the sum of every account's balance.
pub fn net_worth(classification: &Classification, on: CivilDate) -> Result<Money, CoreError> {
	Money::checked_sum(classification.through(on).iter().map(|row| row.amount))
}

pub(crate) fn sum_by_account(
	rows: &[ClassifiedRow],
) -> Result<BTreeMap<AccountId, Money>, CoreError> {
	let mut sums = BTreeMap::new();
	for row in rows {
		add_to(&mut sums, row)?;
	}
	Ok(sums)
}

pub(crate) fn add_to(
	sums: &mut BTreeMap<AccountId, Money>,
	row: &ClassifiedRow,
) -> Result<(), CoreError> {
	let sum = sums.entry(row.account.clone()).or_insert(Money::ZERO);
	*sum = sum.checked_add(row.amount)?;
	Ok(())
}

pub(crate) fn describe(
	budget: &BudgetData,
	sums: &BTreeMap<AccountId, Money>,
) -> Vec<AccountBalance> {
	let mut accounts: Vec<&Account> = budget.accounts.values().collect();
	accounts.sort_by(|a, b| account_order(a, b));
	accounts
		.into_iter()
		.map(|account| {
			let balance = sums.get(&account.id).copied().unwrap_or(Money::ZERO);
			let stored = budget
				.account_meta
				.get(&account.id)
				.and_then(|meta| meta.kind);
			let side = match balance.minor().signum() {
				1 => Side::Asset,
				-1 => Side::Liability,
				_ if stored == Some(AccountKind::Liability) => Side::Liability,
				_ => Side::Asset,
			};
			AccountBalance {
				account: account.id.clone(),
				group: account.group.clone(),
				balance,
				side,
				on_budget: !account.off_budget,
				closed: account.closed,
				deleted: account.tombstone,
			}
		})
		.collect()
}

fn account_order(a: &Account, b: &Account) -> Ordering {
	a.sort_order
		.total_cmp(&b.sort_order)
		.then_with(|| a.id.cmp(&b.id))
}

/// Total assets and total liabilities, liabilities as a positive amount.
pub(crate) fn totals(balances: &[AccountBalance]) -> Result<(Money, Money), CoreError> {
	let mut assets = Money::ZERO;
	let mut owed = Money::ZERO;
	for line in balances {
		match line.side {
			Side::Asset => assets = assets.checked_add(line.balance)?,
			Side::Liability => owed = owed.checked_add(line.balance)?,
		}
	}
	Ok((assets, owed.checked_neg()?))
}
