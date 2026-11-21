//! Goals as the app sees and edits them (R-GOAL-1/3/4, docs/finance/debt.md
//! DEBT-8, D66, D71). The math lives in `veritas_engine::goals`; this plans
//! the rows each goal command writes.

use veritas_domain::{
	BudgetData, CategoryGroupId, CategoryId, CivilDate, CoreError, Goal, GoalId, GoalStatus, Money,
	Row, YearMonth, is_single_emoji,
};
use veritas_engine::goals::{self as engine, Suggestion};
use veritas_engine::{BudgetEditor, BudgetWrites, Envelope, EnvelopeSheet};
use veritas_reports::Classification;

use crate::categories;
use crate::writes::{applied, rows};

pub const GOALS_GROUP: &str = "Goals";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoalState {
	Active,
	Reached,
	/// Reached, then spent from (D66). No suggestion until reopened or
	/// re-targeted.
	ReachedSpent,
	Closed,
}

/// Whether an active goal with a target date gets there in time, by FC-6's
/// on-track rule, the one the forecast's goal ETA uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoalPace {
	/// The median pace's ETA is not after the target month.
	OnTrack { eta_month: YearMonth },
	/// The ETA is after the target month. DEBT-8.1's monthly amount gets
	/// there in time.
	Behind { needed_monthly: Money },
	/// No complete month of budget yet, so no ETA (a brand-new goal).
	NoHistory { needed_monthly: Money },
	/// Nothing added at the median pace lately, so no ETA.
	NotAdding { needed_monthly: Money },
	/// D77: outside a 2-place budget, spending is recorded, so `available`
	/// mixes scales: "No ETA: spending recorded", with no needed figure.
	SpendingRecorded,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GoalView {
	pub goal: Goal,
	pub name: String,
	pub state: GoalState,
	/// The envelope's available balance this month.
	pub available: Money,
	/// DEBT-8's Spending to date: CLS-6 `expense_total` for the category
	/// through today. Refunds lower it, and it can be negative (ST-1.2).
	pub spent: Money,
	/// 0 to 100 for the ring and its label. 100 only once reached.
	pub progress_percent: u8,
	/// The "X of target" figure: `available`, except that a reached goal
	/// that was spent from still reads as its target (design/mockups/goals.html).
	pub progress_amount: Money,
	/// Active and reached goals with a target date only.
	pub suggestion: Option<Suggestion>,
	/// Active goals with a target date only.
	pub pace: Option<GoalPace>,
	/// A closed goal's budget in months after it closed (GOAL-10).
	pub later_budget: Vec<(YearMonth, Money)>,
	/// A closed goal whose envelope moved after it closed (GOAL-6).
	pub new_activity: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClosePreview {
	/// What the close moves out of the goal this month; negative when the goal
	/// is overspent and the destination covers it (DEBT-8.3).
	pub leftover: Money,
	/// Budget already in later months, which "Also move those" moves too.
	pub later_budget: Vec<(YearMonth, Money)>,
	/// The destination's available this month after the release, or To
	/// Budget's. Negative is the "will be $X over" warning (GOAL-3, GOAL-8).
	pub destination_after: Money,
}

/// Every goal whose category still exists, in budget order.
pub fn goals(
	data: &BudgetData,
	sheet: &EnvelopeSheet,
	classification: &Classification,
) -> Result<Vec<GoalView>, CoreError> {
	let month = sheet.today().year_month();
	let mut views = Vec::new();
	for goal in data.goals.values() {
		let Some(category) = data.categories.get(&goal.category).filter(|c| !c.tombstone) else {
			continue;
		};
		let available = available(sheet, &goal.category, month);
		let spent =
			veritas_forecast::spending_to_date(classification, &goal.category, sheet.today())?;
		let state = match goal.status {
			GoalStatus::Active => GoalState::Active,
			GoalStatus::Reached if available < goal.target => GoalState::ReachedSpent,
			GoalStatus::Reached => GoalState::Reached,
			GoalStatus::Closed => GoalState::Closed,
		};
		// A reached goal, spent or not, gets no suggestion and no ETA (D66,
		// GOAL-10, FC-5, FC-6).
		let suggestion = match state {
			GoalState::Active => engine::suggestion(sheet, goal, month)?,
			GoalState::Reached | GoalState::ReachedSpent | GoalState::Closed => None,
		};
		// D77: no ETA and no needed figure once spending exists outside a
		// 2-place budget, so the Goals screen agrees with the forecast.
		let spending_recorded = state == GoalState::Active
			&& veritas_forecast::spending_recorded(data, classification, &goal.category, month)?;
		let (suggestion, pace) = match suggestion {
			Some(_) if spending_recorded => (None, Some(GoalPace::SpendingRecorded)),
			Some(suggestion) => (
				Some(suggestion),
				Some(pace(data, goal, month, available, suggestion)?),
			),
			None => (None, None),
		};
		let (later_budget, new_activity) = match (goal.status, goal.closed_month) {
			(GoalStatus::Closed, Some(closed)) => (
				later_budget(data, sheet, &goal.category, closed),
				new_activity(sheet, goal, closed),
			),
			_ => (Vec::new(), false),
		};
		let order = data
			.category_groups
			.get(&category.group)
			.map_or(f64::MAX, |g| g.sort_order);
		views.push((
			(order, category.sort_order),
			GoalView {
				goal: goal.clone(),
				name: category.name.clone(),
				state,
				available,
				spent,
				progress_percent: progress_percent(state, available, goal.target),
				progress_amount: match state {
					GoalState::ReachedSpent => goal.target,
					GoalState::Active | GoalState::Reached | GoalState::Closed => available,
				},
				suggestion,
				pace,
				later_budget,
				new_activity,
			},
		));
	}
	views.sort_by(|(a, x), (b, y)| {
		a.0.total_cmp(&b.0)
			.then(a.1.total_cmp(&b.1))
			.then_with(|| x.name.cmp(&y.name))
	});
	Ok(views.into_iter().map(|(_, view)| view).collect())
}

fn pace(
	data: &BudgetData,
	goal: &Goal,
	month: YearMonth,
	available: Money,
	suggestion: Suggestion,
) -> Result<GoalPace, CoreError> {
	use veritas_forecast::EtaStatus;
	let eta = veritas_forecast::goal_eta(data, &goal.id, month, available, false, None)?;
	let needed_monthly = suggestion.monthly;
	Ok(match (eta.on_track, eta.status) {
		(_, EtaStatus::NoHistory) => GoalPace::NoHistory { needed_monthly },
		(_, EtaStatus::NotAdding) => GoalPace::NotAdding { needed_monthly },
		(Some(true), EtaStatus::Eta { month, .. }) => GoalPace::OnTrack { eta_month: month },
		(Some(true), _) => GoalPace::OnTrack { eta_month: month },
		_ => GoalPace::Behind { needed_monthly },
	})
}

pub fn close_preview(
	data: &BudgetData,
	sheet: &EnvelopeSheet,
	id: &GoalId,
	release_to: &Envelope,
) -> Result<ClosePreview, CoreError> {
	let goal = open_goal(data, id)?;
	let month = sheet.today().year_month();
	let leftover = available(sheet, &goal.category, month);
	let before = match release_to {
		Envelope::Category(destination) => {
			live_category(data, destination)?;
			available(sheet, destination, month)
		}
		Envelope::ToBudget => sheet
			.summary(month)
			.map_or(Money::ZERO, |summary| summary.to_budget),
	};
	Ok(ClosePreview {
		leftover,
		later_budget: later_budget(data, sheet, &goal.category, month),
		destination_after: before.checked_add(leftover)?,
	})
}

/// A goal starts as a new envelope at the top of the Goals group, which is
/// created if the budget has none (DEBT-8 Definitions).
pub struct NewGoal<'a> {
	pub name: &'a str,
	pub target: Money,
	pub target_date: Option<CivilDate>,
	pub icon: Option<String>,
	pub color: Option<String>,
}

pub fn create_goal(
	data: &BudgetData,
	sheet: &EnvelopeSheet,
	new: NewGoal<'_>,
) -> Result<(Vec<Row>, GoalId), CoreError> {
	let mut planned = Vec::new();
	let group = match goals_group(data) {
		Some(group) => group,
		None => {
			let (group_rows, group) = categories::create_group(data, GOALS_GROUP)?;
			planned.extend(group_rows);
			group
		}
	};
	let with_group = applied(data, &planned);
	let (category_rows, category) = categories::create_category(&with_group, &group, new.name)?;
	let with_category = applied(&with_group, &category_rows);
	planned.extend(category_rows);
	let writes = engine::target_writes(
		&with_category,
		sheet,
		&category,
		new.target,
		sheet.today().year_month(),
	)?;
	planned.extend(rows(&with_category, &writes));
	let goal = Goal {
		id: GoalId::new(veritas_store::new_id()),
		category,
		target: new.target,
		target_date: new.target_date,
		icon: new.icon,
		color: new.color,
		status: GoalStatus::Active,
		reached_month: None,
		closed_month: None,
		released_to: None,
		released: None,
		activity_seen_through: None,
		activity_seen_total: None,
		close_month_dismissed_available: None,
	};
	let id = goal.id.clone();
	planned.push(Row::Goal(goal));
	Ok((planned, id))
}

/// DEBT-8.3: releases this month's leftover to `release_to`, closes and hides
/// the goal. With `also_move_later`, budget in later months moves to the same
/// place (D66, GOAL-10).
pub fn close_goal(
	data: &BudgetData,
	sheet: &EnvelopeSheet,
	id: &GoalId,
	release_to: &Envelope,
	also_move_later: bool,
) -> Result<Vec<Row>, CoreError> {
	let goal = open_goal(data, id)?;
	if let Envelope::Category(destination) = release_to {
		live_category(data, destination)?;
	}
	let month = sheet.today().year_month();
	let editor = editor(data, sheet);
	let closing = engine::close(&editor, sheet, goal, month, release_to)?;
	let mut writes = closing.writes;
	if also_move_later {
		// One move per later month, each with its movement note (GOAL-7,
		// GOAL-10). A negative amount moves the other way.
		for (later, amount) in later_budget(data, sheet, &goal.category, month) {
			extend(
				&mut writes,
				editor.move_budget(later, amount, &goal.category, release_to)?,
			);
		}
	}
	let mut planned = rows(data, &writes);
	planned.push(Row::Goal(closing.goal));
	planned.extend(categories::set_category_hidden(data, &closing.hide, true)?);
	Ok(planned)
}

/// DEBT-8.2 Reopen: reached if the envelope holds the target this month, else
/// active, and shown again.
pub fn reopen_goal(
	data: &BudgetData,
	sheet: &EnvelopeSheet,
	id: &GoalId,
) -> Result<Vec<Row>, CoreError> {
	let goal = live_goal(data, id)?;
	if goal.status != GoalStatus::Closed {
		return Err(CoreError::Invalid("This goal isn't closed.".to_owned()));
	}
	// CLS-3.3: a goal is an expense envelope, and reopening never flips the
	// kind back.
	let kind = match data.category_kind(&goal.category) {
		veritas_domain::CategoryKind::Expense => None,
		veritas_domain::CategoryKind::DebtPrincipal => Some("debt principal"),
		veritas_domain::CategoryKind::Savings => Some("savings"),
		veritas_domain::CategoryKind::Income => Some("income"),
	};
	if let Some(kind) = kind {
		return Err(CoreError::Invalid(format!(
			"This category is now a {kind} category. Change it back to an expense category to reopen this goal."
		)));
	}
	let available = available(sheet, &goal.category, sheet.today().year_month());
	let mut planned = vec![Row::Goal(engine::reopen(goal, available))];
	planned.extend(categories::set_category_hidden(
		data,
		&goal.category,
		false,
	)?);
	Ok(planned)
}

/// D71: a new target on the same goal. It goes back to active and keeps when
/// it was first reached. A closed goal is reopened first (DEBT-8.2).
pub fn retarget_goal(
	data: &BudgetData,
	sheet: &EnvelopeSheet,
	id: &GoalId,
	target: Money,
	target_date: Option<CivilDate>,
) -> Result<Vec<Row>, CoreError> {
	let goal = live_goal(data, id)?;
	if goal.status == GoalStatus::Closed {
		return Err(CoreError::Invalid(
			"Reopen this goal before giving it a new target.".to_owned(),
		));
	}
	let writes = engine::target_writes(
		data,
		sheet,
		&goal.category,
		target,
		sheet.today().year_month(),
	)?;
	let mut planned = rows(data, &writes);
	planned.push(Row::Goal(Goal {
		target,
		target_date,
		status: GoalStatus::Active,
		..goal.clone()
	}));
	Ok(planned)
}

/// DEBT-8.2 active → reached, for every active goal whose envelope holds its
/// target this month. The session writes these in the transaction of the edit
/// that reached them.
pub fn newly_reached(data: &BudgetData, sheet: &EnvelopeSheet) -> Vec<Row> {
	let month = sheet.today().year_month();
	data.goals
		.values()
		.filter(|goal| goal.status == GoalStatus::Active)
		.filter(|goal| {
			data.categories
				.get(&goal.category)
				.is_some_and(|c| !c.tombstone)
		})
		.filter_map(|goal| engine::check_reached(sheet, goal, month))
		.map(Row::Goal)
		.collect()
}

pub fn has_active_goals(data: &BudgetData) -> bool {
	data.goals
		.values()
		.any(|goal| goal.status == GoalStatus::Active)
}

fn goals_group(data: &BudgetData) -> Option<CategoryGroupId> {
	data.category_groups
		.values()
		.filter(|g| !g.tombstone && !g.is_income)
		.find(|g| g.name.trim().eq_ignore_ascii_case(GOALS_GROUP))
		.map(|g| g.id.clone())
}

/// A goal's icon, one emoji, stored only in `veritas_goal`; `None` or blank
/// clears it. A label (DEBT-8): it moves no money, changes no status, ETA or
/// suggestion, and a closed goal keeps it and can still change it.
pub fn set_goal_icon(
	data: &BudgetData,
	id: &GoalId,
	icon: Option<&str>,
) -> Result<Vec<Row>, CoreError> {
	let goal = live_goal(data, id)?;
	let icon = icon.filter(|icon| !icon.is_empty());
	if icon.is_some_and(|icon| !is_single_emoji(icon)) {
		return Err(CoreError::Invalid("Choose one emoji.".to_owned()));
	}
	let icon = icon.map(str::to_owned);
	if goal.icon == icon {
		return Ok(Vec::new());
	}
	Ok(vec![Row::Goal(Goal {
		icon,
		..goal.clone()
	})])
}

fn live_goal<'a>(data: &'a BudgetData, id: &GoalId) -> Result<&'a Goal, CoreError> {
	data.goals
		.get(id)
		.filter(|goal| {
			data.categories
				.get(&goal.category)
				.is_some_and(|c| !c.tombstone)
		})
		.ok_or_else(|| CoreError::NotFound {
			entity: "goal",
			id: id.to_string(),
		})
}

fn open_goal<'a>(data: &'a BudgetData, id: &GoalId) -> Result<&'a Goal, CoreError> {
	let goal = live_goal(data, id)?;
	if goal.status == GoalStatus::Closed {
		return Err(CoreError::Invalid(
			"This goal is already closed.".to_owned(),
		));
	}
	Ok(goal)
}

fn live_category(data: &BudgetData, id: &CategoryId) -> Result<(), CoreError> {
	match data.categories.get(id) {
		Some(category) if !category.tombstone => Ok(()),
		_ => Err(CoreError::NotFound {
			entity: "category",
			id: id.to_string(),
		}),
	}
}

fn editor<'a>(data: &'a BudgetData, sheet: &'a EnvelopeSheet) -> BudgetEditor<'a> {
	BudgetEditor::new(data, sheet)
}

fn extend(writes: &mut BudgetWrites, more: BudgetWrites) {
	writes.cells.extend(more.cells);
	writes.holds.extend(more.holds);
	writes.notes.extend(more.notes);
}

fn available(sheet: &EnvelopeSheet, category: &CategoryId, month: YearMonth) -> Money {
	sheet
		.category(month, category)
		.map_or(Money::ZERO, |cell| cell.balance)
}

/// Rounds down, and stops at 99 until the goal is reached, so the ring never
/// reads full before it is.
fn progress_percent(state: GoalState, available: Money, target: Money) -> u8 {
	if matches!(state, GoalState::Reached | GoalState::ReachedSpent) {
		return 100;
	}
	let (available, target) = (i128::from(available.minor()), i128::from(target.minor()));
	if available <= 0 || target <= 0 {
		return 0;
	}
	u8::try_from((available * 100 / target).min(99)).unwrap_or(99)
}

/// Non-zero budget amounts in the months after `after` that the sheet covers.
fn later_budget(
	data: &BudgetData,
	sheet: &EnvelopeSheet,
	category: &CategoryId,
	after: YearMonth,
) -> Vec<(YearMonth, Money)> {
	sheet
		.months()
		.iter()
		.filter(|month| **month > after)
		.filter_map(|month| {
			let amount = data.budget_cells.get(&(*month, category.clone()))?.budgeted;
			(amount != Money::ZERO).then_some((*month, amount))
		})
		.collect()
}

/// GOAL-6 (D72). The close left the envelope at zero, so (A) a balance in
/// the close month, or (B) activity after it, came later. A dismiss stores
/// what it saw, and the notice returns only when that changes.
fn new_activity(sheet: &EnvelopeSheet, goal: &Goal, closed: YearMonth) -> bool {
	let ending = available(sheet, &goal.category, closed);
	let balance = ending != Money::ZERO && Some(ending) != goal.close_month_dismissed_available;
	let seen_through = goal.activity_seen_through.unwrap_or(closed);
	let later = sheet
		.months()
		.iter()
		.filter(|month| **month > seen_through)
		.any(|month| activity(sheet, &goal.category, *month) != Money::ZERO);
	let seen_changed = goal.activity_seen_through.is_some()
		&& activity_through(sheet, &goal.category, closed, seen_through).ok()
			!= goal.activity_seen_total;
	balance || later || seen_changed
}

/// The category's transaction activity in `month`: every row but a split's
/// parent, as the envelope sheet sums it.
fn activity(sheet: &EnvelopeSheet, category: &CategoryId, month: YearMonth) -> Money {
	sheet
		.category(month, category)
		.map_or(Money::ZERO, |cell| cell.spent)
}

fn activity_through(
	sheet: &EnvelopeSheet,
	category: &CategoryId,
	from: YearMonth,
	through: YearMonth,
) -> Result<Money, CoreError> {
	Money::checked_sum(
		sheet
			.months()
			.iter()
			.filter(|month| (from..=through).contains(*month))
			.map(|month| activity(sheet, category, *month)),
	)
}

/// GOAL-6's dismiss (D72): stores what the notice saw, so it returns only on
/// something new. No money moves.
pub fn dismiss_goal_activity(
	data: &BudgetData,
	sheet: &EnvelopeSheet,
	id: &GoalId,
) -> Result<Vec<Row>, CoreError> {
	let goal = live_goal(data, id)?;
	let (GoalStatus::Closed, Some(closed)) = (goal.status, goal.closed_month) else {
		return Err(CoreError::Invalid("This goal isn't closed.".to_owned()));
	};
	let seen_through = sheet
		.months()
		.iter()
		.copied()
		.filter(|month| *month >= closed)
		.filter(|month| activity(sheet, &goal.category, *month) != Money::ZERO)
		.max()
		.unwrap_or(closed);
	let ending = available(sheet, &goal.category, closed);
	let balance_showing =
		ending != Money::ZERO && Some(ending) != goal.close_month_dismissed_available;
	Ok(vec![Row::Goal(Goal {
		activity_seen_through: Some(seen_through),
		activity_seen_total: Some(activity_through(
			sheet,
			&goal.category,
			closed,
			seen_through,
		)?),
		close_month_dismissed_available: if balance_showing {
			Some(ending)
		} else {
			goal.close_month_dismissed_available
		},
		..goal.clone()
	})])
}

#[cfg(test)]
mod tests {
	use super::*;

	fn percent(state: GoalState, available: i64, target: i64) -> u8 {
		progress_percent(
			state,
			Money::from_minor(available),
			Money::from_minor(target),
		)
	}

	#[test]
	fn progress_rounds_down_and_is_full_only_once_reached() {
		assert_eq!(percent(GoalState::Active, 112_000, 150_000), 74);
		assert_eq!(percent(GoalState::Active, 149_999, 150_000), 99);
		assert_eq!(
			percent(GoalState::Active, 160_000, 150_000),
			99,
			"an active goal over its target still waits for the reached check"
		);
		assert_eq!(percent(GoalState::Active, -2_000, 150_000), 0);
		assert_eq!(percent(GoalState::Closed, 0, 150_000), 0);
		assert_eq!(percent(GoalState::Reached, 150_000, 150_000), 100);
		assert_eq!(percent(GoalState::ReachedSpent, -2_000, 150_000), 100);
		assert_eq!(percent(GoalState::Active, i64::MAX, 1), 99);
	}
}
