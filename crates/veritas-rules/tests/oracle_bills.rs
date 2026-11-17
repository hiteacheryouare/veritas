//! R-BILL-1 to R-BILL-4 against the oracle. Bills Veritas builds are created
//! through Actual's `createSchedule` and must be stored byte for byte as
//! Veritas writes them; then Actual's schedule service posts and advances
//! them, and Veritas's plan for the same budget must agree.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;

use chrono::{Datelike, Weekday};
use common::{
	Rng, apply_only, apply_text_and_dump, budget_from_dump, dump, oracle_dir, repo_root,
	scratch_dir, uuid,
};
use serde_json::{Value, json};
use veritas_domain::{AccountId, CivilDate, Money, PayeeId, ScheduleId};
use veritas_rules::{
	Bill, BillAmount, BillDate, BudgetPayees, Frequency, NextDateWrite, Op, Repeat, RepeatEnd,
	RepeatPattern, RuleBook, ScheduleWrite, WeekendMove, advance_schedules, bill_from_schedule,
	next_date_from, parse_date_value,
};

fn day(text: &str) -> CivilDate {
	CivilDate::from(chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap())
}

fn accounts() -> Vec<String> {
	(1..=3).map(|n| uuid(1, n)).collect()
}

fn payees() -> Vec<(String, &'static str)> {
	vec![
		(uuid(4, 1), "Landlord"),
		(uuid(4, 2), "Power Co"),
		(uuid(4, 3), "Employer"),
		(uuid(4, 4), "Gym"),
	]
}

fn random_bill(rng: &mut Rng, n: usize) -> Bill {
	let start = day(&format!(
		"2026-{:02}-{:02}",
		3 + rng.below(6),
		1 + rng.below(28)
	))
	.naive();
	let start = if rng.chance(25) {
		start.with_day(*rng.pick(&[29, 30, 31])).unwrap_or(start)
	} else {
		start
	};
	let frequency = *rng.pick(&[
		Frequency::Monthly,
		Frequency::Monthly,
		Frequency::Weekly,
		Frequency::Yearly,
		Frequency::Daily,
	]);
	let mut patterns = Vec::new();
	if frequency == Frequency::Monthly && rng.chance(40) {
		for _ in 0..1 + rng.below(2) {
			patterns.push(if rng.chance(60) {
				RepeatPattern::Day(*rng.pick(&[1, 15, 28, 30, 31, -1]))
			} else {
				let weekday = *rng.pick(&[Weekday::Mon, Weekday::Fri, Weekday::Sun]);
				RepeatPattern::Weekday(weekday, *rng.pick(&[1, 2, 4, -1]))
			});
		}
	}
	let end = match rng.below(4) {
		0 => RepeatEnd::AfterOccurrences(1 + rng.below(8) as u32),
		1 => RepeatEnd::OnDate(CivilDate::from(
			start + chrono::Days::new(30 + rng.below(300)),
		)),
		_ => RepeatEnd::Never,
	};
	let repeat = Repeat {
		start: start.into(),
		interval: *rng.pick(&[1, 1, 1, 2, 3]),
		frequency,
		patterns,
		weekend: *rng.pick(&[
			None,
			None,
			Some(WeekendMove::Before),
			Some(WeekendMove::After),
		]),
		end,
	};
	let amount = -(100 + rng.below(200_000) as i64);
	Bill {
		name: Some(format!("Bill {n}")),
		payee: Some(PayeeId::new(&rng.pick(&payees()).0)),
		account: Some(AccountId::new(rng.pick(&accounts()))),
		amount: match rng.below(4) {
			0 => BillAmount::About(Money::from_minor(amount)),
			1 => BillAmount::Between(
				Money::from_minor(amount),
				Money::from_minor(amount - 1 - rng.below(5000) as i64),
			),
			_ => BillAmount::Exact(Money::from_minor(amount)),
		},
		date: if rng.chance(12) {
			BillDate::Once(start.into())
		} else {
			BillDate::Repeat(repeat)
		},
		category: None,
		posts_transaction: rng.chance(70),
	}
}

fn bills(rng: &mut Rng, count: usize, today: CivilDate) -> Vec<(ScheduleId, Bill)> {
	let mut out = Vec::new();
	let mut n = 0;
	while out.len() < count {
		n += 1;
		let bill = random_bill(rng, n);
		if bill.validate(today).is_ok() {
			out.push((ScheduleId::new(uuid(10, n)), bill));
		}
	}
	out
}

fn json_string(text: &str) -> String {
	Value::String(text.to_owned()).to_string()
}

/// The `createSchedule` op for a bill, with the date written by Veritas so
/// Actual stores exactly Veritas's key order.
fn create_op(id: &ScheduleId, bill: &Bill) -> String {
	let rule = bill.new_rule(id);
	let conditions: Vec<Value> = serde_json::from_str(&rule.conditions_json).unwrap();
	let date_text = {
		let start = rule
			.conditions_json
			.find(r#""field":"date","value":"#)
			.unwrap() + r#""field":"date","value":"#.len();
		let end = rule.conditions_json[start..]
			.find(r#"},{"op":"#)
			.map_or(rule.conditions_json.len(), |end| start + end);
		rule.conditions_json[start..end].to_owned()
	};
	let amount = &conditions.last().unwrap();
	format!(
		r#"{{"op":"createSchedule","id":{},"name":{},"postsTransaction":{},"payee":{},"account":{},"amount":{},"amountOp":{},"date":{}}}"#,
		json_string(id.as_str()),
		json_string(bill.name.as_deref().unwrap_or_default()),
		bill.posts_transaction,
		json_string(bill.payee.as_ref().unwrap().as_str()),
		json_string(bill.account.as_ref().unwrap().as_str()),
		amount["value"],
		amount["op"],
		date_text
	)
}

fn setup_ops() -> Vec<String> {
	let mut ops = Vec::new();
	for (n, id) in accounts().iter().enumerate() {
		ops.push(
			json!({"op": "createAccount", "id": id, "name": format!("Account {n}")}).to_string(),
		);
	}
	for (id, name) in payees() {
		ops.push(json!({"op": "createPayee", "id": id, "name": name}).to_string());
	}
	ops
}

fn ops_text(ops: &[String]) -> String {
	format!(r#"{{"version":1,"ops":[{}]}}"#, ops.join(","))
}

/// R-BILL-4: a bill Veritas saves is stored by Actual exactly as Veritas
/// writes it, reads back as the same bill, and gets the next date Veritas
/// computes.
#[test]
fn new_bills_are_stored_byte_for_byte() {
	let Some(oracle) = oracle_dir() else {
		eprintln!(
			"skipping the bill oracle test: tools/actual-oracle/node_modules is missing, run `cargo xtask oracle setup`"
		);
		return;
	};
	let today = "2026-05-01";
	let bills = bills(&mut Rng(0xb111_0001), 150, day(today));
	let mut ops = setup_ops();
	ops.extend(bills.iter().map(|(id, bill)| create_op(id, bill)));
	let scratch = scratch_dir("bills");
	let dump = apply_text_and_dump(
		&oracle,
		&scratch,
		"bills",
		&repo_root().join("fixtures/synthetic/small.zip"),
		&ops_text(&ops),
		today,
	);
	let _ = std::fs::remove_dir_all(&scratch);
	let budget = budget_from_dump(&dump);
	let paths: BTreeMap<&str, &Value> = dump["tables"]["schedules_json_paths"]["rows"]
		.as_array()
		.unwrap()
		.iter()
		.map(|row| (row["schedule_id"].as_str().unwrap(), row))
		.collect();
	for (id, bill) in &bills {
		let schedule = &budget.schedules[id];
		let rule = &budget.rules[&schedule.rule];
		let expected = bill.new_rule(id);
		assert_eq!(rule.conditions_json, expected.conditions_json, "{id}");
		assert_eq!(rule.actions_json, expected.actions_json, "{id}");
		let row = paths[id.as_str()];
		assert_eq!(
			[&row["payee"], &row["account"], &row["amount"], &row["date"]]
				.map(|v| v.as_str().map(str::to_owned)),
			[
				expected.json_paths.payee,
				expected.json_paths.account,
				expected.json_paths.amount,
				expected.json_paths.date
			]
		);
		let date = parse_date_value(
			Op::IsApprox,
			Some(
				&serde_json::from_str(&rule.conditions_json)
					.map(|c: Vec<Value>| c[2]["value"].clone())
					.unwrap(),
			),
		)
		.unwrap();
		assert_eq!(
			schedule.next_date,
			next_date_from(&date, day(today)).unwrap(),
			"{id}"
		);
		assert_eq!(
			bill_from_schedule(schedule, rule, &budget).unwrap().bill,
			*bill,
			"{id}"
		);
	}
	eprintln!("{} bills stored byte for byte", bills.len());
}

type Posted = (
	String,
	i64,
	i64,
	Option<String>,
	String,
	Option<String>,
	Option<String>,
	i64,
);

/// R-BILL-2 and R-BILL-3: Actual's `runSchedules` and Veritas's
/// `advance_schedules` post the same transactions (missed occurrences caught
/// up one by one, rules applied) and leave the same next dates.
#[test]
fn the_schedule_service_matches_the_oracle() {
	let Some(oracle) = oracle_dir() else {
		eprintln!(
			"skipping the posting oracle test: tools/actual-oracle/node_modules is missing, run `cargo xtask oracle setup`"
		);
		return;
	};
	let (created, today) = ("2026-05-01", "2026-11-09");
	let mut rng = Rng(0xb111_0002);
	let bills = bills(&mut rng, 120, day(created));
	let mut ops = setup_ops();
	ops.push(
		json!({"op": "createCategoryGroup", "id": uuid(2, 1), "name": "Recurring"}).to_string(),
	);
	ops.push(
		json!({"op": "createCategory", "id": uuid(3, 1), "name": "Housing", "group": uuid(2, 1)})
			.to_string(),
	);
	ops.push(json!({"op": "createRule", "id": uuid(8, 1), "conditions": [{"op": "is", "field": "payee", "value": payees()[0].0}],
		"actions": [{"op": "set", "field": "category", "value": uuid(3, 1)}]}).to_string());
	ops.push(json!({"op": "createRule", "id": uuid(8, 2), "stage": "post", "conditions": [{"op": "lt", "field": "amount", "value": -100_000}],
		"actions": [{"op": "append-notes", "value": " (big)"}]}).to_string());
	ops.extend(bills.iter().map(|(id, bill)| create_op(id, bill)));
	// Some bills already paid around their first date, some early, some late.
	let mut paid = Vec::new();
	for (n, (id, bill)) in bills.iter().enumerate() {
		let BillDate::Repeat(repeat) = &bill.date else {
			continue;
		};
		if !rng.chance(25) {
			continue;
		}
		let date = CivilDate::from(
			repeat.start.naive() + chrono::Days::new(rng.below(5))
				- chrono::Days::new(rng.below(5)),
		);
		paid.push(json!({"id": uuid(9, n), "date": date.to_string(), "amount": -500, "schedule": id.as_str(), "cleared": true}));
	}
	ops.push(json!({"op": "addTransactions", "account": accounts()[0], "transactions": paid, "runTransfers": false}).to_string());
	ops.push(json!({"op": "closeAccount", "id": accounts()[2]}).to_string());

	let scratch = scratch_dir("posting");
	let base = repo_root().join("fixtures/synthetic/small.zip");
	let _ = apply_text_and_dump(
		&oracle,
		&scratch,
		"created",
		&base,
		&ops_text(&ops),
		created,
	);
	let before = dump(
		&oracle,
		&scratch.join("created.zip").to_string_lossy(),
		today,
	);
	let ran = scratch.join("ran.zip");
	apply_only(
		&oracle,
		&scratch.join("created.zip"),
		vec![json!({"op": "runSchedules"})],
		&ran,
		today,
	);
	let after = dump(&oracle, &ran.to_string_lossy(), today);
	let _ = std::fs::remove_dir_all(&scratch);

	let budget = budget_from_dump(&before);
	let book = RuleBook::new(&budget);
	let mut payees = BudgetPayees::new(&budget, || PayeeId::new("made"));
	let plan = advance_schedules(&book, &budget, &mut payees, day(today));
	assert!(
		plan.left.is_empty(),
		"every rule in this budget runs in Veritas: {:?}",
		plan.left
	);
	assert_eq!(
		plan.writes.last(),
		Some(&ScheduleWrite::ScheduleRunDone(day(today)))
	);

	let key = |schedule: &str,
	           date: CivilDate,
	           amount: i64,
	           payee: Option<&str>,
	           account: &str,
	           category: Option<&str>,
	           notes: Option<&str>,
	           cleared: bool|
	 -> Posted {
		(
			schedule.to_owned(),
			i64::from(date.year()) * 10_000 + i64::from(date.month()) * 100 + i64::from(date.day()),
			amount,
			payee.map(str::to_owned),
			account.to_owned(),
			category.map(str::to_owned),
			notes.map(str::to_owned),
			i64::from(cleared),
		)
	};
	let mut veritas: Vec<Posted> = plan
		.posts
		.iter()
		.map(|p| {
			let t = &p.transaction;
			key(
				p.schedule.as_str(),
				t.date,
				t.amount.minor(),
				t.payee.as_ref().map(|p| p.as_str()),
				t.account.as_str(),
				t.category.as_ref().map(|c| c.as_str()),
				t.notes.as_deref(),
				t.cleared == Some(true),
			)
		})
		.collect();
	let mut actual: Vec<Posted> = after["tables"]["transactions"]["rows"]
		.as_array()
		.unwrap()
		.iter()
		.filter(|row| {
			!budget
				.transactions
				.contains_key(&veritas_domain::TransactionId::new(
					row["id"].as_str().unwrap(),
				))
		})
		.map(|row| {
			let date = row["date"].as_i64().unwrap();
			(
				row["schedule"].as_str().unwrap_or_default().to_owned(),
				date,
				row["amount"].as_i64().unwrap(),
				row["description"].as_str().map(str::to_owned),
				row["acct"].as_str().unwrap().to_owned(),
				row["category"].as_str().map(str::to_owned),
				row["notes"].as_str().map(str::to_owned),
				row["cleared"].as_i64().unwrap(),
			)
		})
		.collect();
	veritas.sort();
	actual.sort();
	assert!(
		!actual.is_empty(),
		"Actual posted nothing, so the test proves nothing"
	);
	assert_eq!(veritas, actual, "posted transactions differ");

	let mut next: BTreeMap<String, Option<String>> = budget
		.schedules
		.values()
		.map(|s| (s.id.to_string(), s.next_date.map(|d| d.to_string())))
		.collect();
	let mut completed: BTreeMap<String, bool> = budget
		.schedules
		.values()
		.map(|s| (s.id.to_string(), s.completed))
		.collect();
	for write in &plan.writes {
		match write {
			ScheduleWrite::NextDate {
				schedule,
				write: NextDateWrite::Local(date) | NextDateWrite::Reset(date),
			} => {
				next.insert(schedule.to_string(), Some(date.to_string()));
			}
			ScheduleWrite::Complete(schedule) => {
				completed.insert(schedule.to_string(), true);
			}
			ScheduleWrite::Restart(schedule) => {
				completed.insert(schedule.to_string(), false);
			}
			ScheduleWrite::ScheduleRunDone(_) | ScheduleWrite::ClearScheduleRun => {}
		}
	}
	let mut differ = Vec::new();
	for (id, computed) in after["computed"]["schedules"].as_object().unwrap() {
		let actual_next = computed["nextDate"].as_str().map(str::to_owned);
		if next[id] != actual_next || completed[id] != (computed["completed"] == true) {
			differ.push(format!(
				"{id}: actual {actual_next:?} {}, veritas {:?} {}",
				computed["completed"], next[id], completed[id]
			));
		}
	}
	assert!(differ.is_empty(), "{}", differ.join("\n"));
	let moved = plan.writes.len();
	eprintln!(
		"{} transactions posted, {moved} schedule writes, all match",
		actual.len()
	);
}
