//! The balance forecast card (`BalanceForecastCard.tsx` and
//! `balanceForecastChartData.ts`): `forecast/generate` for the card's
//! accounts, filters and whole months, then the accounts' combined balance
//! per day or at the end of each month.

use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveDate;
use veritas_domain::{BudgetData, BudgetType, CivilDate, CoreError, Money, YearMonth};
use veritas_engine::EnvelopeSheet;

use super::forecast::{
	Forecast, ForecastRequest, ForecastSource, generate_forecast, last_of_month, live_account_ids,
};
use super::{TimeRange, calculate_time_range};
use crate::json::Json;
use crate::report_query::{day, parse_date};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForecastGranularity {
	Daily,
	Monthly,
}

/// One point on the card's line: `yyyy-MM` when monthly, `yyyy-MM-dd` when
/// daily.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForecastChartPoint {
	pub date: String,
	pub balance: Money,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BalanceForecastCard {
	pub range: TimeRange,
	pub source: ForecastSource,
	pub granularity: ForecastGranularity,
	pub zero_based: bool,
	/// False when the forecast has no accounts to project, and the card
	/// shows its empty message instead of a line.
	pub has_forecast: bool,
	pub points: Vec<ForecastChartPoint>,
	/// The lowest point (the first, on a tie) when it isn't the last point,
	/// which is all the card shows it for.
	pub low: Option<ForecastChartPoint>,
	/// Bill occurrences on the days the line shows, a bill counted once per
	/// day across accounts and transfer legs.
	pub scheduled: usize,
	/// The card has filters, and they apply (not with the tracking source).
	pub filtered: bool,
	/// The point for this month, where the card draws its today line. Only a
	/// monthly line has one.
	pub today: Option<usize>,
}

fn is_month(text: &str) -> bool {
	text.len() == 7 && YearMonth::parse(text).is_ok()
}

fn month_of(text: &str) -> Result<YearMonth, CoreError> {
	text.get(..7)
		.map(YearMonth::parse)
		.unwrap_or_else(|| Err(CoreError::Invalid(format!("a card month {text:?}"))))
}

/// A bound as a day: a month's first or last day, or the day itself.
fn day_of(text: &str, last: bool) -> Result<NaiveDate, CoreError> {
	if is_month(text) {
		let month = month_of(text)?;
		return if last {
			last_of_month(month)
		} else {
			Ok(month.first_day().naive())
		};
	}
	parse_date(text).ok_or_else(|| CoreError::Invalid(format!("a card date {text:?}")))
}

/// Each date's accounts summed, the last point of an account winning when
/// `key` puts several in one bucket.
fn combined(
	forecast: &Forecast,
	key: impl Fn(NaiveDate) -> String,
) -> Result<BTreeMap<String, Money>, CoreError> {
	let mut by_key: BTreeMap<String, BTreeMap<&str, Money>> = BTreeMap::new();
	for point in &forecast.points {
		by_key
			.entry(key(point.date))
			.or_default()
			.insert(point.account.as_str(), point.balance);
	}
	by_key
		.into_iter()
		.map(|(key, balances)| Ok((key, Money::checked_sum(balances.into_values())?)))
		.collect()
}

/// `buildBalanceForecastChartData`.
fn chart(
	forecast: &Forecast,
	range: &TimeRange,
	granularity: ForecastGranularity,
) -> Result<Vec<ForecastChartPoint>, CoreError> {
	if forecast.points.is_empty() {
		return Ok(Vec::new());
	}
	let mut points = Vec::new();
	match granularity {
		ForecastGranularity::Daily => {
			let by_day = combined(forecast, day)?;
			let start = day_of(&range.start, false)?;
			let end = day_of(&range.end, true)?;
			let first = day(start);
			let mut running = by_day
				.range(..first)
				.next_back()
				.map_or(Money::ZERO, |(_, balance)| *balance);
			for date in start.iter_days().take_while(|date| *date <= end) {
				let date = day(date);
				if let Some(balance) = by_day.get(&date) {
					running = *balance;
				}
				points.push(ForecastChartPoint {
					date,
					balance: running,
				});
			}
		}
		ForecastGranularity::Monthly => {
			let by_month = combined(forecast, |date| date.format("%Y-%m").to_string())?;
			let mut running = Money::ZERO;
			let last = month_of(&range.end)?;
			let mut month = month_of(&range.start)?;
			while month <= last {
				let key = month.to_string();
				if let Some(balance) = by_month.get(&key) {
					running = *balance;
				}
				points.push(ForecastChartPoint {
					date: key,
					balance: running,
				});
				month = month.offset(1)?;
			}
		}
	}
	Ok(points)
}

/// `countForecastScheduledOccurrences`.
fn scheduled_count(
	forecast: &Forecast,
	range: &TimeRange,
	granularity: ForecastGranularity,
) -> Result<usize, CoreError> {
	let exact = |text: &str| granularity == ForecastGranularity::Daily && !is_month(text);
	let start = if exact(&range.start) {
		day_of(&range.start, false)?
	} else {
		month_of(&range.start)?.first_day().naive()
	};
	let end = if exact(&range.end) {
		day_of(&range.end, true)?
	} else {
		last_of_month(month_of(&range.end)?)?
	};
	let mut seen = BTreeSet::new();
	for point in &forecast.points {
		if point.date < start || point.date > end {
			continue;
		}
		for bill in &point.bills {
			seen.insert((point.date, &bill.schedule));
		}
	}
	Ok(seen.len())
}

/// The forecast request the card makes for its meta.
fn card_request<'a>(
	budget: &BudgetData,
	meta: Option<&'a Json>,
	today: NaiveDate,
) -> Result<(TimeRange, ForecastRequest<'a>), CoreError> {
	let field = |name: &str| meta.and_then(|meta| meta.get(name));
	let source = if field("source").and_then(Json::as_str) == Some("tracking-budget")
		&& budget.budget_type == BudgetType::Tracking
	{
		ForecastSource::TrackingBudget
	} else {
		ForecastSource::Schedules
	};
	let this_month = CivilDate::from(today).year_month();
	let default_start = this_month.to_string();
	let default_end = this_month.offset(11)?.to_string();
	let range = calculate_time_range(
		field("timeFrame"),
		Some((&default_start, &default_end, "static")),
		None,
		today,
	);
	let accounts = match field("accounts") {
		None | Some(Json::Null) => None,
		Some(Json::Array(ids)) => Some(
			ids.iter()
				.map(|id| {
					id.as_str().map(str::to_owned).ok_or_else(|| {
						CoreError::Invalid("the forecast card's accounts".to_owned())
					})
				})
				.collect::<Result<Vec<_>, _>>()?,
		),
		Some(_) => {
			return Err(CoreError::Invalid(
				"the forecast card's accounts".to_owned(),
			));
		}
	};
	let tracking = source == ForecastSource::TrackingBudget;
	let request = ForecastRequest {
		accounts: Some(accounts.unwrap_or_else(|| live_account_ids(budget))),
		conditions: if tracking {
			&[]
		} else {
			field("conditions")
				.and_then(Json::as_array)
				.unwrap_or_default()
		},
		any: !tracking && field("conditionsOp").and_then(Json::as_str) == Some("or"),
		start: month_of(&range.start)?.first_day().naive(),
		end: last_of_month(month_of(&range.end)?)?,
		include_accountless: !tracking && field("accounts").is_none(),
		source,
	};
	Ok((range, request))
}

/// The forecast behind the card for `meta`: what `forecast/generate`
/// returns for the request the card makes.
pub fn card_forecast(
	budget: &BudgetData,
	sheet: &EnvelopeSheet,
	meta: Option<&Json>,
	today: NaiveDate,
) -> Result<Forecast, CoreError> {
	let (_, request) = card_request(budget, meta, today)?;
	generate_forecast(budget, sheet, &request, today)
}

/// The balance forecast card for `meta`, as of `today`.
pub fn balance_forecast_card(
	budget: &BudgetData,
	sheet: &EnvelopeSheet,
	meta: Option<&Json>,
	today: NaiveDate,
) -> Result<BalanceForecastCard, CoreError> {
	let (range, request) = card_request(budget, meta, today)?;
	let field = |name: &str| meta.and_then(|meta| meta.get(name));
	let granularity = match field("granularity").and_then(Json::as_str) {
		Some("Daily") => ForecastGranularity::Daily,
		_ => ForecastGranularity::Monthly,
	};
	let forecast = generate_forecast(budget, sheet, &request, today)?;
	let points = chart(&forecast, &range, granularity)?;
	let lowest = points
		.iter()
		.fold(None, |low: Option<&ForecastChartPoint>, point| match low {
			Some(low) if point.balance >= low.balance => Some(low),
			_ => Some(point),
		});
	let low = lowest
		.filter(|low| points.last().is_some_and(|last| last.date != low.date))
		.cloned();
	let this_month = CivilDate::from(today).year_month().to_string();
	Ok(BalanceForecastCard {
		scheduled: scheduled_count(&forecast, &range, granularity)?,
		today: points.iter().position(|point| point.date == this_month),
		filtered: request.source == ForecastSource::Schedules && !request.conditions.is_empty(),
		zero_based: field("yAxisBase").and_then(Json::as_str) == Some("Zero-Based"),
		has_forecast: !forecast.points.is_empty(),
		points,
		low,
		range,
		source: request.source,
		granularity,
	})
}
