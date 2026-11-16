//! The net worth card (`NetWorthCard.tsx`, `spreadsheets/net-worth-spreadsheet.ts`):
//! every open or closed account's running balance over the card's filtered
//! rows, per day, week, month or year, with the earlier leg of a transfer
//! pair moved to the later leg's date so the pair never shows a dip.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{Datelike, Days, Months, NaiveDate};
use veritas_domain::{AccountId, BudgetData, CoreError, Money, Transaction, TransactionId};

use super::summary::{earliest_transaction, latest_transaction};
use super::{TimeRange, calculate_time_range, filtered_rows, meta_filter};
use crate::json::Json;
use crate::report_query::{parse_date, week_start};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetWorthInterval {
	Daily,
	Weekly,
	Monthly,
	Yearly,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetWorthPeriod {
	/// The first day of the period (a week's first day for Weekly).
	pub start: NaiveDate,
	pub net_worth: Money,
	/// The positive balances, and the negative ones as a magnitude.
	pub assets: Money,
	pub debt: Money,
	/// Against the period before, or the balance before the first period.
	pub change: Money,
}

/// One account's area in the stacked mode (`NetWorthGraph.tsx:295-325`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetWorthStack {
	pub account: AccountId,
	pub name: String,
	/// The account's balance at each period.
	pub balances: Vec<Money>,
	/// `chartQual1` to `chartQual9`, as 0 to 8.
	pub color: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetWorthCard {
	pub range: TimeRange,
	pub interval: NetWorthInterval,
	/// The days the balances cover, the first being one period before the
	/// range unless the budget's history starts inside it.
	pub start: NaiveDate,
	pub end: NaiveDate,
	pub periods: Vec<NetWorthPeriod>,
	pub has_negative: bool,
	pub net_worth: Money,
	pub total_change: Money,
	pub lowest: Option<Money>,
	pub highest: Option<Money>,
	/// The accounts with a balance other than zero in some period, in
	/// Actual's account order.
	pub accounts: Vec<AccountId>,
	/// The same accounts as the stacked mode stacks them: by their balances
	/// summed over the periods, smallest first, ties in account order.
	pub stacked: Vec<NetWorthStack>,
}

/// `getColorScale('qualitative')`'s length.
const PALETTE: u8 = 9;

struct AccountBalances {
	starting: Money,
	by_period: BTreeMap<NaiveDate, Money>,
}

fn first_of_month(date: NaiveDate) -> NaiveDate {
	date.with_day(1).unwrap_or(date)
}

fn last_of_month(date: NaiveDate) -> NaiveDate {
	first_of_month(date)
		.checked_add_months(Months::new(1))
		.and_then(|next| next.pred_opt())
		.unwrap_or(date)
}

fn period_of(date: NaiveDate, interval: NetWorthInterval, first_day_of_week: u32) -> NaiveDate {
	match interval {
		NetWorthInterval::Daily => date,
		NetWorthInterval::Weekly => week_start(date, first_day_of_week),
		NetWorthInterval::Monthly => first_of_month(date),
		NetWorthInterval::Yearly => NaiveDate::from_ymd_opt(date.year(), 1, 1).unwrap_or(date),
	}
}

fn next_period(period: NaiveDate, interval: NetWorthInterval) -> Option<NaiveDate> {
	match interval {
		NetWorthInterval::Daily => period.checked_add_days(Days::new(1)),
		NetWorthInterval::Weekly => period.checked_add_days(Days::new(7)),
		NetWorthInterval::Monthly => period.checked_add_months(Months::new(1)),
		NetWorthInterval::Yearly => period.checked_add_months(Months::new(12)),
	}
}

/// `alignInternalTransferDates` and `moveTransferLeg`.
struct Alignment {
	start: NaiveDate,
	end: NaiveDate,
	interval: NetWorthInterval,
	first_day_of_week: u32,
}

impl Alignment {
	fn move_leg(
		&self,
		account: &mut AccountBalances,
		leg: &Transaction,
		later: NaiveDate,
	) -> Result<(), CoreError> {
		let date = leg.date.naive();
		if later < self.start {
			return Ok(());
		}
		if date < self.start {
			account.starting = account.starting.checked_sub(leg.amount)?;
		} else {
			if date > self.end {
				return Ok(());
			}
			let key = period_of(date, self.interval, self.first_day_of_week);
			let Some(balance) = account.by_period.get_mut(&key) else {
				return Ok(());
			};
			*balance = balance.checked_sub(leg.amount)?;
		}
		if later > self.end {
			return Ok(());
		}
		let key = period_of(later, self.interval, self.first_day_of_week);
		let balance = account.by_period.entry(key).or_insert(Money::ZERO);
		*balance = balance.checked_add(leg.amount)?;
		Ok(())
	}

	fn align(
		&self,
		accounts: &mut BTreeMap<&AccountId, AccountBalances>,
		legs: &BTreeMap<&TransactionId, &Transaction>,
	) -> Result<(), CoreError> {
		let mut processed: BTreeSet<&TransactionId> = BTreeSet::new();
		for (id, leg) in legs {
			if processed.contains(id) {
				continue;
			}
			let Some(other) = leg.transfer.as_ref().and_then(|other| legs.get(other)) else {
				continue;
			};
			let paired = other.transfer.as_ref() == Some(&leg.id)
				&& other.account != leg.account
				&& other.amount.checked_add(leg.amount)? == Money::ZERO
				&& accounts.contains_key(&leg.account)
				&& accounts.contains_key(&other.account);
			if !paired {
				continue;
			}
			processed.insert(&leg.id);
			processed.insert(&other.id);
			if leg.date == other.date {
				continue;
			}
			let (earlier, later) = if leg.date < other.date {
				(*leg, *other)
			} else {
				(*other, *leg)
			};
			if let Some(account) = accounts.get_mut(&earlier.account) {
				self.move_leg(account, earlier, later.date.naive())?;
			}
		}
		Ok(())
	}
}

pub fn net_worth_card(
	budget: &BudgetData,
	meta: Option<&Json>,
	today: NaiveDate,
	first_day_of_week: u32,
) -> Result<NetWorthCard, CoreError> {
	let range = calculate_time_range(
		meta.and_then(|meta| meta.get("timeFrame")),
		None,
		Some(latest_transaction(budget).unwrap_or(today)),
		today,
	);
	let interval = match meta
		.and_then(|meta| meta.get("interval"))
		.and_then(Json::as_str)
	{
		Some("Daily") => NetWorthInterval::Daily,
		Some("Weekly") => NetWorthInterval::Weekly,
		Some("Yearly") => NetWorthInterval::Yearly,
		_ => NetWorthInterval::Monthly,
	};
	let bad = |text: &str| CoreError::Invalid(format!("the net worth card's date {text:?}"));
	let range_start = first_of_month(parse_date(&range.start).ok_or_else(|| bad(&range.start))?);
	let mut start = match interval {
		NetWorthInterval::Daily => range_start.pred_opt().ok_or(CoreError::Overflow)?,
		NetWorthInterval::Weekly => week_start(
			range_start.pred_opt().ok_or(CoreError::Overflow)?,
			first_day_of_week,
		),
		NetWorthInterval::Monthly | NetWorthInterval::Yearly => range_start
			.checked_sub_months(Months::new(1))
			.ok_or(CoreError::Overflow)?,
	};
	if let Some(earliest) = earliest_transaction(budget).filter(|date| *date >= range_start) {
		start = match interval {
			NetWorthInterval::Daily => earliest,
			NetWorthInterval::Weekly => week_start(earliest, first_day_of_week),
			NetWorthInterval::Monthly | NetWorthInterval::Yearly => range_start,
		};
	}
	let mut end = last_of_month(parse_date(&range.end).ok_or_else(|| bad(&range.end))?);
	if matches!(interval, NetWorthInterval::Daily | NetWorthInterval::Weekly) {
		end = end.min(today);
	}

	let mut listed: Vec<_> = budget
		.accounts
		.values()
		.filter(|account| !account.tombstone)
		.collect();
	listed.sort_by(|a, b| {
		a.sort_order
			.total_cmp(&b.sort_order)
			.then_with(|| a.name.cmp(&b.name))
	});
	let mut accounts: BTreeMap<&AccountId, AccountBalances> = listed
		.iter()
		.map(|account| {
			(
				&account.id,
				AccountBalances {
					starting: Money::ZERO,
					by_period: BTreeMap::new(),
				},
			)
		})
		.collect();

	let filter = meta_filter(meta)?;
	let mut rows = Vec::new();
	for row in filtered_rows(budget, &filter) {
		let row = row?;
		if accounts.contains_key(&row.account) {
			rows.push(row);
		}
	}
	for row in &rows {
		let date = row.date.naive();
		let Some(account) = accounts.get_mut(&row.account) else {
			continue;
		};
		if date < start {
			account.starting = account.starting.checked_add(row.amount)?;
		} else if date <= end {
			let key = period_of(date, interval, first_day_of_week);
			let balance = account.by_period.entry(key).or_insert(Money::ZERO);
			*balance = balance.checked_add(row.amount)?;
		}
	}

	let transfers: BTreeMap<&TransactionId, &Transaction> = rows
		.iter()
		.filter(|row| row.transfer.is_some())
		.map(|row| (&row.id, *row))
		.collect();
	let mut legs: BTreeMap<&TransactionId, &Transaction> = transfers
		.iter()
		.filter(|(_, row)| row.date.naive() <= end)
		.map(|(id, row)| (*id, *row))
		.collect();
	let counterparts: Vec<&TransactionId> = legs
		.values()
		.filter_map(|leg| leg.transfer.as_ref())
		.filter(|other| !legs.contains_key(other))
		.collect();
	for other in counterparts {
		if let Some((id, row)) = transfers.get_key_value(other) {
			legs.insert(id, row);
		}
	}
	Alignment {
		start,
		end,
		interval,
		first_day_of_week,
	}
	.align(&mut accounts, &legs)?;

	let mut periods_list = Vec::new();
	let mut period = period_of(start, interval, first_day_of_week);
	let last = period_of(end, interval, first_day_of_week);
	while period <= last {
		periods_list.push(period);
		period = next_period(period, interval).ok_or(CoreError::Overflow)?;
	}

	let mut running: Vec<Money> = listed
		.iter()
		.map(|account| accounts[&account.id].starting)
		.collect();
	let mut previous = Money::checked_sum(running.iter().copied())?;
	let mut has_balance = vec![false; listed.len()];
	let mut balances: Vec<Vec<Money>> = vec![Vec::with_capacity(periods_list.len()); listed.len()];
	let mut periods = Vec::with_capacity(periods_list.len());
	for period in periods_list {
		let mut total = Money::ZERO;
		let mut assets = Money::ZERO;
		let mut debt = Money::ZERO;
		for (index, account) in listed.iter().enumerate() {
			if let Some(amount) = accounts[&account.id].by_period.get(&period) {
				running[index] = running[index].checked_add(*amount)?;
			}
			let balance = running[index];
			has_balance[index] |= balance != Money::ZERO;
			balances[index].push(balance);
			if balance.is_negative() {
				debt = debt.checked_sub(balance)?;
			} else {
				assets = assets.checked_add(balance)?;
			}
			total = total.checked_add(balance)?;
		}
		periods.push(NetWorthPeriod {
			start: period,
			net_worth: total,
			assets,
			debt,
			change: total.checked_sub(previous)?,
		});
		previous = total;
	}
	let mut stacked: Vec<(i128, AccountId, String, Vec<Money>)> = listed
		.iter()
		.zip(balances)
		.zip(&has_balance)
		.filter(|(_, has)| **has)
		.map(|((account, balances), _)| {
			let sum = balances
				.iter()
				.map(|balance| i128::from(balance.minor()))
				.sum();
			(sum, account.id.clone(), account.name.clone(), balances)
		})
		.collect();
	stacked.sort_by_key(|(sum, ..)| *sum);
	let stacked = stacked
		.into_iter()
		.zip((0..PALETTE).cycle())
		.map(|((_, account, name, balances), color)| NetWorthStack {
			account,
			name,
			balances,
			color,
		})
		.collect();
	let first = periods
		.first()
		.map_or(Money::ZERO, |period| period.net_worth);
	let net_worth = periods
		.last()
		.map_or(Money::ZERO, |period| period.net_worth);
	Ok(NetWorthCard {
		range,
		interval,
		start,
		end,
		has_negative: periods.iter().any(|period| period.net_worth.is_negative()),
		lowest: periods.iter().map(|period| period.net_worth).min(),
		highest: periods.iter().map(|period| period.net_worth).max(),
		net_worth,
		total_change: net_worth.checked_sub(first)?,
		accounts: listed
			.iter()
			.zip(&has_balance)
			.filter(|(_, has)| **has)
			.map(|(account, _)| account.id.clone())
			.collect(),
		stacked,
		periods,
	})
}
