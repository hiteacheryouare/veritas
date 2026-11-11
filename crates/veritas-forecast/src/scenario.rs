use chrono::{Days, Months};
use veritas_domain::{AccountId, CategoryId, CivilDate, CoreError, GoalId, Money, YearMonth};

use crate::math::{round_half_away, to_money};

/// Scenarios compared against the baseline at once (R-FC-2).
pub const MAX_COMPARED: usize = 3;

/// A recurrence anchored on its start date, as bills repeat: the k-th date is
/// `start + k * n` months or weeks, never chained from the previous date, so
/// a month-end anchor comes back to the month end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cadence {
	EveryMonths(u32),
	EveryWeeks(u32),
}

impl Cadence {
	/// Dates from `start` through `last`, inclusive.
	pub fn dates(self, start: CivilDate, last: CivilDate) -> Result<Vec<CivilDate>, CoreError> {
		let step = match self {
			Cadence::EveryMonths(n) | Cadence::EveryWeeks(n) => n,
		};
		if step == 0 {
			return Err(CoreError::Invalid(
				"a recurring change repeats at least every 1 month or week".to_owned(),
			));
		}
		let mut dates = Vec::new();
		for k in 0u32.. {
			let offset = k.checked_mul(step).ok_or(CoreError::Overflow)?;
			let date = match self {
				Cadence::EveryMonths(_) => start.naive().checked_add_months(Months::new(offset)),
				Cadence::EveryWeeks(_) => start
					.naive()
					.checked_add_days(Days::new(u64::from(offset) * 7)),
			}
			.map(CivilDate::from)
			.ok_or(CoreError::Overflow)?;
			if date > last {
				break;
			}
			dates.push(date);
		}
		Ok(dates)
	}
}

/// What an income change does to a month's income.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IncomeDelta {
	/// Added each month, signed (a raise is positive).
	Amount(Money),
	/// Basis points of the month's expected income, signed.
	Percent(i64),
}

/// One overlay line (`veritas_scenario_event`). Amounts use Actual's sign:
/// positive is money in, and every amount changes net worth.
#[derive(Clone, Debug, PartialEq)]
pub enum ScenarioEvent {
	OneTime {
		date: CivilDate,
		account: AccountId,
		category: Option<CategoryId>,
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
	/// A change to expected income, for one income category or, with no
	/// category, for all of it. The difference posts to `account`.
	IncomeChange {
		account: AccountId,
		category: Option<CategoryId>,
		change: IncomeDelta,
		start_month: YearMonth,
		end_month: Option<YearMonth>,
	},
	/// Extra money budgeted to a goal each month. A budget move, so it
	/// changes the goal's ETA and never a balance or net worth.
	GoalContribution {
		goal: GoalId,
		amount: Money,
		start_month: YearMonth,
		end_month: Option<YearMonth>,
	},
}

/// A named what-if (R-FC-2): an overlay on the baseline that never touches
/// the budget it reads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scenario {
	pub name: String,
	pub events: Vec<ScenarioEvent>,
}

/// A dated scenario cash flow.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ScenarioFlow {
	pub date: CivilDate,
	pub account: AccountId,
	pub amount: Money,
}

fn active(month: YearMonth, start: YearMonth, end: Option<YearMonth>) -> bool {
	start <= month && end.is_none_or(|end| month <= end)
}

impl Scenario {
	pub fn baseline() -> Scenario {
		Scenario::default()
	}

	/// A what-if is about the future: a one-time event dated before `today`
	/// is rejected rather than moved.
	pub fn validate(&self, today: CivilDate) -> Result<(), CoreError> {
		let invalid = |text: &str| Err(CoreError::Invalid(format!("{}: {text}", self.name)));
		for event in &self.events {
			let (start, end) = match event {
				ScenarioEvent::OneTime { date, .. } => {
					if *date < today {
						return invalid("a one-time change can't be dated before today");
					}
					continue;
				}
				ScenarioEvent::Recurring {
					start,
					end_month,
					cadence,
					..
				} => {
					if matches!(cadence, Cadence::EveryMonths(0) | Cadence::EveryWeeks(0)) {
						return invalid(
							"a recurring change repeats at least every 1 month or week",
						);
					}
					(start.year_month(), *end_month)
				}
				ScenarioEvent::CategoryPercent {
					start_month,
					end_month,
					..
				}
				| ScenarioEvent::IncomeChange {
					start_month,
					end_month,
					..
				}
				| ScenarioEvent::GoalContribution {
					start_month,
					end_month,
					..
				} => (*start_month, *end_month),
			};
			if end.is_some_and(|end| end < start) {
				return invalid("a change ends before it starts");
			}
		}
		Ok(())
	}

	/// One-time and recurring flows dated `today` through `last`. A recurring
	/// change that started earlier keeps its anchor and contributes from today.
	pub(crate) fn flows(
		&self,
		today: CivilDate,
		last: CivilDate,
	) -> Result<Vec<ScenarioFlow>, CoreError> {
		let mut flows = Vec::new();
		for event in &self.events {
			match event {
				ScenarioEvent::OneTime {
					date,
					account,
					amount,
					..
				} => {
					if today <= *date && *date <= last {
						flows.push(ScenarioFlow {
							date: *date,
							account: account.clone(),
							amount: *amount,
						});
					}
				}
				ScenarioEvent::Recurring {
					start,
					end_month,
					cadence,
					account,
					amount,
					..
				} => {
					for date in cadence.dates(*start, last)? {
						if today <= date && end_month.is_none_or(|end| date.year_month() <= end) {
							flows.push(ScenarioFlow {
								date,
								account: account.clone(),
								amount: *amount,
							});
						}
					}
				}
				_ => {}
			}
		}
		Ok(flows)
	}

	/// FC-3: each active change adds `round_half_away(forecast+ * pct / 10000)`
	/// to the same unadjusted `forecast+`, so changes add and never compound.
	pub(crate) fn adjust_baseline(
		&self,
		category: &CategoryId,
		month: YearMonth,
		forecast: Money,
	) -> Result<Money, CoreError> {
		let forecast = forecast.max(Money::ZERO);
		let mut adjusted = forecast;
		for event in &self.events {
			if let ScenarioEvent::CategoryPercent {
				category: changed,
				percent_bp,
				start_month,
				end_month,
			} = event && changed == category
				&& active(month, *start_month, *end_month)
			{
				let delta = to_money(round_half_away(
					i128::from(forecast.minor()) * i128::from(*percent_bp),
					10_000,
				))?;
				adjusted = adjusted.checked_add(delta)?;
			}
		}
		Ok(adjusted.max(Money::ZERO))
	}

	/// Income changes active in `month`, each with the income it applies to.
	pub(crate) fn income_changes(
		&self,
		month: YearMonth,
	) -> impl Iterator<Item = (&AccountId, Option<&CategoryId>, IncomeDelta)> {
		self.events.iter().filter_map(move |event| match event {
			ScenarioEvent::IncomeChange {
				account,
				category,
				change,
				start_month,
				end_month,
			} if active(month, *start_month, *end_month) => Some((account, category.as_ref(), *change)),
			_ => None,
		})
	}

	/// Extra contribution to `goal` in `month`.
	pub fn goal_contribution(&self, goal: &GoalId, month: YearMonth) -> Result<Money, CoreError> {
		Money::checked_sum(self.events.iter().filter_map(|event| match event {
			ScenarioEvent::GoalContribution {
				goal: target,
				amount,
				start_month,
				end_month,
			} if target == goal && active(month, *start_month, *end_month) => Some(*amount),
			_ => None,
		}))
	}

	/// The start and end month of each contribution to `goal`.
	pub(crate) fn goal_contribution_months(
		&self,
		goal: &GoalId,
	) -> Vec<(YearMonth, Option<YearMonth>)> {
		self.events
			.iter()
			.filter_map(|event| match event {
				ScenarioEvent::GoalContribution {
					goal: target,
					start_month,
					end_month,
					..
				} if target == goal => Some((*start_month, *end_month)),
				_ => None,
			})
			.collect()
	}
}
