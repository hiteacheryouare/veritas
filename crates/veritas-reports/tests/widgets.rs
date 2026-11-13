//! D73: every Actual dashboard card computed natively. Each card's numbers
//! are compared with Actual's own card and spreadsheet code, run by the
//! oracle (`widgets`) on the same fixture and meta.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use chrono::NaiveDate;
use serde_json::{Value, json};
use support::actual::{fixture, oracle};
use veritas_domain::BudgetData;
use veritas_reports::{Json, calendar, summary};

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
