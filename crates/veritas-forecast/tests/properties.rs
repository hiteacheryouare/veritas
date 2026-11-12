//! Properties on random budgets: a scenario (R-FC-2) never changes the
//! baseline's inputs or outputs, a projection's balances always add up to its
//! net worth (R-FC-1), and redacted Wrapped shares (R-WRAP-2) sum to 1000.

#![allow(clippy::unwrap_used, clippy::panic)]

mod support;

use proptest::prelude::*;
use support::*;
use veritas_domain::{GoalId, PayeeId};
use veritas_forecast::{
	Cadence, Certainty, FlowKind, Forecaster, IncomeDelta, KnownFlow, Scenario, ScenarioEvent,
	wrapped,
};
use veritas_reports::classify;

const CATEGORIES: [&str; 8] = [
	"Paycheck",
	"Side gigs",
	"Rent",
	"Electric",
	"Groceries",
	"Dining",
	"Gifts",
	"Fun",
];
const ACCOUNTS: [&str; 3] = ["CHK", "VISA", "SAV"];
const TODAY: &str = "2027-01-15";

#[derive(Clone, Debug)]
struct RowSpec {
	category: usize,
	account: usize,
	month: i32,
	day: u32,
	amount: i64,
	bill: bool,
}

fn row_spec() -> impl Strategy<Value = RowSpec> {
	(
		0..CATEGORIES.len(),
		0..2usize,
		0..25i32,
		1..=28u32,
		-60_000..30_000i64,
		any::<bool>(),
	)
		.prop_map(|(category, account, month, day, amount, bill)| RowSpec {
			category,
			account,
			month,
			day,
			amount,
			bill: bill && amount % 3 == 0,
		})
}

fn build(rows: &[RowSpec]) -> Book {
	let mut book = Book::household();
	book.opening("CHK", "2024-12-31", 500_000);
	for spec in rows {
		let month = ym("2025-01").offset(spec.month).unwrap();
		let row = book.row(
			ACCOUNTS[spec.account],
			&format!("{month}-{:02}", spec.day),
			spec.amount,
		);
		row.in_category(CATEGORIES[spec.category]);
		if spec.bill {
			row.bill(CATEGORIES[spec.category]);
		}
	}
	book
}

fn flow_spec() -> impl Strategy<Value = KnownFlow> {
	(
		0..13i32,
		1..=28u32,
		0..CATEGORIES.len(),
		-200_000..200_000i64,
		0..3usize,
	)
		.prop_map(|(month, day, category, amount, kind)| {
			let month = ym("2027-01").offset(month).unwrap();
			let kind = match kind {
				0 => FlowKind::Income,
				1 => FlowKind::Expense,
				_ => FlowKind::Transfer {
					counterpart: acct("SAV"),
				},
			};
			KnownFlow {
				date: date(&format!("{month}-{day:02}")),
				account: acct("CHK"),
				amount: cents(amount),
				kind,
				category: Some(cat(CATEGORIES[category])),
				schedule: None,
				name: CATEGORIES[category].to_owned(),
				certainty: Certainty::Exact,
			}
		})
}

fn event_spec() -> impl Strategy<Value = ScenarioEvent> {
	let month = || (0..14i32).prop_map(|step| ym("2027-01").offset(step).unwrap());
	prop_oneof![
		(1..14i32, 1..=28u32, -500_000..500_000i64).prop_map(|(step, day, amount)| {
			let month = ym("2027-01").offset(step).unwrap();
			ScenarioEvent::OneTime {
				date: date(&format!("{month}-{day:02}")),
				account: acct("CHK"),
				category: None,
				amount: cents(amount),
			}
		}),
		(
			month(),
			1..=28u32,
			1..4u32,
			any::<bool>(),
			-50_000..50_000i64
		)
			.prop_map(
				|(month, day, step, weeks, amount)| ScenarioEvent::Recurring {
					start: date(&format!("{month}-{day:02}")),
					end_month: None,
					cadence: if weeks {
						Cadence::EveryWeeks(step)
					} else {
						Cadence::EveryMonths(step)
					},
					account: acct("VISA"),
					category: None,
					amount: cents(amount),
				}
			),
		(month(), 0..CATEGORIES.len(), -10_000..10_000i64).prop_map(|(month, category, bp)| {
			ScenarioEvent::CategoryPercent {
				category: cat(CATEGORIES[category]),
				percent_bp: bp,
				start_month: month,
				end_month: None,
			}
		}),
		(month(), any::<bool>(), -3_000..3_000i64).prop_map(|(month, percent, value)| {
			ScenarioEvent::IncomeChange {
				account: acct("CHK"),
				category: None,
				change: if percent {
					IncomeDelta::Percent(value)
				} else {
					IncomeDelta::Amount(cents(value * 10))
				},
				start_month: month,
				end_month: None,
			}
		}),
		(month(), 0..100_000i64).prop_map(|(month, amount)| ScenarioEvent::GoalContribution {
			goal: GoalId::new("laptop"),
			amount: cents(amount),
			start_month: month,
			end_month: None,
		}),
	]
}

proptest! {
	#![proptest_config(ProptestConfig::with_cases(96))]

	#[test]
	fn r_fc_2_scenarios_never_change_the_baseline_or_its_inputs(
		rows in prop::collection::vec(row_spec(), 0..60),
		flows in prop::collection::vec(flow_spec(), 0..12),
		events in prop::collection::vec(event_spec(), 0..6),
	) {
		let book = build(&rows);
		let before = book.budget.clone();
		let flows_before = flows.clone();
		let forecaster = Forecaster::new(&book.budget).unwrap();
		let first = forecaster.project(date(TODAY), &flows, &Scenario::baseline()).unwrap();
		let scenario = Scenario { name: "What if".to_owned(), events };
		let comparison = forecaster.compare(date(TODAY), &flows, &[scenario]).unwrap();
		let after = forecaster.project(date(TODAY), &flows, &Scenario::baseline()).unwrap();

		prop_assert_eq!(&book.budget, &before);
		prop_assert_eq!(&flows, &flows_before);
		prop_assert_eq!(&first, &after);
		prop_assert_eq!(&comparison.baseline, &first);
		let overlay = &comparison.scenarios[0];
		prop_assert_eq!(&overlay.baselines, &first.baselines);
		prop_assert_eq!(&overlay.band, &first.band);
		prop_assert_eq!(overlay.net_worth_today, first.net_worth_today);
		prop_assert_eq!(&overlay.balances_today, &first.balances_today);
	}

	#[test]
	fn r_fc_1_balances_add_up_to_net_worth_every_month(
		rows in prop::collection::vec(row_spec(), 0..60),
		flows in prop::collection::vec(flow_spec(), 0..12),
		events in prop::collection::vec(event_spec(), 0..6),
	) {
		let book = build(&rows);
		let forecaster = Forecaster::new(&book.budget).unwrap();
		let scenario = Scenario { name: "What if".to_owned(), events };
		let projection = forecaster.project(date(TODAY), &flows, &scenario).unwrap();
		let mut worth = projection.net_worth_today.minor();
		for month in &projection.months {
			worth += month.net_income.minor();
			prop_assert_eq!(month.net_worth.minor(), worth);
			let total: i64 = month.balances.values().map(|balance| balance.minor()).sum();
			prop_assert_eq!(total, worth, "{}", month.month);
		}
	}

	#[test]
	fn r_wrap_2_redacted_shares_sum_to_exactly_1000(
		rows in prop::collection::vec(row_spec(), 0..60),
		payees in prop::collection::vec(0..9usize, 60),
	) {
		let mut book = build(&rows);
		for index in 0..9 {
			book.payee(&format!("payee {index}"));
		}
		let ids: Vec<_> = book.budget.transactions.keys().cloned().collect();
		for (id, payee) in ids.iter().zip(&payees) {
			book.budget.transactions.get_mut(id).unwrap().payee =
				Some(PayeeId::new(format!("payee {payee}")));
		}
		let classification = classify(&book.budget);
		for year in [2025, 2026] {
			let result = wrapped(&book.budget, &classification, year, date(TODAY)).unwrap();
			let redacted = result.redacted();
			let any_positive = result.top_categories.iter().any(|rank| rank.total.minor() > 0);
			let categories: i64 = redacted.categories.iter().map(|(_, share)| share).sum();
			prop_assert_eq!(categories, if any_positive { 1000 } else { 0 });
			prop_assert!(redacted.categories.iter().all(|(_, share)| *share >= 0));
			let payees: i64 = redacted.payees.iter().map(|(_, share)| share).sum();
			prop_assert_eq!(payees, if result.top_payees.is_empty() { 0 } else { 1000 });
		}
	}
}
