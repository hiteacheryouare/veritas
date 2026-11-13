use veritas_domain::{Account, AccountId, BudgetData, CoreError, Row};

/// A user action. Each one is a single transaction and a single undo step.
/// Workstreams add commands through `contract` PRs.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum Command {
	RenameAccount { account_id: String, name: String },
}

impl veritas_session::Command for Command {
	fn plan(&self, data: &BudgetData) -> Result<Vec<Row>, CoreError> {
		match self {
			Command::RenameAccount { account_id, name } => rename_account(data, account_id, name),
		}
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
