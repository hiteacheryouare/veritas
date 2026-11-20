use veritas_domain::{BudgetData, CoreError, GoalId, Money, YearMonth};
use veritas_engine::{Envelope, EnvelopeSheet};
use veritas_reports::Classification;
use veritas_session::goals::{
	self, ClosePreview, GoalPace as CorePace, GoalState as CoreState, GoalView,
};

use crate::Month;
use crate::budget::{MoneyTone, envelope};

/// What a goal card shows (DEBT-8, D66).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum GoalState {
	Active,
	Reached,
	/// "Reached → Spent": no suggestion or ETA until reopened or re-targeted.
	ReachedSpent,
	Closed,
}

/// "On track for March" or "Behind, needs $410 a month" (FC-6, DEBT-8.1).
/// `neededMonthly` is DEBT-8.1's monthly amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum GoalPace {
	OnTrack {
		eta_month: Month,
	},
	/// The ETA is after the target month.
	Behind {
		needed_monthly: i64,
	},
	/// No complete month of budget yet: a brand-new goal.
	NoHistory {
		needed_monthly: i64,
	},
	/// Nothing added lately at the median pace.
	NotAdding {
		needed_monthly: i64,
	},
	/// v16, D77: "No ETA: spending recorded", with no needed figure, in a
	/// neutral tone. Agrees with `GoalEtaStatus.spendingRecorded`.
	SpendingRecorded,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MonthAmount {
	pub month: Month,
	pub amount: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct GoalSummary {
	pub id: String,
	pub category_id: String,
	pub name: String,
	pub target: i64,
	/// `YYYY-MM-DD`; none means no suggestion (DEBT-8 Months left).
	pub target_date: Option<String>,
	pub icon: Option<String>,
	pub color: Option<String>,
	pub state: GoalState,
	/// The envelope's available balance this month.
	pub available: i64,
	/// DEBT-8's Spending to date: CLS-6 `expense_total` for the category
	/// through today, the figure every report shows. Not clamped: refunds
	/// can make it negative, shown with a true minus (ST-1.2). On the
	/// register scale in a 0-place budget (CLS-12.14).
	pub spent: i64,
	/// The card's colour: on track, behind, reached, spent past zero.
	pub tone: MoneyTone,
	/// 0 to 100 for the ring and the number beside it, rounded down. 100
	/// only for `reached` and `reachedSpent`.
	pub progress_percent: u8,
	/// The "X of target" figure: `available`, or the target once a reached
	/// goal has been spent from.
	pub progress_amount: i64,
	/// DEBT-8.1, for active and reached goals with a target date.
	pub suggested_monthly: Option<i64>,
	/// Budget months through the target month, this one included.
	pub months_left: Option<u32>,
	/// Active goals with a target date only, by FC-6's on-track rule, so the
	/// card agrees with the forecast's goal ETA.
	pub pace: Option<GoalPace>,
	/// The first month it was reached; kept through a re-target and a second
	/// reach (D71).
	pub reached_month: Option<Month>,
	pub closed_month: Option<Month>,
	/// A closed goal's budget in later months: "closed goal has budget"
	/// (GOAL-10).
	pub later_budget: Vec<MonthAmount>,
	/// A closed goal whose envelope moved after closing: "Closed goal has new
	/// activity" (GOAL-6).
	pub new_activity: bool,
}

/// v12. The Goals screen (R-GOAL-1/3/4): every goal, and what a 0-place
/// budget changes in how they read (CLS-12.10, CLS-12.14).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct GoalList {
	pub goals: Vec<GoalSummary>,
	/// CLS-12.10's line, set only in a 0-place budget, for any view that
	/// shows a goal's Available or progress.
	pub mixed_scale_note: Option<String>,
	/// A 0-place budget (CLS-12.14): a goal's `available` and its "X of
	/// target" figure are labeled only "Available", whatever their sign or
	/// tone, never "over", "left" or "remaining", and Spending to date sits
	/// on its own row, apart from Available.
	pub zero_places: bool,
}

/// What closing a goal will do, for the confirmation sheet (DEBT-8.3).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct GoalClosePreview {
	/// Moved from the goal to the destination this month. Negative when the
	/// goal is overspent and the destination covers it.
	pub leftover: i64,
	/// Budget in later months, for the unchecked "Also move those to …".
	pub later_budget: Vec<MonthAmount>,
	/// The destination's available this month after closing, or To Budget's.
	/// Negative means "{destination} will be $X over" (GOAL-3, GOAL-8).
	pub destination_available_after: i64,
}

pub(crate) fn goal_list(
	data: &BudgetData,
	sheet: &EnvelopeSheet,
	classification: &Classification,
) -> Result<GoalList, CoreError> {
	let zero_places = data.format.decimal_places() == 0;
	Ok(GoalList {
		goals: goals::goals(data, sheet, classification)?
			.into_iter()
			.map(|view| summary(view, zero_places))
			.collect(),
		mixed_scale_note: crate::budget::mixed_scale_note(data),
		zero_places,
	})
}

pub(crate) fn close_preview(
	data: &BudgetData,
	sheet: &EnvelopeSheet,
	goal_id: &str,
	release_to_category_id: &Option<String>,
) -> Result<GoalClosePreview, CoreError> {
	let release_to: Envelope = envelope(release_to_category_id);
	let ClosePreview {
		leftover,
		later_budget,
		destination_after,
	} = goals::close_preview(data, sheet, &GoalId::new(goal_id), &release_to)?;
	Ok(GoalClosePreview {
		leftover: leftover.minor(),
		later_budget: month_amounts(later_budget),
		destination_available_after: destination_after.minor(),
	})
}

/// In a 0-place budget `available` mixes register and budget scales, so a
/// negative one isn't shown as overspent (CLS-12.14: never "over").
fn tone(view: &GoalView, zero_places: bool) -> MoneyTone {
	match view.state {
		CoreState::Active if view.available < Money::ZERO && zero_places => MoneyTone::Neutral,
		CoreState::Active if view.available < Money::ZERO => MoneyTone::Overspent,
		CoreState::Active => match view.pace {
			None | Some(CorePace::OnTrack { .. }) => MoneyTone::Available,
			Some(CorePace::Behind { .. } | CorePace::NotAdding { .. }) => MoneyTone::Low,
			Some(CorePace::SpendingRecorded) => MoneyTone::Neutral,
			Some(CorePace::NoHistory { .. }) => MoneyTone::Neutral,
		},
		CoreState::Reached => MoneyTone::Available,
		CoreState::ReachedSpent if view.available < Money::ZERO && zero_places => {
			MoneyTone::Neutral
		}
		CoreState::ReachedSpent if view.available < Money::ZERO => MoneyTone::Overspent,
		CoreState::ReachedSpent => MoneyTone::Available,
		CoreState::Closed => MoneyTone::Neutral,
	}
}

fn summary(view: GoalView, zero_places: bool) -> GoalSummary {
	let tone = tone(&view, zero_places);
	let goal = view.goal;
	GoalSummary {
		id: goal.id.to_string(),
		category_id: goal.category.to_string(),
		name: view.name,
		target: goal.target.minor(),
		target_date: goal.target_date.map(|date| date.to_string()),
		icon: goal.icon,
		color: goal.color,
		state: match view.state {
			CoreState::Active => GoalState::Active,
			CoreState::Reached => GoalState::Reached,
			CoreState::ReachedSpent => GoalState::ReachedSpent,
			CoreState::Closed => GoalState::Closed,
		},
		available: view.available.minor(),
		spent: view.spent.minor(),
		tone,
		progress_percent: view.progress_percent,
		progress_amount: view.progress_amount.minor(),
		suggested_monthly: view.suggestion.map(|s| s.monthly.minor()),
		months_left: view.suggestion.map(|s| s.months_left),
		pace: view.pace.map(|pace| match pace {
			CorePace::OnTrack { eta_month } => GoalPace::OnTrack {
				eta_month: eta_month.into(),
			},
			CorePace::Behind { needed_monthly } => GoalPace::Behind {
				needed_monthly: needed_monthly.minor(),
			},
			CorePace::NoHistory { needed_monthly } => GoalPace::NoHistory {
				needed_monthly: needed_monthly.minor(),
			},
			CorePace::NotAdding { needed_monthly } => GoalPace::NotAdding {
				needed_monthly: needed_monthly.minor(),
			},
			CorePace::SpendingRecorded => GoalPace::SpendingRecorded,
		}),
		reached_month: goal.reached_month.map(Month::from),
		closed_month: goal.closed_month.map(Month::from),
		later_budget: month_amounts(view.later_budget),
		new_activity: view.new_activity,
	}
}

fn month_amounts(amounts: Vec<(YearMonth, Money)>) -> Vec<MonthAmount> {
	amounts
		.into_iter()
		.map(|(month, amount)| MonthAmount {
			month: month.into(),
			amount: amount.minor(),
		})
		.collect()
}
