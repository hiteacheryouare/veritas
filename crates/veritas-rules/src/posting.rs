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
}

/// A transaction to add for a schedule, after its rules ran. When the payee
/// is a transfer payee the session also creates the other leg, as
/// `addTransactions` does with `runTransfers` (see `trigger::counterpart`).
#[derive(Clone, Debug, PartialEq)]
pub struct Posting {
	pub schedule: ScheduleId,
	/// The transaction to add. `None` when one of its rules is undetermined:
	/// then Veritas doesn't know what Actual would post, so this bill is
	/// left to Actual (see `SchedulePlan::left_to_actual`).
	pub transaction: Option<RuleTransaction>,
	/// What the rules Veritas could run make of it, for showing and for
	/// planning the rest of the run, never for writing.
	pub partial: RuleTransaction,
	pub applied: Vec<RuleId>,
	pub undetermined: Vec<RuleId>,
	/// Why each undetermined rule couldn't run, for the "may need Actual" note.
	pub reasons: BTreeMap<RuleId, String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SchedulePlan {
	pub posts: Vec<Posting>,
	pub writes: Vec<ScheduleWrite>,
}

impl SchedulePlan {
	/// Bills with a posting Veritas can't finish. The session applies none of
	/// their posts or writes and leaves them for Actual, which posts them the
	/// next time it opens the budget.
	pub fn left_to_actual(&self) -> Vec<&ScheduleId> {
		let mut bills: Vec<&ScheduleId> = self
			.posts
			.iter()
			.filter(|posting| posting.transaction.is_none())
			.map(|posting| &posting.schedule)
			.collect();
		bills.dedup();
		bills
	}
}

/// What the service reads about one schedule, from its rule the way
/// `v_schedules` and `schedules_json_paths` expose it.
struct Facts<'a> {
	schedule: &'a Schedule,
	payee: Option<PayeeId>,
	account: Option<AccountId>,
	amount: i64,
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
		.and_then(|id| {
			budget
				.payee_mapping
				.get(&id)
				.cloned()
				.or_else(|| budget.payees.contains_key(&id).then_some(id))
		});
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
		amount: scheduled_amount(value(slots.amount).as_ref()),
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
			!p.partial.deleted && p.partial.schedule.as_ref() == Some(id) && p.partial.date >= start
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
}

impl Planner<'_> {
	/// `postTransactionForSchedule` through `addTransactions`.
	fn post(&mut self, facts: &Facts, date: CivilDate, amount: Option<Money>) {
		let Some(account) = facts.account.clone() else {
			return;
		};
		let draft = RuleTransaction {
			account: account.clone(),
			date,
			amount: amount.unwrap_or(Money::from_minor(facts.amount)),
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
		let outcome = self.book.run(&draft, self.budget, self.payees);
		self.plan.posts.push(Posting {
			schedule: facts.schedule.id.clone(),
			transaction: outcome
				.transaction
				.map(|ruled| added_transaction(ruled, &account)),
			partial: added_transaction(outcome.partial, &account),
			applied: outcome.applied,
			undetermined: outcome.undetermined,
			reasons: outcome.reasons,
		});
	}

	fn write_next(&mut self, schedule: &ScheduleId, write: NextDateWrite) {
		self.plan.writes.push(ScheduleWrite::NextDate {
			schedule: schedule.clone(),
			write,
		});
	}

	/// One schedule's turn in `advanceSchedulesService`.
	fn advance(&mut self, facts: &Facts, today: CivilDate) {
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
					self.post(facts, date, None);
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
	schedule_facts(budget, id)?;
	Ok(SchedulePlan {
		posts: Vec::new(),
		writes: vec![ScheduleWrite::Complete(id.clone())],
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
	Ok(SchedulePlan {
		posts: Vec::new(),
		writes,
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
	};
	planner.post(&facts, date, None);
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
	};
	planner.post(&facts, date, Some(amount));
	if let Some(posting) = planner.plan.posts.last_mut() {
		for transaction in posting.transaction.iter_mut().chain([&mut posting.partial]) {
			transaction.amount = amount;
			transaction.date = date;
		}
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
		assert_eq!(
			plan.posts[0].transaction.as_ref(),
			Some(&plan.posts[0].partial)
		);
		assert!(plan.left_to_actual().is_empty());
		assert_eq!(plan.posts[0].partial.amount, Money::from_minor(-149_000));
		assert_eq!(
			plan.posts[0].partial.schedule,
			Some(ScheduleId::new("rent"))
		);
		assert_eq!(plan.posts[0].partial.cleared, Some(false));
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
			.map(|p| p.partial.date.to_string())
			.collect();
		assert_eq!(
			dates,
			["2026-08-15", "2026-09-15", "2026-10-15"],
			"missed ones catch up; due today posts and stops"
		);
		assert_eq!(plan.writes.len(), 2);

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
			[ScheduleWrite::NextDate {
				schedule: ScheduleId::new("rent"),
				write: NextDateWrite::Local(day("2026-11-15"))
			}]
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
