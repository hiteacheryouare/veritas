//! FC-2 known flows from the budget itself (R-FC-1, R-BILL-5): bills and
//! income schedules expanded with Actual's recurrence, and loan payments
//! from loan metadata.

#![allow(clippy::unwrap_used, clippy::panic)]

mod support;

use serde_json::{Value, json};
use support::*;
use veritas_domain::{
	AccountKind, AccountMeta, ConditionsOp, Rule, RuleId, RuleStage, Schedule, ScheduleId,
	SplitRole,
};
use veritas_forecast::{Certainty, FlowKind, Forecaster, ScheduledFlows};

const TODAY: &str = "2027-01-15";

fn condition(field: &str, op: &str, value: Value) -> Value {
	let kind = match field {
		"payee" | "account" => "id",
		"amount" => "number",
		"date" => "date",
		_ => "string",
	};
	json!({"op": op, "field": field, "value": value, "type": kind})
}

fn monthly_on(start: &str) -> Value {
	json!({"start": start, "frequency": "monthly", "interval": 1, "endMode": "never"})
}

/// A schedule the way Actual stores one: a rule matching account, date and
/// amount, setting the category, plus the schedule row.
fn schedule<'a>(
	book: &'a mut Book,
	id: &str,
	date: Value,
	amount: Value,
	category: Option<&str>,
	next: &str,
) -> &'a mut Schedule {
	let conditions = json!([
		condition("account", "is", json!("CHK")),
		condition("date", "isapprox", date),
		amount,
	]);
	let actions = match category {
		Some(category) => {
			json!([{"op": "set", "field": "category", "value": category, "type": "id"}])
		}
		None => json!([]),
	};
	let rule = RuleId::new(format!("rule-{id}"));
	book.budget.rules.insert(
		rule.clone(),
		Rule {
			id: rule.clone(),
			stage: RuleStage::Default,
			conditions_op: ConditionsOp::And,
			conditions_json: conditions.to_string(),
			actions_json: actions.to_string(),
			tombstone: false,
		},
	);
	book.budget.schedules.insert(
		ScheduleId::new(id),
		Schedule {
			id: ScheduleId::new(id),
			name: Some(id.to_owned()),
			rule,
			posts_transaction: false,
			completed: false,
			next_date: Some(date_of(next)),
			tombstone: false,
		},
	);
	book.budget.schedules.get_mut(&ScheduleId::new(id)).unwrap()
}

fn date_of(text: &str) -> veritas_domain::CivilDate {
	date(text)
}

fn flows(book: &Book) -> ScheduledFlows {
	let forecaster = Forecaster::new(&book.budget).unwrap();
	let through = Forecaster::horizon_end(date(TODAY)).unwrap();
	forecaster.scheduled_flows(date(TODAY), through).unwrap()
}

fn of<'a>(flows: &'a ScheduledFlows, name: &str) -> Vec<&'a veritas_forecast::KnownFlow> {
	flows
		.flows
		.iter()
		.filter(|flow| flow.name == name)
		.collect()
}

#[test]
fn fc_2_exact_bill_repeats_monthly_through_the_horizon() {
	let mut book = Book::household();
	schedule(
		&mut book,
		"Rent",
		monthly_on("2026-06-01"),
		condition("amount", "is", json!(-165000)),
		Some("Rent"),
		"2027-02-01",
	);
	let result = flows(&book);
	let rent = of(&result, "Rent");
	assert_eq!(rent.len(), 12);
	assert_eq!(
		(rent[0].date, rent[11].date),
		(date("2027-02-01"), date("2028-01-01"))
	);
	assert!(rent.iter().all(|flow| flow.amount == cents(-165000)
		&& flow.kind == FlowKind::Expense
		&& flow.category == Some(cat("Rent"))
		&& flow.certainty == Certainty::Exact
		&& flow.account == acct("CHK")));
	assert!(result.unforecast.is_empty());
}

#[test]
fn r_bill_5_biweekly_paycheck_is_income() {
	let mut book = Book::household();
	schedule(
		&mut book,
		"Pay",
		json!({"start": "2027-01-01", "frequency": "weekly", "interval": 2, "endMode": "never"}),
		condition("amount", "is", json!(150000)),
		Some("Paycheck"),
		"2027-01-29",
	);
	let result = flows(&book);
	let pay = of(&result, "Pay");
	assert!(pay.iter().all(|flow| flow.kind == FlowKind::Income));
	assert_eq!(pay[0].date, date("2027-01-29"));
	let in_2027 = pay.iter().filter(|flow| flow.date.year() == 2027).count();
	assert_eq!(
		in_2027, 25,
		"27 in the year, minus Jan 1 and 15 already paid"
	);
}

#[test]
fn fc_2_estimates_use_linked_payments_and_midpoints() {
	let mut book = Book::household();
	for (month, amount) in [
		("2026-10", -9000),
		("2026-11", -9100),
		("2026-12", -9000),
		("2027-01", -9100),
	] {
		book.spend("Electric", &format!("{month}-05"), amount)
			.bill("Electric");
	}
	schedule(
		&mut book,
		"Electric",
		monthly_on("2026-10-05"),
		condition("amount", "isapprox", json!(-8000)),
		Some("Electric"),
		"2027-02-05",
	);
	schedule(
		&mut book,
		"Water",
		monthly_on("2026-10-20"),
		condition("amount", "isbetween", json!({"num1": -3000, "num2": -5001})),
		Some("Electric"),
		"2027-01-20",
	);
	let result = flows(&book);
	let electric = of(&result, "Electric");
	assert_eq!(electric[0].amount, cents(-9050));
	assert_eq!(electric[0].certainty, Certainty::Estimate);
	let water = of(&result, "Water");
	assert_eq!(water[0].date, date("2027-01-20"));
	assert_eq!(water[0].amount, cents(-4001));
}

#[test]
fn fc_2_card_payment_schedules_are_transfers() {
	let mut book = Book::household();
	let card = schedule(
		&mut book,
		"Card",
		monthly_on("2026-12-28"),
		condition("amount", "is", json!(-60000)),
		None,
		"2027-01-28",
	)
	.rule
	.clone();
	let rule = book.budget.rules.get_mut(&card).unwrap();
	let mut conditions: Value = serde_json::from_str(&rule.conditions_json).unwrap();
	conditions
		.as_array_mut()
		.unwrap()
		.push(condition("payee", "is", json!("payee-VISA")));
	rule.conditions_json = conditions.to_string();
	let result = flows(&book);
	assert_eq!(
		of(&result, "Card")[0].kind,
		FlowKind::Transfer {
			counterpart: acct("VISA")
		}
	);
}

#[test]
fn fc_3_overdue_bills_count_now_and_unreadable_schedules_are_reported() {
	let mut book = Book::household();
	schedule(
		&mut book,
		"Late",
		monthly_on("2026-11-10"),
		condition("amount", "is", json!(-2000)),
		Some("Fun"),
		"2027-01-10",
	);
	schedule(
		&mut book,
		"Done",
		monthly_on("2026-11-10"),
		condition("amount", "is", json!(-2000)),
		Some("Fun"),
		"2027-02-10",
	)
	.completed = true;
	schedule(
		&mut book,
		"Vague",
		json!("2027-03"),
		condition("amount", "is", json!(-2000)),
		Some("Fun"),
		"2027-03-01",
	);
	let broken = schedule(
		&mut book,
		"Broken",
		monthly_on("2026-11-10"),
		condition("amount", "is", json!(-2000)),
		None,
		"2027-02-10",
	)
	.rule
	.clone();
	book.budget.rules.get_mut(&broken).unwrap().conditions_json = "not json".to_owned();
	let result = flows(&book);
	let late: Vec<_> = of(&result, "Late").iter().map(|flow| flow.date).collect();
	assert_eq!(late[..2], [date("2027-01-10"), date("2027-02-10")]);
	let projection = Forecaster::new(&book.budget)
		.unwrap()
		.project(date(TODAY), &result.flows, None)
		.unwrap();
	assert_eq!(projection.months[0].known_net_income, cents(-2000));
	assert_eq!(projection.overdue[0].date, date("2027-01-10"));
	assert!(of(&result, "Done").is_empty());
	assert!(of(&result, "Vague").is_empty());
	let unforecast: Vec<_> = result
		.unforecast
		.iter()
		.map(|item| item.schedule.clone().unwrap())
		.collect();
	assert_eq!(
		unforecast,
		[ScheduleId::new("Broken"), ScheduleId::new("Vague")]
	);
}

fn loan_book() -> Book {
	let mut book = Book::household();
	book.opening("CHK", "2026-11-30", 500000);
	book.opening("LOAN", "2026-11-30", -1012000);
	book.row_with_id("pay", "CHK", "2026-12-10", -40000).split = SplitRole::Parent;
	book.transfer("CHK", "LOAN", "2026-12-10", -32000)
		.in_category("Loan principal")
		.child_of("pay");
	book.spend("Loan interest", "2026-12-10", -8000)
		.child_of("pay");
	book.budget.account_meta.insert(
		acct("LOAN"),
		AccountMeta {
			kind: Some(AccountKind::Liability),
			loan_rate_bps: Some(980),
			loan_payment: Some(cents(40000)),
			loan_payment_day: Some(10),
			lender: None,
		},
	);
	book
}

#[test]
fn fc_2_loan_payments_come_from_loan_metadata() {
	let book = loan_book();
	let result = flows(&book);
	let loan = of(&result, "LOAN");
	assert_eq!(loan.len(), 24, "interest and principal for 12 months");
	assert_eq!(loan[0].date, date("2027-02-10"));
	assert_eq!(
		(loan[0].amount, loan[0].kind.clone()),
		(cents(-8003), FlowKind::Expense)
	);
	assert_eq!(loan[0].category, Some(cat("Loan interest")));
	assert_eq!(
		(
			loan[1].amount,
			loan[1].kind.clone(),
			loan[1].category.clone()
		),
		(
			cents(-31997),
			FlowKind::Transfer {
				counterpart: acct("LOAN")
			},
			Some(cat("Loan principal"))
		)
	);

	let projection = Forecaster::new(&book.budget)
		.unwrap()
		.project(date(TODAY), &result.flows, None)
		.unwrap();
	assert_eq!(projection.months[1].balances[&acct("LOAN")], cents(-948003));
}

#[test]
fn fc_2_a_loan_without_a_recorded_payment_is_reported() {
	let mut book = Book::household();
	book.budget.account_meta.insert(
		acct("LOAN"),
		AccountMeta {
			kind: Some(AccountKind::Liability),
			loan_rate_bps: None,
			loan_payment: Some(cents(40000)),
			loan_payment_day: Some(10),
			lender: None,
		},
	);
	let result = flows(&book);
	assert!(result.flows.is_empty());
	assert_eq!(result.unforecast[0].loan, Some(acct("LOAN")));
}

#[test]
fn fc_3_worked_example_from_the_budgets_own_schedules() {
	let mut book = Book::household();
	book.opening("CHK", "2026-06-30", 531800);
	for month in [
		"2026-07", "2026-08", "2026-09", "2026-10", "2026-11", "2026-12",
	] {
		book.spend("Groceries", &format!("{month}-15"), -43000);
	}
	book.spend("Groceries", "2027-01-10", -20000);
	book.spend("Rent", "2027-01-01", -165000).bill("Rent");
	book.spend("Paycheck", "2027-01-01", 300000).bill("Pay");
	schedule(
		&mut book,
		"Rent",
		monthly_on("2026-06-01"),
		condition("amount", "is", json!(-165000)),
		Some("Rent"),
		"2027-02-01",
	);
	schedule(
		&mut book,
		"Pay",
		monthly_on("2026-06-01"),
		condition("amount", "is", json!(300000)),
		Some("Paycheck"),
		"2027-02-01",
	);
	let forecaster = Forecaster::new(&book.budget).unwrap();
	let through = Forecaster::horizon_end(date(TODAY)).unwrap();
	let known = forecaster.scheduled_flows(date(TODAY), through).unwrap();
	let projection = forecaster.project(date(TODAY), &known.flows, None).unwrap();
	assert_eq!(projection.months[0].net_worth, cents(365800));
	assert_eq!(projection.months[1].net_worth, cents(457800));
	assert!(
		projection
			.baselines
			.iter()
			.all(|baseline| baseline.category != cat("Paycheck"))
	);
}

#[test]
fn fc_2_loan_escrow_repeats_from_the_last_payment() {
	let mut book = Book::household();
	book.opening("CHK", "2026-11-30", 500000);
	book.opening("LOAN", "2026-11-30", -1012000);
	book.row_with_id("pay", "CHK", "2026-12-10", -90270).split = SplitRole::Parent;
	book.transfer("CHK", "LOAN", "2026-12-10", -32000)
		.in_category("Loan principal")
		.child_of("pay");
	book.spend("Loan interest", "2026-12-10", -8270)
		.child_of("pay");
	book.spend("Electric", "2026-12-10", -50000).child_of("pay");
	book.budget.account_meta.insert(
		acct("LOAN"),
		AccountMeta {
			kind: Some(AccountKind::Liability),
			loan_rate_bps: Some(980),
			loan_payment: Some(cents(100000)),
			loan_payment_day: Some(10),
			lender: None,
		},
	);
	let result = flows(&book);
	let february: Vec<_> = of(&result, "LOAN")
		.into_iter()
		.filter(|flow| flow.date == date("2027-02-10"))
		.map(|flow| (flow.amount.minor(), flow.category.clone()))
		.collect();
	assert_eq!(
		february,
		[
			(-8003, Some(cat("Loan interest"))),
			(-50000, Some(cat("Electric"))),
			(-41997, Some(cat("Loan principal"))),
		],
		"FC-2, DEBT-2.1a: interest, then escrow, then principal"
	);
}
