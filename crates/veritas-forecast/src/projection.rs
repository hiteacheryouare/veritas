use std::collections::{BTreeMap, BTreeSet};

use veritas_domain::{
	AccountId, BudgetData, CategoryId, CivilDate, CoreError, Money, ScheduleId, YearMonth,
};
use veritas_reports::{Classification, Decision, Period, classify};

use crate::band::{Band, band};
use crate::baseline::{Baseline, WINDOW, complete_months};
use crate::flows::{FlowKind, KnownFlow};
use crate::history::{History, Series, Side, balances, bill_linked, net_worth};
use crate::math::{round_half_away, to_money};
use crate::scenario::{IncomeDelta, MAX_COMPARED, Scenario};

/// FC-3's horizons, in complete months after the current month.
pub const HORIZONS: [u32; 4] = [3, 6, 10, 12];
const LAST_HORIZON: i32 = 12;

#[derive(Clone, Debug, PartialEq)]
pub struct MonthForecast {
	pub month: YearMonth,
	/// Net income from known flows (income minus expenses).
	pub known_net_income: Money,
	/// `F_t`: baselined income minus baselined spending, the uncertain part.
	pub uncertain_net_flow: Money,
	pub scenario_net_income: Money,
	pub net_income: Money,
	/// At the end of the month.
	pub net_worth: Money,
	pub balances: BTreeMap<AccountId, Money>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HorizonForecast {
	pub months: u32,
	pub month: YearMonth,
	pub net_worth: Money,
	/// `[NW + lo, NW + hi]`, absent with no history.
	pub band: Option<(Money, Money)>,
	pub balances: BTreeMap<AccountId, Money>,
}

/// A baseline (or scenario) projection for one "today" (R-FC-1).
#[derive(Clone, Debug, PartialEq)]
pub struct Projection {
	pub scenario: String,
	pub today: CivilDate,
	pub net_worth_today: Money,
	pub balances_today: BTreeMap<AccountId, Money>,
	pub baselines: Vec<Baseline>,
	pub band: Band,
	/// The current month's remainder first, then 12 complete months.
	pub months: Vec<MonthForecast>,
	pub horizons: Vec<HorizonForecast>,
	/// Known flows dated before today, counted as due now.
	pub overdue: Vec<KnownFlow>,
	/// Categories whose unlinked median is at least half an active bill's
	/// amount, where a hand-typed bill may be counted twice (FC-2).
	pub possible_double_counts: Vec<CategoryId>,
	/// On-to-off transfers counted as spending in the baseline windows
	/// (CLS-4 row 8). The projection lowers net worth by them.
	pub capitalized_transfers: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Comparison {
	pub baseline: Projection,
	pub scenarios: Vec<Projection>,
}

/// Reads one budget for every forecast. It only ever borrows the budget, so
/// nothing it computes can change real data.
pub struct Forecaster<'a> {
	pub(crate) budget: &'a BudgetData,
	pub(crate) classification: Classification,
	pub(crate) history: History,
}

impl<'a> Forecaster<'a> {
	pub fn new(budget: &'a BudgetData) -> Result<Forecaster<'a>, CoreError> {
		let classification = classify(budget);
		let history = History::new(budget, &classification)?;
		Ok(Forecaster {
			budget,
			classification,
			history,
		})
	}

	pub fn classification(&self) -> &Classification {
		&self.classification
	}

	/// The series that get a baseline: income categories fed by an income
	/// schedule are known flows instead (FC-1). A schedule with no category
	/// feeds the categories of the rows it posted.
	pub(crate) fn baselined_series(&self, known: &[KnownFlow]) -> Vec<&Series> {
		let schedules: BTreeSet<&ScheduleId> = known
			.iter()
			.filter(|flow| flow.kind == FlowKind::Income)
			.filter_map(|flow| flow.schedule.as_ref())
			.collect();
		let mut scheduled_income: BTreeSet<&CategoryId> = known
			.iter()
			.filter(|flow| flow.kind == FlowKind::Income)
			.filter_map(|flow| flow.category.as_ref())
			.collect();
		scheduled_income.extend(
			self.budget
				.transactions
				.values()
				.filter(|row| {
					!row.tombstone
						&& row
							.schedule
							.as_ref()
							.is_some_and(|schedule| schedules.contains(schedule))
				})
				.filter_map(|row| row.category.as_ref())
				.filter_map(|category| self.budget.resolve_category(category)),
		);
		self.history
			.series
			.iter()
			.filter(|series| {
				series.side == Side::Spending || !scheduled_income.contains(&series.category)
			})
			.collect()
	}

	pub fn baselines(
		&self,
		today: CivilDate,
		known: &[KnownFlow],
	) -> Result<Vec<Baseline>, CoreError> {
		self.baselined_series(known)
			.into_iter()
			.map(|series| Baseline::new(self.budget, series, today.year_month(), Some(today)))
			.collect()
	}

	pub fn project(
		&self,
		today: CivilDate,
		known: &[KnownFlow],
		scenario: &Scenario,
	) -> Result<Projection, CoreError> {
		scenario.validate(today)?;
		let current = today.year_month();
		let series = self.baselined_series(known);
		let baselines = self.baselines(today, known)?;
		let band = band(self.budget, &series, current)?;
		let start = balances(self.budget, &self.classification, today)?;
		let mut running = start.clone();
		let mut worth = net_worth(&start)?;
		let net_worth_today = worth;
		let last = Period::month(current.offset(LAST_HORIZON)?).last();
		let scenario_flows = scenario.flows(today, last)?;

		let mut months = Vec::new();
		for step in 0..=LAST_HORIZON {
			let month = current.offset(step)?;
			let period = Period::month(month);
			// Unpaid flows dated today or earlier are due now (step 0); the
			// caller has already removed occurrences that were posted.
			let in_month = |date: CivilDate| {
				if step == 0 {
					date <= period.last()
				} else {
					period.contains(date)
				}
			};
			let mut known_net = 0i64;
			let mut known_income: BTreeMap<Option<&CategoryId>, i64> = BTreeMap::new();
			for flow in known.iter().filter(|flow| in_month(flow.date)) {
				post(&mut running, &flow.account, flow.amount.minor())?;
				if let FlowKind::Transfer { counterpart } = &flow.kind {
					post(
						&mut running,
						counterpart,
						flow.amount.checked_neg()?.minor(),
					)?;
				}
				known_net = add(known_net, flow.net_income().minor())?;
				if flow.kind == FlowKind::Income {
					let total = known_income.entry(flow.category.as_ref()).or_default();
					*total = add(*total, flow.amount.minor())?;
				}
			}

			let mut uncertain = 0i64;
			let mut baseline_income: BTreeMap<&CategoryId, i64> = BTreeMap::new();
			for baseline in &baselines {
				let forecast = scenario.adjust_baseline(
					&baseline.category,
					month,
					baseline.forecast(month)?,
				)?;
				let amount = if step == 0 {
					forecast
						.checked_sub(baseline.month_to_date)?
						.max(Money::ZERO)
				} else {
					forecast
				}
				.minor();
				let signed = match baseline.side {
					Side::Income => {
						baseline_income.insert(&baseline.category, amount);
						amount
					}
					Side::Spending => amount.checked_neg().ok_or(CoreError::Overflow)?,
				};
				uncertain = add(uncertain, signed)?;
				if let Some(account) = &baseline.account {
					post(&mut running, account, signed)?;
				}
			}

			let mut scenario_net = 0i64;
			for flow in scenario_flows.iter().filter(|flow| in_month(flow.date)) {
				post(&mut running, &flow.account, flow.amount.minor())?;
				scenario_net = add(scenario_net, flow.amount.minor())?;
			}
			for (account, category, change) in scenario.income_changes(month) {
				let delta = match change {
					IncomeDelta::Amount(amount) => amount.minor(),
					IncomeDelta::Percent(bp) => {
						let base = known_income
							.iter()
							.filter(|(flow, _)| category.is_none() || **flow == category)
							.map(|(_, amount)| *amount)
							.chain(
								baseline_income
									.iter()
									.filter(|(id, _)| {
										category.is_none_or(|category| category == **id)
									})
									.map(|(_, amount)| *amount),
							)
							.try_fold(0i64, i64::checked_add)
							.ok_or(CoreError::Overflow)?;
						to_money(round_half_away(i128::from(base) * i128::from(bp), 10_000))?
							.minor()
					}
				};
				post(&mut running, account, delta)?;
				scenario_net = add(scenario_net, delta)?;
			}

			let net_income = add(add(known_net, uncertain)?, scenario_net)?;
			worth = add(worth, net_income)?;
			months.push(MonthForecast {
				month,
				known_net_income: Money::from_minor(known_net),
				uncertain_net_flow: Money::from_minor(uncertain),
				scenario_net_income: Money::from_minor(scenario_net),
				net_income: Money::from_minor(net_income),
				net_worth: Money::from_minor(worth),
				balances: money_map(&running),
			});
		}

		let mut horizons = Vec::new();
		for horizon in HORIZONS {
			let Some(month) = months.get(usize::try_from(horizon).unwrap_or(0)) else {
				continue;
			};
			let band = match band.offsets(horizon)? {
				Some((low, high)) => Some((
					month.net_worth.checked_add(low)?,
					month.net_worth.checked_add(high)?,
				)),
				None => None,
			};
			horizons.push(HorizonForecast {
				months: horizon,
				month: month.month,
				net_worth: month.net_worth,
				band,
				balances: month.balances.clone(),
			});
		}

		Ok(Projection {
			scenario: scenario.name.clone(),
			today,
			net_worth_today: Money::from_minor(net_worth_today),
			balances_today: money_map(&start),
			overdue: known
				.iter()
				.filter(|flow| flow.date < today)
				.cloned()
				.collect(),
			possible_double_counts: double_counts(&baselines, known),
			capitalized_transfers: self.capitalized_transfers(&baselines, current)?,
			baselines,
			band,
			months,
			horizons,
		})
	}

	/// The baseline and up to `MAX_COMPARED` scenarios over the same inputs.
	pub fn compare(
		&self,
		today: CivilDate,
		known: &[KnownFlow],
		scenarios: &[Scenario],
	) -> Result<Comparison, CoreError> {
		if scenarios.len() > MAX_COMPARED {
			return Err(CoreError::Invalid(format!(
				"compare up to {MAX_COMPARED} scenarios at a time"
			)));
		}
		Ok(Comparison {
			baseline: self.project(today, known, &Scenario::baseline())?,
			scenarios: scenarios
				.iter()
				.map(|scenario| self.project(today, known, scenario))
				.collect::<Result<_, _>>()?,
		})
	}

	fn capitalized_transfers(
		&self,
		baselines: &[Baseline],
		current: YearMonth,
	) -> Result<usize, CoreError> {
		let spending: BTreeSet<&CategoryId> = baselines
			.iter()
			.filter(|baseline| baseline.side == Side::Spending)
			.map(|baseline| &baseline.category)
			.collect();
		let window = complete_months(current, WINDOW)?;
		let (Some(first), Some(last)) = (window.first(), window.last()) else {
			return Ok(0);
		};
		let period = Period::new(Period::month(*first).first(), Period::month(*last).last())?;
		Ok(self
			.classification
			.within(period)
			.iter()
			.filter(|row| {
				row.decision == Decision::CapitalizedExpense
					&& row.is_primary()
					&& row
						.category
						.as_ref()
						.is_some_and(|id| spending.contains(id))
					&& !bill_linked(self.budget, row)
			})
			.count())
	}
}

fn add(a: i64, b: i64) -> Result<i64, CoreError> {
	a.checked_add(b).ok_or(CoreError::Overflow)
}

fn post(
	balances: &mut BTreeMap<AccountId, i64>,
	account: &AccountId,
	amount: i64,
) -> Result<(), CoreError> {
	let balance = balances.entry(account.clone()).or_default();
	*balance = add(*balance, amount)?;
	Ok(())
}

fn money_map(balances: &BTreeMap<AccountId, i64>) -> BTreeMap<AccountId, Money> {
	balances
		.iter()
		.map(|(account, balance)| (account.clone(), Money::from_minor(*balance)))
		.collect()
}

/// A spending baseline next to a bill in the same category, or an income
/// baseline next to an income flow in it or in no category.
fn double_counts(baselines: &[Baseline], known: &[KnownFlow]) -> Vec<CategoryId> {
	baselines
		.iter()
		.filter(|baseline| baseline.level > Money::ZERO)
		.filter(|baseline| {
			known.iter().any(|flow| {
				let same = match baseline.side {
					Side::Spending => {
						flow.kind == FlowKind::Expense
							&& flow.category.as_ref() == Some(&baseline.category)
					}
					Side::Income => {
						flow.kind == FlowKind::Income
							&& flow
								.category
								.as_ref()
								.is_none_or(|category| category == &baseline.category)
					}
				};
				same && i128::from(baseline.level.minor()) * 2
					>= i128::from(flow.amount.minor()).abs()
			})
		})
		.map(|baseline| baseline.category.clone())
		.collect()
}
