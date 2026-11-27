//! (a) MIG-3: a custom report that rebuilds the native Net worth card.

use std::collections::BTreeMap;

use veritas_domain::{AccountId, CivilDate, CoreError, CustomReportRow, Money, Widget, YearMonth};
use veritas_reports::{
	BalanceType, CustomReport, GroupBy, Json, account_balances, evaluate_report, report_range,
	transaction_date_bounds,
};

use super::{Confidence, Conversion, Inputs, Suggestion, figure, signal};

pub const NET_WORTH_CARD: &str = "net-worth-card";

/// The report a `custom-report` widget shows, live or not, so a report
/// deleted in Actual changes the fingerprint (MIG-3.8).
pub(super) fn widget_report<'a>(
	data: &'a veritas_domain::BudgetData,
	widget: &Widget,
) -> Option<&'a CustomReportRow> {
	if widget.kind.as_deref() != Some("custom-report") {
		return None;
	}
	let meta = Json::parse(widget.meta.as_deref()?).ok()?;
	let id = meta.get("id")?.as_str()?;
	data.custom_reports
		.get(&veritas_domain::CustomReportId::new(id))
}

pub(super) fn suggestions(inputs: &Inputs<'_>) -> Result<Vec<Suggestion>, CoreError> {
	let data = inputs.data;
	let mut out = Vec::new();
	for widget in data.widgets.values().filter(|widget| !widget.tombstone) {
		let on_live_page = widget
			.page
			.as_ref()
			.and_then(|page| data.dashboard_pages.get(page))
			.is_some_and(|page| !page.tombstone);
		if !on_live_page || data.widget_overrides.contains_key(&widget.id) {
			continue;
		}
		let Some(row) = widget_report(data, widget).filter(|row| !row.tombstone) else {
			continue;
		};
		let Ok(report) = CustomReport::from_row(row) else {
			continue;
		};
		if let Some(suggestion) = net_worth(inputs, widget, row, &report)? {
			out.push(suggestion);
		}
	}
	Ok(out)
}

/// MIG-3.2's shape test, then MIG-3.3's figure test on every sampled date.
fn net_worth(
	inputs: &Inputs<'_>,
	widget: &Widget,
	row: &CustomReportRow,
	report: &CustomReport,
) -> Result<Option<Suggestion>, CoreError> {
	let data = inputs.data;
	let Some(accounts) = shaped_like_net_worth(report) else {
		return Ok(None);
	};
	if data.transactions.values().all(|row| row.tombstone) {
		return Ok(None);
	}
	let today = inputs.today.naive();
	let first = CivilDate::from(transaction_date_bounds(data, today).0);
	let reaches_back = if report.date_static {
		parse_day(&report.start_date).is_some_and(|start| start <= first)
	} else {
		report.date_range.as_deref() == Some("All time")
	};
	if !reaches_back {
		return Ok(None);
	}
	let Ok((start, end)) = report_range(report, data, today, 0) else {
		return Ok(None);
	};
	let Some(end) = parse_day(&end) else {
		return Ok(None);
	};
	let mut totals = Vec::new();
	for day in sample_dates(first, end)? {
		match equal_on(inputs, report, &start, day, &accounts)? {
			Some(total) => totals.push((day, total)),
			None => return Ok(None),
		}
	}
	let name = row.name.clone().unwrap_or_default();
	let mut suggestion = Suggestion::new(Conversion::Report, widget.id.as_str(), &name);
	suggestion.signals.push(signal(
		"shape",
		"Built like a net worth report: totals by account, net balance, off-budget accounts and uncategorized rows included",
		Vec::new(),
	));
	suggestion.signals.push(signal(
		"equal",
		format!(
			"Equal to your net worth, account by account and to the cent, on {} dates",
			totals.len()
		),
		totals
			.iter()
			.map(|(day, total)| figure(&day.to_string(), total.minor()))
			.collect(),
	));
	suggestion.confidence = if report.date_static {
		suggestion.signals.push(signal(
			"static_range",
			"Its dates are fixed, and the native card moves with today",
			Vec::new(),
		));
		Confidence::Shown
	} else {
		Confidence::PreTicked
	};
	suggestion.proposed.native_type = Some(NET_WORTH_CARD.to_owned());
	suggestion.proposed.native_meta = Some(native_meta(row));
	suggestion.confirm_lines.push(format!(
		"Veritas shows its native Net worth card in this slot and hides \"{name}\" from its report list. Actual still shows the report in the same place."
	));
	Ok(Some(suggestion))
}

/// The accounts the report is limited to (empty for all), when the report
/// is built like net worth (MIG-3.2). The graph type doesn't matter.
fn shaped_like_net_worth(report: &CustomReport) -> Option<Vec<AccountId>> {
	if !report.total_mode
		|| report.group_by != GroupBy::Account
		|| report.balance_type != BalanceType::TotalTotals
		|| !report.show_off_budget
		|| !report.show_uncategorized
	{
		return None;
	}
	let mut accounts = Vec::new();
	for condition in &report.conditions {
		if condition.get("field")?.as_str()? != "account" {
			return None;
		}
		let value = condition.get("value")?;
		match condition.get("op")?.as_str()? {
			"is" => accounts.push(AccountId::new(value.as_str()?)),
			"oneOf" => {
				for item in value.as_array()? {
					accounts.push(AccountId::new(item.as_str()?));
				}
			}
			_ => return None,
		}
	}
	Some(accounts)
}

/// E, the last day of the month before E's month, and the last day of the
/// month halfway from the first row's month to E's, rounded down (MIG-3.3).
/// Dates before the first row are left out.
fn sample_dates(first: CivilDate, end: CivilDate) -> Result<Vec<CivilDate>, CoreError> {
	let mut dates = vec![end];
	let previous = last_day(end.year_month().offset(-1)?)?;
	let span = months_between(first.year_month(), end.year_month());
	let middle = last_day(first.year_month().offset(span / 2)?)?.min(end);
	for day in [previous, middle] {
		if day >= first && !dates.contains(&day) {
			dates.push(day);
		}
	}
	Ok(dates)
}

fn months_between(first: YearMonth, last: YearMonth) -> i32 {
	(last.year() - first.year()) * 12 + last.month() as i32 - first.month() as i32
}

fn last_day(month: YearMonth) -> Result<CivilDate, CoreError> {
	month
		.offset(1)?
		.first_day()
		.naive()
		.pred_opt()
		.map(CivilDate::from)
		.ok_or(CoreError::Overflow)
}

/// The report's total when, with its range ending on `day`, it equals the
/// native net worth for its accounts on that day, account by account.
fn equal_on(
	inputs: &Inputs<'_>,
	report: &CustomReport,
	start: &str,
	day: CivilDate,
	accounts: &[AccountId],
) -> Result<Option<Money>, CoreError> {
	let mut ending = report.clone();
	ending.date_static = true;
	ending.start_date = start.to_owned();
	ending.end_date = day.to_string();
	let Ok(evaluated) = evaluate_report(inputs.data, &ending, day.naive(), 0) else {
		return Ok(None);
	};
	let native: BTreeMap<String, Money> =
		account_balances(inputs.data, inputs.classification, day)?
			.into_iter()
			.filter(|balance| !balance.deleted)
			.filter(|balance| accounts.is_empty() || accounts.contains(&balance.account))
			.map(|balance| (balance.account.as_str().to_owned(), balance.balance))
			.collect();
	let shown: BTreeMap<&str, Money> = evaluated
		.groups
		.iter()
		.map(|group| (group.id.as_str(), group.balances.total_totals))
		.collect();
	let every_account = native
		.iter()
		.all(|(id, balance)| shown.get(id.as_str()).copied().unwrap_or(Money::ZERO) == *balance);
	let nothing_else = shown
		.iter()
		.all(|(id, figure)| native.contains_key(*id) || *figure == Money::ZERO);
	let total = Money::checked_sum(native.values().copied())?;
	Ok((every_account && nothing_else && evaluated.totals.total_totals == total).then_some(total))
}

/// MIG-3.4's card meta: the report's name and account conditions, monthly,
/// as a trend.
fn native_meta(row: &CustomReportRow) -> String {
	let conditions = row
		.conditions
		.as_deref()
		.and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
		.filter(serde_json::Value::is_array)
		.unwrap_or_else(|| serde_json::Value::Array(Vec::new()));
	serde_json::json!({
		"name": row.name.clone().unwrap_or_default(),
		"conditions": conditions,
		"conditionsOp": row.conditions_op.clone().unwrap_or_else(|| "and".to_owned()),
		"interval": "Monthly",
		"mode": "trend",
	})
	.to_string()
}

fn parse_day(text: &str) -> Option<CivilDate> {
	chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d")
		.ok()
		.map(CivilDate::from)
}
