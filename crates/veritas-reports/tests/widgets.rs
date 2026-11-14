//! D73: every Actual dashboard card computed natively. Each card's numbers
//! are compared with Actual's own card and spreadsheet code, run by the
//! oracle (`widgets`) on the same fixture and meta.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use chrono::NaiveDate;
use serde_json::{Value, json};
use support::actual::{fixture, oracle, tracking_fixture};
use veritas_domain::BudgetData;
use veritas_domain::CivilDate;
use veritas_engine::EnvelopeSheet;
use veritas_reports::{
	Granularity, Json, Spending, Trend, age_of_money, budget_analysis, calendar, cash_flow_card,
	crossover, spending, summary,
};

const ORACLE_TODAY: &str = "2026-11-09";

fn today() -> NaiveDate {
	NaiveDate::parse_from_str(ORACLE_TODAY, "%Y-%m-%d").unwrap()
}

fn meta(value: &Value) -> Option<Json> {
	(!value.is_null()).then(|| Json::parse(&value.to_string()).unwrap())
}

/// A number as the oracle writes it: integers as JSON, anything else as its
/// JS text.
#[allow(clippy::cast_possible_truncation, clippy::float_cmp)]
fn number(value: f64) -> Value {
	if value.is_finite() && value.fract() == 0.0 && value.abs() < 9e15 {
		json!(value as i64)
	} else if value.is_nan() {
		json!("NaN")
	} else if value.is_infinite() {
		json!(if value > 0.0 { "Infinity" } else { "-Infinity" })
	} else {
		json!(value.to_string())
	}
}

fn same_number(ours: f64, theirs: &Value, what: &str) {
	assert_eq!(&number(ours), theirs, "{what}");
}

/// Runs every case through the oracle, or `None` when it isn't installed.
fn theirs(budget: &str, cases: &[(String, &str, Value)], first_day_of_week: &str) -> Option<Value> {
	let requests: Vec<Value> = cases
		.iter()
		.map(|(id, kind, meta)| {
			json!({"id": id, "type": kind, "meta": meta, "firstDayOfWeekIdx": first_day_of_week})
		})
		.collect();
	oracle("widgets", budget, &Value::Array(requests), ORACLE_TODAY)
}

fn category(budget: &BudgetData, name: &str) -> String {
	budget
		.categories
		.values()
		.find(|category| category.name == name)
		.map(|category| category.id.to_string())
		.unwrap()
}

#[test]
fn d73_summary_card_matches_actual() {
	let budget = fixture("all-features");
	let dining = category(&budget, "Dining Out");
	let static_frame = json!({"start": "2026-01", "end": "2026-10", "mode": "static"});
	let dining_only = json!([{"field": "category", "op": "is", "value": dining, "type": "id"}]);
	let outflows = json!([{"field": "amount", "op": "lt", "value": 0, "type": "number"}]);
	let mut metas = vec![json!(null), json!({"content": "not json"})];
	for content in [
		json!({"type": "sum"}),
		json!({"type": "avgPerMonth"}),
		json!({"type": "avgPerYear"}),
		json!({"type": "avgPerTransact"}),
		json!({"type": "percentage", "divisorConditions": outflows, "divisorConditionsOp": "and"}),
		json!({"type": "percentage", "divisorConditions": outflows, "divisorConditionsOp": "and", "divisorAllTimeDateRange": true}),
		json!({"type": "percentage", "divisorConditions": [{"field": "notes", "op": "is", "value": "nothing has this", "type": "string"}], "divisorConditionsOp": "or"}),
	] {
		for frame in [
			static_frame.clone(),
			json!({"start": "2026-04", "end": "2026-11", "mode": "sliding-window"}),
			json!({"start": "2025-11-01", "end": "2026-11-09", "mode": "full"}),
			json!({"start": "2026-01-15", "end": "2026-02-14", "mode": "sliding-window"}),
			json!({"mode": "lastYear"}),
			json!({"mode": "priorYearToDate"}),
			json!({"mode": "previousQuarter"}),
		] {
			metas.push(json!({"content": content.to_string(), "timeFrame": frame, "conditions": dining_only, "conditionsOp": "and"}));
			metas.push(json!({"content": content.to_string(), "timeFrame": frame}));
		}
	}
	let cases: Vec<(String, &str, Value)> = metas
		.into_iter()
		.enumerate()
		.map(|(index, meta)| (format!("summary-{index}"), "summary-card", meta))
		.collect();
	let ours: Vec<_> = cases
		.iter()
		.map(|(_, _, value)| summary(&budget, meta(value).as_ref(), today()))
		.collect();
	let Some(theirs) = theirs("all-features", &cases, "0") else {
		return;
	};
	for ((id, _, value), ours) in cases.iter().zip(ours) {
		let expected = &theirs[id.as_str()];
		let Ok(ours) = ours else {
			assert!(
				expected.get("error").is_some(),
				"{value}: we failed, Actual didn't: {expected}"
			);
			continue;
		};
		let data = &expected["data"];
		assert!(
			!data.is_null(),
			"{value}: Actual showed nothing: {expected}"
		);
		same_number(ours.total, &data["total"], &format!("{value} total"));
		same_number(
			ours.dividend,
			&data["dividend"],
			&format!("{value} dividend"),
		);
		same_number(ours.divisor, &data["divisor"], &format!("{value} divisor"));
	}
}

#[test]
fn d73_calendar_card_matches_actual() {
	let budget = fixture("all-features");
	let dining = category(&budget, "Dining Out");
	let cases: Vec<(String, &str, Value)> = [
		json!(null),
		json!({"timeFrame": {"start": "2025-11", "end": "2026-11", "mode": "static"}}),
		json!({"timeFrame": {"start": "2026-02", "end": "2026-04", "mode": "static"}, "conditions": [{"field": "category", "op": "isNot", "value": dining, "type": "id"}], "conditionsOp": "and"}),
		json!({"timeFrame": {"start": "2026-08", "end": "2026-11", "mode": "sliding-window"}}),
	]
	.into_iter()
	.enumerate()
	.map(|(index, meta)| (format!("calendar-{index}"), "calendar-card", meta))
	.collect();
	for first_day_of_week in [0, 1] {
		let ours: Vec<_> = cases
			.iter()
			.map(|(_, _, value)| {
				calendar(&budget, meta(value).as_ref(), today(), first_day_of_week).unwrap()
			})
			.collect();
		let Some(theirs) = theirs("all-features", &cases, &first_day_of_week.to_string()) else {
			return;
		};
		for ((id, _, value), months) in cases.iter().zip(ours) {
			let expected = theirs[id.as_str()]["data"]["calendarData"]
				.as_array()
				.unwrap();
			let ours: Vec<Value> = months
				.iter()
				.map(|month| {
					json!({
						"start": month.start.to_string(),
						"end": month.end.to_string(),
						"totalIncome": month.total_income.minor(),
						"totalExpense": month.total_expense.minor(),
						"data": month.days.iter().map(|day| json!({
							"date": day.date.to_string(),
							"incomeValue": day.income.minor(),
							"expenseValue": day.expense.minor(),
							"incomeSize": number(day.income_size),
							"expenseSize": number(day.expense_size),
						})).collect::<Vec<_>>(),
					})
				})
				.collect();
			assert_eq!(
				&ours, expected,
				"{value} with weeks from {first_day_of_week}"
			);
		}
	}
}

fn account(budget: &BudgetData, name: &str) -> String {
	budget
		.accounts
		.values()
		.find(|account| account.name == name)
		.map(|account| account.id.to_string())
		.unwrap()
}

#[test]
fn d73_age_of_money_card_matches_actual() {
	let budget = fixture("all-features");
	let checking = account(&budget, "Checking");
	let savings = account(&budget, "Savings");
	let visa = account(&budget, "Visa Card");
	let year = json!({"start": "2025-11", "end": "2026-11", "mode": "static"});
	let mut metas = vec![json!(null), json!({"timeFrame": year})];
	for granularity in ["daily", "weekly", "monthly"] {
		metas.push(json!({"timeFrame": {"start": "2026-03", "end": "2026-11", "mode": "sliding-window"}, "granularity": granularity}));
	}
	for (conditions, op) in [
		(
			json!([{"field": "account", "op": "is", "value": checking, "type": "id"}]),
			"and",
		),
		(
			json!([{"field": "account", "op": "isNot", "value": visa, "type": "id"}]),
			"and",
		),
		(
			json!([{"field": "account", "op": "oneOf", "value": [checking, savings], "type": "id"}]),
			"and",
		),
		(
			json!([{"field": "account", "op": "notOneOf", "value": [visa], "type": "id"}]),
			"and",
		),
		(
			json!([{"field": "account", "op": "contains", "value": "check", "type": "id"}]),
			"and",
		),
		(
			json!([{"field": "account", "op": "doesNotContain", "value": "card", "type": "id"}]),
			"and",
		),
		(
			json!([{"field": "account", "op": "onBudget", "value": null, "type": "id"}]),
			"and",
		),
		(
			json!([{"field": "account", "op": "matches", "value": "^Che", "type": "id"}]),
			"and",
		),
		(
			json!([{"field": "account", "op": "is", "value": checking, "type": "id"}, {"field": "account", "op": "is", "value": visa, "type": "id"}]),
			"or",
		),
		(
			json!([{"field": "account", "op": "isNot", "value": checking, "type": "id"}, {"field": "account", "op": "isNot", "value": savings, "type": "id"}]),
			"and",
		),
		(
			json!([{"field": "amount", "op": "gt", "value": -50000, "type": "number"}]),
			"and",
		),
	] {
		metas.push(json!({"timeFrame": year, "conditions": conditions, "conditionsOp": op, "granularity": "weekly"}));
	}
	let cases: Vec<(String, &str, Value)> = metas
		.into_iter()
		.enumerate()
		.map(|(index, meta)| (format!("aom-{index}"), "age-of-money-card", meta))
		.collect();
	let ours: Vec<_> = cases
		.iter()
		.map(|(_, _, value)| age_of_money(&budget, meta(value).as_ref(), today()).unwrap())
		.collect();
	let Some(theirs) = theirs("all-features", &cases, "0") else {
		return;
	};
	for ((id, _, value), ours) in cases.iter().zip(ours) {
		let data = &theirs[id.as_str()]["data"];
		let label = |date: NaiveDate| match ours.granularity {
			Granularity::Monthly => date.format("%b %Y").to_string(),
			_ => date.format("%b %-d, %Y").to_string(),
		};
		let graph: Vec<Value> = ours
			.points
			.iter()
			.map(|point| json!({"date": label(point.period), "ageOfMoney": point.age}))
			.collect();
		let trend = match ours.trend {
			Trend::Up => "up",
			Trend::Down => "down",
			Trend::Stable => "stable",
		};
		let ours = json!({
			"graphData": graph,
			"currentAge": ours.current_age,
			"trend": trend,
			"insufficientData": ours.insufficient_data,
		});
		assert_eq!(&ours, data, "{value}");
	}
}

fn group(budget: &BudgetData, name: &str) -> String {
	budget
		.category_groups
		.values()
		.find(|group| group.name == name)
		.map(|group| group.id.to_string())
		.unwrap()
}

#[test]
fn d73_budget_analysis_card_matches_actual() {
	let budget = fixture("all-features");
	let sheet = EnvelopeSheet::compute(&budget, CivilDate::new(2026, 11, 9).unwrap()).unwrap();
	let groceries = category(&budget, "Groceries");
	let dining = category(&budget, "Dining Out");
	let everyday = group(&budget, "Everyday");
	let year = json!({"start": "2025-10", "end": "2027-02", "mode": "static"});
	let mut metas = vec![
		json!(null),
		json!({"timeFrame": year}),
		json!({"timeFrame": year, "showHiddenCategories": true}),
	];
	for (conditions, op) in [
		(
			json!([{"field": "category", "op": "is", "value": groceries, "type": "id"}]),
			"and",
		),
		(
			json!([{"field": "category", "op": "oneOf", "value": [groceries, dining], "type": "id"}]),
			"and",
		),
		(
			json!([{"field": "category", "op": "notOneOf", "value": [groceries], "type": "id"}]),
			"and",
		),
		(
			json!([{"field": "category_group", "op": "is", "value": everyday, "type": "id"}, {"field": "category", "op": "isNot", "value": dining, "type": "id"}]),
			"and",
		),
		(
			json!([{"field": "category_group", "op": "contains", "value": "BILL", "type": "id"}, {"field": "category", "op": "is", "value": groceries, "type": "id"}]),
			"or",
		),
		(
			json!([{"field": "category", "op": "doesNotContain", "value": "fund", "type": "id"}]),
			"and",
		),
		(
			json!([{"field": "category", "op": "matches", "value": "^g", "type": "id"}]),
			"and",
		),
		(
			json!([{"field": "amount", "op": "gt", "value": 0, "type": "number"}]),
			"and",
		),
	] {
		metas.push(json!({"timeFrame": year, "conditions": conditions, "conditionsOp": op, "showHiddenCategories": true}));
	}
	let cases: Vec<(String, &str, Value)> = metas
		.into_iter()
		.enumerate()
		.map(|(index, meta)| (format!("analysis-{index}"), "budget-analysis-card", meta))
		.collect();
	let ours: Vec<_> = cases
		.iter()
		.map(|(_, _, value)| {
			budget_analysis(&budget, &sheet, meta(value).as_ref(), today()).unwrap()
		})
		.collect();
	let Some(theirs) = theirs("all-features", &cases, "0") else {
		return;
	};
	for ((id, _, value), ours) in cases.iter().zip(ours) {
		let data = &theirs[id.as_str()]["data"];
		let ours = json!({
			"intervalData": ours.months.iter().map(|month| json!({
				"date": month.month.to_string(),
				"budgeted": month.budgeted.minor(),
				"spent": month.spent.minor(),
				"balance": month.balance.minor(),
				"overspendingAdjustment": month.overspending_adjustment.minor(),
			})).collect::<Vec<_>>(),
			"startDate": ours.start.to_string(),
			"endDate": ours.end.to_string(),
			"totalBudgeted": ours.total_budgeted.minor(),
			"totalSpent": ours.total_spent.minor(),
			"totalOverspendingAdjustment": ours.total_overspending_adjustment.minor(),
			"finalOverspendingAdjustment": ours.final_overspending_adjustment.minor(),
		});
		assert_eq!(&ours, data, "{value}");
	}
}

fn optional(value: Option<f64>) -> Value {
	value.map_or(Value::Null, number)
}

#[test]
fn d73_crossover_card_matches_actual() {
	let budget = fixture("all-features");
	let brokerage = account(&budget, "Brokerage");
	let savings = account(&budget, "Savings");
	let groceries = category(&budget, "Groceries");
	let dining = category(&budget, "Dining Out");
	let mut metas = vec![json!(null), json!({"incomeAccountIds": []})];
	for projection in ["hampel", "median", "mean"] {
		metas.push(json!({"incomeAccountIds": [brokerage, savings], "projectionType": projection, "expectedContribution": 50000}));
	}
	metas.extend([
		json!({"incomeAccountIds": [brokerage], "expenseCategoryIds": [groceries, dining], "safeWithdrawalRate": 0.035, "estimatedReturn": 0.07}),
		json!({"incomeAccountIds": [savings], "expenseCategoryIds": [], "timeFrame": {"start": "2026-02", "end": "2026-08", "mode": "static"}}),
		json!({"timeFrame": {"start": "2026-03", "end": "2026-09", "mode": "sliding-window"}, "showHiddenCategories": true, "expenseAdjustmentFactor": 1.25}),
		json!({"incomeAccountIds": [brokerage], "timeFrame": {"start": "2020-01", "end": "2030-01", "mode": "static"}, "estimatedReturn": 0.0}),
	]);
	let cases: Vec<(String, &str, Value)> = metas
		.into_iter()
		.enumerate()
		.map(|(index, meta)| (format!("crossover-{index}"), "crossover-card", meta))
		.collect();
	let ours: Vec<_> = cases
		.iter()
		.map(|(_, _, value)| crossover(&budget, meta(value).as_ref(), today()).unwrap())
		.collect();
	let Some(theirs) = theirs("all-features", &cases, "0") else {
		return;
	};
	for ((id, _, value), ours) in cases.iter().zip(ours) {
		let data = &theirs[id.as_str()]["data"];
		let label = |month: veritas_domain::YearMonth| {
			month.first_day().naive().format("%b %Y").to_string()
		};
		let points: Vec<Value> = ours
			.points
			.iter()
			.map(|point| {
				let mut entry = json!({
					"x": label(point.month),
					"investmentIncome": number(point.investment_income),
					"expenses": number(point.expenses),
					"nestEgg": number(point.nest_egg),
				});
				if point.projection {
					entry["adjustedExpenses"] = optional(point.adjusted_expenses);
					entry["isProjection"] = json!(true);
				}
				entry
			})
			.collect();
		let ours = json!({
			"graphData": {
				"data": points,
				"start": ours.start.to_string(),
				"end": ours.end.to_string(),
				"crossoverXLabel": ours.crossover.map(label),
			},
			"lastKnownBalance": number(ours.last_known_balance),
			"lastKnownMonthlyIncome": number(ours.last_known_monthly_income),
			"lastKnownMonthlyExpenses": number(ours.last_known_monthly_expenses),
			"historicalReturn": optional(ours.historical_return),
			"yearsToRetire": optional(ours.years_to_retire),
			"targetMonthlyIncome": optional(ours.target_monthly_income),
			"targetNestEgg": optional(ours.target_nest_egg),
		});
		assert_eq!(&ours, data, "{value}");
	}
}

#[test]
fn d73_cash_flow_card_matches_actual() {
	let budget = fixture("all-features");
	let dining = category(&budget, "Dining Out");
	let mut metas = vec![json!(null), json!({"name": "no frame"})];
	for conditions in [
		json!(null),
		json!([{"field": "category", "op": "is", "value": dining, "type": "id"}]),
		json!([{"field": "amount", "op": "lt", "value": -5000, "type": "number"}, {"field": "notes", "op": "contains", "value": "a", "type": "string"}]),
		json!([{"field": "transfer", "op": "is", "value": true, "type": "boolean"}]),
		json!([{"field": "account", "op": "offBudget", "value": "", "type": "id"}]),
	] {
		for frame in [
			json!({"start": "2026-01", "end": "2026-10", "mode": "static"}),
			json!({"start": "2026-04", "end": "2026-11", "mode": "sliding-window"}),
			json!({"start": "2026-01-15", "end": "2026-02-14", "mode": "sliding-window"}),
			json!({"start": "2025-01", "end": "2026-11", "mode": "full"}),
			json!({"mode": "yearToDate"}),
			json!({"mode": "lastYear"}),
			json!({"mode": "priorYearToDate"}),
			json!({"mode": "currentQuarter"}),
		] {
			for op in ["and", "or"] {
				metas.push(
					json!({"timeFrame": frame, "conditions": conditions, "conditionsOp": op}),
				);
			}
		}
	}
	let cases: Vec<(String, &str, Value)> = metas
		.into_iter()
		.enumerate()
		.map(|(index, meta)| (format!("cash-flow-{index}"), "cash-flow-card", meta))
		.collect();
	let ours: Vec<_> = cases
		.iter()
		.map(|(_, _, value)| cash_flow_card(&budget, meta(value).as_ref(), today()).unwrap())
		.collect();
	assert!(
		ours.iter()
			.any(|card| card.income.minor() != 0 && card.expense.minor() != 0)
	);
	let Some(theirs) = theirs("all-features", &cases, "0") else {
		return;
	};
	for ((id, _, value), ours) in cases.iter().zip(ours) {
		let expected = &theirs[id.as_str()];
		assert_eq!(
			expected["start"],
			ours.range.start.as_str(),
			"{value} start"
		);
		assert_eq!(expected["end"], ours.range.end.as_str(), "{value} end");
		assert_eq!(expected["income"], ours.income.minor(), "{value} income");
		assert_eq!(expected["expense"], ours.expense.minor(), "{value} expense");
	}
}

fn money(value: Option<veritas_domain::Money>) -> Value {
	value.map_or(Value::Null, |money| json!(money.minor()))
}

fn spending_matches(ours: &Spending, expected: &Value, what: &str) {
	assert!(expected.get("error").is_none(), "{what}: {expected}");
	assert_eq!(
		expected["compare"],
		ours.compare.to_string(),
		"{what} compare"
	);
	assert_eq!(
		expected["compareTo"],
		ours.compare_to.to_string(),
		"{what} compareTo"
	);
	assert_eq!(expected["difference"], ours.difference, "{what} difference");
	let data = &expected["data"];
	let (first, last) = ours
		.average_months
		.map(|(first, last)| (json!(first.to_string()), json!(last.to_string())))
		.unwrap_or_default();
	assert_eq!(data["averageRange"]["startMonth"], first, "{what} average");
	assert_eq!(data["averageRange"]["endMonth"], last, "{what} average");
	assert_eq!(data["totalAssets"], ours.total_assets.minor(), "{what}");
	assert_eq!(data["totalDebts"], ours.total_debts.minor(), "{what}");
	let days: Vec<Value> = ours
		.days
		.iter()
		.map(|day| {
			json!({
				"day": format!("{:02}", day.day),
				"compare": money(day.compare),
				"compareTo": money(day.compare_to),
				"average": day.average.minor(),
				"budget": number(day.budget),
			})
		})
		.collect();
	let theirs: Vec<Value> = data["intervalData"]
		.as_array()
		.unwrap()
		.iter()
		.map(|day| {
			json!({
				"day": day["day"],
				"compare": day["compare"],
				"compareTo": day["compareTo"],
				"average": day["average"],
				"budget": day["budget"],
			})
		})
		.collect();
	assert_eq!(days, theirs, "{what}");
}

#[test]
fn d73_spending_card_matches_actual() {
	let budget = fixture("all-features");
	let groceries = category(&budget, "Groceries");
	let dining = category(&budget, "Dining Out");
	let everyday = group(&budget, "Everyday");
	let mut metas = vec![
		json!(null),
		json!({"name": "only a name"}),
		json!({"mode": null, "isLive": null}),
		json!({"compare": "2026-10", "compareTo": "2026-10", "isLive": false}),
		json!({"compare": "2026-09", "isLive": false}),
		json!({"compare": "2026-03", "compareTo": "2025-12", "isLive": false}),
		json!({"compare": "2026-03", "compareTo": "2025-12"}),
		json!({"compare": "2026-02", "compareTo": "2026-12", "isLive": false}),
		json!({"compare": "2027-01", "compareTo": "2026-10", "isLive": false}),
		json!({"compare": "2026-08", "compareTo": "2026-04", "isLive": false}),
		json!({"compare": "2026-08", "compareTo": "2026-07", "isLive": false, "averageRange": {"mode": "last-n-months", "months": 12}}),
		json!({"mode": "budget", "compare": "2026-10", "isLive": null}),
		json!({"mode": "average", "compare": "2026-10", "isLive": null}),
	];
	for mode in ["single-month", "budget", "average"] {
		for live in [json!(true), json!(false)] {
			for range in [
				json!(null),
				json!({"mode": "last-n-months", "months": 3}),
				json!({"mode": "last-n-months", "months": 6}),
				json!({"mode": "last-n-months", "months": 12}),
				json!({"mode": "last-n-months", "months": 5}),
				json!({"mode": "year-to-date"}),
				json!({"mode": "all-time"}),
				json!({"mode": "someday"}),
			] {
				metas.push(json!({"mode": mode, "isLive": live, "compare": "2026-10", "compareTo": "2026-08", "averageRange": range}));
			}
		}
	}
	metas.push(json!({"mode": "average", "isLive": false, "compare": "2026-01", "averageRange": {"mode": "year-to-date"}}));
	metas.push(json!({"mode": "average", "isLive": false, "compare": "2020-01", "averageRange": {"mode": "all-time"}}));
	for (conditions, op) in [
		(
			json!([{"field": "category", "op": "is", "value": groceries, "type": "id"}]),
			"and",
		),
		(
			json!([{"field": "category", "op": "oneOf", "value": [groceries, dining], "type": "id"}]),
			"and",
		),
		(
			json!([{"field": "category", "op": "notOneOf", "value": [groceries], "type": "id"}]),
			"and",
		),
		(
			json!([{"field": "category_group", "op": "is", "value": everyday, "type": "id"}, {"field": "category", "op": "isNot", "value": dining, "type": "id"}]),
			"and",
		),
		(
			json!([{"field": "category_group", "op": "contains", "value": "BILL", "type": "id"}, {"field": "category", "op": "is", "value": groceries, "type": "id"}]),
			"or",
		),
		(
			json!([{"field": "category", "op": "doesNotContain", "value": "fund", "type": "id"}]),
			"and",
		),
		(
			json!([{"field": "category", "op": "matches", "value": "^g", "type": "id"}]),
			"and",
		),
		(
			json!([{"field": "category", "op": "matches", "value": "x".repeat(300), "type": "id"}, {"field": "category", "op": "is", "value": dining, "type": "id"}]),
			"or",
		),
		(
			json!([{"field": "amount", "op": "lt", "value": -5000, "type": "number"}]),
			"and",
		),
		(
			json!([{"field": "amount", "op": "lt", "value": -5000, "type": "number"}, {"field": "category", "op": "is", "value": groceries, "type": "id"}]),
			"or",
		),
		(
			json!([{"field": "category", "op": "is", "value": null, "type": "id"}]),
			"and",
		),
		(
			json!([{"field": "notes", "op": "contains", "value": "a", "type": "string"}, {"field": "category", "op": "is", "value": dining, "type": "id", "customName": "named"}]),
			"and",
		),
	] {
		for mode in ["single-month", "budget"] {
			metas.push(json!({"mode": mode, "isLive": false, "compare": "2026-10", "conditions": conditions, "conditionsOp": op}));
		}
	}
	let cases: Vec<(String, &str, Value)> = metas
		.into_iter()
		.enumerate()
		.map(|(index, meta)| (format!("spending-{index}"), "spending-card", meta))
		.collect();
	let ours: Vec<_> = cases
		.iter()
		.map(|(_, _, value)| spending(&budget, meta(value).as_ref(), today()).unwrap())
		.collect();
	assert!(ours.iter().any(|card| card.difference != 0));
	let Some(theirs) = theirs("all-features", &cases, "0") else {
		return;
	};
	for ((id, _, value), ours) in cases.iter().zip(&ours) {
		spending_matches(ours, &theirs[id.as_str()], &value.to_string());
	}
}

#[test]
fn d73_spending_card_reads_a_tracking_budget() {
	let budget = tracking_fixture("all-features");
	assert_eq!(budget.budget_type, veritas_domain::BudgetType::Tracking);
	let groceries = category(&budget, "Groceries");
	let metas = [
		json!({"mode": "budget", "isLive": false, "compare": "2026-10"}),
		json!({"mode": "budget", "isLive": false, "compare": "2026-10", "conditions": [{"field": "category", "op": "is", "value": groceries, "type": "id"}]}),
		json!({"mode": "budget", "isLive": false, "compare": "2026-09"}),
	];
	let ours: Vec<_> = metas
		.iter()
		.map(|value| spending(&budget, meta(value).as_ref(), today()).unwrap())
		.collect();
	assert!(ours[0].days[27].budget < 0.0);
	let requests: Vec<Value> = metas
		.iter()
		.enumerate()
		.map(|(index, meta)| json!({"id": format!("tracking-{index}"), "type": "spending-card", "meta": meta, "budgetType": "tracking"}))
		.collect();
	let Some(theirs) = oracle(
		"widgets",
		"all-features",
		&Value::Array(requests),
		ORACLE_TODAY,
	) else {
		return;
	};
	for (index, (value, ours)) in metas.iter().zip(&ours).enumerate() {
		spending_matches(
			ours,
			&theirs[format!("tracking-{index}").as_str()],
			&value.to_string(),
		);
	}
}
