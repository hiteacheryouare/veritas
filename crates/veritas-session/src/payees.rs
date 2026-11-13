//! Payees (R-ACC-5), quick entry's payee memory (R-ACC-4, D38) and
//! reconciling (R-ACC-3), as docs/actual-format.md §4.5 and §4.7 describe.

use veritas_domain::{
	Account, AccountId, BudgetData, CategoryId, CivilDate, CoreError, Money, Payee, PayeeId,
	PayeeMapping, Row, SplitRole, Transaction, TransactionMeta, TransactionMetaRow,
};
use veritas_rules::RuleBook;

use crate::accounts::{balances, live_account};
use crate::transactions::{self, Draft, Planned, alive, children, register_order};

fn live_payee<'a>(data: &'a BudgetData, id: &PayeeId) -> Result<&'a Payee, CoreError> {
	data.payees
		.get(id)
		.filter(|payee| !payee.tombstone)
		.ok_or_else(|| CoreError::NotFound {
			entity: "payee",
			id: id.to_string(),
		})
}

fn ordinary<'a>(data: &'a BudgetData, id: &PayeeId) -> Result<&'a Payee, CoreError> {
	let payee = live_payee(data, id)?;
	if payee.transfer_account.is_some() {
		return Err(CoreError::Invalid(
			"That payee stands for an account. Rename the account instead.".to_owned(),
		));
	}
	Ok(payee)
}

pub fn rename_payee(data: &BudgetData, id: &PayeeId, name: &str) -> Result<Vec<Row>, CoreError> {
	let payee = ordinary(data, id)?;
	let name = name.trim();
	if name.is_empty() {
		return Err(CoreError::Invalid("Enter a payee name.".to_owned()));
	}
	Ok(vec![Row::Payee(Payee {
		name: name.to_owned(),
		..payee.clone()
	})])
}

pub fn set_favorite(
	data: &BudgetData,
	id: &PayeeId,
	favorite: bool,
) -> Result<Vec<Row>, CoreError> {
	let payee = live_payee(data, id)?;
	Ok(vec![Row::Payee(Payee {
		favorite,
		..payee.clone()
	})])
}

pub fn set_learn_categories(
	data: &BudgetData,
	id: &PayeeId,
	learn: bool,
) -> Result<Vec<Row>, CoreError> {
	let payee = live_payee(data, id)?;
	Ok(vec![Row::Payee(Payee {
		learn_categories: learn,
		..payee.clone()
	})])
}

/// `mergePayees` (db/index.ts:618-656): transfer payees take no part. Each
/// source's mapping, and every mapping that pointed at it, now points at the
/// target, and the source is tombstoned. Transactions keep their payee id and
/// read the target through the mapping.
pub fn merge_payees(
	data: &BudgetData,
	target: &PayeeId,
	sources: &[PayeeId],
) -> Result<Vec<Row>, CoreError> {
	let target_payee = ordinary(data, target)?;
	let _ = target_payee;
	let mut rows = Vec::new();
	for source in sources {
		if source == target {
			continue;
		}
		let payee = live_payee(data, source)?;
		if payee.transfer_account.is_some() {
			continue;
		}
		for (from, to) in &data.payee_mapping {
			if to == source && from != source {
				rows.push(Row::PayeeMapping(PayeeMapping {
					from: from.clone(),
					to: target.clone(),
				}));
			}
		}
		rows.push(Row::PayeeMapping(PayeeMapping {
			from: source.clone(),
			to: target.clone(),
		}));
		rows.push(Row::Payee(Payee {
			tombstone: true,
			..payee.clone()
		}));
	}
	Ok(rows)
}

#[derive(Clone, Debug, PartialEq)]
pub struct PayeeSummary {
	pub payee: Payee,
	/// The account a transfer payee stands for.
	pub account_name: Option<String>,
	/// Live rows that read this payee through the mapping.
	pub transactions: usize,
}

/// Every live payee, transfer payees included, by name.
pub fn payees(data: &BudgetData) -> Vec<PayeeSummary> {
	let mut counts: std::collections::BTreeMap<&PayeeId, usize> = Default::default();
	for row in data.transactions.values().filter(|row| alive(data, row)) {
		if let Some(payee) = &row.payee {
			*counts
				.entry(data.resolve_payee(payee).unwrap_or(payee))
				.or_default() += 1;
		}
	}
	let mut list: Vec<PayeeSummary> = data
		.payees
		.values()
		.filter(|payee| !payee.tombstone)
		.filter(|payee| {
			payee
				.transfer_account
				.as_ref()
				.is_none_or(|account| data.accounts.get(account).is_some_and(|a| !a.tombstone))
		})
		.map(|payee| PayeeSummary {
			payee: payee.clone(),
			account_name: payee
				.transfer_account
				.as_ref()
				.and_then(|account| data.accounts.get(account))
				.map(|account| account.name.clone()),
			transactions: counts.get(&payee.id).copied().unwrap_or_default(),
		})
		.collect();
	list.sort_by_key(|summary| {
		summary
			.account_name
			.clone()
			.unwrap_or_else(|| summary.payee.name.clone())
			.to_lowercase()
	});
	list
}

/// D38: what quick entry fills for a payee, from its most recent row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PayeeMemory {
	pub category: Option<CategoryId>,
	pub amount: Money,
	pub date: CivilDate,
}

pub fn payee_memory(data: &BudgetData, payee: &PayeeId) -> Option<PayeeMemory> {
	let payee = data.resolve_payee(payee).unwrap_or(payee);
	data.transactions
		.values()
		.filter(|row| alive(data, row) && !matches!(row.split, SplitRole::Child { .. }))
		.filter(|row| {
			row.payee
				.as_ref()
				.is_some_and(|p| data.resolve_payee(p).unwrap_or(p) == payee)
		})
		.min_by(|a, b| register_order(a, b))
		.map(|row| PayeeMemory {
			category: row
				.category
				.as_ref()
				.filter(|_| row.split != SplitRole::Parent)
				.and_then(|category| data.resolve_category(category))
				.filter(|category| data.categories.get(*category).is_some_and(|c| !c.tombstone))
				.cloned(),
			amount: row.amount,
			date: row.date,
		})
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reconciliation {
	pub cleared: Money,
	/// `statement − cleared`. Zero means the reconcile can lock.
	pub difference: Money,
}

pub fn reconciliation(
	data: &BudgetData,
	account: &AccountId,
	statement: Money,
) -> Result<Reconciliation, CoreError> {
	live_account(data, account)?;
	let cleared = balances(data, account)?.cleared;
	Ok(Reconciliation {
		cleared,
		difference: statement.checked_sub(cleared)?,
	})
}

pub const ADJUSTMENT_NOTE: &str = "Reconciliation balance adjustment";

/// The optional adjustment (reconciliation.ts:95-124): today, cleared, no
/// payee or category, through the rules, and marked as an adjustment (CLS-5).
pub fn reconcile_adjustment(
	data: &BudgetData,
	account: &AccountId,
	statement: Money,
	book: &RuleBook,
	today: CivilDate,
	now_ms: f64,
) -> Result<Planned, CoreError> {
	let difference = reconciliation(data, account, statement)?.difference;
	if difference == Money::ZERO {
		return Err(CoreError::Invalid(
			"The cleared balance already matches the statement.".to_owned(),
		));
	}
	let draft = Draft {
		id: None,
		account: account.clone(),
		date: today,
		amount: difference,
		payee: None,
		category: None,
		notes: Some(ADJUSTMENT_NOTE.to_owned()),
		cleared: true,
		splits: Vec::new(),
	};
	let mut planned = transactions::add(data, &draft, book, now_ms)?;
	planned
		.rows
		.retain(|row| !matches!(row, Row::TransactionMeta(meta) if meta.transaction == planned.id));
	planned.rows.push(Row::TransactionMeta(TransactionMetaRow {
		transaction: planned.id.clone(),
		meta: TransactionMeta {
			adjustment: true,
			needs_actual: planned.note.clone(),
		},
	}));
	Ok(planned)
}

/// Done reconciling (reconciliation.ts:82-93, Account.tsx:1107-1129): when
/// the cleared balance matches the statement, every cleared row not yet
/// reconciled is locked, split lines with it. `last_reconciled` is written
/// either way.
pub fn finish_reconcile(
	data: &BudgetData,
	account: &AccountId,
	statement: Money,
	now_ms: f64,
) -> Result<(Vec<Row>, bool), CoreError> {
	let found = live_account(data, account)?;
	let locks = reconciliation(data, account, statement)?.difference == Money::ZERO;
	let mut rows = Vec::new();
	if locks {
		let to_lock: Vec<&Transaction> = data
			.transactions
			.values()
			.filter(|row| &row.account == account && alive(data, row))
			.filter(|row| !matches!(row.split, SplitRole::Child { .. }))
			.filter(|row| row.cleared && !row.reconciled)
			.collect();
		for row in to_lock {
			rows.push(Row::Transaction(Transaction {
				reconciled: true,
				..row.clone()
			}));
			for line in children(data, &row.id) {
				rows.push(Row::Transaction(Transaction {
					reconciled: true,
					..line.clone()
				}));
			}
		}
	}
	rows.push(Row::Account(Account {
		last_reconciled: Some(format!("{}", now_ms as i64)),
		..found.clone()
	}));
	Ok((rows, locks))
}
