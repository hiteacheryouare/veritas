//! Whole-household forecasts the way the app will run them: several
//! accounts, a card paid from checking, savings moves, a scheduled car loan,
//! split purchases, unscheduled side income, then what-ifs compared on top.

#![allow(clippy::unwrap_used, clippy::panic)]

mod support;

use std::collections::BTreeMap;

use support::*;
use veritas_domain::{CategoryKind, CoreError, GoalId, SplitRole};
use veritas_forecast::{
	BandMethod, Basis, Cadence, Certainty, EtaStatus, FlowKind, Forecaster, IncomeDelta, KnownFlow,
	LoanPayment, Projection, Scenario, ScenarioEvent, goal_eta,
};

const TODAY: &str = "2027-01-15";
const HISTORY: [&str; 6] = [
	"2026-07", "2026-08", "2026-09", "2026-10", "2026-11", "2026-12",
];

/// Groceries go on the card (plus a small top-up from checking), dining
/// from checking, the card is paid in full each month, 50000 moves to
/// savings, and the car loan posts from its schedule as a split.
fn household() -> Book {
	let mut book = Book::household();
	book.group("Saving", false)
		.category("Emergency fund", "Saving")
		.kind("Emergency fund", CategoryKind::Savings);
	book.opening("CHK", "2026-06-30", 500000)
		.opening("SAV", "2026-06-30", 1_000_000)
		.opening("LOAN", "2026-06-30", -980000);

	for month in HISTORY {
		let on = |day: u32| format!("{month}-{day:02}");
		book.spend("Rent", &on(1), -165000).bill("Rent");
		book.spend("Paycheck", &on(1), 300000).bill("Paycheck");
		loan_payment(&mut book, &on(10), 32000, 8000);
		let card_spend = if month == "2026-10" {
			let parent = book.row("VISA", &on(15), -50000);
			parent.split = SplitRole::Parent;
			let parent = parent.id.as_str().to_owned();
			book.row("VISA", &on(15), -40000)
				.in_category("Groceries")
				.child_of(&parent);
			book.row("VISA", &on(15), -10000)
				.in_category("Dining")
				.child_of(&parent);
			book.spend("Dining", &on(18), -2000);
			50000
		} else {
			book.row("VISA", &on(15), -40000).in_category("Groceries");
			book.spend("Dining", &on(18), -12000);
			40000
		};
		book.spend("Groceries", &on(20), -5000);
		book.spend("Side gigs", &on(25), 20000);
		book.transfer("CHK", "SAV", &on(26), -50000)
			.in_category("Emergency fund");
		book.transfer("CHK", "VISA", &on(28), -card_spend);
	}
	book.row("VISA", "2026-11-16", -999999)
		.in_category("Groceries")
		.tombstone = true;

	book.spend("Rent", "2027-01-01", -165000).bill("Rent");
	book.spend("Paycheck", "2027-01-01", 300000)
		.bill("Paycheck");
	loan_payment(&mut book, "2027-01-10", 32000, 8000);
	book.row("VISA", "2027-01-08", -18000)
		.in_category("Groceries");
	book.spend("Dining", "2027-01-12", -4000);
	book
}

fn loan_payment(book: &mut Book, on: &str, principal: i64, interest: i64) {
	let parent = book.row("CHK", on, -(principal + interest));
	parent.split = SplitRole::Parent;
	parent.schedule = Some(veritas_domain::ScheduleId::new("Car loan"));
	let parent = parent.id.as_str().to_owned();
	book.transfer("CHK", "LOAN", on, -principal)
		.in_category("Loan principal")
		.child_of(&parent);
	book.spend("Loan interest", on, -interest).child_of(&parent);
}

fn transfer(on: &str, to: &str, amount: i64, name: &str) -> KnownFlow {
	KnownFlow {
		date: date(on),
		account: acct("CHK"),
		amount: cents(-amount),
		kind: FlowKind::Transfer {
			counterpart: acct(to),
		},
		category: None,
		schedule: None,
		name: name.to_owned(),
		certainty: Certainty::Exact,
	}
}

fn car_loan(outstanding: i64) -> LoanPayment {
	LoanPayment {
		from: acct("CHK"),
		loan: acct("LOAN"),
		name: "Car loan".to_owned(),
		payment: cents(40000),
		rate_bps: Some(980),
		last_interest: cents(8000),
		outstanding: cents(outstanding),
		interest_category: Some(cat("Loan interest")),
		principal_category: Some(cat("Loan principal")),
	}
}

/// What the recurrence engine hands over for the next 12 months: only
/// occurrences that haven't posted yet.
fn household_flows() -> Vec<KnownFlow> {
	let mut flows = monthly_flows("2027-02", 12, "Rent", -165000, FlowKind::Expense);
	flows.extend(monthly_flows(
		"2027-02",
		12,
		"Paycheck",
		300000,
		FlowKind::Income,
	));
	let mut loan_dates = Vec::new();
	for step in 0..12 {
		let month = ym("2027-02").offset(step).unwrap();
		loan_dates.push(date(&format!("{month}-10")));
	}
	flows.extend(car_loan(756000).flows(&loan_dates).unwrap());
	for step in 0..13 {
		let month = ym("2027-01").offset(step).unwrap();
		flows.push(transfer(&format!("{month}-26"), "SAV", 50000, "Savings"));
		let card = if step == 0 { 50000 } else { 40000 };
		flows.push(transfer(&format!("{month}-28"), "VISA", card, "Card"));
	}
	flows
}

fn project(book: &Book, flows: &[KnownFlow], scenario: &Scenario) -> Projection {
	Forecaster::new(&book.budget)
		.unwrap()
		.project(date(TODAY), flows, scenario)
		.unwrap()
}

fn balances(projection: &Projection, step: usize) -> [i64; 4] {
	let month = &projection.months[step];
	["CHK", "VISA", "SAV", "LOAN"].map(|id| month.balances[&acct(id)].minor())
}

#[test]
fn household_net_worth_today_counts_every_account_once() {
	let projection = project(&household(), &household_flows(), &Scenario::baseline());
	let today: Vec<i64> = ["CHK", "VISA", "SAV", "LOAN"]
		.iter()
		.map(|id| projection.balances_today[&acct(id)].minor())
		.collect();
	assert_eq!(today, [639000, -18000, 1_300_000, -756000]);
	assert_eq!(projection.net_worth_today, cents(1_165_000));
}

#[test]
fn household_baselines_follow_the_card_and_skip_known_and_moved_money() {
	let projection = project(&household(), &household_flows(), &Scenario::baseline());
	let level = |category: &str| {
		projection
			.baselines
			.iter()
			.find(|baseline| baseline.category == cat(category))
			.map(|baseline| (baseline.level.minor(), baseline.account.clone()))
	};
	assert_eq!(level("Groceries"), Some((45000, Some(acct("VISA")))));
	assert_eq!(level("Dining"), Some((12000, Some(acct("CHK")))));
	assert_eq!(level("Side gigs"), Some((20000, Some(acct("CHK")))));
	assert_eq!(level("Rent").map(|(x, _)| x), Some(0));
	assert_eq!(level("Loan interest").map(|(x, _)| x), Some(0));
	assert_eq!(level("Paycheck"), None);
	assert_eq!(level("Loan principal"), None);
	assert_eq!(level("Emergency fund"), None);
	assert!(projection.possible_double_counts.is_empty());
	assert_eq!(projection.capitalized_transfers, 0);
}

#[test]
fn household_projection_moves_each_account_by_its_own_flows() {
	let projection = project(&household(), &household_flows(), &Scenario::baseline());

	assert_eq!(projection.months[0].uncertain_net_flow, cents(-15000));
	assert_eq!(projection.months[0].known_net_income, cents(0));
	assert_eq!(balances(&projection, 0), [551000, 5000, 1_350_000, -756000]);

	let february = &projection.months[1];
	assert_eq!(february.known_net_income, cents(300000 - 165000 - 6174));
	assert_eq!(february.uncertain_net_flow, cents(-37000));
	assert_eq!(balances(&projection, 1), [564000, 0, 1_400_000, -722174]);

	assert_eq!(
		balances(&projection, 2),
		[577000, -5000, 1_450_000, -688072]
	);
	assert_eq!(
		balances(&projection, 3),
		[590000, -10000, 1_500_000, -653691]
	);

	let net_worth: Vec<i64> = projection
		.months
		.iter()
		.take(4)
		.map(|month| month.net_worth.minor())
		.collect();
	assert_eq!(net_worth, [1_150_000, 1_241_826, 1_333_928, 1_426_309]);
	assert_eq!(projection.horizons[0].months, 3);
	assert_eq!(projection.horizons[0].net_worth, cents(1_426_309));
	assert_eq!(
		projection.horizons[0].band,
		Some((cents(1_426_309), cents(1_426_309))),
		"every month's uncertain flow was the same, so the range is empty"
	);
	assert_eq!(
		projection.band.label(),
		"Based on 6 months of history, not a statistical range"
	);
}

#[test]
fn household_balances_add_up_to_net_worth_every_month() {
	let projection = project(&household(), &household_flows(), &Scenario::baseline());
	let mut previous = projection.net_worth_today.minor();
	for month in &projection.months {
		let total: i64 = month.balances.values().map(|balance| balance.minor()).sum();
		assert_eq!(total, month.net_worth.minor(), "{}", month.month);
		assert_eq!(
			month.net_worth.minor() - previous,
			month.net_income.minor(),
			"{}",
			month.month
		);
		previous = month.net_worth.minor();
	}
}

#[test]
fn household_suggestions_for_next_month() {
	let suggestions = Forecaster::new(&household().budget)
		.unwrap()
		.suggestions(date(TODAY), &household_flows(), &BTreeMap::new())
		.unwrap();
	let reasons: Vec<(&str, Option<i64>, &str)> = suggestions
		.items
		.iter()
		.filter(|item| item.amount.is_some())
		.map(|item| {
			(
				item.category.as_str(),
				item.amount.map(|amount| amount.minor()),
				item.reason.as_str(),
			)
		})
		.collect();
	assert_eq!(
		reasons,
		[
			("Rent", Some(165000), "Rent $1650, scheduled: Rent"),
			("Groceries", Some(45000), "Groceries $450, 6-month median"),
			("Dining", Some(12000), "Dining $120, 6-month median"),
			(
				"Loan interest",
				Some(6200),
				"Loan interest $62, scheduled: Car loan"
			),
			(
				"Loan principal",
				Some(32000),
				"Loan principal $320, principal of the last loan payment"
			),
		]
	);
	assert!(
		suggestions
			.items
			.iter()
			.all(|item| item.category != cat("Emergency fund")),
		"suggestions never include savings moves"
	);
	assert_eq!(suggestions.expected_income, cents(320000));
	assert_eq!(suggestions.exceeds_income_by, None);
}

#[test]
fn unlinked_loan_interest_history_is_flagged_as_a_double_count() {
	let mut book = household();
	for row in book.budget.transactions.values_mut() {
		if row.schedule.as_ref().map(|s| s.as_str()) == Some("Car loan") {
			row.schedule = None;
		}
	}
	let projection = project(&book, &household_flows(), &Scenario::baseline());
	assert_eq!(projection.possible_double_counts, [cat("Loan interest")]);
}

#[test]
fn a_loan_paid_off_inside_the_horizon_stops_at_zero() {
	let mut book = Book::household();
	book.opening("CHK", "2026-12-31", 100000)
		.opening("LOAN", "2026-12-31", -20000);
	let loan = LoanPayment {
		from: acct("CHK"),
		loan: acct("LOAN"),
		name: "Car loan".to_owned(),
		payment: cents(9000),
		rate_bps: Some(1200),
		last_interest: cents(0),
		outstanding: cents(20000),
		interest_category: Some(cat("Loan interest")),
		principal_category: Some(cat("Loan principal")),
	};
	let dates: Vec<_> = (0..12)
		.map(|step| {
			let month = ym("2027-02").offset(step).unwrap();
			date(&format!("{month}-10"))
		})
		.collect();
	let flows = loan.flows(&dates).unwrap();
	let amounts: Vec<i64> = flows.iter().map(|flow| flow.amount.minor()).collect();
	assert_eq!(amounts, [-200, -8800, -112, -8888, -23, -2312]);

	let projection = project(&book, &flows, &Scenario::baseline());
	let loan_balance: Vec<i64> = projection
		.months
		.iter()
		.map(|month| month.balances[&acct("LOAN")].minor())
		.collect();
	assert_eq!(&loan_balance[..5], [-20000, -11200, -2312, 0, 0]);
	assert!(loan_balance[4..].iter().all(|balance| *balance == 0));
	let last = projection.horizons.last().unwrap();
	assert_eq!(last.net_worth, cents(80000 - 335));
	assert_eq!(last.balances[&acct("CHK")], cents(100000 - 20000 - 335));
}

#[test]
fn compared_scenarios_each_change_only_their_own_overlay() {
	let book = household();
	let before = book.budget.clone();
	let flows = household_flows();
	let cheaper_rent = Scenario {
		name: "Cheaper place".to_owned(),
		events: vec![ScenarioEvent::Recurring {
			start: date("2027-03-01"),
			end_month: None,
			cadence: Cadence::EveryMonths(1),
			account: acct("CHK"),
			category: Some(cat("Rent")),
			amount: cents(40000),
		}],
	};
	let pricier_food = Scenario {
		name: "Groceries up 10%".to_owned(),
		events: vec![ScenarioEvent::CategoryPercent {
			category: cat("Groceries"),
			percent_bp: 1000,
			start_month: ym("2027-02"),
			end_month: None,
		}],
	};
	let raise = Scenario {
		name: "Raise".to_owned(),
		events: vec![ScenarioEvent::IncomeChange {
			account: acct("CHK"),
			category: Some(cat("Paycheck")),
			change: IncomeDelta::Percent(500),
			start_month: ym("2027-03"),
			end_month: None,
		}],
	};
	let comparison = Forecaster::new(&book.budget)
		.unwrap()
		.compare(
			date(TODAY),
			&flows,
			&[cheaper_rent, pricier_food, raise.clone()],
		)
		.unwrap();
	assert_eq!(book.budget, before, "a comparison never touches the budget");
	assert_eq!(
		comparison.baseline,
		project(&book, &flows, &Scenario::baseline())
	);
	assert_eq!(comparison.scenarios[2], project(&book, &flows, &raise));

	let at_april = |projection: &Projection| {
		let april = &projection.months[3];
		(april.net_worth.minor(), balances(projection, 3))
	};
	let (base_worth, base) = at_april(&comparison.baseline);
	let deltas: Vec<(i64, [i64; 4])> = comparison
		.scenarios
		.iter()
		.map(|scenario| {
			let (worth, accounts) = at_april(scenario);
			let mut delta = [0; 4];
			for (slot, (after, before)) in accounts.iter().zip(base).enumerate() {
				delta[slot] = after - before;
			}
			(worth - base_worth, delta)
		})
		.collect();
	assert_eq!(
		deltas,
		[
			(80000, [80000, 0, 0, 0]),
			(-13500, [0, -13500, 0, 0]),
			(30000, [30000, 0, 0, 0]),
		]
	);
}

#[test]
fn a_percent_change_to_all_income_counts_scheduled_and_side_income() {
	let book = household();
	let pay_cut = Scenario {
		name: "Hours cut".to_owned(),
		events: vec![ScenarioEvent::IncomeChange {
			account: acct("CHK"),
			category: None,
			change: IncomeDelta::Percent(-1000),
			start_month: ym("2027-02"),
			end_month: Some(ym("2027-02")),
		}],
	};
	let projection = project(&book, &household_flows(), &pay_cut);
	assert_eq!(
		projection.months[1].scenario_net_income,
		cents(-(300000 + 20000) / 10)
	);
	assert_eq!(projection.months[2].scenario_net_income, cents(0));
}

#[test]
fn the_loan_principal_suggestion_never_exceeds_what_is_still_owed() {
	let mut book = Book::household();
	book.opening("CHK", "2026-06-30", 100000)
		.opening("LOAN", "2026-06-30", -50000);
	loan_payment(&mut book, "2026-12-10", 32000, 400);
	let suggestions = Forecaster::new(&book.budget)
		.unwrap()
		.suggestions(date(TODAY), &[], &BTreeMap::new())
		.unwrap();
	let principal = suggestions
		.items
		.iter()
		.find(|item| item.category == cat("Loan principal"))
		.unwrap();
	assert_eq!(principal.amount, Some(cents(18000)));
	assert_eq!(
		principal.reason,
		"Loan principal $180, principal of the last loan payment"
	);
}

#[test]
fn a_category_cut_of_more_than_all_of_it_floors_at_zero() {
	let book = household();
	let scenario = Scenario {
		name: "No eating out".to_owned(),
		events: vec![ScenarioEvent::CategoryPercent {
			category: cat("Dining"),
			percent_bp: -15000,
			start_month: ym("2027-02"),
			end_month: Some(ym("2027-02")),
		}],
	};
	let base = project(&book, &household_flows(), &Scenario::baseline());
	let cut = project(&book, &household_flows(), &scenario);
	assert_eq!(
		cut.months[1].uncertain_net_flow.minor() - base.months[1].uncertain_net_flow.minor(),
		12000
	);
	let march = |projection: &Projection| {
		let month = &projection.months[2];
		(month.uncertain_net_flow.minor(), month.net_worth.minor())
	};
	assert_eq!(march(&cut).0, march(&base).0, "the cut ended in February");
	assert_eq!(march(&cut).1 - march(&base).1, 12000);
}

#[test]
fn a_scenario_flow_on_another_account_moves_only_that_account() {
	let book = household();
	let scenario = Scenario {
		name: "Interest on savings".to_owned(),
		events: vec![ScenarioEvent::Recurring {
			start: date("2027-02-28"),
			end_month: Some(ym("2027-04")),
			cadence: Cadence::EveryMonths(1),
			account: acct("SAV"),
			category: None,
			amount: cents(1500),
		}],
	};
	let base = project(&book, &household_flows(), &Scenario::baseline());
	let with = project(&book, &household_flows(), &scenario);
	let [chk, visa, sav, loan] = balances(&with, 4);
	let [base_chk, base_visa, base_sav, base_loan] = balances(&base, 4);
	assert_eq!(
		(
			chk - base_chk,
			visa - base_visa,
			sav - base_sav,
			loan - base_loan
		),
		(0, 0, 4500, 0)
	);
	assert_eq!(
		with.months[4].net_worth.minor() - base.months[4].net_worth.minor(),
		4500
	);
}

/// Spending 10000 a month except for the listed surprises, so FC-4.2's
/// rolling-origin residuals can be worked out by hand.
fn lumpy_groceries(book: &mut Book) {
	let values: [i64; 9] = [10000, 10000, 10000, 13000, 7000, 10000, 10000, 16000, 10000];
	for (step, x) in values.iter().enumerate() {
		let month = ym("2026-04").offset(step as i32).unwrap();
		book.spend("Groceries", &format!("{month}-15"), -x);
	}
}

#[test]
fn band_comes_from_hand_checked_rolling_origin_residuals() {
	// Residuals for Jul..Dec: -3000, 3000, 0, 0, -6000, 0. P10 = -6000, P90 = 3000.
	let mut book = Book::household();
	book.opening("CHK", "2026-03-31", 500000);
	lumpy_groceries(&mut book);
	let projection = project(&book, &[], &Scenario::baseline());
	assert_eq!(projection.band.method, BandMethod::Residuals);
	assert_eq!(projection.band.months, 6);
	assert_eq!(
		(projection.band.low.minor(), projection.band.high.minor()),
		(-6000, 3000)
	);
	let three = &projection.horizons[0];
	assert_eq!(
		three.band,
		Some((
			cents(three.net_worth.minor() - 10392),
			cents(three.net_worth.minor() + 5196)
		))
	);
	assert_eq!(projection.band.label(), "80% band from 6 past months");
}

#[test]
fn lumpy_side_income_skews_the_band_upward() {
	// The same surprises as income: residuals 3000, -3000, 0, 0, 6000, 0.
	let mut book = Book::household();
	book.opening("CHK", "2026-03-31", 500000);
	let values: [i64; 9] = [10000, 10000, 10000, 13000, 7000, 10000, 10000, 16000, 10000];
	for (step, x) in values.iter().enumerate() {
		let month = ym("2026-04").offset(step as i32).unwrap();
		book.spend("Side gigs", &format!("{month}-25"), *x);
	}
	let band = project(&book, &[], &Scenario::baseline()).band;
	assert_eq!(band.method, BandMethod::Residuals);
	assert_eq!((band.low.minor(), band.high.minor()), (-3000, 6000));
}

#[test]
fn bills_that_vary_never_widen_the_band() {
	let mut book = Book::household();
	book.opening("CHK", "2026-03-31", 500000);
	lumpy_groceries(&mut book);
	let quiet = project(&book, &[], &Scenario::baseline()).band;
	for (step, amount) in [9000, 31000, 4000, 22000, 15000, 8000, 40000, 1000, 12000]
		.iter()
		.enumerate()
	{
		let month = ym("2026-04").offset(step as i32).unwrap();
		book.spend("Electric", &format!("{month}-03"), -amount)
			.bill("Electric");
		book.spend("Paycheck", &format!("{month}-01"), 100000 + amount * 3)
			.bill("Paycheck");
	}
	let pay = monthly_flows("2027-02", 1, "Paycheck", 250000, FlowKind::Income);
	assert_eq!(project(&book, &pay, &Scenario::baseline()).band, quiet);
}

#[test]
fn seasonal_months_are_projected_with_their_own_index() {
	// F-7's series: Dec 8000, other months 5000, 13 complete months by Dec 2027.
	let mut book = Book::household();
	book.opening("CHK", "2026-10-31", 200000);
	let mut values: Vec<i64> = vec![5000, 8000];
	values.extend([5000; 11]);
	for (step, x) in values.iter().enumerate() {
		let month = ym("2026-11").offset(step as i32).unwrap();
		book.spend("Gifts", &format!("{month}-15"), -x);
	}
	let projection = Forecaster::new(&book.budget)
		.unwrap()
		.project(date("2027-12-05"), &[], &Scenario::baseline())
		.unwrap();
	let flows: Vec<(String, i64)> = projection
		.months
		.iter()
		.take(3)
		.map(|month| (month.month.to_string(), month.uncertain_net_flow.minor()))
		.collect();
	assert_eq!(
		flows,
		[
			("2027-12".to_owned(), -6463),
			("2028-01".to_owned(), -5000),
			("2028-02".to_owned(), -5000),
		]
	);
}

#[test]
fn a_brand_new_budget_projects_known_flows_only() {
	let mut book = Book::household();
	book.opening("CHK", "2027-01-02", 250000);
	book.spend("Groceries", "2027-01-05", -12000);
	book.spend("Side gigs", "2027-01-09", 7000);
	let rent = monthly_flows("2027-02", 12, "Rent", -165000, FlowKind::Expense);
	let pay = monthly_flows("2027-02", 12, "Paycheck", 200000, FlowKind::Income);
	let flows: Vec<_> = rent.into_iter().chain(pay).collect();
	let projection = project(&book, &flows, &Scenario::baseline());
	assert_eq!(projection.net_worth_today, cents(245000));
	assert!(
		projection
			.baselines
			.iter()
			.all(|baseline| !baseline.has_history())
	);
	assert_eq!(projection.months[0].net_worth, cents(245000));
	assert_eq!(projection.months[1].net_worth, cents(245000 + 35000));
	assert_eq!(projection.horizons[3].net_worth, cents(245000 + 12 * 35000));
	assert!(
		projection
			.horizons
			.iter()
			.all(|horizon| horizon.band.is_none())
	);
	assert_eq!(projection.band.label(), "No history yet");
}

#[test]
fn a_category_that_went_quiet_forecasts_nothing() {
	let mut book = Book::household();
	book.opening("CHK", "2026-01-31", 100000);
	for month in ["2026-02", "2026-03", "2026-04", "2026-05"] {
		book.spend("Fuel", &format!("{month}-12"), -9000);
	}
	let projection = project(&book, &[], &Scenario::baseline());
	let fuel = projection
		.baselines
		.iter()
		.find(|baseline| baseline.category == cat("Fuel"))
		.unwrap();
	assert_eq!((fuel.level, fuel.window_months), (cents(0), 6));
	assert_eq!(projection.horizons[3].net_worth, projection.net_worth_today);
}

#[test]
fn following_the_goal_suggestion_keeps_the_eta_on_the_target_month() {
	let mut book = Book::household();
	book.category("New laptop", "Goals");
	book.goal("laptop", "New laptop", 150000, Some("2027-06-15"));
	for step in 0..6 {
		let today = ym("2026-12").offset(step).unwrap();
		let carry = 25000 * i64::from(step);
		let envelopes = BTreeMap::from([(
			cat("New laptop"),
			veritas_forecast::Envelope {
				carry_into_next: cents(carry),
			},
		)]);
		let suggestions = Forecaster::new(&book.budget)
			.unwrap()
			.suggestions(date(&format!("{today}-10")), &[], &envelopes)
			.unwrap();
		let laptop = suggestions
			.items
			.iter()
			.find(|item| item.category == cat("New laptop"))
			.unwrap();
		assert_eq!(laptop.basis, Basis::Goal);
		assert_eq!(laptop.amount, Some(cents(25000)), "suggested in {today}");

		let month = today.offset(1).unwrap();
		book.budgeted(&month.to_string(), "New laptop", 25000);
		if month < ym("2027-02") {
			continue;
		}
		let eta = goal_eta(
			&book.budget,
			&GoalId::new("laptop"),
			month,
			cents(carry + 25000),
			&Scenario::baseline(),
		)
		.unwrap();
		let expected = if month == ym("2027-06") {
			EtaStatus::Reached
		} else {
			EtaStatus::Eta {
				month: ym("2027-06"),
				optimistic: ym("2027-06"),
				pessimistic: Some(ym("2027-06")),
			}
		};
		assert_eq!(eta.status, expected, "ETA seen in {month}");
		assert_eq!(eta.on_track, Some(true));
	}
}

#[test]
fn g_3_and_g_5_goal_suggestions() {
	let mut book = Book::household();
	book.category("Trip", "Goals");
	book.goal("trip", "Trip", 100000, Some("2027-04-20"));
	let suggest = |book: &Book, carry: i64| {
		let envelopes = BTreeMap::from([(
			cat("Trip"),
			veritas_forecast::Envelope {
				carry_into_next: cents(carry),
			},
		)]);
		Forecaster::new(&book.budget)
			.unwrap()
			.suggestions(date("2027-01-10"), &[], &envelopes)
			.unwrap()
			.items
			.into_iter()
			.find(|item| item.category == cat("Trip"))
			.unwrap()
	};
	let g_3 = suggest(&book, 99999);
	assert_eq!(g_3.amount, Some(cents(1)));
	assert_eq!(g_3.reason, "Trip $0.01: $0.01 left over 3 months");

	book.goal("trip", "Trip", 100000, Some("2026-11-01"));
	let g_5 = suggest(&book, 50000);
	assert_eq!(g_5.amount, Some(cents(50000)));
	assert_eq!(g_5.reason, "Trip $500.00: $500.00 left over 1 months");
}

#[test]
#[ignore = "R-FC-2 bug: a goal contribution what-if in the current month is ignored, because months_to_go starts counting at next month"]
fn an_extra_goal_contribution_this_month_moves_the_eta() {
	let mut book = Book::household();
	book.category("New laptop", "Goals");
	book.goal("laptop", "New laptop", 150000, Some("2027-12-15"));
	book.budgeted("2026-12", "New laptop", 10000);
	let eta = |scenario: &Scenario| {
		goal_eta(
			&book.budget,
			&GoalId::new("laptop"),
			ym("2027-01"),
			cents(100000),
			scenario,
		)
		.unwrap()
		.status
	};
	let month_of = |status: EtaStatus| match status {
		EtaStatus::Eta { month, .. } => Some(month),
		_ => None,
	};
	assert_eq!(month_of(eta(&Scenario::baseline())), Some(ym("2027-06")));
	let bonus = Scenario {
		name: "Bonus this month".to_owned(),
		events: vec![ScenarioEvent::GoalContribution {
			goal: GoalId::new("laptop"),
			amount: cents(20000),
			start_month: ym("2027-01"),
			end_month: Some(ym("2027-01")),
		}],
	};
	// 50000 left, 20000 extra now, then 10000 a month: 3 more months.
	assert_eq!(month_of(eta(&bonus)), Some(ym("2027-04")));
}

#[test]
#[ignore = "R-FC-1 bug: known flows dated today are dropped as if already posted, so a bill that is due today (or missed) and unpaid never lowers the projection"]
fn an_unpaid_bill_due_today_still_lowers_the_projection() {
	let mut book = Book::household();
	book.opening("CHK", "2026-12-31", 400000);
	let rent_due_today = known(TODAY, "Rent", -165000, FlowKind::Expense);
	let projection = project(&book, &[rent_due_today], &Scenario::baseline());
	assert_eq!(projection.months[0].net_worth, cents(400000 - 165000));
}

#[test]
fn an_invalid_cadence_is_rejected_not_projected() {
	let scenario = Scenario {
		name: "Never".to_owned(),
		events: vec![ScenarioEvent::Recurring {
			start: date("2027-02-01"),
			end_month: None,
			cadence: Cadence::EveryWeeks(0),
			account: acct("CHK"),
			category: None,
			amount: cents(100),
		}],
	};
	let result = Forecaster::new(&household().budget).unwrap().project(
		date(TODAY),
		&household_flows(),
		&scenario,
	);
	assert!(matches!(result, Err(CoreError::Invalid(_))));
}
