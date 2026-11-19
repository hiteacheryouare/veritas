//! Actual's balance forecast, the `forecast/generate` handler
//! (`loot-core/src/server/forecast`): every chosen account's running balance
//! per day over the card's filtered rows, plus each future occurrence of a
//! bill that hasn't posted yet, run through the rules the way Actual
//! simulates it. A tracking budget can instead project its budgeted income
//! less budgeted expenses, month by month.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::NaiveDate;
use serde_json::Value;
use veritas_domain::{
	AccountId, BudgetData, BudgetType, CivilDate, CoreError, Money, PayeeId, Schedule, ScheduleId,
	YearMonth,
};
use veritas_engine::EnvelopeSheet;
use veritas_rules::{
	BudgetPayees, DateValue, JsPattern, Op, RuleBook, RuleTransaction, next_date,
	paid_window_start, parse_date_value, schedule_slots, scheduled_amount,
};

use crate::filter::{FilterObject, Named, ObjectAccount, ObjectCategory, TransactionFilter};
use crate::json::Json;
use crate::report_query::{is_row, parse_date, unsupported};

/// The stand-in account bills without an account forecast into
/// (`FORECAST_UNASSIGNED_ACCOUNT_ID`).
pub const FORECAST_UNASSIGNED_ACCOUNT: &str = "__unassigned_schedule__";
const TRACKING_ACCOUNT: &str = "tracking-budget";
const TRACKING_ACCOUNT_NAME: &str = "Tracking Budget";
const MAX_ITERATIONS: usize = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForecastSource {
	Schedules,
	TrackingBudget,
}

/// `forecast/generate`'s parameters, with the dates the card always sends.
#[derive(Clone, Debug)]
pub struct ForecastRequest<'a> {
	/// `None` is every account.
	pub accounts: Option<Vec<String>>,
	pub conditions: &'a [Json],
	pub any: bool,
	pub start: NaiveDate,
	pub end: NaiveDate,
	pub include_accountless: bool,
	pub source: ForecastSource,
}

/// One bill occurrence on a forecast day (`ForecastTransaction`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForecastBill {
	pub amount: Money,
	/// The payee's name, or the bill's name when it has no live payee.
	pub payee: String,
	pub schedule: ScheduleId,
	pub schedule_name: String,
}

/// One account's balance at the end of one day (`ForecastDataPoint`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForecastPoint {
	pub date: NaiveDate,
	pub balance: Money,
	pub account: String,
	pub account_name: String,
	pub bills: Vec<ForecastBill>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForecastLowest {
	pub date: NaiveDate,
	pub balance: Money,
	pub account: String,
	pub account_name: String,
}

/// `ForecastResult`: the points in date order, each day's accounts in
/// Actual's account order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Forecast {
	pub points: Vec<ForecastPoint>,
	pub lowest: ForecastLowest,
	pub start: NaiveDate,
	pub end: NaiveDate,
}

/// `AccountWithComputedBalance`.
#[derive(Clone, Debug)]
struct ForecastAccount {
	id: String,
	name: String,
	off_budget: bool,
	balance: Money,
}

/// `buildForecastDateContext`: schedules count from today on, unless the
/// whole range is already past.
struct Days {
	start: NaiveDate,
	end: NaiveDate,
	first_forecast: NaiveDate,
}

impl Days {
	fn new(start: NaiveDate, end: NaiveDate, today: NaiveDate) -> Days {
		let first_forecast = if end < today { start } else { start.max(today) };
		Days {
			start,
			end,
			first_forecast,
		}
	}

	fn each(&self) -> impl Iterator<Item = NaiveDate> + '_ {
		self.start.iter_days().take_while(|day| *day <= self.end)
	}
}

fn empty_forecast(days: &Days) -> Forecast {
	Forecast {
		points: Vec::new(),
		lowest: ForecastLowest {
			date: days.start,
			balance: Money::ZERO,
			account: String::new(),
			account_name: String::new(),
		},
		start: days.start,
		end: days.end,
	}
}

/// `db.getAccounts()`: live accounts by sort order, then name.
fn live_accounts(budget: &BudgetData) -> Vec<&veritas_domain::Account> {
	let mut accounts: Vec<_> = budget
		.accounts
		.values()
		.filter(|account| !account.tombstone)
		.collect();
	accounts.sort_by(|a, b| {
		a.sort_order
			.total_cmp(&b.sort_order)
			.then_with(|| a.name.cmp(&b.name))
			.then_with(|| a.id.cmp(&b.id))
	});
	accounts
}

/// What `accounts-get` lists, which the card forecasts when it has no
/// accounts of its own.
pub(crate) fn live_account_ids(budget: &BudgetData) -> Vec<String> {
	live_accounts(budget)
		.into_iter()
		.map(|account| account.id.to_string())
		.collect()
}

/// `getAccounts(ids)`: the live accounts chosen, in Actual's order. Their
/// balances are filled in by the pass over the rows that follows.
fn forecast_accounts(budget: &BudgetData, ids: Option<&[String]>) -> Vec<ForecastAccount> {
	live_accounts(budget)
		.into_iter()
		.filter(|account| ids.is_none_or(|ids| ids.iter().any(|id| id == account.id.as_str())))
		.map(|account| ForecastAccount {
			id: account.id.to_string(),
			name: account.name.clone(),
			off_budget: account.off_budget,
			balance: Money::ZERO,
		})
		.collect()
}

/// The position of a row's account among the forecast's accounts. A
/// forecast has a handful of accounts, so a scan beats hashing each id.
fn account_index(accounts: &[ForecastAccount], account: &AccountId) -> Option<usize> {
	accounts
		.iter()
		.position(|candidate| candidate.id == account.as_str())
}

/// `balance_current`: the sum of every row in each account.
fn add_balances(budget: &BudgetData, accounts: &mut [ForecastAccount]) -> Result<(), CoreError> {
	for row in budget.transactions.values() {
		if let Some(index) = account_index(accounts, &row.account)
			&& is_row(budget, row)
		{
			accounts[index].balance = accounts[index].balance.checked_add(row.amount)?;
		}
	}
	Ok(())
}

/// JS `String(value)` for the values an account condition holds.
fn js_text(value: &Json) -> String {
	match value {
		Json::Null => "null".to_owned(),
		other => other
			.as_str()
			.map_or_else(|| other.stringify(), str::to_owned),
	}
}

/// `matchesAccountCondition` (`forecast-accounts.ts:25-62`), on the stored
/// condition as the card keeps it.
fn matches_account(account: &veritas_domain::Account, condition: &Json) -> Result<bool, CoreError> {
	let value = condition.get("value").unwrap_or(&Json::Null);
	let id = account.id.as_str();
	let name = account.name.to_lowercase();
	let listed = || -> Result<bool, CoreError> {
		match value {
			Json::Array(items) => Ok(items.iter().any(|item| item.as_str() == Some(id))),
			Json::String(text) => Ok(text.contains(id)),
			_ => Err(unsupported(
				"an account condition whose list isn't a list".to_owned(),
			)),
		}
	};
	Ok(match condition.get("op").and_then(Json::as_str) {
		Some("is") => value.as_str() == Some(id),
		Some("isNot") => value.as_str() != Some(id),
		Some("oneOf") => listed()?,
		Some("notOneOf") => !listed()?,
		Some("contains") => name.contains(&js_text(value).to_lowercase()),
		Some("doesNotContain") => !name.contains(&js_text(value).to_lowercase()),
		Some("matches") => match JsPattern::new(&js_text(value)) {
			Ok(pattern) => pattern.test(&name).map_err(|_| {
				unsupported("an account regex on a name JavaScript reads differently".to_owned())
			})?,
			Err(reason) => return Err(unsupported(reason)),
		},
		Some("onBudget") => !account.off_budget,
		Some("offBudget") => account.off_budget,
		_ => false,
	})
}

fn is_account_condition(condition: &Json) -> bool {
	condition.get("field").and_then(Json::as_str) == Some("account")
}

/// `resolveForecastAccounts`: the chosen accounts, narrowed by the account
/// conditions when they alone decide which accounts can match
/// (`getAccountRestrictionMode`).
fn resolve_accounts(
	budget: &BudgetData,
	ids: Option<&[String]>,
	plain: &[&Json],
	any: bool,
) -> Result<Vec<ForecastAccount>, CoreError> {
	let account_conditions: Vec<&Json> = plain
		.iter()
		.copied()
		.filter(|condition| is_account_condition(condition))
		.collect();
	let restricts = !account_conditions.is_empty()
		&& (!any
			|| plain
				.iter()
				.all(|condition| is_account_condition(condition)));
	if !restricts {
		return Ok(forecast_accounts(budget, ids));
	}
	let mut matching = Vec::new();
	for account in live_accounts(budget) {
		let mut results = Vec::new();
		for condition in &account_conditions {
			results.push(matches_account(account, condition)?);
		}
		let passes = if any {
			results.iter().any(|passed| *passed)
		} else {
			results.iter().all(|passed| *passed)
		};
		if passes {
			matching.push(account.id.to_string());
		}
	}
	let narrowed: Vec<String> = match ids {
		Some(ids) => ids
			.iter()
			.filter(|id| matching.contains(id))
			.cloned()
			.collect(),
		None => matching,
	};
	Ok(forecast_accounts(budget, Some(&narrowed)))
}

enum BillDate {
	/// A one-time bill's date; `None` is text JS can't read as a date.
	Once(Option<NaiveDate>),
	Repeats(DateValue),
}

/// `normalizeSchedule`: a live, uncompleted bill read through `v_schedules`.
struct Bill<'a> {
	schedule: &'a Schedule,
	name: String,
	payee: Option<String>,
	account: String,
	amount: Money,
	date: BillDate,
	date_op_is: bool,
}

fn can_not_read_bill(schedule: &Schedule, what: &str) -> CoreError {
	unsupported(format!(
		"the bill {} has {what}",
		schedule.name.as_deref().unwrap_or("with no name")
	))
}

fn read_bill<'a>(
	budget: &BudgetData,
	schedule: &'a Schedule,
) -> Result<Option<Bill<'a>>, CoreError> {
	let conditions: Vec<Value> = budget
		.rules
		.get(&schedule.rule)
		.and_then(|rule| serde_json::from_str(&rule.conditions_json).ok())
		.unwrap_or_default();
	let slots = schedule_slots(&conditions);
	let value = |slot: Option<usize>| {
		slot.and_then(|index| conditions[index].get("value"))
			.filter(|value| !value.is_null())
	};
	let (Some(amount), Some(date)) = (value(slots.amount), value(slots.date)) else {
		return Ok(None);
	};
	let raw_payee = value(slots.payee).and_then(Value::as_str);
	let payee = raw_payee
		.and_then(|id| budget.payee_mapping.get(&PayeeId::new(id)))
		.map(ToString::to_string)
		.or_else(|| raw_payee.map(str::to_owned));
	let account = value(slots.account)
		.and_then(Value::as_str)
		.unwrap_or(FORECAST_UNASSIGNED_ACCOUNT)
		.to_owned();
	let amount = scheduled_amount(Some(amount))
		.map(Money::from_minor)
		.ok_or_else(|| can_not_read_bill(schedule, "an amount that isn't whole cents"))?;
	let date = match date {
		Value::String(text) => BillDate::Once(parse_date(text)),
		recurring => BillDate::Repeats(
			parse_date_value(Op::Is, Some(recurring))
				.map_err(|_| can_not_read_bill(schedule, "a date Veritas can't read"))?,
		),
	};
	let date_op_is = conditions
		.iter()
		.find(|condition| condition.get("field").and_then(Value::as_str) == Some("date"))
		.is_some_and(|condition| condition.get("op").and_then(Value::as_str) == Some("is"));
	Ok(Some(Bill {
		schedule,
		name: schedule
			.name
			.clone()
			.unwrap_or_else(|| "Unknown".to_owned()),
		payee,
		account,
		amount,
		date,
		date_op_is,
	}))
}

/// `getFutureOccurrenceDates`: from the stored next date on, each next date
/// after the last one found, through `end`.
fn occurrence_dates(bill: &Bill, end: NaiveDate) -> Result<Vec<NaiveDate>, CoreError> {
	let value = match &bill.date {
		BillDate::Once(day) => return Ok(day.filter(|day| *day <= end).into_iter().collect()),
		BillDate::Repeats(value) => value,
	};
	let Some(first) = bill.schedule.next_date.map(CivilDate::naive) else {
		return Ok(Vec::new());
	};
	let mut dates = vec![first];
	let mut seen = BTreeSet::from([first]);
	let mut day = first;
	let mut iterations = 0;
	while day <= end && iterations < MAX_ITERATIONS {
		iterations += 1;
		let next = next_date(value, day.into())
			.map_err(|_| can_not_read_bill(bill.schedule, "a repeat Veritas can't expand"))?
			.map(CivilDate::naive);
		let Some(next) = next.filter(|next| *next <= end) else {
			break;
		};
		if seen.contains(&next) {
			day = day.succ_opt().ok_or(CoreError::Overflow)?;
			continue;
		}
		dates.push(next);
		seen.insert(next);
		day = next.succ_opt().ok_or(CoreError::Overflow)?;
	}
	Ok(dates)
}

struct Occurrence {
	transaction: RuleTransaction,
	transfer: bool,
	schedule: ScheduleId,
	schedule_name: String,
}

/// Runs `runRules` the way the forecast calls it, with the stand-in account
/// read as an on-budget account.
struct Simulator<'a> {
	budget: &'a BudgetData,
	book: RuleBook,
	payees: BudgetPayees<'a>,
}

impl Simulator<'_> {
	fn run(&mut self, transaction: &RuleTransaction) -> Result<RuleTransaction, CoreError> {
		let off_budget = if transaction.account.as_str() == FORECAST_UNASSIGNED_ACCOUNT {
			Some(false)
		} else {
			self.budget
				.accounts
				.get(&transaction.account)
				.map(|account| account.off_budget)
		};
		let outcome =
			self.book
				.run_on_account(transaction, self.budget, &mut self.payees, off_budget);
		outcome.transaction.ok_or_else(|| {
			let reasons: Vec<&str> = outcome.reasons.values().map(String::as_str).collect();
			unsupported(format!(
				"a bill run through a rule with {}",
				reasons.join(", ")
			))
		})
	}

	/// The live payee `v_payees` gives, named after its account for a
	/// transfer, or a payee a rule just named.
	fn payee(&self, id: &PayeeId) -> Option<Named> {
		if let Some((id, name)) = self.payees.created.iter().find(|(made, _)| made == id) {
			return Some(Named {
				id: id.to_string(),
				name: name.clone(),
			});
		}
		let payee = self
			.budget
			.payees
			.get(id)
			.filter(|payee| !payee.tombstone)?;
		let name = match &payee.transfer_account {
			None => payee.name.clone(),
			Some(account) => self
				.budget
				.accounts
				.get(account)
				.filter(|account| !account.tombstone)?
				.name
				.clone(),
		};
		Some(Named {
			id: id.to_string(),
			name,
		})
	}
}

fn bill_transaction(bill: &Bill, date: NaiveDate) -> RuleTransaction {
	RuleTransaction {
		account: AccountId::new(bill.account.as_str()),
		date: date.into(),
		amount: bill.amount,
		payee: bill.payee.as_deref().map(PayeeId::new),
		category: None,
		notes: None,
		imported_payee: None,
		cleared: Some(false),
		reconciled: None,
		schedule: Some(bill.schedule.id.clone()),
		deleted: false,
		splits: Vec::new(),
		is_child: false,
	}
}

/// `buildFutureScheduleOccurrences`: each unposted occurrence as the rules
/// leave it, plus the other leg when its payee is a transfer payee.
fn bill_occurrences(
	simulator: &mut Simulator,
	bills: &[Bill],
	end: NaiveDate,
	accounts: &HashMap<&str, &ForecastAccount>,
	include_accountless: bool,
	posted: &HashMap<&ScheduleId, Vec<NaiveDate>>,
) -> Result<Vec<Occurrence>, CoreError> {
	let budget = simulator.budget;
	let rule_account = |id: &AccountId| {
		budget
			.accounts
			.get(id)
			.is_some_and(|account| !account.tombstone)
			|| (include_accountless && id.as_str() == FORECAST_UNASSIGNED_ACCOUNT)
	};
	let mut occurrences = Vec::new();
	for bill in bills {
		for date in occurrence_dates(bill, end)? {
			let start = paid_window_start(
				bill.date_op_is,
				bill.schedule.posts_transaction,
				date.into(),
			)
			.naive();
			let paid = posted
				.get(&bill.schedule.id)
				.is_some_and(|dates| dates.iter().any(|day| *day >= start && *day <= date));
			if paid {
				continue;
			}
			let mut source = simulator.run(&bill_transaction(bill, date))?;
			let transfer_account = if source.account.as_str() == FORECAST_UNASSIGNED_ACCOUNT {
				None
			} else {
				source
					.payee
					.as_ref()
					.and_then(|payee| budget.payees.get(payee))
					.and_then(|payee| payee.transfer_account.clone())
					.filter(|account| *account != source.account)
			};
			let Some(transfer_account) = transfer_account else {
				occurrences.push(Occurrence {
					transaction: source,
					transfer: false,
					schedule: bill.schedule.id.clone(),
					schedule_name: bill.name.clone(),
				});
				continue;
			};
			let reverse = rule_account(&source.account)
				.then(|| {
					budget
						.payees
						.values()
						.filter(|payee| !payee.tombstone)
						.find(|payee| payee.transfer_account.as_ref() == Some(&source.account))
						.map(|payee| payee.id.clone())
				})
				.flatten();
			let mut leg = simulator.run(&RuleTransaction {
				account: transfer_account,
				amount: source.amount.checked_neg()?,
				payee: reverse,
				notes: source.notes.clone(),
				..bill_transaction(bill, date)
			})?;
			let same_side = match (
				accounts.get(source.account.as_str()),
				accounts.get(leg.account.as_str()),
			) {
				(Some(from), Some(to)) => from.off_budget == to.off_budget,
				_ => false,
			};
			if same_side {
				source.category = None;
				leg.category = None;
			}
			for transaction in [source, leg] {
				occurrences.push(Occurrence {
					transaction,
					transfer: true,
					schedule: bill.schedule.id.clone(),
					schedule_name: bill.name.clone(),
				});
			}
		}
	}
	Ok(occurrences)
}

/// `enrichForecastFilterObjects` for one simulated transaction.
fn filter_object(
	simulator: &Simulator,
	occurrence: &Occurrence,
	accounts: &HashMap<&str, &ForecastAccount>,
) -> FilterObject {
	let budget = simulator.budget;
	let transaction = &occurrence.transaction;
	let category = transaction
		.category
		.as_ref()
		.and_then(|id| budget.categories.get(id))
		.filter(|category| !category.tombstone)
		.map(|category| ObjectCategory {
			id: category.id.to_string(),
			name: category.name.clone(),
			group: budget
				.category_groups
				.get(&category.group)
				.filter(|group| !group.tombstone)
				.map(|group| Named {
					id: group.id.to_string(),
					name: group.name.clone(),
				}),
		});
	FilterObject {
		amount: transaction.amount.minor(),
		date: transaction.date.naive(),
		notes: transaction.notes.clone(),
		imported_payee: transaction.imported_payee.clone(),
		cleared: transaction.cleared.unwrap_or(false),
		reconciled: transaction.reconciled.unwrap_or(false),
		transfer: occurrence.transfer,
		account: accounts
			.get(transaction.account.as_str())
			.map(|account| ObjectAccount {
				id: account.id.clone(),
				name: account.name.clone(),
				off_budget: account.off_budget,
			}),
		payee: transaction
			.payee
			.as_ref()
			.and_then(|payee| simulator.payee(payee)),
		category,
	}
}

#[derive(Default)]
struct Posted {
	starting: Money,
	by_day: BTreeMap<NaiveDate, Money>,
}

/// `calculateLowestBalance`: the first day whose accounts sum lowest.
fn lowest_balance(
	points: &[ForecastPoint],
	accounts: &[ForecastAccount],
	start: NaiveDate,
) -> Result<ForecastLowest, CoreError> {
	let mut combined: Vec<(NaiveDate, Money)> = Vec::new();
	for point in points {
		match combined.last_mut() {
			Some((date, total)) if *date == point.date => {
				*total = total.checked_add(point.balance)?
			}
			_ => combined.push((point.date, point.balance)),
		}
	}
	let lowest =
		combined.iter().fold(
			None,
			|lowest: Option<(NaiveDate, Money)>, (date, total)| match lowest {
				Some((_, low)) if *total >= low => lowest,
				_ => Some((*date, *total)),
			},
		);
	let (date, balance) = match lowest {
		Some(found) => found,
		None => (
			start,
			Money::checked_sum(accounts.iter().map(|account| account.balance))?,
		),
	};
	Ok(ForecastLowest {
		date,
		balance,
		account: String::new(),
		account_name: String::new(),
	})
}

/// The month's `total-budget-income` and `total-budgeted` cells of a
/// tracking budget: visible categories, the first income group for income
/// and the visible expense groups for expenses. A month the sheet doesn't
/// have reads as 0.
fn tracking_month(
	budget: &BudgetData,
	sheet: &EnvelopeSheet,
	month: YearMonth,
) -> Result<(Money, Money), CoreError> {
	if !sheet.contains(month) {
		return Ok((Money::ZERO, Money::ZERO));
	}
	let income_group = sheet.income_group();
	let mut income = Money::ZERO;
	let mut expenses = Money::ZERO;
	for (group_id, categories) in sheet.group_order() {
		let Some(group) = budget.category_groups.get(group_id) else {
			continue;
		};
		let counts_as_income = Some(group_id) == income_group;
		if group.is_income && !counts_as_income || !group.is_income && group.hidden {
			continue;
		}
		for id in categories {
			if budget
				.categories
				.get(id)
				.is_some_and(|category| category.hidden)
			{
				continue;
			}
			let budgeted = sheet
				.category(month, id)
				.map_or(Money::ZERO, |cell| cell.budgeted);
			if counts_as_income {
				income = income.checked_add(budgeted)?;
			} else {
				expenses = expenses.checked_add(budgeted)?;
			}
		}
	}
	Ok((income, expenses))
}

pub(crate) fn last_of_month(month: YearMonth) -> Result<NaiveDate, CoreError> {
	month
		.offset(1)?
		.first_day()
		.naive()
		.pred_opt()
		.ok_or(CoreError::Overflow)
}

/// `projectTrackingBudgetForecast`: today's on-budget balance, then each
/// month's budgeted income less budgeted expenses on its last day.
fn tracking_forecast(
	budget: &BudgetData,
	sheet: &EnvelopeSheet,
	days: &Days,
) -> Result<Forecast, CoreError> {
	let mut accounts = forecast_accounts(budget, None);
	add_balances(budget, &mut accounts)?;
	let mut running = Money::checked_sum(
		accounts
			.iter()
			.filter(|account| !account.off_budget)
			.map(|account| account.balance),
	)?;
	let first = CivilDate::from(days.start).year_month();
	let last = CivilDate::from(days.end).year_month();
	let mut points = Vec::new();
	let mut month = first;
	while month <= last {
		let (income, expenses) = tracking_month(budget, sheet, month)?;
		running = running.checked_add(income.checked_sub(expenses)?)?;
		points.push(ForecastPoint {
			date: last_of_month(month)?,
			balance: running,
			account: TRACKING_ACCOUNT.to_owned(),
			account_name: TRACKING_ACCOUNT_NAME.to_owned(),
			bills: Vec::new(),
		});
		month = month.offset(1)?;
	}
	let lowest = points
		.iter()
		.fold(None, |lowest: Option<&ForecastPoint>, point| match lowest {
			Some(low) if point.balance >= low.balance => lowest,
			_ => Some(point),
		})
		.map_or_else(
			|| ForecastLowest {
				date: days.start,
				balance: running,
				account: TRACKING_ACCOUNT.to_owned(),
				account_name: TRACKING_ACCOUNT_NAME.to_owned(),
			},
			|point| ForecastLowest {
				date: point.date,
				balance: point.balance,
				account: point.account.clone(),
				account_name: point.account_name.clone(),
			},
		);
	Ok(Forecast {
		points,
		lowest,
		start: days.start,
		end: days.end,
	})
}

/// `generateForecast` (`server/forecast/app.ts:57-167`) as of `today`.
pub fn generate_forecast(
	budget: &BudgetData,
	sheet: &EnvelopeSheet,
	request: &ForecastRequest,
	today: NaiveDate,
) -> Result<Forecast, CoreError> {
	let days = Days::new(request.start, request.end, today);
	if request.source == ForecastSource::TrackingBudget {
		if budget.budget_type != BudgetType::Tracking {
			return Err(CoreError::Invalid(
				"Tracking budget forecasts require a Tracking Budget file.".to_owned(),
			));
		}
		return tracking_forecast(budget, sheet, &days);
	}

	let filter = TransactionFilter::new(request.conditions, request.any)?;
	let plain: Vec<&Json> = request
		.conditions
		.iter()
		.filter(|condition| {
			condition
				.get("customName")
				.is_none_or(|name| matches!(name, Json::Null))
		})
		.collect();
	if plain
		.iter()
		.any(|condition| condition.get("queryFilter").is_some())
	{
		return Err(unsupported("a filter with a query of its own".to_owned()));
	}
	let mut accounts = resolve_accounts(budget, request.accounts.as_deref(), &plain, request.any)?;
	if accounts.is_empty() {
		return Ok(empty_forecast(&days));
	}
	if request.include_accountless
		&& !accounts
			.iter()
			.any(|account| account.id == FORECAST_UNASSIGNED_ACCOUNT)
	{
		accounts.push(ForecastAccount {
			id: FORECAST_UNASSIGNED_ACCOUNT.to_owned(),
			name: String::new(),
			off_budget: false,
			balance: Money::ZERO,
		});
	}
	let mut posted: Vec<Posted> = accounts.iter().map(|_| Posted::default()).collect();
	let mut paid: HashMap<&ScheduleId, Vec<NaiveDate>> = HashMap::new();
	for row in budget.transactions.values() {
		let Some(index) = account_index(&accounts, &row.account) else {
			continue;
		};
		if !is_row(budget, row) {
			continue;
		}
		accounts[index].balance = accounts[index].balance.checked_add(row.amount)?;
		if !filter.matches(budget, row)? {
			continue;
		}
		let date = row.date.naive();
		if let Some(schedule) = &row.schedule {
			paid.entry(schedule).or_default().push(date);
		}
		let summary = &mut posted[index];
		if date < days.start {
			summary.starting = summary.starting.checked_add(row.amount)?;
		} else if date <= days.end {
			let total = summary.by_day.entry(date).or_insert(Money::ZERO);
			*total = total.checked_add(row.amount)?;
		}
	}
	let by_id: HashMap<&str, &ForecastAccount> = accounts
		.iter()
		.map(|account| (account.id.as_str(), account))
		.collect();

	let mut schedules: Vec<&Schedule> = budget
		.schedules
		.values()
		.filter(|schedule| !schedule.tombstone && !schedule.completed)
		.collect();
	schedules.sort_by(|a, b| a.next_date.cmp(&b.next_date).then_with(|| a.id.cmp(&b.id)));
	let mut bills = Vec::new();
	for schedule in schedules {
		if let Some(bill) = read_bill(budget, schedule)?
			&& (request.include_accountless || bill.account != FORECAST_UNASSIGNED_ACCOUNT)
		{
			bills.push(bill);
		}
	}
	let mut made = 0_usize;
	let mut simulator = Simulator {
		budget,
		book: RuleBook::new(budget),
		payees: BudgetPayees::new(budget, move || {
			made += 1;
			PayeeId::new(format!("forecast-payee-{made}"))
		}),
	};
	let occurrences = bill_occurrences(
		&mut simulator,
		&bills,
		days.end,
		&by_id,
		request.include_accountless,
		&paid,
	)?;

	let mut scheduled: HashMap<(&str, NaiveDate), Vec<ForecastBill>> = HashMap::new();
	for occurrence in &occurrences {
		let transaction = &occurrence.transaction;
		let date = transaction.date.naive();
		let account = transaction.account.as_str();
		if date < days.first_forecast || date > days.end || !by_id.contains_key(account) {
			continue;
		}
		let object = filter_object(&simulator, occurrence, &by_id);
		if !filter.matches_object(&object)? {
			continue;
		}
		scheduled
			.entry((account, date))
			.or_default()
			.push(ForecastBill {
				amount: transaction.amount,
				payee: object
					.payee
					.map_or_else(|| occurrence.schedule_name.clone(), |payee| payee.name),
				schedule: occurrence.schedule.clone(),
				schedule_name: occurrence.schedule_name.clone(),
			});
	}

	let mut running: Vec<Money> = posted.iter().map(|summary| summary.starting).collect();
	let mut points = Vec::new();
	for day in days.each() {
		for ((account, balance), summary) in accounts.iter().zip(running.iter_mut()).zip(&posted) {
			let bills = scheduled
				.remove(&(account.id.as_str(), day))
				.unwrap_or_default();
			let posted_today = summary.by_day.get(&day).copied().unwrap_or(Money::ZERO);
			let billed = Money::checked_sum(bills.iter().map(|bill| bill.amount))?;
			*balance = balance.checked_add(posted_today.checked_add(billed)?)?;
			points.push(ForecastPoint {
				date: day,
				balance: *balance,
				account: account.id.clone(),
				account_name: account.name.clone(),
				bills,
			});
		}
	}
	let lowest = lowest_balance(&points, &accounts, days.start)?;
	Ok(Forecast {
		points,
		lowest,
		start: days.start,
		end: days.end,
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_past_range_counts_bills_from_its_start() {
		let day = |text: &str| NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap();
		let past = Days::new(day("2026-01-01"), day("2026-01-31"), day("2026-11-09"));
		assert_eq!(past.first_forecast, day("2026-01-01"));
		let spanning = Days::new(day("2026-11-01"), day("2026-12-31"), day("2026-11-09"));
		assert_eq!(spanning.first_forecast, day("2026-11-09"));
	}
}
