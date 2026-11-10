use serde_json::json;
use veritas_domain::{
	AccountId, Cadence, CategoryId, CivilDate, CoreError, GoalId, IncomeDelta, Money, Scenario,
	ScenarioChange, ScenarioEvent, YearMonth,
};
use veritas_rules::Recurrence;

use crate::math::{round_half_away, to_money};

/// Scenarios compared against the baseline at once (R-FC-2).
pub const MAX_COMPARED: usize = 3;

/// Dates of a cadence from `start` through `last`, with the recurrence bills
/// use (`veritas_rules::Recurrence`), anchored on the start and never on a
/// weekend move.
pub fn cadence_dates(
	cadence: Cadence,
	start: CivilDate,
	last: CivilDate,
) -> Result<Vec<CivilDate>, CoreError> {
	let (frequency, interval) = match cadence {
		Cadence::EveryMonths(n) => ("monthly", n),
		Cadence::EveryWeeks(n) => ("weekly", n),
	};
	if interval == 0 {
		return Err(CoreError::Invalid(
			"a recurring change repeats at least every 1 month or week".to_owned(),
		));
	}
	let invalid = |error: veritas_rules::RecurError| CoreError::Invalid(error.to_string());
	let recurrence = Recurrence::parse(&json!({
		"frequency": frequency,
		"interval": interval,
		"start": start.to_string(),
		"endMode": "never",
	}))
	.map_err(invalid)?;
	let mut dates = Vec::new();
	let mut from = start.naive();
	while let Some(date) = recurrence.first_on_or_after(from).map_err(invalid)? {
		if date > last.naive() {
			break;
		}
		dates.push(CivilDate::from(date));
		match date.succ_opt() {
			Some(next) => from = next,
			None => break,
		}
	}
	Ok(dates)
}

/// Checks a scenario before it overlays anything. A what-if is about the
/// future: a one-time change dated before `today` is rejected, not moved.
pub fn validate(scenario: &Scenario, today: CivilDate) -> Result<(), CoreError> {
	let invalid = |text: &str| Err(CoreError::Invalid(format!("{}: {text}", scenario.name)));
	for event in &scenario.events {
		let (start, end) = match &event.change {
			ScenarioChange::OneTime { date, .. } => {
				if *date < today {
					return invalid("a one-time change can't be dated before today");
				}
				continue;
			}
			ScenarioChange::Recurring {
				start,
				end_month,
				cadence,
				..
			} => {
				if matches!(cadence, Cadence::EveryMonths(0) | Cadence::EveryWeeks(0)) {
					return invalid("a recurring change repeats at least every 1 month or week");
				}
				(start.year_month(), *end_month)
			}
			ScenarioChange::CategoryPercent {
				start_month,
				end_month,
				..
			}
			| ScenarioChange::IncomeChange {
				start_month,
				end_month,
				..
			}
			| ScenarioChange::GoalContribution {
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

/// A scenario's events read for one projection; the baseline has none.
#[derive(Clone, Copy)]
pub(crate) struct Overlay<'a> {
	events: &'a [ScenarioEvent],
}

impl<'a> Overlay<'a> {
	pub fn new(scenario: Option<&'a Scenario>) -> Overlay<'a> {
		Overlay {
			events: scenario.map_or(&[], |scenario| scenario.events.as_slice()),
		}
	}

	fn changes(self) -> impl Iterator<Item = &'a ScenarioChange> {
		self.events.iter().map(|event| &event.change)
	}

	/// One-time and recurring flows dated `today` through `last`. A recurring
	/// change that started earlier keeps its anchor and contributes from today.
	pub fn flows(self, today: CivilDate, last: CivilDate) -> Result<Vec<ScenarioFlow>, CoreError> {
		let mut flows = Vec::new();
		for change in self.changes() {
			match change {
				ScenarioChange::OneTime {
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
				ScenarioChange::Recurring {
					start,
					end_month,
					cadence,
					account,
					amount,
					..
				} => {
					for date in cadence_dates(*cadence, *start, last)? {
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
	pub fn adjust_baseline(
		self,
		category: &CategoryId,
		month: YearMonth,
		forecast: Money,
	) -> Result<Money, CoreError> {
		let forecast = forecast.max(Money::ZERO);
		let mut adjusted = forecast;
		for change in self.changes() {
			if let ScenarioChange::CategoryPercent {
				category: changed,
				percent_bp,
				start_month,
				end_month,
			} = change && changed == category
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

	/// Income changes active in `month`.
	pub fn income_changes(
		self,
		month: YearMonth,
	) -> impl Iterator<Item = (&'a AccountId, Option<&'a CategoryId>, IncomeDelta)> {
		self.changes().filter_map(move |change| match change {
			ScenarioChange::IncomeChange {
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
	pub fn goal_contribution(self, goal: &GoalId, month: YearMonth) -> Result<Money, CoreError> {
		Money::checked_sum(self.changes().filter_map(|change| match change {
			ScenarioChange::GoalContribution {
				goal: target,
				amount,
				start_month,
				end_month,
			} if target == goal && active(month, *start_month, *end_month) => Some(*amount),
			_ => None,
		}))
	}

	/// The start and end month of each contribution to `goal`.
	pub fn goal_contribution_months(self, goal: &GoalId) -> Vec<(YearMonth, Option<YearMonth>)> {
		self.changes()
			.filter_map(|change| match change {
				ScenarioChange::GoalContribution {
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
