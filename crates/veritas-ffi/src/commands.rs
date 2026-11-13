use veritas_domain::{
	Account, AccountId, BudgetData, CategoryGroupId, CategoryId, CivilDate, CoreError, GoalId,
	Note, Row,
};
use veritas_domain::{AccountGroupId, PayeeId, TransactionId};
use veritas_rules::RuleBook;
use veritas_session::{accounts, categories, goals, payees, transactions, writes};

use crate::accounts::TransactionDraft;
use crate::records::NewAccount;

use veritas_engine::EnvelopeSheet;

use crate::Month;
use crate::budget::{civil_date, edit, envelope, money};
use crate::custom_reports::{self, CustomReportSettings};

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
	/// Shown again, and `reached` if the envelope holds the target this
	/// month, else `active`. `reachedMonth` is kept (DEBT-8.2).
	ReopenGoal {
		goal_id: String,
	},
	/// A new target on the same goal: back to active, first reached month kept
	/// (D71). A closed goal is `Invalid`: reopen it first (DEBT-8.2).
	RetargetGoal {
		goal_id: String,
		target: i64,
		target_date: Option<String>,
	},
	/// Saves a custom report's edited settings in Actual's format. Only the
	/// columns that changed are written (R-REP-4).
	SaveCustomReport {
		settings: CustomReportSettings,
	},
	/// Moves or resizes a dashboard widget, clamped to Actual's 12-column
	/// grid and the type's minimum size. Nothing else about it changes.
	PlaceWidget {
		widget_id: String,
		x: i64,
		y: i64,
		width: i64,
		height: i64,
	},
	/// A new account at the end of its list, with its transfer payee and, for
	/// a non-zero balance, a starting-balance row (R-ACC-1, §4.8). The id is
	/// the `account` entity in the `Change`.
	CreateAccount {
		account: NewAccount,
	},
	/// Before `before_account_id`, or to the end, among the accounts it is
	/// listed with.
	MoveAccount {
		account_id: String,
		before_account_id: Option<String>,
	},
	/// Writes only the flag, as Actual does: no category or transaction changes.
	SetAccountOffBudget {
		account_id: String,
		off_budget: bool,
	},
	SetAccountGroup {
		account_id: String,
		group_id: Option<String>,
	},
	CreateAccountGroup {
		name: String,
	},
	RenameAccountGroup {
		group_id: String,
		name: String,
	},
	MoveAccountGroup {
		group_id: String,
		before_group_id: Option<String>,
	},
	/// Its accounts leave the group.
	DeleteAccountGroup {
		group_id: String,
	},
	/// Blank text removes the note.
	SetAccountNote {
		account_id: String,
		text: String,
	},
	/// Actual's close (§4.9). With no rows the account is deleted instead. A
	/// non-zero balance needs `transfer_to_account_id`: a "Closing account"
	/// transfer moves it there, with `category_id` on this side when the money
	/// leaves the budget.
	CloseAccount {
		account_id: String,
		transfer_to_account_id: Option<String>,
		category_id: Option<String>,
	},
	ReopenAccount {
		account_id: String,
	},
	/// A typed row (R-ACC-2, R-ACC-4): rules fill what was left empty, or it
	/// is saved as typed with a "may need Actual" note when a rule can't run
	/// (D64). Transfers get their counterpart; categories are learned (§4.5).
	/// The new id is the first `transaction` entity in the `Change`.
	AddTransaction {
		transaction: TransactionDraft,
	},
	/// Edits in place without rules; split lines keep their rows by id, and
	/// the transfer hooks run (§4.2, §4.3).
	EditTransaction {
		transaction_id: String,
		transaction: TransactionDraft,
	},
	/// With its split lines and transfer counterparts.
	DeleteTransaction {
		transaction_id: String,
	},
	/// A copy dated the same, uncleared, with its own transfer counterpart.
	/// `new_id` picks the copy's id, as `TransactionDraft.id` does.
	DuplicateTransaction {
		transaction_id: String,
		new_id: Option<String>,
	},
	/// Reconciled rows are skipped; split lines follow their parent.
	SetCleared {
		transaction_ids: Vec<String>,
		cleared: bool,
	},
	/// Unlocks a reconciled row, leaving it cleared.
	UnlockReconciled {
		transaction_id: String,
	},
	/// The optional reconcile adjustment for `statement - cleared`, today,
	/// cleared, through the rules (R-ACC-3, §4.7).
	AddReconcileAdjustment {
		account_id: String,
		statement_balance: i64,
	},
	/// Locks every cleared row when the cleared balance equals the statement,
	/// and records the reconcile either way.
	FinishReconcile {
		account_id: String,
		statement_balance: i64,
	},
	RenamePayee {
		payee_id: String,
		name: String,
	},
	/// Actual's merge (§4.5): transfer payees are left out, mappings follow.
	MergePayees {
		target_payee_id: String,
		payee_ids: Vec<String>,
	},
	SetPayeeFavorite {
		payee_id: String,
		favorite: bool,
	},
	SetPayeeLearnCategories {
		payee_id: String,
		learn: bool,
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
				| Command::ReopenGoal { .. }
				| Command::RetargetGoal { .. }
				| Command::CreateAccount { .. }
				| Command::CloseAccount { .. }
				| Command::AddTransaction { .. }
				| Command::EditTransaction { .. }
				| Command::AddReconcileAdjustment { .. }
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
			Command::ReopenGoal { goal_id } => {
				goals::reopen_goal(data, sheet, &GoalId::new(goal_id.as_str()))
			}
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
			Command::CreateAccount { account } => accounts::create_account(
				data,
				&veritas_domain::NewAccount::from(account),
				sheet.today(),
				now_ms(),
			)
			.map(|(rows, _)| rows),
			Command::CloseAccount {
				account_id,
				transfer_to_account_id,
				category_id,
			} => accounts::close_account(
				data,
				&AccountId::new(account_id.as_str()),
				transfer_to_account_id
					.as_deref()
					.map(AccountId::new)
					.as_ref(),
				category_id.as_deref().map(CategoryId::new).as_ref(),
				sheet.today(),
				now_ms(),
			),
			Command::AddTransaction { transaction } => {
				let (mut rows, draft) = crate::accounts::draft(data, transaction)?;
				let with = writes::applied(data, &rows);
				let book = RuleBook::new(&with);
				let planned = transactions::add(&with, &draft, &book, now_ms())?;
				rows.extend(planned.rows);
				learn(data, rows, &planned.id, sheet.today())
			}
			Command::EditTransaction {
				transaction_id,
				transaction,
			} => {
				let (mut rows, draft) = crate::accounts::draft(data, transaction)?;
				let with = writes::applied(data, &rows);
				let book = RuleBook::new(&with);
				let id = TransactionId::new(transaction_id.as_str());
				rows.extend(transactions::edit(
					&with,
					&id,
					&draft,
					Some(&book),
					now_ms(),
				)?);
				learn(data, rows, &id, sheet.today())
			}
			Command::AddReconcileAdjustment {
				account_id,
				statement_balance,
			} => payees::reconcile_adjustment(
				data,
				&AccountId::new(account_id.as_str()),
				money(*statement_balance),
				&RuleBook::new(data),
				sheet.today(),
				now_ms(),
			)
			.map(|planned| planned.rows),
			_ => self.plan_plain(data),
		}
	}
}

fn now_ms() -> f64 {
	std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map_or(0.0, |elapsed| elapsed.as_millis() as f64)
}

/// Adds the rules `learn_categories` writes for the saved row and its lines,
/// as Actual's desktop register does with its default `learn-categories` on.
fn learn(
	data: &BudgetData,
	mut rows: Vec<Row>,
	id: &TransactionId,
	today: CivilDate,
) -> Result<Vec<Row>, CoreError> {
	let saved: Vec<TransactionId> = rows
		.iter()
		.filter_map(|row| match row {
			Row::Transaction(t) if !t.tombstone => Some(t),
			_ => None,
		})
		.filter(|t| {
			&t.id == id
				|| matches!(&t.split, veritas_domain::SplitRole::Child { parent } if parent == id)
		})
		.map(|t| t.id.clone())
		.collect();
	let after = writes::applied(data, &rows);
	rows.extend(transactions::learn_categories(&after, &saved, today)?);
	Ok(rows)
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
			Command::SaveCustomReport { settings } => {
				custom_reports::save_custom_report(data, settings)
			}
			Command::PlaceWidget {
				widget_id,
				x,
				y,
				width,
				height,
			} => custom_reports::place_widget(data, widget_id, (*x, *y, *width, *height)),
			Command::MoveAccount {
				account_id,
				before_account_id,
			} => accounts::move_account(
				data,
				&AccountId::new(account_id.as_str()),
				before_account_id.as_deref().map(AccountId::new).as_ref(),
			),
			Command::SetAccountOffBudget {
				account_id,
				off_budget,
			} => accounts::set_off_budget(data, &AccountId::new(account_id.as_str()), *off_budget),
			Command::SetAccountGroup {
				account_id,
				group_id,
			} => accounts::set_account_group(
				data,
				&AccountId::new(account_id.as_str()),
				group_id.as_deref().map(AccountGroupId::new).as_ref(),
			),
			Command::CreateAccountGroup { name } => {
				accounts::create_account_group(data, name).map(|(rows, _)| rows)
			}
			Command::RenameAccountGroup { group_id, name } => {
				accounts::rename_account_group(data, &AccountGroupId::new(group_id.as_str()), name)
			}
			Command::MoveAccountGroup {
				group_id,
				before_group_id,
			} => accounts::move_account_group(
				data,
				&AccountGroupId::new(group_id.as_str()),
				before_group_id.as_deref().map(AccountGroupId::new).as_ref(),
			),
			Command::DeleteAccountGroup { group_id } => {
				accounts::delete_account_group(data, &AccountGroupId::new(group_id.as_str()))
			}
			Command::SetAccountNote { account_id, text } => {
				accounts::set_account_note(data, &AccountId::new(account_id.as_str()), text)
			}
			Command::ReopenAccount { account_id } => {
				accounts::reopen_account(data, &AccountId::new(account_id.as_str()))
			}
			Command::DeleteTransaction { transaction_id } => {
				transactions::delete(data, &TransactionId::new(transaction_id.as_str()))
			}
			Command::DuplicateTransaction {
				transaction_id,
				new_id,
			} => transactions::duplicate(
				data,
				&TransactionId::new(transaction_id.as_str()),
				new_id.as_deref().map(TransactionId::new),
				Some(&RuleBook::new(data)),
				now_ms(),
			)
			.map(|planned| planned.rows),
			Command::SetCleared {
				transaction_ids,
				cleared,
			} => {
				let ids: Vec<TransactionId> = transaction_ids
					.iter()
					.map(|id| TransactionId::new(id.as_str()))
					.collect();
				transactions::set_cleared(data, &ids, *cleared)
			}
			Command::UnlockReconciled { transaction_id } => {
				transactions::unreconcile(data, &TransactionId::new(transaction_id.as_str()))
			}
			Command::FinishReconcile {
				account_id,
				statement_balance,
			} => payees::finish_reconcile(
				data,
				&AccountId::new(account_id.as_str()),
				money(*statement_balance),
				now_ms(),
			)
			.map(|(rows, _)| rows),
			Command::RenamePayee { payee_id, name } => {
				payees::rename_payee(data, &PayeeId::new(payee_id.as_str()), name)
			}
			Command::MergePayees {
				target_payee_id,
				payee_ids,
			} => {
				let sources: Vec<PayeeId> = payee_ids
					.iter()
					.map(|id| PayeeId::new(id.as_str()))
					.collect();
				payees::merge_payees(data, &PayeeId::new(target_payee_id.as_str()), &sources)
			}
			Command::SetPayeeFavorite { payee_id, favorite } => {
				payees::set_favorite(data, &PayeeId::new(payee_id.as_str()), *favorite)
			}
			Command::SetPayeeLearnCategories { payee_id, learn } => {
				payees::set_learn_categories(data, &PayeeId::new(payee_id.as_str()), *learn)
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
