//! R-FC-3 next-month suggestions with their one-line reasons (FC-5) and
//! R-FC-4 / R-GOAL-1 goal ETAs (FC-6, DEBT-8.1).

#![allow(clippy::unwrap_used, clippy::panic)]

mod support;

use std::collections::BTreeMap;

use support::*;
use veritas_domain::{CategoryKind, GoalId};
use veritas_forecast::{
	Basis, Envelope, EtaStatus, FlowKind, Forecaster, KnownFlow, Scenario, ScenarioEvent,
	ScheduleAmount, Suggestion, Suggestions, goal_eta,
};

fn six(category: &str, book: &mut Book, x: i64) {
	let months = [
		"2026-07", "2026-08", "2026-09", "2026-10", "2026-11", "2026-12",
	];
	let series: Vec<(&str, i64)> = months.iter().map(|month| (*month, x)).collect();
	book.monthly(category, &series);
}

fn suggest(book: &Book, today: &str, flows: &[KnownFlow]) -> Suggestions {
	suggest_with(book, today, flows, &BTreeMap::new())
}

fn suggest_with(
	book: &Book,
	today: &str,
	flows: &[KnownFlow],
	envelopes: &BTreeMap<veritas_domain::CategoryId, Envelope>,
) -> Suggestions {
	Forecaster::new(&book.budget)
		.unwrap()
		.suggestions(date(today), flows, envelopes)
		.unwrap()
}

fn item<'a>(suggestions: &'a Suggestions, category: &str) -> &'a Suggestion {
	suggestions
		.items
		.iter()
		.find(|item| item.category == cat(category))
		.unwrap_or_else(|| panic!("no suggestion for {category}"))
}

#[test]
fn fc_5_baseline_only_f_1() {
	let mut book = Book::household();
	let months = [
		"2026-07", "2026-08", "2026-09", "2026-10", "2026-11", "2026-12",
	];
	let values = [42000, 42000, 43000, 43000, 44000, 44000];
	let series: Vec<(&str, i64)> = months.iter().copied().zip(values).collect();
	book.monthly("Groceries", &series);
	let suggestions = suggest(&book, "2027-01-15", &[]);
	assert_eq!(suggestions.month, ym("2027-02"));
	let groceries = item(&suggestions, "Groceries");
	assert_eq!(groceries.amount, Some(cents(43000)));
	assert_eq!(groceries.basis, Basis::Baseline);
	assert_eq!(groceries.reason, "Groceries $430, 6-month median");
}

#[test]
fn fc_5_half_dollar_rounds_up() {
	let mut book = Book::household();
	six("Dining", &mut book, 42950);
	let dining = item(&suggest(&book, "2027-01-15", &[]), "Dining").clone();
	assert_eq!(dining.amount, Some(cents(43000)));
	assert_eq!(dining.reason, "Dining $430, 6-month median");
}

#[test]
fn fc_5_bills_only() {
	let book = Book::household();
	let rent = known("2027-02-01", "Rent", -165000, FlowKind::Expense);
	let suggestions = suggest(&book, "2027-01-15", &[rent]);
	let rent = item(&suggestions, "Rent");
	assert_eq!(rent.amount, Some(cents(165000)));
	assert_eq!(rent.basis, Basis::Bills);
	assert_eq!(rent.reason, "Rent $1650, scheduled: Rent");
}

#[test]
fn fc_5_approximate_bill_uses_the_median_of_its_payments() {
	let mut book = Book::household();
	for (month, amount) in [
		("2026-07", -9000),
		("2026-08", -9100),
		("2026-09", -9000),
		("2026-10", -9100),
		("2026-11", -9000),
		("2026-12", -9100),
	] {
		book.spend("Electric", &format!("{month}-02"), amount)
			.bill("Electric");
	}
	let schedule = veritas_domain::ScheduleId::new("Electric");
	let (amount, _) = ScheduleAmount::IsApprox(cents(-8500))
		.per_occurrence(&book.budget, &schedule)
		.unwrap();
	let electric = known("2027-02-02", "Electric", amount.minor(), FlowKind::Expense);
	let suggestions = suggest(&book, "2027-01-15", &[electric]);
	let electric = item(&suggestions, "Electric");
	assert_eq!(electric.amount, Some(cents(9100)));
	assert_eq!(electric.reason, "Electric $91, scheduled: Electric");
}

#[test]
fn fc_5_bills_plus_baseline_parts_add_up() {
	let mut book = Book::household();
	six("Electric", &mut book, 2040);
	let bill = known("2027-02-02", "Electric", -5070, FlowKind::Expense);
	let suggestions = suggest(&book, "2027-01-15", &[bill]);
	let electric = item(&suggestions, "Electric");
	assert_eq!(electric.amount, Some(cents(7100)));
	assert_eq!(electric.basis, Basis::BillsAndBaseline);
	assert_eq!(
		electric.reason,
		"Electric $71: $51 scheduled + $20 6-month median"
	);
}

#[test]
fn fc_5_seasonal_reason_names_the_adjustment() {
	let mut book = Book::household();
	let mut values: Vec<i64> = vec![5000, 5000, 8000];
	values.extend([5000; 11]);
	for (step, x) in values.iter().enumerate() {
		let month = ym("2026-10").offset(step as i32).unwrap();
		book.spend("Gifts", &format!("{month}-15"), -x);
	}
	let gifts = item(&suggest(&book, "2027-11-20", &[]), "Gifts").clone();
	assert_eq!(gifts.basis, Basis::Seasonal);
	assert_eq!(gifts.amount, Some(cents(6500)));
	assert_eq!(
		gifts.reason,
		"Gifts $65, 6-month median, adjusted +26% for December"
	);
}

#[test]
fn fc_5_no_history_keeps_the_current_budget() {
	let mut book = Book::household();
	book.budgeted("2027-02", "Fun", 7550);
	let fun = item(&suggest(&book, "2027-01-15", &[]), "Fun").clone();
	assert_eq!(fun.amount, None);
	assert_eq!(fun.basis, Basis::NoHistory);
	assert_eq!(fun.reason, "Fun: no history yet, keeping $76");
}

#[test]
fn fc_5_skips_income_hidden_archived_and_savings_categories() {
	let mut book = Book::household();
	book.group("Saving", false)
		.category("Emergency fund", "Saving")
		.kind("Emergency fund", CategoryKind::Savings)
		.hide("Books");
	book.budget
		.category_meta
		.entry(cat("Gifts"))
		.or_insert(veritas_domain::CategoryMeta {
			kind: None,
			tax_bucket: None,
			archived_on: None,
		})
		.archived_on = Some(date("2026-01-01"));
	let suggestions = suggest(&book, "2027-01-15", &[]);
	let names: Vec<&str> = suggestions
		.items
		.iter()
		.map(|item| item.category.as_str())
		.collect();
	for skipped in ["Paycheck", "Side gigs", "Books", "Gifts", "Emergency fund"] {
		assert!(!names.contains(&skipped), "{skipped} should be skipped");
	}
	assert_eq!(
		names,
		[
			"Rent",
			"Electric",
			"Groceries",
			"Dining",
			"Fuel",
			"Fun",
			"Loan interest",
			"Loan principal"
		]
	);
}

#[test]
fn fc_5_notice_when_suggestions_exceed_expected_income() {
	let mut book = Book::household();
	six("Groceries", &mut book, 43000);
	six("Side gigs", &mut book, -20000);
	let rent = known("2027-02-01", "Rent", -165000, FlowKind::Expense);
	let pay = known("2027-02-01", "Paycheck", 180000, FlowKind::Income);
	let suggestions = suggest(&book, "2027-01-15", &[rent, pay]);
	assert_eq!(suggestions.expected_income, cents(200000));
	assert_eq!(suggestions.exceeds_income_by, Some(cents(8000)));
	assert_eq!(
		suggestions.notice().unwrap(),
		"Suggestions exceed expected income by $80"
	);
}

#[test]
fn fc_5_goal_category_uses_the_debt_8_1_suggestion() {
	let mut book = Book::household();
	book.category("New laptop", "Goals");
	book.goal("laptop", "New laptop", 150000, Some("2027-06-15"));
	let suggestions = suggest(&book, "2026-12-10", &[]);
	let laptop = item(&suggestions, "New laptop");
	assert_eq!(laptop.basis, Basis::Goal);
	assert_eq!(laptop.amount, Some(cents(25000)));
	assert_eq!(
		laptop.reason,
		"New laptop $250.00: $1500.00 left over 6 months"
	);

	book.goal("laptop", "New laptop", 150000, Some("2027-07-15"));
	let laptop = item(&suggest(&book, "2026-12-10", &[]), "New laptop").clone();
	assert_eq!(laptop.amount, Some(cents(21429)));

	book.goal("laptop", "New laptop", 100000, Some("2027-03-01"));
	let laptop = item(&suggest(&book, "2026-12-10", &[]), "New laptop").clone();
	assert_eq!(laptop.amount, Some(cents(33334)), "G-2");
}

#[test]
fn fc_5_goal_carry_comes_from_the_envelope_sheet() {
	let mut book = Book::household();
	book.category("New laptop", "Goals");
	book.goal("laptop", "New laptop", 150000, Some("2027-06-15"));
	let envelopes = BTreeMap::from([(
		cat("New laptop"),
		Envelope {
			carry_into_next: cents(25000),
		},
	)]);
	let suggestions = suggest_with(&book, "2027-01-20", &[], &envelopes);
	let laptop = item(&suggestions, "New laptop");
	assert_eq!(laptop.amount, Some(cents(25000)), "G-1b");
	assert_eq!(
		laptop.reason,
		"New laptop $250.00: $1250.00 left over 5 months"
	);

	let reached = BTreeMap::from([(
		cat("New laptop"),
		Envelope {
			carry_into_next: cents(160000),
		},
	)]);
	let laptop = item(
		&suggest_with(&book, "2027-01-20", &[], &reached),
		"New laptop",
	)
	.clone();
	assert_eq!(laptop.amount, Some(cents(0)), "G-4");
}

#[test]
fn fc_5_goal_without_a_target_date_keeps_its_budget() {
	let mut book = Book::household();
	book.category("New laptop", "Goals");
	book.goal("laptop", "New laptop", 150000, None);
	book.budgeted("2027-02", "New laptop", 10000);
	let laptop = item(&suggest(&book, "2027-01-20", &[]), "New laptop").clone();
	assert_eq!(laptop.amount, None);
	assert_eq!(laptop.reason, "New laptop: no target date, keeping $100");
}

#[test]
fn fc_5_debt_principal_envelope_uses_the_last_payment() {
	let mut book = Book::household();
	book.opening("LOAN", "2026-06-30", -980000);
	book.row("CHK", "2026-12-10", -40000).split = veritas_domain::SplitRole::Parent;
	let parent = book
		.budget
		.transactions
		.keys()
		.last()
		.unwrap()
		.as_str()
		.to_owned();
	book.transfer("CHK", "LOAN", "2026-12-10", -32050)
		.in_category("Loan principal")
		.child_of(&parent);
	book.spend("Loan interest", "2026-12-10", -7950)
		.child_of(&parent);
	let suggestions = suggest(&book, "2027-01-15", &[]);
	let principal = item(&suggestions, "Loan principal");
	assert_eq!(principal.basis, Basis::LoanPrincipal);
	assert_eq!(principal.amount, Some(cents(32100)));
	assert_eq!(
		principal.reason,
		"Loan principal $321, principal of the last loan payment"
	);
}

fn laptop_book(contributions: &[(&str, i64)]) -> Book {
	let mut book = Book::household();
	book.category("New laptop", "Goals");
	book.goal("laptop", "New laptop", 150000, Some("2027-06-15"));
	for (month, amount) in contributions {
		book.budgeted(month, "New laptop", *amount);
	}
	book
}

fn eta(book: &Book, available: i64, scenario: &Scenario) -> veritas_forecast::GoalEta {
	goal_eta(
		&book.budget,
		&GoalId::new("laptop"),
		ym("2027-01"),
		cents(available),
		scenario,
	)
	.unwrap()
}

#[test]
fn fc_6_e_1_eta_from_the_median_pace() {
	let book = laptop_book(&[("2026-10", 25000), ("2026-11", 25000), ("2026-12", 20000)]);
	let result = eta(&book, 112000, &Scenario::baseline());
	assert_eq!(result.remaining, cents(38000));
	assert_eq!(result.pace.unwrap().0, cents(25000));
	assert!(matches!(result.status, EtaStatus::Eta { month, .. } if month == ym("2027-03")));
	assert_eq!(result.on_track, Some(true));
}

#[test]
fn fc_6_e_2_range_from_optimistic_to_pessimistic() {
	let book = laptop_book(&[("2026-10", 30000), ("2026-11", 20000), ("2026-12", 10000)]);
	let result = eta(&book, 112000, &Scenario::baseline());
	assert_eq!(
		result.status,
		EtaStatus::Eta {
			month: ym("2027-03"),
			optimistic: ym("2027-03"),
			pessimistic: Some(ym("2027-05")),
		}
	);
	assert_eq!(
		result.text(),
		"ETA March 2027, range March 2027 to May 2027"
	);
}

#[test]
fn fc_6_e_3_nothing_being_added() {
	let book = laptop_book(&[("2026-10", 0), ("2026-11", 0), ("2026-12", 0)]);
	let result = eta(&book, 112000, &Scenario::baseline());
	assert_eq!(result.status, EtaStatus::NotAdding);
	assert_eq!(result.text(), "No ETA: nothing is being added");
}

#[test]
fn fc_6_e_4_reached() {
	let book = laptop_book(&[("2026-12", 25000)]);
	let result = eta(&book, 150000, &Scenario::baseline());
	assert_eq!(result.text(), "Reached");
}

#[test]
fn fc_6_e_5_one_month_of_history() {
	let book = laptop_book(&[("2026-12", 25000)]);
	let result = eta(&book, 112000, &Scenario::baseline());
	assert_eq!(
		result.pace,
		Some((cents(25000), cents(25000), cents(25000)))
	);
	assert_eq!(result.history_label(), "1 month of history");
}

#[test]
fn fc_6_e_6_agrees_with_debt_8_1() {
	let book = laptop_book(&[("2026-12", 25000), ("2027-01", 25000)]);
	let result = eta(&book, 25000, &Scenario::baseline());
	assert_eq!(result.remaining, cents(125000));
	assert!(matches!(result.status, EtaStatus::Eta { month, .. } if month == ym("2027-06")));
	assert_eq!(result.on_track, Some(true));
}

#[test]
fn fc_6_no_history_and_slowest_pace() {
	let book = laptop_book(&[]);
	assert_eq!(eta(&book, 0, &Scenario::baseline()).text(), "No ETA yet");
	let stalled = laptop_book(&[("2026-10", 30000), ("2026-11", 20000), ("2026-12", 0)]);
	let result = eta(&stalled, 112000, &Scenario::baseline());
	assert_eq!(result.text(), "ETA March 2027, no ETA at the slowest pace");
}

#[test]
fn fc_6_off_track_when_the_eta_passes_the_target() {
	let book = laptop_book(&[("2026-12", 10000)]);
	let result = eta(&book, 0, &Scenario::baseline());
	assert!(matches!(result.status, EtaStatus::Eta { month, .. } if month == ym("2028-04")));
	assert_eq!(result.on_track, Some(false));
}

#[test]
fn r_fc_2_goal_contribution_scenario_moves_the_eta() {
	let book = laptop_book(&[("2026-12", 10000)]);
	let bounded = Scenario {
		name: "Bonus".to_owned(),
		events: vec![ScenarioEvent::GoalContribution {
			goal: GoalId::new("laptop"),
			amount: cents(40000),
			start_month: ym("2027-02"),
			end_month: Some(ym("2027-03")),
		}],
	};
	let result = eta(&book, 0, &bounded);
	assert!(matches!(result.status, EtaStatus::Eta { month, .. } if month == ym("2027-08")));

	let open = Scenario {
		name: "Steady".to_owned(),
		events: vec![ScenarioEvent::GoalContribution {
			goal: GoalId::new("laptop"),
			amount: cents(15000),
			start_month: ym("2027-02"),
			end_month: None,
		}],
	};
	let result = eta(&book, 0, &open);
	assert!(matches!(result.status, EtaStatus::Eta { month, .. } if month == ym("2027-07")));
}
