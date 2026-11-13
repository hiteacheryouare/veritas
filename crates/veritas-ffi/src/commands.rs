use veritas_domain::{
	Account, AccountId, BudgetData, CategoryGroupId, CategoryId, CivilDate, CoreError, GoalId,
	Note, Row,
};
use veritas_session::{categories, goals};

use veritas_engine::EnvelopeSheet;

use crate::Month;
use crate::budget::{civil_date, edit, envelope, money};

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
	/// A new category at the top of its group (R-BUD-5). Its id comes back in
	/// the `Change`.
	CreateCategory {
		group_id: String,
		name: String,
	},
	RenameCategory {
		category_id: String,
		name: String,
	},
	/// Moves a category before `before_category_id` in `group_id`, or to the end
	/// of it. Income and expense categories stay among their own kind.
	MoveCategory {
		category_id: String,
		group_id: String,
		before_category_id: Option<String>,
	},
	/// Showing a category again also ends its archive.
	SetCategoryHidden {
		category_id: String,
		hidden: bool,
	},
	/// Hides a category and records when, keeping all its history (R-BUD-5).
	/// What the app offers instead of delete for a category with history.
	ArchiveCategory {
		category_id: String,
	},
	/// Deletes a category with no history (`Session.categoryHistory` is empty).
	/// One with history answers `Invalid`.
	DeleteCategory {
		category_id: String,
	},
	/// Deletes a category and moves all its history and budget to another, as
	/// Actual does. Irreversible in Actual, so the app shows its explicit
	/// confirmation first; in Veritas it is still one undo step.
	DeleteCategoryMovingHistory {
		category_id: String,
		to_category_id: String,
	},
	CreateCategoryGroup {
		name: String,
	},
	RenameCategoryGroup {
		group_id: String,
		name: String,
	},
	/// Moves a group before `before_group_id`, or to the end.
	MoveCategoryGroup {
		group_id: String,
		before_group_id: Option<String>,
	},
	SetCategoryGroupHidden {
		group_id: String,
		hidden: bool,
	},
	/// A goal as a new envelope at the top of the Goals group, created if the
	/// budget has none. `target_date` is `YYYY-MM-DD`. The goal's id comes back
	/// in the `Change` (R-GOAL-1, D36).
	CreateGoal {
		name: String,
		target: i64,
		target_date: Option<String>,
		icon: Option<String>,
		color: Option<String>,
	},
	/// Releases this month's leftover to `release_to_category_id`, or To
	/// Budget when `None`, then closes and hides the goal. `also_move_later`
	/// moves budget in later months to the same place (DEBT-8.3, GOAL-10).
	CloseGoal {
		goal_id: String,
		release_to_category_id: Option<String>,
		also_move_later: bool,
	},
	/// Back to reached if it ever was, else active, and shown again.
	ReopenGoal {
		goal_id: String,
	},
	/// A new target on the same goal: back to active, shown again, last
	/// reached month kept (D71).
	RetargetGoal {
		goal_id: String,
		target: i64,
		target_date: Option<String>,
	},
}

impl veritas_session::Command for Command {
	fn plan(&self, data: &BudgetData) -> Result<Vec<Row>, CoreError> {
		if !self.reads_sheet() {
			return self.plan_plain(data);
		}
		let sheet = EnvelopeSheet::compute(data, today(data))?;
		self.plan_with_sheet(data, &sheet)
	}

	fn reads_sheet(&self) -> bool {
		matches!(
			self,
			Command::SetBudget { .. }
				| Command::MoveBudget { .. }
				| Command::CoverOverspending { .. }
				| Command::HoldForNextMonth { .. }
				| Command::ResetHold { .. }
				| Command::SetCarryover { .. }
				| Command::ArchiveCategory { .. }
				| Command::DeleteCategoryMovingHistory { .. }
				| Command::CreateGoal { .. }
				| Command::CloseGoal { .. }
				| Command::RetargetGoal { .. }
		)
	}

	fn plan_with_sheet(
		&self,
		data: &BudgetData,
		sheet: &EnvelopeSheet,
	) -> Result<Vec<Row>, CoreError> {
		match self {
			Command::SetBudget {
				month,
				category_id,
				amount,
			} => edit(data, sheet, *month, |editor, month| {
				editor.set_budget(month, &live_category(data, category_id)?, money(*amount))
			}),
			Command::MoveBudget {
				month,
				amount,
				from_category_id,
				to_category_id,
			} => edit(
				data,
				sheet,
				*month,
				|editor, month| match from_category_id {
					Some(from) => editor.move_budget(
						month,
						money(*amount),
						&live_category(data, from)?,
						&envelope(to_category_id),
					),
					None => match to_category_id {
						Some(to) => editor.fund_from_to_budget(
							month,
							money(*amount),
							&live_category(data, to)?,
						),
						None => Err(CoreError::Invalid(
							"Choose an envelope to move budget to or from.".to_owned(),
						)),
					},
				},
			),
			Command::CoverOverspending {
				month,
				category_id,
				from_category_id,
				amount,
			} => edit(data, sheet, *month, |editor, month| {
				editor.cover_overspending(
					month,
					&live_category(data, category_id)?,
					&envelope(from_category_id),
					amount.map(money),
				)
			}),
			Command::HoldForNextMonth { month, amount } => {
				edit(data, sheet, *month, |editor, month| {
					editor
						.hold_for_next_month(month, money(*amount))?
						.ok_or_else(|| {
							CoreError::Invalid("There's nothing in To Budget to hold.".to_owned())
						})
				})
			}
			Command::ResetHold { month } => edit(data, sheet, *month, |editor, month| {
				Ok(editor.reset_hold(month))
			}),
			Command::SetCarryover {
				month,
				category_id,
				carryover,
			} => edit(data, sheet, *month, |editor, month| {
				editor.set_carryover(month, &live_category(data, category_id)?, *carryover)
			}),
			Command::ArchiveCategory { category_id } => categories::archive_category(
				data,
				&CategoryId::new(category_id.as_str()),
				sheet.today(),
			),
			Command::DeleteCategoryMovingHistory {
				category_id,
				to_category_id,
			} => categories::delete_category_moving_history(
				data,
				sheet.months(),
				&CategoryId::new(category_id.as_str()),
				&CategoryId::new(to_category_id.as_str()),
			),
			Command::CreateGoal {
				name,
				target,
				target_date,
				icon,
				color,
			} => goals::create_goal(
				data,
				sheet,
				goals::NewGoal {
					name,
					target: money(*target),
					target_date: date(target_date)?,
					icon: icon.clone(),
					color: color.clone(),
				},
			)
			.map(|(rows, _)| rows),
			Command::CloseGoal {
				goal_id,
				release_to_category_id,
				also_move_later,
			} => goals::close_goal(
				data,
				sheet,
				&GoalId::new(goal_id.as_str()),
				&envelope(release_to_category_id),
				*also_move_later,
			),
			Command::RetargetGoal {
				goal_id,
				target,
				target_date,
			} => goals::retarget_goal(
				data,
				sheet,
				&GoalId::new(goal_id.as_str()),
				money(*target),
				date(target_date)?,
			),
			_ => self.plan_plain(data),
		}
	}
}

fn date(text: &Option<String>) -> Result<Option<CivilDate>, CoreError> {
	text.as_deref().map(civil_date).transpose()
}

impl Command {
	/// Commands that don't read the envelope sheet.
	fn plan_plain(&self, data: &BudgetData) -> Result<Vec<Row>, CoreError> {
		let category = |id: &String| CategoryId::new(id.as_str());
		let group = |id: &String| CategoryGroupId::new(id.as_str());
		match self {
			Command::RenameAccount { account_id, name } => rename_account(data, account_id, name),
			Command::SetCategoryNote { category_id, text } => {
				let id = live_category(data, category_id)?;
				let text = text.trim();
				Ok(vec![Row::Note(Note {
					id: id.to_string(),
					text: (!text.is_empty()).then(|| text.to_owned()),
				})])
			}
			Command::CreateCategory { group_id, name } => {
				categories::create_category(data, &group(group_id), name).map(|(rows, _)| rows)
			}
			Command::RenameCategory { category_id, name } => {
				categories::rename_category(data, &category(category_id), name)
			}
			Command::MoveCategory {
				category_id,
				group_id,
				before_category_id,
			} => categories::move_category(
				data,
				&category(category_id),
				&group(group_id),
				before_category_id.as_ref().map(category).as_ref(),
			),
			Command::SetCategoryHidden {
				category_id,
				hidden,
			} => categories::set_category_hidden(data, &category(category_id), *hidden),
			Command::DeleteCategory { category_id } => {
				categories::delete_category(data, &category(category_id))
			}
			Command::CreateCategoryGroup { name } => {
				categories::create_group(data, name).map(|(rows, _)| rows)
			}
			Command::RenameCategoryGroup { group_id, name } => {
				categories::rename_group(data, &group(group_id), name)
			}
			Command::MoveCategoryGroup {
				group_id,
				before_group_id,
			} => categories::move_group(
				data,
				&group(group_id),
				before_group_id.as_ref().map(group).as_ref(),
			),
			Command::SetCategoryGroupHidden { group_id, hidden } => {
				categories::set_group_hidden(data, &group(group_id), *hidden)
			}
			Command::ReopenGoal { goal_id } => {
				goals::reopen_goal(data, &GoalId::new(goal_id.as_str()))
			}
			_ => Err(CoreError::Internal(
				"a budget command was planned without the envelope sheet".to_owned(),
			)),
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

/// The sheet's date when a command runs outside a session, as tests do: the
/// latest month the budget touches, so every month it has is covered.
fn today(data: &BudgetData) -> veritas_domain::CivilDate {
	data.budget_cells
		.keys()
		.map(|(month, _)| month.first_day())
		.chain(data.transactions.values().map(|t| t.date))
		.max()
		.unwrap_or_else(|| veritas_domain::CivilDate::from(chrono::NaiveDate::default()))
}
