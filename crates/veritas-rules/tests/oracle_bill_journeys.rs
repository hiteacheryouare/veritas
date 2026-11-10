//! PLAN §9 #13 as a person meets it: rent due on the 31st through February, a
//! bill on Feb 29, and weekend moves that push a due date into another month.
//! Actual creates each bill and reports its next and upcoming dates; Veritas
//! must report the same ones from the rule row Actual stored, and must link a
//! payment to the bill exactly when Actual does (R-BILL-1, R-RULE-1).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;

use chrono::NaiveDate;
use common::{
	apply_and_dump, expected_row, oracle_dir, repo_root, rule_from_row, rule_transaction,
	scratch_dir, uuid,
};
use serde_json::{Value, json};
use veritas_domain::{
	Account, AccountId, BudgetData, BudgetType, CivilDate, Payee, PayeeId, RuleId, Schedule,
	ScheduleId,
};
use veritas_rules::{Op, RuleBook, next_date_from, parse_date_value, schedule_dates};

fn monthly(start: &str, patterns: Value, skip_weekend: Option<&str>) -> Value {
	json!({"start": start, "frequency": "monthly", "interval": 1, "patterns": patterns,
		"skipWeekend": skip_weekend.is_some(), "weekendSolveMode": skip_weekend.unwrap_or("after"),
		"endMode": "never"})
}

fn yearly(start: &str, skip_weekend: Option<&str>) -> Value {
	json!({"start": start, "frequency": "yearly", "interval": 1, "patterns": [],
		"skipWeekend": skip_weekend.is_some(), "weekendSolveMode": skip_weekend.unwrap_or("after"),
		"endMode": "never"})
}

struct Bill {
	name: &'static str,
	date: Value,
	next: &'static str,
	/// The first upcoming dates, or the full list when it's shorter than the
	/// twelve the oracle asks for.
	upcoming: &'static [&'static str],
}

fn bill(
	name: &'static str,
	date: Value,
	next: &'static str,
	upcoming: &'static [&'static str],
) -> Bill {
	Bill {
		name,
		date,
		next,
		upcoming,
	}
}

fn journeys() -> Vec<(&'static str, Vec<Bill>)> {
	let last_day = json!([{"type": "day", "value": -1}]);
	let the_31st = json!([{"type": "day", "value": 31}]);
	let the_1st = json!([{"type": "day", "value": 1}]);
	vec![
		(
			"2027-02-01",
			vec![
				// Actual skips months without a 31st when the start date is the 31st.
				bill(
					"rent dated from Jan 31",
					monthly("2026-01-31", json!([]), None),
					"2027-03-31",
					&[
						"2027-03-31",
						"2027-05-31",
						"2027-07-31",
						"2027-08-31",
						"2027-10-31",
					],
				),
				bill(
					"rent on the 31st",
					monthly("2026-01-31", the_31st.clone(), None),
					"2027-03-31",
					&["2027-03-31", "2027-05-31", "2027-07-31"],
				),
				bill(
					"rent on the last day",
					monthly("2026-01-31", last_day.clone(), None),
					"2027-02-28",
					&["2027-02-28", "2027-03-31", "2027-04-30", "2027-05-31"],
				),
				// Sunday Jan 31 moves to Monday Feb 1, which is today: due, not skipped.
				bill(
					"rent on the last day, paid the Monday after a weekend",
					monthly("2026-01-31", last_day.clone(), Some("after")),
					"2027-02-01",
					&[
						"2027-02-01",
						"2027-03-01",
						"2027-03-31",
						"2027-04-30",
						"2027-05-31",
						"2027-06-30",
						"2027-08-02",
						"2027-08-31",
					],
				),
				bill(
					"rent on the last day, paid the Friday before a weekend",
					monthly("2026-01-31", last_day.clone(), Some("before")),
					"2027-02-26",
					&[
						"2027-02-26",
						"2027-03-31",
						"2027-04-30",
						"2027-05-31",
						"2027-06-30",
						"2027-07-30",
					],
				),
			],
		),
		(
			"2026-11-09",
			vec![
				bill(
					"leap-day renewal",
					yearly("2024-02-29", None),
					"2028-02-29",
					&["2028-02-29", "2032-02-29", "2036-02-29"],
				),
				bill(
					"one-time bill on Feb 29",
					json!("2028-02-29"),
					"2028-02-29",
					&["2028-02-29"],
				),
			],
		),
		(
			"2028-03-01",
			vec![
				bill(
					"leap-day renewal after this year's",
					yearly("2024-02-29", None),
					"2032-02-29",
					&["2032-02-29", "2036-02-29"],
				),
				// Feb 29 2032 is a Sunday.
				bill(
					"leap-day renewal, Friday before a weekend",
					yearly("2024-02-29", Some("before")),
					"2032-02-27",
					&["2032-02-27"],
				),
				bill(
					"leap-day renewal, Monday after a weekend",
					yearly("2024-02-29", Some("after")),
					"2032-03-01",
					&["2032-03-01"],
				),
				bill(
					"monthly from Feb 29",
					monthly("2028-02-29", json!([]), None),
					"2028-03-29",
					&[
						"2028-03-29",
						"2028-04-29",
						"2028-05-29",
						"2028-06-29",
						"2028-07-29",
						"2028-08-29",
						"2028-09-29",
						"2028-10-29",
						"2028-11-29",
						"2028-12-29",
						"2029-01-29",
						"2029-03-29",
					],
				),
			],
		),
		(
			"2026-10-30",
			vec![
				// Sunday Nov 1 moves back to Friday Oct 30.
				bill(
					"bill on the 1st, Friday before a weekend",
					monthly("2026-01-01", the_1st.clone(), Some("before")),
					"2026-10-30",
					&["2026-10-30", "2026-12-01", "2027-01-01"],
				),
			],
		),
		(
			"2026-10-31",
			vec![
				// November's bill already came due yesterday, in October.
				bill(
					"bill on the 1st, a day after it moved to Friday",
					monthly("2026-01-01", the_1st, Some("before")),
					"2026-10-30",
					&["2026-12-01", "2027-01-01"],
				),
			],
		),
		(
			"2026-11-01",
			vec![
				// Saturday Oct 31 moves forward to Monday Nov 2.
				bill(
					"rent on the last day, October's moved into November",
					monthly("2026-01-31", last_day, Some("after")),
					"2026-11-02",
					&[
						"2026-11-02",
						"2026-11-30",
						"2026-12-31",
						"2027-02-01",
						"2027-03-01",
					],
				),
			],
		),
	]
}

fn day(text: &str) -> CivilDate {
	CivilDate::from(NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap())
}

fn stored_dates(dump: &Value) -> BTreeMap<String, Value> {
	let rules: BTreeMap<&str, Vec<Value>> = dump["tables"]["rules"]["rows"]
		.as_array()
		.into_iter()
		.flatten()
		.filter_map(|row| {
			Some((
				row["id"].as_str()?,
				serde_json::from_str(row["conditions"].as_str()?).ok()?,
			))
		})
		.collect();
	dump["tables"]["schedules"]["rows"]
		.as_array()
		.into_iter()
		.flatten()
		.filter_map(|row| {
			let date = rules
				.get(row["rule"].as_str()?)?
				.iter()
				.find(|condition| condition["field"] == "date")?
				.clone();
			Some((row["id"].as_str()?.to_owned(), date))
		})
		.collect()
}

#[test]
fn bills_at_the_end_of_the_month_and_on_leap_day_fall_due_when_actual_says() {
	let Some(oracle) = oracle_dir() else {
		eprintln!(
			"skipping the bill journeys test: tools/actual-oracle/node_modules is missing, run `cargo xtask oracle setup`"
		);
		return;
	};
	let scratch = scratch_dir("bill-journeys");
	let mut failures = Vec::new();
	for (t, (today_text, bills)) in journeys().into_iter().enumerate() {
		let today = day(today_text);
		let id = |n: usize| uuid(0xb111 + t as u32, n);
		let ops: Vec<Value> = bills
			.iter()
			.enumerate()
			.map(
				|(n, bill)| json!({"op": "createSchedule", "id": id(n), "name": bill.name, "date": bill.date}),
			)
			.collect();
		let dump = apply_and_dump(
			&oracle,
			&scratch,
			&format!("journey-{t}"),
			&repo_root().join("fixtures/synthetic/small.zip"),
			ops,
			today_text,
		);
		let stored = stored_dates(&dump);
		for (n, bill) in bills.iter().enumerate() {
			let computed = &dump["computed"]["schedules"][id(n)];
			let condition = &stored[&id(n)];
			assert_eq!(
				condition["value"], bill.date,
				"{}: Actual stored another date",
				bill.name
			);
			let date = parse_date_value(
				Op::from_stored(condition["op"].as_str().unwrap()).unwrap(),
				Some(&condition["value"]),
			)
			.unwrap_or_else(|gap| panic!("{}: {gap:?}", bill.name));
			let next = next_date_from(&date, today)
				.unwrap()
				.map(|next| next.to_string());
			let upcoming: Vec<String> = schedule_dates(&date, today, None, Some(12))
				.unwrap()
				.iter()
				.map(ToString::to_string)
				.collect();

			let actual_upcoming: Vec<String> = computed["upcoming"]
				.as_array()
				.unwrap_or_else(|| panic!("{}: Actual gave no upcoming dates", bill.name))
				.iter()
				.map(|date| date.as_str().unwrap().to_owned())
				.collect();
			if computed["nextDate"].as_str() != next.as_deref() || actual_upcoming != upcoming {
				failures.push(format!(
					"today {today_text}, {}\n  actual:  next {} upcoming {actual_upcoming:?}\n  veritas: next {next:?} upcoming {upcoming:?}",
					bill.name, computed["nextDate"]
				));
			}
			let shown = upcoming.len().min(bill.upcoming.len());
			if next.as_deref() != Some(bill.next) || upcoming[..shown] != *bill.upcoming {
				failures.push(format!(
					"today {today_text}, {}: expected next {} upcoming {:?}, got next {next:?} upcoming {upcoming:?}",
					bill.name, bill.next, bill.upcoming
				));
			}
			if bill.upcoming.len() < 12 && upcoming.len() < 12 {
				assert_eq!(upcoming, bill.upcoming, "{}", bill.name);
			}
		}
	}
	let _ = std::fs::remove_dir_all(&scratch);
	assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const CHECKING: &str = "00000001-0000-4000-8000-0000000000c0";
const LANDLORD: &str = "00000004-0000-4000-8000-0000000000a0";
const STORAGE: &str = "00000004-0000-4000-8000-0000000000a1";
const RENT: &str = "0000000b-0000-4000-8000-000000000001";
const STORAGE_UNIT: &str = "0000000b-0000-4000-8000-000000000002";

fn budget_from(dump: &Value) -> BudgetData {
	let mut budget = BudgetData::empty(BudgetType::Envelope);
	budget.accounts.insert(
		AccountId::new(CHECKING),
		Account {
			id: AccountId::new(CHECKING),
			name: "Household checking".into(),
			off_budget: false,
			closed: false,
			group: None,
			sort_order: 0.0,
			tombstone: false,
		},
	);
	for (id, name) in [(LANDLORD, "Maple Court Apartments"), (STORAGE, "StoreMore")] {
		budget.payees.insert(
			PayeeId::new(id),
			Payee {
				id: PayeeId::new(id),
				name: name.into(),
				transfer_account: None,
				favorite: false,
				learn_categories: true,
				tombstone: false,
			},
		);
	}
	for row in dump["tables"]["rules"]["rows"]
		.as_array()
		.into_iter()
		.flatten()
	{
		let rule = rule_from_row(row);
		budget.rules.insert(rule.id.clone(), rule);
	}
	for row in dump["tables"]["schedules"]["rows"]
		.as_array()
		.into_iter()
		.flatten()
	{
		let id = ScheduleId::new(row["id"].as_str().unwrap());
		budget.schedules.insert(
			id.clone(),
			Schedule {
				id,
				name: row["name"].as_str().map(str::to_owned),
				rule: RuleId::new(row["rule"].as_str().unwrap()),
				posts_transaction: row["posts_transaction"] == 1,
				completed: row["completed"] == 1,
				next_date: None,
				tombstone: row["tombstone"] == 1,
			},
		);
	}
	budget
}

/// A payment is linked to a bill when its date is within two days of a real
/// occurrence, not of the weekend-moved due date, and a bill that skips
/// February has nothing there to link to.
#[test]
fn month_end_payments_link_to_the_bill_exactly_when_actual_links_them() {
	let Some(oracle) = oracle_dir() else {
		eprintln!(
			"skipping the bill payment test: tools/actual-oracle/node_modules is missing, run `cargo xtask oracle setup`"
		);
		return;
	};
	let payments = [
		(
			"rent paid the Friday before Sat Jan 31",
			LANDLORD,
			"2026-01-30",
			-150_000,
			Some(RENT),
		),
		(
			"rent paid the Friday before Sat Feb 28",
			LANDLORD,
			"2026-02-27",
			-150_000,
			Some(RENT),
		),
		(
			"rent paid the Monday after Sat Feb 28",
			LANDLORD,
			"2026-03-02",
			-150_000,
			Some(RENT),
		),
		(
			"rent paid a week late",
			LANDLORD,
			"2026-03-05",
			-150_000,
			None,
		),
		("rent paid short", LANDLORD, "2026-04-30", -149_999, None),
		(
			"storage paid in February",
			STORAGE,
			"2026-02-27",
			-8_900,
			None,
		),
		(
			"storage paid near Mar 31",
			STORAGE,
			"2026-03-30",
			-9_200,
			Some(STORAGE_UNIT),
		),
		(
			"storage after a price hike",
			STORAGE,
			"2026-03-30",
			-12_000,
			None,
		),
	];
	let mut ops = vec![
		json!({"op": "createAccount", "id": CHECKING, "name": "Household checking", "offBudget": false}),
		json!({"op": "createPayee", "id": LANDLORD, "name": "Maple Court Apartments"}),
		json!({"op": "createPayee", "id": STORAGE, "name": "StoreMore"}),
		json!({"op": "createSchedule", "id": RENT, "name": "Rent", "payee": LANDLORD,
			"account": CHECKING, "amount": -150_000, "amountOp": "is",
			"date": monthly("2026-01-31", json!([{"type": "day", "value": -1}]), Some("before"))}),
		json!({"op": "createSchedule", "id": STORAGE_UNIT, "name": "Storage unit", "payee": STORAGE,
			"account": CHECKING, "amount": -8_900, "amountOp": "isapprox",
			"date": monthly("2026-01-31", json!([]), None)}),
	];
	let inputs: Vec<Value> = payments
		.iter()
		.enumerate()
		.map(|(n, (_, payee, date, amount, _))| {
			json!({"id": uuid(9, n), "date": date, "amount": amount, "payee": payee})
		})
		.collect();
	ops.push(
		json!({"op": "addTransactions", "account": CHECKING, "transactions": inputs,
		"runTransfers": false}),
	);

	let scratch = scratch_dir("bill-payments");
	let dump = apply_and_dump(
		&oracle,
		&scratch,
		"payments",
		&repo_root().join("fixtures/synthetic/small.zip"),
		ops,
		"2026-11-09",
	);
	let _ = std::fs::remove_dir_all(&scratch);

	let budget = budget_from(&dump);
	let book = RuleBook::new(&budget);
	let rows: BTreeMap<&str, &Value> = dump["tables"]["transactions"]["rows"]
		.as_array()
		.into_iter()
		.flatten()
		.filter_map(|row| Some((row["id"].as_str()?, row)))
		.collect();
	for (n, (label, _, _, _, linked)) in payments.iter().enumerate() {
		let outcome = book.run(&rule_transaction(&inputs[n], CHECKING), &budget);
		assert!(outcome.undetermined.is_empty(), "{label}");
		let veritas = expected_row(&outcome.transaction, CHECKING);
		let row = rows[uuid(9, n).as_str()];
		for key in veritas.as_object().unwrap().keys() {
			assert_eq!(row[key.as_str()], veritas[key.as_str()], "{label}: {key}");
		}
		assert_eq!(row["schedule"].as_str(), *linked, "{label}");
	}
}
