use std::cmp::Ordering;
use std::collections::BTreeMap;

use veritas_domain::{
	BudgetData, CategoryGroupId, CategoryId, CoreError, GoalStatus, Money, TransactionId,
	TransactionKind,
};

use crate::{Classification, ClassifiedRow, Decision, Period};

/// A number and the rows behind it, by date then id. Flow lines list primary
/// legs only. The in-transit lines (E7 and its ST-6 twin) list whichever leg of
/// a pair is dated in the period, mirror legs included.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Line {
	pub total: Money,
	pub rows: Vec<TransactionId>,
}

/// How a category is shown in a historical report. Hidden, archived, deleted
/// and closed-goal categories are always included, never dropped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CategoryLabels {
	pub hidden: bool,
	pub archived: bool,
	pub deleted: bool,
	pub closed_goal: bool,
	/// The uncategorized line: its rows wait for a category.
	pub needs_review: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CategoryLine {
	/// `None` is the uncategorized line.
	pub category: Option<CategoryId>,
	pub labels: CategoryLabels,
	pub total: Money,
	pub rows: Vec<TransactionId>,
}

/// One Net Profit line with its per-category breakdown, which sums to `total`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Section {
	pub total: Money,
	pub rows: Vec<TransactionId>,
	pub categories: Vec<CategoryLine>,
}

/// One category group's share of a section. A section holds rows of one kind
/// only (CLS-6), so a group's figure counts only rows of that kind whose
/// category is in the group, never every row of the group's categories.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupLine {
	pub group: CategoryGroupId,
	/// `hidden` and `deleted` are the group's own (ST-10.3).
	pub labels: CategoryLabels,
	pub total: Money,
	pub rows: Vec<TransactionId>,
}

/// The section's category lines totalled by group, in budget order. A line
/// whose category has no group record, and the uncategorized line, are in no
/// group, so the groups plus those lines add up to the section's total.
pub fn group_lines(budget: &BudgetData, section: &Section) -> Result<Vec<GroupLine>, CoreError> {
	let mut groups: Vec<GroupLine> = Vec::new();
	for line in &section.categories {
		let Some(group) = line
			.category
			.as_ref()
			.and_then(|id| budget.categories.get(id))
			.and_then(|category| budget.category_groups.get(&category.group))
		else {
			continue;
		};
		match groups.iter_mut().find(|known| known.group == group.id) {
			Some(known) => {
				known.total = known.total.checked_add(line.total)?;
				known.rows.extend(line.rows.iter().cloned());
			}
			None => groups.push(GroupLine {
				group: group.id.clone(),
				labels: CategoryLabels {
					hidden: group.hidden,
					deleted: group.tombstone,
					..CategoryLabels::default()
				},
				total: line.total,
				rows: line.rows.clone(),
			}),
		}
	}
	Ok(groups)
}

/// Every amount is a positive-means-normal magnitude: income earned, money
/// spent, principal paid, money saved. Expenses go negative when refunds
/// exceed spending, and `unreviewed` is positive when money left the budget.
#[derive(Clone, Debug, PartialEq)]
pub struct NetProfit {
	pub period: Period,
	pub income: Section,
	pub expenses: Section,
	pub net_profit: Line,
	pub debt_paydown: Section,
	pub savings: Section,
	pub net_cash_flow: Line,
	/// Shown under net cash flow, never inside it.
	pub unreviewed: Line,
}

pub fn net_profit(
	budget: &BudgetData,
	classification: &Classification,
	period: Period,
) -> Result<NetProfit, CoreError> {
	let mut income = Accumulator::default();
	let mut expenses = Accumulator::default();
	let mut debt_paydown = Accumulator::default();
	let mut savings = Accumulator::default();
	let mut unreviewed = Accumulator::default();
	let mut net_profit_rows = Vec::new();
	let mut net_cash_flow_rows = Vec::new();

	for row in classification.within(period) {
		if !row.is_primary() {
			continue;
		}
		let section = match row.kind() {
			TransactionKind::Income => Some(&mut income),
			TransactionKind::Expense => Some(&mut expenses),
			TransactionKind::DebtPrincipal => Some(&mut debt_paydown),
			TransactionKind::SavingsContribution => Some(&mut savings),
			TransactionKind::InternalTransfer
			| TransactionKind::StartingBalance
			| TransactionKind::Adjustment => None,
		};
		if let Some(section) = section {
			section.add(row)?;
			if matches!(
				row.kind(),
				TransactionKind::Income | TransactionKind::Expense
			) {
				net_profit_rows.push(row.id.clone());
			}
			net_cash_flow_rows.push(row.id.clone());
		} else if is_unreviewed(row) {
			unreviewed.add(row)?;
		}
	}

	let income = income.inflow(budget)?;
	let expenses = expenses.outflow(budget)?;
	let debt_paydown = debt_paydown.outflow(budget)?;
	let savings = savings.outflow(budget)?;
	let net_profit = income.total.checked_sub(expenses.total)?;
	let net_cash_flow = net_profit
		.checked_sub(debt_paydown.total)?
		.checked_sub(savings.total)?;
	let unreviewed = unreviewed.outflow(budget)?;

	Ok(NetProfit {
		period,
		income,
		expenses,
		net_profit: Line {
			total: net_profit,
			rows: net_profit_rows,
		},
		debt_paydown,
		savings,
		net_cash_flow: Line {
			total: net_cash_flow,
			rows: net_cash_flow_rows,
		},
		unreviewed: Line {
			total: unreviewed.total,
			rows: unreviewed.rows,
		},
	})
}

/// An on-budget transfer nobody has explained yet: an uncategorized on↔off
/// transfer, or any transfer whose partner is missing.
fn is_unreviewed(row: &ClassifiedRow) -> bool {
	row.on_budget
		&& row.kind() == TransactionKind::InternalTransfer
		&& (row.decision == Decision::UncategorizedTransfer || row.flags.unpaired)
}

#[derive(Default)]
struct Accumulator {
	sum: Money,
	rows: Vec<TransactionId>,
	categories: BTreeMap<Option<CategoryId>, (Money, Vec<TransactionId>)>,
}

impl Accumulator {
	fn add(&mut self, row: &ClassifiedRow) -> Result<(), CoreError> {
		self.sum = self.sum.checked_add(row.amount)?;
		self.rows.push(row.id.clone());
		let (sum, rows) = self.categories.entry(row.category.clone()).or_default();
		*sum = sum.checked_add(row.amount)?;
		rows.push(row.id.clone());
		Ok(())
	}

	fn inflow(self, budget: &BudgetData) -> Result<Section, CoreError> {
		self.into_section(budget, Ok)
	}

	fn outflow(self, budget: &BudgetData) -> Result<Section, CoreError> {
		self.into_section(budget, Money::checked_neg)
	}

	fn into_section(
		self,
		budget: &BudgetData,
		magnitude: fn(Money) -> Result<Money, CoreError>,
	) -> Result<Section, CoreError> {
		let categories = self
			.categories
			.into_iter()
			.map(|(category, (sum, rows))| {
				Ok(CategoryLine {
					labels: category.as_ref().map_or(
						CategoryLabels {
							needs_review: true,
							..CategoryLabels::default()
						},
						|category| labels(budget, category),
					),
					category,
					total: magnitude(sum)?,
					rows,
				})
			})
			.collect::<Result<Vec<_>, CoreError>>()?;
		let mut keyed: Vec<(OrderKey, CategoryLine)> = categories
			.into_iter()
			.map(|line| (order_key(budget, line.category.as_ref()), line))
			.collect();
		keyed.sort_by(|(a, _), (b, _)| budget_order(a, b));
		let categories = keyed.into_iter().map(|(_, line)| line).collect();
		Ok(Section {
			total: magnitude(self.sum)?,
			rows: self.rows,
			categories,
		})
	}
}

pub fn labels(budget: &BudgetData, id: &CategoryId) -> CategoryLabels {
	let category = budget.categories.get(id);
	let group_hidden = category
		.and_then(|category| budget.category_groups.get(&category.group))
		.is_some_and(|group| group.hidden);
	CategoryLabels {
		hidden: category.is_some_and(|category| category.hidden) || group_hidden,
		archived: budget
			.category_meta
			.get(id)
			.is_some_and(|meta| meta.archived_on.is_some()),
		deleted: category.is_some_and(|category| category.tombstone),
		closed_goal: budget
			.goals
			.values()
			.any(|goal| &goal.category == id && goal.status == GoalStatus::Closed),
		needs_review: false,
	}
}

/// Where a category line sits in the budget screen's order: its group's
/// sort order and id, then its own sort order and id. `None` is the
/// uncategorized line. Looked up once per line, not once per comparison.
type OrderKey<'b> = Option<(f64, Option<&'b CategoryGroupId>, f64, CategoryId)>;

fn order_key<'b>(budget: &'b BudgetData, id: Option<&CategoryId>) -> OrderKey<'b> {
	let id = id?;
	let category = budget.categories.get(id);
	let group = category.and_then(|category| budget.category_groups.get(&category.group));
	Some((
		group.map_or(f64::MAX, |group| group.sort_order),
		group.map(|group| &group.id),
		category.map_or(f64::MAX, |category| category.sort_order),
		id.clone(),
	))
}

/// The budget screen's order: by group, then category, uncategorized last.
fn budget_order(a: &OrderKey, b: &OrderKey) -> Ordering {
	match (a, b) {
		(None, None) => Ordering::Equal,
		(None, Some(_)) => Ordering::Greater,
		(Some(_), None) => Ordering::Less,
		(Some(a), Some(b)) => {
			a.0.total_cmp(&b.0)
				.then_with(|| a.1.cmp(&b.1))
				.then_with(|| a.2.total_cmp(&b.2))
				.then_with(|| a.3.cmp(&b.3))
		}
	}
}
