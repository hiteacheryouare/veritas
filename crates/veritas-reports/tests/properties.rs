//! R-REP-1, R-REP-2, R-REP-7 on random budgets: every live non-parent row
//! gets exactly one kind, the Net Profit identities (CLS-6) tie, and the
//! statement checks of ST-3 hold on every period.

#![allow(clippy::unwrap_used)]

mod support;

use std::collections::BTreeSet;

use proptest::prelude::*;
use support::*;
use veritas_domain::{
	AccountId, AccountKind, AccountMeta, BudgetData, CategoryKind, CivilDate, Money, SplitRole,
	TransactionId, TransactionKind, YearMonth,
};
use veritas_reports::{
	Classification, Leg, NetProfit, NodeKind, Period, Sankey, Section, balance_sheet, cash_flow,
	net_profit, net_worth_by_month, owners_equity, sankey, tax_summary,
};

// Index ACCOUNTS names an account that is never created, so rows there are
// orphaned and transfers to it lose their counterpart account.
const ACCOUNTS: usize = 4;
const CATEGORIES: usize = 5;

#[derive(Clone, Debug)]
struct AccountSpec {
	off_budget: bool,
	kind: Option<AccountKind>,
	closed: bool,
}

#[derive(Clone, Debug)]
struct CategorySpec {
	is_income: bool,
	kind: Option<CategoryKind>,
	hidden: bool,
	merged_into: Option<usize>,
	unmapped: bool,
}

#[derive(Clone, Debug)]
enum Op {
	Plain {
		account: usize,
		day: (u32, u32),
		amount: i64,
		category: Option<usize>,
		starting_balance: bool,
		reconciliation_note: bool,
		veritas_adjustment: bool,
		tombstone: bool,
	},
	Transfer {
		from: usize,
		to: usize,
		day: (u32, u32),
		amount: i64,
		category: Option<usize>,
		mirror: MirrorState,
	},
	Split {
		account: usize,
		day: (u32, u32),
		children: Vec<(i64, Option<usize>, Option<usize>)>,
		parent_tombstone: bool,
	},
}

#[derive(Clone, Debug)]
struct Scenario {
	accounts: Vec<AccountSpec>,
	categories: Vec<CategorySpec>,
	ops: Vec<Op>,
}

#[derive(Clone, Copy, Debug)]
enum MirrorState {
	Present,
	Missing,
	Tombstoned,
	OneWay,
	Dated((u32, u32)),
	OffBy(i64),
}

fn kind() -> impl Strategy<Value = Option<CategoryKind>> {
	prop_oneof![
		Just(None),
		Just(Some(CategoryKind::Income)),
		Just(Some(CategoryKind::Expense)),
		Just(Some(CategoryKind::DebtPrincipal)),
		Just(Some(CategoryKind::Savings)),
	]
}

fn day() -> impl Strategy<Value = (u32, u32)> {
	(1u32..=3, 1u32..=28)
}

fn amount() -> impl Strategy<Value = i64> {
	-1_000_000i64..1_000_000
}

fn op() -> impl Strategy<Value = Op> {
	let category = proptest::option::of(0..CATEGORIES);
	prop_oneof![
		(
			0..=ACCOUNTS,
			day(),
			amount(),
			category.clone(),
			proptest::bool::weighted(0.1),
			proptest::bool::weighted(0.1),
			proptest::bool::weighted(0.05),
			proptest::bool::weighted(0.1),
		)
			.prop_map(
				|(
					account,
					day,
					amount,
					category,
					starting_balance,
					reconciliation_note,
					veritas_adjustment,
					tombstone,
				)| Op::Plain {
					account,
					day,
					amount,
					category,
					starting_balance,
					reconciliation_note,
					veritas_adjustment,
					tombstone,
				}
			),
		(
			0..ACCOUNTS,
			0..=ACCOUNTS,
			day(),
			amount(),
			category.clone(),
			prop_oneof![
				4 => Just(MirrorState::Present),
				1 => Just(MirrorState::Missing),
				1 => Just(MirrorState::Tombstoned),
				1 => Just(MirrorState::OneWay),
				2 => day().prop_map(MirrorState::Dated),
				1 => (1i64..5_000).prop_map(MirrorState::OffBy),
			],
		)
			.prop_map(|(from, to, day, amount, category, mirror)| Op::Transfer {
				from,
				to,
				day,
				amount,
				category,
				mirror,
			}),
		(
			0..ACCOUNTS,
			day(),
			proptest::collection::vec(
				(
					amount(),
					category,
					proptest::option::weighted(0.3, 0..=ACCOUNTS)
				),
				1..4
			),
			proptest::bool::weighted(0.15),
		)
			.prop_map(|(account, day, children, parent_tombstone)| Op::Split {
				account,
				day,
				children,
				parent_tombstone,
			}),
	]
}

fn scenario() -> impl Strategy<Value = Scenario> {
	(
		proptest::collection::vec(
			(
				any::<bool>(),
				prop_oneof![
					Just(None),
					Just(Some(AccountKind::Asset)),
					Just(Some(AccountKind::Liability)),
				],
				proptest::bool::weighted(0.2),
			)
				.prop_map(|(off_budget, kind, closed)| AccountSpec {
					off_budget,
					kind,
					closed,
				}),
			ACCOUNTS,
		),
		proptest::collection::vec(
			(
				any::<bool>(),
				kind(),
				proptest::bool::weighted(0.2),
				proptest::option::weighted(0.2, 0..CATEGORIES),
				proptest::bool::weighted(0.1),
			)
				.prop_map(
					|(is_income, kind, hidden, merged_into, unmapped)| CategorySpec {
						is_income,
						kind,
						hidden,
						merged_into,
						unmapped,
					},
				),
			CATEGORIES,
		),
		proptest::collection::vec(op(), 0..40),
	)
		.prop_map(|(accounts, categories, ops)| Scenario {
			accounts,
			categories,
			ops,
		})
}

fn account(index: usize) -> String {
	format!("A{index}")
}

fn category(index: usize) -> String {
	format!("C{index}")
}

fn on(day: (u32, u32)) -> String {
	format!("2027-{:02}-{:02}", day.0, day.1)
}

fn build(scenario: &Scenario) -> Book {
	let Scenario {
		accounts,
		categories,
		ops,
	} = scenario;
	let mut book = Book::new();
	book.payee("Store");
	book.group("Income", true).group("Spending", false);
	for (index, spec) in accounts.iter().enumerate() {
		book.account(&account(index), spec.off_budget);
		book.budget
			.accounts
			.get_mut(&AccountId::new(account(index)))
			.unwrap()
			.closed = spec.closed;
		if spec.kind.is_some() {
			book.budget.account_meta.insert(
				AccountId::new(account(index)),
				AccountMeta {
					kind: spec.kind,
					..AccountMeta::default()
				},
			);
		}
	}
	for (index, spec) in categories.iter().enumerate() {
		let group = if spec.is_income { "Income" } else { "Spending" };
		book.category(&category(index), group, spec.kind);
		book.budget
			.categories
			.get_mut(&cat(&category(index)))
			.unwrap()
			.hidden = spec.hidden;
		if spec.unmapped {
			book.budget.category_mapping.remove(&cat(&category(index)));
		}
		if let Some(target) = spec.merged_into {
			book.budget
				.category_mapping
				.insert(cat(&category(index)), cat(&category(target)));
		}
	}
	for (index, op) in ops.iter().enumerate() {
		let id = format!("t{index:02}");
		match op {
			Op::Plain {
				account: from,
				day,
				amount,
				category: chosen,
				starting_balance,
				reconciliation_note,
				veritas_adjustment,
				tombstone,
			} => {
				let row = book.row(&id, &account(*from), &on(*day), *amount);
				row.category = chosen.map(|chosen| cat(&category(chosen)));
				row.starting_balance = *starting_balance;
				row.tombstone = *tombstone;
				if *reconciliation_note {
					row.notes = Some(veritas_reports::RECONCILIATION_NOTE.to_owned());
				} else {
					row.payee = Some(veritas_domain::PayeeId::new("Store"));
				}
				if *veritas_adjustment {
					book.mark_adjustment(&id);
				}
			}
			Op::Transfer {
				from,
				to,
				day,
				amount,
				category: chosen,
				mirror,
			} => {
				book.transfer(&id, &account(*from), &account(*to), &on(*day), *amount)
					.category = chosen.map(|chosen| cat(&category(chosen)));
				let mirror_id = TransactionId::new(format!("{id}m"));
				match mirror {
					MirrorState::Present => {}
					MirrorState::Missing => {
						book.budget.transactions.remove(&mirror_id);
					}
					MirrorState::Tombstoned => {
						book.budget
							.transactions
							.get_mut(&mirror_id)
							.unwrap()
							.tombstone = true;
					}
					MirrorState::OneWay => {
						book.budget
							.transactions
							.get_mut(&mirror_id)
							.unwrap()
							.transfer = None;
					}
					MirrorState::Dated(other_day) => {
						book.budget.transactions.get_mut(&mirror_id).unwrap().date =
							date(&on(*other_day));
					}
					MirrorState::OffBy(delta) => {
						let mirror = book.budget.transactions.get_mut(&mirror_id).unwrap();
						mirror.amount = Money::from_minor(mirror.amount.minor() + delta);
					}
				}
			}
			Op::Split {
				account: from,
				day,
				children,
				parent_tombstone,
			} => {
				let total = children.iter().map(|child| child.0).sum();
				book.parent(&id, &account(*from), &on(*day), total)
					.tombstone = *parent_tombstone;
				for (child, (amount, chosen, transfer_to)) in children.iter().enumerate() {
					let child_id = format!("{id}-{child}");
					let row = match transfer_to {
						Some(to) => book.transfer(
							&child_id,
							&account(*from),
							&account(*to),
							&on(*day),
							*amount,
						),
						None => book.row(&child_id, &account(*from), &on(*day), *amount),
					};
					row.category = chosen.map(|chosen| cat(&category(chosen)));
					row.child_of(&id);
				}
			}
		}
	}
	tag_everything(&mut book);
	book
}

const NOTES: [&str; 7] = [
	"",
	"#medical",
	"#medical #education",
	"#notax #medical",
	"#rx#medical",
	"##medical",
	"#hsa",
];
const CATEGORY_BUCKETS: [Option<&str>; 4] =
	[None, Some("charitable"), Some("w2_income"), Some("bogus")];

/// Spreads tags over the rows and tax buckets over the categories, so the
/// tax summary sees tags, conflicts, exclusions and category buckets.
fn tag_everything(book: &mut Book) {
	for (tag, bucket) in [
		("medical", "medical"),
		("rx", "medical"),
		("education", "education"),
		("notax", "none"),
		("hsa", "hsa"),
	] {
		book.budget
			.tax_tags
			.insert(tag.to_owned(), bucket.to_owned());
	}
	for (index, transaction) in book.budget.transactions.values_mut().enumerate() {
		if transaction.notes.is_none() {
			let notes = NOTES[index % NOTES.len()];
			transaction.notes = (!notes.is_empty()).then(|| notes.to_owned());
		}
	}
	for index in 0..CATEGORIES {
		if let Some(bucket) = CATEGORY_BUCKETS[index % CATEGORY_BUCKETS.len()] {
			book.budget
				.category_meta
				.entry(cat(&category(index)))
				.or_insert(veritas_domain::CategoryMeta {
					kind: None,
					tax_bucket: None,
					archived_on: None,
				})
				.tax_bucket = Some(bucket.to_owned());
		}
	}
}

fn live_rows(budget: &BudgetData) -> BTreeSet<TransactionId> {
	budget
		.transactions
		.values()
		.filter(|row| !row.tombstone)
		.filter(|row| match &row.split {
			SplitRole::None => true,
			SplitRole::Parent => false,
			SplitRole::Child { parent } => budget
				.transactions
				.get(parent)
				.is_some_and(|parent| !parent.tombstone),
		})
		.map(|row| row.id.clone())
		.collect()
}

fn signed_sum(classification: &Classification, rows: &[TransactionId]) -> i64 {
	rows.iter()
		.map(|id| classification.get(id).unwrap().amount.minor())
		.sum()
}

fn assert_section_ties(classification: &Classification, section: &Section, sign: i64) {
	assert_eq!(
		section.total.minor(),
		sign * signed_sum(classification, &section.rows)
	);
	let by_category: i64 = section
		.categories
		.iter()
		.map(|line| line.total.minor())
		.sum();
	assert_eq!(section.total.minor(), by_category);
	let mut from_lines: Vec<_> = section
		.categories
		.iter()
		.flat_map(|line| line.rows.iter().cloned())
		.collect();
	from_lines.sort();
	let mut rows = section.rows.clone();
	rows.sort();
	assert_eq!(from_lines, rows);
	for id in &section.rows {
		assert!(classification.get(id).unwrap().is_primary());
	}
}

fn assert_report_ties(classification: &Classification, report: &NetProfit) {
	let minor = |money: Money| money.minor();
	assert_eq!(
		minor(report.income.total) - minor(report.expenses.total),
		minor(report.net_profit.total)
	);
	assert_eq!(
		minor(report.net_profit.total)
			- minor(report.debt_paydown.total)
			- minor(report.savings.total),
		minor(report.net_cash_flow.total)
	);
	assert_section_ties(classification, &report.income, 1);
	assert_section_ties(classification, &report.expenses, -1);
	assert_section_ties(classification, &report.debt_paydown, -1);
	assert_section_ties(classification, &report.savings, -1);
	assert_eq!(
		minor(report.unreviewed.total),
		-signed_sum(classification, &report.unreviewed.rows)
	);
	let mut sections: Vec<_> = [
		&report.income.rows,
		&report.expenses.rows,
		&report.debt_paydown.rows,
		&report.savings.rows,
	]
	.into_iter()
	.flatten()
	.cloned()
	.collect();
	sections.sort_by_key(|id| (classification.get(id).unwrap().date, id.clone()));
	assert_eq!(sections, report.net_cash_flow.rows);
}

fn in_2027(month: u32, day: u32) -> CivilDate {
	CivilDate::new(2027, month, day).unwrap()
}

proptest! {
	#![proptest_config(ProptestConfig::with_cases(512))]

	#[test]
	fn every_live_non_parent_row_gets_exactly_one_kind(scenario in scenario()) {
		let book = build(&scenario);
		let classification = book.classify();
		let classified: Vec<_> = classification.rows().iter().map(|row| row.id.clone()).collect();
		let unique: BTreeSet<_> = classified.iter().cloned().collect();
		prop_assert_eq!(unique.len(), classified.len(), "a row was classified twice");
		let orphans: BTreeSet<_> = classification.orphans().iter().cloned().collect();
		prop_assert!(unique.is_disjoint(&orphans));
		let live = live_rows(&book.budget);
		let expected_orphans: BTreeSet<_> = live
			.iter()
			.filter(|id| !book.budget.accounts.contains_key(&book.budget.transactions[*id].account))
			.cloned()
			.collect();
		prop_assert_eq!(&orphans, &expected_orphans);
		prop_assert_eq!(unique, &live - &orphans);

		for row in classification.rows() {
			prop_assert_eq!(row.kind(), row.decision.kind());
			if row.leg == Leg::Mirror {
				prop_assert!(!row.on_budget, "{} is a mirror in an on-budget account", row.id);
				let partner = book.budget.transactions[&row.id].transfer.clone().unwrap();
				let primary = classification.get(&partner).unwrap();
				prop_assert!(primary.on_budget && primary.is_primary());
				prop_assert_eq!(
					book.budget.transactions[&partner].transfer.as_ref(),
					Some(&row.id),
					"a mirror's link must be reciprocal"
				);
				prop_assert_eq!(primary.decision, row.decision);
				prop_assert_eq!(primary.amount.minor() + row.amount.minor(), 0);
			}
			if row.adjustment_source().is_some() {
				prop_assert_eq!(row.kind(), TransactionKind::Adjustment);
			}
		}
	}

	#[test]
	fn net_profit_identities_tie_and_periods_add_up(scenario in scenario()) {
		let book = build(&scenario);
		let classification = book.classify();
		let report = |first: CivilDate, last: CivilDate| {
			net_profit(&book.budget, &classification, Period::new(first, last).unwrap()).unwrap()
		};
		let january = report(in_2027(1, 1), in_2027(1, 31));
		let february = report(in_2027(2, 1), in_2027(2, 28));
		let both = report(in_2027(1, 1), in_2027(2, 28));
		for period in [&january, &february, &both] {
			assert_report_ties(&classification, period);
		}
		let sum = |pick: fn(&NetProfit) -> Money| pick(&january).minor() + pick(&february).minor();
		prop_assert_eq!(sum(|r| r.income.total), both.income.total.minor());
		prop_assert_eq!(sum(|r| r.expenses.total), both.expenses.total.minor());
		prop_assert_eq!(sum(|r| r.net_cash_flow.total), both.net_cash_flow.total.minor());
		prop_assert_eq!(sum(|r| r.unreviewed.total), both.unreviewed.total.minor());
	}

	#[test]
	fn statements_always_balance_and_reconcile(scenario in scenario()) {
		let book = build(&scenario);
		let classification = book.classify();
		let periods = [
			(in_2027(1, 1), in_2027(1, 31)),
			(in_2027(2, 1), in_2027(2, 28)),
			(in_2027(1, 15), in_2027(3, 10)),
			(in_2027(3, 1), in_2027(3, 31)),
		];
		for (first, last) in periods {
			let period = Period::new(first, last).unwrap();
			let equity = owners_equity(&book.budget, &classification, period).unwrap();
			prop_assert_eq!(equity.unexplained().unwrap(), Money::ZERO, "ST-4 on {:?}", period);
			let sheet = balance_sheet(&book.budget, &classification, last).unwrap();
			prop_assert_eq!(
				sheet.total_assets.checked_sub(sheet.total_liabilities).unwrap(),
				sheet.net_worth,
				"ST-3 check 1"
			);
			let shown: i64 = sheet
				.assets
				.iter()
				.chain(&sheet.liabilities)
				.map(|section| section.total.minor())
				.sum::<i64>();
			let assets: i64 = sheet.assets.iter().map(|section| section.total.minor()).sum();
			prop_assert_eq!(assets, sheet.total_assets.minor());
			prop_assert_eq!(shown - assets, sheet.total_liabilities.minor());
			prop_assert_eq!(sheet.net_worth, equity.closing, "ST-3 check 3");
			let report = net_profit(&book.budget, &classification, period).unwrap();
			prop_assert_eq!(&equity.net_income, &report.net_profit, "ST-3 check 2");
			let flow = cash_flow(&book.budget, &classification, period).unwrap();
			prop_assert_eq!(flow.unexplained().unwrap(), Money::ZERO, "ST-3 check 4 on {:?}", period);
		}

		let january = owners_equity(&book.budget, &classification, Period::new(in_2027(1, 1), in_2027(1, 31)).unwrap()).unwrap();
		let february = owners_equity(&book.budget, &classification, Period::new(in_2027(2, 1), in_2027(2, 28)).unwrap()).unwrap();
		prop_assert_eq!(january.closing, february.opening);

		let series = net_worth_by_month(
			&book.budget,
			&classification,
			YearMonth::new(2027, 1).unwrap(),
			YearMonth::new(2027, 3).unwrap(),
		)
		.unwrap();
		for point in series {
			let last = Period::month(point.month).last();
			let sheet = balance_sheet(&book.budget, &classification, last).unwrap();
			prop_assert_eq!(
				(point.assets, point.liabilities, point.net_worth),
				(sheet.total_assets, sheet.total_liabilities, sheet.net_worth)
			);
		}
	}

	#[test]
	fn sankey_invariants_hold_on_every_range(scenario in scenario()) {
		let book = build(&scenario);
		let classification = book.classify();
		for (first, last) in [
			(in_2027(1, 1), in_2027(1, 31)),
			(in_2027(1, 10), in_2027(3, 20)),
			(in_2027(3, 1), in_2027(3, 31)),
		] {
			let period = Period::new(first, last).unwrap();
			let graph = sankey(&book.budget, &classification, period).unwrap();
			let report = net_profit(&book.budget, &classification, period).unwrap();
			assert_sankey_invariants(&graph, &report);
		}
	}

	#[test]
	fn tax_totals_equal_their_detail_rows(scenario in scenario()) {
		let book = build(&scenario);
		let classification = book.classify();
		let summary = tax_summary(&book.budget, &classification, 2027).unwrap();
		let mut seen = BTreeSet::new();
		for row in &summary.detail {
			prop_assert!(seen.insert(row.row.clone()), "TAX-6 invariant 2: {} twice", row.row);
			let classified = classification.get(&row.row).unwrap();
			prop_assert!(classified.is_primary(), "TAX-6 invariant 2: a mirror leg");
			prop_assert!(matches!(
				classified.kind(),
				TransactionKind::Income
					| TransactionKind::Expense
					| TransactionKind::DebtPrincipal
					| TransactionKind::SavingsContribution
			));
			prop_assert_eq!(row.counted.is_some(), row.bucket.is_some());
		}
		for total in &summary.buckets {
			let detail: i64 = summary
				.detail
				.iter()
				.filter(|row| row.bucket == Some(total.bucket))
				.map(|row| row.counted.unwrap().minor())
				.sum();
			prop_assert_eq!(total.total.minor(), detail, "TAX-6 invariant 1");
		}
	}
}

fn assert_sankey_invariants(graph: &Sankey, report: &NetProfit) {
	let value = |kind: NodeKind| graph.node(&kind).map_or(0, |node| node.value.minor());
	for node in &graph.nodes {
		assert!(node.value > Money::ZERO, "{:?}", node.kind);
	}
	let middle = graph
		.nodes
		.iter()
		.position(|node| node.kind == NodeKind::Income);
	let into: i64 = graph
		.flows
		.iter()
		.filter(|flow| Some(flow.to) == middle)
		.map(|flow| flow.value.minor())
		.sum();
	let out: i64 = graph
		.flows
		.iter()
		.filter(|flow| Some(flow.from) == middle)
		.map(|flow| flow.value.minor())
		.sum();
	let income = value(NodeKind::Income);
	assert_eq!((into, out), (income, income), "ST-8 invariant 2");
	let sources: i64 = graph
		.nodes
		.iter()
		.filter(|node| matches!(node.kind, NodeKind::IncomeCategory(_)))
		.map(|node| node.value.minor())
		.sum();
	assert_eq!(
		sources - value(NodeKind::IncomeReversals),
		report.income.total.minor(),
		"ST-8 invariant 1"
	);
	let gap = report.net_cash_flow.total.minor() - report.unreviewed.total.minor();
	if value(NodeKind::Shortfall) == 0 {
		assert_eq!(value(NodeKind::Unallocated), gap, "ST-8 invariant 3");
	} else {
		assert_eq!(value(NodeKind::Unallocated), 0, "ST-8 invariant 3");
		assert_eq!(value(NodeKind::Shortfall), -gap, "ST-8 invariant 3");
	}
	for (index, node) in graph.nodes.iter().enumerate() {
		if matches!(node.kind, NodeKind::CategoryGroup(_)) {
			let children: i64 = graph
				.flows
				.iter()
				.filter(|flow| flow.from == index)
				.map(|flow| flow.value.minor())
				.sum();
			assert_eq!(
				children,
				node.value.minor(),
				"a group equals its categories"
			);
		}
	}
}
