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
	Granularity, Json, SankeyCard, SankeyColor, SankeyLabel, Spending, Trend, age_of_money,
	budget_analysis, calendar, cash_flow_card, crossover, net_worth_card, sankey_card,
	sankey_height_limit, spending, summary,
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

#[test]
fn d73_net_worth_card_matches_actual() {
	let budget = fixture("all-features");
	let checking = account(&budget, "Checking");
	let savings = account(&budget, "Savings");
	let visa = account(&budget, "Visa Card");
	let dining = category(&budget, "Dining Out");
	let mut metas = vec![json!(null), json!({"interval": "Fortnightly"})];
	let frames = [
		json!(null),
		json!({"start": "2026-01", "end": "2026-11", "mode": "static"}),
		json!({"start": "2020-01", "end": "2027-03", "mode": "static"}),
		json!({"start": "2026-03", "end": "2026-05", "mode": "sliding-window"}),
		json!({"start": "2026-10-01", "end": "2026-11-05", "mode": "sliding-window"}),
		json!({"start": "2025-06", "end": "2026-11", "mode": "full"}),
		json!({"mode": "lastYear"}),
		json!({"mode": "yearToDate"}),
	];
	let filters = [
		(json!([]), "and"),
		(
			json!([{"field": "account", "op": "is", "value": checking, "type": "id"}]),
			"and",
		),
		(
			json!([{"field": "account", "op": "oneOf", "value": [checking, savings], "type": "id"}]),
			"and",
		),
		(
			json!([{"field": "account", "op": "isNot", "value": visa, "type": "id"}, {"field": "category", "op": "is", "value": dining, "type": "id"}]),
			"or",
		),
		(
			json!([{"field": "amount", "op": "lt", "value": 0, "type": "number"}]),
			"and",
		),
		(
			json!([{"field": "transfer", "op": "is", "value": true, "type": "boolean"}]),
			"and",
		),
		(
			json!([{"field": "date", "op": "gte", "value": "2026-04-01", "type": "date"}]),
			"and",
		),
	];
	for interval in ["Daily", "Weekly", "Monthly", "Yearly"] {
		for frame in &frames {
			for (conditions, op) in &filters {
				metas.push(json!({"interval": interval, "timeFrame": frame, "conditions": conditions, "conditionsOp": op}));
			}
		}
	}
	let cases: Vec<(String, &str, Value)> = metas
		.into_iter()
		.enumerate()
		.map(|(index, meta)| (format!("net-worth-{index}"), "net-worth-card", meta))
		.collect();
	for first_day_of_week in [0, 1] {
		let ours: Vec<_> = cases
			.iter()
			.map(|(_, _, value)| {
				net_worth_card(&budget, meta(value).as_ref(), today(), first_day_of_week).unwrap()
			})
			.collect();
		let Some(theirs) = theirs("all-features", &cases, &first_day_of_week.to_string()) else {
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
			let ours = json!({
				"points": ours.periods.iter().map(|period| period.net_worth.minor()).collect::<Vec<_>>(),
				"hasNegative": ours.has_negative,
				"startDate": ours.start.to_string(),
				"endDate": ours.end.to_string(),
				"netWorth": ours.net_worth.minor(),
				"totalChange": ours.total_change.minor(),
				"lowestNetWorth": money(ours.lowest),
				"highestNetWorth": money(ours.highest),
				"accounts": ours.accounts.iter().map(ToString::to_string).collect::<Vec<_>>(),
				"details": ours.periods.iter().map(|period| json!({
					"assets": period.assets.minor(), "debt": period.debt.minor(),
					"change": period.change.minor(),
				})).collect::<Vec<_>>(),
				"stacked": ours.stacked.iter().map(|stack| json!({
					"id": stack.account.to_string(), "name": stack.name,
					"points": stack.balances.iter().map(|balance| balance.minor()).collect::<Vec<_>>(),
					"color": format!("var(--color-chartQual{})", stack.color + 1),
				})).collect::<Vec<_>>(),
			});
			assert_eq!(
				ours, expected["data"],
				"{value} with weeks from {first_day_of_week}"
			);
		}
	}
}

fn sankey_color(color: SankeyColor) -> String {
	match color {
		SankeyColor::Palette(slot) => format!("var(--color-chartQual{})", slot + 1),
		SankeyColor::ToBudgetPositive => "var(--color-toBudgetPositive)".to_owned(),
		SankeyColor::ToBudgetNegative => "var(--color-toBudgetNegative)".to_owned(),
		SankeyColor::Gray => "var(--color-reportsGray)".to_owned(),
		SankeyColor::Blue => "var(--color-reportsBlue)".to_owned(),
	}
}

/// Our card in the shape of Actual's `SankeyData`. A category with more
/// money in than out is named by its category where Actual names it by a
/// payee (ST-10.10), so those names are taken from Actual's side.
fn sankey_as_actual(ours: &SankeyCard, theirs: &Value) -> Value {
	let their_name = |key: &str| {
		theirs["nodes"]
			.as_array()
			.and_then(|nodes| nodes.iter().find(|node| node["key"] == key))
			.map(|node| node["name"].clone())
	};
	let nodes: Vec<Value> = ours
		.nodes
		.iter()
		.map(|node| {
			let mut name = json!(node.label.actual_english());
			if node.key.ends_with("__NEGATIVE")
				&& let Some(theirs) = their_name(&node.key)
			{
				name = theirs;
			}
			json!({
				"key": node.key,
				"name": name,
				"percentageLabel": node.percentage,
				"color": sankey_color(node.color),
				"value": node.value.minor(),
			})
		})
		.collect();
	let links: Vec<Value> = ours
		.links
		.iter()
		.map(|link| {
			json!({
				"source": link.source,
				"target": link.target.map_or(-1, |target| i64::try_from(target).unwrap()),
				"value": link.value.minor(),
				"tooltipInfo": link.folded.iter().map(|(name, value)| json!({"name": name, "value": value.minor()})).collect::<Vec<_>>(),
				"color": link.color.map(sankey_color),
			})
		})
		.collect();
	json!({"nodes": nodes, "links": links})
}

fn payee(budget: &BudgetData, name: &str) -> String {
	budget
		.payees
		.values()
		.find(|payee| payee.name == name && !payee.tombstone)
		.map(|payee| payee.id.to_string())
		.unwrap()
}

fn sankey_cases(budget: &BudgetData) -> Vec<(Value, Option<f64>)> {
	let checking = account(budget, "Checking");
	let savings = account(budget, "Savings");
	let groceries = category(budget, "Groceries");
	let dining = category(budget, "Dining Out");
	let everyday = group(budget, "Everyday");
	let mut cases = vec![
		(json!(null), None),
		(json!({"name": "only a name"}), Some(300.0)),
	];
	let frames = [
		json!(null),
		json!({"start": "2026-01", "end": "2026-11", "mode": "static"}),
		json!({"start": "2020-01", "end": "2027-03", "mode": "static"}),
		json!({"start": "2026-03", "end": "2026-05", "mode": "sliding-window"}),
		json!({"start": "2026-10-01", "end": "2026-11-05", "mode": "sliding-window"}),
		json!({"start": "2025-06", "end": "2026-11", "mode": "full"}),
		json!({"mode": "lastYear"}),
		json!({"mode": "yearToDate"}),
		json!({"start": "2030-01", "end": "2030-02", "mode": "static"}),
	];
	for mode in [
		json!("spent"),
		json!("budgeted"),
		json!(null),
		json!("bogus"),
	] {
		for frame in &frames {
			cases.push((json!({"mode": mode, "timeFrame": frame}), None));
		}
	}
	for mode in ["spent", "budgeted"] {
		for top in [
			json!(null),
			json!(0),
			json!(1),
			json!(2),
			json!(3),
			json!(5),
			json!(100),
		] {
			for sort in [
				json!(null),
				json!("per-group"),
				json!("global"),
				json!("budget-order"),
				json!("odd"),
			] {
				cases.push((
					json!({"mode": mode, "timeFrame": frames[2], "topNcategories": top, "categorySort": sort, "showPercentages": true}),
					None,
				));
			}
		}
		for height in [0.0, 70.0, 140.0, 301.0, 1000.0] {
			cases.push((json!({"mode": mode, "timeFrame": frames[1]}), Some(height)));
		}
		for (from, to) in [
			("payee", "category"),
			("income_category", "category_group"),
			("account", "category"),
			("budget", "budget"),
			("category", "payee"),
			("payee", "account"),
			("nonsense", "category"),
		] {
			cases.push((
				json!({"mode": mode, "timeFrame": frames[2], "layerFrom": from, "layerTo": to, "topNcategories": 4}),
				None,
			));
		}
		for (accounts, transfers) in [(false, true), (true, false), (true, true)] {
			for sort in ["per-group", "global", "budget-order"] {
				cases.push((
					json!({"mode": mode, "timeFrame": frames[2], "groupAccounts": accounts, "showTransfers": transfers, "categorySort": sort, "topNcategories": 3}),
					None,
				));
				cases.push((
					json!({"mode": mode, "timeFrame": frames[3], "groupAccounts": accounts, "showTransfers": transfers, "categorySort": sort}),
					None,
				));
			}
		}
		for (conditions, op) in [
			(
				json!([{"field": "account", "op": "is", "value": checking, "type": "id"}]),
				"and",
			),
			(
				json!([{"field": "account", "op": "oneOf", "value": [checking, savings], "type": "id"}]),
				"and",
			),
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
				json!([{"field": "category", "op": "matches", "value": "/OUT$/i", "type": "id"}]),
				"and",
			),
			(
				json!([{"field": "amount", "op": "lt", "value": -5000, "type": "number"}]),
				"and",
			),
			(
				json!([{"field": "amount", "op": "gt", "value": 0, "type": "number"}, {"field": "category", "op": "is", "value": groceries, "type": "id"}]),
				"or",
			),
			(
				json!([{"field": "notes", "op": "contains", "value": "a", "type": "string"}, {"field": "category", "op": "is", "value": dining, "type": "id", "customName": "named"}]),
				"and",
			),
			(
				json!([{"field": "transfer", "op": "is", "value": true, "type": "boolean"}]),
				"and",
			),
		] {
			for transfers in [false, true] {
				cases.push((
					json!({"mode": mode, "timeFrame": frames[2], "conditions": conditions, "conditionsOp": op, "showTransfers": transfers, "topNcategories": 6}),
					None,
				));
			}
		}
	}
	let payees: Vec<String> = budget
		.payees
		.values()
		.filter(|payee| !payee.tombstone && payee.transfer_account.is_none())
		.map(|payee| payee.name.clone())
		.collect();
	if let Some(name) = payees.first() {
		let id = payee(budget, name);
		cases.push((
			json!({"timeFrame": frames[2], "conditions": [{"field": "payee", "op": "isNot", "value": id, "type": "id"}], "topNcategories": 2, "categorySort": "global"}),
			None,
		));
	}
	cases
}

#[test]
fn d73_sankey_card_matches_actual() {
	let budget = fixture("all-features");
	let sheet = EnvelopeSheet::compute(&budget, CivilDate::new(2026, 11, 9).unwrap()).unwrap();
	let cases = sankey_cases(&budget);
	let ours: Vec<_> = cases
		.iter()
		.map(|(value, height)| {
			sankey_card(
				&budget,
				&sheet,
				meta(value).as_ref(),
				today(),
				height.map(sankey_height_limit),
			)
		})
		.collect();
	let cards: Vec<&SankeyCard> = ours.iter().flatten().collect();
	let covered = |what: &str, test: &dyn Fn(&SankeyCard) -> bool| {
		assert!(cards.iter().any(|card| test(card)), "no case covers {what}");
	};
	covered("a big graph", &|card| card.nodes.len() > 10);
	covered("folding into Other", &|card| {
		card.links.iter().any(|link| !link.folded.is_empty())
	});
	covered("hidden spacers", &|card| {
		card.nodes
			.iter()
			.any(|node| node.label == SankeyLabel::Hidden)
	});
	covered("links cut by the layer range", &|card| {
		card.links.iter().any(|link| link.target.is_none())
	});
	covered("an expense category with money in", &|card| {
		card.nodes
			.iter()
			.any(|node| node.key.ends_with("__NEGATIVE"))
	});
	let accounts: Vec<String> = budget.accounts.keys().map(ToString::to_string).collect();
	covered("transfers between accounts", &|card| {
		card.links.iter().any(|link| {
			link.target.is_some_and(|target| {
				accounts.contains(&card.nodes[link.source].key)
					&& accounts.contains(&card.nodes[target].key)
			})
		})
	});
	covered("budgeted To Budget", &|card| {
		card.nodes.iter().any(|node| {
			matches!(
				node.label,
				SankeyLabel::ToBudget | SankeyLabel::Overbudgeted
			)
		})
	});
	covered("All accounts", &|card| {
		card.nodes
			.iter()
			.any(|node| node.label == SankeyLabel::AllAccounts)
	});
	let requests: Vec<Value> = cases
		.iter()
		.enumerate()
		.map(|(index, (meta, height))| {
			let mut request =
				json!({"id": format!("sankey-{index}"), "type": "sankey-card", "meta": meta});
			if let Some(height) = height {
				request["cardHeight"] = json!(height);
			}
			request
		})
		.collect();
	let Some(theirs) = oracle(
		"widgets",
		"all-features",
		&Value::Array(requests),
		ORACLE_TODAY,
	) else {
		return;
	};
	let mut failures = Vec::new();
	for (index, ((value, _), ours)) in cases.iter().zip(&ours).enumerate() {
		let expected = &theirs[format!("sankey-{index}").as_str()];
		let what = format!("case {index}: {value}");
		match ours {
			Err(error) => {
				if expected.get("error").is_none() {
					failures.push(format!("{what}: we failed with {error:?}, Actual didn't"));
				}
			}
			Ok(ours) => {
				if expected.get("error").is_some() {
					failures.push(format!("{what}: Actual failed with {}", expected["error"]));
					continue;
				}
				assert_eq!(expected["start"], ours.range.start.as_str(), "{what} start");
				assert_eq!(expected["end"], ours.range.end.as_str(), "{what} end");
				assert_eq!(
					expected["boundedStart"],
					ours.first_month.to_string(),
					"{what}"
				);
				assert_eq!(
					expected["boundedEnd"],
					ours.last_month.to_string(),
					"{what}"
				);
				let mine = sankey_as_actual(ours, &expected["data"]);
				if mine != expected["data"] {
					failures.push(format!(
						"{what}\n ours:   {mine}\n theirs: {}",
						expected["data"]
					));
				}
			}
		}
	}
	assert!(
		failures.is_empty(),
		"{} of {} differ:\n{}",
		failures.len(),
		cases.len(),
		failures.join("\n\n")
	);
}

/// Budgeted mode's negative budgets ("From {category}") and a To Budget
/// pushed negative, on the fixture with three budget cells edited in both
/// Veritas and Actual.
#[test]
fn d73_sankey_card_draws_negative_budgets() {
	let mut budget = fixture("all-features");
	let edits = [
		("2026-10", "Groceries", -5_000),
		("2026-09", "Dining Out", -12_000),
		("2026-10", "Dining Out", 9_000_000),
	];
	let mut requests = Vec::new();
	for (month, name, amount) in edits {
		let id = budget
			.categories
			.values()
			.find(|category| category.name == name)
			.map(|category| category.id.clone())
			.unwrap();
		let month_key = veritas_domain::YearMonth::parse(month).unwrap();
		budget
			.budget_cells
			.entry((month_key, id.clone()))
			.or_insert_with(|| veritas_domain::BudgetCell {
				month: month_key,
				category: id.clone(),
				budgeted: veritas_domain::Money::ZERO,
				carryover: false,
				goal: None,
				long_goal: false,
			})
			.budgeted = veritas_domain::Money::from_minor(amount);
		requests.push(json!({"id": format!("edit-{month}-{name}"), "type": "budget-amount", "meta": {"month": month, "category": id.to_string(), "amount": amount}}));
	}
	let sheet = EnvelopeSheet::compute(&budget, CivilDate::new(2026, 11, 9).unwrap()).unwrap();
	let mut metas = Vec::new();
	for frame in [
		json!({"start": "2026-10", "end": "2026-10", "mode": "static"}),
		json!({"start": "2026-09", "end": "2026-10", "mode": "static"}),
		json!({"start": "2026-09", "end": "2026-09", "mode": "static"}),
		json!({"start": "2026-08", "end": "2026-11", "mode": "static"}),
	] {
		for sort in ["per-group", "global", "budget-order"] {
			for top in [3, 15] {
				metas.push(json!({"mode": "budgeted", "timeFrame": frame, "categorySort": sort, "topNcategories": top, "showPercentages": true}));
			}
		}
		metas.push(json!({"mode": "budgeted", "timeFrame": frame, "layerFrom": "account", "layerTo": "category_group"}));
	}
	let ours: Vec<SankeyCard> = metas
		.iter()
		.map(|value| sankey_card(&budget, &sheet, meta(value).as_ref(), today(), None).unwrap())
		.collect();
	assert!(ours.iter().any(|card| {
		card.nodes
			.iter()
			.any(|node| matches!(node.label, SankeyLabel::FromCategory(_)))
	}));
	assert!(ours.iter().any(|card| {
		card.nodes
			.iter()
			.any(|node| node.label == SankeyLabel::Overbudgeted)
	}));
	for (index, meta) in metas.iter().enumerate() {
		requests
			.push(json!({"id": format!("negative-{index}"), "type": "sankey-card", "meta": meta}));
	}
	let Some(theirs) = oracle(
		"widgets",
		"all-features",
		&Value::Array(requests),
		ORACLE_TODAY,
	) else {
		return;
	};
	for (index, (value, ours)) in metas.iter().zip(&ours).enumerate() {
		let expected = &theirs[format!("negative-{index}").as_str()];
		assert_eq!(
			sankey_as_actual(ours, &expected["data"]),
			expected["data"],
			"{value}"
		);
	}
}

/// A tracking budget has no To Budget, carryover or overspending cells, so
/// budgeted mode draws only its budgets and income.
#[test]
fn d73_sankey_card_reads_a_tracking_budget() {
	let budget = tracking_fixture("all-features");
	let sheet = EnvelopeSheet::compute(&budget, CivilDate::new(2026, 11, 9).unwrap()).unwrap();
	let metas = [
		json!({"mode": "budgeted"}),
		json!({"mode": "budgeted", "timeFrame": {"start": "2026-01", "end": "2026-10", "mode": "static"}, "categorySort": "global", "topNcategories": 4}),
		json!({"mode": "spent", "timeFrame": {"start": "2026-01", "end": "2026-10", "mode": "static"}}),
	];
	let ours: Vec<SankeyCard> = metas
		.iter()
		.map(|value| sankey_card(&budget, &sheet, meta(value).as_ref(), today(), None).unwrap())
		.collect();
	assert!(ours[1].nodes.len() > 4);
	let mut requests =
		vec![json!({"id": "switch", "type": "budget-type", "meta": {"type": "tracking"}})];
	for (index, meta) in metas.iter().enumerate() {
		requests
			.push(json!({"id": format!("tracking-{index}"), "type": "sankey-card", "meta": meta}));
	}
	let Some(theirs) = oracle(
		"widgets",
		"all-features",
		&Value::Array(requests),
		ORACLE_TODAY,
	) else {
		return;
	};
	for (index, (value, ours)) in metas.iter().zip(&ours).enumerate() {
		let expected = &theirs[format!("tracking-{index}").as_str()];
		assert_eq!(
			sankey_as_actual(ours, &expected["data"]),
			expected["data"],
			"{value}"
		);
	}
}
