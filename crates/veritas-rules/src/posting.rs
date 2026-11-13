//! Posting bills and moving them forward, as Actual's schedule service does
//! (reference/actual/packages/loot-core/src/server/schedules/app.ts:589-825).
//! Everything here is a plan: the transactions to add and the schedule
//! writes, in order. The session applies a plan in one command.

use serde_json::Value;
use std::collections::BTreeMap;

use veritas_domain::{
	AccountId, BudgetData, CivilDate, CoreError, Money, PayeeId, RuleId, Schedule, ScheduleId,
};

use crate::action::RuleTransaction;
use crate::bill::{schedule_slots, scheduled_amount};
use crate::condition::{DateValue, Op, parse_date_value};
use crate::run::{PayeeDirectory, RuleBook};
use crate::schedule::{ScheduleStatus, next_date_after, next_date_from, paid_window_start, status};
use crate::trigger::added_transaction;

/// How a new next date is stored in `schedules_next_date`
/// (`setNextDate`, `app.ts:226-283`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NextDateWrite {
	/// `local_next_date = date`, `local_next_date_ts = base_next_date_ts`,
	/// so `v_schedules` reads the local date.
	Local(CivilDate),
	/// `base_next_date = date`, `base_next_date_ts = now`, so the base wins.
	Reset(CivilDate),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScheduleWrite {
	NextDate {
		schedule: ScheduleId,
		write: NextDateWrite,
	},
	/// `completed = 1`.
	Complete(ScheduleId),
	/// `completed = 0`, always followed by a reset next date when it changes.
	Restart(ScheduleId),
	/// `metadata.json` `lastScheduleRun = today`: today's run is done
	/// (`app.ts:860-881`). A plan leaves it out when it leaves a bill to
	/// Actual, so Actual runs the service again on its next open.
	ScheduleRunDone(CivilDate),
	/// Clears `lastScheduleRun`, as `allowAutoPostingToRunAgainToday` does
	/// when an auto-post bill is completed or restarted (`app.ts:318-333`),
	/// so the service runs again today.
	ClearScheduleRun,
}

/// A transaction to add for a schedule, after every one of its rules ran.
/// When the payee is a transfer payee the session also creates the other
/// leg, as `addTransactions` does with `runTransfers` (see `trigger::counterpart`).
#[derive(Clone, Debug, PartialEq)]
pub struct Posting {
	pub schedule: ScheduleId,
	pub transaction: RuleTransaction,
	pub applied: Vec<RuleId>,
}

/// What asked for the posting a bill was left over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeftBy {
	/// The daily service, on an auto-post bill that is due or missed.
	/// Actual's own service posts it on its next open, which is why the plan
	/// then leaves out `ScheduleRunDone`.
	Service,
	/// Ryan marking the bill Paid.
	Paid,
	/// "Post transaction" or "Post transaction today".
	PostNow,
}

/// A bill Veritas leaves to Actual (D60): one of its occurrences met a rule
/// Veritas can't run, so the plan holds none of this bill's posts, writes or
/// new payees from this turn.
#[derive(Clone, Debug, PartialEq)]
pub struct LeftBill {
	pub schedule: ScheduleId,
	pub by: LeftBy,
	/// The occurrence as far as Veritas's rules got, for showing only.
	pub partial: RuleTransaction,
	pub undetermined: Vec<RuleId>,
	/// Why each undetermined rule couldn't run, as noun phrases.
	pub reasons: BTreeMap<RuleId, String>,
}

impl LeftBill {
	/// What the bill screen says about it.
	pub fn note(&self) -> String {
		let reasons: Vec<&str> = self
			.undetermined
			.iter()
			.filter_map(|rule| self.reasons.get(rule).map(String::as_str))
			.collect();
		let rules = match self.undetermined.len() {
			1 => "1 rule".to_owned(),
			n => format!("{n} rules"),
		};
		let why = if reasons.is_empty() {
			String::new()
		} else {
			format!(" ({})", reasons.join("; "))
		};
		// Only the service's own case gets posted by Actual without a hand:
		// a Paid or a Post now on any bill, or on an auto-post bill that isn't
		// due yet, waits for Ryan in Actual.
		match self.by {
			LeftBy::Service => format!(
				"Veritas left this bill for Actual: {rules} couldn't run here{why}. Actual posts it the next time it opens this budget."
			),
			LeftBy::Paid => format!(
				"Veritas didn't record this payment: {rules} couldn't run here{why}. Enter the payment in Actual instead."
			),
			LeftBy::PostNow => format!(
				"Veritas didn't post this bill: {rules} couldn't run here{why}. Use Post transaction on it in Actual instead."
			),
		}
	}
}

/// What to apply, in order. Everything in `posts` and `writes` is safe to
/// apply as it is; `left` only says which bills Actual will post instead.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SchedulePlan {
	pub posts: Vec<Posting>,
	pub writes: Vec<ScheduleWrite>,
	pub left: Vec<LeftBill>,
}

/// What the service reads about one schedule, from its rule the way
/// `v_schedules` and `schedules_json_paths` expose it.
struct Facts<'a> {
	schedule: &'a Schedule,
	payee: Option<PayeeId>,
	account: Option<AccountId>,
	/// `None` when the amount isn't whole cents.
	/// Why Veritas can't hold it when it isn't whole cents or can't be read.
	amount: Result<i64, &'static str>,
	raw_date: Option<Value>,
	date: Option<DateValue>,
	date_op_is: bool,
}

impl Facts<'_> {
	fn recurring(&self) -> bool {
		self.raw_date
			.as_ref()
			.and_then(Value::as_object)
			.is_some_and(|config| config.contains_key("frequency"))
	}
}

fn facts<'a>(schedule: &'a Schedule, budget: &BudgetData) -> Option<Facts<'a>> {
	let rule = budget.rules.get(&schedule.rule)?;
	let conditions: Vec<Value> = serde_json::from_str(&rule.conditions_json).ok()?;
	let slots = schedule_slots(&conditions);
	let value = |slot: Option<usize>| slot.map(|index| conditions[index]["value"].clone());
	let payee = value(slots.payee)
		.and_then(|v| v.as_str().map(PayeeId::new))
		// `v_schedules._payee` is `payee_mapping.targetId`: no row, no payee.
		.and_then(|id| budget.payee_mapping.get(&id).cloned());
	let account = value(slots.account).and_then(|v| v.as_str().map(AccountId::new));
	let raw_date = value(slots.date).filter(|v| !v.is_null());
	let date = slots.date.and_then(|index| {
		let op = conditions[index]["op"].as_str().and_then(Op::from_stored)?;
		parse_date_value(op, conditions[index].get("value")).ok()
	});
	let date_op_is = conditions
		.iter()
		.find(|condition| condition["field"] == "date")
		.is_some_and(|condition| condition["op"] == "is");
	Some(Facts {
		schedule,
		payee,
		account,
		amount: {
			let raw = value(slots.amount);
			scheduled_amount(raw.as_ref()).ok_or(match raw {
				Some(Value::Number(_)) => "a bill amount that isn't whole cents",
				_ => "a bill amount Veritas can't read",
			})
		},
		raw_date,
		date,
		date_op_is,
	})
}

/// A transaction linked to the schedule, dated on or after the paid window's
/// start (`getHasTransactionsQuery`; there is no upper bound).
fn has_transaction(
	facts: &Facts,
	next: Option<CivilDate>,
	budget: &BudgetData,
	posted: &[Posting],
) -> bool {
	let Some(next) = next else {
		return false;
	};
	let start = paid_window_start(facts.date_op_is, facts.schedule.posts_transaction, next);
	let id = &facts.schedule.id;
	budget
		.transactions
		.values()
		.any(|t| !t.tombstone && t.schedule.as_ref() == Some(id) && t.date >= start)
		|| posted.iter().any(|p| {
			!p.transaction.deleted
				&& p.transaction.schedule.as_ref() == Some(id)
				&& p.transaction.date >= start
		})
}

fn current_status(
	facts: &Facts,
	next: Option<CivilDate>,
	today: CivilDate,
	budget: &BudgetData,
	posted: &[Posting],
) -> ScheduleStatus {
	// Only paid, due and missed matter to the service, so the upcoming window is moot.
	status(
		next,
		false,
		has_transaction(facts, next, budget, posted),
		7,
		today,
	)
}

/// `setNextDate({advance: true})` through `advanceRecurringScheduleFromNextDate`:
/// the next date after the current one, when it moves.
fn advanced(facts: &Facts, next: Option<CivilDate>) -> Option<CivilDate> {
	let moved = next_date_after(facts.date.as_ref()?, next?).ok()??;
	(Some(moved) != next).then_some(moved)
}

struct Planner<'a> {
	book: &'a RuleBook,
	budget: &'a BudgetData,
	payees: &'a mut dyn PayeeDirectory,
	plan: SchedulePlan,
	by: LeftBy,
}

impl Planner<'_> {
	/// `postTransactionForSchedule` through `addTransactions`. `Err` is a
	/// bill this run leaves to Actual.
	fn post(
		&mut self,
		facts: &Facts,
		date: CivilDate,
		amount: Option<Money>,
	) -> Result<(), Box<LeftBill>> {
		let Some(account) = facts.account.clone() else {
			return Ok(());
		};
		let scheduled = facts.amount.map(Money::from_minor);
		let draft = RuleTransaction {
			account: account.clone(),
			date,
			amount: amount.or(scheduled.ok()).unwrap_or(Money::ZERO),
			payee: facts.payee.clone(),
			category: None,
			notes: None,
			imported_payee: None,
			cleared: Some(false),
			reconciled: None,
			schedule: Some(facts.schedule.id.clone()),
			deleted: false,
			splits: Vec::new(),
			is_child: false,
		};
		if let (None, Err(problem)) = (amount, scheduled) {
			let rule = facts.schedule.rule.clone();
			return Err(Box::new(LeftBill {
				schedule: facts.schedule.id.clone(),
				by: self.by,
				partial: draft,
				undetermined: vec![rule.clone()],
				reasons: BTreeMap::from([(rule, problem.to_owned())]),
			}));
		}
		let outcome = self.book.run(&draft, self.budget, self.payees);
		let mut ruled = match outcome.transaction {
			Some(ruled) => added_transaction(ruled, &account),
			None => {
				let mut partial = added_transaction(outcome.partial, &account);
				if let Some(amount) = amount {
					partial.amount = amount;
					partial.date = date;
				}
				return Err(Box::new(LeftBill {
					schedule: facts.schedule.id.clone(),
					by: self.by,
					partial,
					undetermined: outcome.undetermined,
					reasons: outcome.reasons,
				}));
			}
		};
		// Paid: Ryan's amount and date win over any rule action.
		if let Some(amount) = amount {
			ruled.amount = amount;
			ruled.date = date;
		}
		self.plan.posts.push(Posting {
			schedule: facts.schedule.id.clone(),
			transaction: ruled,
			applied: outcome.applied,
		});
		Ok(())
	}

	fn write_next(&mut self, schedule: &ScheduleId, write: NextDateWrite) {
		self.plan.writes.push(ScheduleWrite::NextDate {
			schedule: schedule.clone(),
			write,
		});
	}

	/// One schedule's turn, all or nothing: a bill left to Actual takes back
	/// its posts, writes and new payees from this turn (D60).
	fn advance(&mut self, facts: &Facts, today: CivilDate) {
		let (made, posts, writes) = (
			self.payees.made(),
			self.plan.posts.len(),
			self.plan.writes.len(),
		);
		if let Err(left) = self.advance_turn(facts, today) {
			self.payees.forget_made_after(made);
			self.plan.posts.truncate(posts);
			self.plan.writes.truncate(writes);
			self.plan.left.push(*left);
		}
	}

	/// One schedule's turn in `advanceSchedulesService`.
	fn advance_turn(&mut self, facts: &Facts, today: CivilDate) -> Result<(), Box<LeftBill>> {
		let schedule = facts.schedule;
		let first = current_status(facts, schedule.next_date, today, self.budget, &[]);
		let posts = schedule.posts_transaction && facts.account.is_some();
		let actionable = matches!(
			first,
			ScheduleStatus::Paid | ScheduleStatus::Due | ScheduleStatus::Missed
		);
		if posts && actionable && (first != ScheduleStatus::Paid || facts.recurring()) {
			let mut next = schedule.next_date;
			let mut state = first;
			loop {
				if state == ScheduleStatus::Paid {
					if next == Some(today) {
						break;
					}
				} else {
					let Some(date) = next else {
						break;
					};
					self.post(facts, date, None)?;
					if state == ScheduleStatus::Due || !facts.recurring() {
						break;
					}
				}
				let Some(moved) = advanced(facts, next) else {
					break;
				};
				self.write_next(&schedule.id, NextDateWrite::Local(moved));
				next = Some(moved);
				state = current_status(facts, next, today, self.budget, &self.plan.posts);
				if !matches!(
					state,
					ScheduleStatus::Paid | ScheduleStatus::Due | ScheduleStatus::Missed
				) {
					break;
				}
			}
		} else if first == ScheduleStatus::Paid && facts.raw_date.is_some() {
			if facts.recurring() {
				let moved = facts
					.date
					.as_ref()
					.and_then(|date| next_date_from(date, today).ok().flatten());
				if let Some(moved) = moved.filter(|moved| Some(*moved) != schedule.next_date) {
					self.write_next(&schedule.id, NextDateWrite::Local(moved));
				}
			} else if facts
				.raw_date
				.as_ref()
				.and_then(Value::as_str)
				.is_some_and(|date| date < today.to_string().as_str())
			{
				self.plan
					.writes
					.push(ScheduleWrite::Complete(schedule.id.clone()));
			}
		}
		Ok(())
	}
}

/// `advanceSchedulesService(syncSuccess = true)`, which Actual runs once a day
/// on open (`lastScheduleRun`): post due and missed auto-post bills, catching
/// up missed occurrences one by one, move paid recurring bills forward and
/// complete paid one-time ones.
pub fn advance_schedules(
	book: &RuleBook,
	budget: &BudgetData,
	payees: &mut dyn PayeeDirectory,
	today: CivilDate,
) -> SchedulePlan {
	let mut planner = Planner {
		book,
		budget,
		payees,
		plan: SchedulePlan::default(),
		by: LeftBy::Service,
	};
	for schedule in budget
		.schedules
		.values()
		.filter(|s| !s.tombstone && !s.completed)
	{
		let Some(facts) = facts(schedule, budget) else {
			continue;
		};
		let open = facts
			.account
			.as_ref()
			.and_then(|account| budget.accounts.get(account))
			.is_some_and(|account| !account.closed && !account.tombstone);
		if open {
			planner.advance(&facts, today);
		}
	}
	if planner.plan.left.is_empty() {
		planner
			.plan
			.writes
			.push(ScheduleWrite::ScheduleRunDone(today));
	}
	planner.plan
}

fn schedule_facts<'a>(budget: &'a BudgetData, id: &ScheduleId) -> Result<Facts<'a>, CoreError> {
	let not_found = || CoreError::NotFound {
		entity: "bill",
		id: id.to_string(),
	};
	let schedule = budget
		.schedules
		.get(id)
		.filter(|s| !s.tombstone)
		.ok_or_else(not_found)?;
	facts(schedule, budget).ok_or_else(not_found)
}

/// Skip one occurrence: `setNextDate({advance: true})`. A one-time bill has
/// nothing after it, so nothing changes, as in Actual.
pub fn skip(budget: &BudgetData, id: &ScheduleId) -> Result<SchedulePlan, CoreError> {
	let facts = schedule_facts(budget, id)?;
	let mut plan = SchedulePlan::default();
	let moved = facts
		.date
		.as_ref()
		.zip(facts.schedule.next_date)
		.and_then(|(date, next)| next_date_after(date, next).ok().flatten())
		.filter(|moved| Some(*moved) != facts.schedule.next_date);
	if let Some(moved) = moved {
		plan.writes.push(ScheduleWrite::NextDate {
			schedule: id.clone(),
			write: NextDateWrite::Local(moved),
		});
	}
	Ok(plan)
}

/// End a bill: Actual's Complete only sets `completed`.
pub fn complete(budget: &BudgetData, id: &ScheduleId) -> Result<SchedulePlan, CoreError> {
	let facts = schedule_facts(budget, id)?;
	let mut writes = vec![ScheduleWrite::Complete(id.clone())];
	if facts.schedule.posts_transaction {
		writes.push(ScheduleWrite::ClearScheduleRun);
	}
	Ok(SchedulePlan {
		writes,
		..SchedulePlan::default()
	})
}

/// Resume an ended bill: `completed: false` and a reset next date from today.
pub fn restart(
	budget: &BudgetData,
	id: &ScheduleId,
	today: CivilDate,
) -> Result<SchedulePlan, CoreError> {
	let facts = schedule_facts(budget, id)?;
	let mut writes = vec![ScheduleWrite::Restart(id.clone())];
	let next = facts
		.date
		.as_ref()
		.and_then(|date| next_date_from(date, today).ok().flatten());
	if let Some(next) = next.filter(|next| Some(*next) != facts.schedule.next_date) {
		writes.push(ScheduleWrite::NextDate {
			schedule: id.clone(),
			write: NextDateWrite::Reset(next),
		});
	}
	if facts.schedule.posts_transaction {
		writes.push(ScheduleWrite::ClearScheduleRun);
	}
	Ok(SchedulePlan {
		writes,
		..SchedulePlan::default()
	})
}

/// Actual's "Post transaction" (dated `next_date`) and "Post transaction
/// today" (`dc/components/schedules/index.tsx:48-75`). The next date isn't
/// touched; the daily service moves the bill once it counts as paid.
pub fn post_now(
	book: &RuleBook,
	budget: &BudgetData,
	payees: &mut dyn PayeeDirectory,
	id: &ScheduleId,
	today: Option<CivilDate>,
) -> Result<SchedulePlan, CoreError> {
	let facts = schedule_facts(budget, id)?;
	let date = today
		.or(facts.schedule.next_date)
		.ok_or_else(|| CoreError::Invalid("This bill has no next date".into()))?;
	let mut planner = Planner {
		book,
		budget,
		payees,
		plan: SchedulePlan::default(),
		by: LeftBy::PostNow,
	};
	if let Err(left) = planner.post(&facts, date, None) {
		planner.plan.left.push(*left);
	}
	Ok(planner.plan)
}

/// R-BILL-3 **Paid**: post the bill with the amount and date Ryan confirmed.
/// His amount and date win over any rule action. If the date falls before
/// the window in which Actual counts the occurrence paid (it would show as
/// due again, and an auto-post bill would post twice), Veritas moves the
/// bill on itself: a recurring bill to its next date after this one with a
/// local write, a one-time bill to completed.
pub fn pay(
	book: &RuleBook,
	budget: &BudgetData,
	payees: &mut dyn PayeeDirectory,
	id: &ScheduleId,
	amount: Money,
	date: CivilDate,
) -> Result<SchedulePlan, CoreError> {
	let facts = schedule_facts(budget, id)?;
	if facts.account.is_none() {
		return Err(CoreError::Invalid(
			"Choose an account for this bill before marking it paid".into(),
		));
	}
	let mut planner = Planner {
		book,
		budget,
		payees,
		plan: SchedulePlan::default(),
		by: LeftBy::Paid,
	};
	if let Err(left) = planner.post(&facts, date, Some(amount)) {
		planner.plan.left.push(*left);
		return Ok(planner.plan);
	}
	let next = facts.schedule.next_date;
	let counted = next.is_some_and(|next| {
		date >= paid_window_start(facts.date_op_is, facts.schedule.posts_transaction, next)
	});
	if !counted && next.is_some() {
		if facts.recurring() {
			if let Some(moved) = advanced(&facts, next) {
				planner.write_next(id, NextDateWrite::Local(moved));
			}
		} else {
			planner
				.plan
				.writes
				.push(ScheduleWrite::Complete(id.clone()));
		}
	}
	Ok(planner.plan)
}

/// Whether a payment on `date` counts as paying the bill's next occurrence
/// in Actual, so the bill screen can say what Paid will do.
pub fn counts_as_paid(
	budget: &BudgetData,
	id: &ScheduleId,
	date: CivilDate,
) -> Result<bool, CoreError> {
	let facts = schedule_facts(budget, id)?;
	Ok(facts.schedule.next_date.is_some_and(|next| {
		date >= paid_window_start(facts.date_op_is, facts.schedule.posts_transaction, next)
	}))
}

#[cfg(test)]
mod tests {
	use veritas_domain::{
		Account, BudgetType, ConditionsOp, Rule, RuleStage, Transaction, TransactionId,
	};

	use super::*;
	use crate::bill::{Bill, BillAmount, BillDate, Repeat, RepeatEnd};
	use crate::recur::Frequency;
	use crate::run::BudgetPayees;

	fn day(text: &str) -> CivilDate {
		CivilDate::from(crate::js::strict_date(text).unwrap())
	}

	fn bill(date: BillDate, posts: bool) -> Bill {
		Bill {
			name: Some("Rent".into()),
			payee: None,
			account: Some(AccountId::new("checking")),
			amount: BillAmount::About(Money::from_minor(-150_000)),
			date,
			category: None,
			posts_transaction: posts,
		}
	}

	fn monthly() -> BillDate {
		BillDate::Repeat(Repeat {
			start: day("2026-08-15"),
			interval: 1,
			frequency: Frequency::Monthly,
			patterns: Vec::new(),
			weekend: None,
			end: RepeatEnd::Never,
		})
	}

	fn budget_with(bill: &Bill, next: &str) -> BudgetData {
		let mut budget = BudgetData::empty(BudgetType::Envelope);
		budget.accounts.insert(
			AccountId::new("checking"),
			Account {
				id: AccountId::new("checking"),
				name: "Checking".into(),
				off_budget: false,
				closed: false,
				group: None,
				sort_order: 0.0,
				tombstone: false,
			},
		);
		let id = ScheduleId::new("rent");
		let text = bill.new_rule(&id);
		budget.rules.insert(
			RuleId::new("rule"),
			Rule {
				id: RuleId::new("rule"),
				stage: RuleStage::Default,
				conditions_op: ConditionsOp::And,
				conditions_json: text.conditions_json,
				actions_json: text.actions_json,
				tombstone: false,
			},
		);
		budget.schedules.insert(
			id.clone(),
			Schedule {
				id,
				name: bill.name.clone(),
				rule: RuleId::new("rule"),
				posts_transaction: bill.posts_transaction,
				completed: false,
				next_date: Some(day(next)),
				tombstone: false,
			},
		);
		budget
	}

	fn paid(budget: &BudgetData, amount: i64, date: &str) -> SchedulePlan {
		let book = RuleBook::new(budget);
		let mut payees = BudgetPayees::new(budget, || PayeeId::new("made"));
		pay(
			&book,
			budget,
			&mut payees,
			&ScheduleId::new("rent"),
			Money::from_minor(amount),
			day(date),
		)
		.unwrap()
	}

	#[test]
	fn paid_on_time_leaves_the_bill_for_the_daily_service() {
		// The window for a confirm-first `isapprox` bill opens two days early.
		let budget = budget_with(&bill(monthly(), false), "2026-12-15");
		let plan = paid(&budget, -149_000, "2026-12-13");
		assert_eq!(plan.posts.len(), 1);
		assert!(plan.left.is_empty());
		assert_eq!(
			plan.posts[0].transaction.amount,
			Money::from_minor(-149_000)
		);
		assert_eq!(
			plan.posts[0].transaction.schedule,
			Some(ScheduleId::new("rent"))
		);
		assert_eq!(plan.posts[0].transaction.cleared, Some(false));
		assert!(plan.writes.is_empty());
	}

	#[test]
	fn paid_early_moves_the_bill_on_itself() {
		// R-BILL-3: Actual wouldn't count a payment before the window, so
		// Veritas writes the next date after this occurrence.
		let budget = budget_with(&bill(monthly(), true), "2026-12-15");
		let plan = paid(&budget, -150_000, "2026-12-01");
		assert_eq!(
			plan.writes,
			[ScheduleWrite::NextDate {
				schedule: ScheduleId::new("rent"),
				write: NextDateWrite::Local(day("2027-01-15"))
			}]
		);
		let once = budget_with(&bill(BillDate::Once(day("2026-12-15")), true), "2026-12-15");
		assert_eq!(
			paid(&once, -150_000, "2026-12-01").writes,
			[ScheduleWrite::Complete(ScheduleId::new("rent"))]
		);
	}

	#[test]
	fn the_service_posts_due_and_missed_bills_and_moves_paid_ones() {
		let budget = budget_with(&bill(monthly(), true), "2026-08-15");
		let book = RuleBook::new(&budget);
		let mut payees = BudgetPayees::new(&budget, || PayeeId::new("made"));
		let plan = advance_schedules(&book, &budget, &mut payees, day("2026-10-15"));
		let dates: Vec<String> = plan
			.posts
			.iter()
			.map(|p| p.transaction.date.to_string())
			.collect();
		assert_eq!(
			dates,
			["2026-08-15", "2026-09-15", "2026-10-15"],
			"missed ones catch up; due today posts and stops"
		);
		assert_eq!(plan.writes.len(), 3);
		assert_eq!(
			plan.writes.last(),
			Some(&ScheduleWrite::ScheduleRunDone(day("2026-10-15")))
		);

		let mut paid_budget = budget_with(&bill(monthly(), false), "2026-10-15");
		paid_budget.transactions.insert(
			TransactionId::new("t"),
			Transaction {
				id: TransactionId::new("t"),
				account: AccountId::new("checking"),
				date: day("2026-10-14"),
				amount: Money::from_minor(-150_000),
				payee: None,
				imported_payee: None,
				category: None,
				notes: None,
				cleared: true,
				reconciled: false,
				split: veritas_domain::SplitRole::None,
				transfer: None,
				schedule: Some(ScheduleId::new("rent")),
				starting_balance: false,
				sort_order: 0.0,
				tombstone: false,
			},
		);
		let book = RuleBook::new(&paid_budget);
		let mut payees = BudgetPayees::new(&paid_budget, || PayeeId::new("made"));
		let plan = advance_schedules(&book, &paid_budget, &mut payees, day("2026-10-20"));
		assert!(plan.posts.is_empty());
		assert_eq!(
			plan.writes,
			[
				ScheduleWrite::NextDate {
					schedule: ScheduleId::new("rent"),
					write: NextDateWrite::Local(day("2026-11-15"))
				},
				ScheduleWrite::ScheduleRunDone(day("2026-10-20"))
			]
		);
	}

	fn add_rule(
		budget: &mut BudgetData,
		id: &str,
		conditions: serde_json::Value,
		actions: serde_json::Value,
	) {
		budget.rules.insert(
			RuleId::new(id),
			Rule {
				id: RuleId::new(id),
				stage: RuleStage::Default,
				conditions_op: ConditionsOp::And,
				conditions_json: conditions.to_string(),
				actions_json: actions.to_string(),
				tombstone: false,
			},
		);
	}

	#[test]
	fn a_bill_that_meets_a_rule_veritas_cannot_run_is_left_whole() {
		// Missed since Aug 15. The Aug 15 post renames to a new payee, and the
		// Sep 15 post meets a template rule, so the whole bill goes to Actual:
		// no posts, no next-date writes past occurrences nobody posted, no new
		// payee, and today's run isn't recorded (D60).
		let mut budget = budget_with(&bill(monthly(), true), "2026-08-15");
		add_rule(
			&mut budget,
			"rename",
			serde_json::json!([{"op": "is", "field": "date", "value": "2026-08-15"}]),
			serde_json::json!([{"op": "set", "field": "payee_name", "value": "New Landlord"}]),
		);
		add_rule(
			&mut budget,
			"template",
			serde_json::json!([{"op": "is", "field": "date", "value": "2026-09-15"}]),
			serde_json::json!([{"op": "set", "field": "notes", "value": "", "options": {"template": "{{date}}"}}]),
		);
		let book = RuleBook::new(&budget);
		let mut payees = BudgetPayees::new(&budget, || PayeeId::new("made"));
		let plan = advance_schedules(&book, &budget, &mut payees, day("2026-10-15"));
		assert!(plan.posts.is_empty());
		assert!(plan.writes.is_empty());
		assert!(
			payees.created.is_empty(),
			"the payee the Aug 15 post made goes too"
		);
		assert_eq!(plan.left.len(), 1);
		let left = &plan.left[0];
		assert_eq!(left.schedule, ScheduleId::new("rent"));
		assert_eq!(left.partial.date, day("2026-09-15"));
		assert_eq!(left.undetermined, [RuleId::new("template")]);
		assert_eq!(
			left.note(),
			"Veritas left this bill for Actual: 1 rule couldn't run here (a template action). Actual posts it the next time it opens this budget."
		);
	}

	#[test]
	fn the_note_for_a_left_bill_says_what_happens_next() {
		// A Paid or Post now that meets a rule Veritas can't run isn't posted by
		// Actual on its own, unlike the daily service's case.
		let mut budget = budget_with(&bill(monthly(), false), "2026-12-15");
		add_rule(
			&mut budget,
			"template",
			serde_json::json!([{"op": "lt", "field": "amount", "value": 0}]),
			serde_json::json!([{"op": "set", "field": "notes", "value": "", "options": {"template": "{{date}}"}}]),
		);
		let book = RuleBook::new(&budget);
		let id = ScheduleId::new("rent");
		let mut payees = BudgetPayees::new(&budget, || PayeeId::new("made"));
		let paid = pay(
			&book,
			&budget,
			&mut payees,
			&id,
			Money::from_minor(-150_000),
			day("2026-12-14"),
		)
		.unwrap();
		assert!(paid.posts.is_empty() && paid.writes.is_empty());
		assert_eq!(paid.left[0].by, LeftBy::Paid);
		assert_eq!(
			paid.left[0].note(),
			"Veritas didn't record this payment: 1 rule couldn't run here (a template action). Enter the payment in Actual instead."
		);
		let posted = post_now(&book, &budget, &mut payees, &id, None).unwrap();
		assert_eq!(posted.left[0].by, LeftBy::PostNow);
		assert!(
			posted.left[0]
				.note()
				.ends_with("Use Post transaction on it in Actual instead.")
		);
	}

	#[test]
	fn an_amount_veritas_cannot_read_says_so() {
		let mut budget = budget_with(&bill(monthly(), true), "2026-10-15");
		let rule = budget.rules.get_mut(&RuleId::new("rule")).unwrap();
		rule.conditions_json = rule.conditions_json.replace("-150000", "\"lots\"");
		let book = RuleBook::new(&budget);
		let mut payees = BudgetPayees::new(&budget, || PayeeId::new("made"));
		let plan = advance_schedules(&book, &budget, &mut payees, day("2026-10-15"));
		assert_eq!(
			plan.left[0].reasons[&RuleId::new("rule")],
			"a bill amount Veritas can't read"
		);
	}

	#[test]
	fn an_amount_that_is_not_whole_cents_is_left_to_actual() {
		let mut budget = budget_with(&bill(monthly(), true), "2026-10-15");
		let rule = budget.rules.get_mut(&RuleId::new("rule")).unwrap();
		rule.conditions_json = rule.conditions_json.replace("-150000", "-1500.5");
		let book = RuleBook::new(&budget);
		let mut payees = BudgetPayees::new(&budget, || PayeeId::new("made"));
		let plan = advance_schedules(&book, &budget, &mut payees, day("2026-10-15"));
		assert!(plan.posts.is_empty());
		assert_eq!(
			plan.left[0].reasons[&RuleId::new("rule")],
			"a bill amount that isn't whole cents"
		);
	}

	#[test]
	fn completing_or_restarting_an_auto_post_bill_lets_the_service_run_again() {
		// allowAutoPostingToRunAgainToday (app.ts:318-333).
		let budget = budget_with(&bill(monthly(), true), "2026-12-15");
		let id = ScheduleId::new("rent");
		assert_eq!(
			complete(&budget, &id).unwrap().writes,
			[
				ScheduleWrite::Complete(id.clone()),
				ScheduleWrite::ClearScheduleRun
			]
		);
		assert_eq!(
			restart(&budget, &id, day("2026-12-16"))
				.unwrap()
				.writes
				.last(),
			Some(&ScheduleWrite::ClearScheduleRun)
		);
	}

	#[test]
	fn skip_complete_and_restart() {
		let budget = budget_with(&bill(monthly(), false), "2026-12-15");
		let id = ScheduleId::new("rent");
		assert_eq!(
			skip(&budget, &id).unwrap().writes,
			[ScheduleWrite::NextDate {
				schedule: id.clone(),
				write: NextDateWrite::Local(day("2027-01-15"))
			}]
		);
		assert!(
			skip(
				&budget_with(
					&bill(BillDate::Once(day("2026-12-15")), false),
					"2026-12-15"
				),
				&id
			)
			.unwrap()
			.writes
			.is_empty()
		);
		assert_eq!(
			complete(&budget, &id).unwrap().writes,
			[ScheduleWrite::Complete(id.clone())]
		);
		assert_eq!(
			restart(&budget, &id, day("2027-03-02")).unwrap().writes,
			[
				ScheduleWrite::Restart(id.clone()),
				ScheduleWrite::NextDate {
					schedule: id.clone(),
					write: NextDateWrite::Reset(day("2027-03-15"))
				}
			]
		);
		assert!(counts_as_paid(&budget, &id, day("2026-12-13")).unwrap());
		assert!(!counts_as_paid(&budget, &id, day("2026-12-12")).unwrap());
		assert!(matches!(
			skip(&budget, &ScheduleId::new("nope")),
			Err(CoreError::NotFound { .. })
		));
	}
}
