use std::collections::{BTreeMap, BTreeSet};

use veritas_domain::{
	BudgetData, CategoryGroupId, CategoryId, CivilDate, CoreError, Money, PayeeId, SplitRole,
	TransactionId, TransactionKind, YearMonth,
};
use veritas_reports::{
	CategoryLabels, Classification, ClassifiedRow, Period, net_profit, net_worth,
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
	/// The other categories with a positive net, uncategorized included.
	pub other_categories: Money,
	pub top_groups: Vec<GroupRank>,
	pub top_payees: Vec<PayeeRank>,
	pub other_payees: Money,
	pub biggest_month: Option<(YearMonth, Money)>,
	pub biggest_vs_typical: Option<i64>,
	pub no_spend: NoSpend,
	pub goals_completed: usize,
	pub net_worth_start: Money,
	pub net_worth_end: Money,
	pub net_worth_change: Money,
	pub prior_year: PriorYear,
}

/// R-WRAP-2: shares in tenths of a percent, never an amount.
#[derive(Clone, Debug, PartialEq)]
pub struct Redacted {
	/// Top categories then `None` for everything else; sums to exactly 1000.
	pub categories: Vec<(Option<CategoryId>, i64)>,
	pub payees: Vec<(Option<PayeeId>, i64)>,
	pub out_vs_in: Option<i64>,
	pub net_worth_change: Option<i64>,
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
		let payees = shares(
			self.top_payees
				.iter()
				.map(|rank| (Some(rank.payee.clone()), rank.total))
				.collect(),
			self.other_payees,
		);
		let tenths = |part: Money, whole: Money| {
			(whole > Money::ZERO).then(|| {
				let value = round_half_away(
					i128::from(part.minor()) * i128::from(WHOLE),
					i128::from(whole.minor()),
				);
				i64::try_from(value).unwrap_or(i64::MAX)
			})
		};
		Redacted {
			categories,
			payees,
			out_vs_in: tenths(self.total_out, self.total_in),
			net_worth_change: tenths(self.net_worth_change, self.net_worth_start),
		}
	}
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
fn rank<T>(mut items: Vec<T>, key: impl Fn(&T) -> (Money, &str, String)) -> (Vec<T>, Money) {
	items.retain(|item| key(item).0 > Money::ZERO);
	items.sort_by(|a, b| {
		let (a, b) = (key(a), key(b));
		b.0.cmp(&a.0)
			.then_with(|| a.1.cmp(b.1))
			.then_with(|| a.2.cmp(&b.2))
	});
	let rest = items.split_off(TOP.min(items.len()));
	let other = rest.iter().fold(0i64, |total, item| {
		total.saturating_add(key(item).0.minor())
	});
	(items, Money::from_minor(other))
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

pub fn wrapped(
	budget: &BudgetData,
	classification: &Classification,
	year: i32,
	today: CivilDate,
) -> Result<Wrapped, CoreError> {
	if year > today.year() {
		return Err(CoreError::Invalid(format!(
			"{year} hasn't started yet, so there's nothing to wrap"
		)));
	}
	let first = CivilDate::new(year, 1, 1)?;
	let last = if year == today.year() {
		today
	} else {
		CivilDate::new(year, 12, 31)?
	};
	let period = Period::new(first, last)?;
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
	let (top_categories, mut other_categories) = rank(categories, |rank| {
		(rank.total, rank.name.as_str(), rank.category.to_string())
	});
	if let Some(uncategorized) = report
		.expenses
		.categories
		.iter()
		.find(|line| line.category.is_none())
		.filter(|line| line.total > Money::ZERO)
	{
		other_categories = other_categories.checked_add(uncategorized.total)?;
	}

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
	let (top_groups, _) = rank(groups, |rank| {
		(rank.total, rank.name.as_str(), rank.group.to_string())
	});

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
	});

	let biggest_month = months.iter().fold(
		None,
		|best: Option<(YearMonth, i64)>, (month, total)| match best {
			Some((_, max)) if max >= *total => best,
			_ => Some((*month, *total)),
		},
	);
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

	let day_before = first.naive().pred_opt().map_or(first, CivilDate::from);
	let net_worth_start = net_worth(classification, day_before)?;
	let net_worth_end = net_worth(classification, last)?;

	let prior = Period::new(
		CivilDate::new(year - 1, 1, 1)?,
		same_day_in(year - 1, last)?,
	)?;
	let (previous, _) = totals(budget, classification, prior)?;
	let net = current.total_in.checked_sub(current.total_out)?;
	let previous_net = previous.total_in.checked_sub(previous.total_out)?;

	Ok(Wrapped {
		year,
		period,
		has_data: !rows.is_empty(),
		total_in: current.total_in,
		total_out: current.total_out,
		net,
		debt_paydown: current.debt_paydown,
		savings: current.savings,
		top_categories,
		other_categories,
		top_groups,
		top_payees,
		other_payees,
		biggest_month: biggest_month.map(|(month, total)| (month, Money::from_minor(total))),
		biggest_vs_typical,
		no_spend,
		goals_completed: budget
			.goals
			.values()
			.filter(|goal| goal.reached_month.is_some_and(|month| month.year() == year))
			.count(),
		net_worth_start,
		net_worth_end,
		net_worth_change: to_money(
			i128::from(net_worth_end.minor()) - i128::from(net_worth_start.minor()),
		)?,
		prior_year: PriorYear {
			total_in: change(current.total_in, previous.total_in),
			total_out: change(current.total_out, previous.total_out),
			net: change(net, previous_net),
		},
	})
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
