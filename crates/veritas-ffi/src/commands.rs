use veritas_domain::{Account, AccountId, BudgetData, CategoryId, CoreError, Note, Row};

use crate::Month;
use crate::budget::{edit, envelope, money};

/// A user action. Each one is a single transaction and a single undo step.
/// Workstreams add commands through `contract` PRs.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum Command {
	RenameAccount {
		account_id: String,
		name: String,
	},
	/// Sets an envelope's budgeted amount for a month (R-BUD-1).
	SetBudget {
		month: Month,
		category_id: String,
		amount: i64,
	},
	/// Moves budget between envelopes, or to or from To Budget when an id is
	/// `None` (R-BUD-4). It's never a transaction.
	MoveBudget {
		month: Month,
		amount: i64,
		from_category_id: Option<String>,
		to_category_id: Option<String>,
	},
	/// Covers an overspent envelope from another, or from To Budget when
	/// `from_category_id` is `None`. `amount` `None` covers all of it (R-BUD-4).
	CoverOverspending {
		month: Month,
		category_id: String,
		from_category_id: Option<String>,
		amount: Option<i64>,
	},
	/// Holds part of To Budget for next month, clamped as Actual clamps it.
	HoldForNextMonth {
		month: Month,
		amount: i64,
	},
	ResetHold {
		month: Month,
	},
	/// Rolls an overspent balance into next month instead of To Budget, from
	/// `month` on.
	SetCarryover {
		month: Month,
		category_id: String,
		carryover: bool,
	},
	/// A category's note (R-BUD-6). Blank text removes it.
	SetCategoryNote {
		category_id: String,
		text: String,
	},
}

impl veritas_session::Command for Command {
	fn plan(&self, data: &BudgetData) -> Result<Vec<Row>, CoreError> {
		match self {
			Command::RenameAccount { account_id, name } => rename_account(data, account_id, name),
			Command::SetBudget {
				month,
				category_id,
				amount,
			} => edit(data, *month, |editor, month| {
				editor.set_budget(month, &live_category(data, category_id)?, money(*amount))
			}),
			Command::MoveBudget {
				month,
				amount,
				from_category_id,
				to_category_id,
			} => edit(data, *month, |editor, month| match from_category_id {
				Some(from) => editor.move_budget(
					month,
					money(*amount),
					&live_category(data, from)?,
					&envelope(to_category_id),
				),
				None => match to_category_id {
					Some(to) => {
						editor.fund_from_to_budget(month, money(*amount), &live_category(data, to)?)
					}
					None => Err(CoreError::Invalid(
						"Choose an envelope to move budget to or from.".to_owned(),
					)),
				},
			}),
			Command::CoverOverspending {
				month,
				category_id,
				from_category_id,
				amount,
			} => edit(data, *month, |editor, month| {
				editor.cover_overspending(
					month,
					&live_category(data, category_id)?,
					&envelope(from_category_id),
					amount.map(money),
				)
			}),
			Command::HoldForNextMonth { month, amount } => edit(data, *month, |editor, month| {
				editor
					.hold_for_next_month(month, money(*amount))?
					.ok_or_else(|| {
						CoreError::Invalid("There's nothing in To Budget to hold.".to_owned())
					})
			}),
			Command::ResetHold { month } => {
				edit(data, *month, |editor, month| Ok(editor.reset_hold(month)))
			}
			Command::SetCarryover {
				month,
				category_id,
				carryover,
			} => edit(data, *month, |editor, month| {
				editor.set_carryover(month, &live_category(data, category_id)?, *carryover)
			}),
			Command::SetCategoryNote { category_id, text } => {
				let id = live_category(data, category_id)?;
				let text = text.trim();
				Ok(vec![Row::Note(Note {
					id: id.to_string(),
					text: (!text.is_empty()).then(|| text.to_owned()),
				})])
			}
		}
	}
}

fn live_category(data: &BudgetData, id: &str) -> Result<CategoryId, CoreError> {
	let id = CategoryId::new(id);
	match data.categories.get(&id) {
		Some(category) if !category.tombstone => Ok(id),
		_ => Err(CoreError::NotFound {
			entity: "category",
			id: id.to_string(),
		}),
	}
}

/// Mirrors Actual's `validateAccountName`: trimmed, not blank, and not the
/// name of another open account. Only `accounts.name` changes; transfer
/// payees take their name from the account
/// (desktop-client/src/components/util/accountValidation.ts:4-22,
/// docs/actual-format.md, account rename).
fn rename_account(data: &BudgetData, account_id: &str, name: &str) -> Result<Vec<Row>, CoreError> {
	let id = AccountId::new(account_id);
	let account = data
		.accounts
		.get(&id)
		.filter(|account| !account.tombstone)
		.ok_or_else(|| CoreError::NotFound {
			entity: "account",
			id: account_id.to_owned(),
		})?;
	let name = name.trim();
	if name.is_empty() {
		return Err(CoreError::Invalid(
			"Enter a name for the account.".to_owned(),
		));
	}
	let taken = data
		.accounts
		.values()
		.any(|other| !other.tombstone && other.id != id && other.name == name);
	if taken {
		return Err(CoreError::Invalid(
			"Another account already has that name. Choose a different one.".to_owned(),
		));
	}
	Ok(vec![Row::Account(Account {
		name: name.to_owned(),
		..account.clone()
	})])
}
