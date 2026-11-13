//! R-BILL-2, R-BILL-3, R-BILL-4 and R-RULE-1 over a year of household bills,
//! in lockstep with Actual. Each step reads the budget Actual left, asks
//! Veritas for its plan (the daily schedule run on open, or what Ryan did on
//! the Bills screen), does the same thing in Actual, and checks that Actual
//! posted exactly the transactions Veritas planned and left the next dates
//! and completions Veritas wrote.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use chrono::Weekday;
use common::{apply_only, budget_from_dump, dump, oracle_dir, repo_root, scratch_dir, uuid};
use serde_json::{Value, json};
use veritas_domain::{AccountId, BudgetData, CivilDate, Money, PayeeId, ScheduleId};
use veritas_rules::{
	Bill, BillAmount, BillDate, BudgetPayees, Frequency, NextDateWrite, Repeat, RepeatEnd,
	RepeatPattern, RuleBook, RuleTransaction, SchedulePlan, ScheduleWrite, WeekendMove,
	advance_schedules, bill_from_schedule, complete, counterpart, counts_as_paid, pay, post_now,
	restart, skip,
};

const CREATED: &str = "2025-12-28";

const RENT: usize = 0;
const PAYCHECK: usize = 1;
const ELECTRIC: usize = 2;
const INSURANCE: usize = 3;
const PHONE: usize = 4;
const GYM: usize = 5;
const STREAMING: usize = 6;
const HOA: usize = 7;
const REGISTRATION: usize = 8;
const SAVINGS: usize = 9;
const PROPERTY_TAX: usize = 10;
const CONFIRM_FIRST: [usize; 3] = [ELECTRIC, STREAMING, REGISTRATION];

fn day(text: &str) -> CivilDate {
	CivilDate::from(chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap())
}

fn checking() -> String {
	uuid(1, 1)
}

fn savings() -> String {
	uuid(1, 2)
}

fn payee(n: usize) -> String {
	uuid(4, n)
}

const PAYEES: [&str; 11] = [
	"Landlord",
	"Employer",
	"Power Co",
	"Insurer",
	"Phone Co",
	"Gym",
	"Streamer",
	"DMV",
	"HOA",
	"County Assessor",
	"City Power",
];

fn category(n: usize) -> String {
	uuid(3, n)
}

fn schedule(bill: usize) -> ScheduleId {
	ScheduleId::new(uuid(10, bill))
}

fn monthly(
	start: &str,
	patterns: Vec<RepeatPattern>,
	weekend: Option<WeekendMove>,
	end: RepeatEnd,
) -> BillDate {
	BillDate::Repeat(Repeat {
		start: day(start),
		interval: 1,
		frequency: Frequency::Monthly,
		patterns,
		weekend,
		end,
	})
}

fn every(start: &str, interval: u32, frequency: Frequency) -> BillDate {
	BillDate::Repeat(Repeat {
		start: day(start),
		interval,
		frequency,
		patterns: Vec::new(),
		weekend: None,
		end: RepeatEnd::Never,
	})
}

fn bill(name: &str, payee: PayeeId, amount: BillAmount, date: BillDate, posts: bool) -> Bill {
	Bill {
		name: Some(name.to_owned()),
		payee: Some(payee),
		account: Some(AccountId::new(checking())),
		amount,
		date,
		category: None,
		posts_transaction: posts,
	}
}

fn household(savings_transfer_payee: &str) -> Vec<Bill> {
	let p = |n: usize| PayeeId::new(payee(n));
	let exact = |n: i64| BillAmount::Exact(Money::from_minor(n));
	vec![
		bill(
			"Rent",
			p(0),
			exact(-185_000),
			monthly("2026-01-01", vec![], None, RepeatEnd::Never),
			true,
		),
		bill(
			"Paycheck",
			p(1),
			exact(245_000),
			every("2026-01-09", 2, Frequency::Weekly),
			true,
		),
		bill(
			"Electric",
			p(2),
			BillAmount::About(Money::from_minor(-12_000)),
			monthly("2026-01-18", vec![], None, RepeatEnd::Never),
			false,
		),
		bill(
			"Car insurance",
			p(3),
			exact(-9_800),
			monthly(
				"2026-01-15",
				vec![],
				Some(WeekendMove::After),
				RepeatEnd::Never,
			),
			true,
		),
		bill(
			"Phone plan",
			p(4),
			exact(-4_200),
			monthly("2026-01-28", vec![], None, RepeatEnd::AfterOccurrences(6)),
			true,
		),
		bill(
			"Gym",
			p(5),
			BillAmount::Between(Money::from_minor(-4_000), Money::from_minor(-4_500)),
			monthly(
				"2026-01-31",
				vec![RepeatPattern::Day(-1)],
				None,
				RepeatEnd::Never,
			),
			true,
		),
		bill(
			"Streaming",
			p(6),
			exact(-1_599),
			monthly("2026-01-05", vec![], None, RepeatEnd::Never),
			false,
		),
		bill(
			"HOA dues",
			p(8),
			exact(-30_000),
			monthly(
				"2026-01-13",
				vec![RepeatPattern::Weekday(Weekday::Tue, 2)],
				None,
				RepeatEnd::Never,
			),
			true,
		),
		bill(
			"Registration",
			p(7),
			exact(-8_500),
			BillDate::Once(day("2026-06-30")),
			false,
		),
		bill(
			"Savings",
			PayeeId::new(savings_transfer_payee),
			exact(-50_000),
			monthly(
				"2026-01-20",
				vec![],
				Some(WeekendMove::Before),
				RepeatEnd::Never,
			),
			true,
		),
		bill(
			"Property tax",
			p(9),
			exact(-210_000),
			every("2026-04-10", 1, Frequency::Yearly),
			true,
		),
	]
}

fn json_text(text: &str) -> String {
	Value::String(text.to_owned()).to_string()
}

fn date_json(date: &BillDate) -> String {
	match date {
		BillDate::Once(day) => json_text(&day.to_string()),
		BillDate::Repeat(repeat) => repeat.to_json(),
	}
}

fn amount_json(amount: &BillAmount) -> (&'static str, String) {
	match amount {
		BillAmount::Exact(n) => ("is", n.minor().to_string()),
		BillAmount::About(n) => ("isapprox", n.minor().to_string()),
		BillAmount::Between(a, b) => (
			"isbetween",
			format!(r#"{{"num1":{},"num2":{}}}"#, a.minor(), b.minor()),
		),
	}
}

fn create_op(id: &ScheduleId, bill: &Bill) -> String {
	let (amount_op, amount) = amount_json(&bill.amount);
	format!(
		r#"{{"op":"createSchedule","id":{},"name":{},"postsTransaction":{},"payee":{},"account":{},"amount":{amount},"amountOp":"{amount_op}","date":{}}}"#,
		json_text(id.as_str()),
		json_text(bill.name.as_deref().unwrap()),
		bill.posts_transaction,
		json_text(bill.payee.as_ref().unwrap().as_str()),
		json_text(bill.account.as_ref().unwrap().as_str()),
		date_json(&bill.date),
	)
}

/// The conditions a bill edit hands `schedule/update`: the payee, account and
/// date conditions with new values, in the stored key order, and a fresh
/// amount condition (`updateScheduleConditions`, `schedule-edit-utils.ts:11-58`).
/// The fields use the names `Rule.serialize` gives the old conditions, the
/// ones `updateSchedule` compares to decide on a reset (`app.ts:461-477`).
fn editor_save_op(id: &ScheduleId, payee: &str, bill: &Bill) -> String {
	let (amount_op, amount) = amount_json(&bill.amount);
	format!(
		r#"{{"op":"updateSchedule","id":{},"conditions":[{{"op":"is","field":"payee","value":{}}},{{"op":"is","field":"account","value":{}}},{{"op":"isapprox","field":"date","value":{}}},{{"op":"{amount_op}","field":"amount","value":{amount}}}]}}"#,
		json_text(id.as_str()),
		json_text(payee),
		json_text(bill.account.as_ref().unwrap().as_str()),
		date_json(&bill.date),
	)
}

#[derive(Clone, Copy, Debug)]
enum Event {
	/// Opening the budget: Actual's daily schedule run.
	Open,
	/// Paid on the Bills screen with this amount and date.
	Pay(usize, i64, &'static str),
	/// "Post transaction", dated the next date.
	PostNow(usize),
	/// "Post transaction today".
	PostToday(usize),
	Skip(usize),
	Complete(usize),
	Restart(usize),
	NewRent,
	NewPayDay,
	/// The power company renamed itself, and Ryan merged the old payee.
	MergePowerCo,
	/// Utilities folded into Bills.
	DeleteUtilities,
}

use Event::*;

const YEAR: &[(&str, Event)] = &[
	("2026-01-01", Open),
	("2026-01-05", PostNow(STREAMING)),
	("2026-01-05", Open),
	("2026-01-09", Open),
	("2026-01-13", Open),
	("2026-01-16", Open),
	("2026-01-18", Pay(ELECTRIC, -11_873, "2026-01-18")),
	("2026-01-20", Open),
	("2026-01-31", Open),
	("2026-02-03", Skip(STREAMING)),
	("2026-02-06", Open),
	("2026-02-16", Open),
	("2026-02-17", Pay(ELECTRIC, -12_950, "2026-02-17")),
	("2026-02-28", Open),
	("2026-03-07", PostToday(STREAMING)),
	("2026-03-08", Open),
	("2026-03-13", Pay(ELECTRIC, -10_410, "2026-03-13")),
	("2026-03-20", Open),
	("2026-04-01", Open),
	("2026-04-10", Open),
	("2026-04-24", Pay(ELECTRIC, -9_875, "2026-04-22")),
	("2026-04-30", Open),
	("2026-05-03", Complete(GYM)),
	("2026-05-10", PostNow(STREAMING)),
	("2026-05-16", Pay(ELECTRIC, -11_120, "2026-05-16")),
	("2026-05-20", Open),
	("2026-06-01", Open),
	("2026-06-27", Pay(RENT, -185_000, "2026-06-27")),
	("2026-06-29", Pay(REGISTRATION, -8_500, "2026-06-28")),
	("2026-07-01", Open),
	("2026-07-15", NewRent),
	("2026-07-18", Pay(ELECTRIC, -14_210, "2026-07-18")),
	("2026-07-20", Open),
	("2026-08-01", Open),
	("2026-08-26", Open),
	("2026-08-27", Restart(GYM)),
	("2026-09-01", Open),
	("2026-09-10", NewPayDay),
	("2026-09-19", Open),
	("2026-10-01", MergePowerCo),
	("2026-10-02", DeleteUtilities),
	("2026-10-17", Pay(ELECTRIC, -13_300, "2026-10-17")),
	("2026-10-31", Open),
	("2026-11-20", Open),
	("2026-12-31", Open),
];

type Row = (
	String,
	i64,
	i64,
	Option<String>,
	String,
	Option<String>,
	Option<String>,
	i64,
);

fn date_number(date: CivilDate) -> i64 {
	date.to_string().replace('-', "").parse().unwrap()
}

fn row_key(t: &RuleTransaction) -> Row {
	(
		t.schedule
			.as_ref()
			.map(|s| s.to_string())
			.unwrap_or_default(),
		date_number(t.date),
		t.amount.minor(),
		t.payee.as_ref().map(|p| p.to_string()),
		t.account.to_string(),
		t.category.as_ref().map(|c| c.to_string()),
		t.notes.clone(),
		i64::from(t.cleared == Some(true)),
	)
}

fn dump_row_key(row: &Value) -> Row {
	(
		row["schedule"].as_str().unwrap_or_default().to_owned(),
		row["date"].as_i64().unwrap(),
		row["amount"].as_i64().unwrap(),
		row["description"].as_str().map(str::to_owned),
		row["acct"].as_str().unwrap().to_owned(),
		row["category"].as_str().map(str::to_owned),
		row["notes"].as_str().map(str::to_owned),
		row["cleared"].as_i64().unwrap(),
	)
}

fn new_rows<'a>(before: &BudgetData, after: &'a Value) -> Vec<&'a Value> {
	after["tables"]["transactions"]["rows"]
		.as_array()
		.unwrap()
		.iter()
		.filter(|row| row["tombstone"] != 1)
		.filter(|row| {
			!before
				.transactions
				.contains_key(&veritas_domain::TransactionId::new(
					row["id"].as_str().unwrap(),
				))
		})
		.collect()
}

/// The rows the session would add for a plan: each post, and for a transfer
/// payee the other leg, which Actual builds and runs the rules on
/// (`transfer.ts:61-79`).
fn planned_rows(plan: &SchedulePlan, book: &RuleBook, budget: &BudgetData) -> Vec<Row> {
	assert!(
		plan.left.is_empty(),
		"every rule here runs in Veritas: {:?}",
		plan.left
	);
	let mut rows = Vec::new();
	for posting in &plan.posts {
		let t = &posting.transaction;
		rows.push(row_key(t));
		let transfer = t
			.payee
			.as_ref()
			.and_then(|p| budget.payees.get(p))
			.and_then(|p| p.transfer_account.clone());
		if let Some(other) = transfer {
			let back = budget
				.payees
				.values()
				.find(|p| p.transfer_account.as_ref() == Some(&t.account))
				.map(|p| p.id.clone());
			let mirror = RuleTransaction {
				account: other,
				amount: Money::from_minor(-t.amount.minor()),
				payee: back,
				cleared: Some(false),
				category: None,
				imported_payee: None,
				reconciled: None,
				..t.clone()
			};
			let mut directory = BudgetPayees::new(budget, || PayeeId::new("made"));
			let ruled = book
				.run(&mirror, budget, &mut directory)
				.transaction
				.expect("the other leg's rules run in Veritas");
			rows.push(row_key(&counterpart(&mirror, &ruled)));
		}
	}
	rows.sort();
	rows
}

fn schedule_state(budget: &BudgetData) -> BTreeMap<String, (Option<String>, bool)> {
	budget
		.schedules
		.values()
		.map(|s| {
			(
				s.id.to_string(),
				(s.next_date.map(|d| d.to_string()), s.completed),
			)
		})
		.collect()
}

fn apply_writes(state: &mut BTreeMap<String, (Option<String>, bool)>, plan: &SchedulePlan) {
	for write in &plan.writes {
		match write {
			ScheduleWrite::NextDate {
				schedule,
				write: NextDateWrite::Local(date) | NextDateWrite::Reset(date),
			} => state.get_mut(schedule.as_str()).unwrap().0 = Some(date.to_string()),
			ScheduleWrite::Complete(schedule) => state.get_mut(schedule.as_str()).unwrap().1 = true,
			ScheduleWrite::Restart(schedule) => state.get_mut(schedule.as_str()).unwrap().1 = false,
			ScheduleWrite::ScheduleRunDone(_) | ScheduleWrite::ClearScheduleRun => {}
		}
	}
}

/// Whether Actual stored each schedule's last next-date write the way Veritas
/// planned it: a local write leaves `local_next_date` in force with the base
/// timestamp, a reset replaces `base_next_date` (`setNextDate`, `app.ts:226-283`).
fn write_kinds(plan: &SchedulePlan, after: &Value) -> Vec<String> {
	let mut last = BTreeMap::new();
	for write in &plan.writes {
		if let ScheduleWrite::NextDate { schedule, write } = write {
			last.insert(schedule.to_string(), *write);
		}
	}
	let rows = after["tables"]["schedules_next_date"]["rows"]
		.as_array()
		.unwrap();
	last.iter()
		.filter_map(|(schedule, write)| {
			let row = rows
				.iter()
				.find(|row| row["schedule_id"] == schedule.as_str())?;
			let stored = match write {
				NextDateWrite::Local(date) => {
					row["local_next_date"] == date_number(*date)
						&& row["local_next_date_ts"] == row["base_next_date_ts"]
				}
				NextDateWrite::Reset(date) => row["base_next_date"] == date_number(*date),
			};
			(!stored).then(|| format!("{schedule}: Veritas plans {write:?}, Actual stored {row}"))
		})
		.collect()
}

fn actual_state(after: &Value) -> BTreeMap<String, (Option<String>, bool)> {
	after["computed"]["schedules"]
		.as_object()
		.unwrap()
		.iter()
		.map(|(id, s)| {
			(
				id.clone(),
				(
					s["nextDate"].as_str().map(str::to_owned),
					s["completed"] == true,
				),
			)
		})
		.collect()
}

/// Every oracle run seeds Actual's id generator the same way by default, so
/// chained runs would hand a new transaction the id of one an earlier run
/// made, and Actual would overwrite that row. Each run gets its own seed.
fn apply_seeded(oracle: &Path, base: &Path, ops: &str, out: &Path, today: &str, seed: usize) {
	let ops_path = out.with_extension("json");
	std::fs::write(&ops_path, ops).unwrap();
	let output = std::process::Command::new("node")
		.current_dir(oracle)
		.arg("src/main.ts")
		.arg("apply")
		.arg(base)
		.arg(&ops_path)
		.arg("--out")
		.arg(out)
		.args(["--today", today, "--seed", &seed.to_string()])
		.output()
		.unwrap();
	assert!(
		output.status.success(),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
}

fn ops_file(ops: &[String]) -> String {
	format!(r#"{{"version":1,"ops":[{}]}}"#, ops.join(","))
}

struct Lockstep {
	oracle: PathBuf,
	scratch: PathBuf,
	zip: PathBuf,
	state: Value,
	step: usize,
}

impl Lockstep {
	fn run(&mut self, ops: &[String], today: &str) -> Value {
		self.step += 1;
		let out = self.scratch.join(format!("step-{}.zip", self.step));
		apply_seeded(
			&self.oracle,
			&self.zip,
			&ops_file(ops),
			&out,
			today,
			1_000 + self.step,
		);
		self.zip = out;
		dump(&self.oracle, &self.zip.to_string_lossy(), today)
	}

	/// What Actual does with a payment that isn't followed by anything: run
	/// the service the day after the bill's next date (or the payment) and
	/// see whether the occurrence counted as paid, so no second post comes and
	/// the bill moves on.
	fn actual_counts_as_paid(
		&self,
		pay_op: &str,
		schedule: &ScheduleId,
		next: CivilDate,
		paid_on: CivilDate,
		today: &str,
	) -> bool {
		let paid = self.scratch.join(format!("probe-{}.zip", self.step));
		apply_seeded(
			&self.oracle,
			&self.zip,
			&ops_file(&[pay_op.to_owned()]),
			&paid,
			today,
			100_000 + self.step,
		);
		let later = (next.max(paid_on).naive() + chrono::Days::new(1)).to_string();
		let before = dump(&self.oracle, &paid.to_string_lossy(), &later);
		let ran = self.scratch.join(format!("probe-{}-ran.zip", self.step));
		apply_only(
			&self.oracle,
			&paid,
			vec![json!({"op": "runSchedules"})],
			&ran,
			&later,
		);
		let after = dump(&self.oracle, &ran.to_string_lossy(), &later);
		let posted_again = new_rows(&budget_from_dump(&before), &after)
			.iter()
			.any(|row| row["schedule"] == schedule.as_str());
		let moved =
			actual_state(&after)[schedule.as_str()] != actual_state(&before)[schedule.as_str()];
		!posted_again && moved
	}
}

fn bill_view(budget: &BudgetData, n: usize) -> (Bill, String) {
	let id = schedule(n);
	let schedule = &budget.schedules[&id];
	let rule = &budget.rules[&schedule.rule];
	let stored_payee: Vec<Value> = serde_json::from_str(&rule.conditions_json).unwrap();
	let payee = stored_payee
		.iter()
		.find(|c| c["field"] == "description")
		.and_then(|c| c["value"].as_str())
		.unwrap()
		.to_owned();
	(
		bill_from_schedule(schedule, rule, budget).unwrap().bill,
		payee,
	)
}

fn payment_op(n: usize, step: usize, payee: &str, amount: i64, date: &str) -> String {
	json!({"op": "addTransactions", "account": checking(), "transactions": [{"id": uuid(9, step), "date": date,
		"amount": amount, "payee": payee, "schedule": schedule(n).as_str(), "cleared": false}]})
	.to_string()
}

fn ops_for_writes(plan: &SchedulePlan, n: usize) -> Vec<String> {
	plan.writes
		.iter()
		.map(|write| match write {
			ScheduleWrite::NextDate { write: NextDateWrite::Local(_), .. } => {
				json!({"op": "skipSchedule", "id": schedule(n).as_str()}).to_string()
			}
			ScheduleWrite::Complete(_) => {
				json!({"op": "updateSchedule", "id": schedule(n).as_str(), "fields": {"completed": true}}).to_string()
			}
			other => panic!("a payment doesn't plan {other:?}"),
		})
		.collect()
}

#[test]
fn a_year_of_household_bills_matches_actual_step_by_step() {
	let Some(oracle) = oracle_dir() else {
		eprintln!(
			"skipping the household year oracle test: tools/actual-oracle/node_modules is missing, run `cargo xtask oracle setup`"
		);
		return;
	};
	let scratch = scratch_dir("household-year");
	let mut setup = vec![
		json!({"op": "createAccount", "id": checking(), "name": "Joint checking"}),
		json!({"op": "createAccount", "id": savings(), "name": "Rainy day"}),
		json!({"op": "createCategoryGroup", "id": uuid(2, 1), "name": "Household"}),
		json!({"op": "createCategory", "id": category(1), "name": "Housing", "group": uuid(2, 1)}),
		json!({"op": "createCategory", "id": category(2), "name": "Utilities", "group": uuid(2, 1)}),
		json!({"op": "createCategory", "id": category(3), "name": "Monthly bills", "group": uuid(2, 1)}),
	];
	for (n, name) in PAYEES.iter().enumerate() {
		setup.push(json!({"op": "createPayee", "id": payee(n), "name": name}));
	}
	setup.extend([
		json!({"op": "createRule", "id": uuid(8, 1), "conditions": [{"op": "is", "field": "payee", "value": payee(0)}],
			"actions": [{"op": "set", "field": "category", "value": category(1)}]}),
		json!({"op": "createRule", "id": uuid(8, 2), "conditions": [{"op": "is", "field": "payee", "value": payee(2)}],
			"actions": [{"op": "set", "field": "category", "value": category(2)}]}),
		json!({"op": "createRule", "id": uuid(8, 3), "stage": "post", "conditions": [{"op": "lt", "field": "amount", "value": -100_000}],
			"actions": [{"op": "append-notes", "value": " (big)"}]}),
		json!({"op": "createRule", "id": uuid(8, 4), "conditions": [{"op": "is", "field": "payee", "value": payee(1)}],
			"actions": [{"op": "set", "field": "notes", "value": "payroll"}]}),
	]);
	let base = scratch.join("base.zip");
	apply_only(
		&oracle,
		&repo_root().join("fixtures/synthetic/small.zip"),
		setup,
		&base,
		CREATED,
	);
	let start = budget_from_dump(&dump(&oracle, &base.to_string_lossy(), CREATED));
	let savings_payee = start
		.payees
		.values()
		.find(|p| p.transfer_account == Some(AccountId::new(savings())))
		.unwrap()
		.id
		.to_string();
	let bills = household(&savings_payee);
	for (n, bill) in bills.iter().enumerate() {
		bill.validate(day(CREATED))
			.unwrap_or_else(|e| panic!("bill {n}: {e:?}"));
	}

	let mut lockstep = Lockstep {
		oracle: oracle.clone(),
		scratch: scratch.clone(),
		zip: base,
		state: Value::Null,
		step: 0,
	};
	let creates: Vec<String> = bills
		.iter()
		.enumerate()
		.map(|(n, b)| create_op(&schedule(n), b))
		.collect();
	lockstep.state = lockstep.run(&creates, CREATED);
	let mut expected_bills = bills.clone();

	let mut service_posts: BTreeMap<usize, usize> = BTreeMap::new();
	let mut posted_rows = 0;
	let mut failures = Vec::new();
	// Bills are only touched with the budget open, and opening runs the
	// service first, so a day with an action starts with an open.
	let mut days: Vec<(&str, Event)> = Vec::new();
	for (today, event) in YEAR {
		if !matches!(event, Open) && days.last().is_none_or(|(last, _)| last != today) {
			days.push((today, Open));
		}
		days.push((today, *event));
	}
	for (today, event) in &days {
		let budget = budget_from_dump(&lockstep.state);
		let book = RuleBook::new(&budget);
		let mut payees = BudgetPayees::new(&budget, || PayeeId::new("made"));
		let label = format!("{today} {event:?}");
		let (plan, ops) = match *event {
			Open => (
				advance_schedules(&book, &budget, &mut payees, day(today)),
				vec![json!({"op": "runSchedules"}).to_string()],
			),
			Pay(n, amount, paid_on) => {
				let (_, stored_payee) = bill_view(&budget, n);
				let mapped = budget.payee_mapping.get(&PayeeId::new(&stored_payee)).map_or(stored_payee.clone(), |p| p.to_string());
				let id = schedule(n);
				let plan = pay(&book, &budget, &mut payees, &id, Money::from_minor(amount), day(paid_on), day(today)).unwrap();
				let payment = payment_op(n, lockstep.step, &mapped, amount, paid_on);
				let next = budget.schedules[&id].next_date.unwrap();
				let veritas = counts_as_paid(&budget, &id, day(paid_on)).unwrap();
				let actual = lockstep.actual_counts_as_paid(&payment, &id, next, day(paid_on), today);
				if veritas != actual {
					failures.push(format!("{label}: Veritas says the payment counts: {veritas}, Actual {actual}"));
				}
				let mut ops = vec![payment];
				ops.extend(ops_for_writes(&plan, n));
				(plan, ops)
			}
			PostNow(n) => (
				post_now(&book, &budget, &mut payees, &schedule(n), None).unwrap(),
				vec![json!({"op": "postSchedule", "id": schedule(n).as_str()}).to_string()],
			),
			PostToday(n) => (
				post_now(&book, &budget, &mut payees, &schedule(n), Some(day(today))).unwrap(),
				vec![json!({"op": "postSchedule", "id": schedule(n).as_str(), "today": true}).to_string()],
			),
			Skip(n) => (
				skip(&budget, &schedule(n)).unwrap(),
				vec![json!({"op": "skipSchedule", "id": schedule(n).as_str()}).to_string()],
			),
			Complete(n) => (
				complete(&budget, &schedule(n)).unwrap(),
				vec![json!({"op": "updateSchedule", "id": schedule(n).as_str(), "fields": {"completed": true}}).to_string()],
			),
			Restart(n) => (
				restart(&budget, &schedule(n), day(today)).unwrap(),
				vec![json!({"op": "updateSchedule", "id": schedule(n).as_str(), "fields": {"completed": false}, "resetNextDate": true}).to_string()],
			),
			NewRent | NewPayDay => {
				let n = if matches!(event, NewRent) { RENT } else { PAYCHECK };
				let (mut edited_bill, stored_payee) = bill_view(&budget, n);
				if n == RENT {
					edited_bill.amount = BillAmount::Exact(Money::from_minor(-192_500));
				} else {
					edited_bill.date = every("2026-09-15", 2, Frequency::Weekly);
				}
				expected_bills[n] = Bill { payee: bills[n].payee.clone(), ..edited_bill.clone() };
				let id = schedule(n);
				let rule = &budget.rules[&budget.schedules[&id].rule];
				let edited = edited_bill.edit_rule(rule, day(today)).unwrap();
				let ops = vec![editor_save_op(&id, &stored_payee, &edited_bill)];
				let after = lockstep.run(&ops, today);
				let stored = budget_from_dump(&after);
				let stored_rule = &stored.rules[&rule.id];
				if stored_rule.conditions_json != edited.conditions_json {
					failures.push(format!(
						"{label}: Actual stored conditions\n  {}\nVeritas writes\n  {}",
						stored_rule.conditions_json, edited.conditions_json
					));
				}
				let actions = edited.actions_json.clone().unwrap_or_else(|| rule.actions_json.clone());
				if stored_rule.actions_json != actions {
					failures.push(format!("{label}: Actual stored actions {}, Veritas {actions}", stored_rule.actions_json));
				}
				let mut plan = SchedulePlan::default();
				if let Some(next) = edited.reset_next_date {
					plan.writes.push(ScheduleWrite::NextDate {
						schedule: id.clone(),
						write: NextDateWrite::Reset(next.unwrap()),
					});
				}
				let mut state = schedule_state(&budget);
				apply_writes(&mut state, &plan);
				failures.extend(write_kinds(&plan, &after).into_iter().map(|w| format!("{label}: {w}")));
				if state != actual_state(&after) {
					let actual = actual_state(&after);
					let differ: Vec<String> = state
						.iter()
						.filter(|(id, veritas)| actual[*id] != **veritas)
						.map(|(id, veritas)| format!("{id}: Actual {:?}, Veritas {veritas:?}", actual[id]))
						.collect();
					failures.push(format!("{label}: next dates differ after the edit: {differ:?}"));
				}
				lockstep.state = after;
				continue;
			}
			MergePowerCo => (
				SchedulePlan::default(),
				vec![json!({"op": "mergePayees", "target": payee(10), "merge": [payee(2)]}).to_string()],
			),
			DeleteUtilities => (
				SchedulePlan::default(),
				vec![json!({"op": "deleteCategory", "id": category(2), "transferCategory": category(3)}).to_string()],
			),
		};
		if matches!(event, Open) {
			for posting in &plan.posts {
				let n = (0..bills.len())
					.find(|n| schedule(*n) == posting.schedule)
					.unwrap();
				*service_posts.entry(n).or_default() += 1;
			}
		}
		let planned = planned_rows(&plan, &book, &budget);
		let after = lockstep.run(&ops, today);
		let mut actual: Vec<Row> = new_rows(&budget, &after)
			.into_iter()
			.map(dump_row_key)
			.collect();
		actual.sort();
		if planned != actual {
			failures.push(format!(
				"{label}: posted rows differ\n  Actual  {actual:?}\n  Veritas {planned:?}"
			));
		}
		posted_rows += actual.len();
		let mut state = schedule_state(&budget);
		apply_writes(&mut state, &plan);
		failures.extend(
			write_kinds(&plan, &after)
				.into_iter()
				.map(|w| format!("{label}: {w}")),
		);
		let actual_after = actual_state(&after);
		for (id, veritas) in &state {
			if actual_after[id] != *veritas {
				failures.push(format!(
					"{label}: schedule {id}: Actual {:?}, Veritas {veritas:?}",
					actual_after[id]
				));
			}
		}
		lockstep.state = after;
	}

	let end = budget_from_dump(&lockstep.state);
	let _ = std::fs::remove_dir_all(&scratch);
	assert!(
		failures.is_empty(),
		"{} differences:\n{}",
		failures.len(),
		failures.join("\n")
	);

	// R-BILL-2: confirm-first bills are never posted by the daily run, and
	// every auto-post bill was.
	for n in CONFIRM_FIRST {
		assert_eq!(service_posts.get(&n), None, "{}", PAYEES[n]);
	}
	for n in [
		RENT,
		PAYCHECK,
		INSURANCE,
		PHONE,
		GYM,
		HOA,
		SAVINGS,
		PROPERTY_TAX,
	] {
		assert!(
			service_posts.get(&n).is_some_and(|count| *count > 0),
			"bill {n} never auto-posted"
		);
	}
	// No occurrence was paid twice, and the phone plan stopped after six.
	let mut seen = BTreeSet::new();
	let mut per_bill: BTreeMap<String, usize> = BTreeMap::new();
	for t in end
		.transactions
		.values()
		.filter(|t| !t.tombstone && t.account.as_str() == checking())
	{
		if let Some(s) = &t.schedule {
			assert!(
				seen.insert((s.clone(), t.date)),
				"{s} posted twice on {}",
				t.date
			);
			*per_bill.entry(s.to_string()).or_default() += 1;
		}
	}
	assert_eq!(per_bill[schedule(PHONE).as_str()], 6);
	assert_eq!(per_bill[schedule(RENT).as_str()], 12);

	// R-BILL-4: after a year of Actual's own writes every schedule still
	// reads back as the bill it was saved as, with nothing custom on its rule.
	for (n, expected) in expected_bills.iter().enumerate() {
		let id = schedule(n);
		let view = bill_from_schedule(
			&end.schedules[&id],
			&end.rules[&end.schedules[&id].rule],
			&end,
		)
		.unwrap();
		assert!(!view.custom_rule, "bill {n}");
		let payee = if n == ELECTRIC {
			Some(PayeeId::new(payee(10)))
		} else {
			expected.payee.clone()
		};
		assert_eq!(
			view.bill,
			Bill {
				payee,
				..expected.clone()
			},
			"bill {n}"
		);
	}
	eprintln!(
		"{} steps, {posted_rows} rows posted, all in lockstep with Actual",
		days.len()
	);
}
