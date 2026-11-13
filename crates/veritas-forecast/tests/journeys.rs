//! Whole-household forecasts the way the app will run them: several
//! accounts, a card paid from checking, savings moves, a scheduled car loan,
//! split purchases, unscheduled side income, then what-ifs compared on top.

#![allow(clippy::unwrap_used, clippy::panic)]

mod support;

use std::collections::BTreeMap;

use support::*;
use veritas_domain::{
	Cadence, CategoryKind, CoreError, GoalId, IncomeDelta, Scenario, ScenarioChange, SplitRole,
};
use veritas_forecast::{
	BandMethod, Basis, Certainty, EtaStatus, FlowKind, Forecaster, KnownFlow, LoanPayment,
	Projection, goal_eta,
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
		escrow: cents(0),
		outstanding: cents(outstanding),
		interest_category: Some(cat("Loan interest")),
		escrow_category: None,
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

fn project(book: &Book, flows: &[KnownFlow], scenario: Option<&Scenario>) -> Projection {
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
	let projection = project(&household(), &household_flows(), None);
	let today: Vec<i64> = ["CHK", "VISA", "SAV", "LOAN"]
		.iter()
		.map(|id| projection.balances_today[&acct(id)].minor())
		.collect();
	assert_eq!(today, [639000, -18000, 1_300_000, -756000]);
	assert_eq!(projection.net_worth_today, cents(1_165_000));
}

#[test]
fn household_baselines_follow_the_card_and_skip_known_and_moved_money() {
	let projection = project(&household(), &household_flows(), None);
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
	assert_eq!(projection.asset_purchases, 0);
}

#[test]
fn household_projection_moves_each_account_by_its_own_flows() {
	let projection = project(&household(), &household_flows(), None);

	assert_eq!(projection.months[0].baselined_net_flow, cents(-15000));
	assert_eq!(projection.months[0].known_net_income, cents(0));
	assert_eq!(balances(&projection, 0), [551000, 5000, 1_350_000, -756000]);

	let february = &projection.months[1];
	assert_eq!(february.known_net_income, cents(300000 - 165000 - 6174));
	assert_eq!(february.baselined_net_flow, cents(-37000));
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
		"every month's baselined flow was the same, so the range is empty"
	);
	assert_eq!(
		projection.band.label(),
		"Based on 6 months of history, not a statistical range"
	);
}

#[test]
fn household_balances_add_up_to_net_worth_every_month() {
	let projection = project(&household(), &household_flows(), None);
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
	let projection = project(&book, &household_flows(), None);
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
		escrow: cents(0),
		outstanding: cents(20000),
		interest_category: Some(cat("Loan interest")),
		escrow_category: None,
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

	let projection = project(&book, &flows, None);
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
	let cheaper_rent = scenario(
		"Cheaper place",
		vec![ScenarioChange::Recurring {
			start: date("2027-03-01"),
			end_month: None,
			cadence: Cadence::EveryMonths(1),
			account: acct("CHK"),
			category: Some(cat("Rent")),
			amount: cents(40000),
		}],
	);
	let pricier_food = scenario(
		"Groceries up 10%",
		vec![ScenarioChange::CategoryPercent {
			category: cat("Groceries"),
			percent_bp: 1000,
			start_month: ym("2027-02"),
			end_month: None,
		}],
	);
	let raise = scenario(
		"Raise",
		vec![ScenarioChange::IncomeChange {
			account: acct("CHK"),
			category: Some(cat("Paycheck")),
			change: IncomeDelta::Percent(500),
			start_month: ym("2027-03"),
			end_month: None,
		}],
	);
	let comparison = Forecaster::new(&book.budget)
		.unwrap()
		.compare(date(TODAY), &flows, &[&cheaper_rent, &pricier_food, &raise])
		.unwrap();
	assert_eq!(book.budget, before, "a comparison never touches the budget");
	assert_eq!(comparison.baseline, project(&book, &flows, None));
	assert_eq!(
		comparison.scenarios[2],
		project(&book, &flows, Some(&raise))
	);

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
	let pay_cut = scenario(
		"Hours cut",
		vec![ScenarioChange::IncomeChange {
			account: acct("CHK"),
			category: None,
			change: IncomeDelta::Percent(-1000),
			start_month: ym("2027-02"),
			end_month: Some(ym("2027-02")),
		}],
	);
	let projection = project(&book, &household_flows(), Some(&pay_cut));
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
	let scenario = scenario(
		"No eating out",
		vec![ScenarioChange::CategoryPercent {
			category: cat("Dining"),
			percent_bp: -15000,
			start_month: ym("2027-02"),
			end_month: Some(ym("2027-02")),
		}],
	);
	let base = project(&book, &household_flows(), None);
	let cut = project(&book, &household_flows(), Some(&scenario));
	assert_eq!(
		cut.months[1].baselined_net_flow.minor() - base.months[1].baselined_net_flow.minor(),
		12000
	);
	let march = |projection: &Projection| {
		let month = &projection.months[2];
		(month.baselined_net_flow.minor(), month.net_worth.minor())
	};
	assert_eq!(march(&cut).0, march(&base).0, "the cut ended in February");
	assert_eq!(march(&cut).1 - march(&base).1, 12000);
}

#[test]
fn a_scenario_flow_on_another_account_moves_only_that_account() {
	let book = household();
	let scenario = scenario(
		"Interest on savings",
		vec![ScenarioChange::Recurring {
			start: date("2027-02-28"),
			end_month: Some(ym("2027-04")),
			cadence: Cadence::EveryMonths(1),
			account: acct("SAV"),
			category: None,
			amount: cents(1500),
		}],
	);
	let base = project(&book, &household_flows(), None);
	let with = project(&book, &household_flows(), Some(&scenario));
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
	// FC-4.2 step 3 at h = 3: scale 22795, so -13677 and 6839.
	let mut book = Book::household();
	book.opening("CHK", "2026-03-31", 500000);
	lumpy_groceries(&mut book);
	let projection = project(&book, &[], None);
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
			cents(three.net_worth.minor() - 13677),
			cents(three.net_worth.minor() + 6839)
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
	let band = project(&book, &[], None).band;
	assert_eq!(band.method, BandMethod::Residuals);
	assert_eq!((band.low.minor(), band.high.minor()), (-3000, 6000));
}

#[test]
fn bills_that_vary_never_widen_the_band() {
	let mut book = Book::household();
	book.opening("CHK", "2026-03-31", 500000);
	lumpy_groceries(&mut book);
	let quiet = project(&book, &[], None).band;
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
	assert_eq!(project(&book, &pay, None).band, quiet);
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
		.project(date("2027-12-05"), &[], None)
		.unwrap();
	let flows: Vec<(String, i64)> = projection
		.months
		.iter()
		.take(3)
		.map(|month| (month.month.to_string(), month.baselined_net_flow.minor()))
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
	let projection = project(&book, &flows, None);
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
	let projection = project(&book, &[], None);
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
			None,
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
fn an_extra_goal_contribution_this_month_moves_the_eta() {
	let mut book = Book::household();
	book.category("New laptop", "Goals");
	book.goal("laptop", "New laptop", 150000, Some("2027-12-15"));
	book.budgeted("2026-12", "New laptop", 10000);
	let eta = |scenario: Option<&Scenario>| {
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
	assert_eq!(month_of(eta(None)), Some(ym("2027-06")));
	let bonus = scenario(
		"Bonus this month",
		vec![ScenarioChange::GoalContribution {
			goal: GoalId::new("laptop"),
			amount: cents(20000),
			start_month: ym("2027-01"),
			end_month: Some(ym("2027-01")),
		}],
	);
	// 50000 left, 20000 extra now, then 10000 a month: 3 more months.
	assert_eq!(month_of(eta(Some(&bonus))), Some(ym("2027-04")));
}

#[test]
fn an_unpaid_bill_due_today_still_lowers_the_projection() {
	let mut book = Book::household();
	book.opening("CHK", "2026-12-31", 400000);
	let rent_due_today = known(TODAY, "Rent", -165000, FlowKind::Expense);
	let projection = project(&book, &[rent_due_today], None);
	assert_eq!(projection.months[0].net_worth, cents(400000 - 165000));
}

#[test]
fn an_invalid_cadence_is_rejected_not_projected() {
	let scenario = scenario(
		"Never",
		vec![ScenarioChange::Recurring {
			start: date("2027-02-01"),
			end_month: None,
			cadence: Cadence::EveryWeeks(0),
			account: acct("CHK"),
			category: None,
			amount: cents(100),
		}],
	);
	let result = Forecaster::new(&household().budget).unwrap().project(
		date(TODAY),
		&household_flows(),
		Some(&scenario),
	);
	assert!(matches!(result, Err(CoreError::Invalid(_))));
}

/// Paychecks that post from an uncategorized schedule in some months and
/// are typed by hand in others, next to side income with no schedule.
fn mixed_paychecks() -> Book {
	let mut book = Book::household();
	book.opening("CHK", "2026-06-30", 100000);
	for (month, linked) in HISTORY.iter().zip([true, false, true, false, true, false]) {
		let pay = book.spend("Paycheck", &format!("{month}-01"), 300000);
		if linked {
			pay.bill("pay");
		}
		book.spend("Side gigs", &format!("{month}-25"), 20000);
	}
	book
}

#[test]
fn hand_typed_paychecks_are_not_counted_on_top_of_their_schedule() {
	let book = mixed_paychecks();
	let mut pay = known("2027-02-01", "Paycheck", 300000, FlowKind::Income);
	pay.category = None;
	pay.schedule = Some(veritas_domain::ScheduleId::new("pay"));
	let projection = project(&book, &[pay], None);
	assert_eq!(projection.months[1].net_income, cents(300000 + 20000));
	let side = projection
		.baselines
		.iter()
		.find(|baseline| baseline.category == cat("Side gigs"))
		.unwrap();
	assert_eq!(
		side.level,
		cents(20000),
		"unscheduled income stays baselined"
	);
}

#[test]
fn interest_the_payment_does_not_cover_never_capitalizes() {
	let mut book = Book::household();
	book.opening("CHK", "2026-12-31", 200000)
		.opening("LOAN", "2026-12-31", -1_500_000);
	let loan = LoanPayment {
		from: acct("CHK"),
		loan: acct("LOAN"),
		name: "Student loan".to_owned(),
		payment: cents(10000),
		rate_bps: Some(1000),
		last_interest: cents(0),
		escrow: cents(0),
		outstanding: cents(1_500_000),
		interest_category: Some(cat("Loan interest")),
		escrow_category: None,
		principal_category: Some(cat("Loan principal")),
	};
	let dates = [date("2027-02-10"), date("2027-03-10"), date("2027-04-10")];
	let projection = project(&book, &loan.flows(&dates).unwrap(), None);
	let path: Vec<(i64, i64, i64)> = projection.months[1..4]
		.iter()
		.map(|month| {
			(
				month.balances[&acct("CHK")].minor(),
				month.balances[&acct("LOAN")].minor(),
				month.net_income.minor(),
			)
		})
		.collect();
	assert_eq!(
		path,
		[
			(190000, -1_500_000, -10000),
			(180000, -1_500_000, -10000),
			(170000, -1_500_000, -10000),
		],
		"FC-2: a shortfall never capitalizes (DEBT-2.2, DEBT-2.6)"
	);
	assert_eq!(
		projection.months[3].net_worth,
		cents(200000 - 1_500_000 - 30000)
	);
}

#[test]
fn a_loan_below_its_interest_budgets_only_the_cash_that_leaves_checking() {
	let book = Book::household();
	let loan = LoanPayment {
		from: acct("CHK"),
		loan: acct("LOAN"),
		name: "Student loan".to_owned(),
		payment: cents(10000),
		rate_bps: Some(1000),
		last_interest: cents(0),
		escrow: cents(0),
		outstanding: cents(1_500_000),
		interest_category: Some(cat("Loan interest")),
		escrow_category: None,
		principal_category: Some(cat("Loan principal")),
	};
	let flows = loan.flows(&[date("2027-02-10")]).unwrap();
	let suggestions = Forecaster::new(&book.budget)
		.unwrap()
		.suggestions(date(TODAY), &flows, &BTreeMap::new())
		.unwrap();
	let interest = suggestions
		.items
		.iter()
		.find(|item| item.category == cat("Loan interest"))
		.unwrap();
	assert_eq!(interest.amount, Some(cents(10000)), "{}", interest.reason);
}

fn mortgage(payment: i64) -> LoanPayment {
	LoanPayment {
		from: acct("CHK"),
		loan: acct("LOAN"),
		name: "Mortgage".to_owned(),
		payment: cents(payment),
		rate_bps: Some(600),
		last_interest: cents(0),
		escrow: cents(50000),
		outstanding: cents(30_000_000),
		interest_category: Some(cat("Loan interest")),
		escrow_category: Some(cat("Property tax and insurance")),
		principal_category: Some(cat("Loan principal")),
	}
}

fn suggested(book: &Book, flows: &[KnownFlow], category: &str) -> (Option<i64>, String) {
	let suggestions = Forecaster::new(&book.budget)
		.unwrap()
		.suggestions(date(TODAY), flows, &BTreeMap::new())
		.unwrap();
	let item = suggestions
		.items
		.iter()
		.find(|item| item.category == cat(category))
		.unwrap();
	(
		item.amount.map(|amount| amount.minor()),
		item.reason.clone(),
	)
}

#[test]
fn a_mortgage_with_escrow_funds_interest_and_escrow_envelopes_separately() {
	let mut book = Book::household();
	book.category("Property tax and insurance", "Bills");
	let flows = mortgage(220000).flows(&[date("2027-02-01")]).unwrap();
	assert_eq!(
		suggested(&book, &flows, "Loan interest"),
		(
			Some(150000),
			"Loan interest $1500, scheduled: Mortgage".to_owned()
		)
	);
	assert_eq!(
		suggested(&book, &flows, "Property tax and insurance"),
		(
			Some(50000),
			"Property tax and insurance $500, scheduled: Mortgage".to_owned()
		)
	);

	// FL-3: 160000 covers the interest and only 10000 of the escrow.
	let short = mortgage(160000).flows(&[date("2027-02-01")]).unwrap();
	assert_eq!(
		suggested(&book, &short, "Property tax and insurance").0,
		Some(10000)
	);
	assert_eq!(suggested(&book, &short, "Loan interest").0, Some(150000));
}

/// Car upkeep of 6000 a month on checking, with a car bought into an
/// off-budget asset account in the same category.
fn car_book(purchase_on: Option<&str>) -> Book {
	let mut book = Book::household();
	book.account("CAR", true).category("Car", "Everyday");
	book.opening("CHK", "2026-06-30", 3_000_000);
	for month in HISTORY {
		book.spend("Car", &format!("{month}-12"), -6000);
	}
	book.spend("Car", "2027-01-05", -2000);
	if let Some(on) = purchase_on {
		book.transfer("CHK", "CAR", on, -1_800_000)
			.in_category("Car");
	}
	book
}

#[test]
fn buying_a_car_this_month_leaves_the_upkeep_forecast_alone() {
	let without = project(&car_book(None), &[], None);
	let with = project(&car_book(Some("2027-01-10")), &[], None);
	let car = |projection: &Projection| {
		projection
			.baselines
			.iter()
			.find(|baseline| baseline.category == cat("Car"))
			.map(|baseline| (baseline.level.minor(), baseline.month_to_date.minor()))
			.unwrap()
	};
	assert_eq!(car(&with), (6000, 2000));
	assert_eq!(with.months[0].baselined_net_flow, cents(-4000));
	assert_eq!(with.net_worth_today, without.net_worth_today);
	assert_eq!(with.balances_today[&acct("CAR")], cents(1_800_000));
	assert_eq!(
		with.horizons[3].net_worth, without.horizons[3].net_worth,
		"owning the car instead of the cash changes no projected net worth"
	);
}

#[test]
fn a_car_bought_and_sold_in_the_window_moves_no_baseline_band_or_suggestion() {
	let quiet = car_book(None);
	let mut traded = car_book(Some("2026-09-20"));
	traded
		.transfer("CHK", "CAR", "2026-11-20", 1_500_000)
		.in_category("Car");
	let base = project(&quiet, &[], None);
	let projection = project(&traded, &[], None);
	assert_eq!(projection.baselines, base.baselines);
	assert_eq!(projection.band, base.band);
	for (traded, quiet) in projection.months.iter().zip(&base.months) {
		assert_eq!(traded.net_income, quiet.net_income, "{}", traded.month);
		assert_eq!(
			traded.net_worth, quiet.net_worth,
			"the car still holds the money"
		);
		assert_eq!(
			traded.balances[&acct("CHK")].minor(),
			quiet.balances[&acct("CHK")].minor() - 300000
		);
		assert_eq!(traded.balances[&acct("CAR")], cents(300000));
	}
	assert_eq!(projection.asset_purchases, 2);
	assert_eq!(
		suggested(&traded, &[], "Car"),
		(Some(6000), "Car $60, 6-month median".to_owned())
	);
}

#[test]
fn rent_still_owed_mid_month_lands_where_paid_rent_would() {
	let paid = project(&household(), &household_flows(), None);
	let mut unpaid = household();
	unpaid.budget.transactions.retain(|_, row| {
		!(row.date == date("2027-01-01")
			&& row.schedule.as_ref().map(|s| s.as_str()) == Some("Rent"))
	});
	let mut flows = household_flows();
	flows.push(known("2027-01-01", "Rent", -165000, FlowKind::Expense));
	let projection = project(&unpaid, &flows, None);

	assert_eq!(
		projection.net_worth_today.minor() - paid.net_worth_today.minor(),
		165000
	);
	assert_eq!(projection.months[0].net_worth, paid.months[0].net_worth);
	assert_eq!(projection.months[0].balances, paid.months[0].balances);
	assert_eq!(projection.horizons, paid.horizons);
	let overdue: Vec<(&str, String)> = projection
		.overdue
		.iter()
		.map(|flow| (flow.name.as_str(), flow.date.to_string()))
		.collect();
	assert_eq!(overdue, [("Rent", "2027-01-01".to_owned())]);
}
