use std::collections::{BTreeMap, BTreeSet};

use veritas_domain::{
	AccountId, BudgetData, CategoryId, CivilDate, CoreError, IncomeDelta, Money, Scenario,
	ScenarioId, ScheduleId, TransactionKind, YearMonth,
};
use veritas_reports::{Classification, Decision, Period, classify, net_worth};

use crate::band::{Band, band};
use crate::baseline::{Baseline, SEASON_MIN, WINDOW, complete_months};
use crate::flows::{FlowKind, KnownFlow};
use crate::history::{History, Series, Side, asset_purchase, balances, bill_linked};
use crate::math::{median, round_half_away, to_money};
use crate::scenario::{MAX_COMPARED, Overlay, validate};

/// FC-3's horizons, in complete months after the current month.
pub const HORIZONS: [u32; 4] = [3, 6, 10, 12];
const LAST_HORIZON: i32 = 12;

#[derive(Clone, Debug, PartialEq)]
pub struct MonthForecast {
	pub month: YearMonth,
	/// Net income from known flows (income minus expenses).
	pub known_net_income: Money,
	/// `F_t`: baselined income minus baselined spending (FC-4.1).
	pub baselined_net_flow: Money,
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
	/// `None` for the baseline.
	pub scenario: Option<ScenarioId>,
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
	/// FC-3: the median monthly spending no baseline covers (uncategorized,
	/// hidden, archived or deleted categories) over the window. It is shown,
	/// never projected.
	pub not_forecast: Money,
	/// FC-3: asset purchases (CLS-4 rows 8 and 9) in the baseline window,
	/// which no baseline counts, and their net amount as money out.
	pub asset_purchases: usize,
	pub asset_purchase_total: Money,
}

impl Projection {
	/// "N asset purchase(s) excluded: $X", omitted when there are none.
	pub fn asset_purchase_note(&self) -> Option<String> {
		let dollars = whole_dollars(self.asset_purchase_total);
		let total = if dollars < 0 {
			format!("-${}", -dollars)
		} else {
			format!("${dollars}")
		};
		match self.asset_purchases {
			0 => None,
			1 => Some(format!("1 asset purchase excluded: {total}")),
			n => Some(format!("{n} asset purchases excluded: {total}")),
		}
	}

	/// "Not forecast: $X a month in uncategorized or hidden spending", in
	/// whole dollars rounded half up, omitted when that rounds to $0.
	pub fn not_forecast_note(&self) -> Option<String> {
		let dollars = whole_dollars(self.not_forecast);
		(dollars > 0).then(|| {
			format!("Not forecast: ${dollars} a month in uncategorized or hidden spending")
		})
	}
}

/// Each schedule's most common category over the classified rows it posted
/// (a split child counts through its linked parent), ties to the lower id.
fn schedule_categories(
	budget: &BudgetData,
	classification: &Classification,
) -> BTreeMap<ScheduleId, CategoryId> {
	let mut counts: BTreeMap<(&ScheduleId, &CategoryId), usize> = BTreeMap::new();
	for row in classification.rows() {
		let Some(category) = row.category.as_ref().filter(|_| row.is_primary()) else {
			continue;
		};
		let schedule = [Some(&row.id), row.parent.as_ref()]
			.into_iter()
			.flatten()
			.find_map(|id| budget.transactions.get(id)?.schedule.as_ref());
		if let Some(schedule) = schedule {
			*counts.entry((schedule, category)).or_default() += 1;
		}
	}
	let mut best: BTreeMap<ScheduleId, (usize, CategoryId)> = BTreeMap::new();
	for ((schedule, category), count) in counts {
		let entry = best
			.entry(schedule.clone())
			.or_insert((0, category.clone()));
		if count > entry.0 {
			*entry = (count, category.clone());
		}
	}
	best.into_iter()
		.map(|(schedule, (_, category))| (schedule, category))
		.collect()
}

/// Cents to whole dollars, rounded half up (toward +infinity on a tie) in
/// i128, so no amount overflows.
fn whole_dollars(amount: Money) -> i128 {
	(2 * i128::from(amount.minor()) + 100).div_euclid(200)
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
	pub(crate) season_min: i32,
	/// FC-2: the most common category of each schedule's posted rows.
	schedule_categories: BTreeMap<ScheduleId, CategoryId>,
}

impl<'a> Forecaster<'a> {
	pub fn new(budget: &'a BudgetData) -> Result<Forecaster<'a>, CoreError> {
		let classification = classify(budget);
		let history = History::new(budget, &classification)?;
		let schedule_categories = schedule_categories(budget, &classification);
		Ok(Forecaster {
			budget,
			classification,
			history,
			season_min: SEASON_MIN,
			schedule_categories,
		})
	}

	/// The same forecaster with seasonality never turning on, for FC-7's
	/// seasonality-off backtest. Not for projections a user sees.
	pub fn without_seasonality(self) -> Forecaster<'a> {
		Forecaster {
			season_min: i32::MAX,
			..self
		}
	}

	pub fn classification(&self) -> &Classification {
		&self.classification
	}

	/// FC-2: the category an income flow feeds. Its own category, or for a
	/// schedule with none, the most common category of the rows it posted
	/// (ties to the lower id).
	fn feeding_category(&self, flow: &KnownFlow) -> Option<CategoryId> {
		flow.category.clone().or_else(|| {
			flow.schedule
				.as_ref()
				.and_then(|schedule| self.schedule_categories.get(schedule))
				.cloned()
		})
	}

	/// The series that get a baseline: income categories fed by an income
	/// schedule are known flows instead (FC-1, FC-2).
	pub(crate) fn baselined_series(&self, known: &[KnownFlow]) -> Vec<&Series> {
		let scheduled_income: BTreeSet<CategoryId> = known
			.iter()
			.filter(|flow| flow.kind == FlowKind::Income)
			.filter_map(|flow| self.feeding_category(flow))
			.collect();
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
			.map(|series| {
				Baseline::new(
					self.budget,
					series,
					today.year_month(),
					Some(today),
					self.season_min,
				)
			})
			.collect()
	}

	pub fn project(
		&self,
		today: CivilDate,
		known: &[KnownFlow],
		scenario: Option<&Scenario>,
	) -> Result<Projection, CoreError> {
		if let Some(scenario) = scenario {
			validate(scenario, today)?;
		}
		let overlay = Overlay::new(scenario);
		let current = today.year_month();
		let series = self.baselined_series(known);
		let baselines = self.baselines(today, known)?;
		let band = band(self.budget, &series, current, self.season_min)?;
		let start = balances(self.budget, &self.classification, today)?;
		let mut running = start.clone();
		let mut worth = net_worth(&self.classification, today)?.minor();
		let net_worth_today = worth;
		let last = Period::month(current.offset(LAST_HORIZON)?).last();
		let scenario_flows = overlay.flows(today, last)?;
		let (asset_purchases, asset_purchase_total) = self.asset_purchases(current)?;

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
			let mut known_income: BTreeMap<Option<CategoryId>, i64> = BTreeMap::new();
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
					let total = known_income.entry(self.feeding_category(flow)).or_default();
					*total = add(*total, flow.amount.minor())?;
				}
			}

			let mut baselined = 0i64;
			let mut baseline_income: BTreeMap<&CategoryId, i64> = BTreeMap::new();
			for baseline in &baselines {
				let forecast = overlay.adjust_baseline(
					&baseline.category,
					month,
					baseline.forecast_plus(month)?,
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
				baselined = add(baselined, signed)?;
				if let Some(account) = &baseline.account {
					post(&mut running, account, signed)?;
				}
			}

			let mut scenario_net = 0i64;
			for flow in scenario_flows.iter().filter(|flow| in_month(flow.date)) {
				post(&mut running, &flow.account, flow.amount.minor())?;
				match &flow.counterpart {
					Some(counterpart) => {
						post(
							&mut running,
							counterpart,
							flow.amount.checked_neg()?.minor(),
						)?;
					}
					None => scenario_net = add(scenario_net, flow.amount.minor())?,
				}
			}
			for (account, category, change) in overlay.income_changes(month) {
				let delta = match change {
					IncomeDelta::Amount(amount) => amount.minor(),
					IncomeDelta::Percent(bp) => {
						let base = known_income
							.iter()
							.filter(|(fed, _)| category.is_none() || fed.as_ref() == category)
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

			let net_income = add(add(known_net, baselined)?, scenario_net)?;
			worth = add(worth, net_income)?;
			months.push(MonthForecast {
				month,
				known_net_income: Money::from_minor(known_net),
				baselined_net_flow: Money::from_minor(baselined),
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
			scenario: scenario.map(|scenario| scenario.id.clone()),
			today,
			net_worth_today: Money::from_minor(net_worth_today),
			balances_today: money_map(&start),
			overdue: known
				.iter()
				.filter(|flow| flow.date < today)
				.cloned()
				.collect(),
			possible_double_counts: double_counts(self, &baselines, known),
			asset_purchases,
			asset_purchase_total,
			not_forecast: self.not_forecast(&baselines, current)?,
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
		scenarios: &[&Scenario],
	) -> Result<Comparison, CoreError> {
		if scenarios.len() > MAX_COMPARED {
			return Err(CoreError::Invalid(format!(
				"compare up to {MAX_COMPARED} scenarios at a time"
			)));
		}
		Ok(Comparison {
			baseline: self.project(today, known, None)?,
			scenarios: scenarios
				.iter()
				.map(|scenario| self.project(today, known, Some(scenario)))
				.collect::<Result<_, _>>()?,
		})
	}

	fn not_forecast(&self, baselines: &[Baseline], current: YearMonth) -> Result<Money, CoreError> {
		let spending: BTreeSet<&CategoryId> = baselines
			.iter()
			.filter(|baseline| baseline.side == Side::Spending)
			.map(|baseline| &baseline.category)
			.collect();
		let first = self
			.classification
			.rows()
			.first()
			.map(|row| row.date.year_month());
		let mut totals = Vec::new();
		for month in complete_months(current, WINDOW)? {
			if first.is_none_or(|first| month < first) {
				continue;
			}
			let mut total = 0i64;
			for row in self.classification.within(Period::month(month)) {
				let covered = row
					.category
					.as_ref()
					.is_some_and(|id| spending.contains(id));
				let spending = row.kind() == TransactionKind::Expense
					&& !asset_purchase(row)
					&& row.decision != Decision::MisfiledTransferCategory;
				if row.is_primary() && spending && !covered && !bill_linked(self.budget, row) {
					total = total
						.checked_sub(row.amount.minor())
						.ok_or(CoreError::Overflow)?;
				}
			}
			totals.push(total);
		}
		Ok(Money::from_minor(median(&totals).unwrap_or(0)))
	}

	/// FC-3's asset purchases in the baseline window: how many, and their
	/// net amount as money out.
	fn asset_purchases(&self, current: YearMonth) -> Result<(usize, Money), CoreError> {
		let window = complete_months(current, WINDOW)?;
		let (Some(first), Some(last)) = (window.first(), window.last()) else {
			return Ok((0, Money::ZERO));
		};
		let period = Period::new(Period::month(*first).first(), Period::month(*last).last())?;
		let rows: Vec<_> = self
			.classification
			.within(period)
			.iter()
			.filter(|row| row.is_primary() && asset_purchase(row) && !bill_linked(self.budget, row))
			.collect();
		let total = Money::checked_sum(rows.iter().map(|row| row.amount))?.checked_neg()?;
		Ok((rows.len(), total))
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

/// FC-2: a spending baseline at least half a bill in the same category, or
/// an income baseline at least half an income flow that feeds no category.
fn double_counts(
	forecaster: &Forecaster,
	baselines: &[Baseline],
	known: &[KnownFlow],
) -> Vec<CategoryId> {
	let unfed: Vec<&KnownFlow> = known
		.iter()
		.filter(|flow| flow.kind == FlowKind::Income && forecaster.feeding_category(flow).is_none())
		.collect();
	baselines
		.iter()
		.filter(|baseline| baseline.level > Money::ZERO)
		.filter(|baseline| {
			let half = |flow: &KnownFlow| {
				i128::from(baseline.level.minor()) * 2 >= i128::from(flow.amount.minor()).abs()
			};
			match baseline.side {
				Side::Spending => known.iter().any(|flow| {
					flow.kind == FlowKind::Expense
						&& flow.category.as_ref() == Some(&baseline.category)
						&& half(flow)
				}),
				Side::Income => unfed.iter().any(|flow| half(flow)),
			}
		})
		.map(|baseline| baseline.category.clone())
		.collect()
}
