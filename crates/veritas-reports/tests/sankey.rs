//! R-REP-6: the Sankey graph per docs/finance/statements.md ST-8, its worked
//! examples X-1 to X-10 and its three invariants.

#![allow(clippy::unwrap_used, clippy::panic)]

mod support;

use support::*;
use veritas_domain::{CategoryKind, Money};
use veritas_reports::{NetProfit, NodeKind, Period, Sankey, sankey};

fn name(kind: &NodeKind) -> String {
	match kind {
		NodeKind::IncomeCategory(category) => category.to_string(),
		NodeKind::CategoryGroup(group) => group
			.as_ref()
			.map_or("(uncategorized)".to_owned(), ToString::to_string),
		NodeKind::Category(category) => category
			.as_ref()
			.map_or("(uncategorized)".to_owned(), ToString::to_string),
		other => format!("{other:?}"),
	}
}

fn graph(book: &Book, period: Period) -> (Sankey, NetProfit) {
	let classification = book.classify();
	let graph = sankey(&book.budget, &classification, period).unwrap();
	let report = book.net_profit(period);
	assert_invariants(&graph, &report);
	(graph, report)
}

fn income_index(graph: &Sankey) -> Option<usize> {
	graph
		.nodes
		.iter()
		.position(|node| node.kind == NodeKind::Income)
}

/// Every (node, value) flowing into Income, then every one flowing out of it.
type Named = Vec<(String, i64)>;

fn sides(graph: &Sankey) -> (Named, Named) {
	let Some(income) = income_index(graph) else {
		return (Vec::new(), Vec::new());
	};
	let collect = |pick: fn(&veritas_reports::Flow) -> (usize, usize)| {
		graph
			.flows
			.iter()
			.filter_map(|flow| {
				let (anchor, other) = pick(flow);
				(anchor == income).then(|| (name(&graph.nodes[other].kind), flow.value.minor()))
			})
			.collect::<Vec<_>>()
	};
	(
		collect(|flow| (flow.to, flow.from)),
		collect(|flow| (flow.from, flow.to)),
	)
}

fn value(graph: &Sankey, kind: NodeKind) -> i64 {
	graph.node(&kind).map_or(0, |node| node.value.minor())
}

fn assert_invariants(graph: &Sankey, report: &NetProfit) {
	for node in &graph.nodes {
		assert!(node.value > Money::ZERO, "{:?} is not positive", node.kind);
	}
	for flow in &graph.flows {
		assert!(flow.value > Money::ZERO);
	}
	let income_sources: i64 = graph
		.nodes
		.iter()
		.filter(|node| matches!(node.kind, NodeKind::IncomeCategory(_)))
		.map(|node| node.value.minor())
		.sum();
	assert_eq!(
		income_sources - value(graph, NodeKind::IncomeReversals),
		report.income.total.minor(),
		"ST-8 invariant 1"
	);
	let (sources, outflows) = sides(graph);
	let middle = value(graph, NodeKind::Income);
	assert_eq!(
		sources.iter().map(|(_, v)| v).sum::<i64>(),
		middle,
		"ST-8 invariant 2"
	);
	assert_eq!(
		outflows.iter().map(|(_, v)| v).sum::<i64>(),
		middle,
		"ST-8 invariant 2"
	);
	for (index, node) in graph.nodes.iter().enumerate() {
		let children: i64 = graph
			.flows
			.iter()
			.filter(|flow| flow.from == index && node.kind != NodeKind::Income)
			.map(|flow| flow.value.minor())
			.sum();
		if children != 0 {
			assert_eq!(
				children,
				node.value.minor(),
				"a group equals its categories"
			);
		}
	}
	let shortfall = value(graph, NodeKind::Shortfall);
	let unallocated = value(graph, NodeKind::Unallocated);
	let ncf = report.net_cash_flow.total.minor();
	let unreviewed = report.unreviewed.total.minor();
	if shortfall == 0 {
		assert_eq!(unallocated, ncf - unreviewed, "ST-8 invariant 3");
	} else {
		assert_eq!(unallocated, 0, "ST-8 invariant 3");
		assert_eq!(shortfall, unreviewed - ncf, "ST-8 invariant 3");
	}
}

fn named(pairs: &[(&str, i64)]) -> Vec<(String, i64)> {
	pairs
		.iter()
		.map(|(name, value)| ((*name).to_owned(), *value))
		.collect()
}

fn book() -> Book {
	let mut book = master_book();
	book.group("Housing", false)
		.category("Rent", "Housing", None)
		.category("Dining", "Food", None)
		.category("Interest", "Income", None);
	book
}

#[test]
fn st8_x1_january_master() {
	let (graph, _) = graph(&master(), month("2027-01"));
	let (sources, outflows) = sides(&graph);
	assert_eq!(sources, named(&[("Paycheck", 300_000)]));
	assert_eq!(
		outflows,
		named(&[
			("Food", 5_000),
			("Shopping", 2_500),
			("Debt costs", 8_000),
			("Big purchases", 50_000),
			("Savings", 20_000),
			("DebtPaydown", 32_000),
			("Unallocated", 182_500),
		])
	);
	assert_eq!(value(&graph, NodeKind::Income), 300_000);
}

#[test]
fn st8_x2_february_has_no_card_payment() {
	let (graph, _) = graph(&master(), month("2027-02"));
	let (_, outflows) = sides(&graph);
	assert_eq!(
		outflows,
		named(&[("Food", 2_500), ("Unallocated", 297_500)])
	);
	assert!(
		graph
			.nodes
			.iter()
			.all(|node| !node.rows.iter().any(|id| id.as_str().starts_with("T13")))
	);
}

#[test]
fn st8_x3_shortfall_when_spending_exceeds_income() {
	let mut book = book();
	book.row("Pay", "CHK", "2027-03-01", 100_000)
		.in_category("Paycheck");
	book.row("Rent", "CHK", "2027-03-02", -90_000)
		.in_category("Rent");
	book.row("Out", "CHK", "2027-03-03", -40_000)
		.in_category("Dining");
	book.group("Eating out", false);
	book.budget
		.categories
		.get_mut(&cat("Dining"))
		.unwrap()
		.group = veritas_domain::CategoryGroupId::new("Eating out");
	let (graph, _) = graph(&book, month("2027-03"));
	let (sources, outflows) = sides(&graph);
	assert_eq!(
		sources,
		named(&[("Paycheck", 100_000), ("Shortfall", 30_000)])
	);
	assert_eq!(value(&graph, NodeKind::Income), 130_000);
	assert_eq!(
		outflows,
		named(&[("Housing", 90_000), ("Eating out", 40_000)])
	);
	assert_eq!(value(&graph, NodeKind::Unallocated), 0);
}

#[test]
fn st8_x4_refund_bigger_than_spending_becomes_a_source() {
	let mut book = book();
	book.row("Pay", "CHK", "2027-03-01", 100_000)
		.in_category("Paycheck");
	book.row("Food", "CHK", "2027-03-02", -20_000)
		.in_category("Groceries");
	book.row("Shirt", "VISA", "2027-03-03", -2_000)
		.in_category("Clothing");
	book.row("Return", "VISA", "2027-03-20", 10_000)
		.in_category("Clothing");
	let (graph, _) = graph(&book, month("2027-03"));
	let (sources, outflows) = sides(&graph);
	assert_eq!(
		sources,
		named(&[("Paycheck", 100_000), ("RefundsAndWithdrawals", 8_000)])
	);
	assert_eq!(value(&graph, NodeKind::Income), 108_000);
	assert_eq!(
		outflows,
		named(&[("Food", 20_000), ("Unallocated", 88_000)])
	);
	let refunds = graph.node(&NodeKind::RefundsAndWithdrawals).unwrap();
	assert_eq!(refunds.rows, ids(&["Shirt", "Return"]));
}

#[test]
fn st8_x5_unreviewed_transfer_out() {
	let mut book = book();
	book.row("Pay", "CHK", "2027-03-01", 100_000)
		.in_category("Paycheck");
	book.row("Food", "CHK", "2027-03-02", -20_000)
		.in_category("Groceries");
	book.transfer("Move", "CHK", "BRK", "2027-03-03", -10_000);
	let (graph, _) = graph(&book, month("2027-03"));
	let (_, outflows) = sides(&graph);
	assert_eq!(
		outflows,
		named(&[
			("Food", 20_000),
			("UnreviewedTransfers", 10_000),
			("Unallocated", 70_000),
		])
	);
}

#[test]
fn st8_x6_empty_range_has_no_nodes() {
	let (graph, _) = graph(&master(), month("2031-01"));
	assert!(graph.nodes.is_empty());
	assert!(graph.flows.is_empty());
}

#[test]
fn st8_x7_split_children_land_in_their_categories() {
	let mut book = book();
	book.parent("S", "CHK", "2027-03-04", -10_000);
	book.row("S1", "CHK", "2027-03-04", -7_000)
		.in_category("Groceries")
		.child_of("S");
	book.row("S2", "CHK", "2027-03-04", -3_000)
		.in_category("Dining")
		.child_of("S");
	let (graph, _) = graph(&book, month("2027-03"));
	assert_eq!(
		value(&graph, NodeKind::Category(Some(cat("Groceries")))),
		7_000
	);
	assert_eq!(
		value(&graph, NodeKind::Category(Some(cat("Dining")))),
		3_000
	);
	assert_eq!(value(&graph, NodeKind::Shortfall), 10_000);
}

#[test]
fn st8_x8_income_reversal_is_an_outflow() {
	let mut book = book();
	book.row("Pay", "CHK", "2027-03-01", 100_000)
		.in_category("Paycheck");
	book.row("Clawback", "CHK", "2027-03-05", -500)
		.in_category("Interest");
	let (graph, report) = graph(&book, month("2027-03"));
	assert_eq!(report.income.total.minor(), 99_500);
	let (sources, outflows) = sides(&graph);
	assert_eq!(sources, named(&[("Paycheck", 100_000)]));
	assert_eq!(
		outflows,
		named(&[("IncomeReversals", 500), ("Unallocated", 99_500)])
	);
}

#[test]
fn st8_x9_unreviewed_transfer_in_is_a_source() {
	let mut book = book();
	book.row("Pay", "CHK", "2027-03-01", 100_000)
		.in_category("Paycheck");
	book.row("Food", "CHK", "2027-03-02", -20_000)
		.in_category("Groceries");
	book.transfer("In", "CHK", "BRK", "2027-03-03", 10_000);
	let (graph, _) = graph(&book, month("2027-03"));
	let (sources, outflows) = sides(&graph);
	assert_eq!(
		sources,
		named(&[("Paycheck", 100_000), ("UnreviewedTransfersIn", 10_000)])
	);
	assert_eq!(value(&graph, NodeKind::Income), 110_000);
	assert_eq!(
		outflows,
		named(&[("Food", 20_000), ("Unallocated", 90_000)])
	);
}

#[test]
fn st8_x10_gross_paycheck_with_splits() {
	let mut book = master_book();
	book.account("401k", true).account("HSA", true);
	book.group("Taxes and deductions", false);
	for category in ["Federal tax", "State tax", "FICA", "Health premium"] {
		book.category(category, "Taxes and deductions", None);
	}
	book.category("Retirement", "Saving", Some(CategoryKind::Savings));
	book.category("HSA contributions", "Saving", Some(CategoryKind::Savings));
	book.parent("Pay", "CHK", "2027-01-15", 234_400);
	for (id, amount, category) in [
		("Gross", 400_000, "Paycheck"),
		("Fed", -50_000, "Federal tax"),
		("State", -20_000, "State tax"),
		("FICA", -30_600, "FICA"),
		("Premium", -15_000, "Health premium"),
	] {
		book.row(id, "CHK", "2027-01-15", amount)
			.in_category(category)
			.child_of("Pay");
	}
	book.transfer("Ret", "CHK", "401k", "2027-01-15", -40_000)
		.in_category("Retirement")
		.child_of("Pay");
	book.transfer("Hsa", "CHK", "HSA", "2027-01-15", -10_000)
		.in_category("HSA contributions")
		.child_of("Pay");
	let (graph, _) = graph(&book, month("2027-01"));
	let (sources, outflows) = sides(&graph);
	assert_eq!(sources, named(&[("Paycheck", 400_000)]));
	assert_eq!(
		outflows,
		named(&[
			("Taxes and deductions", 115_600),
			("Savings", 50_000),
			("Unallocated", 234_400),
		])
	);
}

#[test]
fn st8_rule_5_and_6_hidden_categories_are_labeled_and_nodes_drill_to_rows() {
	let mut book = book();
	book.budget
		.categories
		.get_mut(&cat("Dining"))
		.unwrap()
		.hidden = true;
	book.row("Pay", "CHK", "2027-03-01", 100_000)
		.in_category("Paycheck");
	book.row("Out", "CHK", "2027-03-03", -4_000)
		.in_category("Dining");
	book.row("Food", "CHK", "2027-03-02", -2_000)
		.in_category("Groceries");
	let (graph, _) = graph(&book, month("2027-03"));
	let dining = graph
		.node(&NodeKind::Category(Some(cat("Dining"))))
		.unwrap();
	assert!(dining.labels.hidden);
	assert_eq!(dining.rows, ids(&["Out"]));
	let food = graph
		.node(&NodeKind::CategoryGroup(Some(
			veritas_domain::CategoryGroupId::new("Food"),
		)))
		.unwrap();
	assert_eq!(food.rows, ids(&["Food", "Out"]));
	let unallocated = graph.node(&NodeKind::Unallocated).unwrap();
	assert_eq!(unallocated.rows, ids(&["Pay"]));
}

#[test]
fn st8_rule_1_categories_net_across_the_whole_range() {
	let mut book = book();
	book.row("Pay", "CHK", "2027-03-01", 100_000)
		.in_category("Paycheck");
	book.row("Buy", "VISA", "2027-03-28", -10_000)
		.in_category("Clothing");
	book.row("Return", "VISA", "2027-04-05", 10_000)
		.in_category("Clothing");
	let range = Period::new(date("2027-03-01"), date("2027-04-30")).unwrap();
	let (graph, _) = graph(&book, range);
	assert_eq!(
		value(
			&graph,
			NodeKind::CategoryGroup(Some(veritas_domain::CategoryGroupId::new("Shopping")))
		),
		0
	);
	assert_eq!(value(&graph, NodeKind::RefundsAndWithdrawals), 0);
	assert_eq!(value(&graph, NodeKind::Unallocated), 100_000);
}

#[test]
fn st8_uncategorized_spending_is_its_own_outflow() {
	let mut book = book();
	book.row("Pay", "CHK", "2027-03-01", 100_000)
		.in_category("Paycheck");
	book.row("Mystery", "CHK", "2027-03-02", -3_000);
	let (graph, _) = graph(&book, month("2027-03"));
	assert_eq!(value(&graph, NodeKind::CategoryGroup(None)), 3_000);
	assert_eq!(value(&graph, NodeKind::Category(None)), 3_000);
}
