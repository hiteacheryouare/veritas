//! What Actual's dashboard card code returns, rebuilt from what Veritas
//! shows, and the parts of Actual's output to compare it with. Shared by
//! `widget_journeys.rs` and `tools/shadow`, so both judge a card the same way.

use serde_json::{Value, json};
use veritas_ffi::{
	AgeGranularity, AgeTrend, ForecastCardSource, FormulaValue, SankeyCardColor, WidgetView,
};

/// A number as the oracle writes it: integers as JSON, anything else as its
/// JS text.
#[allow(clippy::cast_possible_truncation, clippy::float_cmp)]
pub fn number(value: f64) -> Value {
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

pub fn optional(value: Option<f64>) -> Value {
	value.map_or(Value::Null, number)
}

pub fn month_label(month: &veritas_ffi::Month) -> String {
	chrono::NaiveDate::from_ymd_opt(month.year, month.month, 1)
		.unwrap()
		.format("%b %Y")
		.to_string()
}

pub fn month_text(month: &veritas_ffi::Month) -> String {
	format!("{}-{:02}", month.year, month.month)
}

/// What Actual's card code returns, rebuilt from what Veritas shows. `None`
/// for the cards whose whole output `tests/widgets.rs` in veritas-reports
/// compares instead.
pub fn as_actual(view: &WidgetView) -> Option<Value> {
	Some(match view {
		WidgetView::NetWorth {
			range,
			points,
			net_worth,
			change,
			lowest,
			highest,
			has_negative,
			accounts,
			series,
			..
		} => json!({
			"start": range.start, "end": range.end,
			"points": points.iter().map(|point| point.net_worth).collect::<Vec<_>>(),
			"hasNegative": has_negative, "netWorth": net_worth, "totalChange": change,
			"lowestNetWorth": lowest, "highestNetWorth": highest, "accounts": accounts,
			"details": points.iter().map(|point| json!({
				"assets": point.assets, "debt": point.debt, "change": point.change,
			})).collect::<Vec<_>>(),
			"stacked": series.iter().map(|stack| json!({
				"id": stack.account_id, "name": stack.name, "points": stack.points,
				"color": format!("var(--color-chartQual{})", stack.color + 1),
			})).collect::<Vec<_>>(),
		}),
		WidgetView::CashFlow {
			range,
			income,
			expense,
			change,
		} => {
			assert_eq!(*change, income + expense);
			json!({"start": range.start, "end": range.end, "income": income, "expense": expense})
		}
		WidgetView::Spending {
			compare,
			compare_to,
			difference,
			days,
			..
		} => json!({
			"compare": month_text(compare), "compareTo": month_text(compare_to),
			"difference": difference,
			"days": days.iter().map(|day| json!({
				"day": format!("{:02}", day.day),
				"compare": day.compare, "compareTo": day.compare_to,
				"average": day.average, "budget": number(day.budget),
			})).collect::<Vec<_>>(),
		}),
		WidgetView::Sankey {
			range,
			first,
			last,
			nodes,
			links,
			..
		} => json!({
			"start": range.start, "end": range.end,
			"boundedStart": month_text(first), "boundedEnd": month_text(last),
			"nodes": nodes.iter().map(|node| json!({
				"key": node.key, "percentageLabel": node.percentage, "color": sankey_color(node.color),
				"value": node.value,
			})).collect::<Vec<_>>(),
			"links": links.iter().map(|link| json!({
				"source": link.source,
				"target": link.target.map_or(-1, i64::from),
				"value": link.value,
				"tooltipInfo": link.folded.iter().map(|folded| json!({"name": folded.name, "value": folded.value})).collect::<Vec<_>>(),
				"color": link.color.map(sankey_color),
			})).collect::<Vec<_>>(),
		}),
		WidgetView::BalanceForecast {
			range,
			source,
			has_forecast,
			points,
			low,
			scheduled,
			..
		} => json!({
			"start": range.start, "end": range.end,
			"source": match source {
				ForecastCardSource::Schedules => "schedules",
				ForecastCardSource::TrackingBudget => "tracking-budget",
			},
			"hasForecast": has_forecast,
			"points": points.iter().map(|point| json!({"date": point.date, "balance": point.balance})).collect::<Vec<_>>(),
			"low": low.as_ref().map(|low| json!({"date": low.date, "balance": low.balance})),
			"scheduled": scheduled,
		}),
		WidgetView::Problem { .. } => json!({"error": true}),
		WidgetView::Formula { value, .. } => match value {
			FormulaValue::Error { error } => json!({"error": error.message}),
			FormulaValue::Number { value } => json!({"value": number(*value), "type": "number"}),
			FormulaValue::Text { text } => json!({"value": text, "type": "string"}),
			FormulaValue::Boolean { value } => json!({"value": value, "type": "boolean"}),
			FormulaValue::Empty => json!({"value": null, "type": "null"}),
		},
		WidgetView::MonteCarlo {
			success_percent,
			start_age,
			end_age,
			..
		} => json!({
			"successPercent": number(*success_percent),
			"startAge": number(*start_age), "endAge": number(*end_age),
		}),
		WidgetView::Summary {
			start,
			end,
			value,
			dividend,
			divisor,
		} => json!({
			"start": start, "end": end,
			"total": number(*value), "dividend": number(*dividend), "divisor": number(*divisor),
		}),
		WidgetView::Calendar { months } => json!({"calendarData": months.iter().map(|m| json!({
			"start": m.start, "end": m.end,
			"totalIncome": m.total_income, "totalExpense": m.total_expense,
			"data": m.days.iter().map(|d| json!({
				"date": d.date, "incomeValue": d.income, "expenseValue": d.expense,
				"incomeSize": number(d.income_size), "expenseSize": number(d.expense_size),
			})).collect::<Vec<_>>(),
		})).collect::<Vec<_>>()}),
		WidgetView::AgeOfMoney {
			granularity,
			points,
			current_days,
			trend,
			insufficient_data,
			..
		} => {
			let label = |period: &str| {
				let date = chrono::NaiveDate::parse_from_str(period, "%Y-%m-%d").unwrap();
				match granularity {
					AgeGranularity::Monthly => date.format("%b %Y").to_string(),
					_ => date.format("%b %-d, %Y").to_string(),
				}
			};
			json!({
				"graphData": points.iter().map(|p| json!({"date": label(&p.period), "ageOfMoney": p.days})).collect::<Vec<_>>(),
				"currentAge": current_days,
				"trend": match trend { AgeTrend::Up => "up", AgeTrend::Down => "down", AgeTrend::Stable => "stable" },
				"insufficientData": insufficient_data,
			})
		}
		WidgetView::BudgetAnalysis {
			start,
			end,
			months,
			total_budgeted,
			total_spent,
			total_overspending_adjustment,
			final_overspending_adjustment,
		} => json!({
			"intervalData": months.iter().map(|m| json!({
				"date": format!("{}-{:02}", m.month.year, m.month.month),
				"budgeted": m.budgeted, "spent": m.spent, "balance": m.balance,
				"overspendingAdjustment": m.overspending_adjustment,
			})).collect::<Vec<_>>(),
			"startDate": start, "endDate": end,
			"totalBudgeted": total_budgeted, "totalSpent": total_spent,
			"totalOverspendingAdjustment": total_overspending_adjustment,
			"finalOverspendingAdjustment": final_overspending_adjustment,
		}),
		WidgetView::Crossover {
			start,
			end,
			points,
			crossover,
			last_known_balance,
			last_known_monthly_income,
			last_known_monthly_expenses,
			historical_return,
			years_to_retire,
			target_monthly_income,
			target_nest_egg,
		} => json!({
			"graphData": {
				"data": points.iter().map(|p| {
					let mut entry = json!({
						"x": month_label(&p.month),
						"investmentIncome": number(p.investment_income),
						"expenses": number(p.expenses),
						"nestEgg": number(p.nest_egg),
					});
					if p.projection {
						entry["adjustedExpenses"] = optional(p.adjusted_expenses);
						entry["isProjection"] = json!(true);
					}
					entry
				}).collect::<Vec<_>>(),
				"start": format!("{}-{:02}", start.year, start.month),
				"end": format!("{}-{:02}", end.year, end.month),
				"crossoverXLabel": crossover.as_ref().map(month_label),
			},
			"lastKnownBalance": number(*last_known_balance),
			"lastKnownMonthlyIncome": number(*last_known_monthly_income),
			"lastKnownMonthlyExpenses": number(*last_known_monthly_expenses),
			"historicalReturn": optional(*historical_return),
			"yearsToRetire": optional(*years_to_retire),
			"targetMonthlyIncome": optional(*target_monthly_income),
			"targetNestEgg": optional(*target_nest_egg),
		}),
		_ => return None,
	})
}

pub fn sankey_color(color: SankeyCardColor) -> String {
	match color {
		SankeyCardColor::Palette { index } => format!("var(--color-chartQual{})", index + 1),
		SankeyCardColor::ToBudgetPositive => "var(--color-toBudgetPositive)".to_owned(),
		SankeyCardColor::ToBudgetNegative => "var(--color-toBudgetNegative)".to_owned(),
		SankeyCardColor::Gray => "var(--color-reportsGray)".to_owned(),
		SankeyCardColor::Blue => "var(--color-reportsBlue)".to_owned(),
	}
}

/// The days `summary-spreadsheet.ts` turns the card's range into: the first
/// of the start's month, and the end's month end, or today in the current
/// month.
pub fn summary_days(result: &Value, today: &str) -> (String, String) {
	let month = |text: &str| text[..7].to_owned();
	let start = format!("{}-01", month(result["start"].as_str().unwrap()));
	let end = month(result["end"].as_str().unwrap());
	let end = if end == month(today) {
		today.to_owned()
	} else {
		let first = chrono::NaiveDate::parse_from_str(&format!("{end}-01"), "%Y-%m-%d").unwrap();
		let next = first.checked_add_months(chrono::Months::new(1)).unwrap();
		(next - chrono::Days::new(1)).to_string()
	};
	(start, end)
}

/// The part of Actual's card output that `as_actual` rebuilds.
pub fn theirs_comparable(kind: &str, result: &Value, today: &str) -> Value {
	let data = &result["data"];
	if result.get("error").is_some() && kind != "formula-card" {
		return json!({"error": true});
	}
	match kind {
		"summary-card" => {
			let (start, end) = summary_days(result, today);
			json!({
				"start": start, "end": end,
				"total": data["total"], "dividend": data["dividend"], "divisor": data["divisor"],
			})
		}
		"calendar-card" => json!({"calendarData": data["calendarData"]}),
		"net-worth-card" => {
			let mut shown = data.clone();
			let fields = shown.as_object_mut().unwrap();
			fields.remove("startDate");
			fields.remove("endDate");
			fields.insert("start".to_owned(), result["start"].clone());
			fields.insert("end".to_owned(), result["end"].clone());
			shown
		}
		"cash-flow-card" => json!({
			"start": result["start"], "end": result["end"],
			"income": result["income"], "expense": result["expense"],
		}),
		"spending-card" => json!({
			"compare": result["compare"], "compareTo": result["compareTo"],
			"difference": result["difference"],
			"days": data["intervalData"].as_array().unwrap().iter().map(|day| json!({
				"day": day["day"], "compare": day["compare"], "compareTo": day["compareTo"],
				"average": day["average"], "budget": day["budget"],
			})).collect::<Vec<_>>(),
		}),
		"sankey-card" => json!({
			"start": result["start"], "end": result["end"],
			"boundedStart": result["boundedStart"], "boundedEnd": result["boundedEnd"],
			"nodes": data["nodes"].as_array().unwrap().iter().map(|node| json!({
				"key": node["key"], "percentageLabel": node["percentageLabel"], "color": node["color"],
				"value": node["value"],
			})).collect::<Vec<_>>(),
			"links": data["links"],
		}),
		"balance-forecast-card" => {
			let points = result["points"].as_array().unwrap();
			let low = match (&result["lowest"], points.last()) {
				(Value::Object(_), Some(last)) if result["lowest"]["date"] != last["date"] => {
					json!({"date": result["lowest"]["date"], "balance": result["lowest"]["balance"]})
				}
				_ => Value::Null,
			};
			json!({
				"start": result["start"], "end": result["end"], "source": result["source"],
				"hasForecast": result["forecast"]["dataPoints"].as_array().is_some_and(|points| !points.is_empty()),
				"points": points, "low": low, "scheduled": result["scheduled"],
			})
		}
		"formula-card" if !result["error"].is_null() => json!({"error": result["error"]}),
		"formula-card" => json!({"value": result["value"], "type": result["type"]}),
		"monte-carlo-card" => json!({
			"successPercent": result["successPercent"],
			"startAge": result["startAge"], "endAge": result["endAge"],
		}),
		_ => data.clone(),
	}
}

/// The Monte Carlo settings' amounts, which Actual's editor saves at the
/// currency's places, on the register's scale (D74).
pub fn register_scale_monte_carlo(meta: &mut Value, in_tax_bands: bool) {
	match meta {
		Value::Object(fields) => {
			for (key, value) in fields.iter_mut() {
				let money = matches!(
					key.as_str(),
					"startingBalance"
						| "annualWithdrawal"
						| "annualAmount" | "minimumSpending"
						| "annualFeeFixed"
				) || (in_tax_bands && key == "from");
				if money {
					if let Some(amount) = value.as_f64() {
						*value = json!(amount * 100.0);
					}
				} else {
					register_scale_monte_carlo(value, key == "taxBands");
				}
			}
		}
		Value::Array(items) => {
			for item in items {
				register_scale_monte_carlo(item, in_tax_bands);
			}
		}
		_ => {}
	}
}

/// The ops that put a 0-place budget's cells on the register's scale for
/// Actual's card code (D74): every budgeted amount and hold times 100.
pub fn register_scale_ops(dump: &Value) -> Vec<Value> {
	let rows = |name: &str| {
		dump["tables"][name]["rows"]
			.as_array()
			.cloned()
			.unwrap_or_default()
	};
	let month = |row: &Value| {
		let month = row["month"].as_i64().unwrap_or_default();
		format!("{}-{:02}", month / 100, month % 100)
	};
	let mut ops: Vec<Value> = rows("zero_budgets")
		.iter()
		.filter(|row| row["amount"].as_i64().unwrap_or(0) != 0)
		.map(|row| {
			json!({"op": "setBudgetAmount", "month": month(row), "category": row["category"],
				"amount": row["amount"].as_i64().unwrap_or_default() * 100})
		})
		.collect();
	ops.extend(
		rows("zero_budget_months")
			.iter()
			.filter(|row| row["buffered"].as_i64().unwrap_or(0) != 0)
			.map(|row| {
				json!({"op": "holdForNextMonth", "month": row["id"],
					"amount": row["buffered"].as_i64().unwrap_or_default() * 100})
			}),
	);
	ops
}
