use veritas_domain::{BudgetData, CategoryGroupId, CategoryId, CoreError, Money, TransactionId};

use crate::{CategoryLabels, CategoryLine, Classification, Period, net_profit};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum NodeKind {
	IncomeCategory(CategoryId),
	RefundsAndWithdrawals,
	UnreviewedTransfersIn,
	Shortfall,
	Income,
	/// `None` gathers uncategorized spending, which belongs to no group.
	CategoryGroup(Option<CategoryGroupId>),
	Category(Option<CategoryId>),
	Savings,
	DebtPaydown,
	IncomeReversals,
	UnreviewedTransfers,
	Unallocated,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
	pub kind: NodeKind,
	pub value: Money,
	pub labels: CategoryLabels,
	/// The primary-leg rows behind the node, by date then id. Shortfall and
	/// Unallocated are derived: Shortfall has none, and Unallocated lists the
	/// income rows it was computed from.
	pub rows: Vec<TransactionId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Flow {
	pub from: usize,
	pub to: usize,
	pub value: Money,
}

/// Income sources flow into one Income node, which flows out to spending by
/// category group, savings, debt paydown and whatever is left. `flows` index
/// into `nodes`. Zero-value nodes and flows are left out.
#[derive(Clone, Debug, PartialEq)]
pub struct Sankey {
	pub period: Period,
	pub nodes: Vec<Node>,
	pub flows: Vec<Flow>,
}

impl Sankey {
	pub fn node(&self, kind: &NodeKind) -> Option<&Node> {
		self.nodes.iter().find(|node| &node.kind == kind)
	}
}

pub fn sankey(
	budget: &BudgetData,
	classification: &Classification,
	period: Period,
) -> Result<Sankey, CoreError> {
	let report = net_profit(budget, classification, period)?;
	let mut refunds = Pending::default();
	let mut reversals = Pending::default();
	let mut income_rows = Vec::new();
	let mut sources = Vec::new();
	for line in &report.income.categories {
		income_rows.extend(line.rows.iter().cloned());
		if line.total.is_negative() {
			reversals.absorb(line.total.checked_neg()?, &line.rows)?;
		} else if let Some(category) = &line.category {
			sources.push(Node {
				kind: NodeKind::IncomeCategory(category.clone()),
				value: line.total,
				labels: line.labels,
				rows: line.rows.clone(),
			});
		}
	}

	let mut groups: Vec<(Option<CategoryGroupId>, Vec<&CategoryLine>)> = Vec::new();
	for line in &report.expenses.categories {
		if line.total.is_negative() {
			refunds.absorb(line.total.checked_neg()?, &line.rows)?;
			continue;
		}
		let group = line
			.category
			.as_ref()
			.and_then(|category| budget.categories.get(category))
			.map(|category| category.group.clone());
		match groups.iter_mut().find(|(id, _)| id == &group) {
			Some((_, lines)) => lines.push(line),
			None => groups.push((group, vec![line])),
		}
	}
	for section in [&report.savings, &report.debt_paydown] {
		if section.total.is_negative() {
			refunds.absorb(section.total.checked_neg()?, &section.rows)?;
		}
	}
	if refunds.value != Money::ZERO {
		sort_rows(classification, &mut refunds.rows);
		sources.push(refunds.into_node(NodeKind::RefundsAndWithdrawals));
	}
	if report.unreviewed.total.is_negative() {
		sources.push(Node {
			kind: NodeKind::UnreviewedTransfersIn,
			value: report.unreviewed.total.checked_neg()?,
			labels: CategoryLabels::default(),
			rows: report.unreviewed.rows.clone(),
		});
	}

	let mut outflows: Vec<(Node, Vec<Node>)> = Vec::new();
	for (group, lines) in groups {
		let children = lines
			.iter()
			.map(|line| Node {
				kind: NodeKind::Category(line.category.clone()),
				value: line.total,
				labels: line.labels,
				rows: line.rows.clone(),
			})
			.collect::<Vec<_>>();
		let mut rows: Vec<TransactionId> = children
			.iter()
			.flat_map(|child| child.rows.iter().cloned())
			.collect();
		sort_rows(classification, &mut rows);
		let group_node = Node {
			kind: NodeKind::CategoryGroup(group.clone()),
			value: Money::checked_sum(children.iter().map(|child| child.value))?,
			labels: group_labels(budget, group.as_ref()),
			rows,
		};
		outflows.push((group_node, children));
	}
	for (kind, section) in [
		(NodeKind::Savings, &report.savings),
		(NodeKind::DebtPaydown, &report.debt_paydown),
	] {
		if !section.total.is_negative() {
			outflows.push((plain(kind, section.total, &section.rows), Vec::new()));
		}
	}
	if reversals.value != Money::ZERO {
		sort_rows(classification, &mut reversals.rows);
		outflows.push((reversals.into_node(NodeKind::IncomeReversals), Vec::new()));
	}
	if !report.unreviewed.total.is_negative() {
		outflows.push((
			plain(
				NodeKind::UnreviewedTransfers,
				report.unreviewed.total,
				&report.unreviewed.rows,
			),
			Vec::new(),
		));
	}

	let spent = Money::checked_sum(outflows.iter().map(|(node, _)| node.value))?;
	let received = Money::checked_sum(sources.iter().map(|node| node.value))?;
	let shortfall = spent.checked_sub(received)?.max(Money::ZERO);
	if shortfall != Money::ZERO {
		sources.push(plain(NodeKind::Shortfall, shortfall, &[]));
	}
	let income_total = received.checked_add(shortfall)?;
	let unallocated = income_total.checked_sub(spent)?;
	sort_rows(classification, &mut income_rows);
	outflows.push((
		plain(NodeKind::Unallocated, unallocated, &income_rows),
		Vec::new(),
	));

	let mut graph = Graph::default();
	let mut middle_rows: Vec<TransactionId> = sources
		.iter()
		.flat_map(|node| node.rows.iter().cloned())
		.collect();
	sort_rows(classification, &mut middle_rows);
	let sources: Vec<_> = sources
		.into_iter()
		.map(|source| (source.value, graph.add(source)))
		.collect();
	let middle = graph.add(plain(NodeKind::Income, income_total, &middle_rows));
	for (value, source) in sources {
		graph.flow(source, middle, value);
	}
	for (node, children) in outflows {
		let value = node.value;
		let group = graph.add(node);
		graph.flow(middle, group, value);
		for child in children {
			let value = child.value;
			let child = graph.add(child);
			graph.flow(group, child, value);
		}
	}

	Ok(Sankey {
		period,
		nodes: graph.nodes,
		flows: graph.flows,
	})
}

fn plain(kind: NodeKind, value: Money, rows: &[TransactionId]) -> Node {
	Node {
		kind,
		value,
		labels: CategoryLabels::default(),
		rows: rows.to_vec(),
	}
}

fn group_labels(budget: &BudgetData, group: Option<&CategoryGroupId>) -> CategoryLabels {
	let group = group.and_then(|id| budget.category_groups.get(id));
	CategoryLabels {
		hidden: group.is_some_and(|group| group.hidden),
		deleted: group.is_some_and(|group| group.tombstone),
		..CategoryLabels::default()
	}
}

fn sort_rows(classification: &Classification, rows: &mut [TransactionId]) {
	rows.sort_by_key(|id| (classification.get(id).map(|row| row.date), id.clone()));
}

#[derive(Default)]
struct Pending {
	value: Money,
	rows: Vec<TransactionId>,
}

impl Pending {
	fn absorb(&mut self, value: Money, rows: &[TransactionId]) -> Result<(), CoreError> {
		self.value = self.value.checked_add(value)?;
		self.rows.extend(rows.iter().cloned());
		Ok(())
	}

	fn into_node(self, kind: NodeKind) -> Node {
		Node {
			kind,
			value: self.value,
			labels: CategoryLabels::default(),
			rows: self.rows,
		}
	}
}

#[derive(Default)]
struct Graph {
	nodes: Vec<Node>,
	flows: Vec<Flow>,
}

impl Graph {
	fn add(&mut self, node: Node) -> Option<usize> {
		if node.value == Money::ZERO {
			return None;
		}
		self.nodes.push(node);
		Some(self.nodes.len() - 1)
	}

	fn flow(&mut self, from: Option<usize>, to: Option<usize>, value: Money) {
		if let (Some(from), Some(to)) = (from, to)
			&& value != Money::ZERO
		{
			self.flows.push(Flow { from, to, value });
		}
	}
}
