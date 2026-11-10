//! R-FC-1 projection and R-FC-2 scenarios: the FC-3 worked example, the
//! horizons with their FC-4 band, known-flow handling and every overlay kind.

#![allow(clippy::unwrap_used, clippy::panic)]

mod support;

use support::*;
use veritas_domain::{CoreError, GoalId, ScheduleId};
use veritas_forecast::{
	BandMethod, Cadence, Certainty, FlowKind, Forecaster, IncomeDelta, KnownFlow, LoanPayment,
	Projection, Scenario, ScenarioEvent, ScheduleAmount,
};

const TODAY: &str = "2027-01-15";

/// FC-3's worked example: checking holds 388800 on 2027-01-15 after rent
/// and pay posted on the 1st and 20000 of groceries. January's occurrences
/// are posted, so the known flows start in February.
fn fc_3_book() -> Book {
	let mut book = Book::household();
	book.opening("CHK", "2026-06-30", 531800);
	book.monthly(
		"Groceries",
		&[
			("2026-07", 43000),
			("2026-08", 43000),
			("2026-09", 43000),
			("2026-10", 43000),
			("2026-11", 43000),
			("2026-12", 43000),
		],
	);
	book.spend("Groceries", "2027-01-10", -20000);
	book.spend("Rent", "2027-01-01", -165000).bill("Rent");
	book.spend("Paycheck", "2027-01-01", 300000)
		.bill("Paycheck");
	book
}

fn fc_3_flows() -> Vec<KnownFlow> {
	let mut flows = monthly_flows("2027-02", 12, "Rent", -165000, FlowKind::Expense);
	flows.extend(monthly_flows(
		"2027-02",
		12,
		"Paycheck",
		300000,
		FlowKind::Income,
	));
	flows
}

fn project(book: &Book, flows: &[KnownFlow], scenario: &Scenario) -> Projection {
	Forecaster::new(&book.budget)
		.unwrap()
		.project(date(TODAY), flows, scenario)
		.unwrap()
}

#[test]
fn fc_3_worked_example_single_account() {
	let book = fc_3_book();
	let projection = project(&book, &fc_3_flows(), &Scenario::baseline());
	assert_eq!(projection.net_worth_today, cents(388800));
	let january = &projection.months[0];
	assert_eq!(january.uncertain_net_flow, cents(-23000));
	assert_eq!(
		january.known_net_income,
		cents(0),
		"January's occurrences are posted"
	);
	assert_eq!(january.net_worth, cents(365800));
	let february = &projection.months[1];
	assert_eq!(february.known_net_income, cents(135000));
	assert_eq!(february.uncertain_net_flow, cents(-43000));
	assert_eq!(february.net_income, cents(92000));
	assert_eq!(february.net_worth, cents(457800));
	assert_eq!(february.balances[&acct("CHK")], cents(457800));
}

#[test]
fn r_fc_1_horizons_at_3_6_10_and_12_months() {
	let book = fc_3_book();
	let projection = project(&book, &fc_3_flows(), &Scenario::baseline());
	let horizons: Vec<_> = projection
		.horizons
		.iter()
		.map(|horizon| {
			(
				horizon.months,
				horizon.month.to_string(),
				horizon.net_worth.minor(),
			)
		})
		.collect();
	assert_eq!(
		horizons,
		[
			(3, "2027-04".to_owned(), 365800 + 3 * 92000),
			(6, "2027-07".to_owned(), 365800 + 6 * 92000),
			(10, "2027-11".to_owned(), 365800 + 10 * 92000),
			(12, "2028-01".to_owned(), 365800 + 12 * 92000),
		]
	);
}

#[test]
fn r_fc_1_band_is_net_worth_plus_scaled_offsets() {
	let mut book = Book::household();
	book.opening("CHK", "2025-12-31", 1_000_000);
	let values: [i64; 12] = [
		41000, 45000, 43000, 39000, 47000, 43000, 42000, 44000, 40000, 46000, 43000, 43000,
	];
	for (step, x) in values.iter().enumerate() {
		let month = ym("2026-01").offset(step as i32).unwrap();
		book.spend("Groceries", &format!("{month}-15"), -x);
	}
	let projection = project(&book, &[], &Scenario::baseline());
	assert_eq!(projection.band.method, BandMethod::Residuals);
	assert_eq!(projection.band.months, 9);
	for horizon in &projection.horizons {
		let (low, high) = projection.band.offsets(horizon.months).unwrap().unwrap();
		let band = horizon.band.unwrap();
		assert_eq!(band.0.minor(), horizon.net_worth.minor() + low.minor());
		assert_eq!(band.1.minor(), horizon.net_worth.minor() + high.minor());
		assert!(band.0 <= horizon.net_worth && horizon.net_worth <= band.1);
	}
}

#[test]
fn r_fc_1_no_history_means_no_band() {
	let mut book = Book::household();
	book.opening("CHK", "2027-01-02", 50000);
	let projection = project(&book, &[], &Scenario::baseline());
	assert_eq!(projection.band.label(), "No history yet");
	assert!(
		projection
			.horizons
			.iter()
			.all(|horizon| horizon.band.is_none())
	);
}

#[test]
fn fc_3_transfers_move_balances_not_net_worth() {
	let book = fc_3_book();
	let card = KnownFlow {
		date: date("2027-02-20"),
		account: acct("CHK"),
		amount: cents(-50000),
		kind: FlowKind::Transfer {
			counterpart: acct("VISA"),
		},
		category: None,
		schedule: None,
		name: "Card".to_owned(),
		certainty: Certainty::Exact,
	};
	let mut flows = fc_3_flows();
	flows.push(card);
	let projection = project(&book, &flows, &Scenario::baseline());
	let february = &projection.months[1];
	assert_eq!(february.net_worth, cents(457800));
	assert_eq!(february.balances[&acct("CHK")], cents(407800));
	assert_eq!(february.balances[&acct("VISA")], cents(50000));
}

#[test]
fn fc_2_loan_payments_split_interest_and_principal() {
	let loan = LoanPayment {
		from: acct("CHK"),
		loan: acct("LOAN"),
		name: "Car loan".to_owned(),
		payment: cents(40000),
		rate_bps: Some(980),
		last_interest: cents(0),
		outstanding: cents(980000),
		interest_category: Some(cat("Loan interest")),
		principal_category: Some(cat("Loan principal")),
	};
	let flows = loan
		.flows(&[date("2027-02-10"), date("2027-03-10")])
		.unwrap();
	let parts: Vec<_> = flows
		.iter()
		.map(|flow| (flow.amount.minor(), flow.kind.clone()))
		.collect();
	let principal = FlowKind::Transfer {
		counterpart: acct("LOAN"),
	};
	assert_eq!(
		parts,
		[
			(-8003, FlowKind::Expense),
			(-31997, principal.clone()),
			(-7742, FlowKind::Expense),
			(-32258, principal),
		]
	);

	let mut book = Book::household();
	book.opening("CHK", "2026-12-31", 100000);
	book.opening("LOAN", "2026-12-31", -980000);
	let projection = project(&book, &flows, &Scenario::baseline());
	assert_eq!(projection.months[1].net_income, cents(-8003));
	assert_eq!(projection.months[1].balances[&acct("LOAN")], cents(-948003));
}

#[test]
fn fc_2_final_loan_payment_is_capped_at_what_is_owed() {
	let loan = LoanPayment {
		from: acct("CHK"),
		loan: acct("LOAN"),
		name: "Car loan".to_owned(),
		payment: cents(40000),
		rate_bps: None,
		last_interest: cents(100),
		outstanding: cents(20000),
		interest_category: None,
		principal_category: None,
	};
	let flows = loan
		.flows(&[date("2027-02-10"), date("2027-03-10")])
		.unwrap();
	let amounts: Vec<i64> = flows.iter().map(|flow| flow.amount.minor()).collect();
	assert_eq!(amounts, [-100, -20000]);
}

#[test]
fn fc_2_amount_per_occurrence() {
	let mut book = Book::household();
	for (month, amount) in [
		("2026-07", -9000),
		("2026-08", -9100),
		("2026-09", -9000),
		("2026-10", -9100),
		("2026-11", -9000),
		("2026-12", -9100),
		("2026-01", -50000),
	] {
		book.spend("Electric", &format!("{month}-02"), amount)
			.bill("electric");
	}
	let electric = ScheduleId::new("electric");
	let approx = ScheduleAmount::IsApprox(cents(-8000))
		.per_occurrence(&book.budget, &electric)
		.unwrap();
	assert_eq!(approx, (cents(-9050), Certainty::Estimate));
	let unlinked = ScheduleAmount::IsApprox(cents(-8000))
		.per_occurrence(&book.budget, &ScheduleId::new("water"))
		.unwrap();
	assert_eq!(unlinked.0, cents(-8000));
	let between = ScheduleAmount::IsBetween(cents(-9001), cents(-11000))
		.per_occurrence(&book.budget, &electric)
		.unwrap();
	assert_eq!(between.0, cents(-10001));
	let exact = ScheduleAmount::Is(cents(-165000))
		.per_occurrence(&book.budget, &electric)
		.unwrap();
	assert_eq!(exact, (cents(-165000), Certainty::Exact));
}

#[test]
fn fc_2_lists_categories_that_may_be_double_counted() {
	let book = fc_3_book();
	let mut flows = fc_3_flows();
	flows.push(known("2027-02-03", "Groceries", -50000, FlowKind::Expense));
	let projection = project(&book, &flows, &Scenario::baseline());
	assert_eq!(projection.possible_double_counts, [cat("Groceries")]);
}

#[test]
fn capitalized_transfers_in_the_window_are_counted_for_the_note() {
	let mut book = fc_3_book();
	book.transfer("CHK", "SAV", "2026-11-20", -10000)
		.in_category("Gifts");
	let projection = project(&book, &fc_3_flows(), &Scenario::baseline());
	assert_eq!(projection.capitalized_transfers, 1);
}

#[test]
fn r_fc_2_recurrence_is_anchored_on_the_start() {
	let year_end = date("2027-12-31");
	let biweekly = Cadence::EveryWeeks(2)
		.dates(date("2027-01-01"), year_end)
		.unwrap();
	assert_eq!(biweekly.len(), 27);
	let from_jan_8 = Cadence::EveryWeeks(2)
		.dates(date("2027-01-08"), year_end)
		.unwrap();
	assert_eq!(from_jan_8.len(), 26);
	let monthly = Cadence::EveryMonths(1)
		.dates(date("2027-01-31"), year_end)
		.unwrap();
	assert_eq!(monthly.len(), 12);
	assert_eq!(monthly[1], date("2027-02-28"));
	assert_eq!(monthly[2], date("2027-03-31"));
	let quarterly = Cadence::EveryMonths(3)
		.dates(date("2027-01-15"), year_end)
		.unwrap();
	assert_eq!(quarterly.len(), 4);
}

#[test]
fn r_fc_2_one_time_and_recurring_events() {
	let book = fc_3_book();
	let flows = fc_3_flows();
	let baseline = project(&book, &flows, &Scenario::baseline());
	let scenario = Scenario {
		name: "Car and side job".to_owned(),
		events: vec![
			ScenarioEvent::OneTime {
				date: date("2027-03-10"),
				account: acct("CHK"),
				category: None,
				amount: cents(-2_000_000),
			},
			ScenarioEvent::Recurring {
				start: date("2027-01-22"),
				end_month: Some(ym("2027-02")),
				cadence: Cadence::EveryWeeks(2),
				account: acct("CHK"),
				category: Some(cat("Side gigs")),
				amount: cents(10000),
			},
		],
	};
	let projection = project(&book, &flows, &scenario);
	let scenario_net: Vec<i64> = projection
		.months
		.iter()
		.take(4)
		.map(|month| month.scenario_net_income.minor())
		.collect();
	assert_eq!(scenario_net, [10000, 20000, -2_000_000, 0]);
	let gap = baseline.horizons[3].net_worth.minor() - projection.horizons[3].net_worth.minor();
	assert_eq!(gap, 2_000_000 - 30000);
}

#[test]
fn r_fc_2_category_percent_change_scales_the_baseline() {
	let book = fc_3_book();
	let scenario = Scenario {
		name: "Cook more".to_owned(),
		events: vec![ScenarioEvent::CategoryPercent {
			category: cat("Groceries"),
			percent_bp: -1000,
			start_month: ym("2027-02"),
			end_month: Some(ym("2027-03")),
		}],
	};
	let projection = project(&book, &fc_3_flows(), &scenario);
	let uncertain: Vec<i64> = projection
		.months
		.iter()
		.take(5)
		.map(|month| month.uncertain_net_flow.minor())
		.collect();
	assert_eq!(uncertain, [-23000, -38700, -38700, -43000, -43000]);
}

#[test]
fn r_fc_2_income_changes_by_amount_and_percent() {
	let book = fc_3_book();
	let scenario = Scenario {
		name: "Raise".to_owned(),
		events: vec![
			ScenarioEvent::IncomeChange {
				account: acct("CHK"),
				category: Some(cat("Paycheck")),
				change: IncomeDelta::Percent(1000),
				start_month: ym("2027-02"),
				end_month: None,
			},
			ScenarioEvent::IncomeChange {
				account: acct("CHK"),
				category: None,
				change: IncomeDelta::Amount(cents(5000)),
				start_month: ym("2027-04"),
				end_month: Some(ym("2027-04")),
			},
		],
	};
	let projection = project(&book, &fc_3_flows(), &scenario);
	let scenario_net: Vec<i64> = projection
		.months
		.iter()
		.take(5)
		.map(|month| month.scenario_net_income.minor())
		.collect();
	assert_eq!(scenario_net, [0, 30000, 30000, 35000, 30000]);
}

#[test]
fn r_fc_2_goal_contributions_never_move_money() {
	let book = fc_3_book();
	let flows = fc_3_flows();
	let baseline = project(&book, &flows, &Scenario::baseline());
	let scenario = Scenario {
		name: "Save for a laptop".to_owned(),
		events: vec![ScenarioEvent::GoalContribution {
			goal: GoalId::new("laptop"),
			amount: cents(25000),
			start_month: ym("2027-02"),
			end_month: None,
		}],
	};
	let projection = project(&book, &flows, &scenario);
	assert_eq!(projection.months, baseline.months);
	assert_eq!(projection.horizons, baseline.horizons);
}

#[test]
fn r_fc_2_compares_up_to_three_scenarios() {
	let book = fc_3_book();
	let forecaster = Forecaster::new(&book.budget).unwrap();
	let scenarios: Vec<Scenario> = (0..4)
		.map(|step| Scenario {
			name: format!("Scenario {step}"),
			events: Vec::new(),
		})
		.collect();
	let three = forecaster
		.compare(date(TODAY), &fc_3_flows(), &scenarios[..3])
		.unwrap();
	assert_eq!(three.scenarios.len(), 3);
	assert_eq!(three.scenarios[2].scenario, "Scenario 2");
	assert!(matches!(
		forecaster.compare(date(TODAY), &fc_3_flows(), &scenarios),
		Err(CoreError::Invalid(_))
	));
}

#[test]
fn r_fc_2_rejects_a_change_that_ends_before_it_starts() {
	let book = fc_3_book();
	let scenario = Scenario {
		name: "Backwards".to_owned(),
		events: vec![ScenarioEvent::CategoryPercent {
			category: cat("Groceries"),
			percent_bp: 500,
			start_month: ym("2027-05"),
			end_month: Some(ym("2027-04")),
		}],
	};
	let result =
		Forecaster::new(&book.budget)
			.unwrap()
			.project(date(TODAY), &fc_3_flows(), &scenario);
	assert!(matches!(result, Err(CoreError::Invalid(_))));
}

#[test]
fn fc_3_unpaid_flows_due_today_or_overdue_count_now() {
	let book = fc_3_book();
	let baseline = project(&book, &fc_3_flows(), &Scenario::baseline());
	let mut flows = fc_3_flows();
	flows.push(known("2027-01-14", "Electric", -12000, FlowKind::Expense));
	flows.push(known("2027-01-15", "Fun", -3000, FlowKind::Expense));
	flows.push(known("2026-12-20", "Side gigs", 5000, FlowKind::Income));
	let projection = project(&book, &flows, &Scenario::baseline());
	assert_eq!(
		projection.months[0].net_worth.minor(),
		baseline.months[0].net_worth.minor() - 12000 - 3000 + 5000
	);
	assert_eq!(
		projection.months[0].balances[&acct("CHK")],
		cents(365800 - 10000)
	);
	let overdue: Vec<&str> = projection
		.overdue
		.iter()
		.map(|flow| flow.name.as_str())
		.collect();
	assert_eq!(overdue, ["Electric", "Side gigs"]);
}

#[test]
fn r_fc_2_what_ifs_count_from_today_and_reject_the_past() {
	let book = fc_3_book();
	let today = Scenario {
		name: "Today".to_owned(),
		events: vec![
			ScenarioEvent::OneTime {
				date: date(TODAY),
				account: acct("CHK"),
				category: None,
				amount: cents(-7000),
			},
			ScenarioEvent::Recurring {
				start: date("2026-12-01"),
				end_month: None,
				cadence: Cadence::EveryWeeks(1),
				account: acct("CHK"),
				category: None,
				amount: cents(100),
			},
		],
	};
	let projection = project(&book, &fc_3_flows(), &today);
	assert_eq!(
		projection.months[0].scenario_net_income,
		cents(-7000 + 2 * 100)
	);
	let past = Scenario {
		name: "Past".to_owned(),
		events: vec![ScenarioEvent::OneTime {
			date: date("2027-01-14"),
			account: acct("CHK"),
			category: None,
			amount: cents(-7000),
		}],
	};
	let result = Forecaster::new(&book.budget)
		.unwrap()
		.project(date(TODAY), &fc_3_flows(), &past);
	assert!(matches!(result, Err(CoreError::Invalid(_))));
}

#[test]
fn fc_2_interest_above_the_payment_capitalizes_on_the_loan() {
	let loan = LoanPayment {
		from: acct("CHK"),
		loan: acct("LOAN"),
		name: "Student loan".to_owned(),
		payment: cents(10000),
		rate_bps: Some(1000),
		last_interest: cents(0),
		outstanding: cents(1_500_000),
		interest_category: Some(cat("Loan interest")),
		principal_category: Some(cat("Loan principal")),
	};
	let flows = loan.flows(&[date("2027-02-10")]).unwrap();
	let parts: Vec<_> = flows
		.iter()
		.map(|flow| {
			(
				flow.account.as_str(),
				flow.amount.minor(),
				flow.kind.clone(),
			)
		})
		.collect();
	assert_eq!(
		parts,
		[
			("CHK", -10000, FlowKind::Expense),
			("LOAN", -2500, FlowKind::Expense)
		]
	);
	assert_eq!(flows[1].name, "Student loan: capitalized interest");

	let mut book = Book::household();
	book.opening("CHK", "2026-12-31", 100000);
	book.opening("LOAN", "2026-12-31", -1_500_000);
	let projection = project(&book, &flows, &Scenario::baseline());
	let february = &projection.months[1];
	assert_eq!(february.net_income, cents(-12500));
	assert_eq!(february.balances[&acct("CHK")], cents(90000));
	assert_eq!(february.balances[&acct("LOAN")], cents(-1_502_500));
}

#[test]
fn fc_2_a_negative_loan_payment_is_rejected_not_wrapped() {
	let loan = LoanPayment {
		from: acct("CHK"),
		loan: acct("LOAN"),
		name: "Odd".to_owned(),
		payment: cents(i64::MIN),
		rate_bps: Some(980),
		last_interest: cents(0),
		outstanding: cents(980000),
		interest_category: None,
		principal_category: None,
	};
	assert!(matches!(
		loan.flows(&[date("2027-02-10")]),
		Err(CoreError::Invalid(_))
	));
}

#[test]
fn fc_1_an_uncategorized_income_schedule_feeds_its_posted_category() {
	let mut book = Book::household();
	for month in [
		"2026-07", "2026-08", "2026-09", "2026-10", "2026-11", "2026-12",
	] {
		book.spend("Paycheck", &format!("{month}-01"), 300000)
			.bill("pay");
	}
	let mut flow = known("2027-02-01", "Paycheck", 300000, FlowKind::Income);
	flow.category = None;
	flow.schedule = Some(ScheduleId::new("pay"));
	let projection = project(&book, &[flow], &Scenario::baseline());
	assert_eq!(projection.months[1].net_income, cents(300000));
	assert!(
		projection
			.baselines
			.iter()
			.all(|baseline| baseline.category != cat("Paycheck"))
	);
}

#[test]
fn fc_2_warns_when_an_income_baseline_sits_next_to_an_income_flow() {
	let mut book = Book::household();
	for month in [
		"2026-07", "2026-08", "2026-09", "2026-10", "2026-11", "2026-12",
	] {
		book.spend("Paycheck", &format!("{month}-01"), 300000);
	}
	let mut flow = known("2027-02-01", "Paycheck", 300000, FlowKind::Income);
	flow.category = None;
	flow.schedule = Some(ScheduleId::new("new pay"));
	let projection = project(&book, &[flow], &Scenario::baseline());
	assert_eq!(projection.possible_double_counts, [cat("Paycheck")]);
}
