//! Bills (R-BILL-1 to R-BILL-5) as the session writes them: a schedule, the
//! rule it owns and its `schedules_json_paths` row, kept the way Actual's
//! schedule handlers keep them (`lc/server/schedules/app.ts`), and the
//! postings and next-date moves of a `SchedulePlan`.

use std::collections::BTreeMap;

use veritas_domain::{
	BudgetData, CivilDate, ConditionsOp, CoreError, Money, NextDateReset, Payee, PayeeId,
	PayeeMapping, Row, Rule, RuleId, RuleStage, Schedule, ScheduleId, ScheduleJsonPathsRow,
	ScheduleMeta, ScheduleMetaRow,
};
use veritas_rules::{
	Bill, BudgetPayees, NextDateWrite, RuleBook, SchedulePlan, ScheduleWrite, bill_from_schedule,
	schedule_name,
};

/// What `metadata.json`'s `lastScheduleRun` should become once a write commits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleRun {
	/// Today's service run is done.
	Done(CivilDate),
	/// Let the service run again today (`allowAutoPostingToRunAgainToday`).
	Clear,
}

/// A plan as rows, with what it leaves to Actual.
#[derive(Clone, Debug, Default)]
pub struct PlannedBills {
	pub rows: Vec<Row>,
	pub schedule_run: Option<ScheduleRun>,
	pub left: Vec<veritas_rules::LeftBill>,
}

fn invalid(message: impl Into<String>) -> CoreError {
	CoreError::Invalid(message.into())
}

fn live_schedule<'a>(data: &'a BudgetData, id: &ScheduleId) -> Result<&'a Schedule, CoreError> {
	data.schedules
		.get(id)
		.filter(|schedule| !schedule.tombstone)
		.ok_or_else(|| CoreError::NotFound {
			entity: "bill",
			id: id.to_string(),
		})
}

fn new_payee_rows(created: &[(PayeeId, String)]) -> Vec<Row> {
	created
		.iter()
		.flat_map(|(id, name)| {
			[
				Row::Payee(Payee {
					id: id.clone(),
					name: name.clone(),
					transfer_account: None,
					favorite: false,
					learn_categories: true,
					tombstone: false,
				}),
				Row::PayeeMapping(PayeeMapping {
					from: id.clone(),
					to: id.clone(),
				}),
			]
		})
		.collect()
}

/// The rows a plan writes, in its order: the payees its rules made, each
/// post with its transfer leg, then the schedule writes. `lastScheduleRun`
/// changes come back apart, since they live outside SQLite.
pub fn plan_rows(
	data: &BudgetData,
	plan: SchedulePlan,
	created: &[(PayeeId, String)],
	book: &RuleBook,
	now_ms: f64,
) -> Result<PlannedBills, CoreError> {
	let mut rows = new_payee_rows(created);
	let with_payees = crate::writes::applied(data, &rows);
	for posting in &plan.posts {
		rows.extend(
			crate::transactions::post(&with_payees, &posting.transaction, book, now_ms)?.rows,
		);
	}
	let mut schedules: BTreeMap<ScheduleId, Schedule> = BTreeMap::new();
	let mut schedule_run = None;
	for write in plan.writes {
		let current = |id: &ScheduleId| -> Result<Schedule, CoreError> {
			match schedules.get(id) {
				Some(schedule) => Ok(schedule.clone()),
				None => live_schedule(data, id).cloned(),
			}
		};
		let changed = match write {
			ScheduleWrite::NextDate {
				schedule,
				write: NextDateWrite::Local(date),
			} => Schedule {
				next_date: Some(date),
				..current(&schedule)?
			},
			ScheduleWrite::NextDate {
				schedule,
				write: NextDateWrite::Reset(date),
			} => {
				let mut after = current(&schedule)?;
				after.next_date = Some(date);
				rows.push(Row::NextDateReset(NextDateReset {
					schedule: schedule.clone(),
					date: Some(date),
				}));
				schedules.insert(schedule, after);
				continue;
			}
			ScheduleWrite::Complete(schedule) => Schedule {
				completed: true,
				..current(&schedule)?
			},
			ScheduleWrite::Restart(schedule) => Schedule {
				completed: false,
				..current(&schedule)?
			},
			ScheduleWrite::ScheduleRunDone(day) => {
				schedule_run = Some(ScheduleRun::Done(day));
				continue;
			}
			ScheduleWrite::ClearScheduleRun => {
				schedule_run = Some(ScheduleRun::Clear);
				continue;
			}
		};
		rows.push(Row::Schedule(changed.clone()));
		schedules.insert(changed.id.clone(), changed);
	}
	Ok(PlannedBills {
		rows,
		schedule_run,
		left: plan.left,
	})
}

fn payees(data: &BudgetData) -> BudgetPayees<'_> {
	BudgetPayees::new(data, || PayeeId::new(veritas_store::new_id()))
}

/// Checks the ids a bill points at, then R-BILL-4's own checks.
fn check(data: &BudgetData, bill: &Bill, today: CivilDate) -> Result<(), CoreError> {
	if let Some(payee) = &bill.payee {
		let live = data
			.resolve_payee(payee)
			.and_then(|id| data.payees.get(id))
			.is_some_and(|row| !row.tombstone);
		if !live {
			return Err(CoreError::NotFound {
				entity: "payee",
				id: payee.to_string(),
			});
		}
	}
	if let Some(account) = &bill.account
		&& !data.accounts.get(account).is_some_and(|row| !row.tombstone)
	{
		return Err(CoreError::NotFound {
			entity: "account",
			id: account.to_string(),
		});
	}
	if let Some(category) = &bill.category
		&& !data
			.categories
			.get(category)
			.is_some_and(|row| !row.tombstone)
	{
		return Err(CoreError::NotFound {
			entity: "category",
			id: category.to_string(),
		});
	}
	bill.validate(today)
}

/// `createSchedule` for a bill (`app.ts:336-392`): the rule, then the
/// schedule with its first next date, then the JSON paths `onRuleUpdate` keeps.
pub fn create_bill(
	data: &BudgetData,
	id: &ScheduleId,
	bill: &Bill,
	today: CivilDate,
) -> Result<Vec<Row>, CoreError> {
	if data.schedules.contains_key(id) {
		return Err(invalid(format!("A bill with id {id} already exists")));
	}
	check(data, bill, today)?;
	let name = schedule_name(bill.name.as_deref(), None, data)?;
	let written = bill.new_rule(id);
	let rule = RuleId::new(veritas_store::new_id());
	Ok(vec![
		Row::Rule(Rule {
			id: rule.clone(),
			stage: RuleStage::Default,
			conditions_op: ConditionsOp::And,
			conditions_json: written.conditions_json,
			actions_json: written.actions_json,
			tombstone: false,
		}),
		Row::Schedule(Schedule {
			id: id.clone(),
			name,
			rule,
			posts_transaction: bill.posts_transaction,
			completed: false,
			next_date: bill.next_date(today)?,
			tombstone: false,
		}),
		Row::ScheduleJsonPaths(ScheduleJsonPathsRow {
			schedule: id.clone(),
			paths: written.json_paths,
		}),
	])
}

/// `updateSchedule` with the bill's conditions (`app.ts:394-492`): only the
/// four schedule conditions and the plain amount and category actions
/// change, and the next date resets only when the date or account did (D65).
pub fn edit_bill(
	data: &BudgetData,
	id: &ScheduleId,
	bill: &Bill,
	today: CivilDate,
) -> Result<Vec<Row>, CoreError> {
	let schedule = live_schedule(data, id)?;
	let rule = data
		.rules
		.get(&schedule.rule)
		.filter(|rule| !rule.tombstone)
		.ok_or_else(|| invalid("This bill's rule is missing, so edit it in Actual"))?;
	if bill_from_schedule(schedule, rule, data).is_none() {
		return Err(invalid(
			"This bill uses a schedule shape Veritas can't show, so edit it in Actual",
		));
	}
	check(data, bill, today)?;
	let name = schedule_name(bill.name.as_deref(), Some(id), data)?;
	let edited = bill.edit_rule(rule, today)?;
	let mut rows = vec![Row::Rule(Rule {
		conditions_json: edited.conditions_json,
		actions_json: edited
			.actions_json
			.unwrap_or_else(|| rule.actions_json.clone()),
		..rule.clone()
	})];
	if data.schedule_json_paths.get(id) != Some(&edited.json_paths) {
		rows.push(Row::ScheduleJsonPaths(ScheduleJsonPathsRow {
			schedule: id.clone(),
			paths: edited.json_paths,
		}));
	}
	if let Some(Some(next)) = edited.reset_next_date
		&& Some(next) != schedule.next_date
	{
		rows.push(Row::NextDateReset(NextDateReset {
			schedule: id.clone(),
			date: Some(next),
		}));
	}
	rows.push(Row::Schedule(Schedule {
		name,
		posts_transaction: bill.posts_transaction,
		next_date: rows
			.iter()
			.find_map(|row| match row {
				Row::NextDateReset(reset) => Some(reset.date),
				_ => None,
			})
			.unwrap_or(schedule.next_date),
		..schedule.clone()
	}));
	Ok(rows)
}

fn label(id: &ScheduleId, paused: bool) -> Row {
	Row::ScheduleMeta(ScheduleMetaRow {
		schedule: id.clone(),
		meta: ScheduleMeta { paused },
	})
}

/// Pause or End: both are Actual's Complete. `paused` only picks the label.
pub fn end_bill(data: &BudgetData, id: &ScheduleId, paused: bool) -> Result<Vec<Row>, CoreError> {
	let schedule = live_schedule(data, id)?;
	let mut rows = if schedule.completed {
		Vec::new()
	} else {
		plan_rows(
			data,
			veritas_rules::complete(data, id)?,
			&[],
			&RuleBook::new(data),
			0.0,
		)?
		.rows
	};
	let labelled = data.schedule_meta.get(id).map(|meta| meta.paused);
	if labelled != Some(paused) && (paused || labelled.is_some()) {
		rows.push(label(id, paused));
	}
	Ok(rows)
}

/// Resume: Actual's Restart, a reset next date from today.
pub fn resume_bill(
	data: &BudgetData,
	id: &ScheduleId,
	today: CivilDate,
) -> Result<Vec<Row>, CoreError> {
	if !live_schedule(data, id)?.completed {
		return Err(invalid("This bill isn't paused or ended"));
	}
	let mut rows = plan_rows(
		data,
		veritas_rules::restart(data, id, today)?,
		&[],
		&RuleBook::new(data),
		0.0,
	)?
	.rows;
	if data.schedule_meta.get(id).is_some_and(|meta| meta.paused) {
		rows.push(label(id, false));
	}
	Ok(rows)
}

/// Skip one occurrence. A one-time bill has nothing after it.
pub fn skip_bill(data: &BudgetData, id: &ScheduleId) -> Result<Vec<Row>, CoreError> {
	let rows = plan_rows(
		data,
		veritas_rules::skip(data, id)?,
		&[],
		&RuleBook::new(data),
		0.0,
	)?
	.rows;
	if rows.is_empty() {
		return Err(invalid(
			"This bill has no later date to skip to. End it instead.",
		));
	}
	Ok(rows)
}

/// A user's post that met a rule Veritas can't run writes nothing (D60), so
/// the note comes back as the error.
fn posted(planned: PlannedBills) -> Result<Vec<Row>, CoreError> {
	match planned.left.first() {
		Some(left) => Err(invalid(left.note())),
		None => Ok(planned.rows),
	}
}

/// R-BILL-3 Paid, with Ryan's amount and date.
pub fn pay_bill(
	data: &BudgetData,
	id: &ScheduleId,
	amount: Money,
	date: CivilDate,
	today: CivilDate,
	now_ms: f64,
) -> Result<Vec<Row>, CoreError> {
	live_schedule(data, id)?;
	let book = RuleBook::new(data);
	let mut directory = payees(data);
	let plan = veritas_rules::pay(&book, data, &mut directory, id, amount, date, today)?;
	posted(plan_rows(data, plan, &directory.created, &book, now_ms)?)
}

/// "Post transaction" on its next date, or "Post transaction today".
pub fn post_bill_now(
	data: &BudgetData,
	id: &ScheduleId,
	today: Option<CivilDate>,
	now_ms: f64,
) -> Result<Vec<Row>, CoreError> {
	live_schedule(data, id)?;
	let book = RuleBook::new(data);
	let mut directory = payees(data);
	let plan = veritas_rules::post_now(&book, data, &mut directory, id, today)?;
	posted(plan_rows(data, plan, &directory.created, &book, now_ms)?)
}

/// `advanceSchedulesService` for `today`, as rows.
pub fn advance(
	data: &BudgetData,
	today: CivilDate,
	now_ms: f64,
) -> Result<PlannedBills, CoreError> {
	let book = RuleBook::new(data);
	let mut directory = payees(data);
	let plan = veritas_rules::advance_schedules(&book, data, &mut directory, today);
	plan_rows(data, plan, &directory.created, &book, now_ms)
}

/// Whether saving or ending this schedule lets the service run again today,
/// as `allowAutoPostingToRunAgainToday` does for an auto-post bill.
pub fn clears_schedule_run(data: &BudgetData, id: &ScheduleId) -> bool {
	data.schedules
		.get(id)
		.is_some_and(|schedule| schedule.posts_transaction)
}
