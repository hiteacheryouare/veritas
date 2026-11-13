//! Accounts and account groups as Actual keeps them (R-ACC-1,
//! docs/actual-format.md §4.6, §4.8, §4.9).

use veritas_domain::{
	Account, AccountGroup, AccountGroupId, AccountId, BudgetData, CategoryId, CivilDate, CoreError,
	Money, NewAccount, Note, Row, SplitRole,
};

use crate::categories::shove;
use crate::transactions::{self, Draft, alive};

const SORT_INCREMENT: f64 = 16384.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Balances {
	pub cleared: Money,
	pub uncleared: Money,
	pub total: Money,
}

/// Cleared, uncleared and total over live rows, split parents left out, as
/// Actual's balance queries count them (accounts/app.ts:142-170).
pub fn balances(data: &BudgetData, account: &AccountId) -> Result<Balances, CoreError> {
	let mut balances = Balances::default();
	for row in data.transactions.values() {
		if &row.account != account || row.split == SplitRole::Parent || !alive(data, row) {
			continue;
		}
		balances.total = balances.total.checked_add(row.amount)?;
		if row.cleared {
			balances.cleared = balances.cleared.checked_add(row.amount)?;
		} else {
			balances.uncleared = balances.uncleared.checked_add(row.amount)?;
		}
	}
	Ok(balances)
}

pub(crate) fn live_account<'a>(
	data: &'a BudgetData,
	id: &AccountId,
) -> Result<&'a Account, CoreError> {
	data.accounts
		.get(id)
		.filter(|account| !account.tombstone)
		.ok_or_else(|| CoreError::NotFound {
			entity: "account",
			id: id.to_string(),
		})
}

/// Actual's `validateAccountName`: trimmed, not blank, and not the name of
/// another open account.
fn account_name(
	data: &BudgetData,
	text: &str,
	except: Option<&AccountId>,
) -> Result<String, CoreError> {
	let name = text.trim();
	if name.is_empty() {
		return Err(CoreError::Invalid(
			"Enter a name for the account.".to_owned(),
		));
	}
	let taken = data.accounts.values().any(|other| {
		!other.tombstone && !other.closed && Some(&other.id) != except && other.name == name
	});
	if taken {
		return Err(CoreError::Invalid(
			"Another account already has that name. Choose a different one.".to_owned(),
		));
	}
	Ok(name.to_owned())
}

/// `createAccount` (§4.8): the account at the end of its on- or off-budget
/// list, its transfer payee, and a starting-balance row for a non-zero balance.
pub fn create_account(
	data: &BudgetData,
	new: &NewAccount,
	today: CivilDate,
	now_ms: f64,
) -> Result<(Vec<Row>, AccountId), CoreError> {
	let name = account_name(data, &new.name, None)?;
	let new = NewAccount {
		name,
		..new.clone()
	};
	let rows = veritas_store::plan_new_account(data, &new, today, now_ms)?;
	let id = rows
		.iter()
		.find_map(|row| match row {
			Row::Account(account) => Some(account.id.clone()),
			_ => None,
		})
		.ok_or_else(|| CoreError::Internal("a new account had no account row".to_owned()))?;
	Ok((rows, id))
}

/// Moves an account before `before`, or to the end, among the accounts it is
/// listed with: closed ones, or open ones on the same side of the budget
/// (db/index.ts:766-800).
pub fn move_account(
	data: &BudgetData,
	id: &AccountId,
	before: Option<&AccountId>,
) -> Result<Vec<Row>, CoreError> {
	let account = live_account(data, id)?;
	let mut peers: Vec<(AccountId, f64)> = data
		.accounts
		.values()
		.filter(|other| !other.tombstone && other.id != *id)
		.filter(|other| {
			if account.closed {
				other.closed
			} else {
				!other.closed && other.off_budget == account.off_budget
			}
		})
		.map(|other| (other.id.clone(), other.sort_order))
		.collect();
	peers.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
	let (shoved, order) = shove(&peers, before);
	let mut rows = Vec::new();
	for (other, order) in shoved {
		if let Some(other) = data.accounts.get(other) {
			rows.push(Row::Account(Account {
				sort_order: order,
				..other.clone()
			}));
		}
	}
	rows.push(Row::Account(Account {
		sort_order: order,
		..account.clone()
	}));
	Ok(rows)
}

/// Actual's API writes the flag and nothing else: no category or transaction
/// changes (§4.2, "An account flipping between on and off budget").
pub fn set_off_budget(
	data: &BudgetData,
	id: &AccountId,
	off_budget: bool,
) -> Result<Vec<Row>, CoreError> {
	let account = live_account(data, id)?;
	Ok(vec![Row::Account(Account {
		off_budget,
		..account.clone()
	})])
}

pub fn set_account_group(
	data: &BudgetData,
	id: &AccountId,
	group: Option<&AccountGroupId>,
) -> Result<Vec<Row>, CoreError> {
	let account = live_account(data, id)?;
	if let Some(group) = group {
		live_group(data, group)?;
	}
	Ok(vec![Row::Account(Account {
		group: group.cloned(),
		..account.clone()
	})])
}

/// Notes on an account live in `notes` as `account-<id>`. Blank text
/// removes the note.
pub fn set_account_note(
	data: &BudgetData,
	id: &AccountId,
	text: &str,
) -> Result<Vec<Row>, CoreError> {
	live_account(data, id)?;
	let text = text.trim();
	Ok(vec![Row::Note(Note {
		id: note_id(id),
		text: (!text.is_empty()).then(|| text.to_owned()),
	})])
}

pub fn note_id(id: &AccountId) -> String {
	format!("account-{id}")
}

fn live_group<'a>(
	data: &'a BudgetData,
	id: &AccountGroupId,
) -> Result<&'a AccountGroup, CoreError> {
	data.account_groups
		.get(id)
		.filter(|group| !group.tombstone)
		.ok_or_else(|| CoreError::NotFound {
			entity: "account group",
			id: id.to_string(),
		})
}

fn group_name(text: &str) -> Result<String, CoreError> {
	let name = text.trim();
	if name.is_empty() {
		return Err(CoreError::Invalid("Enter a name for the group.".to_owned()));
	}
	Ok(name.to_owned())
}

/// A new group goes at `max(live sort_order) + 16384` (db/index.ts:819-823).
pub fn create_account_group(
	data: &BudgetData,
	name: &str,
) -> Result<(Vec<Row>, AccountGroupId), CoreError> {
	let name = group_name(name)?;
	let last = data
		.account_groups
		.values()
		.filter(|group| !group.tombstone)
		.map(|group| group.sort_order)
		.fold(0.0, f64::max);
	let id = AccountGroupId::new(veritas_store::new_id());
	Ok((
		vec![Row::AccountGroup(AccountGroup {
			id: id.clone(),
			name,
			sort_order: last + SORT_INCREMENT,
			tombstone: false,
		})],
		id,
	))
}

pub fn rename_account_group(
	data: &BudgetData,
	id: &AccountGroupId,
	name: &str,
) -> Result<Vec<Row>, CoreError> {
	let group = live_group(data, id)?;
	Ok(vec![Row::AccountGroup(AccountGroup {
		name: group_name(name)?,
		..group.clone()
	})])
}

pub fn move_account_group(
	data: &BudgetData,
	id: &AccountGroupId,
	before: Option<&AccountGroupId>,
) -> Result<Vec<Row>, CoreError> {
	let group = live_group(data, id)?;
	let mut peers: Vec<(AccountGroupId, f64)> = data
		.account_groups
		.values()
		.filter(|other| !other.tombstone && other.id != *id)
		.map(|other| (other.id.clone(), other.sort_order))
		.collect();
	peers.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
	let (shoved, order) = shove(&peers, before);
	let mut rows = Vec::new();
	for (other, order) in shoved {
		if let Some(other) = data.account_groups.get(other) {
			rows.push(Row::AccountGroup(AccountGroup {
				sort_order: order,
				..other.clone()
			}));
		}
	}
	rows.push(Row::AccountGroup(AccountGroup {
		sort_order: order,
		..group.clone()
	}));
	Ok(rows)
}

/// Tombstones the group; its accounts leave it.
pub fn delete_account_group(data: &BudgetData, id: &AccountGroupId) -> Result<Vec<Row>, CoreError> {
	let group = live_group(data, id)?;
	let mut rows: Vec<Row> = data
		.accounts
		.values()
		.filter(|account| account.group.as_ref() == Some(id))
		.map(|account| {
			Row::Account(Account {
				group: None,
				..account.clone()
			})
		})
		.collect();
	rows.push(Row::AccountGroup(AccountGroup {
		tombstone: true,
		..group.clone()
	}));
	Ok(rows)
}

/// `account-close` without `forced` (§4.9). An account with no live rows is
/// deleted instead. Otherwise it is closed, and a non-zero balance moves to
/// `transfer_to` as a "Closing account" transfer, with `category` on this
/// side when the money leaves the budget.
pub fn close_account(
	data: &BudgetData,
	id: &AccountId,
	transfer_to: Option<&AccountId>,
	category: Option<&CategoryId>,
	today: CivilDate,
	now_ms: f64,
) -> Result<Vec<Row>, CoreError> {
	let account = live_account(data, id)?;
	if account.closed {
		return Err(CoreError::Invalid(
			"This account is already closed.".to_owned(),
		));
	}
	let has_rows = data
		.transactions
		.values()
		.any(|row| &row.account == id && !row.tombstone);
	if !has_rows {
		return Ok(vec![Row::Account(Account {
			tombstone: true,
			..account.clone()
		})]);
	}
	let closed = Account {
		closed: true,
		..account.clone()
	};
	let mut rows = vec![Row::Account(closed)];
	let balance = balances(data, id)?.total;
	if balance == Money::ZERO {
		return Ok(rows);
	}
	let Some(to) = transfer_to else {
		return Err(CoreError::Invalid(
			"This account still has money in it. Choose an account to move it to.".to_owned(),
		));
	};
	if to == id {
		return Err(CoreError::Invalid(
			"Choose a different account to move the balance to.".to_owned(),
		));
	}
	live_account(data, to)?;
	let payee = transactions::transfer_payee(data, to).ok_or_else(|| CoreError::NotFound {
		entity: "transfer payee",
		id: to.to_string(),
	})?;
	let with_closed = crate::writes::applied(data, &rows);
	let draft = Draft {
		id: None,
		account: id.clone(),
		date: today,
		amount: balance.checked_neg()?,
		payee: Some(payee),
		category: category.cloned(),
		notes: Some("Closing account".to_owned()),
		cleared: true,
		splits: Vec::new(),
	};
	let added = transactions::insert(&with_closed, &draft, None, now_ms)?;
	rows.extend(added.rows);
	Ok(rows)
}

/// Reopening writes only `closed = 0` (accounts/app.ts:708-710).
pub fn reopen_account(data: &BudgetData, id: &AccountId) -> Result<Vec<Row>, CoreError> {
	let account = live_account(data, id)?;
	if !account.closed {
		return Err(CoreError::Invalid("This account isn't closed.".to_owned()));
	}
	Ok(vec![Row::Account(Account {
		closed: false,
		..account.clone()
	})])
}
