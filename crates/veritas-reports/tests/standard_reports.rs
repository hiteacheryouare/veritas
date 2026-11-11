//! R-REP-5: the four standard reports (Net worth, Cash flow, Spending by
//! category, Income vs expense) over a household's books from August 2026 to
//! December 2027. Each report is checked against a spreadsheet kept while the
//! rows were entered, against balances summed straight from the transactions,
//! and against the other reports and statements. The Sankey and the tax
//! summary are checked for money conservation on the same books.

#![allow(clippy::unwrap_used, clippy::panic)]

mod support;

use std::collections::BTreeMap;

use support::household::*;
use support::*;
use veritas_domain::{
	AccountId, BudgetData, CategoryGroupId, CategoryMeta, Money, PayeeId, TransactionId, YearMonth,
};
use veritas_reports::{
	Bucket, Classification, NetProfit, NodeKind, Period, Sankey, Section, account_balances,
	balance_sheet, cash_flow, cash_flow_by_month, detail_csv, months, net_profit,
	net_profit_by_month, net_worth_by_month, owners_equity, sankey, summary_csv, tax_summary,
};

const UNCATEGORIZED: &str = "(uncategorized)";
const MONTHS_2027: [&str; 11] = [
	"2027-02", "2027-03", "2027-04", "2027-05", "2027-06", "2027-07", "2027-08", "2027-09",
	"2027-10", "2027-11", "2027-12",
];

/// What each month should report, written down as the rows were entered.
#[derive(Default)]
struct Spreadsheet {
	spending: BTreeMap<String, BTreeMap<String, i64>>,
	income: BTreeMap<String, BTreeMap<String, i64>>,
	savings: BTreeMap<String, i64>,
	debt: BTreeMap<String, i64>,
	unreviewed: BTreeMap<String, i64>,
}

fn month_of(on: &str) -> String {
	on[..7].to_owned()
}

struct Household {
	book: Book,
	sheet: Spreadsheet,
	card_owed: i64,
}

impl Household {
	fn spent(&mut self, on: &str, category: Option<&str>, amount: i64) {
		*self
			.sheet
			.spending
			.entry(month_of(on))
			.or_default()
			.entry(category.unwrap_or(UNCATEGORIZED).to_owned())
			.or_default() += amount;
	}

	fn earned(&mut self, on: &str, category: &str, amount: i64) {
		*self
			.sheet
			.income
			.entry(month_of(on))
			.or_default()
			.entry(category.to_owned())
			.or_default() += amount;
	}

	fn saved(&mut self, on: &str, amount: i64) {
		*self.sheet.savings.entry(month_of(on)).or_default() += amount;
	}

	fn swipe(&mut self, id: &str, on: &str, category: &str, amount: i64) {
		spend(&mut self.book, id, "VISA", on, category, -amount);
		self.spent(on, Some(category), amount);
		self.card_owed += amount;
	}

	fn debit(&mut self, id: &str, on: &str, category: &str, amount: i64) {
		spend(&mut self.book, id, "CHK", on, category, -amount);
		self.spent(on, Some(category), amount);
	}

	fn paycheck(&mut self, id: &str, on: &str) {
		paycheck(&mut self.book, id, on);
		self.earned(on, "Paycheck", GROSS);
		self.spent(on, Some("Federal tax"), FEDERAL);
		self.spent(on, Some("State tax"), STATE);
		self.spent(on, Some("Social Security and Medicare"), FICA);
		self.saved(on, RETIREMENT);
	}

	fn loan(&mut self, id: &str, on: &str, principal: i64, interest: i64) {
		loan_payment(&mut self.book, id, on, principal, interest);
		*self.sheet.debt.entry(month_of(on)).or_default() += principal;
		self.spent(on, Some("Student loan interest"), interest);
	}

	fn save(&mut self, id: &str, on: &str, amount: i64) {
		self.book
			.transfer(id, "CHK", "HYSA", on, -amount)
			.in_category("Savings");
		self.saved(on, amount);
	}

	fn pay_card(&mut self, id: &str, on: &str, credited_on: &str) {
		let owed = self.card_owed;
		self.book.transfer(id, "CHK", "VISA", on, -owed);
		self.book.get(&format!("{id}m")).date = date(credited_on);
		self.card_owed = 0;
	}

	fn interest(&mut self, id: &str, on: &str, amount: i64) {
		self.book
			.row(id, "CHK", on, amount)
			.in_category("Interest")
			.payee = Some(PayeeId::new("Bank"));
		self.earned(on, "Interest", amount);
	}
}

/// The household from support::household, carried on monthly through 2027,
/// ending with Phone deleted outright and the Car group deleted with its
/// categories.
fn household_2027() -> Household {
	let mut book = through_year_end();
	book.account("CARASSET", true)
		.group("Health", false)
		.group("Giving", false)
		.group("Big purchases", false)
		.category("Medical", "Health", None)
		.category("Donations", "Giving", None)
		.category("Mobile", "Bills", None)
		.category("Car purchase", "Big purchases", None)
		.category("Tires", "Car", None);
	let mut house = Household {
		book,
		sheet: Spreadsheet::default(),
		card_owed: 0,
	};
	let revaluations = [
		15_000, -22_000, 31_000, 4_500, -61_000, 18_250, 9_900, -3_100, 27_000, 12_000, 6_600,
	];
	for (index, (name, revaluation)) in MONTHS_2027.iter().zip(revaluations).enumerate() {
		let step = index as i64;
		let on = |day: u32| format!("{name}-{day:02}");
		let id = |what: &str| format!("{name}-{what}");
		house.debit(&id("rent"), &on(1), "Rent", 165_000);
		house.loan(&id("loan"), &on(5), 20_067 + 95 * step, 10_933 - 95 * step);
		house.swipe(&id("groceries-1"), &on(6), "Groceries", 12_000 + 311 * step);
		house.debit(&id("gym"), &on(8), "Gym", 4_000);
		house.swipe(&id("streaming"), &on(10), "Streaming", 1_599);
		house.paycheck(&id("pay-1"), &on(15));
		house.save(&id("save"), &on(16), 50_000);
		house.swipe(&id("dining"), &on(18), "Dining", 4_000 + 225 * step);
		house.swipe(
			&id("groceries-2"),
			&on(21),
			"Groceries",
			11_500 + 173 * step,
		);
		let phone = if *name <= "2027-06" {
			"Phone"
		} else {
			"Mobile"
		};
		house.swipe(&id("phone"), &on(25), phone, 6_500);
		if *name == "2027-06" {
			house.pay_card(&id("card"), &on(30), "2027-07-01");
		} else {
			house.pay_card(&id("card"), &on(27), &on(27));
		}
		house.paycheck(&id("pay-2"), &on(28));
		house.interest(&id("chk-interest"), &on(28), 110 + 3 * step);
		house
			.book
			.row(&id("hysa-interest"), "HYSA", &on(28), 4_400 + 20 * step)
			.payee = Some(PayeeId::new("Bank"));
		house
			.book
			.row(&id("brk-value"), "BRK", &on(28), revaluation);
	}

	house
		.book
		.parent("feb-dentist", "VISA", "2027-02-11", -18_000);
	house
		.book
		.row("feb-dentist-exam", "VISA", "2027-02-11", -12_000)
		.in_category("Medical")
		.child_of("feb-dentist");
	house
		.book
		.row("feb-dentist-toothpaste", "VISA", "2027-02-11", -6_000)
		.in_category("Groceries")
		.child_of("feb-dentist");
	house.spent("2027-02-11", Some("Medical"), 12_000);
	house.spent("2027-02-11", Some("Groceries"), 6_000);
	house.card_owed += 18_000;

	house.debit("mar-insurance", "2027-03-09", "Medical", -4_000);
	house.swipe("mar-tires", "2027-03-14", "Tires", 48_000);

	house
		.book
		.row("apr-unknown-charge", "VISA", "2027-04-12", -2_599);
	house.spent("2027-04-12", None, 2_599);
	house.card_owed += 2_599;

	house
		.book
		.transfer("may-to-brokerage", "CHK", "BRK", "2027-05-12", -100_000);
	*house
		.sheet
		.unreviewed
		.entry("2027-05".to_owned())
		.or_default() += 100_000;

	house.swipe("jun-pharmacy", "2027-06-13", "Groceries", 2_350);
	house.book.get("jun-pharmacy").notes = Some("pharmacy #medical".to_owned());

	house
		.book
		.transfer("jul-withdrawal", "CHK", "HYSA", "2027-07-03", 150_000)
		.in_category("Savings");
	house.saved("2027-07-03", -150_000);

	house
		.book
		.transfer("aug-close-cu", "CU", "CHK", "2027-08-02", -24_925);
	house
		.book
		.budget
		.accounts
		.get_mut(&AccountId::new("CU"))
		.unwrap()
		.closed = true;

	house.interest("sep-interest-clawback", "2027-09-29", -300);

	house
		.book
		.transfer("oct-car", "CHK", "CARASSET", "2027-10-04", -900_000)
		.in_category("Car purchase");
	house.spent("2027-10-04", Some("Car purchase"), 900_000);

	house.debit("nov-food-bank", "2027-11-20", "Donations", 10_000);
	house.book.get("nov-food-bank").notes = Some("Food bank, \"holiday drive\"".to_owned());
	house.swipe("dec-charity", "2027-12-10", "Donations", 25_000);
	house.book.get("dec-charity").notes = Some("St. Mark's, year-end gift".to_owned());
	house
		.book
		.row("dec-car-value", "CARASSET", "2027-12-31", -45_000);
	house.save("dec-year-end-save-2027", "2027-12-31", 50_000);
	house.book.get("dec-year-end-save-2027m").date = date("2028-01-03");

	let budget = &mut house.book.budget;
	budget.categories.get_mut(&cat("Phone")).unwrap().tombstone = true;
	budget
		.category_groups
		.get_mut(&CategoryGroupId::new("Car"))
		.unwrap()
		.tombstone = true;
	for category in ["Car repair", "Tires"] {
		budget.categories.get_mut(&cat(category)).unwrap().tombstone = true;
	}
	house
}

fn ym(text: &str) -> YearMonth {
	YearMonth::parse(text).unwrap()
}

fn year(year: i32) -> Period {
	period(&format!("{year}-01-01"), &format!("{year}-12-31"))
}

fn by_category(section: &Section) -> BTreeMap<String, i64> {
	section
		.categories
		.iter()
		.map(|line| {
			(
				line.category
					.as_ref()
					.map_or(UNCATEGORIZED.to_owned(), ToString::to_string),
				line.total.minor(),
			)
		})
		.collect()
}

fn signed_sum(classification: &Classification, rows: &[TransactionId]) -> i64 {
	rows.iter()
		.map(|id| classification.get(id).unwrap().amount.minor())
		.sum()
}

fn off_budget_balance(budget: &BudgetData, classification: &Classification, on: &str) -> i64 {
	account_balances(budget, classification, date(on))
		.unwrap()
		.iter()
		.filter(|line| !line.on_budget)
		.map(|line| line.balance.minor())
		.sum()
}

#[test]
fn income_vs_expense_and_spending_by_category_match_the_spreadsheet_every_month() {
	let house = household_2027();
	let classification = house.book.classify();
	let reports = net_profit_by_month(
		&house.book.budget,
		&classification,
		ym("2027-02"),
		ym("2027-12"),
	)
	.unwrap();
	assert_eq!(reports.len(), MONTHS_2027.len());
	for (name, report) in MONTHS_2027.iter().zip(&reports) {
		let name = name.to_string();
		let spending = house.sheet.spending[&name].clone();
		let income = house.sheet.income[&name].clone();
		assert_eq!(by_category(&report.expenses), spending, "{name}: spending");
		assert_eq!(by_category(&report.income), income, "{name}: income");
		assert_eq!(
			report.expenses.total.minor(),
			spending.values().sum::<i64>(),
			"{name}: spending by category adds up to the expense total"
		);
		assert_eq!(
			report.income.total.minor(),
			income.values().sum::<i64>(),
			"{name}"
		);
		assert_eq!(
			report.savings.total.minor(),
			house.sheet.savings[&name],
			"{name}: savings"
		);
		assert_eq!(
			report.debt_paydown.total.minor(),
			house.sheet.debt[&name],
			"{name}: debt paydown"
		);
		assert_eq!(
			report.unreviewed.total.minor(),
			house.sheet.unreviewed.get(&name).copied().unwrap_or(0),
			"{name}: unreviewed"
		);
	}

	let july = &reports[5];
	assert_eq!(
		july.savings.total.minor(),
		50_000 + 2 * RETIREMENT - 150_000,
		"a withdrawal bigger than the month's saving leaves savings negative"
	);
	let march = by_category(&reports[1].expenses);
	assert_eq!(
		march["Medical"], -4_000,
		"a reimbursement is a negative expense"
	);
	assert!(!by_category(&reports[1].income).contains_key("Medical"));
	assert_eq!(
		by_category(&reports[7].income)["Interest"],
		110 + 3 * 7 - 300,
		"the clawback nets inside Interest"
	);
}

#[test]
fn spending_by_category_over_the_year_is_the_sum_of_its_months_and_keeps_retired_categories() {
	let house = household_2027();
	let budget = &house.book.budget;
	let classification = house.book.classify();
	let monthly =
		net_profit_by_month(budget, &classification, ym("2027-01"), ym("2027-12")).unwrap();
	let whole = net_profit(budget, &classification, year(2027)).unwrap();

	let mut summed: BTreeMap<String, i64> = BTreeMap::new();
	for report in &monthly {
		for (category, total) in by_category(&report.expenses) {
			*summed.entry(category).or_default() += total;
		}
	}
	assert_eq!(by_category(&whole.expenses), summed);
	let total = |pick: fn(&NetProfit) -> Money| -> i64 {
		monthly.iter().map(|report| pick(report).minor()).sum()
	};
	assert_eq!(total(|r| r.income.total), whole.income.total.minor());
	assert_eq!(total(|r| r.expenses.total), whole.expenses.total.minor());
	assert_eq!(
		total(|r| r.net_profit.total),
		whole.net_profit.total.minor()
	);
	assert_eq!(
		total(|r| r.net_cash_flow.total),
		whole.net_cash_flow.total.minor()
	);

	let line = |name: Option<&str>| {
		whole
			.expenses
			.categories
			.iter()
			.find(|line| line.category.as_ref().map(ToString::to_string).as_deref() == name)
			.unwrap_or_else(|| panic!("{name:?} is missing from the year"))
	};
	assert_eq!(line(Some("Phone")).total.minor(), 5 * 6_500);
	assert!(line(Some("Phone")).labels.deleted);
	assert_eq!(line(Some("Tires")).total.minor(), 48_000);
	assert!(line(Some("Tires")).labels.deleted);
	assert!(line(Some("Gym")).labels.hidden);
	assert!(line(Some("Streaming")).labels.archived);
	assert_eq!(line(None).total.minor(), 2_599);
	assert!(line(None).labels.needs_review);
	assert_eq!(
		whole.expenses.categories.last().unwrap().category,
		None,
		"uncategorized is listed last"
	);
}

#[test]
fn the_four_reports_agree_with_each_other_and_with_the_statements_every_month() {
	let house = household_2027();
	let budget = &house.book.budget;
	let classification = house.book.classify();
	let raw = RawLedger::new(budget);
	let (first, last) = (ym("2026-08"), ym("2027-12"));
	let worth = net_worth_by_month(budget, &classification, first, last).unwrap();
	let flows = cash_flow_by_month(budget, &classification, first, last).unwrap();
	let profits = net_profit_by_month(budget, &classification, first, last).unwrap();
	let all = months(first, last).unwrap();
	assert_eq!(
		(worth.len(), flows.len(), profits.len(), all.len()),
		(17, 17, 17, 17)
	);

	let mut previous_worth = raw.net_worth_before(first.first_day());
	let mut previous_cash = raw.budget_cash_before(first.first_day());
	for (((month, point), flow), profit) in all.iter().zip(&worth).zip(&flows).zip(&profits) {
		let period = Period::month(*month);
		let end = period.last();
		let name = month.to_string();

		assert_eq!(point.month, *month);
		assert_eq!(
			point.net_worth.minor(),
			raw.net_worth(end),
			"{name}: net worth"
		);
		assert_eq!(
			point.assets.minor() - point.liabilities.minor(),
			point.net_worth.minor()
		);
		let sheet = balance_sheet(budget, &classification, end).unwrap();
		assert_eq!(
			(sheet.total_assets, sheet.total_liabilities, sheet.net_worth),
			(point.assets, point.liabilities, point.net_worth),
			"{name}: Net worth report vs Balance Sheet"
		);

		assert_eq!(flow, &cash_flow(budget, &classification, period).unwrap());
		assert_eq!(flow.opening.minor(), previous_cash, "{name}: cash opening");
		assert_eq!(
			flow.closing.minor(),
			raw.budget_cash(end),
			"{name}: cash closing"
		);
		assert_eq!(flow.unexplained().unwrap(), Money::ZERO, "{name}");

		assert_eq!(
			flow.net_profit.total.minor(),
			profit.income.total.minor() - profit.expenses.total.minor(),
			"{name}: Cash flow's Net Profit is Income vs expense"
		);
		assert_eq!(
			profit.expenses.total.minor(),
			profit
				.expenses
				.categories
				.iter()
				.map(|line| line.total.minor())
				.sum::<i64>(),
			"{name}: Spending by category sums to the expenses"
		);
		assert_eq!(
			flow.net_cash_flow.total.minor(),
			profit.net_cash_flow.total.minor()
		);

		let equity = owners_equity(budget, &classification, period).unwrap();
		assert_eq!(
			equity.opening.minor(),
			previous_worth,
			"{name}: equity opening"
		);
		assert_eq!(equity.closing, point.net_worth, "{name}: equity closing");
		assert_eq!(equity.net_income, flow.net_profit, "{name}: one net income");
		let non_cash: i64 = equity
			.lines()
			.iter()
			.skip(1)
			.map(|line| line.total.minor())
			.sum();
		let change = point.net_worth.minor() - previous_worth;
		assert_eq!(
			change,
			flow.net_profit.total.minor() + non_cash,
			"{name}: net worth moved by net profit plus the non-cash lines"
		);
		let off_budget_change = off_budget_balance(budget, &classification, &end.to_string())
			- (previous_worth - previous_cash);
		assert_eq!(
			change,
			(flow.closing.minor() - flow.opening.minor()) + off_budget_change,
			"{name}: net worth moved by budget cash plus off-budget accounts"
		);

		previous_worth = point.net_worth.minor();
		previous_cash = flow.closing.minor();
	}

	let whole = cash_flow(budget, &classification, year(2027)).unwrap();
	let in_2027 = &flows[5..];
	assert_eq!(whole.opening, in_2027[0].opening);
	assert_eq!(whole.closing, in_2027.last().unwrap().closing);
	let sum = |pick: fn(&veritas_reports::CashFlow) -> Money| -> i64 {
		in_2027.iter().map(|flow| pick(flow).minor()).sum()
	};
	assert_eq!(
		sum(|f| f.net_cash_flow.total),
		whole.net_cash_flow.total.minor()
	);
	assert_eq!(
		sum(|f| f.unreviewed_transfers.total),
		whole.unreviewed_transfers.total.minor()
	);
	assert_eq!(
		(sum(|f| f.in_transit.total), whole.in_transit.total.minor()),
		(0, 0),
		"June's card payment is in transit for a month, and cancels inside the year"
	);
}

#[test]
fn month_end_stragglers_show_up_in_the_cash_flow_and_equity_of_the_right_months() {
	let house = household_2027();
	let budget = &house.book.budget;
	let classification = house.book.classify();
	let flows = cash_flow_by_month(budget, &classification, ym("2027-06"), ym("2027-07")).unwrap();
	let amount = |id: &str| budget.transactions[&TransactionId::new(id)].amount.minor();
	let owed: i64 = ["groceries-1", "streaming", "dining", "groceries-2", "phone"]
		.iter()
		.map(|what| -amount(&format!("2027-06-{what}")))
		.sum();
	assert_eq!(amount("2027-06-card"), -owed);
	assert_eq!(
		flows[0].in_transit.total.minor(),
		-owed,
		"June pays the card, VISA hears in July"
	);
	assert_eq!(flows[1].in_transit.total.minor(), owed);

	let december = owners_equity(budget, &classification, month("2027-12")).unwrap();
	assert_eq!(december.in_transit.total.minor(), -50_000);
	assert_eq!(
		december.revaluations.total.minor(),
		6_600 + 4_400 + 200 - 45_000
	);
	let october = owners_equity(budget, &classification, month("2027-10")).unwrap();
	assert_eq!(
		october.capitalized_transfers.total.minor(),
		900_000,
		"the car is spending on the budget but still owned"
	);
}

fn assert_sankey_conserves_money(
	graph: &Sankey,
	report: &NetProfit,
	classification: &Classification,
	label: &str,
) {
	let value = |kind: NodeKind| graph.node(&kind).map_or(0, |node| node.value.minor());
	let income = value(NodeKind::Income);
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
	assert_eq!((into, out), (income, income), "{label}: Income in = out");

	let leaves: i64 = graph
		.nodes
		.iter()
		.enumerate()
		.filter(|(index, node)| {
			!matches!(node.kind, NodeKind::Income)
				&& !graph.flows.iter().any(|flow| flow.from == *index)
				&& graph.flows.iter().any(|flow| flow.to == *index)
		})
		.map(|(_, node)| node.value.minor())
		.sum();
	assert_eq!(leaves, income, "{label}: every cent reaches a leaf");

	let sources_shown: i64 = graph
		.nodes
		.iter()
		.filter(|node| matches!(node.kind, NodeKind::IncomeCategory(_)))
		.map(|node| node.value.minor())
		.sum();
	assert_eq!(
		sources_shown - value(NodeKind::IncomeReversals),
		report.income.total.minor(),
		"{label}: invariant 1"
	);
	let gap = report.net_cash_flow.total.minor() - report.unreviewed.total.minor();
	if value(NodeKind::Shortfall) == 0 {
		assert_eq!(value(NodeKind::Unallocated), gap, "{label}: invariant 3");
	} else {
		assert_eq!(value(NodeKind::Unallocated), 0, "{label}: invariant 3");
		assert_eq!(value(NodeKind::Shortfall), -gap, "{label}: invariant 3");
	}

	let positive: BTreeMap<String, i64> = by_category(&report.expenses)
		.into_iter()
		.filter(|(_, total)| *total > 0)
		.collect();
	let leaves_by_category: BTreeMap<String, i64> = graph
		.nodes
		.iter()
		.filter_map(|node| match &node.kind {
			NodeKind::Category(category) => Some((
				category
					.as_ref()
					.map_or(UNCATEGORIZED.to_owned(), ToString::to_string),
				node.value.minor(),
			)),
			_ => None,
		})
		.collect();
	assert_eq!(
		leaves_by_category, positive,
		"{label}: Sankey categories are Spending by category's spending lines"
	);

	for node in &graph.nodes {
		let rows = signed_sum(classification, &node.rows);
		let expected = match node.kind {
			NodeKind::IncomeCategory(_)
			| NodeKind::RefundsAndWithdrawals
			| NodeKind::UnreviewedTransfersIn => node.value.minor(),
			NodeKind::CategoryGroup(_)
			| NodeKind::Category(_)
			| NodeKind::Savings
			| NodeKind::DebtPaydown
			| NodeKind::IncomeReversals
			| NodeKind::UnreviewedTransfers => -node.value.minor(),
			NodeKind::Income => node.value.minor() - value(NodeKind::Shortfall),
			NodeKind::Shortfall | NodeKind::Unallocated => continue,
		};
		assert_eq!(
			rows, expected,
			"{label}: {:?} drills to rows that add up to it",
			node.kind
		);
	}
}

#[test]
fn the_sankey_conserves_money_in_every_month_the_year_and_the_whole_history() {
	let house = household_2027();
	let budget = &house.book.budget;
	let classification = house.book.classify();
	let mut periods: Vec<(String, Period)> = months(ym("2026-08"), ym("2027-12"))
		.unwrap()
		.into_iter()
		.map(|month| (month.to_string(), Period::month(month)))
		.collect();
	periods.push(("2027".to_owned(), year(2027)));
	periods.push(("history".to_owned(), period("2026-08-01", "2027-12-31")));
	for (label, period) in &periods {
		let graph = sankey(budget, &classification, *period).unwrap();
		let report = net_profit(budget, &classification, *period).unwrap();
		assert_sankey_conserves_money(&graph, &report, &classification, label);
	}

	let october = sankey(budget, &classification, month("2027-10")).unwrap();
	assert!(
		october.node(&NodeKind::Shortfall).is_some(),
		"the car costs more than October earned"
	);
	let july = sankey(budget, &classification, month("2027-07")).unwrap();
	assert_eq!(
		july.node(&NodeKind::RefundsAndWithdrawals)
			.unwrap()
			.value
			.minor(),
		150_000 - 50_000 - 2 * RETIREMENT
	);
	let september = sankey(budget, &classification, month("2027-09")).unwrap();
	assert_eq!(
		september
			.node(&NodeKind::IncomeReversals)
			.unwrap()
			.value
			.minor(),
		300 - (110 + 3 * 7)
	);
	let whole = sankey(budget, &classification, year(2027)).unwrap();
	let car = whole
		.node(&NodeKind::CategoryGroup(Some(CategoryGroupId::new("Car"))))
		.unwrap();
	assert!(car.labels.deleted);
	assert_eq!(car.value.minor(), 48_000);
}

fn bucket(budget: &mut BudgetData, category: &str, bucket: &str) {
	budget
		.category_meta
		.entry(cat(category))
		.or_insert(CategoryMeta {
			kind: None,
			tax_bucket: None,
			archived_on: None,
		})
		.tax_bucket = Some(bucket.to_owned());
}

/// A minimal RFC 4180 reader for the files the tax export writes.
fn parse_csv(text: &str) -> Vec<Vec<String>> {
	let mut rows = Vec::new();
	let mut row = Vec::new();
	let mut cell = String::new();
	let mut quoted = false;
	let mut chars = text.chars().peekable();
	while let Some(c) = chars.next() {
		match (quoted, c) {
			(true, '"') if chars.peek() == Some(&'"') => {
				chars.next();
				cell.push('"');
			}
			(true, '"') => quoted = false,
			(true, c) => cell.push(c),
			(false, '"') => quoted = true,
			(false, ',') => row.push(std::mem::take(&mut cell)),
			(false, '\n') => {
				row.push(std::mem::take(&mut cell));
				rows.push(std::mem::take(&mut row));
			}
			(false, c) => cell.push(c),
		}
	}
	assert!(
		row.is_empty() && cell.is_empty(),
		"the file ends with a newline"
	);
	rows
}

#[test]
fn the_2027_tax_summary_matches_the_income_statement_and_its_exports() {
	let mut house = household_2027();
	let budget = &mut house.book.budget;
	for (category, id) in [
		("Paycheck", "w2_income"),
		("Interest", "interest_income"),
		("Federal tax", "taxes_withheld"),
		("State tax", "taxes_withheld"),
		("Social Security and Medicare", "taxes_withheld"),
		("Retirement", "retirement_contribution"),
		("Medical", "medical"),
		("Donations", "charitable"),
	] {
		bucket(budget, category, id);
	}
	budget
		.tax_tags
		.insert("medical".to_owned(), "medical".to_owned());
	let budget = &house.book.budget;
	let classification = house.book.classify();
	let summary = tax_summary(budget, &classification, 2027).unwrap();
	let report = net_profit(budget, &classification, year(2027)).unwrap();
	let income = by_category(&report.income);
	let spending = by_category(&report.expenses);
	let saving = by_category(&report.savings);
	let total = |bucket: Bucket| summary.bucket(bucket).map(|total| total.total.minor());

	assert_eq!(total(Bucket::W2Income), Some(24 * GROSS));
	assert_eq!(total(Bucket::W2Income), Some(income["Paycheck"]));
	assert_eq!(total(Bucket::InterestIncome), Some(income["Interest"]));
	assert_eq!(
		total(Bucket::TaxesWithheld),
		Some(
			spending["Federal tax"]
				+ spending["State tax"]
				+ spending["Social Security and Medicare"]
		)
	);
	assert_eq!(
		total(Bucket::TaxesWithheld),
		Some(24 * (FEDERAL + STATE + FICA))
	);
	assert_eq!(
		total(Bucket::RetirementContribution),
		Some(saving["Retirement"])
	);
	assert_eq!(total(Bucket::Medical), Some(12_000 - 4_000 + 2_350));
	assert_eq!(total(Bucket::Charitable), Some(spending["Donations"]));
	assert_eq!(summary.conflicts().count(), 0);

	let detail = parse_csv(&detail_csv(budget, &summary));
	let header = &detail[0];
	let column = |name: &str| header.iter().position(|cell| cell == name).unwrap();
	let mut from_detail: BTreeMap<String, (i64, usize)> = BTreeMap::new();
	for row in &detail[1..] {
		assert_eq!(row.len(), header.len(), "{row:?}");
		let entry = from_detail
			.entry(row[column("bucket")].clone())
			.or_default();
		entry.0 += row[column("counted_cents")].parse::<i64>().unwrap();
		entry.1 += 1;
	}
	let food_bank = detail
		.iter()
		.find(|row| row[column("transaction_id")] == "nov-food-bank")
		.unwrap();
	assert_eq!(food_bank[column("notes")], "Food bank, \"holiday drive\"");
	assert_eq!(food_bank[column("counted_cents")], "10000");
	let charity = detail
		.iter()
		.find(|row| row[column("transaction_id")] == "dec-charity")
		.unwrap();
	assert_eq!(charity[column("notes")], "St. Mark's, year-end gift");
	let pharmacy = detail
		.iter()
		.find(|row| row[column("transaction_id")] == "jun-pharmacy")
		.unwrap();
	assert_eq!(
		(
			pharmacy[column("source")].as_str(),
			pharmacy[column("category")].as_str()
		),
		("tag", "Groceries")
	);

	let totals = parse_csv(&summary_csv(&summary));
	let columns = &totals[0];
	let at = |name: &str| columns.iter().position(|cell| cell == name).unwrap();
	let from_summary: BTreeMap<String, (i64, usize)> = totals[1..]
		.iter()
		.map(|row| {
			let cents: i64 = row[at("total_cents")].parse().unwrap();
			assert_eq!(row[at("total")], Money::from_minor(cents).to_string());
			(
				row[at("bucket")].clone(),
				(cents, row[at("transaction_count")].parse().unwrap()),
			)
		})
		.collect();
	assert_eq!(
		from_summary, from_detail,
		"TAX-6 invariant 1, read back from the files"
	);
}

#[test]
fn with_every_category_bucketed_the_tax_summary_accounts_for_the_whole_net_cash_flow() {
	let mut house = household_2027();
	let budget = &mut house.book.budget;
	let categories: Vec<(String, bool)> = budget
		.categories
		.values()
		.map(|category| (category.id.to_string(), category.is_income))
		.collect();
	for (category, is_income) in categories {
		let id = if is_income {
			"business_income"
		} else {
			"business_expense"
		};
		bucket(budget, &category, id);
	}
	budget
		.tax_tags
		.insert("unknown".to_owned(), "business_expense".to_owned());
	house.book.get("apr-unknown-charge").notes = Some("#unknown".to_owned());
	let budget = &house.book.budget;
	let classification = house.book.classify();
	for tax_year in [2026, 2027] {
		let summary = tax_summary(budget, &classification, tax_year).unwrap();
		let report = net_profit(budget, &classification, year(tax_year)).unwrap();
		let total = |bucket: Bucket| {
			summary
				.bucket(bucket)
				.map_or(0, |total| total.total.minor())
		};
		assert_eq!(
			total(Bucket::BusinessIncome) - total(Bucket::BusinessExpense),
			report.net_cash_flow.total.minor(),
			"{tax_year}: every eligible cent lands in exactly one bucket"
		);
		assert_eq!(
			total(Bucket::BusinessIncome),
			report.income.total.minor(),
			"{tax_year}"
		);
	}
}
