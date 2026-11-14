//! The register as one paged screen model (R-ACC-2, R-PERF-3): rows read the
//! way `v_transactions` shows them, with payees and categories through their
//! mappings, split lines under their parent and the other side of a transfer.

use veritas_domain::{
	AccountId, BudgetData, CategoryId, CivilDate, CoreError, Money, PayeeId, ScheduleId, SplitRole,
	Transaction, TransactionId,
};

use crate::transactions::{alive, children, register_order, transfer_account};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
	Account(AccountId),
	OnBudget,
	OffBudget,
	All,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
	Uncleared,
	Cleared,
	Reconciled,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filter {
	/// Case-insensitive text in the payee, category, account or notes.
	pub search: Option<String>,
	pub status: Option<Status>,
	pub from: Option<CivilDate>,
	pub to: Option<CivilDate>,
	pub category: Option<CategoryId>,
	pub payee: Option<PayeeId>,
	/// On-budget rows with no category that need one (dc/queries/index.ts:45-55).
	pub uncategorized: bool,
}

impl Filter {
	fn is_empty(&self) -> bool {
		*self == Filter::default()
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
	/// Actual's own order: newest first.
	Date,
	Account,
	Payee,
	Category,
	Notes,
	Amount,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sort {
	pub key: SortKey,
	pub ascending: bool,
}

impl Sort {
	pub const DEFAULT: Sort = Sort {
		key: SortKey::Date,
		ascending: false,
	};
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
	pub id: TransactionId,
	pub amount: Money,
	pub payee: Option<PayeeId>,
	pub payee_name: Option<String>,
	pub category: Option<CategoryId>,
	pub category_name: Option<String>,
	pub notes: Option<String>,
	pub transfer_account: Option<AccountId>,
	pub counterpart: Option<TransactionId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegisterRow {
	pub id: TransactionId,
	pub account: AccountId,
	pub account_name: String,
	pub date: CivilDate,
	pub amount: Money,
	pub payee: Option<PayeeId>,
	/// A transfer payee reads as its account's name.
	pub payee_name: Option<String>,
	pub category: Option<CategoryId>,
	pub category_name: Option<String>,
	pub notes: Option<String>,
	pub tags: Vec<String>,
	pub cleared: bool,
	pub reconciled: bool,
	/// The account on the other side of a transfer, and its row.
	pub transfer_account: Option<AccountId>,
	pub counterpart: Option<TransactionId>,
	pub lines: Vec<Line>,
	/// A split whose lines don't add up to it, as some imported files hold:
	/// `amount − sum(lines)`. Shown, never fixed silently (edge case 6).
	pub split_difference: Option<Money>,
	pub starting_balance: bool,
	pub schedule: Option<ScheduleId>,
	pub needs_actual: Option<String>,
	/// The account's balance after this row, in a single account's register
	/// in Actual's order with no filter.
	pub running_balance: Option<Money>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
	/// Rows matching the scope and filter, across all pages.
	pub total: usize,
	pub rows: Vec<RegisterRow>,
}

fn in_scope(data: &BudgetData, scope: &Scope, row: &Transaction) -> bool {
	let account = data.accounts.get(&row.account);
	match scope {
		Scope::Account(id) => &row.account == id,
		Scope::OnBudget => account.is_some_and(|a| !a.off_budget && !a.tombstone),
		Scope::OffBudget => account.is_some_and(|a| a.off_budget && !a.tombstone),
		Scope::All => account.is_some_and(|a| !a.tombstone),
	}
}

fn payee_name(data: &BudgetData, payee: Option<&PayeeId>) -> (Option<PayeeId>, Option<String>) {
	let Some(payee) = payee else {
		return (None, None);
	};
	let mapped = data.resolve_payee(payee).unwrap_or(payee);
	let Some(found) = data.payees.get(mapped).filter(|p| !p.tombstone) else {
		return (None, None);
	};
	let name = match &found.transfer_account {
		Some(account) => data.accounts.get(account).map(|a| a.name.clone()),
		None => Some(found.name.clone()),
	};
	(Some(mapped.clone()), name)
}

fn category_name(
	data: &BudgetData,
	category: Option<&CategoryId>,
) -> (Option<CategoryId>, Option<String>) {
	let resolved = category
		.and_then(|category| data.resolve_category(category))
		.and_then(|category| data.categories.get(category))
		.filter(|category| !category.tombstone);
	match resolved {
		Some(category) => (Some(category.id.clone()), Some(category.name.clone())),
		None => (None, None),
	}
}

/// Actual's tags are `#words` in the notes.
pub fn tags(notes: Option<&str>) -> Vec<String> {
	notes
		.unwrap_or_default()
		.split_whitespace()
		.filter_map(|word| word.strip_prefix('#'))
		.filter(|tag| !tag.is_empty() && !tag.starts_with('#'))
		.map(str::to_owned)
		.collect()
}

fn line(data: &BudgetData, row: &Transaction) -> Line {
	let (payee, payee_name) = payee_name(data, row.payee.as_ref());
	let (category, category_name) = category_name(data, row.category.as_ref());
	Line {
		id: row.id.clone(),
		amount: row.amount,
		payee,
		payee_name,
		category,
		category_name,
		notes: row.notes.clone(),
		transfer_account: row
			.transfer
			.as_ref()
			.and(transfer_account(data, row.payee.as_ref())),
		counterpart: row.transfer.clone(),
	}
}

fn row_model(
	data: &BudgetData,
	row: &Transaction,
	children: &[&Transaction],
) -> Result<RegisterRow, CoreError> {
	let lines: Vec<Line> = if row.split == SplitRole::Parent {
		children.iter().map(|child| line(data, child)).collect()
	} else {
		Vec::new()
	};
	let split_difference = if row.split == SplitRole::Parent {
		let total = Money::checked_sum(lines.iter().map(|l| l.amount))?;
		let difference = row.amount.checked_sub(total)?;
		(difference != Money::ZERO).then_some(difference)
	} else {
		None
	};
	let head = line(data, row);
	let (category, category_name) = if row.split == SplitRole::Parent {
		(None, None)
	} else {
		(head.category, head.category_name)
	};
	Ok(RegisterRow {
		id: row.id.clone(),
		account: row.account.clone(),
		account_name: data
			.accounts
			.get(&row.account)
			.map(|a| a.name.clone())
			.unwrap_or_default(),
		date: row.date,
		amount: row.amount,
		payee: head.payee,
		payee_name: head.payee_name,
		category,
		category_name,
		notes: row.notes.clone(),
		tags: tags(row.notes.as_deref()),
		cleared: row.cleared,
		reconciled: row.reconciled,
		transfer_account: head.transfer_account,
		counterpart: head.counterpart,
		lines,
		split_difference,
		starting_balance: row.starting_balance,
		schedule: row.schedule.clone(),
		needs_actual: data
			.transaction_meta
			.get(&row.id)
			.and_then(|meta| meta.needs_actual.clone()),
		running_balance: None,
	})
}

fn matches(data: &BudgetData, filter: &Filter, row: &Transaction, lines: &[&Transaction]) -> bool {
	if filter.from.is_some_and(|from| row.date < from) || filter.to.is_some_and(|to| row.date > to)
	{
		return false;
	}
	if let Some(status) = filter.status {
		let actual = if row.reconciled {
			Status::Reconciled
		} else if row.cleared {
			Status::Cleared
		} else {
			Status::Uncleared
		};
		if actual != status {
			return false;
		}
	}
	let rows_and_lines = || std::iter::once(row).chain(lines.iter().copied());
	if let Some(category) = &filter.category {
		let hit = rows_and_lines().any(|r| {
			r.split != SplitRole::Parent
				&& category_name(data, r.category.as_ref()).0.as_ref() == Some(category)
		});
		if !hit {
			return false;
		}
	}
	if let Some(payee) = &filter.payee
		&& !rows_and_lines().any(|r| payee_name(data, r.payee.as_ref()).0.as_ref() == Some(payee))
	{
		return false;
	}
	if filter.uncategorized {
		let on_budget = data
			.accounts
			.get(&row.account)
			.is_some_and(|a| !a.off_budget);
		let needs = rows_and_lines().any(|r| {
			r.split != SplitRole::Parent
				&& category_name(data, r.category.as_ref()).0.is_none()
				&& transfer_account(data, r.payee.as_ref())
					.and_then(|to| data.accounts.get(&to))
					.is_none_or(|to| to.off_budget)
		});
		if !on_budget || !needs {
			return false;
		}
	}
	if let Some(search) = filter
		.search
		.as_deref()
		.map(str::trim)
		.filter(|s| !s.is_empty())
	{
		let search = search.to_lowercase();
		let has = |text: Option<String>| text.is_some_and(|t| t.to_lowercase().contains(&search));
		let account = data.accounts.get(&row.account).map(|a| a.name.clone());
		let hit = has(account)
			|| rows_and_lines().any(|r| {
				has(payee_name(data, r.payee.as_ref()).1)
					|| has(category_name(data, r.category.as_ref()).1)
					|| has(r.notes.clone())
			});
		if !hit {
			return false;
		}
	}
	true
}

/// The register's rows in order, with running balances when they apply: the
/// part of a page that reads every row. The session keeps one between
/// commits so scrolling only builds the rows it shows (R-PERF-3).
#[derive(Clone, Debug, PartialEq)]
pub struct Index {
	pub scope: Scope,
	pub filter: Filter,
	pub sort: Sort,
	ids: Vec<TransactionId>,
	balances: Vec<Money>,
	lines: std::collections::HashMap<TransactionId, Vec<TransactionId>>,
}

impl Index {
	pub fn answers(&self, scope: &Scope, filter: &Filter, sort: Sort) -> bool {
		self.scope == *scope && self.filter == *filter && self.sort == sort
	}

	pub fn page(&self, data: &BudgetData, offset: usize, limit: usize) -> Result<Page, CoreError> {
		let mut rows = Vec::new();
		for (index, id) in self.ids.iter().enumerate().skip(offset).take(limit) {
			let Some(found) = data.transactions.get(id) else {
				continue;
			};
			let lines: Vec<&Transaction> = self
				.lines
				.get(id)
				.into_iter()
				.flatten()
				.filter_map(|line| data.transactions.get(line))
				.collect();
			let mut model = row_model(data, found, &lines)?;
			model.running_balance = self.balances.get(index).copied();
			rows.push(model);
		}
		Ok(Page {
			total: self.ids.len(),
			rows,
		})
	}
}

/// One page of the register: `offset` rows in, at most `limit` long.
pub fn page(
	data: &BudgetData,
	scope: &Scope,
	filter: &Filter,
	sort: Sort,
	offset: usize,
	limit: usize,
) -> Result<Page, CoreError> {
	index(data, scope, filter, sort)?.page(data, offset, limit)
}

pub fn index(
	data: &BudgetData,
	scope: &Scope,
	filter: &Filter,
	sort: Sort,
) -> Result<Index, CoreError> {
	let mut lines_of: std::collections::HashMap<&TransactionId, Vec<&Transaction>> =
		std::collections::HashMap::new();
	if !filter.is_empty() {
		for row in data.transactions.values().filter(|r| !r.tombstone) {
			if let SplitRole::Child { parent } = &row.split {
				lines_of.entry(parent).or_default().push(row);
			}
		}
	}
	let mut rows: Vec<&Transaction> = data
		.transactions
		.values()
		.filter(|row| !matches!(row.split, SplitRole::Child { .. }))
		.filter(|row| alive(data, row) && in_scope(data, scope, row))
		.filter(|row| {
			filter.is_empty()
				|| matches(
					data,
					filter,
					row,
					lines_of.get(&row.id).map_or(&[][..], Vec::as_slice),
				)
		})
		.collect();
	rows.sort_by(|a, b| register_order(a, b));
	let sort_text = |row: &&Transaction| -> String {
		match sort.key {
			SortKey::Account => data
				.accounts
				.get(&row.account)
				.map(|a| a.name.to_lowercase()),
			SortKey::Payee => payee_name(data, row.payee.as_ref())
				.1
				.map(|n| n.to_lowercase()),
			SortKey::Category => category_name(data, row.category.as_ref())
				.1
				.map(|n| n.to_lowercase()),
			SortKey::Notes => row.notes.as_ref().map(|n| n.to_lowercase()),
			SortKey::Date | SortKey::Amount => None,
		}
		.unwrap_or_default()
	};
	match sort.key {
		SortKey::Date if !sort.ascending => {}
		SortKey::Date => rows.reverse(),
		SortKey::Amount => rows.sort_by(|a, b| {
			let order = a.amount.cmp(&b.amount);
			if sort.ascending {
				order
			} else {
				order.reverse()
			}
		}),
		_ => {
			rows.sort_by_cached_key(sort_text);
			if !sort.ascending {
				rows.reverse();
			}
		}
	}
	let running = matches!(scope, Scope::Account(_)) && filter.is_empty() && sort == Sort::DEFAULT;
	let mut balances = Vec::new();
	if running {
		let mut balance = Money::ZERO;
		for row in rows.iter().rev() {
			balance = balance.checked_add(row.amount)?;
			balances.push(balance);
		}
		balances.reverse();
	}
	Ok(Index {
		scope: scope.clone(),
		filter: filter.clone(),
		sort,
		lines: line_index(data, &rows),
		ids: rows.into_iter().map(|row| row.id.clone()).collect(),
		balances,
	})
}

/// Each listed split's live lines in Actual's order, from one pass.
fn line_index(
	data: &BudgetData,
	rows: &[&Transaction],
) -> std::collections::HashMap<TransactionId, Vec<TransactionId>> {
	let parents: std::collections::HashSet<&TransactionId> = rows
		.iter()
		.filter(|row| row.split == SplitRole::Parent)
		.map(|row| &row.id)
		.collect();
	let mut lines: std::collections::HashMap<TransactionId, Vec<&Transaction>> = Default::default();
	if !parents.is_empty() {
		for row in data.transactions.values().filter(|row| !row.tombstone) {
			if let SplitRole::Child { parent } = &row.split
				&& parents.contains(parent)
			{
				lines.entry(parent.clone()).or_default().push(row);
			}
		}
	}
	lines
		.into_iter()
		.map(|(parent, mut children)| {
			children.sort_by(|a, b| {
				b.sort_order
					.total_cmp(&a.sort_order)
					.then_with(|| a.id.cmp(&b.id))
			});
			(parent, children.into_iter().map(|c| c.id.clone()).collect())
		})
		.collect()
}

/// One row, as the register shows it, for refreshing after an edit.
pub fn row(data: &BudgetData, id: &TransactionId) -> Result<RegisterRow, CoreError> {
	let found = data
		.transactions
		.get(id)
		.filter(|row| alive(data, row))
		.ok_or_else(|| CoreError::NotFound {
			entity: "transaction",
			id: id.to_string(),
		})?;
	row_model(data, found, &children(data, id))
}
