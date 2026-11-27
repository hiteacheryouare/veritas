//! docs/finance/migration.md MIG-8's worked examples, on budgets built by
//! hand. Today is 2026-11-22 unless an example says otherwise.

use veritas_domain::{
	Account, AccountId, AccountKind, AccountMeta, BudgetCell, BudgetData, BudgetType, Category,
	CategoryGroup, CategoryGroupId, CategoryId, CategoryKind, CivilDate, CustomReportId,
	CustomReportRow, DashboardPage, DashboardPageId, Money, Payee, PayeeId, Row, Rule, RuleId,
	RuleStage, Schedule, ScheduleId, SplitRole, Transaction, TransactionId, Widget, WidgetId,
	YearMonth,
};
use veritas_engine::EnvelopeSheet;
use veritas_reports::classify;

use super::*;

fn day(text: &str) -> CivilDate {
	CivilDate::from(chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap())
}

fn month(text: &str) -> YearMonth {
	YearMonth::parse(text).unwrap()
}

fn key(name: &str) -> String {
	name.trim()
		.to_lowercase()
		.split_whitespace()
		.map(|word| {
			word.strip_suffix('s')
				.filter(|_| word.len() > 3)
				.unwrap_or(word)
				.to_owned()
		})
		.collect::<Vec<_>>()
		.join(" ")
}

struct Budget {
	data: BudgetData,
	next: u32,
}

impl Budget {
	fn new() -> Budget {
		let mut budget = Budget {
			data: BudgetData::empty(BudgetType::Envelope),
			next: 0,
		};
		budget.group("income", "Income", true, 99.0);
		budget.category("salary", "Salary", "income", 1.0);
		budget
	}

	fn id(&mut self) -> String {
		self.next += 1;
		format!("t{:04}", self.next)
	}

	fn account(&mut self, id: &str, name: &str, off_budget: bool) {
		let account = AccountId::new(id);
		self.data.accounts.insert(
			account.clone(),
			Account {
				id: account.clone(),
				name: name.to_owned(),
				off_budget,
				closed: false,
				group: None,
				sort_order: self.data.accounts.len() as f64,
				tombstone: false,
				last_reconciled: None,
			},
		);
		let payee = PayeeId::new(format!("to-{id}"));
		self.data.payees.insert(
			payee.clone(),
			Payee {
				id: payee.clone(),
				name: String::new(),
				transfer_account: Some(account),
				favorite: false,
				learn_categories: false,
				tombstone: false,
			},
		);
		self.data.payee_mapping.insert(payee.clone(), payee);
	}

	fn payee(&mut self, id: &str, name: &str) {
		let payee = PayeeId::new(id);
		self.data.payees.insert(
			payee.clone(),
			Payee {
				id: payee.clone(),
				name: name.to_owned(),
				transfer_account: None,
				favorite: false,
				learn_categories: false,
				tombstone: false,
			},
		);
		self.data.payee_mapping.insert(payee.clone(), payee);
	}

	fn group(&mut self, id: &str, name: &str, is_income: bool, sort_order: f64) {
		let group = CategoryGroupId::new(id);
		self.data.category_groups.insert(
			group.clone(),
			CategoryGroup {
				id: group,
				name: name.to_owned(),
				is_income,
				hidden: false,
				sort_order,
				tombstone: false,
			},
		);
	}

	fn category(&mut self, id: &str, name: &str, group: &str, sort_order: f64) {
		let category = CategoryId::new(id);
		let is_income = self
			.data
			.category_groups
			.get(&CategoryGroupId::new(group))
			.is_some_and(|group| group.is_income);
		self.data.categories.insert(
			category.clone(),
			Category {
				id: category.clone(),
				name: name.to_owned(),
				group: CategoryGroupId::new(group),
				is_income,
				hidden: false,
				sort_order,
				tombstone: false,
			},
		);
		self.data
			.category_mapping
			.insert(category.clone(), category);
	}

	fn row(
		&mut self,
		account: &str,
		date: &str,
		amount: i64,
		category: Option<&str>,
		payee: Option<&str>,
	) -> TransactionId {
		let id = TransactionId::new(self.id());
		self.data.transactions.insert(
			id.clone(),
			Transaction {
				id: id.clone(),
				account: AccountId::new(account),
				date: day(date),
				amount: Money::from_minor(amount),
				payee: payee.map(PayeeId::new),
				imported_payee: None,
				category: category.map(CategoryId::new),
				notes: None,
				cleared: true,
				reconciled: false,
				split: SplitRole::None,
				transfer: None,
				schedule: None,
				starting_balance: false,
				sort_order: 0.0,
				tombstone: false,
				split_error: None,
			},
		);
		id
	}

	fn opening(&mut self, account: &str, amount: i64) {
		let id = self.row(account, "2025-08-01", amount, None, None);
		if let Some(row) = self.data.transactions.get_mut(&id) {
			row.starting_balance = true;
		}
	}

	/// An on-budget payment into an off-budget account, both legs.
	fn transfer(&mut self, from: &str, to: &str, category: &str, date: &str, amount: i64) {
		let out = self.row(
			from,
			date,
			-amount,
			Some(category),
			Some(&format!("to-{to}")),
		);
		let back = self.row(to, date, amount, None, Some(&format!("to-{from}")));
		if let Some(row) = self.data.transactions.get_mut(&out) {
			row.transfer = Some(back.clone());
		}
		if let Some(row) = self.data.transactions.get_mut(&back) {
			row.transfer = Some(out);
		}
	}

	fn budget(&mut self, category: &str, at: &str, amount: i64) {
		let key = (month(at), CategoryId::new(category));
		self.data.budget_cells.insert(
			key,
			BudgetCell {
				month: month(at),
				category: CategoryId::new(category),
				budgeted: Money::from_minor(amount),
				carryover: false,
				goal: None,
				long_goal: false,
			},
		);
	}

	fn note(&mut self, category: &str, text: &str) {
		self.data
			.notes
			.insert(category.to_owned(), Some(text.to_owned()));
	}

	fn suggestions(&self, today: &str) -> Vec<Suggestion> {
		let sheet = EnvelopeSheet::compute(&self.data, day(today)).unwrap();
		let classification = classify(&self.data);
		let inputs = Inputs {
			data: &self.data,
			sheet: &sheet,
			classification: &classification,
			today: day(today),
			name_key: &key,
		};
		suggestions(&inputs).unwrap()
	}

	fn apply(&self, today: &str, selection: &Selection) -> Result<Vec<Row>, CoreError> {
		let sheet = EnvelopeSheet::compute(&self.data, day(today)).unwrap();
		let classification = classify(&self.data);
		let inputs = Inputs {
			data: &self.data,
			sheet: &sheet,
			classification: &classification,
			today: day(today),
			name_key: &key,
		};
		apply(&inputs, selection)
	}

	fn revert(
		&self,
		today: &str,
		key: &str,
		group: Option<&CategoryGroupId>,
		confirmed: bool,
	) -> RevertPlan {
		let sheet = EnvelopeSheet::compute(&self.data, day(today)).unwrap();
		let classification = classify(&self.data);
		let inputs = Inputs {
			data: &self.data,
			sheet: &sheet,
			classification: &classification,
			today: day(today),
			name_key: &key_fn,
		};
		revert(&inputs, key, group, confirmed).unwrap()
	}

	fn try_revert(&self, today: &str, key: &str) -> Result<RevertPlan, CoreError> {
		let sheet = EnvelopeSheet::compute(&self.data, day(today)).unwrap();
		let classification = classify(&self.data);
		let inputs = Inputs {
			data: &self.data,
			sheet: &sheet,
			classification: &classification,
			today: day(today),
			name_key: &key_fn,
		};
		revert(&inputs, key, None, false)
	}

	fn preview(&self, today: &str, selection: &Selection) -> Vec<Suggestion> {
		let sheet = EnvelopeSheet::compute(&self.data, day(today)).unwrap();
		let classification = classify(&self.data);
		let inputs = Inputs {
			data: &self.data,
			sheet: &sheet,
			classification: &classification,
			today: day(today),
			name_key: &key_fn,
		};
		preview(&inputs, selection).unwrap()
	}

	fn commit(&mut self, rows: &[Row]) {
		self.data = crate::writes::applied(&self.data, rows);
	}
}

fn key_fn(name: &str) -> String {
	key(name)
}

const TODAY: &str = "2026-11-22";

fn find<'a>(all: &'a [Suggestion], wanted: &str) -> Option<&'a Suggestion> {
	all.iter().find(|suggestion| suggestion.key == wanted)
}

fn tick(keys: &[&str]) -> Selection {
	Selection {
		items: keys
			.iter()
			.map(|key| Selected {
				key: (*key).to_owned(),
				edits: Edits::default(),
			})
			.collect(),
		consent_show_goals_group: false,
	}
}

// (a) MIG-3

fn net_worth_budget() -> Budget {
	let mut budget = Budget::new();
	budget.account("checking", "Checking", false);
	budget.account("savings", "Savings", false);
	budget.account("visa", "Visa", false);
	budget.account("loan", "Car Loan", true);
	budget.account("brokerage", "Brokerage", true);
	budget.opening("checking", 200_000);
	budget.opening("savings", 500_000);
	budget.opening("visa", -30_000);
	budget.opening("loan", -980_000);
	budget.opening("brokerage", 1_000_000);
	let page = DashboardPageId::new("page");
	budget.data.dashboard_pages.insert(
		page.clone(),
		DashboardPage {
			id: page.clone(),
			name: Some("Overview".to_owned()),
			tombstone: false,
			position: 1,
		},
	);
	let report = CustomReportId::new("report");
	budget
		.data
		.custom_reports
		.insert(report.clone(), net_worth_report(&report));
	let widget = WidgetId::new("widget");
	budget.data.widgets.insert(
		widget.clone(),
		Widget {
			id: widget,
			kind: Some("custom-report".to_owned()),
			width: Some(4),
			height: Some(2),
			x: Some(0),
			y: Some(0),
			meta: Some("{\"id\":\"report\"}".to_owned()),
			page: Some(page),
			tombstone: false,
		},
	);
	budget
}

fn net_worth_report(id: &CustomReportId) -> CustomReportRow {
	CustomReportRow {
		id: id.clone(),
		name: Some("Net worth".to_owned()),
		start_date: Some("2025-08-01".to_owned()),
		end_date: Some("2026-11-22".to_owned()),
		date_static: Some(0),
		date_range: Some("All time".to_owned()),
		mode: Some("total".to_owned()),
		group_by: Some("Account".to_owned()),
		balance_type: Some("Net".to_owned()),
		show_empty: Some(0),
		show_offbudget: Some(1),
		show_hidden: Some(0),
		show_uncategorized: Some(1),
		selected_categories: None,
		graph_type: Some("TableGraph".to_owned()),
		conditions: Some("[]".to_owned()),
		conditions_op: Some("and".to_owned()),
		metadata: None,
		interval: Some("Monthly".to_owned()),
		color_scheme: None,
		tombstone: false,
		include_current: Some(0),
		sort_by: Some("desc".to_owned()),
		trim_intervals: Some(0),
		show_trend_lines: Some(0),
	}
}

fn edit_report(budget: &mut Budget, edit: impl FnOnce(&mut CustomReportRow)) {
	if let Some(row) = budget
		.data
		.custom_reports
		.get_mut(&CustomReportId::new("report"))
	{
		edit(row);
	}
}

#[test]
fn example_1_an_equivalent_live_net_worth_report_is_pre_ticked() {
	let budget = net_worth_budget();
	let all = budget.suggestions(TODAY);
	let report = find(&all, "report:widget").expect("suggested");
	assert_eq!(report.confidence, Confidence::PreTicked);
	let equal = report
		.signals
		.iter()
		.find(|signal| signal.code == "equal")
		.unwrap();
	assert!(equal.figures.iter().all(|figure| figure.value == 690_000));
	let rows = budget.apply(TODAY, &tick(&["report:widget"])).unwrap();
	assert!(
		matches!(&rows[0], Row::WidgetOverride(row) if row.value.as_ref().is_some_and(|value| value.native_type == "net-worth-card"))
	);
	assert!(
		rows.iter()
			.all(|row| !matches!(row, Row::CustomReport(_) | Row::Widget(_))),
		"Actual's rows are untouched"
	);
}

#[test]
fn example_2_a_report_that_differs_by_a_cent_is_not_suggested() {
	let mut budget = net_worth_budget();
	edit_report(&mut budget, |row| row.show_offbudget = Some(0));
	assert!(find(&budget.suggestions(TODAY), "report:widget").is_none());
	let mut budget = net_worth_budget();
	edit_report(&mut budget, |row| row.show_uncategorized = Some(0));
	budget.row("checking", "2026-10-03", -5_000, None, None);
	assert!(find(&budget.suggestions(TODAY), "report:widget").is_none());
}

#[test]
fn example_3_a_static_range_is_shown_unticked() {
	let mut budget = net_worth_budget();
	edit_report(&mut budget, |row| {
		row.date_static = Some(1);
		row.start_date = Some("2025-08-01".to_owned());
		row.end_date = Some("2026-06-30".to_owned());
	});
	let all = budget.suggestions(TODAY);
	assert_eq!(
		find(&all, "report:widget").map(|s| s.confidence),
		Some(Confidence::Shown)
	);
}

#[test]
fn example_4_a_report_changed_in_actual_shows_again() {
	let mut budget = net_worth_budget();
	let rows = budget.apply(TODAY, &tick(&["report:widget"])).unwrap();
	budget.commit(&rows);
	let widget = WidgetId::new("widget");
	assert!(matches!(
		override_state(&budget.data, &widget),
		OverrideState::Active(_)
	));
	assert!(overridden_reports(&budget.data).contains(&CustomReportId::new("report")));
	edit_report(&mut budget, |row| {
		row.conditions =
			Some("[{\"field\":\"account\",\"op\":\"is\",\"value\":\"checking\"}]".to_owned());
	});
	assert_eq!(override_state(&budget.data, &widget), OverrideState::Stale);
	assert!(overridden_reports(&budget.data).is_empty());
	assert!(
		budget.data.widget_overrides.contains_key(&widget),
		"the stale row is kept"
	);
}

#[test]
fn renaming_or_moving_a_report_keeps_the_fingerprint() {
	let budget = net_worth_budget();
	let report = budget.data.custom_reports[&CustomReportId::new("report")].clone();
	let widget = budget.data.widgets[&WidgetId::new("widget")].clone();
	let before = fingerprint(&report, &widget);
	let renamed = CustomReportRow {
		name: Some("Everything".to_owned()),
		graph_type: Some("BarGraph".to_owned()),
		..report.clone()
	};
	let moved = Widget {
		x: Some(6),
		width: Some(8),
		meta: Some("{ \"id\" : \"report\" }".to_owned()),
		..widget.clone()
	};
	assert_eq!(fingerprint(&renamed, &moved), before);
	let narrowed = CustomReportRow {
		show_offbudget: Some(0),
		..report
	};
	assert_ne!(fingerprint(&narrowed, &widget), before);
}

#[test]
fn example_24_a_closed_account_the_report_leaves_out_fails_an_earlier_date() {
	let mut budget = net_worth_budget();
	budget.account("old", "Old savings", false);
	budget.row("old", "2026-09-01", 5_000, None, None);
	budget.row("old", "2026-10-31", -5_000, None, None);
	if let Some(account) = budget.data.accounts.get_mut(&AccountId::new("old")) {
		account.closed = true;
	}
	let all = budget.suggestions(TODAY);
	let report = find(&all, "report:widget");
	if let Some(report) = report {
		assert!(
			report.signals.iter().any(|signal| signal.code == "equal"),
			"suggested only when the report counts the closed account too"
		);
	}
}

// (b) MIG-4

fn goal_budget() -> Budget {
	let mut budget = Budget::new();
	budget.account("checking", "Checking", false);
	budget.opening("checking", 1_000_000);
	budget.group("everyday", "Everyday", false, 1.0);
	budget.category("groceries", "Groceries", "everyday", 1.0);
	budget.category("vacation", "Vacation", "everyday", 2.0);
	budget.category("gas", "Gas", "everyday", 3.0);
	budget.row("checking", "2026-11-03", 400_000, Some("salary"), None);
	budget
}

#[test]
fn example_5_a_template_with_a_date_is_pre_ticked() {
	let mut budget = goal_budget();
	budget.note("vacation", "#template 3000 by 2027-06");
	budget.budget("vacation", "2026-10", 120_000);
	let all = budget.suggestions(TODAY);
	let goal = find(&all, "goal:vacation").expect("suggested");
	assert_eq!(goal.confidence, Confidence::PreTicked);
	assert_eq!(goal.proposed.target, Some(Money::from_minor(300_000)));
	assert_eq!(goal.proposed.target_date, Some(day("2027-06-30")));
	assert!(!goal.target_in_actual);
}

#[test]
fn example_6_a_goal_line_is_pre_ticked_and_already_in_actual() {
	let mut budget = goal_budget();
	budget.note("vacation", "Trip money\n#goal 10000");
	let all = budget.suggestions(TODAY);
	let goal = find(&all, "goal:vacation").expect("suggested");
	assert_eq!(goal.confidence, Confidence::PreTicked);
	assert_eq!(goal.proposed.target, Some(Money::from_minor(1_000_000)));
	assert_eq!(goal.proposed.target_date, None);
	assert!(goal.target_in_actual);
}

#[test]
fn example_7_a_steady_accumulation_alone_is_shown_with_no_target() {
	let mut budget = goal_budget();
	for at in ["2026-05", "2026-06", "2026-08", "2026-09", "2026-10"] {
		budget.budget("vacation", at, 5_000);
	}
	let all = budget.suggestions(TODAY);
	let goal = find(&all, "goal:vacation").expect("suggested");
	assert_eq!(goal.confidence, Confidence::Shown);
	assert_eq!(goal.proposed.target, None);
	assert!(
		budget.apply(TODAY, &tick(&["goal:vacation"])).is_err(),
		"the target is required"
	);
}

#[test]
fn example_8_a_name_alone_or_regular_spending_is_never_shown() {
	let mut budget = goal_budget();
	budget.category("stuff", "Savings stuff", "everyday", 4.0);
	budget.category("plain", "Stuff", "everyday", 5.0);
	for at in [
		"2026-05", "2026-06", "2026-07", "2026-08", "2026-09", "2026-10",
	] {
		budget.budget("groceries", at, 30_000);
		budget.row(
			"checking",
			&format!("{at}-12"),
			-28_000,
			Some("groceries"),
			None,
		);
	}
	let all = budget.suggestions(TODAY);
	assert!(find(&all, "goal:groceries").is_none());
	assert!(find(&all, "goal:stuff").is_none());
	assert!(find(&all, "goal:plain").is_none());
}

#[test]
fn mig_1_7_two_identical_categories_with_different_names_get_the_same_suggestion() {
	let mut budget = goal_budget();
	budget.category("a", "Emergency fund", "everyday", 4.0);
	budget.category("b", "Kitchen", "everyday", 5.0);
	for category in ["a", "b"] {
		for at in ["2026-05", "2026-06", "2026-08", "2026-09", "2026-10"] {
			budget.budget(category, at, 5_000);
		}
	}
	let all = budget.suggestions(TODAY);
	let (a, b) = (find(&all, "goal:a").unwrap(), find(&all, "goal:b").unwrap());
	assert_eq!(a.confidence, b.confidence);
	assert_eq!(a.proposed, b.proposed);
	assert!(a.signals.iter().any(|signal| signal.code == "s3_name"));
	assert!(!b.signals.iter().any(|signal| signal.code == "s3_name"));
}

#[test]
fn example_9_a_yen_budget_reads_targets_on_the_budget_scale_and_skips_s2() {
	let mut budget = goal_budget();
	budget.data.format.currency_code = Some("JPY".to_owned());
	budget.category("trip", "Trip", "everyday", 4.0);
	budget.category("rice", "Rice fund", "everyday", 5.0);
	budget.note("trip", "#goal 100000");
	for at in ["2026-05", "2026-06", "2026-08", "2026-09", "2026-10"] {
		budget.budget("rice", at, 5_000);
	}
	let all = budget.suggestions(TODAY);
	assert_eq!(
		find(&all, "goal:trip").and_then(|s| s.proposed.target),
		Some(Money::from_minor(100_000))
	);
	assert!(find(&all, "goal:rice").is_none());
}

#[test]
fn example_22_conflicting_sources_propose_the_first_and_are_shown() {
	let mut budget = goal_budget();
	budget.note("vacation", "#goal 10000");
	budget.data.category_templates.insert(
		CategoryId::new("vacation"),
		veritas_domain::CategoryTemplates {
			goal_def: Some(
				"[{\"type\":\"goal\",\"amount\":20000,\"directive\":\"goal\"}]".to_owned(),
			),
			template_settings: Some("{\"source\":\"ui\"}".to_owned()),
		},
	);
	let all = budget.suggestions(TODAY);
	let goal = find(&all, "goal:vacation").unwrap();
	assert_eq!(goal.proposed.target, Some(Money::from_minor(2_000_000)));
	assert_eq!(goal.confidence, Confidence::Shown);
	assert_eq!(goal.conflicts.len(), 2);
	let mut long = goal_budget();
	long.budget("vacation", "2026-10", 0);
	if let Some(cell) = long
		.data
		.budget_cells
		.get_mut(&(month("2026-10"), CategoryId::new("vacation")))
	{
		cell.goal = Some(Money::from_minor(1_000_000));
		cell.long_goal = true;
	}
	let all = long.suggestions(TODAY);
	assert_eq!(
		find(&all, "goal:vacation").and_then(|s| s.proposed.target),
		Some(Money::from_minor(1_000_000)),
		"a long-term goal figure is never multiplied again"
	);
}

#[test]
fn template_lines_follow_actuals_shapes() {
	let line =
		|text: &str| goal::from_line(text, 2).map(|target| (target.amount.minor(), target.date));
	assert_eq!(
		line("#template 3000 by 2027-06"),
		Some((300_000, Some(day("2027-06-30"))))
	);
	assert_eq!(
		line("#template-2 3000.5 by 2027-06"),
		Some((300_050, Some(day("2027-06-30"))))
	);
	assert_eq!(
		line("#template 3000 by 2027-06 spend from 2027-01"),
		Some((300_000, Some(day("2027-06-30"))))
	);
	assert_eq!(line("#template 3000 by 2027-06 repeat every year"), None);
	assert_eq!(
		line("#template 50"),
		None,
		"a monthly template is a budgeting rule"
	);
	assert_eq!(line("#goal 0"), None);
	assert_eq!(
		goal::from_line("#goal 100000", 0).map(|t| t.amount.minor()),
		Some(100_000)
	);
}

#[test]
fn example_18_adopting_moves_to_the_top_of_goals_and_records_the_neighbour() {
	let mut budget = goal_budget();
	budget.group("goals", "Goals", false, 2.0);
	budget.category("laptop", "New laptop", "goals", 1.0);
	budget.category("gift", "Gift fund", "goals", 2.0);
	budget.note("vacation", "#goal 1000");
	let rows = budget.apply(TODAY, &tick(&["goal:vacation"])).unwrap();
	budget.commit(&rows);
	let order = |budget: &Budget, group: &str| {
		let mut members: Vec<&Category> = budget
			.data
			.categories
			.values()
			.filter(|category| category.group.as_str() == group && !category.tombstone)
			.collect();
		members.sort_by(|a, b| a.sort_order.total_cmp(&b.sort_order));
		members
			.into_iter()
			.map(|category| category.name.clone())
			.collect::<Vec<_>>()
	};
	assert_eq!(
		order(&budget, "goals"),
		["Vacation", "New laptop", "Gift fund"]
	);
	assert_eq!(order(&budget, "everyday"), ["Groceries", "Gas"]);
	let record = &budget.data.migration_applied["goal:vacation"];
	let prior: serde_json::Value = serde_json::from_str(&record.prior).unwrap();
	assert_eq!(prior["group"], "everyday");
	assert_eq!(prior["before"], "gas");
	assert!(
		budget
			.data
			.goals
			.values()
			.any(|goal| goal.category.as_str() == "vacation")
	);
	assert!(
		rows.iter()
			.all(|row| !matches!(row, Row::Note(_) | Row::BudgetCell(_))),
		"no Actual note or cell unless ticked"
	);

	match budget.revert(TODAY, "goal:vacation", None, false) {
		RevertPlan::Rows { rows, notes } => {
			assert!(notes.is_empty());
			budget.commit(&rows);
		}
		other => panic!("{other:?}"),
	}
	assert_eq!(order(&budget, "everyday"), ["Groceries", "Vacation", "Gas"]);
	assert!(
		!budget
			.data
			.goals
			.values()
			.any(|goal| goal.category.as_str() == "vacation")
	);
	assert!(budget.data.migration_applied.is_empty());
}

#[test]
fn example_19_no_goals_group_makes_one_and_a_hidden_one_needs_consent() {
	let mut budget = goal_budget();
	budget.category("trip", "Trip", "everyday", 4.0);
	budget.note("vacation", "#goal 1000");
	budget.note("trip", "#goal 2000");
	let rows = budget
		.apply(TODAY, &tick(&["goal:vacation", "goal:trip"]))
		.unwrap();
	let created: Vec<&CategoryGroup> = rows
		.iter()
		.filter_map(|row| match row {
			Row::CategoryGroup(group) => Some(group),
			_ => None,
		})
		.collect();
	assert_eq!(created.len(), 1);
	assert_eq!(created[0].name, "Goals");
	assert_eq!(created[0].sort_order, 99.0 + 16_384.0);
	budget.commit(&rows);
	let goals = goal::goals_group(&budget.data).unwrap().id.clone();
	let mut members: Vec<&Category> = budget
		.data
		.categories
		.values()
		.filter(|c| c.group == goals)
		.collect();
	members.sort_by(|a, b| a.sort_order.total_cmp(&b.sort_order));
	assert_eq!(
		members.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
		["Vacation", "Trip"]
	);

	let mut hidden = goal_budget();
	hidden.group("goals", " goals ", false, 2.0);
	if let Some(group) = hidden
		.data
		.category_groups
		.get_mut(&CategoryGroupId::new("goals"))
	{
		group.hidden = true;
	}
	hidden.note("vacation", "#goal 1000");
	assert!(hidden.apply(TODAY, &tick(&["goal:vacation"])).is_err());
	let mut consent = tick(&["goal:vacation"]);
	consent.consent_show_goals_group = true;
	let rows = hidden.apply(TODAY, &consent).unwrap();
	assert!(rows.iter().any(
		|row| matches!(row, Row::CategoryGroup(group) if group.id.as_str() == "goals" && !group.hidden)
	));
	assert!(
		!rows
			.iter()
			.any(|row| matches!(row, Row::CategoryGroup(group) if group.id.as_str() != "goals")),
		"no second group"
	);
}

#[test]
fn examples_20_and_23_one_name_twice_blocks_the_item() {
	let mut budget = goal_budget();
	budget.group("goals", "Goals", false, 2.0);
	budget.category("other", "vacation", "goals", 1.0);
	budget.note("vacation", "#goal 1000");
	let all = budget.suggestions(TODAY);
	assert_eq!(
		find(&all, "goal:vacation")
			.and_then(|s| s.blocked.clone())
			.as_deref(),
		Some(goal::RENAME_FIRST)
	);
	assert!(budget.apply(TODAY, &tick(&["goal:vacation"])).is_err());

	let mut twice = goal_budget();
	twice.group("fun", "Fun", false, 2.0);
	twice.category("vacation2", "Vacation", "fun", 1.0);
	twice.note("vacation", "#goal 1000");
	twice.note("vacation2", "#goal 2000");
	let all = twice.suggestions(TODAY);
	assert_eq!(
		find(&all, "goal:vacation").and_then(|s| s.blocked_by.clone()),
		None
	);
	assert_eq!(
		find(&all, "goal:vacation2")
			.and_then(|s| s.blocked_by.clone())
			.as_deref(),
		Some("goal:vacation")
	);
	assert!(
		twice
			.apply(TODAY, &tick(&["goal:vacation", "goal:vacation2"]))
			.is_err()
	);
	assert!(twice.apply(TODAY, &tick(&["goal:vacation2"])).is_ok());
}

#[test]
fn example_21_group_totals_move_with_the_category_and_grand_totals_stay() {
	let mut budget = goal_budget();
	budget.note("vacation", "#goal 100000");
	budget.row("checking", "2026-10-04", -41_200, Some("groceries"), None);
	budget.row("checking", "2026-10-05", -10_000, Some("gas"), None);
	budget.row("checking", "2026-10-06", -5_000, Some("vacation"), None);
	let all = budget.suggestions(TODAY);
	let goal = find(&all, "goal:vacation").unwrap();
	let everyday = goal
		.effects
		.iter()
		.find(|effect| effect.report == "Spending in Everyday")
		.unwrap();
	assert_eq!(
		(everyday.before.minor(), everyday.after.minor()),
		(56_200, 51_200)
	);
	let goals = goal
		.effects
		.iter()
		.find(|effect| effect.report == "Spending in Goals")
		.unwrap();
	assert_eq!((goals.before.minor(), goals.after.minor()), (0, 5_000));
}

#[test]
fn adjacent_goals_adopted_together_record_the_same_neighbour_mig_7_4() {
	let mut budget = goal_budget();
	budget.category("x", "X", "everyday", 0.5);
	budget.category("a", "A", "everyday", 2.1);
	budget.category("b", "B", "everyday", 2.2);
	budget.category("c", "C", "everyday", 2.3);
	budget.note("a", "#goal 1000");
	budget.note("b", "#goal 1000");
	let rows = budget.apply(TODAY, &tick(&["goal:a", "goal:b"])).unwrap();
	budget.commit(&rows);
	for item in ["goal:a", "goal:b"] {
		let prior: serde_json::Value =
			serde_json::from_str(&budget.data.migration_applied[item].prior).unwrap();
		assert_eq!(prior["before"], "c", "{item}");
	}
	for item in ["goal:b", "goal:a"] {
		if let RevertPlan::Rows { rows, .. } = budget.revert(TODAY, item, None, false) {
			budget.commit(&rows);
		}
	}
	let mut members: Vec<&Category> = budget
		.data
		.categories
		.values()
		.filter(|c| c.group.as_str() == "everyday")
		.collect();
	members.sort_by(|a, b| a.sort_order.total_cmp(&b.sort_order));
	let names: Vec<&str> = members.iter().map(|c| c.name.as_str()).collect();
	let c = names.iter().position(|name| *name == "C").unwrap();
	assert_eq!(&names[c - 2..=c], ["B", "A", "C"], "{names:?}");
}

#[test]
fn a_revert_whose_group_is_gone_writes_nothing_and_asks() {
	let mut budget = goal_budget();
	budget.note("vacation", "#goal 1000");
	let rows = budget.apply(TODAY, &tick(&["goal:vacation"])).unwrap();
	budget.commit(&rows);
	if let Some(group) = budget
		.data
		.category_groups
		.get_mut(&CategoryGroupId::new("everyday"))
	{
		group.tombstone = true;
	}
	match budget.revert(TODAY, "goal:vacation", None, false) {
		RevertPlan::ChooseGroup { message } => {
			assert_eq!(
				message,
				"The group Everyday no longer exists. Choose a group for Vacation."
			);
		}
		other => panic!("{other:?}"),
	}
	budget.group("fun", "Fun", false, 3.0);
	match budget.revert(
		TODAY,
		"goal:vacation",
		Some(&CategoryGroupId::new("fun")),
		false,
	) {
		RevertPlan::Rows { rows, .. } => {
			budget.commit(&rows);
			assert_eq!(
				budget.data.categories[&CategoryId::new("vacation")]
					.group
					.as_str(),
				"fun"
			);
		}
		other => panic!("{other:?}"),
	}
}

#[test]
fn the_opt_in_goal_line_is_written_and_reverted_only_while_unchanged() {
	let mut budget = goal_budget();
	budget.budget("vacation", "2026-11", 0);
	budget.data.category_templates.insert(
		CategoryId::new("vacation"),
		veritas_domain::CategoryTemplates {
			goal_def: Some("[{\"type\":\"goal\",\"amount\":500}]".to_owned()),
			template_settings: Some("{\"source\":\"ui\"}".to_owned()),
		},
	);
	let mut selection = tick(&["goal:vacation"]);
	selection.items[0].edits.write_note_in_actual = true;
	let rows = budget.apply(TODAY, &selection).unwrap();
	assert!(
		rows.iter().any(
			|row| matches!(row, Row::Note(note) if note.text.as_deref() == Some("#goal 500.00"))
		)
	);
	budget.commit(&rows);
	match budget.revert(TODAY, "goal:vacation", None, false) {
		RevertPlan::Rows { rows, notes } => {
			assert!(notes.is_empty());
			assert!(
				rows.iter()
					.any(|row| matches!(row, Row::Note(note) if note.text.is_none()))
			);
		}
		other => panic!("{other:?}"),
	}
	budget.note("vacation", "#goal 600");
	match budget.revert(TODAY, "goal:vacation", None, false) {
		RevertPlan::Rows { rows, notes } => {
			assert_eq!(notes.len(), 1);
			assert!(!rows.iter().any(|row| matches!(row, Row::Note(_))));
		}
		other => panic!("{other:?}"),
	}
}

// (c) MIG-5

fn loan_budget() -> Budget {
	let mut budget = Budget::new();
	budget.account("checking", "Checking", false);
	budget.account("loan", "Car Loan", true);
	budget.opening("checking", 1_000_000);
	budget.opening("loan", -980_000);
	budget.data.account_meta.insert(
		AccountId::new("loan"),
		AccountMeta {
			kind: Some(AccountKind::Liability),
			..AccountMeta::default()
		},
	);
	budget.group("bills", "Bills", false, 1.0);
	budget.category("carloan", "Car Loan", "bills", 1.0);
	budget.category("food", "Food", "bills", 2.0);
	for date in ["2026-08-20", "2026-09-20", "2026-10-20"] {
		budget.transfer("checking", "loan", "carloan", date, 38_550);
	}
	budget.row("checking", "2026-10-02", 310_000, Some("salary"), None);
	budget.row("checking", "2026-10-03", -120_000, Some("food"), None);
	budget
}

#[test]
fn example_10_transfers_to_a_loan_are_pre_ticked_debt_principal() {
	let budget = loan_budget();
	let all = budget.suggestions(TODAY);
	let kind = find(&all, "kind:carloan").expect("suggested");
	assert_eq!(kind.confidence, Confidence::PreTicked);
	assert_eq!(kind.proposed.kind, Some(CategoryKind::DebtPrincipal));
	let change = kind.kind_change.as_ref().unwrap();
	assert_eq!(
		(change.rows, change.transfers, change.amount.minor()),
		(3, 3, 115_650)
	);
	let figure = |name: &str| {
		kind.effects
			.iter()
			.find(|effect| effect.report == name)
			.map(|e| (e.before.minor(), e.after.minor()))
	};
	assert_eq!(figure("Expenses"), Some((158_550, 120_000)));
	assert_eq!(figure("Debt paydown"), Some((0, 38_550)));
	let (before, after) = figure("Net cash flow").unwrap();
	assert_eq!(before, after, "net cash flow is unchanged");

	let mut older = loan_budget();
	older.transfer("checking", "loan", "carloan", "2025-06-20", 38_550);
	let all = older.suggestions(TODAY);
	let change = find(&all, "kind:carloan")
		.and_then(|s| s.kind_change.clone())
		.unwrap();
	assert_eq!(
		(change.transfers, change.amount.minor()),
		(4, 154_200),
		"the Confirm step counts all history"
	);
}

#[test]
fn example_10_revert_shows_the_reversed_confirm_before_writing() {
	let mut budget = loan_budget();
	let rows = budget.apply(TODAY, &tick(&["kind:carloan"])).unwrap();
	budget.commit(&rows);
	assert_eq!(
		budget.data.category_kind(&CategoryId::new("carloan")),
		CategoryKind::DebtPrincipal
	);
	match budget.revert(TODAY, "kind:carloan", None, false) {
		RevertPlan::ConfirmKind(confirm) => {
			assert_eq!(
				(
					confirm.change.rows,
					confirm.change.transfers,
					confirm.change.amount.minor()
				),
				(3, 3, 115_650)
			);
			assert_eq!(confirm.replaces, None);
		}
		other => panic!("{other:?}"),
	}
	match budget.revert(TODAY, "kind:carloan", None, true) {
		RevertPlan::Rows { rows, .. } => budget.commit(&rows),
		other => panic!("{other:?}"),
	}
	assert_eq!(
		budget.data.category_kind(&CategoryId::new("carloan")),
		CategoryKind::Expense
	);
}

#[test]
fn example_11_transfers_to_an_investment_account_are_savings() {
	let mut budget = Budget::new();
	budget.account("checking", "Checking", false);
	budget.account("brokerage", "Brokerage", true);
	budget.opening("checking", 1_000_000);
	budget.opening("brokerage", 100_000);
	budget.group("bills", "Bills", false, 1.0);
	budget.category("fund", "Emergency Fund", "bills", 1.0);
	for date in ["2026-08-05", "2026-09-05", "2026-10-05"] {
		budget.transfer("checking", "brokerage", "fund", date, 25_000);
	}
	budget
		.data
		.account_types
		.insert(AccountId::new("brokerage"), "investment".to_owned());
	let all = budget.suggestions(TODAY);
	let kind = find(&all, "kind:fund").expect("suggested");
	assert_eq!(
		(kind.confidence, kind.proposed.kind),
		(Confidence::PreTicked, Some(CategoryKind::Savings))
	);
	assert!(
		find(&all, "goal:fund").is_none(),
		"example 13: a (c) candidate is never a goal"
	);
	budget.data.account_types.clear();
	let all = budget.suggestions(TODAY);
	assert_eq!(
		find(&all, "kind:fund").map(|s| s.confidence),
		Some(Confidence::Shown)
	);
}

#[test]
fn example_12_one_transfer_mixed_counterparties_or_a_name_get_nothing() {
	let mut budget = loan_budget();
	budget.account("car", "Car", true);
	budget.opening("car", 1_500_000);
	budget.category("car_cat", "Car", "bills", 3.0);
	budget.transfer("checking", "car", "car_cat", "2026-10-11", 50_000);
	budget.category("principal", "Loan Principal", "bills", 4.0);
	budget.account("savings", "Savings", true);
	budget.opening("savings", 10_000);
	budget.category("mixed", "Mixed", "bills", 5.0);
	for date in ["2026-08-10", "2026-09-10"] {
		budget.transfer("checking", "loan", "mixed", date, 1_000);
		budget.transfer("checking", "savings", "mixed", date, 1_000);
	}
	let all = budget.suggestions(TODAY);
	assert!(find(&all, "kind:car_cat").is_none());
	assert!(find(&all, "kind:principal").is_none());
	assert!(find(&all, "kind:mixed").is_none());
}

// (d) MIG-6

fn bill_budget() -> Budget {
	let mut budget = Budget::new();
	budget.account("checking", "Checking", false);
	budget.account("loan", "Car Loan", true);
	budget.opening("loan", -980_000);
	budget.data.account_meta.insert(
		AccountId::new("loan"),
		AccountMeta {
			loan_rate_bps: Some(649),
			..AccountMeta::default()
		},
	);
	budget.group("bills", "Bills", false, 1.0);
	budget.category("fitness", "Fitness", "bills", 1.0);
	budget.category("health", "Health", "bills", 2.0);
	budget.payee("netflix", "Netflix");
	budget.payee("gym", "Gym");
	budget.payee("landlord", "Landlord");
	budget.payee("work", "Work");
	budget
}

fn schedule(
	budget: &mut Budget,
	id: &str,
	name: Option<&str>,
	payee: &str,
	amount: i64,
	category: Option<&str>,
) {
	let rule = RuleId::new(format!("rule-{id}"));
	let conditions = format!(
		"[{{\"op\":\"is\",\"field\":\"payee\",\"value\":\"{payee}\"}},{{\"op\":\"is\",\"field\":\"account\",\"value\":\"checking\"}},{{\"op\":\"isapprox\",\"field\":\"date\",\"value\":{{\"start\":\"2026-01-05\",\"frequency\":\"monthly\",\"interval\":1,\"patterns\":[],\"skipWeekend\":false,\"weekendSolveMode\":\"after\",\"endMode\":\"never\"}}}},{{\"op\":\"isapprox\",\"field\":\"amount\",\"value\":{amount}}}]"
	);
	let actions = match category {
		Some(category) => format!(
			"[{{\"op\":\"link-schedule\",\"value\":\"{id}\"}},{{\"op\":\"set\",\"field\":\"category\",\"value\":\"{category}\"}}]"
		),
		None => format!("[{{\"op\":\"link-schedule\",\"value\":\"{id}\"}}]"),
	};
	budget.data.rules.insert(
		rule.clone(),
		Rule {
			id: rule.clone(),
			stage: RuleStage::Default,
			conditions_op: veritas_domain::ConditionsOp::And,
			conditions_json: conditions,
			actions_json: actions,
			tombstone: false,
		},
	);
	let schedule = ScheduleId::new(id);
	budget.data.schedules.insert(
		schedule.clone(),
		Schedule {
			id: schedule,
			name: name.map(str::to_owned),
			rule,
			posts_transaction: false,
			completed: false,
			next_date: Some(day("2026-12-05")),
			tombstone: false,
		},
	);
}

#[test]
fn example_14_names_come_from_the_payee_and_collisions_get_told_apart() {
	let mut budget = bill_budget();
	schedule(&mut budget, "netflix", None, "netflix", -1_599, None);
	schedule(&mut budget, "g1", None, "gym", -4_000, Some("fitness"));
	schedule(&mut budget, "g2", None, "gym", -4_000, Some("health"));
	schedule(
		&mut budget,
		"rent",
		Some("Rent"),
		"landlord",
		-150_000,
		None,
	);
	let all = budget.suggestions(TODAY);
	let name = |key: &str| find(&all, key).and_then(|s| s.proposed.display_name.clone());
	assert_eq!(name("bill:netflix").as_deref(), Some("Netflix"));
	assert_eq!(
		find(&all, "bill:netflix").map(|s| s.confidence),
		Some(Confidence::PreTicked)
	);
	assert_eq!(name("bill:g1").as_deref(), Some("Gym (Fitness)"));
	assert_eq!(name("bill:g2").as_deref(), Some("Gym (Health)"));
	assert!(
		find(&all, "bill:rent").is_none(),
		"a named bill is left alone"
	);

	let mut same = bill_budget();
	schedule(&mut same, "s1", None, "gym", -4_000, Some("fitness"));
	schedule(&mut same, "s2", None, "gym", -4_000, Some("fitness"));
	let all = same.suggestions(TODAY);
	assert_eq!(
		find(&all, "bill:s1")
			.and_then(|s| s.proposed.display_name.clone())
			.as_deref(),
		Some("Gym 1")
	);
	assert_eq!(
		find(&all, "bill:s2")
			.and_then(|s| s.proposed.display_name.clone())
			.as_deref(),
		Some("Gym 2")
	);
}

#[test]
fn example_16_icons_come_only_from_structure() {
	let mut budget = bill_budget();
	schedule(&mut budget, "pay", Some("Paycheck"), "work", 310_000, None);
	schedule(
		&mut budget,
		"car",
		Some("Car payment"),
		"to-loan",
		-38_550,
		None,
	);
	schedule(&mut budget, "gym", Some("Gym"), "gym", -4_000, None);
	let all = budget.suggestions(TODAY);
	assert_eq!(
		find(&all, "bill:pay")
			.and_then(|s| s.proposed.icon.clone())
			.as_deref(),
		Some(bill::INCOME_ICON)
	);
	assert_eq!(
		find(&all, "bill:car")
			.and_then(|s| s.proposed.icon.clone())
			.as_deref(),
		Some(bill::LOAN_ICON)
	);
	assert!(find(&all, "bill:gym").is_none());
}

#[test]
fn a_bill_revert_brings_back_the_replaced_name_and_icon() {
	let mut budget = bill_budget();
	schedule(&mut budget, "pay", Some("Paycheck"), "work", 310_000, None);
	budget.data.schedule_meta.insert(
		ScheduleId::new("pay"),
		veritas_domain::ScheduleMeta {
			paused: false,
			display_name: None,
			icon: Some("💵".to_owned()),
		},
	);
	let all = budget.suggestions(TODAY);
	assert_eq!(
		find(&all, "bill:pay").map(|s| s.confidence),
		Some(Confidence::Shown),
		"it replaces an icon"
	);
	let rows = budget.apply(TODAY, &tick(&["bill:pay"])).unwrap();
	assert!(
		rows.iter()
			.all(|row| !matches!(row, Row::Schedule(_) | Row::Rule(_)))
	);
	budget.commit(&rows);
	assert_eq!(
		budget.data.schedule_meta[&ScheduleId::new("pay")]
			.icon
			.as_deref(),
		Some(bill::INCOME_ICON)
	);
	if let RevertPlan::Rows { rows, .. } = budget.revert(TODAY, "bill:pay", None, false) {
		budget.commit(&rows);
	}
	assert_eq!(
		budget.data.schedule_meta[&ScheduleId::new("pay")]
			.icon
			.as_deref(),
		Some("💵")
	);
}

// MIG-1

#[test]
fn dismissing_and_offering_are_settings_rows() {
	let mut budget = goal_budget();
	assert_eq!(offered_rows(&budget.data).len(), 1);
	budget.commit(&offered_rows(&budget.data));
	assert!(was_offered(&budget.data));
	assert!(offered_rows(&budget.data).is_empty());
	budget.note("vacation", "#goal 1000");
	let rows = dismissal_rows(&budget.data, "goal:vacation", true).unwrap();
	budget.commit(&rows);
	assert!(
		find(&budget.suggestions(TODAY), "goal:vacation")
			.unwrap()
			.dismissed
	);
	let rows = dismissal_rows(&budget.data, "goal:vacation", false).unwrap();
	budget.commit(&rows);
	assert!(
		!find(&budget.suggestions(TODAY), "goal:vacation")
			.unwrap()
			.dismissed
	);
	assert!(
		!budget
			.data
			.settings
			.contains_key(&veritas_domain::SettingKey::MigrationDismissed)
	);
	assert!(dismissal_rows(&budget.data, "nonsense", true).is_err());
}

#[test]
fn detection_is_pure_and_deterministic_mig_7_2() {
	let mut budget = loan_budget();
	budget.note("food", "#goal 1000");
	let before = budget.data.clone();
	assert_eq!(budget.suggestions(TODAY), budget.suggestions(TODAY));
	assert_eq!(budget.data, before);
}

#[test]
fn a_key_that_is_no_longer_suggested_refuses_the_whole_apply() {
	let mut budget = goal_budget();
	budget.note("vacation", "#goal 1000");
	assert!(
		budget
			.apply(TODAY, &tick(&["goal:vacation", "goal:groceries"]))
			.is_err()
	);
	assert!(
		budget
			.apply(TODAY, &tick(&["goal:vacation", "goal:vacation"]))
			.is_err()
	);
}

#[test]
fn example_17_one_apply_holds_every_conversion() {
	let mut budget = loan_budget();
	budget.category("vacation", "Vacation", "bills", 3.0);
	budget.note("vacation", "#template 3000 by 2027-06");
	budget.payee("netflix", "Netflix");
	schedule(&mut budget, "netflix", None, "netflix", -1_599, None);
	let rows = budget
		.apply(
			TODAY,
			&tick(&["goal:vacation", "kind:carloan", "bill:netflix"]),
		)
		.unwrap();
	let records = rows
		.iter()
		.filter(|row| matches!(row, Row::MigrationApplied(_)))
		.count();
	assert_eq!(records, 3);
	budget.commit(&rows);
	let items: Vec<String> = applied_items(&budget.data)
		.into_iter()
		.map(|item| item.key)
		.collect();
	assert_eq!(items, ["bill:netflix", "goal:vacation", "kind:carloan"]);
	let all = budget.suggestions(TODAY);
	assert!(
		all.iter().all(|s| !items.contains(&s.key)),
		"converted items disappear by themselves"
	);
}

// Fred's rulings on #122 (docs/finance/migration.md as amended in #123)

#[test]
fn example_7a_s2_takes_fc_1s_median_half_away_from_zero() {
	let steady = |spent: i64| {
		let mut budget = goal_budget();
		for (at, amount) in [("2026-06", 5_000), ("2026-07", 5_000), ("2026-09", 5_001), ("2026-10", 5_001)] {
			budget.budget("vacation", at, amount);
		}
		budget.row("checking", "2026-10-14", -spent, Some("vacation"), None);
		find(&budget.suggestions(TODAY), "goal:vacation").is_some()
	};
	assert!(!steady(10_001), "the median is 5001, so 10001 left is under the 10002 threshold");
	assert!(steady(10_000), "10002 left meets it");
}

#[test]
fn example_7b_and_mig_4_2_lines_follow_actuals_grammar() {
	let line = |text: &str, places| goal::from_line(text, places).map(|target| target.amount.minor());
	assert_eq!(line("#goal 100.5", 0), Some(101));
	assert_eq!(line("#goal 100.49", 0), Some(100));
	assert_eq!(line("#goal ¥ 3000.", 0), Some(3_000));
	assert_eq!(line("#goal¥100.5", 0), Some(101));
	assert_eq!(line("#goal $ 3000", 2), Some(300_000));
	assert_eq!(line("Vacation #goal 3000", 2), Some(300_000), "read from the first #");
	assert_eq!(line("#goal 3000.55", 1), Some(30_006));
	assert_eq!(line("#goal 3000.55", 3), Some(3_000_550));
	assert_eq!(line("#GOAL 3000", 2), None, "the start is case-sensitive");
	assert_eq!(line("#goal 3,000", 2), None);
	assert_eq!(line("#goal 3000.123", 2), None);
	assert_eq!(line("#goal -5", 2), None);
	let dated = goal::from_line("#template-2 3000.5 BY 2027-06", 2).unwrap();
	assert_eq!((dated.amount.minor(), dated.date), (300_050, Some(day("2027-06-30"))));
	assert!(goal::from_line("#template 3000 by 2027-06 SPEND FROM 2027-01", 2).is_some());
	assert!(goal::from_line("#template 3000 by 2027-06 repeat every year", 2).is_none());
}

#[test]
fn mig_4_1_the_same_name_is_the_moves_own_upper_case_test() {
	let mut budget = goal_budget();
	budget.group("goals", "Goals", false, 2.0);
	budget.category("street", "STRASSE", "goals", 1.0);
	budget.category("strasse", "Straße", "everyday", 4.0);
	budget.category("spaced", "Vacation ", "goals", 2.0);
	budget.note("strasse", "#goal 100");
	budget.note("vacation", "#goal 100");
	let all = budget.suggestions(TODAY);
	assert_eq!(
		find(&all, "goal:strasse").and_then(|s| s.blocked.clone()).as_deref(),
		Some(goal::RENAME_FIRST),
		"Straße upper-cases to STRASSE"
	);
	assert_eq!(
		find(&all, "goal:vacation").and_then(|s| s.blocked.clone()),
		None,
		"\"Vacation \" with a trailing space is a different name"
	);
	assert!(budget.apply(TODAY, &tick(&["goal:vacation"])).is_ok(), "a name called free never fails Apply");
}

#[test]
fn mig_6_3_an_overdrawn_checking_account_never_gets_the_loan_icon() {
	let mut budget = bill_budget();
	budget.account("joint", "Joint checking", false);
	budget.opening("joint", -5_000);
	schedule(&mut budget, "top-up", Some("Top up"), "to-joint", -5_000, None);
	schedule(&mut budget, "car", Some("Car payment"), "to-loan", -38_550, None);
	let all = budget.suggestions(TODAY);
	assert!(find(&all, "bill:top-up").is_none(), "a balance below zero is ST-9.1 rule 3, which MIG-6.3 leaves out");
	let car = find(&all, "bill:car").unwrap();
	assert!(car.signals.iter().any(|signal| signal.text == "It pays Car Loan, a debt"));
}

#[test]
fn mig_3_3_a_deleted_account_with_live_rows_counts_in_the_comparison() {
	let mut budget = net_worth_budget();
	budget.account("old", "Old savings", false);
	budget.row("old", "2026-09-01", 5_000, None, None);
	if let Some(account) = budget.data.accounts.get_mut(&AccountId::new("old")) {
		account.tombstone = true;
	}
	let all = budget.suggestions(TODAY);
	if let Some(report) = find(&all, "report:widget") {
		let equal = report.signals.iter().find(|signal| signal.code == "equal").unwrap();
		assert!(
			equal.figures.iter().all(|figure| figure.value == 695_000),
			"suggested only when the report counts the deleted account's 5000 too"
		);
	}
}

#[test]
fn mig_4_7_a_closed_goal_is_not_reverted_and_shows_as_closed() {
	let mut budget = goal_budget();
	budget.note("vacation", "#goal 1000");
	let rows = budget.apply(TODAY, &tick(&["goal:vacation"])).unwrap();
	budget.commit(&rows);
	for goal in budget.data.goals.values_mut() {
		goal.status = veritas_domain::GoalStatus::Closed;
	}
	assert!(budget.try_revert(TODAY, "goal:vacation").is_err());
	let items = applied_items(&budget.data);
	assert!(items.iter().any(|item| item.key == "goal:vacation" && item.closed));
	assert!(budget.data.goals.values().any(|goal| goal.category.as_str() == "vacation"), "the goal and its history stay");
}

#[test]
fn mig_2_4_example_5_the_confirm_step_has_the_goal_lines() {
	let mut budget = goal_budget();
	budget.note("vacation", "#template 3000 by 2027-06");
	budget.budget("vacation", "2026-10", 120_000);
	let all = budget.suggestions(TODAY);
	let preview = find(&all, "goal:vacation").and_then(|s| s.goal_preview.clone()).unwrap();
	assert_eq!(preview.monthly, Some(Money::from_minor(22_500)), "example 5: ceil(180000 / 8)");
	assert_eq!(preview.months_left, Some(8));
	assert_eq!(preview.reached_month, None);
	let mut edited = tick(&["goal:vacation"]);
	edited.items[0].edits.target = Some(Money::from_minor(100_000));
	let shown = budget.preview(TODAY, &edited);
	assert_eq!(
		shown[0].goal_preview.as_ref().and_then(|p| p.reached_month),
		Some(month("2026-10")),
		"an edited target the envelope already holds is reached, backfilled to its first month (GOAL-9)"
	);
}

#[test]
fn mig_2_4_and_5_4_the_confirm_figures_follow_the_chosen_kind() {
	let budget = loan_budget();
	let mut selection = tick(&["kind:carloan"]);
	selection.items[0].edits.kind = Some(CategoryKind::Savings);
	let shown = budget.preview(TODAY, &selection);
	let change = shown[0].kind_change.clone().unwrap();
	assert_eq!(change.kind, CategoryKind::Savings);
	assert!(shown[0].effects.iter().any(|effect| effect.report == "Savings"));
	assert!(!shown[0].effects.iter().any(|effect| effect.report == "Debt paydown"));
}
