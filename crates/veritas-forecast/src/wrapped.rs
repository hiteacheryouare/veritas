use std::collections::{BTreeMap, BTreeSet};

use veritas_domain::{
	BudgetData, CategoryGroupId, CategoryId, CivilDate, CoreError, GoalId, Money, PayeeId,
	SplitRole, TransactionId, TransactionKind, YearMonth,
};
use veritas_reports::{
	CategoryLabels, Classification, ClassifiedRow, Leg, Period, net_profit, net_worth,
};

use crate::history::bill_linked;
use crate::math::{median, round_half_away, to_money};

const TOP: usize = 5;
const WHOLE: i64 = 1000;

#[derive(Clone, Debug, PartialEq)]
pub struct CategoryRank {
	pub category: CategoryId,
	pub name: String,
	pub total: Money,
	pub labels: CategoryLabels,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GroupRank {
	pub group: CategoryGroupId,
	pub name: String,
	pub total: Money,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PayeeRank {
	pub payee: PayeeId,
	pub name: String,
	pub total: Money,
	pub rows: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Streak {
	pub start: CivilDate,
	pub days: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NoSpend {
	/// `max(Jan 1, first row) ..= min(Dec 31, today)`; `None` before the first row.
	pub evaluated: Option<Period>,
	pub days: u32,
	pub longest: Option<Streak>,
	/// The streak running through today, for the current year only.
	pub current: Option<u32>,
}

impl NoSpend {
	/// Days in the evaluated range, the base of the no-spend share.
	pub fn evaluated_days(&self) -> u32 {
		self.evaluated.map_or(0, |range| {
			let days = (range.last().naive() - range.first().naive()).num_days() + 1;
			u32::try_from(days).unwrap_or(0)
		})
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
	/// Basis points over the prior year's absolute value.
	BasisPoints(i64),
	NewThisYear,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PriorYear {
	pub total_in: Change,
	pub total_out: Change,
	pub net: Change,
	/// Set when the budget's first row falls inside the prior range, so the
	/// prior figures cover only the days from it ("prior year from {date}").
	pub from: Option<CivilDate>,
}

/// FC-8's aggregates for one calendar year, or the current year to date (R-WRAP-1).
#[derive(Clone, Debug, PartialEq)]
pub struct Wrapped {
	pub year: i32,
	pub period: Period,
	pub has_data: bool,
	pub total_in: Money,
	pub total_out: Money,
	pub net: Money,
	pub debt_paydown: Money,
	pub savings: Money,
	pub top_categories: Vec<CategoryRank>,
	/// The other categories with a positive net. Uncategorized rows are not a
	/// category, so they are in neither the ranking nor this (FC-8).
	pub other_categories: Money,
	pub top_groups: Vec<GroupRank>,
	/// The other groups with a positive net, for the group shares.
	pub other_groups: Money,
	pub top_payees: Vec<PayeeRank>,
	pub other_payees: Money,
	pub biggest_month: Option<(YearMonth, Money)>,
	pub biggest_vs_typical: Option<i64>,
	/// FC-8's monthly series: each month's `expense_total` from the month of
	/// the budget's first row (or January) through the period's last month,
	/// 0 for a month without rows. Never read by `biggest_month`.
	pub monthly_spending: Vec<(YearMonth, Money)>,
	pub no_spend: NoSpend,
	pub goals_completed: usize,
	/// The goals behind `goals_completed`, by id.
	pub completed_goals: Vec<GoalId>,
	pub net_worth_start: Money,
	pub net_worth_end: Money,
	pub net_worth_change: Money,
	pub prior_year: PriorYear,
}

/// R-WRAP-2: shares in tenths of a percent, never an amount. `None` is
/// "n/a": the base is zero or negative.
#[derive(Clone, Debug, PartialEq)]
pub struct Redacted {
	/// Top categories then `None` for everything else; sums to exactly 1000.
	pub categories: Vec<(Option<CategoryId>, i64)>,
	/// The same rule over category groups.
	pub groups: Vec<(Option<CategoryGroupId>, i64)>,
	pub payees: Vec<(Option<PayeeId>, i64)>,
	pub out_vs_in: Option<i64>,
	/// `1000 - out_vs_in`, so out and net always add up to 1000.
	pub net: Option<i64>,
	/// Over total in; either can pass 1000.
	pub debt_paydown: Option<i64>,
	pub savings: Option<i64>,
	/// The biggest month's spending over total out.
	pub biggest_month: Option<i64>,
	/// Each of `monthly_spending` over total out, in its order, rounded one
	/// by one (FC-8 monthly series).
	pub months: Vec<Option<i64>>,
	pub net_worth_change: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Measure {
	TotalIn,
	TotalOut,
	Net,
}

/// A comparison made only from the user's own numbers (FC-8). Each carries
/// its figures; the shell writes the words.
#[derive(Clone, Debug, PartialEq)]
pub enum FunFact {
	/// Only when last year's figure is non-zero and covers the whole range.
	PriorYear {
		measure: Measure,
		basis_points: i64,
	},
	BiggestMonthVsTypical {
		month: YearMonth,
		basis_points: i64,
	},
	/// The top category's share of categorized spending, not of total out:
	/// uncategorized rows aren't in the base.
	TopCategoryShare {
		category: CategoryId,
		tenths: i64,
	},
	/// No-spend days over the evaluated days. Bills and loan lines don't break
	/// a no-spend day, so it never means nothing was paid.
	NoSpendShare {
		days: u32,
		evaluated_days: u32,
		tenths: i64,
	},
}

impl Wrapped {
	pub fn no_data_text(&self) -> Option<&'static str> {
		(!self.has_data).then_some("No data for this year")
	}

	pub fn redacted(&self) -> Redacted {
		let categories = shares(
			self.top_categories
				.iter()
				.map(|rank| (Some(rank.category.clone()), rank.total))
				.collect(),
			self.other_categories,
		);
		let groups = shares(
			self.top_groups
				.iter()
				.map(|rank| (Some(rank.group.clone()), rank.total))
				.collect(),
			self.other_groups,
		);
		let payees = shares(
			self.top_payees
				.iter()
				.map(|rank| (Some(rank.payee.clone()), rank.total))
				.collect(),
			self.other_payees,
		);
		let out_vs_in = tenths(self.total_out, self.total_in);
		Redacted {
			categories,
			groups,
			payees,
			out_vs_in,
			net: out_vs_in.map(|out| WHOLE - out),
			debt_paydown: tenths(self.debt_paydown, self.total_in),
			savings: tenths(self.savings, self.total_in),
			biggest_month: self
				.biggest_month
				.and_then(|(_, total)| tenths(total, self.total_out)),
			months: self
				.monthly_spending
				.iter()
				.map(|(_, total)| tenths(*total, self.total_out))
				.collect(),
			net_worth_change: tenths(self.net_worth_change, self.net_worth_start),
		}
	}

	pub fn fun_facts(&self) -> Vec<FunFact> {
		let mut facts = Vec::new();
		if self.prior_year.from.is_none() {
			for (measure, change) in [
				(Measure::TotalIn, self.prior_year.total_in),
				(Measure::TotalOut, self.prior_year.total_out),
				(Measure::Net, self.prior_year.net),
			] {
				if let Change::BasisPoints(basis_points) = change {
					facts.push(FunFact::PriorYear {
						measure,
						basis_points,
					});
				}
			}
		}
		if let (Some((month, _)), Some(basis_points)) =
			(self.biggest_month, self.biggest_vs_typical)
		{
			facts.push(FunFact::BiggestMonthVsTypical {
				month,
				basis_points,
			});
		}
		// FC-8: rounded half away from zero on its own, not the redaction's
		// largest remainder, which can hand the top row an extra tenth.
		let categorized = Money::checked_sum(self.top_categories.iter().map(|rank| rank.total))
			.and_then(|top| top.checked_add(self.other_categories));
		if let (Some(top), Ok(categorized)) = (self.top_categories.first(), categorized)
			&& let Some(tenths) = tenths(top.total, categorized)
		{
			facts.push(FunFact::TopCategoryShare {
				category: top.category.clone(),
				tenths,
			});
		}
		let evaluated_days = self.no_spend.evaluated_days();
		if let Some(tenths) = tenths(
			Money::from_minor(i64::from(self.no_spend.days)),
			Money::from_minor(i64::from(evaluated_days)),
		) {
			facts.push(FunFact::NoSpendShare {
				days: self.no_spend.days,
				evaluated_days,
				tenths,
			});
		}
		facts
	}
}

/// `part` over `whole` in tenths of a percent, `round_half_away` because
/// `part` can be negative (FC-8). `None` when `whole <= 0`.
fn tenths(part: Money, whole: Money) -> Option<i64> {
	(whole > Money::ZERO).then(|| {
		let value = round_half_away(
			i128::from(part.minor()) * i128::from(WHOLE),
			i128::from(whole.minor()),
		);
		i64::try_from(value).unwrap_or(i64::MAX)
	})
}

fn shares<Id: Clone>(mut set: Vec<(Option<Id>, Money)>, other: Money) -> Vec<(Option<Id>, i64)> {
	if other > Money::ZERO {
		set.push((None, other));
	}
	let amounts: Vec<i64> = set.iter().map(|(_, amount)| amount.minor()).collect();
	match largest_remainder(&amounts, WHOLE) {
		Some(shares) => set.into_iter().map(|(id, _)| id).zip(shares).collect(),
		None => Vec::new(),
	}
}

/// Positive nets, largest first, ties by name (code point order) then id.
fn rank<T>(
	mut items: Vec<T>,
	key: impl Fn(&T) -> (Money, &str, String),
) -> Result<(Vec<T>, Money), CoreError> {
	items.retain(|item| key(item).0 > Money::ZERO);
	items.sort_by(|a, b| {
		let (a, b) = (key(a), key(b));
		b.0.cmp(&a.0)
			.then_with(|| a.1.cmp(b.1))
			.then_with(|| a.2.cmp(&b.2))
	});
	let rest = items.split_off(TOP.min(items.len()));
	let other = Money::checked_sum(rest.iter().map(|item| key(item).0))?;
	Ok((items, other))
}

fn change(current: Money, previous: Money) -> Change {
	if previous == Money::ZERO {
		return Change::NewThisYear;
	}
	let delta = i128::from(current.minor()) - i128::from(previous.minor());
	let value = round_half_away(delta * 10_000, i128::from(previous.minor()).abs());
	Change::BasisPoints(i64::try_from(value).unwrap_or(i64::MAX))
}

fn same_day_in(year: i32, date: CivilDate) -> Result<CivilDate, CoreError> {
	CivilDate::new(year, date.month(), date.day())
		.or_else(|_| CivilDate::new(year, date.month(), date.day().saturating_sub(1)))
}

fn payee_of(budget: &BudgetData, row: &ClassifiedRow) -> Option<PayeeId> {
	let own = budget
		.transactions
		.get(&row.id)
		.and_then(|row| row.payee.clone());
	let payee = own.or_else(|| {
		row.parent
			.as_ref()
			.and_then(|parent| budget.transactions.get(parent))
			.and_then(|parent| parent.payee.clone())
	})?;
	budget.resolve_payee(&payee).cloned()
}

struct Totals {
	total_in: Money,
	total_out: Money,
	debt_paydown: Money,
	savings: Money,
}

fn totals(
	budget: &BudgetData,
	classification: &Classification,
	period: Period,
) -> Result<(Totals, veritas_reports::NetProfit), CoreError> {
	let report = net_profit(budget, classification, period)?;
	Ok((
		Totals {
			total_in: report.income.total,
			total_out: report.expenses.total,
			debt_paydown: report.debt_paydown.total,
			savings: report.savings.total,
		},
		report,
	))
}

/// FC-8's period: the calendar year, or January 1 through today for the
/// current year.
fn period_of(year: i32, today: CivilDate) -> Result<Period, CoreError> {
	if year > today.year() {
		return Err(CoreError::Invalid(format!(
			"{year} hasn't started yet, so there's nothing to wrap"
		)));
	}
	let last = if year == today.year() {
		today
	} else {
		CivilDate::new(year, 12, 31)?
	};
	Period::new(CivilDate::new(year, 1, 1)?, last)
}

/// The one test behind both `Wrapped::has_data` and `wrapped_years`, so the
/// picker lists a year exactly when its Wrapped isn't "No data for this year":
/// a non-mirror leg dated in the period, transfers included (FC-8).
fn has_data(classification: &Classification, period: Period) -> bool {
	classification
		.within(period)
		.iter()
		.any(|row| row.leg != Leg::Mirror)
}

/// FC-8's years with data, newest first: from the year of the first row
/// through today's year, each one whose Wrapped has data.
pub fn wrapped_years(
	classification: &Classification,
	today: CivilDate,
) -> Result<Vec<i32>, CoreError> {
	let Some(first) = classification.rows().first().map(|row| row.date.year()) else {
		return Ok(Vec::new());
	};
	let mut years = Vec::new();
	for year in (first..=today.year()).rev() {
		if has_data(classification, period_of(year, today)?) {
			years.push(year);
		}
	}
	Ok(years)
}

pub fn wrapped(
	budget: &BudgetData,
	classification: &Classification,
	year: i32,
	today: CivilDate,
) -> Result<Wrapped, CoreError> {
	let period = period_of(year, today)?;
	let (first, last) = (period.first(), period.last());
	let rows = classification.within(period);
	let (current, report) = totals(budget, classification, period)?;

	let categories = report
		.expenses
		.categories
		.iter()
		.filter_map(|line| {
			let category = line.category.as_ref()?;
			Some(CategoryRank {
				category: category.clone(),
				name: budget
					.categories
					.get(category)
					.map(|category| category.name.clone())
					.unwrap_or_default(),
				total: line.total,
				labels: line.labels,
			})
		})
		.collect();
	let (top_categories, other_categories) = rank(categories, |rank| {
		(rank.total, rank.name.as_str(), rank.category.to_string())
	})?;

	let mut groups: BTreeMap<CategoryGroupId, i64> = BTreeMap::new();
	for line in &report.expenses.categories {
		let Some(group) = line
			.category
			.as_ref()
			.and_then(|category| budget.categories.get(category))
			.map(|category| category.group.clone())
		else {
			continue;
		};
		let total = groups.entry(group).or_default();
		*total = total
			.checked_add(line.total.minor())
			.ok_or(CoreError::Overflow)?;
	}
	let groups = groups
		.into_iter()
		.map(|(group, total)| GroupRank {
			name: budget
				.category_groups
				.get(&group)
				.map(|group| group.name.clone())
				.unwrap_or_default(),
			group,
			total: Money::from_minor(total),
		})
		.collect();
	let (top_groups, other_groups) = rank(groups, |rank| {
		(rank.total, rank.name.as_str(), rank.group.to_string())
	})?;

	let expense_rows = rows
		.iter()
		.filter(|row| row.is_primary() && row.kind() == TransactionKind::Expense);
	let mut payees: BTreeMap<PayeeId, (i64, usize)> = BTreeMap::new();
	let mut months: BTreeMap<YearMonth, i64> = BTreeMap::new();
	for row in expense_rows.clone() {
		let month = months.entry(row.date.year_month()).or_default();
		*month = month
			.checked_sub(row.amount.minor())
			.ok_or(CoreError::Overflow)?;
		let Some(payee) = payee_of(budget, row).filter(|payee| {
			budget
				.payees
				.get(payee)
				.is_some_and(|payee| payee.transfer_account.is_none())
		}) else {
			continue;
		};
		let (total, count) = payees.entry(payee).or_default();
		*total = total
			.checked_sub(row.amount.minor())
			.ok_or(CoreError::Overflow)?;
		*count += 1;
	}
	let payees = payees
		.into_iter()
		.map(|(payee, (total, rows))| PayeeRank {
			name: budget
				.payees
				.get(&payee)
				.map(|payee| payee.name.clone())
				.unwrap_or_default(),
			payee,
			total: Money::from_minor(total),
			rows,
		})
		.collect();
	let (top_payees, other_payees) = rank(payees, |rank| {
		(rank.total, rank.name.as_str(), rank.payee.to_string())
	})?;

	let biggest_month = months.iter().fold(
		None,
		|best: Option<(YearMonth, i64)>, (month, total)| match best {
			Some((_, max)) if max >= *total => best,
			_ => Some((*month, *total)),
		},
	);
	let monthly_spending = monthly_series(classification, period, &months)?;
	let monthly: Vec<i64> = months.values().copied().collect();
	let biggest_vs_typical = match (biggest_month, median(&monthly)) {
		(Some((_, max)), Some(mid)) if mid > 0 => Some(
			i64::try_from(round_half_away(
				(i128::from(max) - i128::from(mid)) * 10_000,
				i128::from(mid),
			))
			.map_err(|_| CoreError::Overflow)?,
		),
		_ => None,
	};

	let no_spend = no_spend(budget, classification, period, year == today.year())?;
	let completed_goals: Vec<GoalId> = budget
		.goals
		.values()
		.filter(|goal| goal.reached_month.is_some_and(|month| month.year() == year))
		.map(|goal| goal.id.clone())
		.collect();

	let day_before = first.naive().pred_opt().map_or(first, CivilDate::from);
	let net_worth_start = net_worth(classification, day_before)?;
	let net_worth_end = net_worth(classification, last)?;

	let prior = Period::new(
		CivilDate::new(year - 1, 1, 1)?,
		same_day_in(year - 1, last)?,
	)?;
	let (previous, _) = totals(budget, classification, prior)?;
	let from = classification
		.rows()
		.first()
		.map(|row| row.date)
		.filter(|first| prior.first() < *first && *first <= prior.last());
	let net = current.total_in.checked_sub(current.total_out)?;
	let previous_net = previous.total_in.checked_sub(previous.total_out)?;

	Ok(Wrapped {
		year,
		period,
		has_data: has_data(classification, period),
		total_in: current.total_in,
		total_out: current.total_out,
		net,
		debt_paydown: current.debt_paydown,
		savings: current.savings,
		top_categories,
		other_categories,
		top_groups,
		other_groups,
		top_payees,
		other_payees,
		biggest_month: biggest_month.map(|(month, total)| (month, Money::from_minor(total))),
		biggest_vs_typical,
		monthly_spending,
		no_spend,
		goals_completed: completed_goals.len(),
		completed_goals,
		net_worth_start,
		net_worth_end,
		net_worth_change: to_money(
			i128::from(net_worth_end.minor()) - i128::from(net_worth_start.minor()),
		)?,
		prior_year: PriorYear {
			total_in: change(current.total_in, previous.total_in),
			total_out: change(current.total_out, previous.total_out),
			net: change(net, previous_net),
			from,
		},
	})
}

/// Every month of `period` from the month of the budget's first row, with
/// its `expense_total` or 0. Months before that row are left out, as the
/// evaluated range leaves out its days, so no bar reads "spent nothing"
/// before the budget existed (FC-8 monthly series).
fn monthly_series(
	classification: &Classification,
	period: Period,
	totals: &BTreeMap<YearMonth, i64>,
) -> Result<Vec<(YearMonth, Money)>, CoreError> {
	let Some(first_row) = classification
		.rows()
		.first()
		.map(|row| row.date.year_month())
	else {
		return Ok(Vec::new());
	};
	let mut month = first_row.max(period.first().year_month());
	let last = period.last().year_month();
	let mut series = Vec::new();
	while month <= last {
		let total = totals.get(&month).copied().unwrap_or(0);
		series.push((month, Money::from_minor(total)));
		month = month.offset(1)?;
	}
	Ok(series)
}

/// A discretionary row breaks a no-spend day: spending that isn't a bill or
/// part of a loan payment (FC-8).
fn no_spend(
	budget: &BudgetData,
	classification: &Classification,
	period: Period,
	current_year: bool,
) -> Result<NoSpend, CoreError> {
	let empty = NoSpend {
		evaluated: None,
		days: 0,
		longest: None,
		current: None,
	};
	let Some(first_row) = classification.rows().first().map(|row| row.date) else {
		return Ok(empty);
	};
	let Ok(evaluated) = Period::new(first_row.max(period.first()), period.last()) else {
		return Ok(empty);
	};
	let loans = loan_payment_parents(budget, classification);
	let spent: BTreeSet<CivilDate> = classification
		.within(evaluated)
		.iter()
		.filter(|row| {
			row.is_primary()
				&& row.kind() == TransactionKind::Expense
				&& row.amount.is_negative()
				&& !bill_linked(budget, row)
				&& !row
					.parent
					.as_ref()
					.is_some_and(|parent| loans.contains(parent))
		})
		.map(|row| row.date)
		.collect();

	let mut days = 0u32;
	let mut longest: Option<Streak> = None;
	let mut run: Option<Streak> = None;
	let mut date = evaluated.first();
	loop {
		if spent.contains(&date) {
			run = None;
		} else {
			days += 1;
			let streak = run.map_or(
				Streak {
					start: date,
					days: 1,
				},
				|streak| Streak {
					days: streak.days + 1,
					..streak
				},
			);
			if longest.is_none_or(|best| streak.days > best.days) {
				longest = Some(streak);
			}
			run = Some(streak);
		}
		match date.naive().succ_opt().map(CivilDate::from) {
			Some(next) if next <= evaluated.last() => date = next,
			_ => break,
		}
	}
	Ok(NoSpend {
		evaluated: Some(evaluated),
		days,
		longest,
		current: current_year.then(|| run.map_or(0, |streak| streak.days)),
	})
}

/// Largest remainder to `whole` (FC-8 redaction): floors first, then each
/// missing unit to the largest remainder, ties to the earlier entry. `None`
/// when the amounts don't sum to a positive total or one is negative.
fn largest_remainder(amounts: &[i64], whole: i64) -> Option<Vec<i64>> {
	if amounts.iter().any(|amount| *amount < 0) {
		return None;
	}
	let total: i128 = amounts.iter().map(|amount| i128::from(*amount)).sum();
	if total <= 0 {
		return None;
	}
	let whole = i128::from(whole);
	let scaled: Vec<i128> = amounts
		.iter()
		.map(|amount| i128::from(*amount) * whole)
		.collect();
	let mut shares: Vec<i128> = scaled.iter().map(|value| value / total).collect();
	let mut order: Vec<usize> = (0..amounts.len()).collect();
	order.sort_by_key(|&index| {
		(
			std::cmp::Reverse(scaled.get(index).map_or(0, |value| value % total)),
			index,
		)
	});
	let missing = whole - shares.iter().sum::<i128>();
	for &index in order.iter().take(usize::try_from(missing).ok()?) {
		if let Some(share) = shares.get_mut(index) {
			*share += 1;
		}
	}
	shares
		.into_iter()
		.map(|share| i64::try_from(share).ok())
		.collect()
}

/// Rows that are one part of a loan payment (DEBT-6): split children whose
/// parent also has a `debt_principal` child.
fn loan_payment_parents(
	budget: &BudgetData,
	classification: &Classification,
) -> BTreeSet<TransactionId> {
	classification
		.rows()
		.iter()
		.filter(|row| row.kind() == TransactionKind::DebtPrincipal && row.is_primary())
		.filter_map(|row| row.parent.clone())
		.filter(|parent| {
			budget
				.transactions
				.get(parent)
				.is_some_and(|parent| parent.split == SplitRole::Parent)
		})
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn w_6_redaction_shares_include_everything_else() {
		let set = ["Groceries", "Dining", "Fuel"]
			.into_iter()
			.zip([43000, 30000, 17000])
			.map(|(id, amount)| (Some(id), Money::from_minor(amount)))
			.collect();
		let shares = shares(set, Money::from_minor(10000));
		assert_eq!(
			shares,
			[
				(Some("Groceries"), 430),
				(Some("Dining"), 300),
				(Some("Fuel"), 170),
				(None, 100)
			]
		);
	}

	#[test]
	fn largest_remainder_breaks_ties_to_the_earlier_row_w_7() {
		assert_eq!(
			largest_remainder(&[1, 1, 1], 1000),
			Some(vec![334, 333, 333])
		);
		assert_eq!(largest_remainder(&[0, 0], 1000), None);
	}

	#[test]
	fn w_9_prior_year_change_in_basis_points() {
		let change = change(Money::from_minor(430000), Money::from_minor(400000));
		assert_eq!(change, Change::BasisPoints(750));
		assert_eq!(
			super::change(Money::from_minor(5), Money::ZERO),
			Change::NewThisYear
		);
	}
}
