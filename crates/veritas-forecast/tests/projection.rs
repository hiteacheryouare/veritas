//! R-FC-1 projection and R-FC-2 scenarios: the FC-3 worked example, the
//! horizons with their FC-4 band, known-flow handling and every overlay kind.

#![allow(clippy::unwrap_used, clippy::panic)]

mod support;

use support::*;
use veritas_domain::{
	Cadence, CoreError, GoalId, IncomeDelta, Scenario, ScenarioChange, ScenarioId, ScheduleId,
};
use veritas_forecast::{
	BandMethod, Certainty, FlowKind, Forecaster, KnownFlow, LoanPayment, Projection,
	ScheduleAmount, cadence_dates,
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

fn project(book: &Book, flows: &[KnownFlow], scenario: Option<&Scenario>) -> Projection {
	Forecaster::new(&book.budget)
		.unwrap()
		.project(date(TODAY), flows, scenario)
		.unwrap()
}

#[test]
fn fc_3_worked_example_single_account() {
	let book = fc_3_book();
	let projection = project(&book, &fc_3_flows(), None);
	assert_eq!(projection.net_worth_today, cents(388800));
	let january = &projection.months[0];
	assert_eq!(january.baselined_net_flow, cents(-23000));
	assert_eq!(
		january.known_net_income,
		cents(0),
		"January's occurrences are posted"
	);
	assert_eq!(january.net_worth, cents(365800));
	let february = &projection.months[1];
	assert_eq!(february.known_net_income, cents(135000));
	assert_eq!(february.baselined_net_flow, cents(-43000));
	assert_eq!(february.net_income, cents(92000));
	assert_eq!(february.net_worth, cents(457800));
	assert_eq!(february.balances[&acct("CHK")], cents(457800));
}

#[test]
fn r_fc_1_horizons_at_3_6_10_and_12_months() {
	let book = fc_3_book();
	let projection = project(&book, &fc_3_flows(), None);
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
	let projection = project(&book, &[], None);
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
	let projection = project(&book, &[], None);
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
	let projection = project(&book, &flows, None);
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
		escrow: cents(0),
		outstanding: cents(980000),
		interest_category: Some(cat("Loan interest")),
		escrow_category: None,
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
	let projection = project(&book, &flows, None);
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
		escrow: cents(0),
		outstanding: cents(20000),
		interest_category: None,
		escrow_category: None,
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
	let projection = project(&book, &flows, None);
	assert_eq!(projection.possible_double_counts, [cat("Groceries")]);
}

#[test]
fn fc_3_ap_1_an_asset_purchase_is_not_spending() {
	let mut book = Book::household();
	book.account("CAR", true);
	book.opening("CHK", "2026-10-31", 1_000_000);
	book.spend("Fuel", "2026-11-10", -20000);
	book.spend("Fuel", "2026-12-10", -20000);
	book.transfer("CHK", "CAR", "2026-12-20", -50000)
		.in_category("Fuel");
	let projection = project(&book, &[], None);
	let fuel = projection
		.baselines
		.iter()
		.find(|baseline| baseline.category == cat("Fuel"))
		.unwrap();
	assert_eq!(fuel.level, cents(20000));
	assert_eq!(projection.months[1].net_income, cents(-20000));
	assert_eq!(projection.asset_purchases, 1);
	assert_eq!(
		projection.asset_purchase_note().as_deref(),
		Some("1 asset purchase excluded: $500")
	);
	assert_eq!(projection.not_forecast_note(), None);
}

#[test]
fn r_fc_2_recurrence_is_anchored_on_the_start() {
	let year_end = date("2027-12-31");
	let biweekly = cadence_dates(Cadence::EveryWeeks(2), date("2027-01-01"), year_end).unwrap();
	assert_eq!(biweekly.len(), 27);
	let from_jan_8 = cadence_dates(Cadence::EveryWeeks(2), date("2027-01-08"), year_end).unwrap();
	assert_eq!(from_jan_8.len(), 26);
	let monthly = cadence_dates(Cadence::EveryMonths(1), date("2027-01-31"), year_end).unwrap();
	// Actual's rschedule skips months without a 31st, as a bill on the 31st does.
	let months: Vec<u32> = monthly.iter().map(|day| day.month()).collect();
	assert_eq!(months, [1, 3, 5, 7, 8, 10, 12]);
	let mid_month = cadence_dates(Cadence::EveryMonths(1), date("2027-01-15"), year_end).unwrap();
	assert_eq!(mid_month.len(), 12);
	let quarterly = cadence_dates(Cadence::EveryMonths(3), date("2027-01-15"), year_end).unwrap();
	assert_eq!(quarterly.len(), 4);
}

#[test]
fn r_fc_2_one_time_and_recurring_events() {
	let book = fc_3_book();
	let flows = fc_3_flows();
	let baseline = project(&book, &flows, None);
	let scenario = scenario(
		"Car and side job",
		vec![
			ScenarioChange::OneTime {
				date: date("2027-03-10"),
				account: acct("CHK"),
				category: None,
				amount: cents(-2_000_000),
			},
			ScenarioChange::Recurring {
				start: date("2027-01-22"),
				end_month: Some(ym("2027-02")),
				cadence: Cadence::EveryWeeks(2),
				account: acct("CHK"),
				category: Some(cat("Side gigs")),
				amount: cents(10000),
			},
		],
	);
	let projection = project(&book, &flows, Some(&scenario));
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
	let scenario = scenario(
		"Cook more",
		vec![ScenarioChange::CategoryPercent {
			category: cat("Groceries"),
			percent_bp: -1000,
			start_month: ym("2027-02"),
			end_month: Some(ym("2027-03")),
		}],
	);
	let projection = project(&book, &fc_3_flows(), Some(&scenario));
	let baselined: Vec<i64> = projection
		.months
		.iter()
		.take(5)
		.map(|month| month.baselined_net_flow.minor())
		.collect();
	assert_eq!(baselined, [-23000, -38700, -38700, -43000, -43000]);
}

#[test]
fn r_fc_2_income_changes_by_amount_and_percent() {
	let book = fc_3_book();
	let scenario = scenario(
		"Raise",
		vec![
			ScenarioChange::IncomeChange {
				account: acct("CHK"),
				category: Some(cat("Paycheck")),
				change: IncomeDelta::Percent(1000),
				start_month: ym("2027-02"),
				end_month: None,
			},
			ScenarioChange::IncomeChange {
				account: acct("CHK"),
				category: None,
				change: IncomeDelta::Amount(cents(5000)),
				start_month: ym("2027-04"),
				end_month: Some(ym("2027-04")),
			},
		],
	);
	let projection = project(&book, &fc_3_flows(), Some(&scenario));
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
	let baseline = project(&book, &flows, None);
	let scenario = scenario(
		"Save for a laptop",
		vec![ScenarioChange::GoalContribution {
			goal: GoalId::new("laptop"),
			amount: cents(25000),
			start_month: ym("2027-02"),
			end_month: None,
		}],
	);
	let projection = project(&book, &flows, Some(&scenario));
	assert_eq!(projection.months, baseline.months);
	assert_eq!(projection.horizons, baseline.horizons);
}

#[test]
fn r_fc_2_compares_up_to_three_scenarios() {
	let book = fc_3_book();
	let forecaster = Forecaster::new(&book.budget).unwrap();
	let stored: Vec<Scenario> = (0..4)
		.map(|step| scenario(&format!("Scenario {step}"), Vec::new()))
		.collect();
	let scenarios: Vec<&Scenario> = stored.iter().collect();
	let three = forecaster
		.compare(date(TODAY), &fc_3_flows(), &scenarios[..3])
		.unwrap();
	assert_eq!(three.scenarios.len(), 3);
	assert_eq!(
		three.scenarios[2].scenario,
		Some(ScenarioId::new("Scenario 2"))
	);
	assert_eq!(three.baseline.scenario, None);
	assert!(matches!(
		forecaster.compare(date(TODAY), &fc_3_flows(), &scenarios),
		Err(CoreError::Invalid(_))
	));
}

#[test]
fn r_fc_2_rejects_a_change_that_ends_before_it_starts() {
	let book = fc_3_book();
	let scenario = scenario(
		"Backwards",
		vec![ScenarioChange::CategoryPercent {
			category: cat("Groceries"),
			percent_bp: 500,
			start_month: ym("2027-05"),
			end_month: Some(ym("2027-04")),
		}],
	);
	let result =
		Forecaster::new(&book.budget)
			.unwrap()
			.project(date(TODAY), &fc_3_flows(), Some(&scenario));
	assert!(matches!(result, Err(CoreError::Invalid(_))));
}

#[test]
fn fc_3_unpaid_flows_due_today_or_overdue_count_now() {
	let book = fc_3_book();
	let baseline = project(&book, &fc_3_flows(), None);
	let mut flows = fc_3_flows();
	flows.push(known("2027-01-14", "Electric", -12000, FlowKind::Expense));
	flows.push(known("2027-01-15", "Fun", -3000, FlowKind::Expense));
	flows.push(known("2026-12-20", "Side gigs", 5000, FlowKind::Income));
	let projection = project(&book, &flows, None);
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
	let today = scenario(
		"Today",
		vec![
			ScenarioChange::OneTime {
				date: date(TODAY),
				account: acct("CHK"),
				category: None,
				amount: cents(-7000),
			},
			ScenarioChange::Recurring {
				start: date("2026-12-01"),
				end_month: None,
				cadence: Cadence::EveryWeeks(1),
				account: acct("CHK"),
				category: None,
				amount: cents(100),
			},
		],
	);
	let projection = project(&book, &fc_3_flows(), Some(&today));
	assert_eq!(
		projection.months[0].scenario_net_income,
		cents(-7000 + 2 * 100)
	);
	let past = scenario(
		"Past",
		vec![ScenarioChange::OneTime {
			date: date("2027-01-14"),
			account: acct("CHK"),
			category: None,
			amount: cents(-7000),
		}],
	);
	let result =
		Forecaster::new(&book.budget)
			.unwrap()
			.project(date(TODAY), &fc_3_flows(), Some(&past));
	assert!(matches!(result, Err(CoreError::Invalid(_))));
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
		escrow: cents(0),
		outstanding: cents(980000),
		interest_category: None,
		escrow_category: None,
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
	let projection = project(&book, &[flow], None);
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
	let projection = project(&book, &[flow], None);
	assert_eq!(projection.possible_double_counts, [cat("Paycheck")]);
}

fn mortgage(outstanding: i64, rate: Option<i64>, last_interest: i64, payment: i64) -> LoanPayment {
	LoanPayment {
		from: acct("CHK"),
		loan: acct("LOAN"),
		name: "Mortgage".to_owned(),
		payment: cents(payment),
		rate_bps: rate,
		last_interest: cents(last_interest),
		escrow: cents(50000),
		outstanding: cents(outstanding),
		interest_category: Some(cat("Loan interest")),
		escrow_category: Some(cat("Electric")),
		principal_category: Some(cat("Loan principal")),
	}
}

fn parts(flows: &[KnownFlow]) -> Vec<(&str, i64, Option<&str>)> {
	flows
		.iter()
		.map(|flow| {
			(
				flow.account.as_str(),
				flow.amount.minor(),
				flow.category.as_ref().map(|category| category.as_str()),
			)
		})
		.collect()
}

#[test]
fn fc_2_fl_1_escrow_is_an_expense_not_principal() {
	let flows = mortgage(30_000_000, Some(600), 0, 220000)
		.flows(&[date("2027-02-01"), date("2027-03-01")])
		.unwrap();
	assert_eq!(
		parts(&flows),
		[
			("CHK", -150000, Some("Loan interest")),
			("CHK", -50000, Some("Electric")),
			("CHK", -20000, Some("Loan principal")),
			("CHK", -149900, Some("Loan interest")),
			("CHK", -50000, Some("Electric")),
			("CHK", -20100, Some("Loan principal")),
		]
	);
	let mut book = Book::household();
	book.opening("CHK", "2026-12-31", 1_000_000);
	book.opening("LOAN", "2026-12-31", -30_000_000);
	let projection = project(&book, &flows, None);
	assert_eq!(projection.months[1].net_income, cents(-200000));
	assert_eq!(projection.months[2].net_income, cents(-199900));
	assert_eq!(
		projection.months[2].balances[&acct("CHK")],
		cents(1_000_000 - 440000)
	);
	assert_eq!(
		projection.months[2].balances[&acct("LOAN")],
		cents(-29_959_900)
	);
}

#[test]
fn fc_2_fl_2_final_payment_with_escrow_stops_the_loan() {
	let flows = mortgage(20000, None, 100, 220000)
		.flows(&[date("2027-02-01"), date("2027-03-01")])
		.unwrap();
	assert_eq!(
		parts(&flows),
		[
			("CHK", -100, Some("Loan interest")),
			("CHK", -50000, Some("Electric")),
			("CHK", -20000, Some("Loan principal")),
		]
	);
}

#[test]
fn fc_2_fl_3_a_payment_below_interest_and_escrow_never_capitalizes() {
	let flows = mortgage(30_000_000, Some(600), 0, 160000)
		.flows(&[date("2027-02-01")])
		.unwrap();
	assert_eq!(
		parts(&flows),
		[
			("CHK", -150000, Some("Loan interest")),
			("CHK", -10000, Some("Electric")),
		]
	);
	let mut book = Book::household();
	book.opening("CHK", "2026-12-31", 1_000_000);
	book.opening("LOAN", "2026-12-31", -30_000_000);
	let february = &project(&book, &flows, None).months[1];
	assert_eq!(february.net_income, cents(-160000));
	assert_eq!(february.balances[&acct("CHK")], cents(840000));
	assert_eq!(february.balances[&acct("LOAN")], cents(-30_000_000));
}

fn percent(changes: &[i64]) -> Scenario {
	scenario(
		"Percent",
		changes
			.iter()
			.map(|bp| ScenarioChange::CategoryPercent {
				category: cat("Groceries"),
				percent_bp: *bp,
				start_month: ym("2027-02"),
				end_month: None,
			})
			.collect(),
	)
}

fn groceries_book(level: i64) -> Book {
	let mut book = Book::household();
	for month in [
		"2026-07", "2026-08", "2026-09", "2026-10", "2026-11", "2026-12",
	] {
		book.spend("Groceries", &format!("{month}-15"), -level);
	}
	book
}

#[test]
fn fc_3_sc_1_to_sc_4_percent_changes() {
	let february = |level: i64, changes: &[i64]| {
		project(&groceries_book(level), &[], Some(&percent(changes))).months[1].baselined_net_flow
	};
	assert_eq!(
		february(43005, &[-1000]),
		cents(-38704),
		"SC-1 rounds half away"
	);
	assert_eq!(february(43000, &[-15000]), cents(0), "SC-2 clamped at 0");
	assert_eq!(
		february(43000, &[1000, 500]),
		cents(-49450),
		"SC-3 deltas add"
	);
	assert_eq!(february(-3000, &[1000]), cents(0), "SC-4 negative forecast");
}

#[test]
fn fc_4_2_residuals_use_forecast_plus() {
	let mut book = Book::household();
	for step in 0..9 {
		let month = ym("2026-04").offset(step).unwrap();
		book.spend("Fun", &format!("{month}-15"), 3000);
	}
	let band = project(&book, &[], None).band;
	assert_eq!((band.low, band.high), (cents(3000), cents(3000)));
}

#[test]
fn fc_2_an_income_schedule_feeds_its_most_common_posted_category() {
	let mut book = Book::household();
	for month in [
		"2026-07", "2026-08", "2026-09", "2026-10", "2026-11", "2026-12",
	] {
		book.spend("Paycheck", &format!("{month}-01"), 300000)
			.bill("pay");
		book.spend("Side gigs", &format!("{month}-20"), 20000);
	}
	book.spend("Side gigs", "2026-12-01", 1000).bill("pay");
	let mut flow = known("2027-02-01", "Paycheck", 300000, FlowKind::Income);
	flow.category = None;
	flow.schedule = Some(ScheduleId::new("pay"));
	let projection = project(&book, &[flow], None);
	let baselined: Vec<&str> = projection
		.baselines
		.iter()
		.filter(|baseline| baseline.side == veritas_forecast::Side::Income)
		.map(|baseline| baseline.category.as_str())
		.collect();
	assert_eq!(baselined, ["Side gigs"]);
	assert!(projection.possible_double_counts.is_empty());
}

#[test]
fn fc_3_spending_no_baseline_covers_is_noted_not_projected() {
	let mut book = groceries_book(43000);
	book.hide("Books");
	for month in [
		"2026-07", "2026-08", "2026-09", "2026-10", "2026-11", "2026-12",
	] {
		book.row("CHK", &format!("{month}-05"), -2000);
		book.spend("Books", &format!("{month}-06"), -1050);
	}
	let projection = project(&book, &[], None);
	assert_eq!(projection.not_forecast, cents(3050));
	assert_eq!(
		projection.not_forecast_note().as_deref(),
		Some("Not forecast: $31 a month in uncategorized or hidden spending")
	);
	assert_eq!(projection.months[1].baselined_net_flow, cents(-43000));
	let clean = project(&groceries_book(43000), &[], None);
	assert_eq!(clean.not_forecast_note(), None);
}

#[test]
fn fc_3_not_forecast_skips_asset_purchases_and_misfiled_rows() {
	let mut book = groceries_book(43000);
	book.account("CAR", true)
		.group("Saving", false)
		.category("Emergency fund", "Saving")
		.kind("Emergency fund", veritas_domain::CategoryKind::Savings)
		.hide("Books");
	for month in [
		"2026-07", "2026-08", "2026-09", "2026-10", "2026-11", "2026-12",
	] {
		book.spend("Books", &format!("{month}-06"), -1000);
		book.spend("Emergency fund", &format!("{month}-07"), -5000);
		book.transfer("CHK", "CAR", &format!("{month}-08"), -9000)
			.in_category("Books");
	}
	let projection = project(&book, &[], None);
	assert_eq!(
		projection.not_forecast,
		cents(1000),
		"row 8 and row 14 are not spending to note"
	);
	assert_eq!(projection.asset_purchases, 6);
}

#[test]
fn fc_3_notes_round_half_up_without_overflow() {
	let mut projection = project(&groceries_book(43000), &[], None);
	projection.not_forecast = cents(i64::MAX);
	assert_eq!(
		projection.not_forecast_note().as_deref(),
		Some("Not forecast: $92233720368547758 a month in uncategorized or hidden spending")
	);
	projection.asset_purchases = 2;
	projection.asset_purchase_total = cents(-250);
	assert_eq!(
		projection.asset_purchase_note().as_deref(),
		Some("2 asset purchases excluded: -$2"),
		"half up, not away"
	);
	projection.asset_purchase_total = cents(250);
	assert_eq!(
		projection.asset_purchase_note().as_deref(),
		Some("2 asset purchases excluded: $3")
	);
}

#[test]
fn fc_2_a_split_paycheck_feeds_the_category_of_its_children() {
	let mut book = Book::household();
	let months = [
		"2026-07", "2026-08", "2026-09", "2026-10", "2026-11", "2026-12",
	];
	for (step, month) in months.into_iter().enumerate() {
		let parent = format!("pay-{month}");
		let dues = if step < 2 { -20000 } else { 0 };
		book.row_with_id(&parent, "CHK", &format!("{month}-01"), 300000 + dues)
			.bill("pay")
			.split = veritas_domain::SplitRole::Parent;
		book.spend("Paycheck", &format!("{month}-01"), 300000)
			.child_of(&parent);
		if dues != 0 {
			book.spend("Fun", &format!("{month}-01"), dues)
				.child_of(&parent);
		}
	}
	let mut flow = known("2027-02-01", "Paycheck", 300000, FlowKind::Income);
	flow.category = None;
	flow.schedule = Some(ScheduleId::new("pay"));
	let projection = project(&book, &[flow], None);
	assert!(
		projection
			.baselines
			.iter()
			.all(|baseline| baseline.category != cat("Paycheck")),
		"FC-2: the schedule feeds Paycheck through its split children"
	);
}

#[test]
fn fc_3_a_percent_income_change_reaches_an_uncategorized_schedule() {
	let mut book = Book::household();
	for month in ["2026-11", "2026-12"] {
		book.spend("Paycheck", &format!("{month}-01"), 300000)
			.bill("pay");
	}
	let mut flow = known("2027-02-01", "Paycheck", 300000, FlowKind::Income);
	flow.category = None;
	flow.schedule = Some(ScheduleId::new("pay"));
	let raise = scenario(
		"Raise",
		vec![ScenarioChange::IncomeChange {
			account: acct("CHK"),
			category: Some(cat("Paycheck")),
			change: IncomeDelta::Percent(1000),
			start_month: ym("2027-02"),
			end_month: Some(ym("2027-02")),
		}],
	);
	let projection = project(&book, &[flow], Some(&raise));
	assert_eq!(
		projection.months[1].scenario_net_income,
		cents(30000),
		"FC-3 income change"
	);
}

#[test]
fn fc_3_a_planned_asset_purchase_is_a_one_time_transfer() {
	let mut book = fc_3_book();
	book.account("CAR", true);
	let flows = fc_3_flows();
	let baseline = project(&book, &flows, None);
	let buy = scenario(
		"Buy a car",
		vec![ScenarioChange::OneTimeTransfer {
			date: date("2027-03-10"),
			from: acct("CHK"),
			to: acct("CAR"),
			amount: cents(2_000_000),
		}],
	);
	let projection = project(&book, &flows, Some(&buy));
	let march = &projection.months[2];
	assert_eq!(
		march.net_worth, baseline.months[2].net_worth,
		"net worth doesn't move"
	);
	assert_eq!(march.scenario_net_income, cents(0));
	assert_eq!(
		march.balances[&acct("CHK")].minor(),
		baseline.months[2].balances[&acct("CHK")].minor() - 2_000_000
	);
	assert_eq!(march.balances[&acct("CAR")], cents(2_000_000));

	let forecaster = Forecaster::new(&book.budget).unwrap();
	for (from, to, on, amount) in [
		("CHK", "CHK", "2027-03-10", 100),
		("CHK", "CAR", "2027-03-10", 0),
		("CHK", "CAR", "2027-01-14", 100),
	] {
		let bad = scenario(
			"Bad",
			vec![ScenarioChange::OneTimeTransfer {
				date: date(on),
				from: acct(from),
				to: acct(to),
				amount: cents(amount),
			}],
		);
		assert!(matches!(
			forecaster.project(date(TODAY), &flows, Some(&bad)),
			Err(CoreError::Invalid(_))
		));
	}
}
