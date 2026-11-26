//! The Wrapped story for a year (R-WRAP-1) and the shares its "hide amounts"
//! mode shows instead of amounts (R-WRAP-2), per docs/finance/forecasting.md
//! FC-8.

use std::sync::Arc;

use veritas_domain::{BudgetData, CoreError};
use veritas_forecast::{self as forecast, Change, FunFact, Measure};
use veritas_reports::Classification;

use crate::blocking::{CancelToken as CancelHandle, run};
use crate::budget::civil_date;
use crate::reports::ReportLabels;
use crate::session::Session;
use crate::{FfiError, Month};

/// A share is in tenths of a percent: 430 is 43.0%.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct WrappedCategory {
	pub category_id: String,
	pub name: String,
	/// Net spending, refunds netted; always positive.
	pub total: i64,
	pub labels: ReportLabels,
	/// Of categorized spending: the top five and "Everything else" add up to
	/// exactly 1000.
	pub share: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct WrappedGroup {
	pub group_id: String,
	pub name: String,
	pub total: i64,
	pub share: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct WrappedPayee {
	pub payee_id: String,
	pub name: String,
	pub total: i64,
	pub count: u64,
	pub share: i64,
}

/// v18: one bar of the biggest-month card's chart (FC-8 monthly series).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct WrappedSpendingMonth {
	pub month: Month,
	/// The month's `expense_total`, register scale; negative when refunds
	/// beat spending.
	pub total: i64,
	/// `total` over total out in tenths of a percent, for "hide amounts";
	/// `nil` when total out is 0 or less.
	pub share: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct WrappedMonth {
	pub month: Month,
	/// The month's Net Profit expenses.
	pub total: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct WrappedStreak {
	/// `YYYY-MM-DD`.
	pub start: String,
	pub days: u32,
}

/// Days with no discretionary spending. Bills and loan payments don't break
/// one, so a no-spend day can still have money going out.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct WrappedNoSpend {
	/// The evaluated range: from the budget's first row (or Jan 1) to Dec 31
	/// or today. `nil` before the budget's first row.
	pub first: Option<String>,
	pub last: Option<String>,
	pub days: u32,
	pub evaluated_days: u32,
	/// Ties go to the earliest.
	pub longest: Option<WrappedStreak>,
	/// The streak running through today, for the current year only.
	pub current: Option<u32>,
	/// v24, FC-8 N3: every no-spend date in the evaluated range, ascending,
	/// `YYYY-MM-DD`. A date of the year is no-spend (listed), spent (in the
	/// range, not listed) or not evaluated (outside `first`...`last`).
	#[uniffi(default = [])]
	pub dates: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum YearChange {
	/// Over last year's absolute value: 750 is +7.5%.
	BasisPoints { value: i64 },
	/// Last year's figure was zero.
	NewThisYear,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct WrappedPriorYear {
	pub total_in: YearChange,
	pub total_out: YearChange,
	pub net: YearChange,
	/// Set when the budget's first row falls inside last year's range, so
	/// last year's figures start there: "prior year from {date}".
	pub from: Option<String>,
}

/// "Hide amounts" (R-WRAP-2). Each is in tenths of a percent; `nil` is
/// "n/a", where the base is zero or negative. Counts, days and month names
/// show as they are.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct WrappedShares {
	/// Total out over total in; negative when refunds beat spending.
	pub out_vs_in: Option<i64>,
	/// `1000 - outVsIn`, so the two always add up to 1000.
	pub net: Option<i64>,
	/// Over total in; can pass 1000.
	pub debt_paydown: Option<i64>,
	pub savings: Option<i64>,
	/// The biggest month's spending over total out.
	pub biggest_month: Option<i64>,
	/// Over opening net worth.
	pub net_worth_change: Option<i64>,
	/// "Everything else" beside each top-five list, when it has any.
	pub other_categories: Option<i64>,
	pub other_groups: Option<i64>,
	pub other_payees: Option<i64>,
	/// v24, FC-8 N2: the typical month over total out; `nil` when total out
	/// is 0 or less.
	#[uniffi(default = None)]
	pub typical_month: Option<i64>,
}

/// v24, FC-8 N1: net worth at the end of one month of the period.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct WrappedMonthEnd {
	pub month: Month,
	/// The month's last day, or the period's `last` for a final month it ends inside.
	pub date: String,
	pub net_worth: i64,
	/// The change over `netWorthStart` in tenths of a percent; `nil` when
	/// `netWorthStart` is 0 or less, and then the line isn't drawn.
	pub share: Option<i64>,
}

/// v24, FC-8 N4: a goal reached in the year and the month it was first reached.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct WrappedReachedGoal {
	pub goal_id: String,
	/// The goal's category name, with no brackets.
	pub name: String,
	/// The category has been deleted: show "(deleted) {name}".
	pub deleted: bool,
	pub month: Month,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum WrappedMeasure {
	TotalIn,
	TotalOut,
	Net,
}

/// A comparison from the user's own numbers only, never a benchmark (FC-8).
/// The shell writes the words.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum WrappedFact {
	/// Only when last year's figure is non-zero and covers the same range.
	PriorYear {
		measure: WrappedMeasure,
		basis_points: i64,
	},
	BiggestMonthVsTypical {
		month: Month,
		basis_points: i64,
	},
	/// The top category's share of categorized spending, not of total out.
	TopCategoryShare {
		category_id: String,
		tenths: i64,
	},
	NoSpendShare {
		days: u32,
		evaluated_days: u32,
		tenths: i64,
	},
}

/// One year's Wrapped, or the current year to date. Amounts are stored
/// integers on the register scale, like every report.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct WrappedReport {
	pub year: i32,
	pub first: String,
	/// Dec 31, or today for the current year.
	pub last: String,
	/// `false`: "No data for this year", and every figure is zero.
	pub has_data: bool,
	/// Net Profit's income and expenses. Debt paydown and savings are their
	/// own cards, never inside total out.
	pub total_in: i64,
	pub total_out: i64,
	pub net: i64,
	pub debt_paydown: i64,
	pub savings: i64,
	/// Top five by net spending; uncategorized rows aren't a category.
	pub top_categories: Vec<WrappedCategory>,
	pub other_categories: i64,
	pub top_groups: Vec<WrappedGroup>,
	pub other_groups: i64,
	/// Transfer payees left out.
	pub top_payees: Vec<WrappedPayee>,
	pub other_payees: i64,
	pub biggest_month: Option<WrappedMonth>,
	/// Over the median month with data; `nil` when that median isn't
	/// positive.
	pub biggest_vs_typical: Option<i64>,
	pub no_spend: WrappedNoSpend,
	/// Goals first reached this year, closed or spent since included.
	pub completed_goal_ids: Vec<String>,
	pub net_worth_start: i64,
	pub net_worth_end: i64,
	pub net_worth_change: i64,
	pub prior_year: WrappedPriorYear,
	pub shares: WrappedShares,
	pub fun_facts: Vec<WrappedFact>,
	/// v18: every month from January, or the month of the budget's first
	/// row, through the last month of the period, oldest first.
	#[uniffi(default = [])]
	pub months: Vec<WrappedSpendingMonth>,
	/// v24: the open budget's name, for the cover. The FC-8 copy rule lists
	/// it as a report field.
	#[uniffi(default = "")]
	pub budget_name: String,
	/// v24, N1: the months of `months`, each at its month end.
	#[uniffi(default = [])]
	pub month_ends: Vec<WrappedMonthEnd>,
	/// v24, N1: the lowest and highest of `monthEnds`, ties to the earliest.
	#[uniffi(default = None)]
	pub lowest_month_end: Option<WrappedMonthEnd>,
	#[uniffi(default = None)]
	pub highest_month_end: Option<WrappedMonthEnd>,
	/// v24, N2: the median month `biggestVsTypical` compares with, as an
	/// amount. Draw its reference line only when it is above 0.
	#[uniffi(default = None)]
	pub typical_month: Option<i64>,
	/// v24, N4: by month, then goal id. Its length is the goals completed.
	#[uniffi(default = [])]
	pub reached_goals: Vec<WrappedReachedGoal>,
}

fn year_change(change: Change) -> YearChange {
	match change {
		Change::BasisPoints(value) => YearChange::BasisPoints { value },
		Change::NewThisYear => YearChange::NewThisYear,
	}
}

/// The share for each top entry, in order, and "Everything else" when the
/// set has one.
fn split<Id>(shares: &[(Option<Id>, i64)]) -> (Vec<i64>, Option<i64>) {
	let ranked = shares
		.iter()
		.filter(|(id, _)| id.is_some())
		.map(|(_, share)| *share)
		.collect();
	let other = shares
		.iter()
		.find(|(id, _)| id.is_none())
		.map(|(_, share)| *share);
	(ranked, other)
}

fn fact(fact: FunFact) -> WrappedFact {
	match fact {
		FunFact::PriorYear {
			measure,
			basis_points,
		} => WrappedFact::PriorYear {
			measure: match measure {
				Measure::TotalIn => WrappedMeasure::TotalIn,
				Measure::TotalOut => WrappedMeasure::TotalOut,
				Measure::Net => WrappedMeasure::Net,
			},
			basis_points,
		},
		FunFact::BiggestMonthVsTypical {
			month,
			basis_points,
		} => WrappedFact::BiggestMonthVsTypical {
			month: month.into(),
			basis_points,
		},
		FunFact::TopCategoryShare { category, tenths } => WrappedFact::TopCategoryShare {
			category_id: category.to_string(),
			tenths,
		},
		FunFact::NoSpendShare {
			days,
			evaluated_days,
			tenths,
		} => WrappedFact::NoSpendShare {
			days,
			evaluated_days,
			tenths,
		},
	}
}

fn month_end(end: forecast::MonthEnd, share: Option<i64>) -> WrappedMonthEnd {
	WrappedMonthEnd {
		month: end.month.into(),
		date: end.date.to_string(),
		net_worth: end.net_worth.minor(),
		share,
	}
}

pub(crate) fn wrapped_report(
	budget: &BudgetData,
	classification: &Classification,
	year: i32,
	today: veritas_domain::CivilDate,
) -> Result<WrappedReport, CoreError> {
	let wrapped = forecast::wrapped(budget, classification, year, today)?;
	let redacted = wrapped.redacted();
	let (category_shares, other_categories_share) = split(&redacted.categories);
	let (group_shares, other_groups_share) = split(&redacted.groups);
	let (payee_shares, other_payees_share) = split(&redacted.payees);
	let share = |shares: &[i64], index: usize| shares.get(index).copied().unwrap_or_default();
	Ok(WrappedReport {
		year,
		first: wrapped.period.first().to_string(),
		last: wrapped.period.last().to_string(),
		has_data: wrapped.has_data,
		total_in: wrapped.total_in.minor(),
		total_out: wrapped.total_out.minor(),
		net: wrapped.net.minor(),
		debt_paydown: wrapped.debt_paydown.minor(),
		savings: wrapped.savings.minor(),
		top_categories: wrapped
			.top_categories
			.iter()
			.enumerate()
			.map(|(index, rank)| WrappedCategory {
				category_id: rank.category.to_string(),
				name: rank.name.clone(),
				total: rank.total.minor(),
				labels: rank.labels.into(),
				share: share(&category_shares, index),
			})
			.collect(),
		other_categories: wrapped.other_categories.minor(),
		top_groups: wrapped
			.top_groups
			.iter()
			.enumerate()
			.map(|(index, rank)| WrappedGroup {
				group_id: rank.group.to_string(),
				name: rank.name.clone(),
				total: rank.total.minor(),
				share: share(&group_shares, index),
			})
			.collect(),
		other_groups: wrapped.other_groups.minor(),
		top_payees: wrapped
			.top_payees
			.iter()
			.enumerate()
			.map(|(index, rank)| WrappedPayee {
				payee_id: rank.payee.to_string(),
				name: rank.name.clone(),
				total: rank.total.minor(),
				count: rank.rows as u64,
				share: share(&payee_shares, index),
			})
			.collect(),
		other_payees: wrapped.other_payees.minor(),
		biggest_month: wrapped.biggest_month.map(|(month, total)| WrappedMonth {
			month: month.into(),
			total: total.minor(),
		}),
		biggest_vs_typical: wrapped.biggest_vs_typical,
		no_spend: WrappedNoSpend {
			first: wrapped
				.no_spend
				.evaluated
				.map(|range| range.first().to_string()),
			last: wrapped
				.no_spend
				.evaluated
				.map(|range| range.last().to_string()),
			days: wrapped.no_spend.days,
			evaluated_days: wrapped.no_spend.evaluated_days(),
			longest: wrapped.no_spend.longest.map(|streak| WrappedStreak {
				start: streak.start.to_string(),
				days: streak.days,
			}),
			current: wrapped.no_spend.current,
			dates: wrapped
				.no_spend
				.dates
				.iter()
				.map(ToString::to_string)
				.collect(),
		},
		completed_goal_ids: wrapped
			.completed_goals
			.iter()
			.map(ToString::to_string)
			.collect(),
		net_worth_start: wrapped.net_worth_start.minor(),
		net_worth_end: wrapped.net_worth_end.minor(),
		net_worth_change: wrapped.net_worth_change.minor(),
		prior_year: WrappedPriorYear {
			total_in: year_change(wrapped.prior_year.total_in),
			total_out: year_change(wrapped.prior_year.total_out),
			net: year_change(wrapped.prior_year.net),
			from: wrapped.prior_year.from.map(|date| date.to_string()),
		},
		shares: WrappedShares {
			out_vs_in: redacted.out_vs_in,
			net: redacted.net,
			debt_paydown: redacted.debt_paydown,
			savings: redacted.savings,
			biggest_month: redacted.biggest_month,
			net_worth_change: redacted.net_worth_change,
			other_categories: other_categories_share,
			other_groups: other_groups_share,
			other_payees: other_payees_share,
			typical_month: redacted.typical_month,
		},
		fun_facts: wrapped.fun_facts().into_iter().map(fact).collect(),
		months: wrapped
			.monthly_spending
			.iter()
			.zip(&redacted.months)
			.map(|((month, total), share)| WrappedSpendingMonth {
				month: (*month).into(),
				total: total.minor(),
				share: *share,
			})
			.collect(),
		budget_name: String::new(),
		month_ends: wrapped
			.month_ends
			.iter()
			.zip(&redacted.month_ends)
			.map(|(end, share)| month_end(*end, *share))
			.collect(),
		lowest_month_end: wrapped
			.lowest_month_end
			.map(|end| month_end(end, redacted.lowest_month_end)),
		highest_month_end: wrapped
			.highest_month_end
			.map(|end| month_end(end, redacted.highest_month_end)),
		typical_month: wrapped.typical_month.map(|typical| typical.minor()),
		reached_goals: wrapped
			.reached_goals
			.iter()
			.map(|goal| WrappedReachedGoal {
				goal_id: goal.goal.to_string(),
				name: goal.name.clone(),
				deleted: goal.deleted,
				month: goal.month.into(),
			})
			.collect(),
	})
}

#[uniffi::export]
impl Session {
	/// Wrapped for a completed year, or the current year to date. `today`
	/// is the local civil date, `YYYY-MM-DD`; a year after it is `Invalid`.
	pub async fn wrapped(
		&self,
		year: i32,
		today: String,
		cancel: Arc<CancelHandle>,
	) -> Result<WrappedReport, FfiError> {
		let core = self.core.clone();
		run(&cancel, move |cancel| {
			core.pin_today(&today)?;
			let budget_name = core.call(|session| session.budget_name())?;
			let mut report = core.report(cancel, |budget, classification| {
				wrapped_report(budget, classification, year, civil_date(&today)?)
			})?;
			report.budget_name = budget_name.unwrap_or_default();
			Ok(report)
		})
		.await
	}

	/// v18: the years Wrapped's picker offers, newest first: each year from
	/// the budget's first row through today's whose Wrapped has data.
	pub async fn wrapped_years(
		&self,
		today: String,
		cancel: Arc<CancelHandle>,
	) -> Result<Vec<i32>, FfiError> {
		let core = self.core.clone();
		run(&cancel, move |cancel| {
			core.pin_today(&today)?;
			core.report(cancel, |_, classification| {
				forecast::wrapped_years(classification, civil_date(&today)?)
			})
		})
		.await
	}
}
