use std::collections::BTreeSet;

use veritas_domain::{
	BudgetCell, BudgetData, CategoryId, CivilDate, CoreError, Hold, Money, Note, Row, YearMonth,
};
use veritas_engine::{
	BudgetEditor, BudgetWrites, CategoryMonth, Envelope, EnvelopeSheet,
	NumberFormat as EngineFormat,
};

use crate::Month;

/// How an amount reads on screen. The app picks the colour, icon and word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MoneyTone {
	Available,
	/// 20% or less of the envelope is left.
	Low,
	Overspent,
	Income,
	/// Exactly zero.
	Neutral,
}

/// One envelope in one month.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct EnvelopeRow {
	pub id: String,
	pub name: String,
	pub hidden: bool,
	pub budgeted: i64,
	pub spent: i64,
	pub available: i64,
	/// Rolls an overspent balance into next month instead of To Budget.
	pub carryover: bool,
	pub tone: MoneyTone,
	/// How full the drain bar is: available ÷ (carried in + budgeted), 0 to 1.
	pub fill: f64,
	pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct EnvelopeGroup {
	pub id: String,
	pub name: String,
	pub is_income: bool,
	pub hidden: bool,
	pub budgeted: i64,
	pub spent: i64,
	pub available: i64,
	pub envelopes: Vec<EnvelopeRow>,
}

/// The Budget screen for one month (R-BUD-1). Groups and envelopes are in
/// Actual's order, income last.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct BudgetMonth {
	pub month: Month,
	/// The months Actual has created, oldest and newest.
	pub first_month: Month,
	pub last_month: Month,
	pub to_budget: i64,
	pub income: i64,
	pub from_last_month: i64,
	pub overspent_last_month: i64,
	pub budgeted: i64,
	/// Held for next month.
	pub for_next_month: i64,
	pub spent: i64,
	pub available: i64,
	pub groups: Vec<EnvelopeGroup>,
}

/// `YYYY-MM-DD`, the local civil date the app passes as today.
fn civil_date(text: &str) -> Result<CivilDate, CoreError> {
	let invalid = || CoreError::Invalid(format!("{text} isn't a date."));
	let mut parts = text.splitn(3, '-').map(str::parse::<u32>);
	match (parts.next(), parts.next(), parts.next()) {
		(Some(Ok(year)), Some(Ok(month)), Some(Ok(day))) => {
			CivilDate::new(i32::try_from(year).map_err(|_| invalid())?, month, day)
		}
		_ => Err(invalid()),
	}
}

pub(crate) fn year_month(month: Month) -> Result<YearMonth, CoreError> {
	YearMonth::new(month.year, month.month)
}

pub(crate) fn budget_month(
	data: &BudgetData,
	month: Month,
	today: &str,
) -> Result<BudgetMonth, CoreError> {
	let month = year_month(month)?;
	let sheet = EnvelopeSheet::compute(data, civil_date(today)?)?;
	let (Some(&first), Some(&last)) = (sheet.months().first(), sheet.months().last()) else {
		return Err(CoreError::Invalid(
			"This budget has no months yet.".to_owned(),
		));
	};
	let month = month.clamp(first, last);
	let summary = sheet
		.summary(month)
		.ok_or_else(|| CoreError::Invalid(format!("{month} isn't in this budget.")))?;
	let mut groups = Vec::new();
	for (group_id, categories) in sheet.group_order() {
		let (Some(group), Some(totals)) = (
			data.category_groups.get(group_id),
			sheet.group(month, group_id),
		) else {
			continue;
		};
		let envelopes = categories
			.into_iter()
			.filter_map(|id| {
				let category = data.categories.get(id)?;
				let cell = sheet.category(month, id)?;
				let (tone, fill) = tone_and_fill(cell);
				Some(EnvelopeRow {
					id: id.to_string(),
					name: category.name.clone(),
					hidden: category.hidden,
					budgeted: cell.budgeted.minor(),
					spent: cell.spent.minor(),
					available: cell.balance.minor(),
					carryover: cell.carryover,
					tone,
					fill,
					note: data.notes.get(id.as_str()).cloned().flatten(),
				})
			})
			.collect();
		groups.push(EnvelopeGroup {
			id: group_id.to_string(),
			name: group.name.clone(),
			is_income: group.is_income,
			hidden: group.hidden,
			budgeted: totals.budgeted.minor(),
			spent: totals.spent.minor(),
			available: totals.balance.minor(),
			envelopes,
		});
	}
	Ok(BudgetMonth {
		month: month.into(),
		first_month: first.into(),
		last_month: last.into(),
		to_budget: summary.to_budget.minor(),
		income: summary.total_income.minor(),
		from_last_month: summary.from_last_month.minor(),
		overspent_last_month: summary.last_month_overspent.minor(),
		budgeted: summary.total_budgeted.minor(),
		for_next_month: summary.for_next_month.minor(),
		spent: summary.total_spent.minor(),
		available: summary.total_balance.minor(),
		groups,
	})
}

/// design/tokens.md, drain bars: what's left of carried-in plus budgeted.
/// Spending is negative, so carried in + budgeted is available − spent.
fn tone_and_fill(cell: &CategoryMonth) -> (MoneyTone, f64) {
	let available = cell.balance.minor();
	if cell.is_income {
		return (MoneyTone::Income, 0.0);
	}
	if available < 0 {
		return (MoneyTone::Overspent, 0.0);
	}
	if available == 0 {
		return (MoneyTone::Neutral, 0.0);
	}
	let start = available - cell.spent.minor();
	let fill = if start <= 0 {
		1.0
	} else {
		(available as f64 / start as f64).clamp(0.0, 1.0)
	};
	let tone = if fill <= 0.2 {
		MoneyTone::Low
	} else {
		MoneyTone::Available
	};
	(tone, fill)
}

/// Where budget moves to or from: an envelope, or To Budget when `None`.
pub(crate) fn envelope(id: &Option<String>) -> Envelope {
	match id {
		Some(id) => Envelope::Category(CategoryId::new(id)),
		None => Envelope::ToBudget,
	}
}

/// Runs a budget edit against a sheet that covers `month`. Values in a month
/// don't depend on later ones, so the sheet stops there.
pub(crate) fn edit(
	data: &BudgetData,
	month: Month,
	write: impl FnOnce(&BudgetEditor<'_>, YearMonth) -> Result<BudgetWrites, CoreError>,
) -> Result<Vec<Row>, CoreError> {
	let month = year_month(month)?;
	let sheet = EnvelopeSheet::compute(data, month.first_day())?;
	let format =
		EngineFormat::from_preference(data.format.number_format.as_deref().unwrap_or("comma-dot"));
	let editor = BudgetEditor::new(data, &sheet, format, data.format.hide_fraction);
	let writes = write(&editor, month)?;
	Ok(rows(data, &writes))
}

/// The rows that hold `writes` once applied: each touched budget cell, hold
/// and note as it ends up.
fn rows(data: &BudgetData, writes: &BudgetWrites) -> Vec<Row> {
	let mut after = data.clone();
	writes.apply_to(&mut after);
	let cells: BTreeSet<(YearMonth, CategoryId)> = writes
		.cells
		.iter()
		.map(|cell| (cell.month, cell.category.clone()))
		.collect();
	let holds: BTreeSet<YearMonth> = writes.holds.iter().map(|(month, _)| *month).collect();
	let notes: BTreeSet<&String> = writes.notes.iter().map(|(id, _)| id).collect();
	let mut rows = Vec::new();
	for key in cells {
		if let Some(cell) = after.budget_cells.get(&key) {
			rows.push(Row::BudgetCell(BudgetCell::clone(cell)));
		}
	}
	for month in holds {
		rows.push(Row::Hold(Hold {
			month,
			amount: after.holds.get(&month).copied().flatten(),
		}));
	}
	for id in notes {
		rows.push(Row::Note(Note {
			id: id.clone(),
			text: after.notes.get(id).cloned().flatten(),
		}));
	}
	rows
}

pub(crate) fn money(minor: i64) -> Money {
	Money::from_minor(minor)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn cell(budgeted: i64, spent: i64, balance: i64) -> CategoryMonth {
		CategoryMonth {
			budgeted: Money::from_minor(budgeted),
			spent: Money::from_minor(spent),
			balance: Money::from_minor(balance),
			..CategoryMonth::default()
		}
	}

	#[test]
	fn the_drain_bar_shows_what_is_left_of_carried_in_plus_budgeted() {
		// design/mockups/budget.html: Gas has 43.90 of 140.00 left.
		let (tone, fill) = tone_and_fill(&cell(14_000, -9_610, 4_390));
		assert_eq!(tone, MoneyTone::Available);
		assert!((fill - 0.3136).abs() < 0.001);
	}

	#[test]
	fn twenty_percent_or_less_left_is_low() {
		assert_eq!(tone_and_fill(&cell(6_000, -5_120, 880)).0, MoneyTone::Low);
		assert_eq!(
			tone_and_fill(&cell(10_000, -8_000, 2_000)).0,
			MoneyTone::Low
		);
		assert_eq!(
			tone_and_fill(&cell(10_000, -7_999, 2_001)).0,
			MoneyTone::Available
		);
	}

	#[test]
	fn overspent_zero_and_income_have_their_own_tones() {
		assert_eq!(
			tone_and_fill(&cell(18_000, -21_475, -3_475)),
			(MoneyTone::Overspent, 0.0)
		);
		assert_eq!(
			tone_and_fill(&cell(165_000, -165_000, 0)),
			(MoneyTone::Neutral, 0.0)
		);
		let income = CategoryMonth {
			is_income: true,
			..cell(0, 400_000, 0)
		};
		assert_eq!(tone_and_fill(&income).0, MoneyTone::Income);
	}

	#[test]
	fn carried_in_money_counts_toward_a_full_bar() {
		// Gifts: 40.00 budgeted, 80.00 carried in, nothing spent: a full bar.
		assert_eq!(
			tone_and_fill(&cell(4_000, 0, 12_000)),
			(MoneyTone::Available, 1.0)
		);
		// A refund bigger than the budget still reads as full, not over 100%.
		assert_eq!(tone_and_fill(&cell(1_000, 500, 1_500)).1, 1.0);
	}

	#[test]
	fn dates_must_be_whole() {
		assert!(civil_date("2026-11-15").is_ok());
		assert!(civil_date("2026-11").is_err());
		assert!(civil_date("tomorrow").is_err());
	}
}
