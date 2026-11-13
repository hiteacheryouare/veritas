use veritas_domain::{
	AccountId, BudgetData, CategoryId, CoreError, Money, PayeeId, Row, TransactionId,
};
use veritas_session::register::{self, Filter, Scope, Sort, SortKey, Status};
use veritas_session::transactions::{Draft, Line};
use veritas_session::{accounts, payees, transactions};

use crate::budget::civil_date;

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AccountDetail {
	pub id: String,
	pub name: String,
	pub off_budget: bool,
	pub closed: bool,
	pub group_id: Option<String>,
	pub cleared: i64,
	pub uncleared: i64,
	pub total: i64,
	pub note: Option<String>,
	/// Epoch milliseconds as Actual stores them, when a reconcile last finished.
	pub last_reconciled: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AccountGroupDetail {
	pub id: String,
	pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AccountList {
	/// Open accounts on budget, then off budget, then closed, each in Actual's order.
	pub accounts: Vec<AccountDetail>,
	pub groups: Vec<AccountGroupDetail>,
}

/// What the register shows: one account, or every on-budget, off-budget or
/// open account.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum RegisterScope {
	Account { account_id: String },
	OnBudget,
	OffBudget,
	All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ClearedStatus {
	Uncleared,
	Cleared,
	Reconciled,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, uniffi::Record)]
pub struct RegisterFilter {
	pub search: Option<String>,
	pub status: Option<ClearedStatus>,
	/// `YYYY-MM-DD`, inclusive.
	pub from: Option<String>,
	pub to: Option<String>,
	pub category_id: Option<String>,
	pub payee_id: Option<String>,
	pub uncategorized: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum RegisterSortKey {
	Date,
	Account,
	Payee,
	Category,
	Notes,
	Amount,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct RegisterSort {
	pub key: RegisterSortKey,
	pub ascending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RegisterLine {
	pub id: String,
	pub amount: i64,
	pub payee_id: Option<String>,
	pub payee_name: Option<String>,
	pub category_id: Option<String>,
	pub category_name: Option<String>,
	pub notes: Option<String>,
	pub transfer_account_id: Option<String>,
	pub counterpart_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RegisterRow {
	pub id: String,
	pub account_id: String,
	pub account_name: String,
	pub date: String,
	pub amount: i64,
	pub payee_id: Option<String>,
	pub payee_name: Option<String>,
	pub category_id: Option<String>,
	pub category_name: Option<String>,
	pub notes: Option<String>,
	pub tags: Vec<String>,
	pub cleared: bool,
	pub reconciled: bool,
	pub transfer_account_id: Option<String>,
	pub counterpart_id: Option<String>,
	pub lines: Vec<RegisterLine>,
	pub split_difference: Option<i64>,
	pub starting_balance: bool,
	pub schedule_id: Option<String>,
	pub needs_actual: Option<String>,
	pub running_balance: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RegisterPage {
	pub total: u64,
	pub rows: Vec<RegisterRow>,
}

/// A transaction as the register or quick entry edits it. A payee is
/// `payeeId`, or `payeeName`, reused case-insensitively or created.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TransactionDraft {
	pub account_id: String,
	pub date: String,
	pub amount: i64,
	pub payee_id: Option<String>,
	pub payee_name: Option<String>,
	pub category_id: Option<String>,
	pub notes: Option<String>,
	pub cleared: bool,
	/// Empty for a plain transaction. Lines must add up to `amount`.
	pub splits: Vec<SplitLineDraft>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SplitLineDraft {
	/// An existing line's id when editing, so it keeps its row; `nil` is new.
	pub id: Option<String>,
	pub amount: i64,
	pub payee_id: Option<String>,
	pub payee_name: Option<String>,
	pub category_id: Option<String>,
	pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PayeeDetail {
	pub id: String,
	/// An account's name for a transfer payee.
	pub name: String,
	pub transfer_account_id: Option<String>,
	pub favorite: bool,
	pub learn_categories: bool,
	pub transactions: u64,
}

/// D38: quick entry's fill for a known payee.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PayeeMemory {
	pub category_id: Option<String>,
	pub amount: i64,
	pub date: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct Reconciliation {
	pub cleared: i64,
	/// Statement minus cleared. Zero means Finish locks the cleared rows.
	pub difference: i64,
}

pub(crate) fn account_list(data: &BudgetData) -> Result<AccountList, CoreError> {
	let mut live: Vec<_> = data.accounts.values().filter(|a| !a.tombstone).collect();
	live.sort_by(|a, b| {
		(a.closed, a.off_budget)
			.cmp(&(b.closed, b.off_budget))
			.then(a.sort_order.total_cmp(&b.sort_order))
			.then_with(|| a.id.cmp(&b.id))
	});
	let mut accounts = Vec::new();
	for account in live {
		let balances = accounts::balances(data, &account.id)?;
		accounts.push(AccountDetail {
			id: account.id.to_string(),
			name: account.name.clone(),
			off_budget: account.off_budget,
			closed: account.closed,
			group_id: account.group.as_ref().map(ToString::to_string),
			cleared: balances.cleared.minor(),
			uncleared: balances.uncleared.minor(),
			total: balances.total.minor(),
			note: data
				.notes
				.get(&accounts::note_id(&account.id))
				.cloned()
				.flatten(),
			last_reconciled: account.last_reconciled.clone(),
		});
	}
	let mut groups: Vec<_> = data
		.account_groups
		.values()
		.filter(|g| !g.tombstone)
		.collect();
	groups.sort_by(|a, b| {
		a.sort_order
			.total_cmp(&b.sort_order)
			.then_with(|| a.id.cmp(&b.id))
	});
	Ok(AccountList {
		accounts,
		groups: groups
			.into_iter()
			.map(|g| AccountGroupDetail {
				id: g.id.to_string(),
				name: g.name.clone(),
			})
			.collect(),
	})
}

fn date(text: &Option<String>) -> Result<Option<veritas_domain::CivilDate>, CoreError> {
	text.as_deref().map(civil_date).transpose()
}

pub(crate) fn register_page(
	data: &BudgetData,
	scope: &RegisterScope,
	filter: &RegisterFilter,
	sort: RegisterSort,
	offset: u64,
	limit: u64,
) -> Result<RegisterPage, CoreError> {
	let scope = match scope {
		RegisterScope::Account { account_id } => {
			Scope::Account(AccountId::new(account_id.as_str()))
		}
		RegisterScope::OnBudget => Scope::OnBudget,
		RegisterScope::OffBudget => Scope::OffBudget,
		RegisterScope::All => Scope::All,
	};
	let filter = Filter {
		search: filter.search.clone(),
		status: filter.status.map(|status| match status {
			ClearedStatus::Uncleared => Status::Uncleared,
			ClearedStatus::Cleared => Status::Cleared,
			ClearedStatus::Reconciled => Status::Reconciled,
		}),
		from: date(&filter.from)?,
		to: date(&filter.to)?,
		category: filter.category_id.as_deref().map(CategoryId::new),
		payee: filter.payee_id.as_deref().map(PayeeId::new),
		uncategorized: filter.uncategorized,
	};
	let sort = Sort {
		key: match sort.key {
			RegisterSortKey::Date => SortKey::Date,
			RegisterSortKey::Account => SortKey::Account,
			RegisterSortKey::Payee => SortKey::Payee,
			RegisterSortKey::Category => SortKey::Category,
			RegisterSortKey::Notes => SortKey::Notes,
			RegisterSortKey::Amount => SortKey::Amount,
		},
		ascending: sort.ascending,
	};
	let offset = usize::try_from(offset).unwrap_or(usize::MAX);
	let limit = usize::try_from(limit).unwrap_or(usize::MAX);
	let page = register::page(data, &scope, &filter, sort, offset, limit)?;
	Ok(RegisterPage {
		total: page.total as u64,
		rows: page.rows.into_iter().map(RegisterRow::from).collect(),
	})
}

pub(crate) fn register_row(data: &BudgetData, id: &str) -> Result<RegisterRow, CoreError> {
	register::row(data, &TransactionId::new(id)).map(RegisterRow::from)
}

fn text(id: &Option<impl ToString>) -> Option<String> {
	id.as_ref().map(ToString::to_string)
}

impl From<register::Line> for RegisterLine {
	fn from(line: register::Line) -> Self {
		RegisterLine {
			id: line.id.to_string(),
			amount: line.amount.minor(),
			payee_id: text(&line.payee),
			payee_name: line.payee_name,
			category_id: text(&line.category),
			category_name: line.category_name,
			notes: line.notes,
			transfer_account_id: text(&line.transfer_account),
			counterpart_id: text(&line.counterpart),
		}
	}
}

impl From<register::RegisterRow> for RegisterRow {
	fn from(row: register::RegisterRow) -> Self {
		RegisterRow {
			id: row.id.to_string(),
			account_id: row.account.to_string(),
			account_name: row.account_name,
			date: row.date.to_string(),
			amount: row.amount.minor(),
			payee_id: text(&row.payee),
			payee_name: row.payee_name,
			category_id: text(&row.category),
			category_name: row.category_name,
			notes: row.notes,
			tags: row.tags,
			cleared: row.cleared,
			reconciled: row.reconciled,
			transfer_account_id: text(&row.transfer_account),
			counterpart_id: text(&row.counterpart),
			lines: row.lines.into_iter().map(RegisterLine::from).collect(),
			split_difference: row.split_difference.map(Money::minor),
			starting_balance: row.starting_balance,
			schedule_id: text(&row.schedule),
			needs_actual: row.needs_actual,
			running_balance: row.running_balance.map(Money::minor),
		}
	}
}

/// The session's draft, and the rows for any payee named but not yet made.
pub(crate) fn draft(
	data: &BudgetData,
	input: &TransactionDraft,
) -> Result<(Vec<Row>, Draft), CoreError> {
	let mut rows = Vec::new();
	let mut payee = |id: &Option<String>,
	                 name: &Option<String>,
	                 data: &BudgetData|
	 -> Result<Option<PayeeId>, CoreError> {
		match (id, name.as_deref().map(str::trim).filter(|n| !n.is_empty())) {
			(Some(id), _) => Ok(Some(PayeeId::new(id.as_str()))),
			(None, Some(name)) => {
				let with = veritas_session::writes::applied(data, &rows);
				let (made, id) = transactions::payee_named(&with, name)?;
				rows.extend(made);
				Ok(Some(id))
			}
			(None, None) => Ok(None),
		}
	};
	let top = payee(&input.payee_id, &input.payee_name, data)?;
	let mut splits = Vec::new();
	for line in &input.splits {
		splits.push(Line {
			id: line.id.as_deref().map(TransactionId::new),
			amount: Money::from_minor(line.amount),
			payee: payee(&line.payee_id, &line.payee_name, data)?,
			category: line.category_id.as_deref().map(CategoryId::new),
			notes: line.notes.clone(),
		});
	}
	Ok((
		rows,
		Draft {
			account: AccountId::new(input.account_id.as_str()),
			date: civil_date(&input.date)?,
			amount: Money::from_minor(input.amount),
			payee: top,
			category: input.category_id.as_deref().map(CategoryId::new),
			notes: input.notes.clone(),
			cleared: input.cleared,
			splits,
		},
	))
}

pub(crate) fn payee_list(data: &BudgetData) -> Vec<PayeeDetail> {
	payees::payees(data)
		.into_iter()
		.map(|summary| PayeeDetail {
			id: summary.payee.id.to_string(),
			name: summary.account_name.unwrap_or(summary.payee.name),
			transfer_account_id: text(&summary.payee.transfer_account),
			favorite: summary.payee.favorite,
			learn_categories: summary.payee.learn_categories,
			transactions: summary.transactions as u64,
		})
		.collect()
}

pub(crate) fn payee_memory(data: &BudgetData, payee: &str) -> Option<PayeeMemory> {
	payees::payee_memory(data, &PayeeId::new(payee)).map(|memory| PayeeMemory {
		category_id: text(&memory.category),
		amount: memory.amount.minor(),
		date: memory.date.to_string(),
	})
}

pub(crate) fn reconciliation(
	data: &BudgetData,
	account: &str,
	statement: i64,
) -> Result<Reconciliation, CoreError> {
	let state =
		payees::reconciliation(data, &AccountId::new(account), Money::from_minor(statement))?;
	Ok(Reconciliation {
		cleared: state.cleared.minor(),
		difference: state.difference.minor(),
	})
}
