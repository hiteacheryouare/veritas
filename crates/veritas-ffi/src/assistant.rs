//! The on-device assistant's two computed reads (R-AI-1,
//! docs/finance/assistant.md AI-10 and AI-12). Every figure is an integer in
//! minor units, computed here with checked arithmetic; the app only formats.

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::sync::Arc;

use veritas_domain::{BudgetData, CoreError, Money, YearMonth};
use veritas_reports::{self as reports, Classification, Period};

use crate::blocking::{CancelToken as CancelHandle, run};
use crate::budget::{BudgetMonth, civil_date, year_month};
use crate::money::CategoryKind;
use crate::names::name_key;
use crate::reports::{ReportCategory, period, report_category};
use crate::session::Session;
use crate::{FfiError, Month};

/// One category over the window's budgeted months (AI-10.3 to AI-10.5).
/// Amounts are on the budget scale, as the Budget screen shows them.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct HabitsCategory {
	pub category_id: String,
	pub name: String,
	pub group_name: String,
	/// Months with a budget above 0 (B).
	pub budgeted_months: u32,
	/// Budgeted months whose activity exceeded the budget (O).
	pub over_months: u32,
	/// Rounded once to the minor unit, half away from zero.
	pub average_activity: i64,
	pub average_budgeted: i64,
	/// `average_activity - average_budgeted`, from the two rounded averages.
	pub overage: i64,
}

/// What `budgetHabits` found (AI-10.1, AI-10.2, AI-10.4, AI-10.8).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum HabitsOutcome {
	/// No window month is on or after the budget's first month.
	NoHistory,
	/// Exactly one window month counts: no pattern yet.
	OneMonth,
	/// The currency's places aren't 2, so budgeted and activity can't be
	/// compared. `note` is the core's CLS-12.10 line when there is one.
	DifferentScales { note: Option<String> },
	/// No considered category has 2 or more budgeted months.
	NoBudgets,
	/// Nothing ran over in most months: the largest by average activity, at most 3.
	NothingOver { largest: Vec<HabitsCategory> },
	/// `qualifying` categories ran over; `shown` is the largest 3 by overage.
	/// The suggestion is the first shown category's overage and budgeted months.
	Over {
		qualifying: u32,
		shown: Vec<HabitsCategory>,
		suggestion: i64,
		suggestion_months: u32,
	},
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BudgetHabits {
	/// The window months that count, oldest first (N is its length).
	pub months: Vec<Month>,
	pub outcome: HabitsOutcome,
}

/// The categories left out of the shown lines (AI-12.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct EverythingElse {
	pub count: u32,
	pub total: i64,
}

/// The Net Profit Expenses section by category (AI-12), ranked and summed
/// here. `shown`, `everything_else` and `uncategorized` add up to
/// `expense_total` to the cent.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CategoryBreakdown {
	pub expense_total: i64,
	pub shown: Vec<ReportCategory>,
	pub everything_else: Option<EverythingElse>,
	pub uncategorized: Option<ReportCategory>,
}

const HABITS_SHOWN: usize = 3;
const BREAKDOWN_IN_FULL: usize = 6;

/// AI-10.1: the last 3 complete months before today's month that aren't
/// before the budget's first month, oldest first.
pub(crate) fn habits_window(
	today: YearMonth,
	first: YearMonth,
) -> Result<Vec<YearMonth>, CoreError> {
	let mut months = Vec::new();
	for back in [3, 2, 1] {
		let month = today.offset(-back)?;
		if month >= first {
			months.push(month);
		}
	}
	Ok(months)
}

/// AI-10.5: `sum / count` rounded once to the minor unit, half away from zero.
fn average(sum: i64, count: u32) -> Result<i64, CoreError> {
	let overflow = || CoreError::Invalid("A budget habits figure is too large.".to_owned());
	let count = i64::from(count);
	let doubled = sum
		.checked_abs()
		.and_then(|abs| abs.checked_mul(2))
		.and_then(|abs| abs.checked_add(count))
		.ok_or_else(overflow)?;
	let magnitude = doubled / count.checked_mul(2).ok_or_else(overflow)?;
	Ok(if sum < 0 { -magnitude } else { magnitude })
}

#[derive(Default)]
struct Tally {
	category_id: String,
	name: String,
	group_name: String,
	budgeted_months: u32,
	over_months: u32,
	activity: Money,
	budgeted: Money,
}

/// AI-10.2 to AI-10.8 over the window's Budget screen sheets, oldest first.
pub(crate) fn budget_habits_from(
	sheets: &[BudgetMonth],
	goal_categories: &BTreeSet<String>,
	decimal_places: u8,
) -> Result<BudgetHabits, CoreError> {
	let months = sheets.iter().map(|sheet| sheet.month).collect();
	let outcome = match sheets {
		[] => HabitsOutcome::NoHistory,
		[_] => HabitsOutcome::OneMonth,
		[.., last] if decimal_places != 2 => HabitsOutcome::DifferentScales {
			note: last.mixed_scale_note.clone(),
		},
		[.., last] => compare(sheets, last, goal_categories)?,
	};
	Ok(BudgetHabits { months, outcome })
}

fn compare(
	sheets: &[BudgetMonth],
	today: &BudgetMonth,
	goal_categories: &BTreeSet<String>,
) -> Result<HabitsOutcome, CoreError> {
	let compared = compared_categories(sheets, today, goal_categories)?;
	if compared.is_empty() {
		return Ok(HabitsOutcome::NoBudgets);
	}
	let mut listed = ranked_over(&compared);
	let Some(top) = listed.first().cloned() else {
		return Ok(HabitsOutcome::NothingOver {
			largest: largest(compared),
		});
	};
	let qualifying = listed.len() as u32;
	listed.truncate(HABITS_SHOWN);
	Ok(HabitsOutcome::Over {
		qualifying,
		shown: listed,
		suggestion: top.overage,
		suggestion_months: top.budgeted_months,
	})
}

/// AI-10.2 and AI-10.3: the considered categories with at least 2 budgeted
/// months, with their counts and rounded averages (AI-10.5).
fn compared_categories(
	sheets: &[BudgetMonth],
	today: &BudgetMonth,
	goal_categories: &BTreeSet<String>,
) -> Result<Vec<HabitsCategory>, CoreError> {
	let mut tallies: Vec<Tally> = Vec::new();
	for group in today
		.groups
		.iter()
		.filter(|group| !group.is_income && !group.hidden)
	{
		for row in &group.envelopes {
			if !row.hidden
				&& row.kind == CategoryKind::Expense
				&& !goal_categories.contains(&row.id)
			{
				tallies.push(Tally {
					category_id: row.id.clone(),
					name: row.name.clone(),
					group_name: group.name.clone(),
					..Tally::default()
				});
			}
		}
	}
	for sheet in sheets {
		for tally in &mut tallies {
			let Some(row) = sheet
				.groups
				.iter()
				.flat_map(|group| &group.envelopes)
				.find(|row| row.id == tally.category_id)
			else {
				continue;
			};
			if row.budgeted <= 0 {
				continue;
			}
			let activity = Money::from_minor(row.spent).checked_neg()?;
			let budgeted = Money::from_minor(row.budgeted);
			tally.budgeted_months += 1;
			tally.activity = tally.activity.checked_add(activity)?;
			tally.budgeted = tally.budgeted.checked_add(budgeted)?;
			if activity > budgeted {
				tally.over_months += 1;
			}
		}
	}
	let mut compared = Vec::new();
	for tally in tallies
		.into_iter()
		.filter(|tally| tally.budgeted_months >= 2)
	{
		let average_activity = average(tally.activity.minor(), tally.budgeted_months)?;
		let average_budgeted = average(tally.budgeted.minor(), tally.budgeted_months)?;
		compared.push(HabitsCategory {
			overage: Money::from_minor(average_activity)
				.checked_sub(Money::from_minor(average_budgeted))?
				.minor(),
			category_id: tally.category_id,
			name: tally.name,
			group_name: tally.group_name,
			budgeted_months: tally.budgeted_months,
			over_months: tally.over_months,
			average_activity,
			average_budgeted,
		});
	}
	Ok(compared)
}

/// AI-10.6's tie order: the AI-3.6 name key, then the category id.
fn ties(a: &HabitsCategory, b: &HabitsCategory) -> Ordering {
	name_key(&a.name)
		.cmp(&name_key(&b.name))
		.then_with(|| a.category_id.cmp(&b.category_id))
}

/// AI-10.4 and AI-10.6: the categories over in strictly more than half their
/// budgeted months with a positive overage, largest overage first.
fn ranked_over(compared: &[HabitsCategory]) -> Vec<HabitsCategory> {
	let mut listed: Vec<HabitsCategory> = compared
		.iter()
		.filter(|category| {
			2 * category.over_months > category.budgeted_months && category.overage > 0
		})
		.cloned()
		.collect();
	listed.sort_by(|a, b| {
		b.overage
			.cmp(&a.overage)
			.then_with(|| b.average_activity.cmp(&a.average_activity))
			.then_with(|| ties(a, b))
	});
	listed
}

/// AI-10.8: the largest by average activity, at most 3.
fn largest(mut compared: Vec<HabitsCategory>) -> Vec<HabitsCategory> {
	compared.sort_by(|a, b| {
		b.average_activity
			.cmp(&a.average_activity)
			.then_with(|| ties(a, b))
	});
	compared.truncate(HABITS_SHOWN);
	compared
}

/// AI-12.2 to AI-12.5: categories largest first, a negative last, zero lines
/// left out; above 6, the top 5 and the rest summed; Uncategorized apart.
pub(crate) fn rank_breakdown(
	expense_total: i64,
	lines: Vec<ReportCategory>,
) -> Result<CategoryBreakdown, CoreError> {
	let uncategorized = lines
		.iter()
		.find(|line| line.category_id.is_none() && line.total != 0)
		.cloned();
	let mut ranked: Vec<ReportCategory> = lines
		.into_iter()
		.filter(|line| line.category_id.is_some() && line.total != 0)
		.collect();
	ranked.sort_by(|a, b| match b.total.cmp(&a.total) {
		Ordering::Equal => name_key(&a.name)
			.cmp(&name_key(&b.name))
			.then_with(|| a.category_id.cmp(&b.category_id)),
		order => order,
	});
	let rest = if ranked.len() > BREAKDOWN_IN_FULL {
		ranked.split_off(BREAKDOWN_IN_FULL - 1)
	} else {
		Vec::new()
	};
	let everything_else = if rest.is_empty() {
		None
	} else {
		let total = rest.iter().try_fold(Money::ZERO, |sum, line| {
			sum.checked_add(Money::from_minor(line.total))
		})?;
		Some(EverythingElse {
			count: rest.len() as u32,
			total: total.minor(),
		})
	};
	Ok(CategoryBreakdown {
		expense_total,
		shown: ranked,
		everything_else,
		uncategorized,
	})
}

fn category_breakdown(
	budget: &BudgetData,
	classification: &Classification,
	period: Period,
) -> Result<CategoryBreakdown, CoreError> {
	let report = reports::net_profit(budget, classification, period)?;
	let lines = report
		.expenses
		.categories
		.iter()
		.map(|line| report_category(budget, line))
		.collect();
	rank_breakdown(report.expenses.total.minor(), lines)
}

#[uniffi::export]
impl Session {
	/// v20, R-AI-1 (AI-10): the categories that ran over their monthly budget
	/// in most of the last 3 complete months before `today`'s month.
	pub fn budget_habits(&self, today: String) -> Result<BudgetHabits, FfiError> {
		let this_month = civil_date(&today)?.year_month();
		let latest_month = this_month.offset(-1)?;
		let latest = self.core.budget_month(latest_month.into(), &today)?;
		let mut sheets = Vec::new();
		for month in habits_window(this_month, year_month(latest.first_month)?)? {
			let sheet = if month == latest_month {
				latest.clone()
			} else {
				self.core.budget_month(month.into(), &today)?
			};
			if year_month(sheet.month)? == month {
				sheets.push(sheet);
			}
		}
		let (goal_categories, decimal_places) = self.core.call(|session| {
			let data = session.data()?;
			Ok((
				data.goals
					.values()
					.map(|goal| goal.category.to_string())
					.collect::<BTreeSet<_>>(),
				data.format.decimal_places(),
			))
		})?;
		Ok(budget_habits_from(
			&sheets,
			&goal_categories,
			decimal_places,
		)?)
	}

	/// v20, R-AI-1 (AI-12): the Expenses line of Net Profit for an inclusive
	/// `YYYY-MM-DD` range, by category, ranked and summed in the core.
	pub async fn spending_by_category(
		&self,
		first: String,
		last: String,
		cancel: Arc<CancelHandle>,
	) -> Result<CategoryBreakdown, FfiError> {
		let core = self.core.clone();
		run(&cancel, move |cancel| {
			core.report(cancel, |budget, classification| {
				category_breakdown(budget, classification, period(&first, &last)?)
			})
		})
		.await
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::MoneyTone;
	use crate::budget::{EnvelopeGroup, EnvelopeRow};
	use crate::reports::ReportLabels;

	fn month(year: i32, month: u32) -> Month {
		YearMonth::new(year, month).unwrap().into()
	}

	/// One envelope across the window, oldest first: budgeted and activity per month.
	struct Envelope {
		id: &'static str,
		name: &'static str,
		group: &'static str,
		budgeted: Vec<i64>,
		activity: Vec<i64>,
		hidden: bool,
		kind: CategoryKind,
	}

	fn envelope(
		id: &'static str,
		name: &'static str,
		budgeted: [i64; 3],
		activity: [i64; 3],
	) -> Envelope {
		Envelope {
			id,
			name,
			group: "Everyday",
			budgeted: budgeted.to_vec(),
			activity: activity.to_vec(),
			hidden: false,
			kind: CategoryKind::Expense,
		}
	}

	fn sheets(
		envelopes: &[Envelope],
		months: &[Month],
		hidden_groups: &[&str],
		note: Option<&str>,
	) -> Vec<BudgetMonth> {
		let mut groups: Vec<&str> = Vec::new();
		for e in envelopes {
			if !groups.contains(&e.group) {
				groups.push(e.group);
			}
		}
		months
			.iter()
			.enumerate()
			.map(|(index, &month)| BudgetMonth {
				month,
				first_month: months[0],
				last_month: month,
				to_budget: 0,
				to_budget_tone: MoneyTone::Neutral,
				income: 0,
				from_last_month: 0,
				overspent_last_month: 0,
				budgeted: 0,
				for_next_month: 0,
				spent: 0,
				available: 0,
				groups: groups
					.iter()
					.map(|&group| EnvelopeGroup {
						id: group.to_owned(),
						name: group.to_owned(),
						is_income: group == "Income",
						hidden: hidden_groups.contains(&group),
						budgeted: 0,
						spent: 0,
						available: 0,
						envelopes: envelopes
							.iter()
							.filter(|e| e.group == group)
							.map(|e| EnvelopeRow {
								id: e.id.to_owned(),
								name: e.name.to_owned(),
								hidden: e.hidden,
								budgeted: e.budgeted[index],
								spent: -e.activity[index],
								available: 0,
								carryover: false,
								tone: MoneyTone::Neutral,
								fill: 0.0,
								note: None,
								kind: e.kind,
							})
							.collect(),
					})
					.collect(),
				mixed_scale_note: note.map(str::to_owned),
			})
			.collect()
	}

	fn window() -> Vec<Month> {
		vec![month(2026, 8), month(2026, 9), month(2026, 10)]
	}

	fn scenario_h() -> Vec<Envelope> {
		vec![
			envelope("dining", "Dining", [30_000; 3], [45_000, 38_250, 40_500]),
			envelope(
				"shopping",
				"Shopping",
				[15_000; 3],
				[26_010, 14_020, 29_000],
			),
			envelope(
				"rideshare",
				"Rideshare",
				[6_000; 3],
				[12_000, 6_600, 10_200],
			),
			envelope(
				"groceries",
				"Groceries",
				[45_000; 3],
				[44_000, 47_000, 41_000],
			),
			envelope("gas", "Gas", [10_000; 3], [12_000, 9_000, 9_000]),
		]
	}

	fn habits(envelopes: &[Envelope], goals: &[&str], hidden_groups: &[&str]) -> HabitsOutcome {
		let goals = goals.iter().map(|g| (*g).to_owned()).collect();
		budget_habits_from(
			&sheets(envelopes, &window(), hidden_groups, None),
			&goals,
			2,
		)
		.unwrap()
		.outcome
	}

	fn figures(category: &HabitsCategory) -> Figures<'_> {
		(
			category.name.as_str(),
			category.over_months,
			category.budgeted_months,
			category.average_activity,
			category.average_budgeted,
			category.overage,
		)
	}

	/// A shown category's name, O, B, averages and overage.
	type Figures<'a> = (&'a str, u32, u32, i64, i64, i64);

	fn over(outcome: &HabitsOutcome) -> (u32, Vec<Figures<'_>>, i64, u32) {
		let HabitsOutcome::Over {
			qualifying,
			shown,
			suggestion,
			suggestion_months,
		} = outcome
		else {
			panic!("{outcome:?}")
		};
		(
			*qualifying,
			shown.iter().map(figures).collect(),
			*suggestion,
			*suggestion_months,
		)
	}

	#[test]
	fn ex1_ryans_preview() {
		let outcome = habits(&scenario_h(), &[], &[]);
		let (k, shown, suggestion, months) = over(&outcome);
		assert_eq!(k, 3);
		assert_eq!(
			shown,
			[
				("Dining", 3, 3, 41_250, 30_000, 11_250),
				("Shopping", 2, 3, 23_010, 15_000, 8_010),
				("Rideshare", 3, 3, 9_600, 6_000, 3_600)
			]
		);
		assert_eq!((suggestion, months), (11_250, 3));
	}

	#[test]
	fn ex2_rounding_and_the_limit() {
		let mut envelopes = scenario_h();
		envelopes.push(envelope(
			"fun",
			"Fun",
			[20_000; 3],
			[21_000, 20_001, 20_000],
		));
		let outcome = habits(&envelopes, &[], &[]);
		let (k, shown, _, _) = over(&outcome);
		assert_eq!(k, 4);
		assert_eq!(shown.len(), 3);
		assert_eq!(average(61_001, 3).unwrap(), 20_334);
		let sheets = sheets(&envelopes, &window(), &[], None);
		let compared =
			compared_categories(&sheets, sheets.last().unwrap(), &BTreeSet::new()).unwrap();
		let ranked = ranked_over(&compared);
		assert_eq!(
			ranked.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
			["Dining", "Shopping", "Rideshare", "Fun"],
			"Fun ranks fourth"
		);
		assert_eq!(figures(&ranked[3]), ("Fun", 2, 3, 20_334, 20_000, 334));
	}

	#[test]
	fn ex3_nothing_over() {
		let outcome = habits(&scenario_h()[3..], &[], &[]);
		let HabitsOutcome::NothingOver { largest } = outcome else {
			panic!("{outcome:?}")
		};
		assert_eq!(
			largest.iter().map(figures).collect::<Vec<_>>(),
			[
				("Groceries", 1, 3, 44_000, 45_000, -1_000),
				("Gas", 1, 3, 10_000, 10_000, 0)
			]
		);
	}

	#[test]
	fn ex4_excluded_rows_change_nothing() {
		let mut envelopes = scenario_h();
		let mut hobbies = envelope("hobbies", "Hobbies", [10_000; 3], [30_000; 3]);
		hobbies.hidden = true;
		let mut principal = envelope("principal", "Car Loan Principal", [30_000; 3], [30_000; 3]);
		principal.kind = CategoryKind::DebtPrincipal;
		let mut salary = envelope("salary", "Salary", [0; 3], [-500_000; 3]);
		salary.group = "Income";
		salary.kind = CategoryKind::Income;
		let mut games = envelope("games", "Games", [1_000; 3], [9_000; 3]);
		games.group = "Hidden group";
		envelopes.extend([
			hobbies,
			envelope("vacation", "Vacation", [20_000; 3], [0, 0, 60_000]),
			principal,
			salary,
			envelope("coffee", "Coffee", [0; 3], [5_000; 3]),
			games,
		]);
		assert_eq!(
			habits(&envelopes, &["vacation"], &["Hidden group"]),
			habits(&scenario_h(), &[], &[])
		);
	}

	#[test]
	fn ex5_a_category_budgeted_partway() {
		let outcome = habits(
			&[envelope(
				"transit",
				"Transit",
				[0, 8_000, 8_000],
				[3_000, 9_500, 9_000],
			)],
			&[],
			&[],
		);
		let (_, shown, suggestion, months) = over(&outcome);
		assert_eq!(shown, [("Transit", 2, 2, 9_250, 8_000, 1_250)]);
		assert_eq!((suggestion, months), (1_250, 2));
	}

	#[test]
	fn ex6_over_in_most_months_but_under_on_average() {
		assert!(matches!(
			habits(
				&[envelope("odd", "Odd", [10_000; 3], [10_100, 10_100, 1_000])],
				&[],
				&[]
			),
			HabitsOutcome::NothingOver { .. }
		));
	}

	#[test]
	fn ex7_carryover_does_not_hide_it() {
		let mut with_carry = sheets(&scenario_h(), &window(), &[], None);
		for sheet in &mut with_carry {
			sheet.groups[0].envelopes[0].available = 50_000;
		}
		assert_eq!(
			budget_habits_from(&with_carry, &BTreeSet::new(), 2)
				.unwrap()
				.outcome,
			habits(&scenario_h(), &[], &[])
		);
	}

	#[test]
	fn ex8_and_15_the_window_starts_at_the_first_month() {
		let ym = |y, m| YearMonth::new(y, m).unwrap();
		assert_eq!(
			habits_window(ym(2026, 11), ym(2026, 1)).unwrap(),
			[ym(2026, 8), ym(2026, 9), ym(2026, 10)]
		);
		assert_eq!(
			habits_window(ym(2026, 11), ym(2026, 10)).unwrap(),
			[ym(2026, 10)]
		);
		assert_eq!(
			habits_window(ym(2026, 11), ym(2026, 9)).unwrap(),
			[ym(2026, 9), ym(2026, 10)],
			"a clamped August read is never asked for"
		);
		assert!(
			habits_window(ym(2026, 11), ym(2026, 11))
				.unwrap()
				.is_empty()
		);
		assert_eq!(
			budget_habits_from(&[], &BTreeSet::new(), 2)
				.unwrap()
				.outcome,
			HabitsOutcome::NoHistory
		);
		let dining_one = Envelope {
			budgeted: vec![30_000],
			activity: vec![45_000],
			..envelope("dining", "Dining", [0; 3], [0; 3])
		};
		assert_eq!(
			budget_habits_from(
				&sheets(&[dining_one], &[month(2026, 10)], &[], None),
				&BTreeSet::new(),
				2
			)
			.unwrap()
			.outcome,
			HabitsOutcome::OneMonth
		);
		let dining_two = Envelope {
			budgeted: vec![30_000, 30_000],
			activity: vec![45_000, 28_000],
			..envelope("dining", "Dining", [0; 3], [0; 3])
		};
		assert!(matches!(
			budget_habits_from(
				&sheets(&[dining_two], &[month(2026, 9), month(2026, 10)], &[], None),
				&BTreeSet::new(),
				2
			)
			.unwrap()
			.outcome,
			HabitsOutcome::NothingOver { .. }
		));
		let dining_15 = Envelope {
			budgeted: vec![30_000, 30_000],
			activity: vec![45_000, 40_500],
			..envelope("dining", "Dining", [0; 3], [0; 3])
		};
		let outcome = budget_habits_from(
			&sheets(&[dining_15], &[month(2026, 9), month(2026, 10)], &[], None),
			&BTreeSet::new(),
			2,
		)
		.unwrap()
		.outcome;
		let (_, shown, _, _) = over(&outcome);
		assert_eq!(shown, [("Dining", 2, 2, 42_750, 30_000, 12_750)]);
	}

	#[test]
	fn ex9_negative_activity_is_never_over() {
		let outcome = habits(
			&[envelope(
				"returns",
				"Returns",
				[10_000; 3],
				[12_000, -3_000, 12_000],
			)],
			&[],
			&[],
		);
		let HabitsOutcome::NothingOver { largest } = outcome else {
			panic!("{outcome:?}")
		};
		assert_eq!(
			figures(&largest[0]),
			("Returns", 2, 3, 7_000, 10_000, -3_000)
		);
	}

	#[test]
	fn ex10_a_currency_without_2_places() {
		let note = "Spending is recorded at 2 places and budgets at 0, so these numbers mix both, as in Actual.";
		let yen = budget_habits_from(
			&sheets(&scenario_h(), &window(), &[], Some(note)),
			&BTreeSet::new(),
			0,
		)
		.unwrap()
		.outcome;
		assert_eq!(
			yen,
			HabitsOutcome::DifferentScales {
				note: Some(note.to_owned())
			}
		);
		let dinar = budget_habits_from(
			&sheets(&scenario_h(), &window(), &[], None),
			&BTreeSet::new(),
			3,
		)
		.unwrap()
		.outcome;
		assert_eq!(dinar, HabitsOutcome::DifferentScales { note: None });
	}

	#[test]
	fn ex11_two_names() {
		let mut fun = envelope("misc-b", "Misc", [1_000; 3], [2_000; 3]);
		fun.group = "Fun";
		let outcome = habits(
			&[envelope("misc-a", "Misc", [1_000; 3], [3_000; 3]), fun],
			&[],
			&[],
		);
		let (_, shown, _, _) = over(&outcome);
		assert_eq!(
			shown.iter().map(|s| s.0).collect::<Vec<_>>(),
			["Misc", "Misc"]
		);
	}

	#[test]
	fn ex12_no_zero_overage() {
		assert!(matches!(
			habits(
				&[envelope(
					"even",
					"Even",
					[10_000; 3],
					[10_001, 10_001, 9_999]
				)],
				&[],
				&[]
			),
			HabitsOutcome::NothingOver { .. }
		));
	}

	#[test]
	fn ex13_and_14_windows_across_a_year() {
		let ym = |y, m| YearMonth::new(y, m).unwrap();
		assert_eq!(
			habits_window(ym(2026, 2), ym(2025, 1)).unwrap(),
			[ym(2025, 11), ym(2025, 12), ym(2026, 1)]
		);
		assert_eq!(
			habits_window(ym(2026, 2), ym(2025, 12)).unwrap(),
			[ym(2025, 12), ym(2026, 1)]
		);
		let months = [month(2025, 11), month(2025, 12), month(2026, 1)];
		let outcome = budget_habits_from(
			&sheets(
				&[envelope(
					"dining",
					"Dining",
					[30_000; 3],
					[31_000, 32_000, 30_000],
				)],
				&months,
				&[],
				None,
			),
			&BTreeSet::new(),
			2,
		)
		.unwrap()
		.outcome;
		let (_, shown, _, _) = over(&outcome);
		assert_eq!(shown, [("Dining", 2, 3, 31_000, 30_000, 1_000)]);
		let two = Envelope {
			budgeted: vec![30_000, 30_000],
			activity: vec![31_000, 32_000],
			..envelope("dining", "Dining", [0; 3], [0; 3])
		};
		let outcome = budget_habits_from(
			&sheets(&[two], &[month(2025, 12), month(2026, 1)], &[], None),
			&BTreeSet::new(),
			2,
		)
		.unwrap()
		.outcome;
		let (_, shown, suggestion, months) = over(&outcome);
		assert_eq!(shown, [("Dining", 2, 2, 31_500, 30_000, 1_500)]);
		assert_eq!((suggestion, months), (1_500, 2));
	}

	#[test]
	fn ties_go_to_the_ai_3_6_key_not_the_plain_fold() {
		assert_eq!(name_key("The Kids"), "kid");
		assert_eq!(name_key("Café & Snacks"), "cafe and snack");
		assert_eq!(name_key("Gifts"), name_key("gift"));
		assert!(
			crate::names::fold("Laundry") < crate::names::fold("The Kids")
				&& name_key("The Kids") < name_key("Laundry"),
			"the two keys disagree on this pair"
		);
		let tied = [
			envelope("laundry", "Laundry", [1_000; 3], [2_000; 3]),
			envelope("kids", "The Kids", [1_000; 3], [2_000; 3]),
		];
		let outcome = habits(&tied, &[], &[]);
		let (_, shown, _, _) = over(&outcome);
		assert_eq!(
			shown.iter().map(|s| s.0).collect::<Vec<_>>(),
			["The Kids", "Laundry"]
		);
		let b = breakdown(vec![
			line("Rent", 150_000),
			line("Laundry", 5_000),
			line("The Kids", 5_000),
		]);
		assert_eq!(
			names(&b),
			[("Rent", 150_000), ("The Kids", 5_000), ("Laundry", 5_000)]
		);
	}

	#[test]
	fn ex16_one_category_over() {
		assert_eq!(over(&habits(&scenario_h()[..1], &[], &[])).0, 1);
	}

	#[test]
	fn no_category_has_a_monthly_budget() {
		assert_eq!(
			habits(
				&[envelope("coffee", "Coffee", [0; 3], [5_000; 3])],
				&[],
				&[]
			),
			HabitsOutcome::NoBudgets
		);
	}

	#[test]
	fn an_overflowing_sum_is_an_error_not_a_figure() {
		let huge = envelope("huge", "Huge", [i64::MAX; 3], [1; 3]);
		assert!(
			budget_habits_from(&sheets(&[huge], &window(), &[], None), &BTreeSet::new(), 2)
				.is_err()
		);
	}

	// AI-12.8

	fn line(name: &str, total: i64) -> ReportCategory {
		ReportCategory {
			category_id: Some(name.to_lowercase()),
			name: name.to_owned(),
			group_id: None,
			group_name: None,
			labels: ReportLabels {
				hidden: false,
				archived: false,
				deleted: false,
				closed_goal: false,
				needs_review: false,
			},
			total,
			count: 1,
		}
	}

	fn uncategorized(total: i64) -> ReportCategory {
		ReportCategory {
			category_id: None,
			name: String::new(),
			labels: ReportLabels {
				needs_review: true,
				..line("", 0).labels
			},
			..line("", total)
		}
	}

	fn breakdown(lines: Vec<ReportCategory>) -> CategoryBreakdown {
		let total = lines.iter().map(|l| l.total).sum();
		rank_breakdown(total, lines).unwrap()
	}

	fn example1() -> Vec<ReportCategory> {
		vec![
			line("Rent", 150_000),
			line("Groceries", 41_200),
			line("Utilities", 12_000),
			line("Gas", 10_000),
			line("Dining out", 5_000),
			line("Fun Money", 4_500),
			line("Gifts", 2_500),
		]
	}

	fn names(b: &CategoryBreakdown) -> Vec<(&str, i64)> {
		b.shown.iter().map(|l| (l.name.as_str(), l.total)).collect()
	}

	fn adds_up(b: &CategoryBreakdown) {
		let shown: i64 = b.shown.iter().map(|l| l.total).sum();
		let rest = b.everything_else.map_or(0, |e| e.total);
		let unc = b.uncategorized.as_ref().map_or(0, |u| u.total);
		assert_eq!(shown + rest + unc, b.expense_total);
	}

	#[test]
	fn ex1_more_than_6_lines() {
		let b = breakdown(example1());
		assert_eq!(b.expense_total, 225_200);
		assert_eq!(
			names(&b),
			[
				("Rent", 150_000),
				("Groceries", 41_200),
				("Utilities", 12_000),
				("Gas", 10_000),
				("Dining out", 5_000)
			]
		);
		assert_eq!(
			b.everything_else,
			Some(EverythingElse {
				count: 2,
				total: 7_000
			})
		);
		adds_up(&b);
	}

	#[test]
	fn ex2_exactly_6_categories() {
		let b = breakdown(example1()[..6].to_vec());
		assert_eq!(b.shown.len(), 6);
		assert_eq!(b.everything_else, None);
		adds_up(&b);
	}

	#[test]
	fn ex3_a_negative_line_falls_into_everything_else() {
		let mut lines: Vec<_> = ["A", "B", "C", "D", "E", "F"]
			.iter()
			.enumerate()
			.map(|(i, n)| line(n, 100_000 - 10_000 * i as i64))
			.collect();
		lines.push(line("Dining out", -8_000));
		let b = breakdown(lines);
		assert_eq!(b.expense_total, 442_000);
		assert_eq!(
			b.everything_else,
			Some(EverythingElse {
				count: 2,
				total: 42_000
			})
		);
		adds_up(&b);
	}

	#[test]
	fn ex4_uncategorized_and_deleted() {
		let b = breakdown(vec![
			line("Groceries", 41_200),
			line("Gas", 10_000),
			line("Dining out", 5_000),
			line("Old Gym", 1_200),
			uncategorized(-4_000),
		]);
		assert_eq!(b.expense_total, 53_400);
		assert_eq!(b.shown.len(), 4);
		assert_eq!(b.uncategorized.as_ref().map(|u| u.total), Some(-4_000));
		adds_up(&b);
	}

	#[test]
	fn ex5_ties_go_to_the_folded_name() {
		let b = breakdown(vec![
			line("Rent", 150_000),
			line("Groceries", 41_200),
			line("Utilities", 12_000),
			line("Gas", 10_000),
			line("Dining out", 5_000),
			line("Coffee", 5_000),
			line("Gifts", 2_500),
		]);
		assert!(b.shown.iter().any(|l| l.name == "Coffee"));
		assert!(!b.shown.iter().any(|l| l.name == "Dining out"));
	}

	#[test]
	fn ex6_and_8_zero_lines_are_left_out() {
		let b = breakdown(vec![line("Groceries", 41_200), line("Dining out", 0)]);
		assert_eq!(names(&b), [("Groceries", 41_200)]);
		let empty = breakdown(vec![line("Dining out", 0)]);
		assert!(
			empty.shown.is_empty()
				&& empty.everything_else.is_none()
				&& empty.uncategorized.is_none()
		);
		assert!(breakdown(Vec::new()).shown.is_empty());
	}

	#[test]
	fn ex12_uncategorized_sits_outside_the_limit() {
		let mut lines = example1();
		lines.push(uncategorized(3_000));
		let b = breakdown(lines);
		assert_eq!(
			b.everything_else,
			Some(EverythingElse {
				count: 2,
				total: 7_000
			})
		);
		assert_eq!(b.uncategorized.as_ref().map(|u| u.total), Some(3_000));
		adds_up(&b);
	}

	#[test]
	fn an_overflowing_everything_else_is_an_error() {
		let tail = vec![
			line("A", 900),
			line("B", 800),
			line("C", 700),
			line("D", 600),
			line("E", 500),
			line("F", i64::MIN + 1),
			line("G", i64::MIN + 1),
		];
		assert!(
			rank_breakdown(0, tail).is_err(),
			"two near-minimum lines overflow the Everything else sum"
		);
	}

	#[test]
	fn the_fixtures_breakdown_adds_up_to_its_expenses() {
		let (_scratch, core) = crate::test_support::imported("all-features");
		let b = core
			.report(
				&veritas_session::CancelToken::default(),
				|budget, classification| {
					category_breakdown(budget, classification, period("2026-01-01", "2026-12-31")?)
				},
			)
			.unwrap();
		assert!(b.shown.len() <= BREAKDOWN_IN_FULL);
		adds_up(&b);
		assert_ne!(b.expense_total, 0);
	}
}
