use std::collections::BTreeSet;

use veritas_domain::{AccountId, AccountKind, BudgetData, CivilDate, CoreError, Money};
use veritas_reports::{account_balances, classify};

use veritas_domain::NumberFormat;

/// docs/finance/statements.md ST-9.2, in the order the sidebar lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AccountGroupKind {
	Cash,
	Credit,
	Investments,
	Loans,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverviewAccount {
	pub id: AccountId,
	pub name: String,
	/// Cents as of today, Actual's sign: liabilities are negative (ST-9.6).
	pub balance: Money,
	pub closed: bool,
	/// Tombstoned but still carrying live rows (ST-9.4).
	pub deleted: bool,
	/// Placed only by its negative balance, so the shell offers "Set account
	/// type" (ST-9.5).
	pub placed_by_balance: bool,
}

/// What the shell shows when a budget opens: its name and its accounts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Overview {
	pub budget_name: Option<String>,
	/// `None` until veritas-store loads Actual's preferences; the shell then
	/// uses Actual's defaults.
	pub number_format: Option<NumberFormat>,
	pub groups: Vec<OverviewGroup>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverviewGroup {
	pub kind: AccountGroupKind,
	pub accounts: Vec<OverviewAccount>,
	pub total: Money,
}

/// The accounts sidebar: every group in ST-9.2 order, including empty ones,
/// with accounts in the budget's own order.
pub fn account_groups(
	data: &BudgetData,
	today: CivilDate,
) -> Result<Vec<OverviewGroup>, CoreError> {
	let classification = classify(data);
	let with_rows: BTreeSet<&AccountId> = classification
		.rows()
		.iter()
		.map(|row| &row.account)
		.collect();
	let mut groups: Vec<OverviewGroup> = [
		AccountGroupKind::Cash,
		AccountGroupKind::Credit,
		AccountGroupKind::Investments,
		AccountGroupKind::Loans,
	]
	.into_iter()
	.map(|kind| OverviewGroup {
		kind,
		accounts: Vec::new(),
		total: Money::ZERO,
	})
	.collect();

	for line in account_balances(data, &classification, today)? {
		let Some(account) = data.accounts.get(&line.account) else {
			continue;
		};
		if account.tombstone && !with_rows.contains(&account.id) {
			continue;
		}
		let (liability, placed_by_balance) = placement(data, &account.id, line.balance);
		let kind = match (account.off_budget, liability) {
			(false, false) => AccountGroupKind::Cash,
			(false, true) => AccountGroupKind::Credit,
			(true, false) => AccountGroupKind::Investments,
			(true, true) => AccountGroupKind::Loans,
		};
		let Some(group) = groups.iter_mut().find(|group| group.kind == kind) else {
			continue;
		};
		group.total = group.total.checked_add(line.balance)?;
		group.accounts.push(OverviewAccount {
			id: account.id.clone(),
			name: account.name.clone(),
			balance: line.balance,
			closed: account.closed,
			deleted: account.tombstone,
			placed_by_balance,
		});
	}
	Ok(groups)
}

/// ST-9.1: explicit kind, then loan fields, then a negative balance. Returns
/// whether the account is a liability, and whether only the balance said so.
fn placement(data: &BudgetData, id: &AccountId, balance: Money) -> (bool, bool) {
	let meta = data.account_meta.get(id);
	match meta.and_then(|meta| meta.kind) {
		Some(AccountKind::Liability) => return (true, false),
		Some(AccountKind::Asset) => return (false, false),
		None => {}
	}
	let has_loan = meta.is_some_and(|meta| {
		meta.loan_rate_bps.is_some() || meta.loan_payment.is_some() || meta.lender.is_some()
	});
	if has_loan {
		(true, false)
	} else if balance.is_negative() {
		(true, true)
	} else {
		(false, false)
	}
}

#[cfg(test)]
mod tests {
	use veritas_domain::{
		Account, AccountMeta, BudgetType, CategoryId, SplitRole, Transaction, TransactionId,
	};

	use super::*;

	struct Case {
		name: &'static str,
		off_budget: bool,
		kind: Option<AccountKind>,
		balance: i64,
		loan_rate_bps: Option<i64>,
		lender: bool,
		closed: bool,
	}

	fn today() -> CivilDate {
		CivilDate::new(2026, 11, 10).unwrap()
	}

	fn budget(cases: &[Case]) -> BudgetData {
		let mut data = BudgetData::empty(BudgetType::Envelope);
		for (index, case) in cases.iter().enumerate() {
			let id = AccountId::new(case.name);
			data.accounts.insert(
				id.clone(),
				Account {
					id: id.clone(),
					name: case.name.to_owned(),
					off_budget: case.off_budget,
					closed: case.closed,
					group: None,
					sort_order: index as f64,
					tombstone: false,
				},
			);
			data.account_meta.insert(
				id.clone(),
				AccountMeta {
					kind: case.kind,
					loan_rate_bps: case.loan_rate_bps,
					lender: case.lender.then(|| "Lender".to_owned()),
					..AccountMeta::default()
				},
			);
			if case.balance != 0 {
				let transaction = TransactionId::new(format!("{}-opening", case.name));
				data.transactions.insert(
					transaction.clone(),
					Transaction {
						id: transaction,
						account: id,
						date: CivilDate::new(2026, 10, 1).unwrap(),
						amount: Money::from_minor(case.balance),
						payee: None,
						imported_payee: None,
						category: None::<CategoryId>,
						notes: None,
						cleared: true,
						reconciled: false,
						split: SplitRole::None,
						transfer: None,
						schedule: None,
						starting_balance: true,
						sort_order: 0.0,
						tombstone: false,
					},
				);
			}
		}
		data
	}

	fn case(name: &'static str, off_budget: bool, kind: Option<AccountKind>, balance: i64) -> Case {
		Case {
			name,
			off_budget,
			kind,
			balance,
			loan_rate_bps: None,
			lender: false,
			closed: false,
		}
	}

	#[test]
	fn the_sidebar_groups_follow_st_9_examples() {
		// docs/finance/statements.md ST-9, examples G-1 to G-10.
		use AccountKind::{Asset, Liability};
		let cases = [
			case("G-1", false, None, 120_000),
			case("G-2", false, Some(Liability), -45_000),
			case("G-3", false, Some(Liability), 2_000),
			case("G-4", false, None, -1_500),
			case("G-5", false, Some(Asset), -1_500),
			case("G-6", true, None, 900_000),
			Case {
				lender: true,
				..case("G-7", true, Some(Liability), -25_000_000)
			},
			Case {
				loan_rate_bps: Some(600),
				..case("G-8", false, None, 0)
			},
			Case {
				closed: true,
				..case("G-9", false, Some(Liability), 0)
			},
			case("G-10", false, None, 0),
		];
		let groups = account_groups(&budget(&cases), today()).unwrap();
		let names = |kind: AccountGroupKind| -> Vec<&str> {
			groups
				.iter()
				.find(|group| group.kind == kind)
				.unwrap()
				.accounts
				.iter()
				.map(|account| account.name.as_str())
				.collect()
		};
		assert_eq!(names(AccountGroupKind::Cash), ["G-1", "G-5", "G-10"]);
		assert_eq!(
			names(AccountGroupKind::Credit),
			["G-2", "G-3", "G-4", "G-8", "G-9"]
		);
		assert_eq!(names(AccountGroupKind::Investments), ["G-6"]);
		assert_eq!(names(AccountGroupKind::Loans), ["G-7"]);

		let credit = &groups[1];
		assert_eq!(credit.total, Money::from_minor(-45_000 + 2_000 - 1_500));
		let flagged: Vec<_> = credit
			.accounts
			.iter()
			.filter(|account| account.placed_by_balance)
			.map(|account| account.name.as_str())
			.collect();
		assert_eq!(flagged, ["G-4"]);
		assert!(
			credit
				.accounts
				.iter()
				.any(|account| account.name == "G-9" && account.closed)
		);
	}

	#[test]
	fn a_deleted_account_shows_only_while_it_still_has_live_rows() {
		// ST-9.4
		let mut data = budget(&[
			case("kept", false, None, 5_000),
			case("gone", false, None, 0),
		]);
		for account in data.accounts.values_mut() {
			account.tombstone = true;
		}
		let groups = account_groups(&data, today()).unwrap();
		let cash = &groups[0].accounts;
		assert_eq!(cash.len(), 1);
		assert_eq!(cash[0].name, "kept");
		assert!(cash[0].deleted);
	}
}
