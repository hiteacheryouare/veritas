//! What-if scenarios (R-FC-2), stored in `veritas_scenario` and
//! `veritas_scenario_event`. They overlay a forecast and never touch real data.

use crate::{
	AccountId, CategoryId, CivilDate, GoalId, Money, ScenarioEventId, ScenarioId, YearMonth,
};

#[derive(Clone, Debug, PartialEq)]
pub struct Scenario {
	pub id: ScenarioId,
	pub name: String,
	pub sort_order: f64,
	/// Unix milliseconds.
	pub created_at: i64,
	/// Ordered by id.
	pub events: Vec<ScenarioEvent>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScenarioEvent {
	pub id: ScenarioEventId,
	pub change: ScenarioChange,
}

/// A recurrence anchored on its start date, as bills repeat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cadence {
	EveryMonths(u32),
	EveryWeeks(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IncomeDelta {
	/// Added each month, signed (a raise is positive).
	Amount(Money),
	/// Basis points of the month's expected income, signed.
	Percent(i64),
}

/// One overlay line. Amounts use Actual's sign: positive is money in, and
/// every amount changes net worth.
#[derive(Clone, Debug, PartialEq)]
pub enum ScenarioChange {
	OneTime {
		date: CivilDate,
		account: AccountId,
		category: Option<CategoryId>,
		amount: Money,
	},
	/// Money moved between two of the user's accounts on one date, such as a
	/// planned purchase of an off-budget asset (FC-3): `from` falls by
	/// `amount`, `to` rises by it, and net worth doesn't move.
	OneTimeTransfer {
		date: CivilDate,
		from: AccountId,
		to: AccountId,
		amount: Money,
	},
	Recurring {
		start: CivilDate,
		/// The last month with an occurrence; open-ended when `None`.
		end_month: Option<YearMonth>,
		cadence: Cadence,
		account: AccountId,
		category: Option<CategoryId>,
		amount: Money,
	},
	/// Changes a category's baseline by `round_half_away(baseline * percent_bp / 10000)`.
	CategoryPercent {
		category: CategoryId,
		percent_bp: i64,
		start_month: YearMonth,
		end_month: Option<YearMonth>,
	},
	/// A change to expected income for one income category, or for all of it
	/// when `category` is `None`. The difference posts to `account`.
	IncomeChange {
		account: AccountId,
		category: Option<CategoryId>,
		change: IncomeDelta,
		start_month: YearMonth,
		end_month: Option<YearMonth>,
	},
	/// Extra money budgeted to a goal each month: a budget move, so it changes
	/// the goal's ETA and never a balance or net worth.
	GoalContribution {
		goal: GoalId,
		amount: Money,
		start_month: YearMonth,
		end_month: Option<YearMonth>,
	},
}

impl ScenarioChange {
	/// The `kind` column value.
	pub fn kind(&self) -> &'static str {
		match self {
			ScenarioChange::OneTime { .. } | ScenarioChange::OneTimeTransfer { .. } => "one_time",
			ScenarioChange::Recurring { .. } => "recurring",
			ScenarioChange::CategoryPercent { .. } => "category_percent",
			ScenarioChange::IncomeChange { .. } => "income_change",
			ScenarioChange::GoalContribution { .. } => "goal_contribution",
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn every_kind_is_one_the_schema_accepts() {
		let schema = include_str!("../../../contracts/veritas-schema-v1.sql");
		let check = schema
			.lines()
			.find(|line| line.contains("kind TEXT NOT NULL CHECK (kind IN ('one_time'"))
			.unwrap();
		let month = YearMonth::new(2027, 1).unwrap();
		let changes = [
			ScenarioChange::OneTime {
				date: month.first_day(),
				account: AccountId::new("a"),
				category: None,
				amount: Money::ZERO,
			},
			ScenarioChange::OneTimeTransfer {
				date: month.first_day(),
				from: AccountId::new("a"),
				to: AccountId::new("b"),
				amount: Money::ZERO,
			},
			ScenarioChange::Recurring {
				start: month.first_day(),
				end_month: None,
				cadence: Cadence::EveryWeeks(2),
				account: AccountId::new("a"),
				category: None,
				amount: Money::ZERO,
			},
			ScenarioChange::CategoryPercent {
				category: CategoryId::new("c"),
				percent_bp: 0,
				start_month: month,
				end_month: None,
			},
			ScenarioChange::IncomeChange {
				account: AccountId::new("a"),
				category: None,
				change: IncomeDelta::Percent(0),
				start_month: month,
				end_month: None,
			},
			ScenarioChange::GoalContribution {
				goal: GoalId::new("g"),
				amount: Money::ZERO,
				start_month: month,
				end_month: None,
			},
		];
		for change in changes {
			assert!(
				check.contains(&format!("'{}'", change.kind())),
				"{}",
				change.kind()
			);
		}
	}
}
