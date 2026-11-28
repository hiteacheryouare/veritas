//! The To Budget card's popover (docs/finance/to-budget.md): one read with
//! everything the popover shows, and the refusals the two commands share
//! with it, so a row the read disables is a command the core refuses.

use veritas_domain::{BudgetData, CategoryId, CoreError, YearMonth};
use veritas_engine::{BudgetEditor, EnvelopeSheet, FillReason, Skip, ToBudgetPlan};

use crate::budget::year_month;
use crate::session::Session;
use crate::{FfiError, Month};

/// TB-1.1: Actual's label follows it, `To Budget:` or `Overbudgeted:`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ToBudgetState {
	Positive,
	Zero,
	Overbudgeted,
}

/// Why a row can't act (TB-1.3, TB-1.4, TB-2.6). The shell writes the
/// subtitle from it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ToBudgetBlock {
	/// "To Budget is $0.00".
	Zero,
	/// "Overbudgeted by {amount}. Move budget from an envelope to cover it."
	/// `amount` is the overbudgeted figure as a positive number.
	Overbudgeted { amount: i64 },
	/// "Nothing is underfunded".
	NothingUnderfunded,
	/// "No active goal needs funding".
	NoGoalNeedsFunding,
	/// "Goals are funded for the current month only."
	NotCurrentMonth,
	/// "Spending is recorded at 2 places and budgets at {places}, so To
	/// Budget can't be trusted here."
	MixedScale { places: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ToBudgetFillReason {
	/// TB-3.2: covers the envelope's overspending.
	Overspent,
	/// TB-3.3: up to Actual's stored monthly goal.
	GoalShortfall,
	/// TB-4.2: a Veritas goal's monthly suggestion.
	GoalMonthly,
}

/// One envelope a plan writes: what it needs and what it gets, less when To
/// Budget runs out (TB-2.2), shown "$100.00 of $137.50".
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ToBudgetFill {
	pub category_id: String,
	pub name: String,
	pub reason: ToBudgetFillReason,
	pub goal_id: Option<String>,
	pub need: i64,
	pub amount: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ToBudgetSkipReason {
	NoTargetDate,
}

/// A goal Fund goals' monthly leaves out (TB-4.3).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ToBudgetSkip {
	pub goal_id: String,
	pub name: String,
	pub reason: ToBudgetSkipReason,
}

/// What Assign to underfunded or Fund goals' monthly would do now.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ToBudgetPlanView {
	/// `nil` when the row can act.
	pub block: Option<ToBudgetBlock>,
	/// In order of writing; empty when blocked.
	pub fills: Vec<ToBudgetFill>,
	/// Envelopes written, each counted once ("{n} envelopes").
	pub envelopes: u32,
	pub total: i64,
	/// Hidden envelopes that would otherwise qualify (TB-2.3).
	pub hidden_left_out: u32,
	pub skipped: Vec<ToBudgetSkip>,
}

/// Hold for next month… (TB-5) and the hold rows Actual shows beside it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ToBudgetHold {
	/// Offered when nothing is auto-held, as Actual's menu offers it.
	pub offered: bool,
	pub block: Option<ToBudgetBlock>,
	/// The field's default, the whole To Budget.
	pub default_amount: i64,
	/// The month's manual hold (`buffered`).
	pub held: i64,
	/// Income held by auto hold this month.
	pub auto_held: i64,
	/// "Reset next month's buffer": something is manually held. Available in
	/// every currency, since it only sets the hold to 0 (TB-5.4).
	pub can_reset: bool,
}

/// Everything the To Budget card and its popover show (TB-10.1).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ToBudgetActions {
	pub month: Month,
	pub to_budget: i64,
	pub state: ToBudgetState,
	pub assign: ToBudgetPlanView,
	pub fund_goals: ToBudgetPlanView,
	pub hold: ToBudgetHold,
}

/// TB-2.6: outside a 2-place currency To Budget mixes two scales, so every
/// cap would be measured against a figure that isn't true.
pub(crate) fn mixed_scale(data: &BudgetData) -> Option<ToBudgetBlock> {
	let places = data.format.decimal_places();
	(places != 2).then_some(ToBudgetBlock::MixedScale { places })
}

/// TB-1.3: nothing to spend.
fn empty(to_budget: i64) -> Option<ToBudgetBlock> {
	match to_budget {
		..0 => Some(ToBudgetBlock::Overbudgeted {
			amount: to_budget.saturating_neg(),
		}),
		0 => Some(ToBudgetBlock::Zero),
		_ => None,
	}
}

/// A quick action's plan and, when it can't act, why. A plan that writes
/// nothing keeps its hidden count and skipped goals, so the row can still
/// say what was left out (TB-2.3, TB-4.3).
pub(crate) struct Planned {
	pub(crate) block: Option<ToBudgetBlock>,
	pub(crate) plan: ToBudgetPlan,
}

impl Planned {
	fn blocked(block: ToBudgetBlock) -> Self {
		Planned {
			block: Some(block),
			plan: ToBudgetPlan::default(),
		}
	}

	fn from(plan: ToBudgetPlan, nothing: ToBudgetBlock) -> Self {
		Planned {
			block: plan.fills.is_empty().then_some(nothing),
			plan,
		}
	}

	/// The command's writes, or its refusal.
	pub(crate) fn writes(self) -> Result<veritas_engine::BudgetWrites, CoreError> {
		match self.block {
			Some(block) => Err(refusal(&block)),
			None => Ok(self.plan.writes),
		}
	}
}

/// Assign to underfunded's plan for `month`, or why it can't act.
pub(crate) fn plan_assign(
	data: &BudgetData,
	editor: &BudgetEditor<'_>,
	month: YearMonth,
) -> Result<Planned, CoreError> {
	if let Some(block) = mixed_scale(data).or_else(|| empty(editor.to_budget(month))) {
		return Ok(Planned::blocked(block));
	}
	Ok(Planned::from(
		editor.assign_to_underfunded(month)?,
		ToBudgetBlock::NothingUnderfunded,
	))
}

/// Fund goals' monthly's plan for `month`, or why it can't act. `current` is
/// the month of the session's today (TB-4.1).
pub(crate) fn plan_fund_goals(
	data: &BudgetData,
	editor: &BudgetEditor<'_>,
	month: YearMonth,
	current: YearMonth,
) -> Result<Planned, CoreError> {
	let not_current = (month != current).then_some(ToBudgetBlock::NotCurrentMonth);
	if let Some(block) = mixed_scale(data)
		.or(not_current)
		.or_else(|| empty(editor.to_budget(month)))
	{
		return Ok(Planned::blocked(block));
	}
	Ok(Planned::from(
		editor.fund_goals_monthly(month)?,
		ToBudgetBlock::NoGoalNeedsFunding,
	))
}

/// The command's refusal for a blocked row, in words without figures: the
/// popover has already shown the figures.
pub(crate) fn refusal(block: &ToBudgetBlock) -> CoreError {
	CoreError::Invalid(match block {
		ToBudgetBlock::Zero => "There's nothing in To Budget to assign.".to_owned(),
		ToBudgetBlock::Overbudgeted { .. } => {
			"To Budget is overbudgeted. Move budget from an envelope to cover it.".to_owned()
		}
		ToBudgetBlock::NothingUnderfunded => "Nothing is underfunded.".to_owned(),
		ToBudgetBlock::NoGoalNeedsFunding => "No active goal needs funding.".to_owned(),
		ToBudgetBlock::NotCurrentMonth => "Goals are funded for the current month only.".to_owned(),
		ToBudgetBlock::MixedScale { places } => format!(
			"Spending is recorded at 2 places and budgets at {places}, so To Budget can't be trusted here."
		),
	})
}

fn view(data: &BudgetData, planned: Planned) -> Result<ToBudgetPlanView, CoreError> {
	let name = |category: &CategoryId| {
		data.categories
			.get(category)
			.map(|c| c.name.clone())
			.unwrap_or_default()
	};
	let Planned { block, plan } = planned;
	let acts = block.is_none();
	Ok(ToBudgetPlanView {
		envelopes: if acts {
			u32::try_from(plan.envelopes()).unwrap_or(u32::MAX)
		} else {
			0
		},
		total: if acts { plan.total()?.minor() } else { 0 },
		hidden_left_out: plan.hidden_left_out,
		skipped: plan
			.skipped
			.iter()
			.map(|(goal, Skip::NoTargetDate)| ToBudgetSkip {
				goal_id: goal.to_string(),
				name: data
					.goals
					.get(goal)
					.map(|g| name(&g.category))
					.unwrap_or_default(),
				reason: ToBudgetSkipReason::NoTargetDate,
			})
			.collect(),
		fills: if acts {
			plan.fills
				.iter()
				.map(|fill| {
					let (reason, goal_id) = match &fill.reason {
						FillReason::Overspent => (ToBudgetFillReason::Overspent, None),
						FillReason::GoalShortfall => (ToBudgetFillReason::GoalShortfall, None),
						FillReason::GoalMonthly(goal) => {
							(ToBudgetFillReason::GoalMonthly, Some(goal.to_string()))
						}
					};
					ToBudgetFill {
						category_id: fill.category.to_string(),
						name: name(&fill.category),
						reason,
						goal_id,
						need: fill.need.minor(),
						amount: fill.amount.minor(),
					}
				})
				.collect()
		} else {
			Vec::new()
		},
		block,
	})
}

/// The popover for `month`, clamped to the months Actual has created as the
/// Budget screen clamps it. Writes nothing (TB-1.3).
pub(crate) fn actions(
	data: &BudgetData,
	sheet: &EnvelopeSheet,
	month: Month,
) -> Result<ToBudgetActions, CoreError> {
	let (Some(&first), Some(&last)) = (sheet.months().first(), sheet.months().last()) else {
		return Err(CoreError::Invalid(
			"This budget has no months yet.".to_owned(),
		));
	};
	let month = year_month(month)?.clamp(first, last);
	let summary = sheet
		.summary(month)
		.ok_or_else(|| CoreError::Invalid(format!("{month} isn't in this budget.")))?;
	let editor = BudgetEditor::new(data, sheet);
	let to_budget = summary.to_budget.minor();
	let assign = view(data, plan_assign(data, &editor, month)?)?;
	let fund_goals = view(
		data,
		plan_fund_goals(data, &editor, month, sheet.today().year_month())?,
	)?;
	let held = summary.manual_hold.minor();
	let auto_held = summary.auto_hold.minor();
	Ok(ToBudgetActions {
		month: month.into(),
		to_budget,
		state: match to_budget {
			..0 => ToBudgetState::Overbudgeted,
			0 => ToBudgetState::Zero,
			_ => ToBudgetState::Positive,
		},
		assign,
		fund_goals,
		hold: ToBudgetHold {
			offered: auto_held == 0,
			block: mixed_scale(data).or_else(|| empty(to_budget)),
			default_amount: to_budget.max(0),
			held,
			auto_held,
			can_reset: held != 0,
		},
	})
}

#[uniffi::export]
impl Session {
	/// v25, docs/finance/to-budget.md TB-10.1: everything the To Budget card's
	/// popover shows for `month`, with each quick action's plan or the reason
	/// it can't act. `today` is the local civil date, `YYYY-MM-DD`. Writes nothing.
	pub fn to_budget_actions(
		&self,
		month: Month,
		today: String,
	) -> Result<ToBudgetActions, FfiError> {
		self.core.call(|session| {
			let today = crate::budget::civil_date(&today)?;
			session.with_sheet(today, |data, sheet| actions(data, sheet, month))
		})
	}
}

#[cfg(test)]
mod tests {
	#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

	use veritas_domain::{CategoryId, Money};

	use super::*;
	use crate::accounts::TransactionDraft;
	use crate::commands::Command;
	use crate::test_support::imported;

	const TODAY: &str = "2026-11-15";
	const NOVEMBER: Month = Month {
		year: 2026,
		month: 11,
	};

	/// The first two expense envelopes on the Budget screen, and the first
	/// on-budget account.
	fn places(core: &crate::session::Core<veritas_store::Store>) -> (String, String, String) {
		core.call(|session| {
			let data = session.data()?;
			let sheet = EnvelopeSheet::compute(data, crate::budget::civil_date(TODAY)?)?;
			let envelopes: Vec<String> = sheet
				.group_order()
				.filter(|(group, _)| {
					data.category_groups
						.get(*group)
						.is_some_and(|g| !g.is_income && !g.hidden)
				})
				.flat_map(|(_, categories)| categories)
				.filter(|id| {
					data.categories
						.get(*id)
						.is_some_and(|c| !c.hidden && !c.is_income)
				})
				.map(ToString::to_string)
				.collect();
			let account = data
				.accounts
				.values()
				.find(|a| !a.off_budget && !a.closed && !a.tombstone)
				.unwrap()
				.id
				.to_string();
			Ok((envelopes[0].clone(), envelopes[1].clone(), account))
		})
		.unwrap()
	}

	fn spend(
		core: &crate::session::Core<veritas_store::Store>,
		id: &str,
		account: &str,
		category: &str,
		amount: i64,
	) {
		let draft = TransactionDraft {
			id: Some(id.to_owned()),
			account_id: account.to_owned(),
			date: "2026-11-10".to_owned(),
			amount,
			payee_id: None,
			payee_name: None,
			category_id: Some(category.to_owned()),
			notes: None,
			cleared: false,
			splits: Vec::new(),
		};
		core.call(|session| session.perform(&Command::AddTransaction { transaction: draft }))
			.unwrap();
	}

	fn read(core: &crate::session::Core<veritas_store::Store>) -> ToBudgetActions {
		core.call(|session| {
			session.with_sheet(crate::budget::civil_date(TODAY)?, |data, sheet| {
				actions(data, sheet, NOVEMBER)
			})
		})
		.unwrap()
	}

	fn budgeted(core: &crate::session::Core<veritas_store::Store>, category: &str) -> i64 {
		core.call(|session| {
			Ok(session
				.data()?
				.budget_cells
				.get(&(YearMonth::new(2026, 11)?, CategoryId::new(category)))
				.map_or(0, |cell| cell.budgeted.minor()))
		})
		.unwrap()
	}

	fn month_note(core: &crate::session::Core<veritas_store::Store>) -> Option<String> {
		core.call(|session| {
			Ok(session
				.data()?
				.notes
				.get("budget-2026-11")
				.cloned()
				.flatten())
		})
		.unwrap()
	}

	#[test]
	fn tb_8_example_9_assign_to_underfunded_is_one_undo_step() {
		let (_scratch, core) = imported("small");
		core.pin_today(TODAY).unwrap();
		let (first, second, account) = places(&core);
		spend(&core, "tb-over-1", &account, &first, -1_000_000);
		spend(&core, "tb-over-2", &account, &second, -1_000_000);
		let before = read(&core);
		assert!(
			before.to_budget > 0,
			"small.zip has money to budget in November"
		);
		assert_eq!(before.state, ToBudgetState::Positive);
		assert_eq!(before.assign.block, None);
		assert!(
			before
				.assign
				.fills
				.iter()
				.any(|fill| fill.category_id == first
					&& fill.reason == ToBudgetFillReason::Overspent)
		);
		let (first_before, second_before, note_before) = (
			budgeted(&core, &first),
			budgeted(&core, &second),
			month_note(&core),
		);
		assert_eq!(read(&core), before, "TB-1.3: the read writes nothing");

		core.call(|session| session.perform(&Command::AssignToUnderfunded { month: NOVEMBER }))
			.unwrap();
		let after = read(&core);
		assert_eq!(
			after.to_budget,
			before.to_budget - before.assign.total,
			"TB-2.2: To Budget falls by exactly what was written"
		);
		assert!(after.to_budget >= 0);
		assert!(budgeted(&core, &first) > first_before);
		let note = month_note(&core).unwrap();
		let added = note
			.strip_prefix(note_before.as_deref().unwrap_or(""))
			.unwrap()
			.trim_start_matches('\n');
		let covered = before
			.assign
			.fills
			.iter()
			.filter(|fill| fill.reason == ToBudgetFillReason::Overspent)
			.count();
		assert_eq!(
			added.lines().count(),
			covered,
			"TB-10.3: a movement note per covered envelope, appended"
		);

		core.call(|session| session.undo()).unwrap();
		assert_eq!(budgeted(&core, &first), first_before, "TB-8 example 9");
		assert_eq!(budgeted(&core, &second), second_before);
		assert_eq!(month_note(&core), note_before, "the notes return exactly");
		assert_eq!(
			read(&core).to_budget,
			before.to_budget,
			"one undo step undoes the whole action"
		);
	}

	#[test]
	fn a_blocked_row_is_a_command_the_core_refuses() {
		let (_scratch, core) = imported("small");
		core.pin_today(TODAY).unwrap();
		let november = read(&core);
		if november.assign.block == Some(ToBudgetBlock::NothingUnderfunded) {
			let refused = core
				.call(|session| session.perform(&Command::AssignToUnderfunded { month: NOVEMBER }));
			assert!(refused.is_err(), "Nothing is underfunded");
		}
		let october = Month {
			year: 2026,
			month: 10,
		};
		let refused =
			core.call(|session| session.perform(&Command::FundGoalsMonthly { month: october }));
		assert!(
			matches!(refused, Err(FfiError::Invalid { message }) if message == "Goals are funded for the current month only."),
			"TB-8 example 8"
		);
		let other = core
			.call(|session| {
				session.with_sheet(crate::budget::civil_date(TODAY)?, |data, sheet| {
					actions(data, sheet, october)
				})
			})
			.unwrap();
		assert_eq!(other.fund_goals.block, Some(ToBudgetBlock::NotCurrentMonth));
		assert_ne!(
			other.assign.block,
			Some(ToBudgetBlock::NotCurrentMonth),
			"Assign to underfunded stays available in another month"
		);
	}

	#[test]
	fn tb_8_example_7_a_currency_without_two_places_disables_all_three() {
		let (_scratch, core) = imported("small");
		core.pin_today(TODAY).unwrap();
		let (block, sheet_to_budget, hold, reset) = core
			.call(|session| {
				let mut data = session.data()?.clone();
				data.format.currency_code = Some("JPY".to_owned());
				let sheet = EnvelopeSheet::compute(&data, crate::budget::civil_date(TODAY)?)?;
				let read = actions(&data, &sheet, NOVEMBER)?;
				let plan = |command: Command| {
					veritas_session::Command::plan_with_sheet(&command, &data, &sheet)
				};
				Ok((
					(read.assign.block, read.fund_goals.block, read.hold.block),
					sheet
						.summary(YearMonth::new(2026, 11)?)
						.map(|s| s.to_budget),
					plan(Command::HoldForNextMonth {
						month: NOVEMBER,
						amount: 1_000,
					})
					.map_err(|refused| refused.to_string()),
					plan(Command::ResetHold { month: NOVEMBER }).is_ok(),
				))
			})
			.unwrap();
		let yen = Some(ToBudgetBlock::MixedScale { places: 0 });
		assert_eq!(block, (yen.clone(), yen.clone(), yen), "TB-2.6");
		assert!(sheet_to_budget.is_some_and(|figure| figure != Money::ZERO));
		assert_eq!(
			hold.err().as_deref(),
			Some(
				"Spending is recorded at 2 places and budgets at 0, so To Budget can't be trusted here."
			),
			"TB-5.4: the Hold command is refused with the block the row shows"
		);
		assert!(reset, "TB-5.4: Reset only sets the hold to 0");
		assert_eq!(
			refusal(&ToBudgetBlock::MixedScale { places: 3 }).to_string(),
			"Spending is recorded at 2 places and budgets at 3, so To Budget can't be trusted here."
		);
	}

	#[test]
	fn tb_2_3_a_row_with_only_hidden_envelopes_still_counts_them() {
		let (_scratch, core) = imported("small");
		core.pin_today(TODAY).unwrap();
		let (first, _, account) = places(&core);
		spend(&core, "tb-hidden-1", &account, &first, -1_000_000);
		let assign = core
			.call(|session| {
				let mut data = session.data()?.clone();
				for category in data.categories.values_mut() {
					category.hidden = true;
				}
				let sheet = EnvelopeSheet::compute(&data, crate::budget::civil_date(TODAY)?)?;
				Ok(actions(&data, &sheet, NOVEMBER)?.assign)
			})
			.unwrap();
		assert_eq!(assign.block, Some(ToBudgetBlock::NothingUnderfunded));
		assert!(assign.fills.is_empty());
		assert_eq!((assign.envelopes, assign.total), (0, 0));
		assert!(
			assign.hidden_left_out >= 1,
			"the overspent hidden envelope is counted, not filled"
		);
	}

	#[test]
	fn tb_1_3_an_empty_to_budget_names_its_state() {
		assert_eq!(empty(0), Some(ToBudgetBlock::Zero));
		assert_eq!(
			empty(-5_000),
			Some(ToBudgetBlock::Overbudgeted { amount: 5_000 })
		);
		assert_eq!(empty(1), None);
	}
}
