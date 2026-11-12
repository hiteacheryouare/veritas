use veritas_domain::{CoreError, CustomReportId, CustomReportRow};

use crate::json::Json;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupBy {
	Category,
	Group,
	CategoryGroup,
	Payee,
	Account,
	Tag,
	Interval,
}

/// Actual's balance types, named by the field each one reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BalanceType {
	/// "Payment": money out, negative.
	TotalDebts,
	/// "Deposit": money in.
	TotalAssets,
	/// "Net".
	TotalTotals,
	/// "Net Payment".
	NetDebts,
	/// "Net Deposit".
	NetAssets,
	/// "Budgeted".
	TotalBudgeted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interval {
	Daily,
	Weekly,
	Monthly,
	Yearly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortBy {
	Asc,
	Desc,
	Name,
	Budget,
}

/// A custom report's settings, read with Actual's defaults
/// (`reports/app.ts` `reportModel`). The row it came from is kept, and
/// `to_row` writes only what an edit changed.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomReport {
	pub id: CustomReportId,
	pub name: String,
	pub start_date: String,
	pub end_date: String,
	pub date_static: bool,
	pub date_range: Option<String>,
	pub total_mode: bool,
	pub group_by: GroupBy,
	pub balance_type: BalanceType,
	pub sort_by: SortBy,
	pub interval: Interval,
	pub show_empty: bool,
	pub show_off_budget: bool,
	pub show_hidden: bool,
	pub show_uncategorized: bool,
	pub include_current: bool,
	pub trim_intervals: bool,
	pub conditions: Vec<Json>,
	pub conditions_or: bool,
	pub row: CustomReportRow,
}

fn flag(value: Option<i64>) -> bool {
	value.is_some_and(|value| value != 0)
}

impl CustomReport {
	pub fn from_row(row: &CustomReportRow) -> Result<CustomReport, CoreError> {
		let invalid = |what: &str| CoreError::Invalid(format!("custom report {}: {what}", row.id));
		let group_by = match row.group_by.as_deref().unwrap_or("Category") {
			"Category" => GroupBy::Category,
			"Group" => GroupBy::Group,
			"CategoryGroup" => GroupBy::CategoryGroup,
			"Payee" => GroupBy::Payee,
			"Account" => GroupBy::Account,
			"Tag" => GroupBy::Tag,
			"Interval" => GroupBy::Interval,
			other => return Err(invalid(&format!("unknown group_by {other:?}"))),
		};
		let balance_type = match row.balance_type.as_deref().unwrap_or("Payment") {
			"Payment" | "Expense" => BalanceType::TotalDebts,
			"Deposit" => BalanceType::TotalAssets,
			"Net" => BalanceType::TotalTotals,
			"Net Payment" => BalanceType::NetDebts,
			"Net Deposit" => BalanceType::NetAssets,
			"Budgeted" => BalanceType::TotalBudgeted,
			other => return Err(invalid(&format!("unknown balance_type {other:?}"))),
		};
		let interval = match row.interval.as_deref().unwrap_or("Monthly") {
			"Daily" => Interval::Daily,
			"Weekly" => Interval::Weekly,
			"Monthly" => Interval::Monthly,
			"Yearly" => Interval::Yearly,
			other => return Err(invalid(&format!("unknown interval {other:?}"))),
		};
		let sort_by = match row.sort_by.as_deref().unwrap_or("desc") {
			"asc" => SortBy::Asc,
			"desc" => SortBy::Desc,
			"name" => SortBy::Name,
			"budget" => SortBy::Budget,
			other => return Err(invalid(&format!("unknown sort_by {other:?}"))),
		};
		let conditions = match row.conditions.as_deref() {
			None | Some("") => Vec::new(),
			Some(text) => match Json::parse(text).map_err(|reason| invalid(&reason))? {
				Json::Array(items) => items,
				_ => return Err(invalid("conditions is not a list")),
			},
		};
		Ok(CustomReport {
			id: row.id.clone(),
			name: row.name.clone().unwrap_or_default(),
			start_date: row
				.start_date
				.clone()
				.unwrap_or_else(|| "2023-06".to_owned()),
			end_date: row.end_date.clone().unwrap_or_else(|| "2023-09".to_owned()),
			date_static: flag(row.date_static),
			date_range: row.date_range.clone(),
			total_mode: row.mode.as_deref() != Some("time"),
			group_by,
			balance_type,
			sort_by,
			interval,
			show_empty: flag(row.show_empty),
			show_off_budget: flag(row.show_offbudget),
			show_hidden: flag(row.show_hidden),
			show_uncategorized: flag(row.show_uncategorized),
			include_current: flag(row.include_current),
			trim_intervals: flag(row.trim_intervals),
			conditions,
			conditions_or: row.conditions_op.as_deref() == Some("or"),
			row: row.clone(),
		})
	}

	/// The row to write back. Columns this report never changed keep their
	/// stored text; changed ones are written the way Actual's `fromJS` writes
	/// them, with JSON in `JSON.stringify` form.
	pub fn to_row(&self) -> CustomReportRow {
		let original = CustomReport::from_row(&self.row).ok();
		let changed = |pick: &dyn Fn(&CustomReport) -> String| {
			original
				.as_ref()
				.is_none_or(|original| pick(original) != pick(self))
		};
		let mut row = self.row.clone();
		let int = |value: bool| Some(i64::from(value));
		if changed(&|report| report.name.clone()) {
			row.name = Some(self.name.clone());
		}
		if changed(&|report| format!("{}|{}", report.start_date, report.end_date)) {
			row.start_date = Some(self.start_date.clone());
			row.end_date = Some(self.end_date.clone());
		}
		if changed(&|report| format!("{}|{:?}", report.date_static, report.date_range)) {
			row.date_static = int(self.date_static);
			row.date_range = self.date_range.clone();
		}
		if changed(&|report| report.total_mode.to_string()) {
			row.mode = Some(if self.total_mode { "total" } else { "time" }.to_owned());
		}
		if changed(&|report| format!("{:?}", report.group_by)) {
			row.group_by = Some(format!("{:?}", self.group_by));
		}
		if changed(&|report| format!("{:?}", report.balance_type)) {
			row.balance_type = Some(
				match self.balance_type {
					BalanceType::TotalDebts => "Payment",
					BalanceType::TotalAssets => "Deposit",
					BalanceType::TotalTotals => "Net",
					BalanceType::NetDebts => "Net Payment",
					BalanceType::NetAssets => "Net Deposit",
					BalanceType::TotalBudgeted => "Budgeted",
				}
				.to_owned(),
			);
		}
		if changed(&|report| format!("{:?}", report.sort_by)) {
			row.sort_by = Some(
				match self.sort_by {
					SortBy::Asc => "asc",
					SortBy::Desc => "desc",
					SortBy::Name => "name",
					SortBy::Budget => "budget",
				}
				.to_owned(),
			);
		}
		if changed(&|report| format!("{:?}", report.interval)) {
			row.interval = Some(format!("{:?}", self.interval));
		}
		for (pick, column) in [
			(
				(|report: &CustomReport| report.show_empty) as fn(&CustomReport) -> bool,
				&mut row.show_empty,
			),
			(|report| report.show_off_budget, &mut row.show_offbudget),
			(|report| report.show_hidden, &mut row.show_hidden),
			(
				|report| report.show_uncategorized,
				&mut row.show_uncategorized,
			),
			(|report| report.include_current, &mut row.include_current),
			(|report| report.trim_intervals, &mut row.trim_intervals),
		] {
			if original
				.as_ref()
				.is_none_or(|original| pick(original) != pick(self))
			{
				*column = int(pick(self));
			}
		}
		if changed(&|report| Json::Array(report.conditions.clone()).stringify()) {
			row.conditions = Some(Json::Array(self.conditions.clone()).stringify());
		}
		if changed(&|report| report.conditions_or.to_string()) {
			row.conditions_op = Some(if self.conditions_or { "or" } else { "and" }.to_owned());
		}
		row
	}
}
