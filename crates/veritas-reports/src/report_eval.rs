//! A custom report's numbers, computed the way Actual's report page computes
//! them from its query rows (`createCustomSpreadsheet`, `recalculate`,
//! `filterHiddenItems`, `filterEmptyRows`, `trimIntervals`, `sortData`).

use std::cmp::Ordering;

use chrono::{Datelike, Days, Months, NaiveDate};
use veritas_domain::{BudgetData, CoreError, Money};

use crate::custom_report::{BalanceType, CustomReport, GroupBy, Interval, SortBy};
use crate::report_query::{
	QueryRow, day, normalise, parse_date, report_range, report_rows, unsupported, week_start,
};

/// Which kind of row an Uncategorized entry collects (Actual's
/// `uncategorized_id`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Uncategorized {
	/// "Uncategorized": on-budget rows without a category, not transfers.
	Other,
	/// "Off budget".
	OffBudget,
	/// "Transfers": on-budget transfers without a category.
	Transfer,
	/// The "Uncategorized & Off budget" group.
	All,
}

/// The five balances Actual reports for every row, interval and total.
/// Debts are negative; `net_debts` is negative or zero.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Balances {
	pub total_assets: Money,
	pub total_debts: Money,
	pub net_assets: Money,
	pub net_debts: Money,
	pub total_totals: Money,
}

impl Balances {
	fn from_parts(assets: Money, debts: Money) -> Result<Balances, CoreError> {
		let total = assets.checked_add(debts)?;
		Ok(Balances {
			total_assets: assets,
			total_debts: debts,
			net_assets: total.max(Money::ZERO),
			net_debts: total.min(Money::ZERO),
			total_totals: total,
		})
	}

	pub fn get(&self, balance: BalanceType) -> Money {
		match balance {
			BalanceType::TotalAssets => self.total_assets,
			BalanceType::TotalDebts => self.total_debts,
			BalanceType::NetAssets => self.net_assets,
			BalanceType::NetDebts => self.net_debts,
			BalanceType::TotalTotals | BalanceType::TotalBudgeted => self.total_totals,
		}
	}
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntervalBalances {
	/// `YYYY-MM`, `YYYY` or the `YYYY-MM-DD` first day (Daily, Weekly).
	pub interval: String,
	pub start: String,
	pub end: String,
	pub balances: Balances,
	/// This interval's net minus the previous one's (rows only).
	pub change: Money,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportGroup {
	/// Empty for the Uncategorized entries, as in Actual.
	pub id: String,
	pub name: String,
	pub uncategorized: Option<Uncategorized>,
	pub balances: Balances,
	pub intervals: Vec<IntervalBalances>,
}

/// One interval across the whole report: the per-group stacked amount in
/// the report's balance type, plus the interval's balances.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportInterval {
	pub interval: IntervalBalances,
	/// Group id (or name for the Uncategorized entries) and its amount.
	pub stacked: Vec<(String, Money)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportData {
	pub start: String,
	pub end: String,
	pub groups: Vec<ReportGroup>,
	pub intervals: Vec<ReportInterval>,
	pub totals: Balances,
}

struct Item {
	id: String,
	name: String,
	uncategorized: Option<Uncategorized>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Label {
	Category,
	CategoryGroup,
	Payee,
	Account,
}

impl Label {
	fn of(self, row: &QueryRow) -> Option<String> {
		match self {
			Label::Category => row.category.as_ref().map(ToString::to_string),
			Label::CategoryGroup => row.category_group.as_ref().map(ToString::to_string),
			Label::Payee => row.payee.as_ref().map(ToString::to_string),
			Label::Account => row.account.as_ref().map(ToString::to_string),
		}
	}
}

fn uncategorized(name: &str, kind: Uncategorized) -> Item {
	Item {
		id: String::new(),
		name: name.to_owned(),
		uncategorized: Some(kind),
	}
}

fn category_name(name: &str) -> String {
	if name.eq_ignore_ascii_case("starting balances") {
		"Starting Balances".to_owned()
	} else {
		name.to_owned()
	}
}

/// Actual's `get-categories` (live groups by is_income, sort_order, id, each
/// with its live categories by sort_order, id) followed by the Uncategorized
/// entries, and `getPayees`/`getAccounts` for the other groupings.
fn items(budget: &BudgetData, group_by: GroupBy) -> (Vec<Item>, Label) {
	let mut groups: Vec<_> = budget
		.category_groups
		.values()
		.filter(|group| !group.tombstone)
		.collect();
	groups.sort_by(|a, b| {
		a.is_income
			.cmp(&b.is_income)
			.then_with(|| a.sort_order.total_cmp(&b.sort_order))
			.then_with(|| a.id.cmp(&b.id))
	});
	match group_by {
		GroupBy::Category | GroupBy::Interval | GroupBy::Tag => {
			let mut list = Vec::new();
			for group in &groups {
				let mut categories: Vec<_> = budget
					.categories
					.values()
					.filter(|category| !category.tombstone && category.group == group.id)
					.collect();
				categories.sort_by(|a, b| {
					a.sort_order
						.total_cmp(&b.sort_order)
						.then_with(|| a.id.cmp(&b.id))
				});
				list.extend(categories.into_iter().map(|category| Item {
					id: category.id.to_string(),
					name: category_name(&category.name),
					uncategorized: None,
				}));
			}
			list.push(uncategorized("Uncategorized", Uncategorized::Other));
			list.push(uncategorized("Off budget", Uncategorized::OffBudget));
			list.push(uncategorized("Transfers", Uncategorized::Transfer));
			(list, Label::Category)
		}
		GroupBy::Group | GroupBy::CategoryGroup => {
			let mut list: Vec<Item> = groups
				.iter()
				.map(|group| Item {
					id: group.id.to_string(),
					name: group.name.clone(),
					uncategorized: None,
				})
				.collect();
			list.push(Item {
				id: "uncategorized".to_owned(),
				name: "Uncategorized & Off budget".to_owned(),
				uncategorized: Some(Uncategorized::All),
			});
			(list, Label::CategoryGroup)
		}
		GroupBy::Payee => {
			let live_account = |payee: &veritas_domain::Payee| {
				payee
					.transfer_account
					.as_ref()
					.and_then(|id| budget.accounts.get(id))
					.filter(|account| !account.tombstone)
			};
			let mut payees: Vec<_> = budget
				.payees
				.values()
				.filter(|payee| !payee.tombstone)
				.filter(|payee| payee.transfer_account.is_none() || live_account(payee).is_some())
				.map(|payee| {
					let account = live_account(payee);
					let name =
						account.map_or_else(|| payee.name.clone(), |account| account.name.clone());
					(payee, account, name)
				})
				.collect();
			payees.sort_by(|(a, a_account, a_name), (b, b_account, b_name)| {
				a.transfer_account
					.is_some()
					.cmp(&b.transfer_account.is_some())
					.then_with(|| nocase(a_name).cmp(&nocase(b_name)))
					.then_with(|| {
						a_account
							.map(|account| account.off_budget)
							.cmp(&b_account.map(|account| account.off_budget))
					})
					.then_with(|| {
						let order = |account: Option<&&veritas_domain::Account>| {
							account.map_or(0.0, |account| account.sort_order)
						};
						order(a_account.as_ref()).total_cmp(&order(b_account.as_ref()))
					})
			});
			let list = payees
				.into_iter()
				.map(|(payee, _, name)| Item {
					id: payee.id.to_string(),
					name,
					uncategorized: None,
				})
				.collect();
			(list, Label::Payee)
		}
		GroupBy::Account => {
			let mut accounts: Vec<_> = budget
				.accounts
				.values()
				.filter(|account| !account.tombstone)
				.collect();
			accounts.sort_by(|a, b| {
				a.sort_order
					.total_cmp(&b.sort_order)
					.then_with(|| a.name.cmp(&b.name))
			});
			let list = accounts
				.into_iter()
				.map(|account| Item {
					id: account.id.to_string(),
					name: account.name.clone(),
					uncategorized: None,
				})
				.collect();
			(list, Label::Account)
		}
	}
}

/// SQLite's `COLLATE NOCASE`: ASCII letters only are folded.
fn nocase(text: &str) -> String {
	text.to_ascii_lowercase()
}

fn visible(report: &CustomReport, row: &QueryRow) -> bool {
	(report.show_hidden || (!row.category_hidden && !row.category_group_hidden))
		&& (report.show_off_budget || !row.account_off_budget)
		&& (report.show_uncategorized || row.category.is_some() || row.account_off_budget)
}

/// `filterHiddenItems`: which rows an item may collect.
fn belongs(item: &Item, row: &QueryRow, label: Label, by_category: bool) -> bool {
	if by_category {
		let off_budget = row.account_off_budget;
		let transfer = row.transfer_account.is_some();
		let allowed = if row.category.is_some() && !off_budget {
			item.uncategorized.is_none()
		} else {
			match item.uncategorized {
				Some(Uncategorized::OffBudget) => off_budget,
				Some(Uncategorized::Transfer) => transfer && !off_budget,
				Some(Uncategorized::Other) => !off_budget && !transfer,
				Some(Uncategorized::All) => true,
				None => false,
			}
		};
		if !allowed {
			return false;
		}
		if item.uncategorized.is_some() {
			return true;
		}
	}
	label.of(row).as_deref() == Some(item.id.as_str())
}

fn intervals(
	report: &CustomReport,
	start: &str,
	end: &str,
	first_day_of_week: u32,
) -> Result<Vec<String>, CoreError> {
	let bad = || CoreError::Invalid(format!("bad report dates {start:?} to {end:?}"));
	let first = parse_date(start).ok_or_else(bad)?;
	let last = parse_date(end).ok_or_else(bad)?;
	let mut out = Vec::new();
	match report.interval {
		Interval::Monthly | Interval::Yearly => {
			let yearly = report.interval == Interval::Yearly;
			let key = |date: NaiveDate| {
				if yearly {
					date.format("%Y").to_string()
				} else {
					date.format("%Y-%m").to_string()
				}
			};
			let step = Months::new(if yearly { 12 } else { 1 });
			let mut at = if yearly {
				first
					.with_month(1)
					.and_then(|d| d.with_day(1))
					.ok_or_else(bad)?
			} else {
				first.with_day(1).ok_or_else(bad)?
			};
			let stop = key(last);
			while key(at) < stop {
				out.push(key(at));
				at = at.checked_add_months(step).ok_or_else(bad)?;
			}
			out.push(key(at));
		}
		Interval::Daily => {
			let mut at = first;
			while at < last {
				out.push(day(at));
				at = at.checked_add_days(Days::new(1)).ok_or_else(bad)?;
			}
			out.push(day(at));
		}
		Interval::Weekly => {
			let mut at = week_start(first, first_day_of_week);
			let stop = week_start(last, first_day_of_week);
			while at < stop {
				out.push(day(at));
				at = at.checked_add_days(Days::new(7)).ok_or_else(bad)?;
			}
			out.push(day(at));
		}
	}
	Ok(out)
}

/// `monthUtils.subDays(next, 1)`: the day before the next interval starts.
fn day_before(interval: &str) -> String {
	parse_date(interval)
		.and_then(|date| date.pred_opt())
		.map_or_else(|| interval.to_owned(), day)
}

fn sum(
	rows: &[QueryRow],
	interval: &str,
	keep: impl Fn(&QueryRow) -> bool,
) -> Result<Money, CoreError> {
	Money::checked_sum(
		rows.iter()
			.filter(|row| row.date == interval && keep(row))
			.map(|row| row.amount),
	)
}

/// Evaluates a custom report as of `today`, with the week starting on
/// `first_day_of_week` (0 = Sunday), like Actual's report page.
pub fn evaluate_report(
	budget: &BudgetData,
	report: &CustomReport,
	today: NaiveDate,
	first_day_of_week: u32,
) -> Result<ReportData, CoreError> {
	if report.balance_type == BalanceType::TotalBudgeted {
		return Err(unsupported("the Budgeted balance type".to_owned()));
	}
	if report.group_by == GroupBy::Tag {
		return Err(unsupported("grouping by tag".to_owned()));
	}
	let (start, end) = report_range(report, budget, today, first_day_of_week)?;
	let (mut assets, mut debts) = report_rows(budget, report, &start, &end)?;
	if report.interval == Interval::Weekly {
		for row in assets.iter_mut().chain(debts.iter_mut()) {
			if let Some(date) = parse_date(&row.date) {
				row.date = day(week_start(date, first_day_of_week));
			}
		}
	}
	assets.retain(|row| visible(report, row));
	debts.retain(|row| visible(report, row));
	let (items, label) = items(budget, report.group_by);
	let by_category = matches!(label, Label::Category | Label::CategoryGroup);
	let keys = intervals(report, &start, &end, first_day_of_week)?;
	let bounds = |index: usize, key: &str| {
		let first = if index == 0 {
			start.clone()
		} else {
			key.to_owned()
		};
		let last = keys
			.get(index + 1)
			.map_or_else(|| end.clone(), |next| day_before(next));
		(first, last)
	};
	let balance = report.balance_type;

	let mut totals_assets = Money::ZERO;
	let mut totals_debts = Money::ZERO;
	let mut totals_net_assets = Money::ZERO;
	let mut totals_net_debts = Money::ZERO;
	let mut report_intervals = Vec::new();
	let mut groups: Vec<ReportGroup> = items
		.iter()
		.map(|item| ReportGroup {
			id: item.id.clone(),
			name: item.name.clone(),
			uncategorized: item.uncategorized,
			balances: Balances::default(),
			intervals: Vec::new(),
		})
		.collect();
	for (index, key) in keys.iter().enumerate() {
		let mut interval_assets = Money::ZERO;
		let mut interval_debts = Money::ZERO;
		let mut stacked = Vec::new();
		for (item, group) in items.iter().zip(groups.iter_mut()) {
			let keep = |row: &QueryRow| belongs(item, row, label, by_category);
			let item_assets = sum(&assets, key, keep)?;
			let item_debts = sum(&debts, key, keep)?;
			interval_assets = interval_assets.checked_add(item_assets)?;
			interval_debts = interval_debts.checked_add(item_debts)?;
			let parts = Balances::from_parts(item_assets, item_debts)?;
			let stack = match balance {
				BalanceType::TotalAssets => item_assets,
				BalanceType::TotalDebts => item_debts.checked_neg()?,
				BalanceType::NetAssets => parts.net_assets,
				BalanceType::NetDebts => parts.net_debts.checked_neg()?,
				BalanceType::TotalTotals | BalanceType::TotalBudgeted => parts.total_totals,
			};
			let stack_key = if item.id.is_empty() {
				item.name.clone()
			} else {
				item.id.clone()
			};
			stacked.push((stack_key, stack));
			let previous = group
				.intervals
				.last()
				.map(|last| last.balances.total_totals);
			let (first, last) = bounds(index, key);
			group.intervals.push(IntervalBalances {
				interval: key.clone(),
				start: first,
				end: last,
				balances: parts,
				change: previous.map_or(Ok(Money::ZERO), |previous| {
					parts.total_totals.checked_sub(previous)
				})?,
			});
		}
		let parts = Balances::from_parts(interval_assets, interval_debts)?;
		totals_assets = totals_assets.checked_add(interval_assets)?;
		totals_debts = totals_debts.checked_add(interval_debts)?;
		totals_net_assets = totals_net_assets.checked_add(parts.net_assets)?;
		totals_net_debts = totals_net_debts.checked_add(parts.net_debts)?;
		let (first, last) = bounds(index, key);
		report_intervals.push(ReportInterval {
			interval: IntervalBalances {
				interval: key.clone(),
				start: first,
				end: last,
				balances: parts,
				change: Money::ZERO,
			},
			stacked,
		});
	}
	for group in &mut groups {
		let assets = Money::checked_sum(group.intervals.iter().map(|i| i.balances.total_assets))?;
		let debts = Money::checked_sum(group.intervals.iter().map(|i| i.balances.total_debts))?;
		group.balances = Balances::from_parts(assets, debts)?;
	}

	groups.retain(|group| {
		report.show_empty
			|| match balance {
				BalanceType::TotalTotals | BalanceType::TotalBudgeted => {
					group.balances.total_debts != Money::ZERO
						|| group.balances.total_assets != Money::ZERO
						|| group.balances.total_totals != Money::ZERO
				}
				other => group.balances.get(other) != Money::ZERO,
			}
	});

	if report.trim_intervals {
		let empty = |interval: &IntervalBalances| interval.balances.get(balance) == Money::ZERO;
		let series = groups
			.iter()
			.map(|group| group.intervals.iter().collect::<Vec<_>>())
			.chain(std::iter::once(
				report_intervals.iter().map(|i| &i.interval).collect(),
			));
		let mut first: Option<usize> = None;
		let mut last: Option<usize> = None;
		for intervals in series {
			if let Some(found) = intervals.iter().position(|i| !empty(i)) {
				first = Some(first.map_or(found, |f| f.min(found)));
			}
			if let Some(found) = intervals.iter().rposition(|i| !empty(i)) {
				last = Some(last.map_or(found, |l| l.max(found)));
			}
		}
		let keep = |index: usize| {
			first
				.zip(last)
				.is_some_and(|(f, l)| (f..=l).contains(&index))
		};
		let trim = |len: usize| (0..len).map(keep).collect::<Vec<_>>();
		let mask = trim(report_intervals.len());
		let mut index = 0;
		report_intervals.retain(|_| {
			index += 1;
			mask.get(index - 1).copied().unwrap_or(false)
		});
		for group in &mut groups {
			let mask = trim(group.intervals.len());
			let mut index = 0;
			group.intervals.retain(|_| {
				index += 1;
				mask.get(index - 1).copied().unwrap_or(false)
			});
		}
	}

	let mut sort_by = report.sort_by;
	if matches!(balance, BalanceType::TotalDebts | BalanceType::NetDebts) {
		sort_by = match sort_by {
			SortBy::Asc => SortBy::Desc,
			SortBy::Desc => SortBy::Asc,
			other => other,
		};
	}
	match sort_by {
		SortBy::Asc => groups.sort_by_key(|group| group.balances.get(balance)),
		SortBy::Desc => groups.sort_by_key(|group| std::cmp::Reverse(group.balances.get(balance))),
		SortBy::Name => groups.sort_by(|a, b| locale_compare(&a.name, &b.name)),
		SortBy::Budget => {}
	}

	Ok(ReportData {
		start,
		end,
		groups,
		intervals: report_intervals,
		totals: Balances {
			total_assets: totals_assets,
			total_debts: totals_debts,
			net_assets: totals_net_assets,
			net_debts: totals_net_debts,
			total_totals: totals_assets.checked_add(totals_debts)?,
		},
	})
}

/// An approximation of ICU's root collation, which `localeCompare` uses:
/// letters compare without case or accents first, then lowercase before
/// uppercase.
fn locale_compare(a: &str, b: &str) -> Ordering {
	let primary = |text: &str| normalise(text);
	primary(a).cmp(&primary(b)).then_with(|| b.cmp(a))
}
