//! The Sankey card (`SankeyCard.tsx`, `spreadsheets/sankey-spreadsheet.ts`):
//! categorized money flowing from payees and income categories through
//! accounts to category groups and categories, or, budgeted, through the
//! month's available funds to what was budgeted. Small nodes fold into
//! Other and hidden spacer nodes keep each layer in its column, exactly as
//! Actual builds the graph, so the node and link lists match Actual's.

use std::collections::{HashMap, HashSet};

use chrono::{Months, NaiveDate};
use veritas_domain::{
	BudgetData, BudgetType, CategoryId, CoreError, Money, Transaction, YearMonth,
};
use veritas_engine::EnvelopeSheet;
use veritas_rules::{Condition, ConditionValue};

use super::summary::{earliest_transaction, latest_transaction};
use super::{TimeRange, calculate_time_range, filtered_rows_between, meta_filter};
use crate::json::Json;
use crate::report_query::{parse_date, unsupported};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SankeyMode {
	/// Actual's `spent`: categorized transactions.
	Spent,
	Budgeted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SankeyLayer {
	IncomePayee,
	IncomeCategory,
	Account,
	Budget,
	CategoryGroup,
	Category,
}

const LAYERS: [SankeyLayer; 6] = [
	SankeyLayer::IncomePayee,
	SankeyLayer::IncomeCategory,
	SankeyLayer::Account,
	SankeyLayer::Budget,
	SankeyLayer::CategoryGroup,
	SankeyLayer::Category,
];

impl SankeyLayer {
	fn parse(value: Option<&Json>) -> Option<Self> {
		LAYERS
			.into_iter()
			.find(|layer| value.and_then(Json::as_str) == Some(layer.key()))
	}

	fn key(self) -> &'static str {
		match self {
			SankeyLayer::IncomePayee => "payee",
			SankeyLayer::IncomeCategory => "income_category",
			SankeyLayer::Account => "account",
			SankeyLayer::Budget => "budget",
			SankeyLayer::CategoryGroup => "category_group",
			SankeyLayer::Category => "category",
		}
	}

	fn index(self) -> usize {
		LAYERS
			.iter()
			.position(|layer| *layer == self)
			.unwrap_or_default()
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SankeySort {
	PerGroup,
	Global,
	BudgetOrder,
}

/// A node's words. Actual's English text for each is in `actual_english`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SankeyLabel {
	/// A payee, account, category group or category, by its own name.
	Name(String),
	Budgeted,
	AvailableFunds,
	/// Money budgeted back out of a category (a negative budget).
	FromCategory(String),
	ToBudget,
	Overbudgeted,
	/// Every account as one node, with Group accounts on.
	AllAccounts,
	FromMonth(YearMonth),
	ForMonth(YearMonth),
	Overspent,
	Other,
	/// A spacer that keeps a node in its layer's column; never drawn.
	Hidden,
}

impl SankeyLabel {
	/// The text Actual's card shows before translation. Only the oracle
	/// check reads it; the shell words labels under ST-10.10.
	pub fn actual_english(&self) -> String {
		match self {
			SankeyLabel::Name(name) => name.clone(),
			SankeyLabel::Budgeted => "Budgeted".to_owned(),
			SankeyLabel::AvailableFunds => "Available funds".to_owned(),
			SankeyLabel::FromCategory(category) => format!("From {category}"),
			SankeyLabel::ToBudget => "To budget".to_owned(),
			SankeyLabel::Overbudgeted => "Overbudgeted".to_owned(),
			SankeyLabel::AllAccounts => "Income".to_owned(),
			SankeyLabel::FromMonth(month) => format!("From {month}"),
			SankeyLabel::ForMonth(month) => format!("For {month}"),
			SankeyLabel::Overspent => "Overspent".to_owned(),
			SankeyLabel::Other => "Other".to_owned(),
			SankeyLabel::Hidden => String::new(),
		}
	}
}

/// Actual's node colors: a slot of the qualitative chart palette, or a
/// theme color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SankeyColor {
	/// `chartQual1` to `chartQual9`, as 0 to 8.
	Palette(u8),
	ToBudgetPositive,
	ToBudgetNegative,
	Gray,
	Blue,
}

const PALETTE_SIZE: u32 = 9;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SankeyNode {
	/// Actual's node key: an id, an id with a suffix, or a fixed name.
	pub key: String,
	pub label: SankeyLabel,
	/// The node's share of its column, as Actual writes it ("12.5%"); empty
	/// for hidden nodes.
	pub percentage: String,
	pub color: SankeyColor,
	/// The amount the graph prints under the node (`SankeyGraph.tsx`), which
	/// recharts computes, never Actual's spreadsheet: see `node_value`.
	pub value: Money,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SankeyLink {
	pub source: usize,
	/// `None` when the target was cut by the layer range; Actual keeps such a
	/// link with a target of -1, and draws nothing for it.
	pub target: Option<usize>,
	/// Links into or out of a hidden node carry -1 minor units.
	pub value: Money,
	/// The nodes folded into an Other node along this link, largest first.
	pub folded: Vec<(String, Money)>,
	pub color: Option<SankeyColor>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SankeyCard {
	/// The card's range as its subtitle shows it.
	pub range: TimeRange,
	/// The months the graph covers: the range clamped to the months that
	/// have transactions.
	pub first_month: YearMonth,
	pub last_month: YearMonth,
	pub mode: SankeyMode,
	pub show_percentages: bool,
	pub nodes: Vec<SankeyNode>,
	pub links: Vec<SankeyLink>,
}

/// `topNNodes`: how many nodes per column fit in a card this many points
/// tall.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn sankey_height_limit(card_height: f64) -> usize {
	let fit = (card_height / 35.0).floor();
	if fit.is_nan() || fit < 2.0 {
		2
	} else if fit >= 1e9 {
		1_000_000_000
	} else {
		fit as usize
	}
}

const OTHER: &str = "__OTHER_BUCKET";
const HIDDEN: &str = "__HIDDEN";
const NEGATIVE: &str = "__NEGATIVE";
const TO_BUDGET: &str = "to_budget";
const BUDGETED: &str = "budgeted";
const LAST_MONTH_OVERSPENT: &str = "last_month_overspent";
const FOR_NEXT_MONTH: &str = "for_next_month";
const FROM_PREVIOUS_MONTH: &str = "from_previous_month";
const AVAILABLE_INCOME: &str = "available_income";
const ALL_ACCOUNTS: &str = "all_income";
const STRUCTURAL: [&str; 10] = [
	TO_BUDGET,
	BUDGETED,
	LAST_MONTH_OVERSPENT,
	FOR_NEXT_MONTH,
	FROM_PREVIOUS_MONTH,
	AVAILABLE_INCOME,
	ALL_ACCOUNTS,
	OTHER,
	HIDDEN,
	NEGATIVE,
];

fn is_hidden(key: &str) -> bool {
	key.ends_with(HIDDEN)
}

#[derive(Clone, Debug)]
struct Node {
	to: Vec<(String, i64)>,
	layer: SankeyLayer,
	name: Option<String>,
	label: Option<SankeyLabel>,
	is_negative: bool,
	percentage: Option<String>,
	color: Option<SankeyColor>,
}

impl Node {
	fn new(layer: SankeyLayer, name: Option<String>, label: Option<SankeyLabel>) -> Self {
		Node {
			to: Vec::new(),
			layer,
			name,
			label,
			is_negative: false,
			percentage: None,
			color: None,
		}
	}

	fn link(&self, to: &str) -> Option<i64> {
		self.to
			.iter()
			.find(|(target, _)| target == to)
			.map(|(_, value)| *value)
	}

	fn add_link(&mut self, to: &str, value: i64) {
		match self.to.iter_mut().find(|(target, _)| target == to) {
			Some((_, existing)) => *existing = existing.saturating_add(value),
			None => self.to.push((to.to_owned(), value)),
		}
	}

	fn remove_link(&mut self, to: &str) {
		self.to.retain(|(target, _)| target != to);
	}
}

/// A JS `Map` of nodes: insertion-ordered, and a deleted key added again
/// goes to the end.
#[derive(Clone, Debug, Default)]
struct Graph {
	slots: Vec<Option<(String, Node)>>,
	index: HashMap<String, usize>,
}

impl Graph {
	fn from_entries(entries: Vec<(String, Node)>) -> Self {
		let mut graph = Graph::default();
		for (key, node) in entries {
			graph.add(&key, node);
		}
		graph
	}

	fn get(&self, key: &str) -> Option<&Node> {
		let slot = *self.index.get(key)?;
		self.slots
			.get(slot)
			.and_then(Option::as_ref)
			.map(|(_, node)| node)
	}

	fn get_mut(&mut self, key: &str) -> Option<&mut Node> {
		let slot = *self.index.get(key)?;
		self.slots
			.get_mut(slot)
			.and_then(Option::as_mut)
			.map(|(_, node)| node)
	}

	fn contains(&self, key: &str) -> bool {
		self.index.contains_key(key)
	}

	fn add(&mut self, key: &str, node: Node) {
		if !self.contains(key) {
			self.index.insert(key.to_owned(), self.slots.len());
			self.slots.push(Some((key.to_owned(), node)));
		}
	}

	fn add_node(&mut self, key: &str, layer: SankeyLayer, name: &str) {
		self.add(key, Node::new(layer, Some(name.to_owned()), None));
	}

	fn add_labeled(
		&mut self,
		key: &str,
		layer: SankeyLayer,
		label: SankeyLabel,
		is_negative: bool,
	) {
		let mut node = Node::new(layer, None, Some(label));
		node.is_negative = is_negative;
		self.add(key, node);
	}

	fn add_value(&mut self, from: &str, to: &str, value: i64) {
		if let Some(node) = self.get_mut(from) {
			node.add_link(to, value);
		}
	}

	fn remove(&mut self, key: &str) {
		if let Some(slot) = self.index.remove(key)
			&& let Some(entry) = self.slots.get_mut(slot)
		{
			*entry = None;
		}
	}

	fn iter(&self) -> impl Iterator<Item = (&str, &Node)> {
		self.slots
			.iter()
			.flatten()
			.map(|(key, node)| (key.as_str(), node))
	}

	fn iter_mut(&mut self) -> impl Iterator<Item = (&str, &mut Node)> {
		self.slots
			.iter_mut()
			.flatten()
			.map(|(key, node)| (key.as_str(), &mut *node))
	}

	fn keys(&self) -> Vec<String> {
		self.iter().map(|(key, _)| key.to_owned()).collect()
	}

	fn keys_in(&self, layer: SankeyLayer) -> Vec<String> {
		self.iter()
			.filter(|(_, node)| node.layer == layer)
			.map(|(key, _)| key.to_owned())
			.collect()
	}

	fn has_parent(&self, key: &str) -> bool {
		self.iter().any(|(_, node)| node.link(key).is_some())
	}
}

/// `getLayersByStronglyConnectedComponents`: each node's column, the
/// longest path to it from a node without parents, a cycle counting as one
/// node.
fn layers(graph: &Graph) -> HashMap<String, usize> {
	let keys = graph.keys();
	let position: HashMap<&str, usize> = keys
		.iter()
		.enumerate()
		.map(|(index, key)| (key.as_str(), index))
		.collect();
	let edges: Vec<Vec<usize>> = keys
		.iter()
		.map(|key| {
			graph.get(key).map_or_else(Vec::new, |node| {
				node.to
					.iter()
					.filter_map(|(target, _)| position.get(target.as_str()).copied())
					.collect()
			})
		})
		.collect();
	let component = tarjan(&edges);
	let count = component.iter().copied().max().map_or(0, |max| max + 1);
	let mut parents: Vec<HashSet<usize>> = vec![HashSet::new(); count];
	for (source, targets) in edges.iter().enumerate() {
		for target in targets {
			if component[source] != component[*target] {
				parents[component[*target]].insert(component[source]);
			}
		}
	}
	let mut layer_of: Vec<Option<usize>> = vec![None; count];
	fn resolve(
		component: usize,
		parents: &[HashSet<usize>],
		layer_of: &mut [Option<usize>],
	) -> usize {
		if let Some(layer) = layer_of[component] {
			return layer;
		}
		let layer = parents[component]
			.iter()
			.map(|parent| resolve(*parent, parents, layer_of) + 1)
			.max()
			.unwrap_or(0);
		layer_of[component] = Some(layer);
		layer
	}
	keys.iter()
		.enumerate()
		.map(|(index, key)| {
			(
				key.clone(),
				resolve(component[index], &parents, &mut layer_of),
			)
		})
		.collect()
}

fn tarjan(edges: &[Vec<usize>]) -> Vec<usize> {
	struct State<'a> {
		edges: &'a [Vec<usize>],
		index: Vec<Option<usize>>,
		low: Vec<usize>,
		on_stack: Vec<bool>,
		stack: Vec<usize>,
		component: Vec<usize>,
		next: usize,
		components: usize,
	}
	fn connect(state: &mut State<'_>, node: usize) {
		state.index[node] = Some(state.next);
		state.low[node] = state.next;
		state.next += 1;
		state.stack.push(node);
		state.on_stack[node] = true;
		for &child in &state.edges[node] {
			match state.index[child] {
				None => {
					connect(state, child);
					state.low[node] = state.low[node].min(state.low[child]);
				}
				Some(index) if state.on_stack[child] => {
					state.low[node] = state.low[node].min(index);
				}
				Some(_) => {}
			}
		}
		if Some(state.low[node]) == state.index[node] {
			while let Some(popped) = state.stack.pop() {
				state.on_stack[popped] = false;
				state.component[popped] = state.components;
				if popped == node {
					break;
				}
			}
			state.components += 1;
		}
	}
	let size = edges.len();
	let mut state = State {
		edges,
		index: vec![None; size],
		low: vec![0; size],
		on_stack: vec![false; size],
		stack: Vec::new(),
		component: vec![0; size],
		next: 0,
		components: 0,
	};
	for node in 0..size {
		if state.index[node].is_none() {
			connect(&mut state, node);
		}
	}
	state.component
}

/// `getNodeValue` for every node: a first-column node's outgoing total,
/// any other node's incoming total, hidden links aside.
fn node_values(graph: &Graph, layers: &HashMap<String, usize>) -> HashMap<String, i64> {
	let mut incoming: HashMap<&str, i64> = HashMap::new();
	for (key, node) in graph.iter() {
		if is_hidden(key) {
			continue;
		}
		for (target, value) in &node.to {
			let sum = incoming.entry(target.as_str()).or_insert(0);
			*sum = sum.saturating_add(*value);
		}
	}
	let outgoing = |node: &Node| {
		node.to
			.iter()
			.filter(|(target, _)| !is_hidden(target))
			.fold(0_i64, |sum, (_, value)| sum.saturating_add(*value))
	};
	graph
		.iter()
		.map(|(key, node)| {
			let value = if layers.get(key).copied().unwrap_or(0) == 0 {
				outgoing(node)
			} else {
				let value = incoming.get(key).copied().unwrap_or(0);
				if value < 0 { outgoing(node) } else { value }
			};
			(key.to_owned(), value)
		})
		.collect()
}

#[derive(Clone, Debug)]
struct Entry {
	group: String,
	group_id: String,
	category: String,
	category_id: String,
	value: i64,
	is_income: bool,
	is_negative: bool,
	account_name: String,
	account_id: String,
	payee_name: String,
	payee_id: String,
}

struct CategoryView {
	key: CategoryId,
	id: String,
	/// The name as the budget API gives it.
	name: String,
	/// The name as the card's category list gives it, with Starting Balances
	/// translated.
	listed: String,
}

struct GroupView {
	id: String,
	name: String,
	is_income: bool,
	categories: Vec<CategoryView>,
}

/// `q('category_groups')` with its categories, in Actual's order.
fn grouped_categories(budget: &BudgetData) -> Vec<GroupView> {
	let mut groups: Vec<_> = budget
		.category_groups
		.values()
		.filter(|group| !group.tombstone)
		.collect();
	groups.sort_by(|a, b| {
		a.is_income
			.cmp(&b.is_income)
			.then_with(|| a.sort_order.total_cmp(&b.sort_order))
			.then_with(|| a.id.cmp(&b.id))
	});
	groups
		.into_iter()
		.map(|group| {
			let mut categories: Vec<_> = budget
				.categories
				.values()
				.filter(|category| !category.tombstone && category.group == group.id)
				.collect();
			categories.sort_by(|a, b| {
				a.sort_order
					.total_cmp(&b.sort_order)
					.then_with(|| a.id.cmp(&b.id))
			});
			GroupView {
				id: group.id.to_string(),
				name: group.name.clone(),
				is_income: group.is_income,
				categories: categories
					.into_iter()
					.map(|category| CategoryView {
						key: category.id.clone(),
						id: category.id.to_string(),
						name: category.name.clone(),
						listed: if category.name.to_lowercase() == "starting balances" {
							"Starting Balances".to_owned()
						} else {
							category.name.clone()
						},
					})
					.collect(),
			}
		})
		.collect()
}

/// `ORDER BY date DESC, starting_balance_flag, sort_order DESC, id`.
fn actual_order(a: &Transaction, b: &Transaction) -> std::cmp::Ordering {
	b.date
		.cmp(&a.date)
		.then_with(|| a.starting_balance.cmp(&b.starting_balance))
		.then_with(|| b.sort_order.total_cmp(&a.sort_order))
		.then_with(|| a.id.cmp(&b.id))
}

/// The row SQLite reads a grouped query's other columns from: the group's
/// first row in the `trans_date` index scan, which is the earliest, ties in
/// insertion order (approximated by `sort_order`, then id).
fn earlier_row<'a>(a: &'a Transaction, b: &'a Transaction) -> &'a Transaction {
	let order = a
		.date
		.cmp(&b.date)
		.then_with(|| a.sort_order.total_cmp(&b.sort_order))
		.then_with(|| a.id.cmp(&b.id));
	if order.is_gt() { b } else { a }
}

fn live_account(budget: &BudgetData, row: &Transaction) -> (String, String) {
	budget
		.accounts
		.get(&row.account)
		.filter(|account| !account.tombstone)
		.map_or_else(Default::default, |account| {
			(account.id.to_string(), account.name.clone())
		})
}

fn live_payee(budget: &BudgetData, row: &Transaction) -> (String, String) {
	row.payee
		.as_ref()
		.and_then(|id| budget.resolve_payee(id))
		.and_then(|id| budget.payees.get(id))
		.filter(|payee| !payee.tombstone)
		.map_or_else(Default::default, |payee| {
			(payee.id.to_string(), payee.name.clone())
		})
}

/// `fetchCategoryData`: per category, the filtered rows summed by account
/// (and by payee for income), in the order Actual's grouped query returns
/// them.
fn spent_entries(
	budget: &BudgetData,
	groups: &[GroupView],
	rows: &[&Transaction],
	group_accounts: bool,
) -> Result<Vec<Entry>, CoreError> {
	let mut by_category: HashMap<String, Vec<&Transaction>> = HashMap::new();
	for row in rows {
		if let Some(category) = row
			.category
			.as_ref()
			.and_then(|id| budget.resolve_category(id))
		{
			by_category
				.entry(category.to_string())
				.or_default()
				.push(row);
		}
	}
	let mut entries = Vec::new();
	for group in groups {
		for category in &group.categories {
			let Some(rows) = by_category.get(&category.id) else {
				continue;
			};
			let mut sums: Vec<(String, Option<String>, Money, &Transaction)> = Vec::new();
			for row in rows {
				let account = row.account.to_string();
				let payee = if group.is_income {
					row.payee
						.as_ref()
						.and_then(|id| budget.resolve_payee(id))
						.map(ToString::to_string)
				} else {
					None
				};
				match sums
					.iter_mut()
					.find(|(a, p, _, _)| *a == account && *p == payee)
				{
					Some((_, _, sum, last)) => {
						*sum = sum.checked_add(row.amount)?;
						*last = earlier_row(last, row);
					}
					None => sums.push((account, payee, row.amount, row)),
				}
			}
			sums.sort_by(|a, b| actual_order(a.3, b.3));
			for (_, _, sum, last) in sums {
				let (account_id, account_name) = live_account(budget, last);
				let (payee_id, payee_name) = live_payee(budget, last);
				let amount = sum.minor();
				entries.push(Entry {
					group: group.name.clone(),
					group_id: group.id.clone(),
					category: category.listed.clone(),
					category_id: category.id.clone(),
					value: amount.saturating_abs(),
					is_income: group.is_income,
					is_negative: amount < 0,
					account_name,
					account_id,
					payee_name,
					payee_id,
				});
			}
		}
	}
	if group_accounts {
		for entry in &mut entries {
			if !entry.account_name.is_empty() && !entry.account_id.is_empty() {
				ALL_ACCOUNTS.clone_into(&mut entry.account_name);
				ALL_ACCOUNTS.clone_into(&mut entry.account_id);
			}
		}
	}
	Ok(entries)
}

struct TransferPair {
	from_id: String,
	from_name: String,
	to_id: String,
	to_name: String,
	amount: i64,
}

/// `fetchTransferData` and `aggregateTransferPairs`: net transfers between
/// two on-budget accounts, when both legs pass the card's filter.
fn transfer_pairs(budget: &BudgetData, rows: &[&Transaction]) -> Vec<TransferPair> {
	let mut legs: Vec<&Transaction> = rows
		.iter()
		.copied()
		.filter(|row| row.transfer.is_some())
		.filter(|row| {
			budget
				.accounts
				.get(&row.account)
				.filter(|account| !account.tombstone)
				.is_some_and(|account| !account.off_budget)
		})
		.collect();
	legs.sort_by(|a, b| actual_order(a, b));
	let named: Vec<(String, String, i64, String, String)> = legs
		.iter()
		.map(|row| {
			let (account_id, account_name) = live_account(budget, row);
			(
				row.id.to_string(),
				account_id,
				row.amount.minor(),
				account_name,
				row.transfer
					.as_ref()
					.map(ToString::to_string)
					.unwrap_or_default(),
			)
		})
		.collect();
	let by_id: HashMap<&str, usize> = named
		.iter()
		.enumerate()
		.map(|(index, leg)| (leg.0.as_str(), index))
		.collect();
	let account_names: HashMap<&str, &str> = named
		.iter()
		.map(|leg| (leg.1.as_str(), leg.3.as_str()))
		.collect();
	let mut pairs: Vec<(String, String, i64)> = Vec::new();
	for from in &named {
		let Some(to) = by_id.get(from.4.as_str()).map(|index| &named[*index]) else {
			continue;
		};
		if from.1 > to.1 {
			continue;
		}
		let (first, second) = (&from.1, &to.1);
		let source = if from.2 < 0 { &from.1 } else { &to.1 };
		let signed = if source == first {
			from.2.saturating_abs()
		} else {
			-from.2.saturating_abs()
		};
		match pairs.iter_mut().find(|(a, b, _)| a == first && b == second) {
			Some((_, _, sum)) => *sum = sum.saturating_add(signed),
			None => pairs.push((first.clone(), second.clone(), signed)),
		}
	}
	pairs
		.into_iter()
		.filter(|(_, _, value)| *value != 0)
		.map(|(first, second, value)| {
			let (from_id, to_id) = if value > 0 {
				(first, second)
			} else {
				(second, first)
			};
			TransferPair {
				from_name: account_names
					.get(from_id.as_str())
					.map(|name| (*name).to_owned())
					.unwrap_or_default(),
				to_name: account_names
					.get(to_id.as_str())
					.map(|name| (*name).to_owned())
					.unwrap_or_default(),
				from_id,
				to_id,
				amount: value.saturating_abs(),
			}
		})
		.collect()
}

fn add_account(graph: &mut Graph, id: &str, name: &str) {
	if id == ALL_ACCOUNTS {
		graph.add_labeled(id, SankeyLayer::Account, SankeyLabel::AllAccounts, false);
	} else {
		graph.add_node(id, SankeyLayer::Account, name);
	}
}

/// `createTransactionsGraph`. A category with more money in than out is
/// named by its category, where Actual names it by the payee of the
/// account's latest row (ST-10.10).
fn transactions_graph(
	entries: &[Entry],
	transfers: &[TransferPair],
	group_accounts: bool,
) -> Graph {
	let mut graph = Graph::default();
	for entry in entries {
		if entry.account_id.is_empty()
			|| entry.account_name.is_empty()
			|| entry.category_id.is_empty()
		{
			continue;
		}
		let negative_key = format!("{}{NEGATIVE}", entry.category_id);
		match (entry.is_income, entry.is_negative) {
			(true, true) => {
				add_account(&mut graph, &entry.account_id, &entry.account_name);
				let name = if entry.payee_name.is_empty() {
					&entry.category
				} else {
					&entry.payee_name
				};
				graph.add_labeled(
					&negative_key,
					SankeyLayer::CategoryGroup,
					SankeyLabel::Name(name.clone()),
					true,
				);
				graph.add_value(&entry.account_id, &negative_key, entry.value);
			}
			(true, false) => {
				graph.add_node(
					&entry.category_id,
					SankeyLayer::IncomeCategory,
					&entry.category,
				);
				add_account(&mut graph, &entry.account_id, &entry.account_name);
				graph.add_value(&entry.category_id, &entry.account_id, entry.value);
				if !entry.payee_id.is_empty() {
					graph.add_node(&entry.payee_id, SankeyLayer::IncomePayee, &entry.payee_name);
					graph.add_value(&entry.payee_id, &entry.category_id, entry.value);
				}
			}
			(false, true) => {
				add_account(&mut graph, &entry.account_id, &entry.account_name);
				graph.add_node(&entry.group_id, SankeyLayer::CategoryGroup, &entry.group);
				graph.add_node(&entry.category_id, SankeyLayer::Category, &entry.category);
				graph.add_value(&entry.account_id, &entry.group_id, entry.value);
				graph.add_value(&entry.group_id, &entry.category_id, entry.value);
			}
			(false, false) => {
				graph.add_node(&negative_key, SankeyLayer::IncomeCategory, &entry.category);
				add_account(&mut graph, &entry.account_id, &entry.account_name);
				graph.add_value(&negative_key, &entry.account_id, entry.value);
			}
		}
	}
	if !group_accounts {
		for pair in transfers {
			add_account(&mut graph, &pair.from_id, &pair.from_name);
			add_account(&mut graph, &pair.to_id, &pair.to_name);
			graph.add_value(&pair.from_id, &pair.to_id, pair.amount);
		}
	}
	graph
}

struct Aggregated {
	to_budget: i64,
	from_previous_month: i64,
	last_month_overspent: i64,
	for_next_month: i64,
	first_month: YearMonth,
	last_month: YearMonth,
}

/// `createBudgetGraph`.
fn budget_graph(entries: &[Entry], aggregated: &Aggregated) -> Result<Graph, CoreError> {
	let mut graph = Graph::default();
	graph.add(
		BUDGETED,
		Node::new(
			SankeyLayer::Budget,
			Some("Budgeted".to_owned()),
			Some(SankeyLabel::Budgeted),
		),
	);
	graph.add(
		AVAILABLE_INCOME,
		Node::new(
			SankeyLayer::Account,
			Some("Available funds".to_owned()),
			Some(SankeyLabel::AvailableFunds),
		),
	);
	for entry in entries {
		if entry.is_income {
			graph.add_node(
				&entry.category_id,
				SankeyLayer::IncomeCategory,
				&entry.category,
			);
			graph.add_value(&entry.category_id, AVAILABLE_INCOME, entry.value);
		} else if entry.value >= 0 {
			graph.add_node(&entry.group_id, SankeyLayer::CategoryGroup, &entry.group);
			graph.add_node(&entry.category_id, SankeyLayer::Category, &entry.category);
			graph.add_value(&entry.group_id, &entry.category_id, entry.value);
			graph.add_value(BUDGETED, &entry.group_id, entry.value);
			graph.add_value(AVAILABLE_INCOME, BUDGETED, entry.value);
		} else {
			graph.add_labeled(
				&entry.category_id,
				SankeyLayer::Account,
				SankeyLabel::FromCategory(entry.category.clone()),
				true,
			);
			graph.add_value(&entry.category_id, BUDGETED, entry.value.saturating_abs());
			graph.add_value(AVAILABLE_INCOME, BUDGETED, entry.value);
		}
	}
	if aggregated.to_budget > 0 {
		graph.add_labeled(
			TO_BUDGET,
			SankeyLayer::CategoryGroup,
			SankeyLabel::ToBudget,
			false,
		);
		graph.add_value(AVAILABLE_INCOME, TO_BUDGET, aggregated.to_budget);
	} else {
		graph.add_labeled(
			TO_BUDGET,
			SankeyLayer::Account,
			SankeyLabel::Overbudgeted,
			true,
		);
		graph.add_value(TO_BUDGET, BUDGETED, aggregated.to_budget.saturating_abs());
		graph.add_value(
			AVAILABLE_INCOME,
			BUDGETED,
			-aggregated.to_budget.saturating_abs(),
		);
	}
	graph.add_labeled(
		FROM_PREVIOUS_MONTH,
		SankeyLayer::IncomeCategory,
		SankeyLabel::FromMonth(aggregated.first_month.offset(-1)?),
		false,
	);
	graph.add_value(
		FROM_PREVIOUS_MONTH,
		AVAILABLE_INCOME,
		aggregated.from_previous_month,
	);
	graph.add_labeled(
		FOR_NEXT_MONTH,
		SankeyLayer::Budget,
		SankeyLabel::ForMonth(aggregated.last_month.offset(1)?),
		false,
	);
	graph.add_value(AVAILABLE_INCOME, FOR_NEXT_MONTH, aggregated.for_next_month);
	graph.add(
		LAST_MONTH_OVERSPENT,
		Node::new(
			SankeyLayer::Budget,
			Some("Overspent".to_owned()),
			Some(SankeyLabel::Overspent),
		),
	);
	graph.add_value(
		AVAILABLE_INCOME,
		LAST_MONTH_OVERSPENT,
		aggregated.last_month_overspent.saturating_abs(),
	);
	Ok(graph)
}

enum NameTest {
	Is(Option<String>),
	IsNot(Option<String>),
	OneOf(Option<Vec<Option<String>>>),
	NotOneOf(Option<Vec<Option<String>>>),
	Contains(Option<String>, bool),
	Matches(Option<ConditionValue>),
	Never,
	BadOp,
}

struct CategoryTest {
	group: bool,
	test: NameTest,
}

/// `filterCategoryGroups`' `matchesStringCondition`, read ahead of time.
fn category_test(condition: &Json) -> Result<CategoryTest, CoreError> {
	let group = condition.get("field").and_then(Json::as_str) == Some("category_group");
	let value = condition.get("value").unwrap_or(&Json::Null);
	let text = |value: &Json| value.as_str().map(str::to_owned);
	let list = || {
		value
			.as_array()
			.map(|values| values.iter().map(text).collect())
	};
	let Some(op) = condition.get("op").and_then(Json::as_str) else {
		return Ok(CategoryTest {
			group,
			test: NameTest::BadOp,
		});
	};
	let test = match op {
		"is" => NameTest::Is(text(value)),
		"isNot" => NameTest::IsNot(text(value)),
		"oneOf" => NameTest::OneOf(list()),
		"notOneOf" => NameTest::NotOneOf(list()),
		"contains" => NameTest::Contains(text(value), true),
		"doesNotContain" => NameTest::Contains(text(value), false),
		"matches" => match value.as_str() {
			Some(source) if source.encode_utf16().count() <= 256 => {
				let pattern = match source.rfind('/') {
					Some(last) if source.starts_with('/') && last > 0 => &source[1..last],
					_ => source,
				};
				// `new RegExp(pattern, 'i')`. Lowercasing both sides gives the
				// same answer only while the pattern has no escapes.
				if pattern.contains('\\') {
					return Err(unsupported(
						"a case-insensitive regex with escapes in a Sankey filter".to_owned(),
					));
				}
				let parsed = Condition::parse(&serde_json::json!({
					"field": "category", "op": "matches", "value": pattern.to_lowercase()
				}))
				.map_err(|_| unsupported("a Sankey filter regex".to_owned()))?;
				NameTest::Matches(Some(parsed.value))
			}
			_ => NameTest::Matches(None),
		},
		_ => NameTest::Never,
	};
	Ok(CategoryTest { group, test })
}

fn passes(test: &NameTest, id: &str, name: &str) -> Result<bool, CoreError> {
	let is = |value: &Option<String>| value.as_deref() == Some(id);
	Ok(match test {
		NameTest::Is(value) => is(value),
		NameTest::IsNot(value) => !is(value),
		NameTest::OneOf(values) => values.as_ref().is_some_and(|values| values.iter().any(is)),
		NameTest::NotOneOf(values) => values.as_ref().is_none_or(|values| !values.iter().any(is)),
		NameTest::Contains(needle, wanted) => needle
			.as_ref()
			.is_some_and(|needle| name.to_lowercase().contains(&needle.to_lowercase()) == *wanted),
		NameTest::Matches(Some(ConditionValue::Pattern(pattern))) => pattern
			.test(&name.to_lowercase())
			.map_err(|_| unsupported("a Sankey filter regex".to_owned()))?,
		NameTest::Matches(_) | NameTest::Never => false,
		NameTest::BadOp => return Err(CoreError::Invalid("Invalid op".to_owned())),
	})
}

/// `filterCategoryGroups`: budgeted mode reads every category from the
/// budget, so only its category and category-group conditions narrow it.
fn keep_category(
	tests: &[CategoryTest],
	any: bool,
	group: &GroupView,
	category: &CategoryView,
) -> Result<bool, CoreError> {
	let check = |test: &CategoryTest| {
		if test.group {
			passes(&test.test, &group.id, &group.name)
		} else {
			passes(&test.test, &category.id, &category.name)
		}
	};
	let (groups, categories): (Vec<&CategoryTest>, Vec<&CategoryTest>) =
		tests.iter().partition(|test| test.group);
	if any {
		for test in categories.iter().chain(&groups) {
			if check(test)? {
				return Ok(true);
			}
		}
		return Ok(false);
	}
	// Both lists are checked even when the first fails, as Actual does.
	let all = |tests: &[&CategoryTest]| -> Result<bool, CoreError> {
		for test in tests {
			if !check(test)? {
				return Ok(false);
			}
		}
		Ok(true)
	};
	let categories_pass = all(&categories)?;
	let groups_pass = all(&groups)?;
	Ok(categories_pass && groups_pass)
}

/// `createBudgetSpreadsheet` over the envelope sheet. A tracking budget has
/// no To Budget, carryover or overspending cells, which Actual reads as 0.
fn budgeted_entries(
	budget: &BudgetData,
	sheet: &EnvelopeSheet,
	groups: &[GroupView],
	meta: Option<&Json>,
	first: YearMonth,
	last: YearMonth,
) -> Result<(Vec<Entry>, Aggregated), CoreError> {
	let budget_bound = sheet.months().last().copied();
	let check = |month: YearMonth| {
		if sheet.contains(month) && Some(month) != budget_bound {
			Ok(())
		} else {
			// Actual throws "No budget exists for month: {month}".
			Err(CoreError::NotFound {
				entity: "budget month",
				id: month.to_string(),
			})
		}
	};
	let mut months = Vec::new();
	let mut month = first;
	while month <= last {
		check(month)?;
		months.push(month);
		month = month.offset(1)?;
	}
	let next = last.offset(1)?;
	check(next)?;
	let tracking = budget.budget_type == BudgetType::Tracking;
	let summary = |month: YearMonth| {
		if tracking { None } else { sheet.summary(month) }
	};
	let to_budget = summary(last).map_or(0, |summary| summary.to_budget.minor());
	let from_previous_month = summary(first).map_or(0, |summary| summary.from_last_month.minor());
	let mut last_month_overspent = 0_i64;
	for month in &months {
		last_month_overspent = last_month_overspent.saturating_add(
			summary(*month).map_or(0, |summary| summary.last_month_overspent.minor()),
		);
	}
	let next_from_last = summary(next).map_or(0, |summary| summary.from_last_month.minor());

	let tests: Vec<CategoryTest> = meta
		.and_then(|meta| meta.get("conditions"))
		.and_then(Json::as_array)
		.unwrap_or_default()
		.iter()
		.filter(|condition| {
			matches!(
				condition.get("field").and_then(Json::as_str),
				Some("category" | "category_group")
			)
		})
		.map(category_test)
		.collect::<Result<_, _>>()?;
	let any = meta
		.and_then(|meta| meta.get("conditionsOp"))
		.and_then(Json::as_str)
		== Some("or");

	let mut entries = Vec::new();
	for group in groups {
		for category in &group.categories {
			if !tests.is_empty() && !keep_category(&tests, any, group, category)? {
				continue;
			}
			let mut value = 0_i64;
			for month in &months {
				let cell = sheet.category(*month, &category.key);
				let amount = if group.is_income {
					cell.map_or(0, |cell| cell.spent.minor())
				} else {
					cell.map_or(0, |cell| cell.budgeted.minor())
				};
				value = value.saturating_add(amount);
			}
			if value == 0 {
				continue;
			}
			entries.push(Entry {
				group: group.name.clone(),
				group_id: group.id.clone(),
				category: category.name.clone(),
				category_id: category.id.clone(),
				value,
				is_income: group.is_income,
				is_negative: value < 0,
				account_name: String::new(),
				account_id: String::new(),
				payee_name: String::new(),
				payee_id: String::new(),
			});
		}
	}
	Ok((
		entries,
		Aggregated {
			to_budget,
			from_previous_month,
			last_month_overspent,
			for_next_month: next_from_last.saturating_sub(to_budget),
			first_month: first,
			last_month: last,
		},
	))
}

type Folded = HashMap<(String, String), Vec<(String, i64)>>;

fn note_folded(folded: &mut Folded, from: &str, to: &str, name: &str, value: i64) {
	let list = folded.entry((from.to_owned(), to.to_owned())).or_default();
	if !list.iter().any(|(existing, _)| existing == name) {
		list.push((name.to_owned(), value));
	}
}

/// `moveToOther`.
fn move_to_other(graph: &mut Graph, key: &str, folded: &mut Folded, global: bool) {
	let Some(node) = graph.get(key).cloned() else {
		return;
	};
	let to_keys: Vec<String> = node.to.iter().map(|(target, _)| target.clone()).collect();
	let from_keys: Vec<String> = graph
		.iter()
		.filter(|(_, data)| data.link(key).is_some())
		.map(|(from, _)| from.to_owned())
		.collect();
	let mut other = format!("{}{OTHER}", node.layer.key());
	if !global {
		if node.layer == SankeyLayer::Category {
			if let Some((group, _)) = graph.iter().find(|(_, data)| {
				data.link(key).is_some() && data.layer == SankeyLayer::CategoryGroup
			}) {
				other = format!("{group}{OTHER}");
			}
		} else if node.layer == SankeyLayer::IncomePayee
			&& let Some(category) = to_keys.iter().find(|target| {
				graph
					.get(target)
					.is_some_and(|data| data.layer == SankeyLayer::IncomeCategory)
			}) {
			other = format!("{category}{OTHER}");
		}
	}
	graph.add(
		&other,
		Node::new(
			node.layer,
			Some("Other".to_owned()),
			Some(SankeyLabel::Other),
		),
	);
	let name = node.name.clone().unwrap_or_else(|| key.to_owned());
	for from in &from_keys {
		let value = graph.get(from).and_then(|data| data.link(key));
		if let Some(value) = value
			&& *from != other
		{
			graph.add_value(from, &other, value);
			note_folded(folded, from, &other, &name, value);
		}
	}
	for to in &to_keys {
		let value = graph.get(key).and_then(|data| data.link(to));
		if let Some(value) = value
			&& other != *to
		{
			graph.add_value(&other, to, value);
			note_folded(folded, &other, to, &name, value);
		}
	}
	for from in &from_keys {
		if let Some(data) = graph.get_mut(from) {
			data.remove_link(key);
		}
	}
	graph.remove(key);
}

/// `groupOtherCategories`: per column, the smallest nodes fold into Other
/// until the column has `top_n` nodes.
fn group_other(graph: &mut Graph, top_n: f64, global: bool) -> Folded {
	let mut folded = Folded::new();
	for layer in LAYERS {
		loop {
			let in_layer = graph.keys_in(layer);
			let ordinary: Vec<&String> = in_layer
				.iter()
				.filter(|key| {
					!key.ends_with(OTHER)
						&& !STRUCTURAL.contains(&key.as_str())
						&& graph.get(key).is_some_and(|node| !node.is_negative)
				})
				.collect();
			let others = in_layer.iter().filter(|key| key.ends_with(OTHER)).count();
			#[allow(clippy::cast_precision_loss)]
			let count = (ordinary.len() + others) as f64;
			if count <= top_n || top_n.is_nan() {
				break;
			}
			let values = node_values(graph, &layers(graph));
			let mut smallest: Option<(&String, i64)> = None;
			for key in &ordinary {
				let value = values.get(key.as_str()).copied().unwrap_or(0);
				if smallest.is_none_or(|(_, min)| value < min) {
					smallest = Some((key, value));
				}
			}
			let Some((key, _)) = smallest else {
				break;
			};
			let key = key.clone();
			move_to_other(graph, &key, &mut folded, global);
		}
	}
	folded
}

fn move_to_end(entries: &mut Vec<String>, key: &str) {
	if let Some(index) = entries.iter().position(|entry| entry == key) {
		let entry = entries.remove(index);
		entries.push(entry);
	}
}

fn move_to_start(entries: &mut Vec<String>, key: &str) {
	if let Some(index) = entries.iter().position(|entry| entry == key) {
		let entry = entries.remove(index);
		entries.insert(0, entry);
	}
}

/// `sortRelatedNodesAroundAnchors`.
fn place_around_anchors(
	graph: &Graph,
	values: &HashMap<String, i64>,
	entries: &mut Vec<String>,
	anchor_layer: SankeyLayer,
	related_layer: SankeyLayer,
	after: bool,
) {
	let value = |key: &String| values.get(key).copied().unwrap_or(0);
	for anchor_key in graph.keys_in(anchor_layer) {
		let Some(anchor) = graph.get(&anchor_key) else {
			continue;
		};
		let related: Vec<String> = if after {
			anchor
				.to
				.iter()
				.map(|(target, _)| target.clone())
				.filter(|target| {
					graph
						.get(target)
						.is_some_and(|node| node.layer == related_layer)
				})
				.collect()
		} else {
			graph
				.iter()
				.filter(|(_, node)| node.layer == related_layer && node.link(&anchor_key).is_some())
				.map(|(key, _)| key.to_owned())
				.collect()
		};
		let other_key = format!("{anchor_key}{OTHER}");
		let related_other = graph
			.get(&other_key)
			.is_some_and(|node| node.layer == related_layer)
			.then_some(other_key.clone());
		let ordered: Vec<String> = related
			.into_iter()
			.filter(|key| *key != other_key)
			.collect();
		let mut moving: Vec<String> = ordered
			.iter()
			.filter(|key| entries.contains(key))
			.cloned()
			.collect();
		moving.sort_by_key(|key| std::cmp::Reverse(value(key)));
		if let Some(other) = related_other
			.as_ref()
			.filter(|other| entries.contains(other))
		{
			moving.push(other.clone());
		}
		let to_move: HashSet<&String> = ordered.iter().chain(related_other.iter()).collect();
		let mut without: Vec<String> = entries
			.iter()
			.filter(|entry| !to_move.contains(entry))
			.cloned()
			.collect();
		let Some(anchor_index) = without.iter().position(|entry| *entry == anchor_key) else {
			continue;
		};
		let at = if after {
			anchor_index + 1
		} else {
			anchor_index
		};
		without.splice(at..at, moving);
		*entries = without;
	}
}

/// `sortGraph`.
fn sort_graph(graph: Graph, sort: SankeySort, groups: &[GroupView]) -> Graph {
	let values = node_values(&graph, &layers(&graph));
	let value = |key: &String| values.get(key).copied().unwrap_or(0);
	let by_value = || {
		let mut keys = graph.keys();
		keys.sort_by_key(|key| std::cmp::Reverse(value(key)));
		keys
	};
	let mut entries = match sort {
		SankeySort::Global => {
			// Actual moves Other nodes to the end while walking the same
			// array, so an Other right after another Other stays put.
			let mut keys = by_value();
			for index in 0..keys.len() {
				if let Some(key) = keys.get(index).cloned()
					&& key.ends_with(OTHER)
				{
					move_to_end(&mut keys, &key);
				}
			}
			keys
		}
		SankeySort::PerGroup => {
			let mut keys = by_value();
			place_around_anchors(
				&graph,
				&values,
				&mut keys,
				SankeyLayer::CategoryGroup,
				SankeyLayer::Category,
				true,
			);
			place_around_anchors(
				&graph,
				&values,
				&mut keys,
				SankeyLayer::IncomeCategory,
				SankeyLayer::IncomePayee,
				false,
			);
			keys
		}
		SankeySort::BudgetOrder => {
			let mut used = HashSet::new();
			let mut keys = Vec::new();
			for group in groups {
				let other = format!("{}{OTHER}", group.id);
				let candidates = std::iter::once(group.id.clone())
					.chain(group.categories.iter().map(|category| category.id.clone()))
					.chain(std::iter::once(other));
				for key in candidates {
					if graph.contains(&key) {
						used.insert(key.clone());
						keys.push(key);
					}
				}
			}
			keys.extend(graph.keys().into_iter().filter(|key| !used.contains(key)));
			keys
		}
	};
	let negatives: Vec<String> = entries
		.iter()
		.filter(|key| graph.get(key).is_some_and(|node| node.is_negative))
		.cloned()
		.collect();
	for key in negatives {
		move_to_start(&mut entries, &key);
	}
	if let Some(node) = graph.get(TO_BUDGET) {
		if node.is_negative {
			move_to_start(&mut entries, TO_BUDGET);
		} else {
			move_to_end(&mut entries, TO_BUDGET);
		}
	}
	move_to_end(&mut entries, LAST_MONTH_OVERSPENT);
	move_to_end(&mut entries, FOR_NEXT_MONTH);
	move_to_end(&mut entries, FROM_PREVIOUS_MONTH);
	let mut graph = graph;
	Graph::from_entries(
		entries
			.into_iter()
			.filter_map(|key| {
				let node = graph.get(&key)?.clone();
				graph.remove(&key);
				Some((key, node))
			})
			.collect(),
	)
}

/// `cleanUpNodes`: zero links go, then nodes left without any link.
fn clean_up(graph: &mut Graph) {
	for (_, node) in graph.iter_mut() {
		node.to.retain(|(_, value)| *value != 0);
	}
	let incoming: HashSet<String> = graph
		.iter()
		.flat_map(|(_, node)| node.to.iter().map(|(target, _)| target.clone()))
		.collect();
	let lonely: Vec<String> = graph
		.iter()
		.filter(|(key, node)| node.to.is_empty() && !incoming.contains(*key))
		.map(|(key, _)| key.to_owned())
		.collect();
	for key in lonely {
		graph.remove(&key);
	}
}

/// `addHiddenNodes`.
fn add_hidden_nodes(graph: &mut Graph) {
	let mut by_type: Vec<(SankeyLayer, Vec<String>)> = Vec::new();
	for (key, node) in graph.iter() {
		match by_type.iter_mut().find(|(layer, _)| *layer == node.layer) {
			Some((_, keys)) => keys.push(key.to_owned()),
			None => by_type.push((node.layer, vec![key.to_owned()])),
		}
	}
	let connectivity: Vec<(bool, bool)> = by_type
		.iter()
		.map(|(_, keys)| {
			(
				keys.iter().any(|key| graph.has_parent(key)),
				keys.iter()
					.any(|key| graph.get(key).is_some_and(|node| !node.to.is_empty())),
			)
		})
		.collect();
	let active: Vec<SankeyLayer> = LAYERS
		.into_iter()
		.filter(|layer| by_type.iter().any(|(present, _)| present == layer))
		.collect();
	let position = |layer: SankeyLayer| active.iter().position(|active| *active == layer);
	for ((layer, keys), (type_has_parent, type_has_child)) in by_type.iter().zip(connectivity) {
		let Some(at) = position(*layer) else {
			continue;
		};
		for key in keys {
			let has_parent = graph.has_parent(key);
			let has_child = graph.get(key).is_some_and(|node| !node.to.is_empty());
			if !has_parent && type_has_parent && at > 0 {
				let mut child = key.clone();
				for parent_layer in active[..at].iter().rev() {
					let parent = format!("{key}_{}{HIDDEN}", parent_layer.key());
					graph.add_node(&parent, *parent_layer, "");
					graph.add_value(&parent, &child, -1);
					child = parent;
				}
			}
			if !has_child && type_has_child && at + 1 < active.len() {
				let mut parent = key.clone();
				for child_layer in &active[at + 1..] {
					let child = format!("{key}_{}{HIDDEN}", child_layer.key());
					graph.add_node(&child, *child_layer, "");
					graph.add_value(&parent, &child, -1);
					parent = child;
				}
			}
		}
	}
	category_groups_after_accounts(graph);
}

/// `ensureCategoryGroupsFollowAccountLayers`.
fn category_groups_after_accounts(graph: &mut Graph) {
	let mut computed = layers(graph);
	let Some(deepest_account) = graph
		.iter()
		.filter(|(key, node)| node.layer == SankeyLayer::Account && !is_hidden(key))
		.filter_map(|(key, _)| computed.get(key).copied())
		.max()
	else {
		return;
	};
	let to_push: Vec<String> = graph
		.iter()
		.filter(|(key, node)| {
			node.layer == SankeyLayer::CategoryGroup
				&& computed
					.get(*key)
					.is_some_and(|layer| *layer <= deepest_account)
		})
		.map(|(key, _)| key.to_owned())
		.collect();
	for key in to_push {
		let mut guard = graph.slots.len() + LAYERS.len();
		while let Some(current) = computed
			.get(&key)
			.copied()
			.filter(|layer| *layer <= deepest_account)
		{
			let mut deepest: Option<(String, usize)> = None;
			for (parent, node) in graph.iter() {
				if node.link(&key).is_none() {
					continue;
				}
				if let Some(layer) = computed.get(parent).copied()
					&& deepest.as_ref().is_none_or(|(_, best)| layer > *best)
				{
					deepest = Some((parent.to_owned(), layer));
				}
			}
			let Some((parent, _)) = deepest else {
				break;
			};
			if guard == 0 {
				break;
			}
			guard -= 1;
			let hidden = format!("{key}_{}_{current}{HIDDEN}", SankeyLayer::Account.key());
			graph.add_node(&hidden, SankeyLayer::Account, "");
			graph.add_value(&parent, &hidden, -1);
			graph.add_value(&hidden, &key, -1);
			computed = layers(graph);
		}
	}
}

/// JS `Number.prototype.toFixed(1)`: the nearest tenth, ties away from zero.
fn to_fixed_1(x: f64) -> String {
	let negative = x < 0.0;
	let digits = format!("{:.80}", x.abs());
	let (whole, fraction) = digits.split_once('.').unwrap_or((&digits, ""));
	let mut fraction = fraction.bytes();
	let tenth = fraction.next().map_or(0, |digit| u128::from(digit - b'0'));
	let up = fraction.next().is_some_and(|digit| digit >= b'5');
	let scaled = whole.parse::<u128>().unwrap_or(0) * 10 + tenth + u128::from(up);
	format!(
		"{}{}.{}",
		if negative { "-" } else { "" },
		scaled / 10,
		scaled % 10
	)
}

/// `addPercentageLabels`: each node's share of its column's total.
#[allow(clippy::cast_precision_loss)]
fn add_percentages(graph: &mut Graph) {
	let computed = layers(graph);
	let values = node_values(graph, &computed);
	let mut sums: HashMap<usize, i64> = HashMap::new();
	for (key, _) in graph.iter() {
		if is_hidden(key) {
			continue;
		}
		if let Some(layer) = computed.get(key) {
			let sum = sums.entry(*layer).or_insert(0);
			*sum = sum.saturating_add(values.get(key).copied().unwrap_or(0));
		}
	}
	for (key, node) in graph.iter_mut() {
		node.percentage = if is_hidden(key) {
			None
		} else {
			computed.get(key).map(|layer| {
				let total = sums.get(layer).copied().unwrap_or(1);
				let value = values.get(key).copied().unwrap_or(0);
				let percentage = if total == 0 {
					0.0
				} else {
					(value as f64 / total as f64) * 100.0
				};
				format!("{}%", to_fixed_1(percentage))
			})
		};
	}
}

/// `colorIndexFromKey`: the key's first three hex digits pick a palette
/// slot.
fn palette_slot(key: &str) -> u8 {
	let hex: String = key
		.chars()
		.filter(char::is_ascii_hexdigit)
		.take(3)
		.collect();
	let value = u32::from_str_radix(&hex, 16).unwrap_or(0);
	u8::try_from(value % PALETTE_SIZE).unwrap_or(0)
}

/// `addColors`.
fn add_colors(graph: &mut Graph) {
	for (key, node) in graph.iter_mut() {
		node.color = Some(SankeyColor::Palette(palette_slot(key)));
	}
	let mut set = |key: &str, color: SankeyColor| {
		if let Some(node) = graph.get_mut(key) {
			node.color = Some(color);
		}
	};
	set(TO_BUDGET, SankeyColor::ToBudgetPositive);
	for (_, node) in graph.iter_mut() {
		if node.is_negative {
			node.color = Some(SankeyColor::ToBudgetNegative);
		}
	}
	let mut set = |key: &str, color: SankeyColor| {
		if let Some(node) = graph.get_mut(key) {
			node.color = Some(color);
		}
	};
	set(LAST_MONTH_OVERSPENT, SankeyColor::ToBudgetNegative);
	set(FROM_PREVIOUS_MONTH, SankeyColor::Gray);
	set(FOR_NEXT_MONTH, SankeyColor::Gray);
	set(BUDGETED, SankeyColor::Blue);
	set(AVAILABLE_INCOME, SankeyColor::Blue);
	set(ALL_ACCOUNTS, SankeyColor::Blue);
}

/// `filterGraphByLayers`. Links into a removed node stay, as in Actual.
fn keep_layers(graph: &mut Graph, from: SankeyLayer, to: SankeyLayer) {
	let (from, to) = (from.index(), to.index());
	let outside: Vec<String> = graph
		.iter()
		.filter(|(_, node)| node.layer.index() < from || node.layer.index() > to)
		.map(|(key, _)| key.to_owned())
		.collect();
	for key in outside {
		graph.remove(&key);
	}
}

/// `convertToSankeyData`.
fn convert(graph: &Graph, mut folded: Folded) -> (Vec<SankeyNode>, Vec<SankeyLink>) {
	let position: HashMap<&str, usize> = graph
		.iter()
		.enumerate()
		.map(|(index, (key, _))| (key, index))
		.collect();
	let mut nodes: Vec<SankeyNode> = graph
		.iter()
		.map(|(key, node)| SankeyNode {
			key: key.to_owned(),
			label: node.label.clone().unwrap_or_else(|| match &node.name {
				Some(name) if name.is_empty() && is_hidden(key) => SankeyLabel::Hidden,
				Some(name) => SankeyLabel::Name(name.clone()),
				None => SankeyLabel::Name(key.to_owned()),
			}),
			percentage: node.percentage.clone().unwrap_or_default(),
			color: node.color.unwrap_or(SankeyColor::Palette(0)),
			value: Money::ZERO,
		})
		.collect();
	let color_of = |key: &str| graph.get(key).and_then(|node| node.color);
	let mut links = Vec::new();
	for (index, (key, node)) in graph.iter().enumerate() {
		for (target, value) in &node.to {
			let mut tips = folded
				.remove(&(key.to_owned(), target.clone()))
				.unwrap_or_default();
			tips.sort_by_key(|(_, value)| std::cmp::Reverse(*value));
			let mut color = match node.layer {
				SankeyLayer::IncomePayee
				| SankeyLayer::IncomeCategory
				| SankeyLayer::Account
				| SankeyLayer::CategoryGroup => node.color,
				SankeyLayer::Category | SankeyLayer::Budget => color_of(target),
			};
			if target == LAST_MONTH_OVERSPENT
				|| target.ends_with(NEGATIVE)
				|| target == TO_BUDGET
				|| target == FOR_NEXT_MONTH
			{
				color = color_of(target);
			}
			if node.is_negative {
				color = node.color;
			}
			links.push(SankeyLink {
				source: index,
				target: position.get(target.as_str()).copied(),
				value: Money::from_minor(*value),
				folded: tips
					.into_iter()
					.map(|(name, value)| (name, Money::from_minor(value)))
					.collect(),
				color,
			});
		}
	}
	for (index, node) in nodes.iter_mut().enumerate() {
		node.value = node_value(&links, index);
	}
	(nodes, links)
}

/// recharts' `getNodesTree` (`Sankey.tsx`): the larger of a node's incoming
/// and outgoing link sums, counting -1 links to hidden nodes and links whose
/// target the layer range cut, exactly as the graph receives them.
fn node_value(links: &[SankeyLink], index: usize) -> Money {
	let sum = |linked: &dyn Fn(&SankeyLink) -> bool| {
		links
			.iter()
			.filter(|link| linked(link))
			.fold(0_i64, |sum, link| sum.saturating_add(link.value.minor()))
	};
	let incoming = sum(&|link| link.target == Some(index));
	let outgoing = sum(&|link| link.source == index);
	Money::from_minor(incoming.max(outgoing))
}

fn month_of(text: &str) -> Result<YearMonth, CoreError> {
	let date = parse_date(text)
		.ok_or_else(|| CoreError::Invalid(format!("the Sankey card's date {text:?}")))?;
	YearMonth::new(
		chrono::Datelike::year(&date),
		chrono::Datelike::month(&date),
	)
}

fn date_month(date: NaiveDate) -> Result<YearMonth, CoreError> {
	YearMonth::new(
		chrono::Datelike::year(&date),
		chrono::Datelike::month(&date),
	)
}

/// `boundMonthRange`.
fn bound(
	earliest: YearMonth,
	latest: YearMonth,
	start: YearMonth,
	end: YearMonth,
) -> (YearMonth, YearMonth) {
	let clamp = |month: YearMonth| {
		if month < earliest {
			earliest
		} else if month > latest {
			latest
		} else {
			month
		}
	};
	let start = clamp(start);
	(start, clamp(end).max(start))
}

/// `sheet` is the envelope sheet for `budget` as of `today`; only the
/// budgeted mode reads it. `height_limit` is the card's
/// `sankey_height_limit`, or `None` before it has a size.
pub fn sankey_card(
	budget: &BudgetData,
	sheet: &EnvelopeSheet,
	meta: Option<&Json>,
	today: NaiveDate,
	height_limit: Option<usize>,
) -> Result<SankeyCard, CoreError> {
	let field = |name: &str| {
		meta.and_then(|meta| meta.get(name))
			.filter(|value| !matches!(value, Json::Null))
	};
	let flag = |name: &str| field(name).and_then(Json::as_bool).unwrap_or(false);
	let latest = latest_transaction(budget).unwrap_or(today);
	let earliest = earliest_transaction(budget).unwrap_or(today);
	let range = calculate_time_range(field("timeFrame"), None, Some(latest), today);
	let (first_month, last_month) = bound(
		date_month(earliest)?,
		date_month(latest)?,
		month_of(&range.start)?,
		month_of(&range.end)?,
	);

	let mode_text = field("mode").map_or(Some("spent"), Json::as_str);
	let mode = if mode_text == Some("budgeted") {
		SankeyMode::Budgeted
	} else {
		SankeyMode::Spent
	};
	let top_n = match field("topNcategories") {
		None => 15.0,
		Some(value) => value.as_f64().ok_or_else(|| {
			CoreError::Invalid(format!(
				"the Sankey card's topNcategories {}",
				value.stringify()
			))
		})?,
	};
	#[allow(clippy::cast_precision_loss)]
	let top_n = height_limit.map_or(top_n, |limit| top_n.min(limit as f64));
	let sort = match field("categorySort") {
		None => SankeySort::PerGroup,
		Some(value) => match value.as_str() {
			Some("per-group") => SankeySort::PerGroup,
			Some("global") => SankeySort::Global,
			_ => SankeySort::BudgetOrder,
		},
	};
	let (layer_from, layer_to) = match (
		SankeyLayer::parse(field("layerFrom")),
		SankeyLayer::parse(field("layerTo")),
	) {
		(Some(from), Some(to)) => (from, to),
		_ if mode == SankeyMode::Budgeted => (SankeyLayer::IncomeCategory, SankeyLayer::Category),
		_ => (SankeyLayer::IncomePayee, SankeyLayer::Category),
	};
	let group_accounts = flag("groupAccounts");

	let groups = grouped_categories(budget);
	let graph = match mode_text {
		Some("budgeted") => {
			let (entries, aggregated) =
				budgeted_entries(budget, sheet, &groups, meta, first_month, last_month)?;
			budget_graph(&entries, &aggregated)?
		}
		Some("spent") => {
			let filter = meta_filter(meta)?;
			let start = first_month.first_day().naive();
			let end = last_month
				.first_day()
				.naive()
				.checked_add_months(Months::new(1))
				.and_then(|next| next.pred_opt())
				.ok_or(CoreError::Overflow)?;
			let rows = filtered_rows_between(budget, &filter, start, end)
				.collect::<Result<Vec<_>, _>>()?;
			let entries = spent_entries(budget, &groups, &rows, group_accounts)?;
			let transfers = if flag("showTransfers") {
				transfer_pairs(budget, &rows)
			} else {
				Vec::new()
			};
			transactions_graph(&entries, &transfers, group_accounts)
		}
		_ => Graph::default(),
	};

	let mut graph = graph;
	let folded = group_other(&mut graph, top_n, sort == SankeySort::Global);
	let mut graph = sort_graph(graph, sort, &groups);
	clean_up(&mut graph);
	add_hidden_nodes(&mut graph);
	add_percentages(&mut graph);
	add_colors(&mut graph);
	keep_layers(&mut graph, layer_from, layer_to);
	let (nodes, links) = convert(&graph, folded);
	Ok(SankeyCard {
		range,
		first_month,
		last_month,
		mode,
		show_percentages: flag("showPercentages"),
		nodes,
		links,
	})
}

#[cfg(test)]
mod tests {
	use super::to_fixed_1;

	#[test]
	fn to_fixed_rounds_ties_away_from_zero_like_js() {
		assert_eq!(to_fixed_1(12.25), "12.3");
		assert_eq!(to_fixed_1(0.05), "0.1");
		assert_eq!(to_fixed_1(1.45), "1.4");
		assert_eq!(to_fixed_1(-0.04), "-0.0");
		assert_eq!(to_fixed_1(-0.0), "0.0");
		assert_eq!(to_fixed_1(99.95), "100.0");
		assert_eq!(to_fixed_1(100.0), "100.0");
	}
}
