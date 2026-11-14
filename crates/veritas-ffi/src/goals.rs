use veritas_domain::{BudgetData, CoreError, GoalId, Money, YearMonth};
use veritas_engine::{Envelope, EnvelopeSheet};
use veritas_session::goals::{
	self, ClosePreview, GoalPace as CorePace, GoalState as CoreState, GoalView,
};

use crate::Month;
use crate::budget::envelope;

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
	/// Net spending from the envelope over all months, at least zero.
	pub spent: i64,
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

pub(crate) fn summaries(
	data: &BudgetData,
	sheet: &EnvelopeSheet,
) -> Result<Vec<GoalSummary>, CoreError> {
	Ok(goals::goals(data, sheet)?
		.into_iter()
		.map(GoalSummary::from)
		.collect())
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

impl From<GoalView> for GoalSummary {
	fn from(view: GoalView) -> Self {
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
			}),
			reached_month: goal.reached_month.map(Month::from),
			closed_month: goal.closed_month.map(Month::from),
			later_budget: month_amounts(view.later_budget),
			new_activity: view.new_activity,
		}
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
