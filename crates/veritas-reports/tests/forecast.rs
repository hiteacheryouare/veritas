//! D73: the balance forecast card, compared with Actual's own
//! `forecast/generate` handler and the card's chart code, run by the oracle
//! (`widgets`) on the same fixture, meta and bills.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use chrono::NaiveDate;
use serde_json::{Value, json};
use support::actual::{fixture, oracle, tracking_fixture};
use veritas_domain::{
	BudgetCell, BudgetData, CivilDate, ConditionsOp, Money, Rule, RuleId, RuleStage, Schedule,
	ScheduleId, YearMonth,
};
use veritas_engine::EnvelopeSheet;
use veritas_reports::{BalanceForecastCard, Forecast, ForecastSource, Json, balance_forecast_card};
use veritas_rules::{Op, next_date_from, parse_date_value};

const ORACLE_TODAY: &str = "2026-11-09";

fn today() -> NaiveDate {
	NaiveDate::parse_from_str(ORACLE_TODAY, "%Y-%m-%d").unwrap()
}

fn meta(value: &Value) -> Option<Json> {
	(!value.is_null()).then(|| Json::parse(&value.to_string()).unwrap())
}

fn day(date: NaiveDate) -> String {
	date.format("%Y-%m-%d").to_string()
}

fn id_of<'a>(names: impl Iterator<Item = (&'a str, String)>, name: &str) -> String {
	names
		.into_iter()
		.find(|(candidate, _)| *candidate == name)
		.map(|(_, id)| id)
		.unwrap_or_else(|| panic!("no {name}"))
}

fn account(budget: &BudgetData, name: &str) -> String {
	id_of(
		budget
			.accounts
			.values()
			.map(|a| (a.name.as_str(), a.id.to_string())),
		name,
	)
}

fn payee(budget: &BudgetData, name: &str) -> String {
	id_of(
		budget
			.payees
			.values()
			.filter(|p| !p.tombstone)
			.map(|p| (p.name.as_str(), p.id.to_string())),
		name,
	)
}

fn transfer_payee(budget: &BudgetData, account_name: &str) -> String {
	let target = account(budget, account_name);
	budget
		.payees
		.values()
		.find(|p| p.transfer_account.as_ref().map(ToString::to_string) == Some(target.clone()))
		.map(|p| p.id.to_string())
		.unwrap()
}

fn category(budget: &BudgetData, name: &str) -> String {
	id_of(
		budget
			.categories
			.values()
			.map(|c| (c.name.as_str(), c.id.to_string())),
		name,
	)
}

/// Adds a bill the way `schedule/create` stores it, and returns the oracle
/// request that makes the same bill in Actual.
fn add_bill(
	budget: &mut BudgetData,
	id: &str,
	name: &str,
	posts: bool,
	conditions: Value,
) -> Value {
	let date = conditions
		.as_array()
		.unwrap()
		.iter()
		.find(|condition| condition["field"] == "date")
		.unwrap();
	let op = Op::from_stored(date["op"].as_str().unwrap()).unwrap();
	let value = parse_date_value(op, date.get("value")).unwrap();
	let next = next_date_from(&value, CivilDate::from(today())).unwrap();
	let rule = RuleId::new(format!("{id}-rule"));
	budget.rules.insert(
		rule.clone(),
		Rule {
			id: rule.clone(),
			stage: RuleStage::Default,
			conditions_op: ConditionsOp::And,
			conditions_json: conditions.to_string(),
			actions_json: json!([{"op": "link-schedule", "value": id}]).to_string(),
			tombstone: false,
		},
	);
	budget.schedules.insert(
		ScheduleId::new(id),
		Schedule {
			id: ScheduleId::new(id),
			name: Some(name.to_owned()),
			rule,
			posts_transaction: posts,
			completed: false,
			next_date: next,
			tombstone: false,
		},
	);
	json!({
		"id": format!("create-{id}"),
		"type": "schedule-create",
		"meta": {"schedule": {"id": id, "name": name, "posts_transaction": posts}, "conditions": conditions}
	})
}

fn bill_sort_key(bill: &Value) -> String {
	bill.to_string()
}

/// `forecast/generate`'s result as Actual returns it, with each day's bills
/// in a fixed order, since Actual's tie between bills on one date follows
/// SQLite's row order.
fn sorted_bills(forecast: &Value) -> Value {
	let mut forecast = forecast.clone();
	if let Some(points) = forecast["dataPoints"].as_array_mut() {
		for point in points {
			if let Some(bills) = point["transactions"].as_array_mut() {
				bills.sort_by_key(bill_sort_key);
			}
		}
	}
	forecast
}

fn forecast_as_actual(forecast: &Forecast) -> Value {
	let points: Vec<Value> = forecast
		.points
		.iter()
		.map(|point| {
			let mut bills: Vec<Value> = point
				.bills
				.iter()
				.map(|bill| {
					json!({
						"amount": bill.amount.minor(),
						"payee": bill.payee,
						"scheduleId": bill.schedule.to_string(),
						"scheduleName": bill.schedule_name,
					})
				})
				.collect();
			bills.sort_by_key(bill_sort_key);
			json!({
				"date": day(point.date),
				"balance": point.balance.minor(),
				"accountId": point.account,
				"accountName": point.account_name,
				"transactions": bills,
			})
		})
		.collect();
	json!({
		"dataPoints": points,
		"lowestBalance": {
			"date": day(forecast.lowest.date),
			"balance": forecast.lowest.balance.minor(),
			"accountId": forecast.lowest.account,
			"accountName": forecast.lowest.account_name,
		},
		"forecastStartDate": day(forecast.start),
		"forecastEndDate": day(forecast.end),
	})
}

fn check_card(
	label: &str,
	ours: &Result<(BalanceForecastCard, Forecast), veritas_domain::CoreError>,
	theirs: &Value,
) {
	let (card, forecast) = match ours {
		Ok(ours) => ours,
		Err(error) => {
			assert!(
				theirs.get("error").is_some(),
				"{label}: we failed ({error}), Actual didn't: {theirs}"
			);
			return;
		}
	};
	assert!(
		theirs.get("error").is_none(),
		"{label}: Actual failed, we didn't: {theirs}"
	);
	assert_eq!(theirs["start"], json!(card.range.start), "{label} start");
	assert_eq!(theirs["end"], json!(card.range.end), "{label} end");
	let source = match card.source {
		ForecastSource::Schedules => "schedules",
		ForecastSource::TrackingBudget => "tracking-budget",
	};
	assert_eq!(theirs["source"], json!(source), "{label} source");
	assert_eq!(
		sorted_bills(&theirs["forecast"]),
		forecast_as_actual(forecast),
		"{label} forecast"
	);
	let points: Vec<Value> = card
		.points
		.iter()
		.map(|point| json!({"date": point.date, "balance": point.balance.minor()}))
		.collect();
	assert_eq!(theirs["points"], Value::Array(points), "{label} points");
	assert_eq!(
		theirs["scheduled"],
		json!(card.scheduled),
		"{label} scheduled"
	);
	let last = theirs["points"].as_array().and_then(|points| points.last());
	let low = match (&theirs["lowest"], last) {
		(Value::Object(_), Some(last)) if theirs["lowest"]["date"] != last["date"] => {
			theirs["lowest"].clone()
		}
		_ => Value::Null,
	};
	let ours_low = card.low.as_ref().map_or(
		Value::Null,
		|low| json!({"date": low.date, "balance": low.balance.minor()}),
	);
	assert_eq!(low, ours_low, "{label} low");
	let has_forecast = theirs["forecast"]["dataPoints"]
		.as_array()
		.is_some_and(|points| !points.is_empty());
	assert_eq!(card.has_forecast, has_forecast, "{label} has a forecast");
	let today_index = theirs["points"]
		.as_array()
		.and_then(|points| points.iter().position(|point| point["date"] == "2026-11"));
	assert_eq!(card.today, today_index, "{label} today line");
}

fn ours(
	budget: &BudgetData,
	sheet: &EnvelopeSheet,
	value: &Value,
) -> Result<(BalanceForecastCard, Forecast), veritas_domain::CoreError> {
	let meta = meta(value);
	let card = balance_forecast_card(budget, sheet, meta.as_ref(), today())?;
	let forecast = veritas_reports::card_forecast(budget, sheet, meta.as_ref(), today())?;
	Ok((card, forecast))
}

/// Runs every meta through both apps and compares them. Returns how many
/// cards had bills on their line, so a test can tell it compared something.
fn compare(budget_name: &str, setup: Vec<Value>, budget: &BudgetData, metas: &[Value]) -> usize {
	let mut requests = setup;
	let sheet = EnvelopeSheet::compute(budget, CivilDate::from(today())).unwrap();
	let expected: Vec<_> = metas
		.iter()
		.enumerate()
		.map(|(index, value)| {
			let id = format!("forecast-{index}");
			requests.push(json!({"id": id, "type": "forecast-card", "meta": value}));
			(id, value, ours(budget, &sheet, value))
		})
		.collect();
	let billed = expected
		.iter()
		.filter(|(_, _, ours)| ours.as_ref().is_ok_and(|(card, _)| card.scheduled > 0))
		.count();
	let Some(theirs) = oracle(
		"widgets",
		budget_name,
		&Value::Array(requests),
		ORACLE_TODAY,
	) else {
		return billed;
	};
	for (id, value, ours) in &expected {
		check_card(&format!("{id} {value}"), ours, &theirs[id.as_str()]);
	}
	billed
}

fn frames() -> Vec<Value> {
	vec![
		json!(null),
		json!({"start": "2026-09", "end": "2027-02", "mode": "static"}),
		json!({"start": "2026-01", "end": "2026-06", "mode": "static"}),
		json!({"start": "2026-05", "end": "2026-11", "mode": "sliding-window"}),
		json!({"start": "2026-11-01", "end": "2026-12-20", "mode": "sliding-window"}),
		json!({"start": "2026-11-05", "end": "2027-01-20", "mode": "static"}),
		json!({"mode": "yearToDate"}),
		json!({"mode": "lastMonth"}),
		json!({"start": "2026-10", "end": "2028-02", "mode": "static"}),
	]
}

fn card_metas(budget: &BudgetData) -> Vec<Value> {
	let checking = account(budget, "Checking");
	let visa = account(budget, "Visa Card");
	let savings = account(budget, "Savings");
	let old = account(budget, "Old Checking");
	let landlord = payee(budget, "Landlord");
	let mut metas = Vec::new();
	for frame in frames() {
		for granularity in ["Monthly", "Daily"] {
			let mut base = json!({"granularity": granularity});
			if !frame.is_null() {
				base["timeFrame"] = frame.clone();
			}
			metas.push(base.clone());
			let mut chosen = base.clone();
			chosen["accounts"] = json!([checking, visa]);
			metas.push(chosen);
		}
	}
	let with = |extra: Value| {
		let mut meta =
			json!({"timeFrame": {"start": "2026-11", "end": "2027-04", "mode": "static"}});
		for (key, value) in extra.as_object().unwrap() {
			meta[key] = value.clone();
		}
		meta
	};
	let conditions = [
		json!([{"field": "payee", "op": "is", "value": landlord, "type": "id"}]),
		json!([{"field": "amount", "op": "lt", "value": 0, "type": "number"}]),
		json!([{"field": "amount", "op": "gt", "value": 100000, "type": "number", "options": {"inflow": true}}]),
		json!([{"field": "account", "op": "contains", "value": "card", "type": "id"}]),
		json!([{"field": "account", "op": "is", "value": checking, "type": "id"}, {"field": "payee", "op": "is", "value": landlord, "type": "id"}]),
		json!([{"field": "account", "op": "oneOf", "value": [checking, savings], "type": "id"}]),
		json!([{"field": "account", "op": "onBudget", "value": null, "type": "id"}]),
		json!([{"field": "account", "op": "matches", "value": "^(che|vis)", "type": "id"}]),
		json!([{"field": "payee", "op": "contains", "value": "gym", "type": "id"}]),
		json!([{"field": "payee", "op": "doesNotContain", "value": "gym", "type": "id"}]),
		json!([{"field": "notes", "op": "doesNotContain", "value": "zzz", "type": "string"}]),
		json!([{"field": "notes", "op": "is", "value": "", "type": "string"}]),
		json!([{"field": "category", "op": "is", "value": null, "type": "id"}]),
		json!([{"field": "date", "op": "gte", "value": "2026-12-01", "type": "date"}]),
		json!([{"field": "date", "op": "is", "value": "2026-12", "type": "date"}]),
		json!([{"field": "cleared", "op": "is", "value": false, "type": "boolean"}]),
		json!([{"field": "transfer", "op": "is", "value": true, "type": "boolean"}]),
	];
	for list in &conditions {
		for op in ["and", "or"] {
			metas.push(with(json!({"conditions": list, "conditionsOp": op})));
		}
		metas.push(with(
			json!({"conditions": list, "accounts": [checking, savings], "granularity": "Daily"}),
		));
	}
	metas.push(with(json!({"accounts": []})));
	metas.push(with(json!({"accounts": [old]})));
	metas.push(with(json!({"accounts": null})));
	metas.push(with(json!({"source": "tracking-budget"})));
	metas.push(with(
		json!({"yAxisBase": "Zero-Based", "granularity": "Weekly"}),
	));
	metas.push(with(json!({"conditions": [{"field": "payee", "op": "is", "value": landlord, "type": "id", "customName": "saved"}]})));
	metas
}

#[test]
fn d73_balance_forecast_card_matches_actual() {
	let budget = fixture("all-features");
	let metas = card_metas(&budget);
	assert!(compare("all-features", Vec::new(), &budget, &metas) > metas.len() / 2);
}

/// Transfers, card payments, accountless bills and rule-set categories, made
/// in both apps the way the schedule editor saves them.
#[test]
fn d73_balance_forecast_card_forecasts_new_bills() {
	let mut budget = fixture("all-features");
	let checking = account(&budget, "Checking");
	let cafe = payee(&budget, "Bean There Cafe");
	let monthly = |start: &str, skip: bool| json!({"start": start, "frequency": "monthly", "interval": 1, "patterns": [], "skipWeekend": skip, "weekendSolveMode": "after", "endMode": "never"});
	let bills = [
		(
			"forecast-savings",
			"Savings transfer",
			false,
			json!([
				{"op": "is", "field": "payee", "value": transfer_payee(&budget, "Savings")},
				{"op": "is", "field": "account", "value": checking},
				{"op": "isapprox", "field": "date", "value": monthly("2026-11-20", true)},
				{"op": "isapprox", "field": "amount", "value": -25000}
			]),
		),
		(
			"forecast-visa",
			"Card payment",
			true,
			json!([
				{"op": "is", "field": "payee", "value": transfer_payee(&budget, "Visa Card")},
				{"op": "is", "field": "account", "value": checking},
				{"op": "is", "field": "date", "value": monthly("2026-11-25", false)},
				{"op": "is", "field": "amount", "value": -40000}
			]),
		),
		(
			"forecast-brokerage",
			"Invest",
			false,
			json!([
				{"op": "is", "field": "payee", "value": transfer_payee(&budget, "Brokerage")},
				{"op": "is", "field": "account", "value": checking},
				{"op": "isapprox", "field": "date", "value": {"start": "2026-11-12", "frequency": "weekly", "interval": 2, "patterns": [], "skipWeekend": false, "weekendSolveMode": "after", "endMode": "after_n_occurrences", "endOccurrences": 6}},
				{"op": "isbetween", "field": "amount", "value": {"num1": -10001, "num2": -20000}}
			]),
		),
		(
			"forecast-coffee",
			"Coffee club",
			false,
			json!([
				{"op": "is", "field": "payee", "value": cafe},
				{"op": "isapprox", "field": "date", "value": monthly("2026-11-15", false)},
				{"op": "is", "field": "amount", "value": -1200}
			]),
		),
		(
			"forecast-once",
			"Deposit",
			false,
			json!([
				{"op": "is", "field": "account", "value": account(&budget, "Savings")},
				{"op": "is", "field": "date", "value": "2027-01-04"},
				{"op": "is", "field": "amount", "value": 52000}
			]),
		),
	];
	let mut setup = Vec::new();
	for (id, name, posts, conditions) in bills {
		setup.push(add_bill(&mut budget, id, name, posts, conditions));
	}
	let coffee_category = category(&budget, "Dining Out");
	let mut metas = card_metas(&budget);
	let frame = json!({"start": "2026-11", "end": "2027-03", "mode": "static"});
	for conditions in [
		json!([{"field": "category", "op": "is", "value": coffee_category, "type": "id"}]),
		json!([{"field": "category", "op": "isNot", "value": null, "type": "id"}]),
		json!([{"field": "category_group", "op": "is", "value": "b850ef40-801c-488b-9843-186722e75add", "type": "id"}]),
		json!([{"field": "category", "op": "contains", "value": "din", "type": "id"}]),
		json!([{"field": "payee", "op": "contains", "value": "sav", "type": "id"}]),
		json!([{"field": "payee", "op": "matches", "value": "^bean", "type": "id"}]),
		json!([{"field": "account", "op": "offBudget", "value": null, "type": "id"}]),
		json!([{"field": "transfer", "op": "is", "value": false, "type": "boolean"}]),
	] {
		metas.push(json!({"timeFrame": frame, "conditions": conditions}));
		metas.push(json!({"timeFrame": frame, "conditions": conditions, "granularity": "Daily", "accounts": [checking]}));
	}
	assert!(compare("all-features", setup, &budget, &metas) > metas.len() / 2);
}

#[test]
fn d73_balance_forecast_card_projects_a_tracking_budget() {
	let mut budget = tracking_fixture("all-features");
	let mut setup =
		vec![json!({"id": "switch", "type": "budget-type", "meta": {"type": "tracking"}})];
	for (month, name, amount) in [
		("2026-11", "Income", 450_000),
		("2026-11", "Interest", 1_000),
		("2026-11", "Groceries", 60_000),
		("2026-11", "Gifts", 9_000),
		("2026-11", "Old Hobby", 7_000),
		("2026-12", "Income", 300_000),
		("2026-12", "Rent", 180_000),
		("2027-02", "Groceries", 500_000),
		("2027-11", "Income", 10_000),
	] {
		let id = category(&budget, name);
		let month_key = YearMonth::parse(month).unwrap();
		let cell = budget
			.budget_cells
			.entry((month_key, veritas_domain::CategoryId::new(id.as_str())))
			.or_insert_with(|| BudgetCell {
				month: month_key,
				category: veritas_domain::CategoryId::new(id.as_str()),
				budgeted: Money::ZERO,
				carryover: false,
				goal: None,
				long_goal: false,
			});
		cell.budgeted = Money::from_minor(amount);
		setup.push(json!({"id": format!("edit-{month}-{name}"), "type": "budget-amount", "meta": {"month": month, "category": id, "amount": amount}}));
	}
	let mut metas = Vec::new();
	for frame in frames() {
		for granularity in ["Monthly", "Daily"] {
			let mut meta = json!({"source": "tracking-budget", "granularity": granularity, "conditions": [{"field": "amount", "op": "lt", "value": 0, "type": "number"}]});
			if !frame.is_null() {
				meta["timeFrame"] = frame.clone();
			}
			metas.push(meta);
		}
	}
	metas.push(json!({"source": "schedules"}));
	compare("all-features", setup, &budget, &metas);
	let sheet = EnvelopeSheet::compute(&budget, CivilDate::from(today())).unwrap();
	let card = balance_forecast_card(&budget, &sheet, meta(&metas[0]).as_ref(), today()).unwrap();
	assert_eq!(card.source, ForecastSource::TrackingBudget);
	assert!(
		card.points
			.windows(2)
			.any(|pair| pair[0].balance != pair[1].balance)
	);
}
