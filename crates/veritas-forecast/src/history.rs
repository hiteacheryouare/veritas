use std::collections::BTreeMap;

use veritas_domain::{
	AccountId, BudgetData, CategoryId, CategoryKind, CivilDate, CoreError, TransactionId,
	TransactionKind, YearMonth,
};
use veritas_reports::{Classification, ClassifiedRow, Period, account_balances};

/// Whether a baseline forecasts money spent or money earned. Both are
/// positive magnitudes: a spending baseline of 43000 is $430 out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Side {
	Spending,
	Income,
}

/// One category's monthly series `x(c, m)` (FC-1): non-bill rows of the
/// category's own kind, as a positive-means-normal magnitude.
#[derive(Clone, Debug)]
pub(crate) struct Series {
	pub category: CategoryId,
	pub side: Side,
	/// The category's first row of its kind, bill-linked rows included.
	pub first: Option<CivilDate>,
	days: BTreeMap<CivilDate, i64>,
	accounts: BTreeMap<(YearMonth, AccountId), i64>,
}

impl Series {
	pub fn first_month(&self) -> Option<YearMonth> {
		self.first.map(CivilDate::year_month)
	}

	pub fn month(&self, month: YearMonth) -> Result<i64, CoreError> {
		let period = Period::month(month);
		self.sum(period.first(), period.last())
	}

	pub fn sum(&self, first: CivilDate, last: CivilDate) -> Result<i64, CoreError> {
		if first > last {
			return Ok(0);
		}
		self.days
			.range(first..=last)
			.try_fold(0i64, |total, (_, x)| total.checked_add(*x))
			.ok_or(CoreError::Overflow)
	}

	/// The account with the largest `|amount|` over `months`, ties to the
	/// lower account sort order (FC-3).
	pub fn account(
		&self,
		budget: &BudgetData,
		months: &[YearMonth],
	) -> Result<Option<AccountId>, CoreError> {
		let mut totals: BTreeMap<&AccountId, i64> = BTreeMap::new();
		for month in months {
			for ((_, account), value) in self
				.accounts
				.range((*month, AccountId::new(""))..)
				.take_while(|((at, _), _)| at == month)
			{
				let total = totals.entry(account).or_default();
				*total = total.checked_add(*value).ok_or(CoreError::Overflow)?;
			}
		}
		let order = |account: &AccountId| {
			budget
				.accounts
				.get(account)
				.map_or(f64::MAX, |account| account.sort_order)
		};
		Ok(totals
			.into_iter()
			.max_by(|(a, x), (b, y)| {
				x.cmp(y)
					.then_with(|| order(b).total_cmp(&order(a)))
					.then_with(|| b.cmp(a))
			})
			.map(|(account, _)| account.clone()))
	}
}

/// Every baselined category's series. Built once per budget; a forecast for
/// any "today" reads it.
#[derive(Clone, Debug)]
pub(crate) struct History {
	pub series: Vec<Series>,
}

impl History {
	pub fn new(budget: &BudgetData, classification: &Classification) -> Result<History, CoreError> {
		let mut series: BTreeMap<CategoryId, Series> = budget
			.categories
			.keys()
			.filter(|id| baselined(budget, id))
			.filter_map(|id| {
				let side = match category_kind(budget, id) {
					CategoryKind::Expense => Side::Spending,
					CategoryKind::Income => Side::Income,
					CategoryKind::DebtPrincipal | CategoryKind::Savings => return None,
				};
				Some((
					id.clone(),
					Series {
						category: id.clone(),
						side,
						first: None,
						days: BTreeMap::new(),
						accounts: BTreeMap::new(),
					},
				))
			})
			.collect();

		for row in classification.rows() {
			let Some(category) = row.category.as_ref().filter(|_| row.is_primary()) else {
				continue;
			};
			let Some(entry) = series.get_mut(category) else {
				continue;
			};
			let x = match (entry.side, row.kind()) {
				(Side::Spending, TransactionKind::Expense) => row.amount.checked_neg()?.minor(),
				(Side::Income, TransactionKind::Income) => row.amount.minor(),
				_ => continue,
			};
			entry.first = Some(entry.first.map_or(row.date, |first| first.min(row.date)));
			if bill_linked(budget, row) {
				continue;
			}
			let day = entry.days.entry(row.date).or_default();
			*day = day.checked_add(x).ok_or(CoreError::Overflow)?;
			let account = entry
				.accounts
				.entry((row.date.year_month(), row.account.clone()))
				.or_default();
			*account = account
				.checked_add(
					row.amount
						.minor()
						.checked_abs()
						.ok_or(CoreError::Overflow)?,
				)
				.ok_or(CoreError::Overflow)?;
		}
		Ok(History {
			series: series.into_values().collect(),
		})
	}
}

/// CLS-3.1's effective kind: Actual's `is_income` wins on the income/expense
/// axis. veritas-reports keeps the same rule private to `classify`.
pub(crate) fn category_kind(budget: &BudgetData, id: &CategoryId) -> CategoryKind {
	let is_income = budget
		.categories
		.get(id)
		.is_some_and(|category| category.is_income);
	let stored = budget.category_meta.get(id).and_then(|meta| meta.kind);
	match (is_income, stored) {
		(true, _) => CategoryKind::Income,
		(false, None | Some(CategoryKind::Income)) => CategoryKind::Expense,
		(false, Some(kind)) => kind,
	}
}

pub(crate) fn hidden(budget: &BudgetData, id: &CategoryId) -> bool {
	budget.categories.get(id).is_none_or(|category| {
		category.hidden
			|| budget
				.category_groups
				.get(&category.group)
				.is_some_and(|group| group.hidden)
	})
}

pub(crate) fn archived(budget: &BudgetData, id: &CategoryId) -> bool {
	budget
		.category_meta
		.get(id)
		.is_some_and(|meta| meta.archived_on.is_some())
}

/// Categories that get a baseline: live, visible and not archived.
pub(crate) fn baselined(budget: &BudgetData, id: &CategoryId) -> bool {
	budget
		.categories
		.get(id)
		.is_some_and(|category| !category.tombstone)
		&& !hidden(budget, id)
		&& !archived(budget, id)
}

/// A row posted from a bill (`transactions.schedule`). A split child counts
/// when its parent is linked, since Actual links the parent.
pub(crate) fn bill_linked(budget: &BudgetData, row: &ClassifiedRow) -> bool {
	let linked = |id: &TransactionId| {
		budget
			.transactions
			.get(id)
			.is_some_and(|transaction| transaction.schedule.is_some())
	};
	linked(&row.id) || row.parent.as_ref().is_some_and(linked)
}

/// ST-2's balance of every account at the end of `on`, from veritas-reports.
pub(crate) fn balances(
	budget: &BudgetData,
	classification: &Classification,
	on: CivilDate,
) -> Result<BTreeMap<AccountId, i64>, CoreError> {
	Ok(account_balances(budget, classification, on)?
		.into_iter()
		.map(|line| (line.account, line.balance.minor()))
		.collect())
}
