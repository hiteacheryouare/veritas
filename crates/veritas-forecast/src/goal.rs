use veritas_domain::{BudgetData, CoreError, GoalId, Money, YearMonth};

use crate::baseline::months_between;
use crate::math::{ceil_div, median};
use crate::scenario::Scenario;

const PACE_MONTHS: i32 = 3;
/// ETAs further out than 100 years are reported as too far, not computed.
const MAX_MONTHS: i32 = 1200;

enum Reach {
	At(YearMonth),
	Never,
	TooFar,
}

impl Reach {
	fn month(self) -> Option<YearMonth> {
		match self {
			Reach::At(month) => Some(month),
			Reach::Never | Reach::TooFar => None,
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EtaStatus {
	Reached,
	/// No complete month of contributions yet.
	NoHistory,
	/// The median pace is 0 or less.
	NotAdding,
	/// The median pace takes more than 100 years.
	TooFar,
	Eta {
		month: YearMonth,
		optimistic: YearMonth,
		/// `None` when the slowest pace adds nothing.
		pessimistic: Option<YearMonth>,
	},
}

/// FC-6's goal ETA from the contribution pace (R-FC-4).
#[derive(Clone, Debug, PartialEq)]
pub struct GoalEta {
	pub goal: GoalId,
	pub remaining: Money,
	/// Median, maximum and minimum of the last up to 3 complete months.
	pub pace: Option<(Money, Money, Money)>,
	pub history_months: usize,
	pub status: EtaStatus,
	/// Whether the ETA is not after the target month, when there's a target date.
	pub on_track: Option<bool>,
}

impl GoalEta {
	pub fn text(&self) -> String {
		match self.status {
			EtaStatus::Reached => "Reached".to_owned(),
			EtaStatus::NoHistory => "No ETA yet".to_owned(),
			EtaStatus::NotAdding => "No ETA: nothing is being added".to_owned(),
			EtaStatus::TooFar => "No ETA: more than 100 years at this pace".to_owned(),
			EtaStatus::Eta {
				month,
				optimistic,
				pessimistic,
			} => match pessimistic {
				Some(pessimistic) => format!(
					"ETA {}, range {} to {}",
					month_name(month),
					month_name(optimistic),
					month_name(pessimistic)
				),
				None => format!("ETA {}, no ETA at the slowest pace", month_name(month)),
			},
		}
	}

	pub fn history_label(&self) -> String {
		match self.history_months {
			1 => "1 month of history".to_owned(),
			n => format!("{n} months of history"),
		}
	}
}

pub fn month_name(month: YearMonth) -> String {
	const NAMES: [&str; 12] = [
		"January",
		"February",
		"March",
		"April",
		"May",
		"June",
		"July",
		"August",
		"September",
		"October",
		"November",
		"December",
	];
	let name = usize::try_from(month.month())
		.ok()
		.and_then(|month| NAMES.get(month.wrapping_sub(1)))
		.copied()
		.unwrap_or_default();
	format!("{name} {}", month.year())
}

/// `available` is the goal envelope's available balance for `current` from
/// the envelope sheet, so `current`'s own budgeted amount is already in it.
pub fn goal_eta(
	budget: &BudgetData,
	goal: &GoalId,
	current: YearMonth,
	available: Money,
	scenario: &Scenario,
) -> Result<GoalEta, CoreError> {
	let found = budget.goals.get(goal).ok_or_else(|| CoreError::NotFound {
		entity: "goal",
		id: goal.to_string(),
	})?;
	let remaining = found.target.checked_sub(available)?.max(Money::ZERO);
	let first = budget
		.budget_cells
		.values()
		.filter(|cell| cell.category == found.category && cell.month < current)
		.map(|cell| cell.month)
		.min();
	let contributions: Vec<i64> = match first {
		Some(first) => {
			let span = months_between(first, current)?.min(PACE_MONTHS);
			(1..=span)
				.rev()
				.map(|back| {
					let month = current.offset(-back)?;
					Ok(budget
						.budget_cells
						.get(&(month, found.category.clone()))
						.map_or(0, |cell| cell.budgeted.minor()))
				})
				.collect::<Result<_, CoreError>>()?
		}
		None => Vec::new(),
	};
	let pace = match (
		median(&contributions),
		contributions.iter().max(),
		contributions.iter().min(),
	) {
		(Some(mid), Some(max), Some(min)) => Some((mid, *max, *min)),
		_ => None,
	};
	let eta = |pace: i64| months_to_go(remaining, pace, goal, current, scenario);
	let status = match pace {
		_ if remaining == Money::ZERO => EtaStatus::Reached,
		None => EtaStatus::NoHistory,
		Some((mid, max, min)) => match eta(mid)? {
			Reach::Never => EtaStatus::NotAdding,
			Reach::TooFar => EtaStatus::TooFar,
			Reach::At(month) => EtaStatus::Eta {
				month,
				optimistic: eta(max)?.month().unwrap_or(month),
				pessimistic: eta(min)?.month(),
			},
		},
	};
	let on_track = match (status, found.target_date) {
		(EtaStatus::Reached, Some(_)) => Some(true),
		(EtaStatus::Eta { month, .. }, Some(target)) => Some(month <= target.year_month()),
		_ => None,
	};
	Ok(GoalEta {
		goal: goal.clone(),
		remaining,
		pace: pace.map(|(mid, max, min)| {
			(
				Money::from_minor(mid),
				Money::from_minor(max),
				Money::from_minor(min),
			)
		}),
		history_months: contributions.len(),
		status,
		on_track,
	})
}

/// `ceil(remaining / pace)` months after `current`. A scenario's extra
/// contributions are added month by month until the last one changes, then
/// the steady pace finishes the count.
fn months_to_go(
	remaining: Money,
	pace: i64,
	goal: &GoalId,
	current: YearMonth,
	scenario: &Scenario,
) -> Result<Reach, CoreError> {
	let remaining = i128::from(remaining.minor());
	let mut simulated = 0i32;
	let mut saved = 0i128;
	for (start, end) in scenario.goal_contribution_months(goal) {
		let last = end.unwrap_or(start);
		let span = months_between(current, last)?.min(MAX_MONTHS - 1) + 1;
		simulated = simulated.max(span);
	}
	for step in 1..=simulated {
		let month = current.offset(step)?;
		saved += i128::from(pace) + i128::from(scenario.goal_contribution(goal, month)?.minor());
		if saved >= remaining {
			return Ok(Reach::At(month));
		}
	}
	let after = current.offset(simulated + 1)?;
	let steady = i128::from(pace) + i128::from(scenario.goal_contribution(goal, after)?.minor());
	if steady <= 0 {
		return Ok(Reach::Never);
	}
	let total = i128::from(simulated) + ceil_div(remaining - saved, steady);
	if total > i128::from(MAX_MONTHS) {
		return Ok(Reach::TooFar);
	}
	let total = i32::try_from(total).map_err(|_| CoreError::Overflow)?;
	current.offset(total).map(Reach::At)
}
