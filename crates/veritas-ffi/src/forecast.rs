//! The Insights screen models (R-FC-1 to R-FC-4): the forecast with up to
//! three what-ifs, next month's budget suggestions, goal ETAs, and the
//! what-if scenarios themselves (docs/finance/forecasting.md).

use std::collections::BTreeMap;
use std::sync::Arc;

use veritas_domain::{
	AccountId, BudgetData, Cadence, CategoryId, CivilDate, CoreError, GoalId, IncomeDelta, Money,
	ScenarioChange as CoreChange, ScenarioId,
};
use veritas_engine::{EnvelopeSheet, register_currency_text};
use veritas_forecast::{
	BandMethod, Basis, Certainty, ESTIMATED_SPLIT, EtaStatus, FlowKind, GoalEta, KnownFlow,
	NO_INTEREST_LINE, Projection,
};
use veritas_session::forecast::{self as core, ForecastView};

use crate::blocking::{CancelToken as CancelHandle, run};
use crate::budget::{civil_date, year_month};
use crate::session::{Core, Session};
use crate::{FfiError, Month};

/// An account's balance in a projection, live accounts only.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ForecastAccount {
	pub account_id: String,
	pub name: String,
	pub off_budget: bool,
	pub balance: i64,
}

/// One month of a projection: the current month's remainder first, then
/// the next 12 complete months (FC-3).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ForecastMonth {
	pub month: Month,
	/// Net income from bills, income schedules and loan payments.
	pub known_net_income: i64,
	/// FC-4.1's `F_t`: baselined income minus baselined spending.
	pub baselined_net_flow: i64,
	/// What the what-if adds; 0 for the baseline.
	pub scenario_net_income: i64,
	pub net_income: i64,
	/// At the end of the month.
	pub net_worth: i64,
}

/// FC-3's horizon, 3, 6, 10 or 12 complete months after the current month.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ForecastHorizon {
	pub months: u32,
	pub month: Month,
	pub net_worth: i64,
	/// FC-4's 80% band around `net_worth`, `nil` with no history.
	pub band_low: Option<i64>,
	pub band_high: Option<i64>,
	pub accounts: Vec<ForecastAccount>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum GoalEtaStatus {
	Reached,
	/// GOAL-10's "Reached → Spent": no ETA.
	Spent,
	/// "No ETA yet".
	NoHistory,
	/// "No ETA: nothing is being added".
	NotAdding,
	/// `pessimistic` is `nil` when the slowest pace adds nothing.
	Eta {
		month: Month,
		optimistic: Month,
		pessimistic: Option<Month>,
	},
}

/// FC-6's goal ETA from the contribution pace (R-FC-4), for a goal that
/// isn't closed. `pace`, `optimisticPace` and `pessimisticPace` are the
/// median, maximum and minimum of the last up to 3 complete months, on the
/// budget scale.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct GoalEtaItem {
	pub goal_id: String,
	pub name: String,
	pub status: GoalEtaStatus,
	pub remaining: i64,
	pub pace: Option<i64>,
	pub optimistic_pace: Option<i64>,
	pub pessimistic_pace: Option<i64>,
	pub history_months: u32,
	/// "1 month of history", "3 months of history".
	pub history_label: String,
	/// Whether the ETA is not after the target month; `nil` without a target
	/// date or an ETA.
	pub on_track: Option<bool>,
	/// FC-6's display line, such as "ETA March 2027, range March 2027 to May 2027".
	pub text: String,
}

/// The baseline (`scenarioId` `nil`) or one what-if over the same inputs.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ForecastProjection {
	pub scenario_id: Option<String>,
	/// The what-if's name, or empty for the baseline.
	pub name: String,
	pub months: Vec<ForecastMonth>,
	pub horizons: Vec<ForecastHorizon>,
	/// D67: the what-if's stored events dated in the past, left out of the
	/// projection. Show them as "in the past" to delete or re-date.
	pub events_in_the_past: Vec<String>,
	/// FC-6 with this projection's goal contributions. The what-if changes
	/// these and nothing else about goals (FC-3).
	pub goal_etas: Vec<GoalEtaItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ForecastFlowKind {
	Income,
	Expense,
	/// Between two of the user's accounts: net worth doesn't move. Loan
	/// principal is one (FC-2).
	Transfer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FlowCertainty {
	/// An exact bill or income, or a loan payment with a known split.
	Known,
	/// An About or Between amount, or a loan split estimated from the amounts.
	Estimated,
}

/// One dated flow the forecast knows ahead of time (FC-2).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ForecastFlow {
	pub date: String,
	/// Dated before today and not yet posted: counted as due now.
	pub due_now: bool,
	pub account_id: String,
	pub account_name: String,
	/// Actual's sign, seen from `accountId`.
	pub amount: i64,
	pub kind: ForecastFlowKind,
	pub counterpart_account_id: Option<String>,
	pub category_id: Option<String>,
	pub category_name: Option<String>,
	pub bill_id: Option<String>,
	pub name: String,
	pub certainty: FlowCertainty,
}

/// FC-2's notes for a loan whose payment the forecast splits.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LoanForecast {
	pub account_id: String,
	pub name: String,
	/// No usable D76 record, so the split is read from the amounts.
	pub estimated_from_amounts: bool,
	/// A known split with no interest line and no rate: interest is 0.
	pub interest_assumed_zero: bool,
	/// In order: "Estimated from the amounts", "No interest line on the last
	/// payment, interest taken as 0", "Escrow taken from {category} {amount}"
	/// and one "Not repeated: {category} {amount}" per part not repeated.
	pub notes: Vec<String>,
}

/// A bill or loan the forecast couldn't turn into flows, and why. Listed,
/// never dropped silently.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct UnforecastItem {
	pub bill_id: Option<String>,
	pub loan_account_id: Option<String>,
	pub name: String,
	pub reason: String,
}

/// A loan payment below its interest and escrow (DEBT-2.2): `uncovered` is
/// not recorded and the balance moves only by principal.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ForecastShortfall {
	pub date: String,
	pub loan_account_id: String,
	pub loan_name: String,
	pub uncovered: i64,
}

/// One category baseline the projection uses (FC-1).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ForecastBaseline {
	pub category_id: String,
	pub name: String,
	pub is_income: bool,
	/// `L`, which may be negative (FC-1).
	pub level: i64,
	/// "6-month median", "2-month median" or "No history yet".
	pub label: String,
	pub seasonal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ForecastCategory {
	pub category_id: String,
	pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ForecastBandMethod {
	/// P10 and P90 of the rolling-origin residuals (FC-4.2).
	Residuals,
	/// Fewer than 6 residuals: the range of the monthly flow (FC-4.3).
	Range,
	NoHistory,
}

/// The forecast screen (R-FC-1, R-FC-2, R-FC-4). Amounts are register
/// scale, except goal paces and remaining, which are budget scale.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Forecast {
	pub today: String,
	pub net_worth_today: i64,
	pub accounts_today: Vec<ForecastAccount>,
	pub baseline: ForecastProjection,
	pub scenarios: Vec<ForecastProjection>,
	pub band_method: ForecastBandMethod,
	/// "80% band from N past months", FC-4.3's fallback, or "No history yet".
	pub band_label: String,
	pub baselines: Vec<ForecastBaseline>,
	pub known_flows: Vec<ForecastFlow>,
	pub loans: Vec<LoanForecast>,
	pub unforecast: Vec<UnforecastItem>,
	pub shortfalls: Vec<ForecastShortfall>,
	/// FC-2: categories where a hand-typed bill may be counted twice.
	pub possible_double_counts: Vec<ForecastCategory>,
	/// FC-3's "Not forecast: $X a month in uncategorized or hidden spending".
	pub not_forecast_note: Option<String>,
	/// FC-3's "N asset purchase(s) excluded: $X".
	pub asset_purchase_note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SuggestionBasis {
	Baseline,
	Seasonal,
	Bills,
	BillsAndBaseline,
	Goal,
	LoanPrincipal,
	NoHistory,
	NoTargetDate,
	GoalReached,
}

/// FC-5's suggestion for one envelope, with its one reason line.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BudgetSuggestion {
	pub category_id: String,
	pub name: String,
	/// What "Apply suggestions" writes, on the budget scale (CLS-12.11).
	/// `nil` keeps the current budget, and the reason says so.
	pub amount: Option<i64>,
	pub basis: SuggestionBasis,
	pub reason: String,
}

/// Next month's suggestions (R-FC-3).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SuggestionList {
	pub month: Month,
	pub items: Vec<BudgetSuggestion>,
	/// Register scale: known income flows plus income baselines.
	pub expected_income: i64,
	/// Register scale, set when the suggestions add up to more.
	pub exceeds_income_by: Option<i64>,
	/// "Suggestions exceed expected income by $X".
	pub notice: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ScenarioCadence {
	EveryMonths { count: u32 },
	EveryWeeks { count: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum IncomeChangeAmount {
	/// Added each month, signed: a raise is positive.
	Amount { amount: i64 },
	/// Basis points of the month's expected income, signed.
	Percent { percent_bp: i64 },
}

/// One what-if line (FC-3). Amounts are Actual's sign, register scale,
/// except a goal contribution, which is budget scale (CLS-12.9).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ScenarioChange {
	OneTime {
		date: String,
		account_id: String,
		category_id: Option<String>,
		amount: i64,
	},
	/// Anchored on `start`, the way bills repeat. `endMonth` is the last
	/// month with an occurrence, or `nil` for open-ended.
	Recurring {
		start: String,
		end_month: Option<Month>,
		cadence: ScenarioCadence,
		account_id: String,
		category_id: Option<String>,
		amount: i64,
	},
	/// Changes the category's baseline by `percentBp` basis points.
	CategoryPercent {
		category_id: String,
		percent_bp: i64,
		start_month: Month,
		end_month: Option<Month>,
	},
	/// For one income category, or all income when `categoryId` is `nil`;
	/// the difference posts to `accountId`.
	IncomeChange {
		account_id: String,
		category_id: Option<String>,
		change: IncomeChangeAmount,
		start_month: Month,
		end_month: Option<Month>,
	},
	/// A budget move: it changes the goal's ETA and nothing else.
	GoalContribution {
		goal_id: String,
		amount: i64,
		start_month: Month,
		end_month: Option<Month>,
	},
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ScenarioEventItem {
	pub id: String,
	pub change: ScenarioChange,
	/// D67: kept, left out of the projection, shown to delete or re-date.
	pub in_the_past: bool,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ScenarioSummary {
	pub id: String,
	pub name: String,
	/// Events ordered by id.
	pub events: Vec<ScenarioEventItem>,
}

pub(crate) fn core_change(change: &ScenarioChange) -> Result<CoreChange, CoreError> {
	let month = |month: &Month| year_month(*month);
	let end = |month: &Option<Month>| month.map(year_month).transpose();
	let category = |id: &Option<String>| id.as_deref().map(CategoryId::new);
	Ok(match change {
		ScenarioChange::OneTime {
			date,
			account_id,
			category_id,
			amount,
		} => CoreChange::OneTime {
			date: civil_date(date)?,
			account: AccountId::new(account_id.as_str()),
			category: category(category_id),
			amount: Money::from_minor(*amount),
		},
		ScenarioChange::Recurring {
			start,
			end_month,
			cadence,
			account_id,
			category_id,
			amount,
		} => CoreChange::Recurring {
			start: civil_date(start)?,
			end_month: end(end_month)?,
			cadence: match cadence {
				ScenarioCadence::EveryMonths { count } => Cadence::EveryMonths(*count),
				ScenarioCadence::EveryWeeks { count } => Cadence::EveryWeeks(*count),
			},
			account: AccountId::new(account_id.as_str()),
			category: category(category_id),
			amount: Money::from_minor(*amount),
		},
		ScenarioChange::CategoryPercent {
			category_id,
			percent_bp,
			start_month,
			end_month,
		} => CoreChange::CategoryPercent {
			category: CategoryId::new(category_id.as_str()),
			percent_bp: *percent_bp,
			start_month: month(start_month)?,
			end_month: end(end_month)?,
		},
		ScenarioChange::IncomeChange {
			account_id,
			category_id,
			change,
			start_month,
			end_month,
		} => CoreChange::IncomeChange {
			account: AccountId::new(account_id.as_str()),
			category: category(category_id),
			change: match change {
				IncomeChangeAmount::Amount { amount } => {
					IncomeDelta::Amount(Money::from_minor(*amount))
				}
				IncomeChangeAmount::Percent { percent_bp } => IncomeDelta::Percent(*percent_bp),
			},
			start_month: month(start_month)?,
			end_month: end(end_month)?,
		},
		ScenarioChange::GoalContribution {
			goal_id,
			amount,
			start_month,
			end_month,
		} => CoreChange::GoalContribution {
			goal: GoalId::new(goal_id.as_str()),
			amount: Money::from_minor(*amount),
			start_month: month(start_month)?,
			end_month: end(end_month)?,
		},
	})
}

fn ffi_change(change: &CoreChange) -> Option<ScenarioChange> {
	let id = |id: &Option<CategoryId>| id.as_ref().map(ToString::to_string);
	let end = |month: &Option<veritas_domain::YearMonth>| month.map(Month::from);
	Some(match change {
		CoreChange::OneTime {
			date,
			account,
			category,
			amount,
		} => ScenarioChange::OneTime {
			date: date.to_string(),
			account_id: account.to_string(),
			category_id: id(category),
			amount: amount.minor(),
		},
		CoreChange::OneTimeTransfer { .. } => return None,
		CoreChange::Recurring {
			start,
			end_month,
			cadence,
			account,
			category,
			amount,
		} => ScenarioChange::Recurring {
			start: start.to_string(),
			end_month: end(end_month),
			cadence: match cadence {
				Cadence::EveryMonths(count) => ScenarioCadence::EveryMonths { count: *count },
				Cadence::EveryWeeks(count) => ScenarioCadence::EveryWeeks { count: *count },
			},
			account_id: account.to_string(),
			category_id: id(category),
			amount: amount.minor(),
		},
		CoreChange::CategoryPercent {
			category,
			percent_bp,
			start_month,
			end_month,
		} => ScenarioChange::CategoryPercent {
			category_id: category.to_string(),
			percent_bp: *percent_bp,
			start_month: (*start_month).into(),
			end_month: end(end_month),
		},
		CoreChange::IncomeChange {
			account,
			category,
			change,
			start_month,
			end_month,
		} => ScenarioChange::IncomeChange {
			account_id: account.to_string(),
			category_id: id(category),
			change: match change {
				IncomeDelta::Amount(amount) => IncomeChangeAmount::Amount {
					amount: amount.minor(),
				},
				IncomeDelta::Percent(percent_bp) => IncomeChangeAmount::Percent {
					percent_bp: *percent_bp,
				},
			},
			start_month: (*start_month).into(),
			end_month: end(end_month),
		},
		CoreChange::GoalContribution {
			goal,
			amount,
			start_month,
			end_month,
		} => ScenarioChange::GoalContribution {
			goal_id: goal.to_string(),
			amount: amount.minor(),
			start_month: (*start_month).into(),
			end_month: end(end_month),
		},
	})
}

fn scenario_list(data: &BudgetData, today: CivilDate) -> Result<Vec<ScenarioSummary>, CoreError> {
	let mut scenarios: Vec<_> = data.scenarios.values().collect();
	scenarios.sort_by(|a, b| {
		a.sort_order
			.total_cmp(&b.sort_order)
			.then_with(|| a.created_at.cmp(&b.created_at))
			.then_with(|| a.id.cmp(&b.id))
	});
	scenarios
		.into_iter()
		.map(|scenario| {
			let mut events = Vec::new();
			for event in &scenario.events {
				let Some(change) = ffi_change(&event.change) else {
					continue;
				};
				events.push(ScenarioEventItem {
					id: event.id.to_string(),
					change,
					in_the_past: veritas_forecast::in_the_past(&event.change, today)?,
				});
			}
			Ok(ScenarioSummary {
				id: scenario.id.to_string(),
				name: scenario.name.clone(),
				events,
			})
		})
		.collect()
}

fn account_name(data: &BudgetData, id: &AccountId) -> String {
	data.accounts
		.get(id)
		.map(|account| account.name.clone())
		.unwrap_or_default()
}

fn category_name(data: &BudgetData, id: &CategoryId) -> String {
	data.categories
		.get(id)
		.map(|category| category.name.clone())
		.unwrap_or_default()
}

fn accounts(data: &BudgetData, balances: &BTreeMap<AccountId, Money>) -> Vec<ForecastAccount> {
	let mut rows: Vec<_> = balances
		.iter()
		.filter_map(|(id, balance)| {
			let account = data.accounts.get(id).filter(|account| !account.tombstone)?;
			Some((
				(account.off_budget, account.sort_order),
				ForecastAccount {
					account_id: id.to_string(),
					name: account.name.clone(),
					off_budget: account.off_budget,
					balance: balance.minor(),
				},
			))
		})
		.collect();
	rows.sort_by(|(a, x), (b, y)| {
		a.0.cmp(&b.0)
			.then(a.1.total_cmp(&b.1))
			.then_with(|| x.account_id.cmp(&y.account_id))
	});
	rows.into_iter().map(|(_, row)| row).collect()
}

fn goal_eta(data: &BudgetData, eta: &GoalEta) -> GoalEtaItem {
	let name = data
		.goals
		.get(&eta.goal)
		.map(|goal| category_name(data, &goal.category))
		.unwrap_or_default();
	GoalEtaItem {
		goal_id: eta.goal.to_string(),
		name,
		status: match eta.status {
			EtaStatus::Reached => GoalEtaStatus::Reached,
			EtaStatus::Spent => GoalEtaStatus::Spent,
			EtaStatus::NoHistory => GoalEtaStatus::NoHistory,
			EtaStatus::NotAdding => GoalEtaStatus::NotAdding,
			EtaStatus::Eta {
				month,
				optimistic,
				pessimistic,
			} => GoalEtaStatus::Eta {
				month: month.into(),
				optimistic: optimistic.into(),
				pessimistic: pessimistic.map(Month::from),
			},
		},
		remaining: eta.remaining.minor(),
		pace: eta.pace.map(|(mid, _, _)| mid.minor()),
		optimistic_pace: eta.pace.map(|(_, max, _)| max.minor()),
		pessimistic_pace: eta.pace.map(|(_, _, min)| min.minor()),
		history_months: u32::try_from(eta.history_months).unwrap_or(u32::MAX),
		history_label: eta.history_label(),
		on_track: eta.on_track,
		text: eta.text(),
	}
}

fn projection(
	data: &BudgetData,
	projection: &Projection,
	name: String,
	etas: &[GoalEta],
) -> ForecastProjection {
	ForecastProjection {
		scenario_id: projection.scenario.as_ref().map(ToString::to_string),
		name,
		months: projection
			.months
			.iter()
			.map(|month| ForecastMonth {
				month: month.month.into(),
				known_net_income: month.known_net_income.minor(),
				baselined_net_flow: month.baselined_net_flow.minor(),
				scenario_net_income: month.scenario_net_income.minor(),
				net_income: month.net_income.minor(),
				net_worth: month.net_worth.minor(),
			})
			.collect(),
		horizons: projection
			.horizons
			.iter()
			.map(|horizon| ForecastHorizon {
				months: horizon.months,
				month: horizon.month.into(),
				net_worth: horizon.net_worth.minor(),
				band_low: horizon.band.map(|(low, _)| low.minor()),
				band_high: horizon.band.map(|(_, high)| high.minor()),
				accounts: accounts(data, &horizon.balances),
			})
			.collect(),
		events_in_the_past: projection
			.in_the_past
			.iter()
			.map(ToString::to_string)
			.collect(),
		goal_etas: etas.iter().map(|eta| goal_eta(data, eta)).collect(),
	}
}

fn flow(data: &BudgetData, today: CivilDate, flow: &KnownFlow) -> ForecastFlow {
	let (kind, counterpart) = match &flow.kind {
		FlowKind::Income => (ForecastFlowKind::Income, None),
		FlowKind::Expense => (ForecastFlowKind::Expense, None),
		FlowKind::Transfer { counterpart } => {
			(ForecastFlowKind::Transfer, Some(counterpart.to_string()))
		}
	};
	ForecastFlow {
		date: flow.date.to_string(),
		due_now: flow.date < today,
		account_id: flow.account.to_string(),
		account_name: account_name(data, &flow.account),
		amount: flow.amount.minor(),
		kind,
		counterpart_account_id: counterpart,
		category_id: flow.category.as_ref().map(ToString::to_string),
		category_name: flow.category.as_ref().map(|id| category_name(data, id)),
		bill_id: flow.schedule.as_ref().map(ToString::to_string),
		name: flow.name.clone(),
		certainty: match flow.certainty {
			Certainty::Exact => FlowCertainty::Known,
			Certainty::Estimate => FlowCertainty::Estimated,
		},
	}
}

/// FC-2's notes, one entry per loan the flows mention, in account order.
fn loans(data: &BudgetData, view: &ForecastView) -> Vec<LoanForecast> {
	let flows = &view.flows;
	let mut ids: Vec<&AccountId> = flows
		.estimated_splits
		.iter()
		.chain(&flows.interest_assumed_zero)
		.chain(flows.escrow_sources.iter().map(|source| &source.loan))
		.chain(flows.not_repeated.iter().map(|part| &part.loan))
		.collect();
	ids.sort_by(|a, b| {
		let order = |id: &AccountId| data.accounts.get(id).map_or(f64::MAX, |a| a.sort_order);
		order(a).total_cmp(&order(b)).then_with(|| a.cmp(b))
	});
	ids.dedup();
	ids.into_iter()
		.map(|loan| {
			let estimated = flows.estimated_splits.contains(loan);
			let zero = flows.interest_assumed_zero.contains(loan);
			let mut notes = Vec::new();
			if estimated {
				notes.push(ESTIMATED_SPLIT.to_owned());
			}
			if zero {
				notes.push(NO_INTEREST_LINE.to_owned());
			}
			notes.extend(
				flows
					.escrow_sources
					.iter()
					.filter(|source| &source.loan == loan)
					.map(|source| source.text(data)),
			);
			notes.extend(
				flows
					.not_repeated
					.iter()
					.filter(|part| &part.loan == loan)
					.map(|part| {
						let name = part.category.as_ref().map_or_else(
							|| "Uncategorized".to_owned(),
							|id| category_name(data, id),
						);
						format!(
							"Not repeated: {name} {}",
							register_currency_text(part.amount, &data.format)
						)
					}),
			);
			LoanForecast {
				account_id: loan.to_string(),
				name: account_name(data, loan),
				estimated_from_amounts: estimated,
				interest_assumed_zero: zero,
				notes,
			}
		})
		.collect()
}

fn forecast_screen(data: &BudgetData, today: CivilDate, view: &ForecastView) -> Forecast {
	let baseline = &view.comparison.baseline;
	let empty = Vec::new();
	let etas = |index: usize| view.goal_etas.get(index).unwrap_or(&empty);
	Forecast {
		today: today.to_string(),
		net_worth_today: baseline.net_worth_today.minor(),
		accounts_today: accounts(data, &baseline.balances_today),
		baseline: projection(data, baseline, String::new(), etas(0)),
		scenarios: view
			.comparison
			.scenarios
			.iter()
			.zip(&view.scenarios)
			.enumerate()
			.map(|(index, (projected, scenario))| {
				projection(data, projected, scenario.name.clone(), etas(index + 1))
			})
			.collect(),
		band_method: match baseline.band.method {
			BandMethod::Residuals => ForecastBandMethod::Residuals,
			BandMethod::Range => ForecastBandMethod::Range,
			BandMethod::NoHistory => ForecastBandMethod::NoHistory,
		},
		band_label: baseline.band.label(),
		baselines: baseline
			.baselines
			.iter()
			.map(|baseline| ForecastBaseline {
				category_id: baseline.category.to_string(),
				name: category_name(data, &baseline.category),
				is_income: baseline.side == veritas_forecast::Side::Income,
				level: baseline.level.minor(),
				label: baseline.label(),
				seasonal: baseline.seasonal_index.is_some(),
			})
			.collect(),
		known_flows: view
			.flows
			.flows
			.iter()
			.map(|known| flow(data, today, known))
			.collect(),
		loans: loans(data, view),
		unforecast: view
			.flows
			.unforecast
			.iter()
			.map(|item| UnforecastItem {
				bill_id: item.schedule.as_ref().map(ToString::to_string),
				loan_account_id: item.loan.as_ref().map(ToString::to_string),
				name: match (&item.schedule, &item.loan) {
					(Some(schedule), _) => data
						.schedules
						.get(schedule)
						.and_then(|schedule| schedule.name.clone())
						.unwrap_or_default(),
					(None, Some(loan)) => account_name(data, loan),
					(None, None) => String::new(),
				},
				reason: item.reason.clone(),
			})
			.collect(),
		shortfalls: view
			.flows
			.shortfalls
			.iter()
			.map(|shortfall| ForecastShortfall {
				date: shortfall.date.to_string(),
				loan_account_id: shortfall.loan.to_string(),
				loan_name: account_name(data, &shortfall.loan),
				uncovered: shortfall.uncovered.minor(),
			})
			.collect(),
		possible_double_counts: baseline
			.possible_double_counts
			.iter()
			.map(|id| ForecastCategory {
				category_id: id.to_string(),
				name: category_name(data, id),
			})
			.collect(),
		not_forecast_note: baseline.not_forecast_note(&data.format),
		asset_purchase_note: baseline.asset_purchase_note(&data.format),
	}
}

fn suggestion_list(
	data: &BudgetData,
	suggestions: &veritas_forecast::Suggestions,
) -> SuggestionList {
	SuggestionList {
		month: suggestions.month.into(),
		items: suggestions
			.items
			.iter()
			.map(|item| BudgetSuggestion {
				category_id: item.category.to_string(),
				name: category_name(data, &item.category),
				amount: item.amount.map(Money::minor),
				basis: match item.basis {
					Basis::Baseline => SuggestionBasis::Baseline,
					Basis::Seasonal => SuggestionBasis::Seasonal,
					Basis::Bills => SuggestionBasis::Bills,
					Basis::BillsAndBaseline => SuggestionBasis::BillsAndBaseline,
					Basis::Goal => SuggestionBasis::Goal,
					Basis::LoanPrincipal => SuggestionBasis::LoanPrincipal,
					Basis::NoHistory => SuggestionBasis::NoHistory,
					Basis::NoTargetDate => SuggestionBasis::NoTargetDate,
					Basis::GoalReached => SuggestionBasis::GoalReached,
				},
				reason: item.reason.clone(),
			})
			.collect(),
		expected_income: suggestions.expected_income.minor(),
		exceeds_income_by: suggestions.exceeds_income_by.map(Money::minor),
		notice: suggestions.notice.clone(),
	}
}

impl<B: veritas_session::Backend> Core<B> {
	pub(crate) fn forecast(
		&self,
		today: &str,
		scenario_ids: &[String],
		cancel: &veritas_session::CancelToken,
	) -> Result<Forecast, FfiError> {
		if cancel.is_cancelled() {
			return Err(FfiError::Cancelled);
		}
		self.call(|session| {
			let today = civil_date(today)?;
			let ids: Vec<ScenarioId> = scenario_ids
				.iter()
				.map(|id| ScenarioId::new(id.as_str()))
				.collect();
			session.with_sheet(today, |data, sheet| {
				let view = core::forecast(data, sheet, today, &ids, cancel)?;
				Ok(forecast_screen(data, today, &view))
			})
		})
	}

	pub(crate) fn budget_suggestions(
		&self,
		today: &str,
		cancel: &veritas_session::CancelToken,
	) -> Result<SuggestionList, FfiError> {
		if cancel.is_cancelled() {
			return Err(FfiError::Cancelled);
		}
		self.call(|session| {
			let today = civil_date(today)?;
			session.with_sheet(today, |data, sheet: &EnvelopeSheet| {
				let suggestions = core::suggestions(data, sheet, today)?;
				Ok(suggestion_list(data, &suggestions))
			})
		})
	}

	pub(crate) fn scenarios(&self, today: &str) -> Result<Vec<ScenarioSummary>, FfiError> {
		self.call(|session| scenario_list(session.data()?, civil_date(today)?))
	}
}

#[uniffi::export]
impl Session {
	/// The forecast screen as of `today` (`YYYY-MM-DD`), with up to three
	/// what-ifs compared against the baseline (R-FC-1, R-FC-2, R-FC-4). More
	/// than three is `Invalid`; an unknown id is `NotFound`.
	pub async fn forecast(
		&self,
		today: String,
		scenario_ids: Vec<String>,
		cancel: Arc<CancelHandle>,
	) -> Result<Forecast, FfiError> {
		let core = self.core.clone();
		run(&cancel, move |cancel| {
			core.forecast(&today, &scenario_ids, cancel)
		})
		.await
	}

	/// FC-5's suggestions for the month after `today` (R-FC-3).
	/// `Command::ApplySuggestions` writes them.
	pub async fn budget_suggestions(
		&self,
		today: String,
		cancel: Arc<CancelHandle>,
	) -> Result<SuggestionList, FfiError> {
		let core = self.core.clone();
		run(&cancel, move |cancel| {
			core.budget_suggestions(&today, cancel)
		})
		.await
	}

	/// Every what-if with its events, in the order they were made (R-FC-2).
	/// `today` decides which events are "in the past" (D67).
	pub fn scenarios(&self, today: String) -> Result<Vec<ScenarioSummary>, FfiError> {
		self.core.scenarios(&today)
	}
}

#[cfg(test)]
mod tests {
	use std::collections::BTreeMap;
	use std::path::Path;

	use veritas_domain::{BudgetData, Row};

	use super::*;
	use crate::commands::Command;
	use crate::records::EntityKind;
	use crate::test_support::{Scratch, imported, reopened};

	type Store = veritas_store::Store;

	const TODAY: &str = "2026-11-18";

	fn token() -> veritas_session::CancelToken {
		veritas_session::CancelToken::default()
	}

	fn db(scratch: &Scratch) -> std::path::PathBuf {
		Path::new(&scratch.budget()).join(veritas_session::DB_FILE)
	}

	/// Every row of every table, by table, through a fresh connection.
	fn dump(db: &Path) -> BTreeMap<String, Vec<String>> {
		let connection = rusqlite::Connection::open(db).unwrap();
		let tables: Vec<String> = connection
			.prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
			.unwrap()
			.query_map([], |row| row.get(0))
			.unwrap()
			.map(Result::unwrap)
			.collect();
		tables
			.into_iter()
			.map(|table| {
				let mut select = connection
					.prepare(&format!("SELECT * FROM \"{table}\""))
					.unwrap();
				let width = select.column_count();
				let mut rows: Vec<String> = select
					.query_map([], |row| {
						let values: Vec<rusqlite::types::Value> =
							(0..width).map(|i| row.get(i)).collect::<Result<_, _>>()?;
						Ok(format!("{values:?}"))
					})
					.unwrap()
					.map(Result::unwrap)
					.collect();
				rows.sort();
				(table, rows)
			})
			.collect()
	}

	fn without_scenarios(mut dump: BTreeMap<String, Vec<String>>) -> BTreeMap<String, Vec<String>> {
		dump.retain(|table, _| !table.starts_with("veritas_scenario"));
		dump
	}

	fn data(core: &Core<Store>) -> BudgetData {
		core.call(|session| Ok(session.data()?.clone())).unwrap()
	}

	fn expense_and_account(core: &Core<Store>) -> (String, String) {
		let data = data(core);
		let account = data
			.accounts
			.values()
			.filter(|account| !account.tombstone && !account.closed && !account.off_budget)
			.min_by(|a, b| a.sort_order.total_cmp(&b.sort_order))
			.unwrap()
			.id
			.to_string();
		let category = data
			.categories
			.values()
			.filter(|category| {
				!category.tombstone
					&& data.category_kind(&category.id) == veritas_domain::CategoryKind::Expense
			})
			.min_by(|a, b| a.id.cmp(&b.id))
			.unwrap()
			.id
			.to_string();
		(account, category)
	}

	fn created_scenario(core: &Core<Store>, name: &str) -> String {
		let change = core
			.perform(Command::CreateScenario {
				name: name.to_owned(),
			})
			.unwrap();
		assert_eq!(change.entities.len(), 1);
		assert_eq!(change.entities[0].kind, EntityKind::Scenario);
		change.entities[0].id.clone().unwrap()
	}

	fn trip(account: &str, category: &str) -> ScenarioChange {
		ScenarioChange::OneTime {
			date: "2027-03-10".to_owned(),
			account_id: account.to_owned(),
			category_id: Some(category.to_owned()),
			amount: -200_000,
		}
	}

	#[test]
	fn r_fc_2_what_ifs_write_only_scenario_tables_one_undo_step_each() {
		// PLAN §10: adding a scenario changes only veritas_scenario* tables
		// (verified by dump diff), and each command is one undo step.
		let (scratch, core) = imported("all-features");
		let (account, category) = expense_and_account(&core);
		let original = dump(&db(&scratch));
		let id = created_scenario(&core, "Vacation");
		core.perform(Command::AddScenarioEvent {
			scenario_id: id.clone(),
			change: trip(&account, &category),
			today: TODAY.to_owned(),
		})
		.unwrap();
		core.perform(Command::AddScenarioEvent {
			scenario_id: id.clone(),
			change: ScenarioChange::Recurring {
				start: "2027-06-01".to_owned(),
				end_month: None,
				cadence: ScenarioCadence::EveryWeeks { count: 2 },
				account_id: account.clone(),
				category_id: None,
				amount: -30_000,
			},
			today: TODAY.to_owned(),
		})
		.unwrap();
		core.perform(Command::RenameScenario {
			scenario_id: id.clone(),
			name: "Vacation, longer".to_owned(),
		})
		.unwrap();
		let edited = dump(&db(&scratch));
		assert_eq!(
			without_scenarios(edited.clone()),
			without_scenarios(original.clone())
		);
		assert_eq!(edited["veritas_scenario"].len(), 1);
		assert_eq!(edited["veritas_scenario_event"].len(), 2);

		let listed = core.scenarios(TODAY).unwrap();
		assert_eq!(listed.len(), 1);
		assert_eq!(listed[0].name, "Vacation, longer");
		assert_eq!(listed[0].events.len(), 2);
		let (trip_event, other): (Vec<_>, Vec<_>) = listed[0]
			.events
			.iter()
			.partition(|event| event.change == trip(&account, &category));
		let event = trip_event[0].id.clone();
		core.perform(Command::EditScenarioEvent {
			scenario_id: id.clone(),
			event_id: event.clone(),
			change: ScenarioChange::CategoryPercent {
				category_id: category.clone(),
				percent_bp: -1_000,
				start_month: Month {
					year: 2027,
					month: 1,
				},
				end_month: None,
			},
			today: TODAY.to_owned(),
		})
		.unwrap();
		core.perform(Command::RemoveScenarioEvent {
			scenario_id: id.clone(),
			event_id: other[0].id.clone(),
		})
		.unwrap();
		assert_eq!(core.scenarios(TODAY).unwrap()[0].events.len(), 1);

		core.undo().unwrap();
		assert_eq!(
			core.scenarios(TODAY).unwrap()[0].events.len(),
			2,
			"one step"
		);
		core.undo().unwrap();
		assert!(
			core.scenarios(TODAY).unwrap()[0]
				.events
				.iter()
				.any(|event| event.change == trip(&account, &category))
		);
		core.undo().unwrap();
		assert_eq!(core.scenarios(TODAY).unwrap()[0].name, "Vacation");
		core.redo().unwrap();
		assert_eq!(dump(&db(&scratch)), edited);
		core.perform(Command::DeleteScenario {
			scenario_id: id.clone(),
		})
		.unwrap();
		assert!(core.scenarios(TODAY).unwrap().is_empty());
		core.undo().unwrap();
		assert_eq!(
			dump(&db(&scratch)),
			edited,
			"undoing a delete puts it all back"
		);
		for _ in 0..4 {
			core.undo().unwrap();
		}
		assert_eq!(dump(&db(&scratch)), original);

		core.redo().unwrap();
		drop(core);
		let core = reopened(&scratch);
		assert_eq!(core.scenarios(TODAY).unwrap()[0].name, "Vacation");
	}

	#[test]
	fn d67_a_new_what_if_dated_before_today_is_refused() {
		let (_scratch, core) = imported("all-features");
		let (account, category) = expense_and_account(&core);
		let id = created_scenario(&core, "Late");
		let past = ScenarioChange::OneTime {
			date: "2026-11-17".to_owned(),
			account_id: account.clone(),
			category_id: Some(category.clone()),
			amount: -200_000,
		};
		assert!(matches!(
			core.perform(Command::AddScenarioEvent {
				scenario_id: id.clone(),
				change: past,
				today: TODAY.to_owned(),
			}),
			Err(FfiError::Invalid { .. })
		));
		assert!(matches!(
			core.perform(Command::CreateScenario {
				name: " vacation ".to_owned(),
			})
			.and_then(|_| core.perform(Command::CreateScenario {
				name: "VACATION".to_owned(),
			})),
			Err(FfiError::Invalid { .. })
		));
		assert!(matches!(
			core.perform(Command::AddScenarioEvent {
				scenario_id: id,
				change: ScenarioChange::GoalContribution {
					goal_id: "no-such-goal".to_owned(),
					amount: 100,
					start_month: Month {
						year: 2027,
						month: 1,
					},
					end_month: None,
				},
				today: TODAY.to_owned(),
			}),
			Err(FfiError::NotFound { .. })
		));
	}

	#[test]
	fn r_fc_1_and_fc_3_a_one_time_what_if_moves_net_worth_from_its_month_on_never_the_band() {
		let (_scratch, core) = imported("all-features");
		let (account, category) = expense_and_account(&core);
		let id = created_scenario(&core, "Vacation");
		core.perform(Command::AddScenarioEvent {
			scenario_id: id.clone(),
			change: trip(&account, &category),
			today: TODAY.to_owned(),
		})
		.unwrap();
		let forecast = core
			.forecast(TODAY, std::slice::from_ref(&id), &token())
			.unwrap();
		let baseline = &forecast.baseline;
		assert_eq!(
			baseline
				.horizons
				.iter()
				.map(|h| h.months)
				.collect::<Vec<_>>(),
			[3, 6, 10, 12]
		);
		assert_eq!(baseline.months.len(), 13);
		assert_eq!(
			baseline.months[0].month,
			Month {
				year: 2026,
				month: 11
			}
		);
		let what_if = &forecast.scenarios[0];
		assert_eq!(what_if.scenario_id.as_deref(), Some(id.as_str()));
		assert_eq!(what_if.name, "Vacation");
		for (base, with) in baseline.months.iter().zip(&what_if.months) {
			let expected = if (base.month.year, base.month.month) >= (2027, 3) {
				-200_000
			} else {
				0
			};
			assert_eq!(
				with.net_worth - base.net_worth,
				expected,
				"{:?}",
				base.month
			);
		}
		for (base, with) in baseline.horizons.iter().zip(&what_if.horizons) {
			let moved = if (base.month.year, base.month.month) >= (2027, 3) {
				-200_000
			} else {
				0
			};
			let width = |h: &ForecastHorizon| h.band_high.zip(h.band_low).map(|(hi, lo)| hi - lo);
			assert_eq!(
				width(base),
				width(with),
				"FC-3: the band offsets don't move"
			);
			let balance = |h: &ForecastHorizon| {
				h.accounts
					.iter()
					.find(|row| row.account_id == account)
					.map(|row| row.balance)
			};
			assert_eq!(
				balance(with).unwrap() - balance(base).unwrap(),
				moved,
				"the account it posts to moves"
			);
		}
		assert_eq!(baseline.goal_etas, what_if.goal_etas);
		assert!(matches!(
			core.forecast(TODAY, &[id.clone(), id.clone(), id.clone(), id], &token()),
			Err(FfiError::Invalid { .. })
		));
		let cancelled = token();
		cancelled.cancel();
		assert!(matches!(
			core.forecast(TODAY, &[], &cancelled),
			Err(FfiError::Cancelled)
		));
	}

	#[test]
	fn fc_3_sn_5_a_goal_contribution_moves_only_the_goals_eta() {
		let (_scratch, core) = imported("all-features");
		let change = core
			.perform(Command::CreateGoal {
				name: "New laptop".to_owned(),
				target: 150_000,
				target_date: Some("2027-06-15".to_owned()),
				icon: None,
				color: None,
			})
			.unwrap();
		let goal = change
			.entities
			.iter()
			.find(|entity| entity.kind == EntityKind::Goal)
			.unwrap()
			.id
			.clone()
			.unwrap();
		let id = created_scenario(&core, "Save harder");
		core.perform(Command::AddScenarioEvent {
			scenario_id: id.clone(),
			change: ScenarioChange::GoalContribution {
				goal_id: goal.clone(),
				amount: 25_000,
				start_month: Month {
					year: 2027,
					month: 2,
				},
				end_month: None,
			},
			today: TODAY.to_owned(),
		})
		.unwrap();
		let forecast = core.forecast(TODAY, &[id], &token()).unwrap();
		let what_if = &forecast.scenarios[0];
		assert_eq!(forecast.baseline.months, what_if.months);
		assert_eq!(forecast.baseline.horizons, what_if.horizons);
		let eta = |projection: &ForecastProjection| {
			projection
				.goal_etas
				.iter()
				.find(|eta| eta.goal_id == goal)
				.unwrap()
				.clone()
		};
		assert_eq!(eta(&forecast.baseline).status, GoalEtaStatus::NoHistory);
		// FC-6 with a scenario: no history means no pace, so nothing to add the
		// extra to; the line stays "No ETA yet" until a month of budget exists.
		assert_eq!(eta(what_if).text, "No ETA yet");
	}

	#[test]
	fn r_fc_3_apply_suggestions_writes_every_amount_in_one_undo_step() {
		let (_scratch, core) = imported("all-features");
		let suggestions = core.budget_suggestions(TODAY, &token()).unwrap();
		assert_eq!(
			suggestions.month,
			Month {
				year: 2026,
				month: 12
			}
		);
		let suggested: Vec<(String, i64)> = suggestions
			.items
			.iter()
			.filter_map(|item| item.amount.map(|amount| (item.category_id.clone(), amount)))
			.collect();
		assert!(
			!suggested.is_empty(),
			"the fixture has history to suggest from"
		);
		let december = veritas_domain::YearMonth::new(2026, 12).unwrap();
		let budgeted = |data: &BudgetData| -> BTreeMap<CategoryId, i64> {
			data.budget_cells
				.iter()
				.filter(|((month, _), cell)| *month == december && cell.budgeted != Money::ZERO)
				.map(|((_, category), cell)| (category.clone(), cell.budgeted.minor()))
				.collect()
		};
		let before = budgeted(&data(&core));
		let change = core
			.perform(Command::ApplySuggestions {
				today: TODAY.to_owned(),
			})
			.unwrap();
		assert!(
			change
				.entities
				.iter()
				.all(|e| e.kind == EntityKind::BudgetCell)
		);
		let after = data(&core);
		for (category, amount) in &suggested {
			let cell = after
				.budget_cells
				.get(&(december, CategoryId::new(category.as_str())))
				.map_or(0, |cell| cell.budgeted.minor());
			assert_eq!(cell, *amount, "{category}");
		}
		core.undo().unwrap();
		assert_eq!(budgeted(&data(&core)), before, "one undo step");
	}

	#[test]
	fn cls_12_11_a_yen_budget_applies_suggestions_on_the_budget_scale() {
		let (_scratch, core) = imported("all-features");
		let dollars = core.budget_suggestions(TODAY, &token()).unwrap();
		let mut format = data(&core).format.clone();
		format.currency_code = Some("JPY".to_owned());
		format.hide_fraction = true;
		core.call(|session| {
			session.with_sheet(civil_date(TODAY)?, |data, sheet| {
				let mut yen = data.clone();
				yen.format = format.clone();
				let suggestions = core::suggestions(&yen, sheet, civil_date(TODAY)?)?;
				for (item, usd) in suggestions.items.iter().zip(&dollars.items) {
					let expected = match usd.basis {
						SuggestionBasis::Goal => usd.amount,
						_ => usd.amount.map(|amount| amount / 100),
					};
					assert_eq!(item.amount.map(Money::minor), expected, "{}", usd.name);
					assert!(!item.reason.contains('$'), "{}", item.reason);
				}
				let rows = core::apply_suggestions(&yen, sheet, civil_date(TODAY)?)?;
				for row in rows {
					let Row::BudgetCell(cell) = row else {
						panic!("only budget cells");
					};
					let item = suggestions
						.items
						.iter()
						.find(|item| item.category == cell.category)
						.unwrap();
					assert_eq!(Some(cell.budgeted), item.amount);
				}
				Ok(())
			})
		})
		.unwrap();
	}

	/// Actual's own balance forecast for `zip`, with its clock at `today`.
	fn actual_occurrences(zip: &Path, today: &str, end: &str) -> Option<serde_json::Value> {
		let oracle = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/actual-oracle");
		if !oracle.join("node_modules").exists() {
			assert!(
				std::env::var_os("VERITAS_REQUIRE_ORACLE").is_none(),
				"VERITAS_REQUIRE_ORACLE is set but tools/actual-oracle/node_modules is missing"
			);
			eprintln!("skipping the Actual side: run `cargo xtask oracle setup`");
			return None;
		}
		let output = std::process::Command::new("node")
			.current_dir(&oracle)
			.arg("src/main.ts")
			.arg("forecast-occurrences")
			.arg(zip)
			.arg(end)
			.args(["--today", today])
			.output()
			.expect("node runs");
		assert!(
			output.status.success(),
			"Actual couldn't forecast: {}",
			String::from_utf8_lossy(&output.stderr)
		);
		Some(serde_json::from_slice(&output.stdout).unwrap())
	}

	#[test]
	fn fc_2_known_flows_match_actuals_forecast_schedules() {
		// FC-2 against Actual's `forecast/generate` (server/forecast/
		// forecast-schedules.ts): the same occurrence dates for every bill,
		// and the same amount per date on the paying account for exact and
		// Between bills, loan payments included (their split adds back up to
		// the payment). About bills use the median of their payments (FC-2),
		// so only their dates are compared.
		let zip =
			Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/all-features.zip");
		let (_scratch, core) = imported("all-features");
		let forecast = core.forecast(TODAY, &[], &token()).unwrap();
		let end = forecast.baseline.months.last().unwrap().month;
		let end = veritas_reports::Period::month(
			veritas_domain::YearMonth::new(end.year, end.month).unwrap(),
		)
		.last()
		.to_string();
		let Some(actual) = actual_occurrences(&zip, TODAY, &end) else {
			return;
		};
		let about: Vec<String> = core
			.call(|session| crate::bills::bills(session.data()?, civil_date(TODAY)?))
			.unwrap()
			.into_iter()
			.filter(|bill| bill.amount_kind == Some(crate::BillAmountKind::About))
			.map(|bill| bill.id)
			.collect();
		let mut ours: BTreeMap<(String, String, String), i64> = BTreeMap::new();
		for flow in forecast
			.known_flows
			.iter()
			.filter(|flow| flow.date.as_str() >= TODAY)
		{
			let Some(bill) = &flow.bill_id else {
				continue;
			};
			*ours
				.entry((bill.clone(), flow.date.clone(), flow.account_id.clone()))
				.or_default() += flow.amount;
		}
		assert!(!ours.is_empty(), "the fixture has bills");
		let mut theirs: BTreeMap<(String, String, String), i64> = BTreeMap::new();
		for occurrence in actual.as_array().unwrap() {
			let text = |key: &str| occurrence[key].as_str().unwrap().to_owned();
			let key = (text("schedule"), text("date"), text("account"));
			if ours.contains_key(&key)
				|| !ours
					.keys()
					.any(|(bill, date, _)| (bill, date) == (&key.0, &key.1))
			{
				*theirs.entry(key).or_default() += occurrence["amount"].as_i64().unwrap();
			}
		}
		let dates = |map: &BTreeMap<(String, String, String), i64>| {
			map.keys()
				.map(|(bill, date, _)| (bill.clone(), date.clone()))
				.collect::<std::collections::BTreeSet<_>>()
		};
		assert_eq!(dates(&ours), dates(&theirs), "occurrence dates");
		for (key, amount) in &ours {
			if about.contains(&key.0) {
				continue;
			}
			assert_eq!(Some(amount), theirs.get(key), "{key:?}");
		}
	}
}
