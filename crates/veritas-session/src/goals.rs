//! Goals as the app sees and edits them (R-GOAL-1/3/4, docs/finance/debt.md
//! DEBT-8, D66, D71). The math lives in `veritas_engine::goals`; this plans
//! the rows each goal command writes.

use veritas_domain::{
	BudgetData, CategoryGroupId, CategoryId, CivilDate, CoreError, Goal, GoalId, GoalStatus, Money,
	Row, YearMonth,
};
use veritas_engine::goals::{self as engine, Suggestion};
use veritas_engine::{BudgetEditor, BudgetWrites, Envelope, EnvelopeSheet, NumberFormat};

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

#[derive(Clone, Debug, PartialEq)]
pub struct GoalView {
	pub goal: Goal,
	pub name: String,
	pub state: GoalState,
	/// The envelope's available balance this month.
	pub available: Money,
	/// The category's net spending over every month, at least zero (DEBT-8).
	pub spent: Money,
	/// Active and reached goals with a target date only.
	pub suggestion: Option<Suggestion>,
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
pub fn goals(data: &BudgetData, sheet: &EnvelopeSheet) -> Result<Vec<GoalView>, CoreError> {
	let month = sheet.today().year_month();
	let mut views = Vec::new();
	for goal in data.goals.values() {
		let Some(category) = data.categories.get(&goal.category).filter(|c| !c.tombstone) else {
			continue;
		};
		let available = available(sheet, &goal.category, month);
		let spent = spent(sheet, &goal.category)?;
		let state = match goal.status {
			GoalStatus::Active => GoalState::Active,
			GoalStatus::Reached if spent > Money::ZERO => GoalState::ReachedSpent,
			GoalStatus::Reached => GoalState::Reached,
			GoalStatus::Closed => GoalState::Closed,
		};
		let suggestion = match state {
			GoalState::Active | GoalState::Reached => engine::suggestion(sheet, goal, month)?,
			GoalState::ReachedSpent | GoalState::Closed => None,
		};
		let (later_budget, new_activity) = match (goal.status, goal.closed_month) {
			(GoalStatus::Closed, Some(closed)) => (
				later_budget(data, sheet, &goal.category, closed),
				new_activity(sheet, &goal.category, closed),
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
				suggestion,
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
		for (later, amount) in later_budget(data, sheet, &goal.category, month) {
			extend(
				&mut writes,
				editor.set_budget(later, &goal.category, Money::ZERO)?,
			);
			if let Envelope::Category(destination) = release_to {
				let total =
					Money::from_minor(editor.budgeted(later, destination)).checked_add(amount)?;
				extend(&mut writes, editor.set_budget(later, destination, total)?);
			}
		}
	}
	let mut planned = rows(data, &writes);
	planned.push(Row::Goal(closing.goal));
	planned.extend(categories::set_category_hidden(data, &closing.hide, true)?);
	Ok(planned)
}

/// DEBT-8.2 Reopen: back to reached or active, and shown again.
pub fn reopen_goal(data: &BudgetData, id: &GoalId) -> Result<Vec<Row>, CoreError> {
	let goal = live_goal(data, id)?;
	if goal.status != GoalStatus::Closed {
		return Err(CoreError::Invalid("This goal isn't closed.".to_owned()));
	}
	let mut planned = vec![Row::Goal(engine::reopen(goal))];
	planned.extend(categories::set_category_hidden(
		data,
		&goal.category,
		false,
	)?);
	Ok(planned)
}

/// D71: a new target on the same goal. It goes back to active, keeps when it
/// was last reached, and is shown again if it was closed.
pub fn retarget_goal(
	data: &BudgetData,
	sheet: &EnvelopeSheet,
	id: &GoalId,
	target: Money,
	target_date: Option<CivilDate>,
) -> Result<Vec<Row>, CoreError> {
	let goal = live_goal(data, id)?;
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
		closed_month: None,
		released_to: None,
		released: None,
		..goal.clone()
	}));
	if data
		.categories
		.get(&goal.category)
		.is_some_and(|c| c.hidden)
	{
		planned.extend(categories::set_category_hidden(
			data,
			&goal.category,
			false,
		)?);
	}
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
	let format =
		NumberFormat::from_preference(data.format.number_format.as_deref().unwrap_or("comma-dot"));
	BudgetEditor::new(data, sheet, format, data.format.hide_fraction)
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

fn spent(sheet: &EnvelopeSheet, category: &CategoryId) -> Result<Money, CoreError> {
	let mut activity = Money::ZERO;
	for month in sheet.months() {
		if let Some(cell) = sheet.category(*month, category) {
			activity = activity.checked_add(cell.spent)?;
		}
	}
	Ok(Money::from_minor(activity.checked_neg()?.minor().max(0)))
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

/// The close left the envelope at zero, so any balance in that month or
/// activity after it came later.
fn new_activity(sheet: &EnvelopeSheet, category: &CategoryId, closed: YearMonth) -> bool {
	available(sheet, category, closed) != Money::ZERO
		|| sheet
			.months()
			.iter()
			.filter(|month| **month > closed)
			.any(|month| {
				sheet
					.category(*month, category)
					.is_some_and(|c| c.spent != Money::ZERO)
			})
}
