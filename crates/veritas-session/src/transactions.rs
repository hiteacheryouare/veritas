//! Register writes as Actual's `batchUpdateTransactions` makes them (R-ACC-2,
//! docs/actual-format.md §4.1–§4.3): rules on a typed row (D60, D64), the
//! transfer hooks, splits kept line by line, and category learning (§4.5).

use std::collections::{BTreeMap, BTreeSet};

use chrono::Days;
use serde_json::Value;
use veritas_domain::{
	AccountId, BudgetData, CategoryId, CivilDate, CoreError, Money, Payee, PayeeId, PayeeMapping,
	Row, Rule, RuleId, RuleStage, SplitRole, Transaction, TransactionId, TransactionMeta,
	TransactionMetaRow,
};
use veritas_rules::{
	BudgetPayees, RuleBook, RuleTransaction, SplitLine, Transfers, counterpart, manual_entry,
	transfers,
};

/// A transaction as the register edits it. A split has `splits`; their
/// amounts must add up to `amount` (edge case 6).
#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
	/// For a new row, the id the app chose for it, as Actual's client makes
	/// ids; `None` makes one. Ignored by an edit.
	pub id: Option<TransactionId>,
	pub account: AccountId,
	pub date: CivilDate,
	pub amount: Money,
	pub payee: Option<PayeeId>,
	pub category: Option<CategoryId>,
	pub notes: Option<String>,
	pub cleared: bool,
	pub splits: Vec<Line>,
}

/// One split line. `id` names an existing line when editing, so it keeps its
/// row; `None` is a new line.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
	pub id: Option<TransactionId>,
	pub amount: Money,
	pub payee: Option<PayeeId>,
	pub category: Option<CategoryId>,
	pub notes: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Planned {
	pub rows: Vec<Row>,
	pub id: TransactionId,
	/// D64's "may need Actual" note, when a rule couldn't run.
	pub note: Option<String>,
}

/// Live, and for a split line, its parent is live too (schema/index.ts:404-411).
pub fn alive(data: &BudgetData, row: &Transaction) -> bool {
	if row.tombstone {
		return false;
	}
	match &row.split {
		SplitRole::Child { parent } => data.transactions.get(parent).is_some_and(|p| !p.tombstone),
		_ => true,
	}
}

/// The payee Actual keeps for transfers to `account`.
pub fn transfer_payee(data: &BudgetData, account: &AccountId) -> Option<PayeeId> {
	data.payees
		.values()
		.find(|payee| !payee.tombstone && payee.transfer_account.as_ref() == Some(account))
		.map(|payee| payee.id.clone())
}

/// The live account a payee transfers to, read through `payee_mapping`.
pub fn transfer_account(data: &BudgetData, payee: Option<&PayeeId>) -> Option<AccountId> {
	let payee = payee?;
	let payee = data.resolve_payee(payee).unwrap_or(payee);
	let account = data.payees.get(payee)?.transfer_account.clone()?;
	data.accounts
		.get(&account)
		.filter(|account| !account.tombstone)
		.map(|account| account.id.clone())
}

fn off_budget(data: &BudgetData, account: &AccountId) -> bool {
	data.accounts.get(account).is_some_and(|a| a.off_budget)
}

fn new_transaction_id() -> TransactionId {
	TransactionId::new(veritas_store::new_id())
}

fn top_level<'a>(data: &'a BudgetData, id: &TransactionId) -> Result<&'a Transaction, CoreError> {
	let row = data
		.transactions
		.get(id)
		.filter(|row| alive(data, row))
		.ok_or_else(|| CoreError::NotFound {
			entity: "transaction",
			id: id.to_string(),
		})?;
	if matches!(row.split, SplitRole::Child { .. }) {
		return Err(CoreError::Invalid(
			"That's one line of a split. Edit the split to change it.".to_owned(),
		));
	}
	Ok(row)
}

pub fn children<'a>(data: &'a BudgetData, parent: &TransactionId) -> Vec<&'a Transaction> {
	let mut lines: Vec<&Transaction> = data
		.transactions
		.values()
		.filter(|row| !row.tombstone)
		.filter(|row| matches!(&row.split, SplitRole::Child { parent: p } if p == parent))
		.collect();
	lines.sort_by(|a, b| {
		b.sort_order
			.total_cmp(&a.sort_order)
			.then_with(|| a.id.cmp(&b.id))
	});
	lines
}

fn check(data: &BudgetData, draft: &Draft) -> Result<(), CoreError> {
	let account = data
		.accounts
		.get(&draft.account)
		.filter(|account| !account.tombstone)
		.ok_or_else(|| CoreError::NotFound {
			entity: "account",
			id: draft.account.to_string(),
		})?;
	let _ = account;
	if draft.splits.is_empty() {
		return Ok(());
	}
	let total = Money::checked_sum(draft.splits.iter().map(|line| line.amount))?;
	if total != draft.amount {
		return Err(CoreError::Invalid(format!(
			"The split lines add up to {total}, not {}. Make them match before saving.",
			draft.amount
		)));
	}
	Ok(())
}

fn ruled(draft: &Draft) -> RuleTransaction {
	RuleTransaction {
		account: draft.account.clone(),
		date: draft.date,
		amount: draft.amount,
		payee: draft.payee.clone(),
		category: draft.category.clone(),
		notes: draft.notes.clone().filter(|notes| !notes.is_empty()),
		imported_payee: None,
		cleared: Some(draft.cleared),
		reconciled: Some(false),
		schedule: None,
		deleted: false,
		splits: draft
			.splits
			.iter()
			.map(|line| SplitLine {
				amount: line.amount,
				// `makeChild`: a line without a payee takes the parent's.
				payee: line.payee.clone().or_else(|| draft.payee.clone()),
				category: line.category.clone(),
				notes: line.notes.clone(),
			})
			.collect(),
		is_child: false,
	}
}

/// A row typed by hand (D64): the rules fill what was left empty when every
/// rule ran; otherwise it is saved as typed with the "may need Actual" note.
/// Payees a rule makes are written only from a run that wholly ran (D60).
pub fn add(
	data: &BudgetData,
	draft: &Draft,
	book: &RuleBook,
	now_ms: f64,
) -> Result<Planned, CoreError> {
	check(data, draft)?;
	let entered = ruled(draft);
	let mut payees = BudgetPayees::new(data, || PayeeId::new(veritas_store::new_id()));
	let outcome = book.run(&entered, data, &mut payees);
	let entry = manual_entry(&entered, &outcome, &[]);
	let mut rows = Vec::new();
	if entry.note.is_none() {
		for (id, name) in &payees.created {
			rows.push(Row::Payee(new_payee(id, name)));
			rows.push(Row::PayeeMapping(PayeeMapping {
				from: id.clone(),
				to: id.clone(),
			}));
		}
	}
	let with_payees = crate::writes::applied(data, &rows);
	let mut planned = write_new(
		&with_payees,
		draft.id.clone(),
		&entry.transaction,
		Some(book),
		now_ms,
	)?;
	if let Some(note) = &entry.note {
		planned.rows.push(meta(&planned.id, note));
	}
	rows.append(&mut planned.rows);
	Ok(Planned {
		rows,
		id: planned.id,
		note: entry.note,
	})
}

/// A new row written without running rules on it, as `transaction-add` does
/// for a closing transfer or a duplicate. Counterparts still go through the
/// rules when `book` is given, as `addTransfer` does.
pub fn insert(
	data: &BudgetData,
	draft: &Draft,
	book: Option<&RuleBook>,
	now_ms: f64,
) -> Result<Planned, CoreError> {
	check(data, draft)?;
	write_new(data, draft.id.clone(), &ruled(draft), book, now_ms)
}

fn new_payee(id: &PayeeId, name: &str) -> Payee {
	Payee {
		id: id.clone(),
		name: name.to_owned(),
		transfer_account: None,
		favorite: false,
		learn_categories: true,
		tombstone: false,
	}
}

fn meta(id: &TransactionId, note: &str) -> Row {
	Row::TransactionMeta(TransactionMetaRow {
		transaction: id.clone(),
		meta: TransactionMeta {
			adjustment: false,
			needs_actual: Some(note.to_owned()),
		},
	})
}

/// `batchUpdateTransactions` for one added row: insert it (a parent or an
/// off-budget row gets no category), then `onInsert`'s `addTransfer` for the
/// row or each split line with a transfer payee.
fn write_new(
	data: &BudgetData,
	id: Option<TransactionId>,
	added: &RuleTransaction,
	book: Option<&RuleBook>,
	now_ms: f64,
) -> Result<Planned, CoreError> {
	let Transfers { transaction, legs } = transfers(added, data);
	let id = match id {
		Some(id) if data.transactions.contains_key(&id) => {
			return Err(CoreError::Invalid(format!(
				"A transaction with id {id} already exists."
			)));
		}
		Some(id) => id,
		None => new_transaction_id(),
	};
	let off = off_budget(data, &transaction.account);
	let cleared = transaction.cleared.unwrap_or(true);
	let reconciled = transaction.reconciled.unwrap_or(false);
	let split = !transaction.splits.is_empty();
	let mut top = Transaction {
		id: id.clone(),
		account: transaction.account.clone(),
		date: transaction.date,
		amount: transaction.amount,
		payee: transaction.payee.clone(),
		imported_payee: transaction.imported_payee.clone(),
		category: if split || off {
			None
		} else {
			transaction.category.clone()
		},
		notes: transaction.notes.clone(),
		cleared,
		reconciled,
		split: if split {
			SplitRole::Parent
		} else {
			SplitRole::None
		},
		transfer: None,
		schedule: transaction.schedule.clone(),
		starting_balance: false,
		sort_order: now_ms,
		tombstone: transaction.deleted,
	};
	let mut lines: Vec<Transaction> = transaction
		.splits
		.iter()
		.enumerate()
		.map(|(n, line)| Transaction {
			id: new_transaction_id(),
			account: top.account.clone(),
			date: top.date,
			amount: line.amount,
			payee: line.payee.clone(),
			imported_payee: None,
			category: if off { None } else { line.category.clone() },
			notes: line.notes.clone(),
			cleared,
			reconciled,
			split: SplitRole::Child { parent: id.clone() },
			transfer: None,
			schedule: None,
			starting_balance: false,
			sort_order: -((n + 1) as f64),
			tombstone: transaction.deleted,
		})
		.collect();
	let mut extra = Vec::new();
	if !transaction.deleted {
		for leg in legs {
			let source = match leg.line {
				None => &mut top,
				Some(n) => match lines.get_mut(n) {
					Some(line) => line,
					None => continue,
				},
			};
			let (other, note) = counterpart_row(data, &leg.mirror, &source.id, book, now_ms);
			source.transfer = Some(other.id.clone());
			if let Some(note) = note {
				extra.push(meta(&other.id, &note));
			}
			extra.push(Row::Transaction(other));
		}
	}
	let mut rows = vec![Row::Transaction(top)];
	rows.extend(lines.into_iter().map(Row::Transaction));
	rows.extend(extra);
	Ok(Planned {
		rows,
		id,
		note: None,
	})
}

/// `addTransfer`'s counterpart: the mirror run through the rules, keeping
/// only `notes`, `cleared` and `schedule` (transfer.ts:61-79). A run that
/// can't finish keeps the mirror as built and says so (D60).
fn counterpart_row(
	data: &BudgetData,
	mirror: &RuleTransaction,
	source: &TransactionId,
	book: Option<&RuleBook>,
	now_ms: f64,
) -> (Transaction, Option<String>) {
	let (kept, note) = match book {
		Some(book) => {
			let mut payees = BudgetPayees::new(data, || PayeeId::new(veritas_store::new_id()));
			let outcome = book.run(mirror, data, &mut payees);
			match &outcome.transaction {
				Some(ruled) => (counterpart(mirror, ruled), None),
				None => (mirror.clone(), veritas_rules::may_need_actual(&outcome)),
			}
		}
		None => (mirror.clone(), None),
	};
	(
		Transaction {
			id: new_transaction_id(),
			account: kept.account.clone(),
			date: kept.date,
			amount: kept.amount,
			payee: kept.payee.clone(),
			imported_payee: None,
			category: None,
			notes: kept.notes.clone(),
			cleared: kept.cleared.unwrap_or(false),
			reconciled: false,
			split: SplitRole::None,
			transfer: Some(source.clone()),
			schedule: kept.schedule.clone(),
			starting_balance: false,
			sort_order: now_ms,
			tombstone: false,
		},
		note,
	)
}

fn as_ruled(row: &Transaction) -> RuleTransaction {
	RuleTransaction {
		account: row.account.clone(),
		date: row.date,
		amount: row.amount,
		payee: row.payee.clone(),
		category: row.category.clone(),
		notes: row.notes.clone(),
		imported_payee: row.imported_payee.clone(),
		cleared: Some(row.cleared),
		reconciled: Some(row.reconciled),
		schedule: row.schedule.clone(),
		deleted: false,
		splits: Vec::new(),
		is_child: matches!(row.split, SplitRole::Child { .. }),
	}
}

/// `removeTransfer` (transfer.ts:95-118): a counterpart that is a split line
/// becomes an ordinary line; any other is deleted.
fn remove_counterpart(data: &BudgetData, counterpart: &TransactionId) -> Option<Row> {
	let other = data
		.transactions
		.get(counterpart)
		.filter(|row| alive(data, row))?;
	Some(Row::Transaction(match other.split {
		SplitRole::Child { .. } => Transaction {
			transfer: None,
			payee: None,
			..other.clone()
		},
		_ => Transaction {
			tombstone: true,
			..other.clone()
		},
	}))
}

/// `onUpdate` for one row as it now stands (transfer.ts:154-172). `row` is
/// updated in place; the counterpart's rows are returned.
fn sync_transfer(
	data: &BudgetData,
	row: &mut Transaction,
	book: Option<&RuleBook>,
	now_ms: f64,
) -> Vec<Row> {
	let mut rows = Vec::new();
	if row.split == SplitRole::Parent {
		if let Some(counterpart) = row.transfer.take() {
			rows.extend(remove_counterpart(data, &counterpart));
		}
		return rows;
	}
	let to = transfer_account(data, row.payee.as_ref());
	match (to, row.transfer.clone()) {
		(Some(_), None) => {
			let Transfers { transaction, legs } = transfers(&as_ruled(row), data);
			row.category = transaction.category;
			if let Some(leg) = legs.into_iter().next() {
				let (other, note) = counterpart_row(data, &leg.mirror, &row.id, book, now_ms);
				row.transfer = Some(other.id.clone());
				if let Some(note) = note {
					rows.push(meta(&other.id, &note));
				}
				rows.push(Row::Transaction(other));
			}
		}
		(None, Some(counterpart)) => {
			rows.extend(remove_counterpart(data, &counterpart));
			row.transfer = None;
		}
		(Some(to), Some(counterpart)) => {
			let Some(other) = data
				.transactions
				.get(&counterpart)
				.filter(|o| alive(data, o))
			else {
				return rows;
			};
			let same_side = off_budget(data, &row.account) == off_budget(data, &to);
			if same_side {
				row.category = None;
			}
			rows.push(Row::Transaction(Transaction {
				account: to,
				payee: transfer_payee(data, &row.account),
				notes: row.notes.clone(),
				amount: Money::from_minor(-row.amount.minor()),
				schedule: row.schedule.clone(),
				category: if same_side {
					None
				} else {
					other.category.clone()
				},
				..other.clone()
			}));
		}
		(None, None) => {}
	}
	rows
}

/// Edits a transaction in place, line by line for a split, and runs the
/// transfer hooks. Editing never runs rules (§6.2).
pub fn edit(
	data: &BudgetData,
	id: &TransactionId,
	draft: &Draft,
	book: Option<&RuleBook>,
	now_ms: f64,
) -> Result<Vec<Row>, CoreError> {
	check(data, draft)?;
	let existing = top_level(data, id)?;
	let off = off_budget(data, &draft.account);
	let moved = existing.account != draft.account;
	let split = !draft.splits.is_empty();
	let mut top = Transaction {
		account: draft.account.clone(),
		date: draft.date,
		amount: draft.amount,
		payee: draft.payee.clone(),
		category: if split || off {
			None
		} else {
			draft.category.clone()
		},
		notes: draft.notes.clone().filter(|notes| !notes.is_empty()),
		cleared: draft.cleared,
		reconciled: existing.reconciled && !moved,
		split: if split {
			SplitRole::Parent
		} else {
			SplitRole::None
		},
		sort_order: if existing.date == draft.date {
			existing.sort_order
		} else {
			now_ms
		},
		..existing.clone()
	};
	let old_lines = children(data, id);
	let kept: BTreeSet<&TransactionId> =
		draft.splits.iter().filter_map(|l| l.id.as_ref()).collect();
	let mut rows = Vec::new();
	let mut lines = Vec::new();
	let mut lowest = old_lines.iter().map(|l| l.sort_order).fold(0.0, f64::min);
	for line in &draft.splits {
		let base = match &line.id {
			Some(line_id) => old_lines
				.iter()
				.find(|old| &old.id == line_id)
				.map(|old| (*old).clone())
				.ok_or_else(|| CoreError::NotFound {
					entity: "split line",
					id: line_id.to_string(),
				})?,
			None => {
				lowest -= 1.0;
				Transaction {
					id: new_transaction_id(),
					transfer: None,
					imported_payee: None,
					schedule: None,
					starting_balance: false,
					sort_order: lowest,
					tombstone: false,
					..top.clone()
				}
			}
		};
		lines.push(Transaction {
			account: top.account.clone(),
			date: top.date,
			cleared: top.cleared,
			reconciled: top.reconciled,
			amount: line.amount,
			payee: line.payee.clone().or_else(|| draft.payee.clone()),
			category: if off { None } else { line.category.clone() },
			notes: line.notes.clone().filter(|notes| !notes.is_empty()),
			split: SplitRole::Child { parent: id.clone() },
			..base
		});
	}
	for old in &old_lines {
		if kept.contains(&old.id) {
			continue;
		}
		if let Some(counterpart) = &old.transfer {
			rows.extend(remove_counterpart(data, counterpart));
		}
		rows.push(Row::Transaction(Transaction {
			tombstone: true,
			transfer: None,
			..(*old).clone()
		}));
	}
	let mut hooks = sync_transfer(data, &mut top, book, now_ms);
	for line in &mut lines {
		hooks.extend(sync_transfer(data, line, book, now_ms));
	}
	rows.push(Row::Transaction(top));
	rows.extend(lines.into_iter().map(Row::Transaction));
	rows.extend(hooks);
	Ok(rows)
}

/// Deletes a transaction with its split lines, and each transfer counterpart
/// (§4.2 `removeTransfer`, §4.3 deleting).
pub fn delete(data: &BudgetData, id: &TransactionId) -> Result<Vec<Row>, CoreError> {
	let top = top_level(data, id)?;
	let mut doomed = vec![top];
	doomed.extend(children(data, id));
	let mut rows = Vec::new();
	for row in doomed {
		if let Some(counterpart) = &row.transfer {
			rows.extend(remove_counterpart(data, counterpart));
		}
		rows.push(Row::Transaction(Transaction {
			tombstone: true,
			transfer: None,
			..row.clone()
		}));
	}
	Ok(rows)
}

/// The draft that reproduces a transaction and its lines.
pub fn draft_of(data: &BudgetData, id: &TransactionId) -> Result<Draft, CoreError> {
	let top = top_level(data, id)?;
	Ok(Draft {
		id: None,
		account: top.account.clone(),
		date: top.date,
		amount: top.amount,
		payee: top.payee.clone(),
		category: top.category.clone(),
		notes: top.notes.clone(),
		cleared: top.cleared,
		splits: children(data, id)
			.into_iter()
			.map(|line| Line {
				id: Some(line.id.clone()),
				amount: line.amount,
				payee: line.payee.clone(),
				category: line.category.clone(),
				notes: line.notes.clone(),
			})
			.collect(),
	})
}

/// Actual's Duplicate (useTransactionBatchActions.ts:286-316): a copy with new
/// ids, uncleared and unreconciled, through the add path without rules, so a
/// transfer gets a counterpart of its own.
pub fn duplicate(
	data: &BudgetData,
	id: &TransactionId,
	new_id: Option<TransactionId>,
	book: Option<&RuleBook>,
	now_ms: f64,
) -> Result<Planned, CoreError> {
	let mut draft = draft_of(data, id)?;
	draft.cleared = false;
	draft.id = new_id;
	for line in &mut draft.splits {
		line.id = None;
	}
	insert(data, &draft, book, now_ms)
}

/// Checks rows off (or back on), as a batch edit does: reconciled rows are
/// skipped, and a split's lines follow it (§4.7).
pub fn set_cleared(
	data: &BudgetData,
	ids: &[TransactionId],
	cleared: bool,
) -> Result<Vec<Row>, CoreError> {
	let mut rows = Vec::new();
	for id in ids {
		let top = top_level(data, id)?;
		if top.reconciled || top.cleared == cleared {
			continue;
		}
		rows.push(Row::Transaction(Transaction {
			cleared,
			..top.clone()
		}));
		for line in children(data, id) {
			rows.push(Row::Transaction(Transaction {
				cleared,
				..line.clone()
			}));
		}
	}
	Ok(rows)
}

/// Unlocks a reconciled row, leaving `cleared` alone (TransactionsTable.tsx:1212-1227).
pub fn unreconcile(data: &BudgetData, id: &TransactionId) -> Result<Vec<Row>, CoreError> {
	let top = top_level(data, id)?;
	let mut rows = vec![Row::Transaction(Transaction {
		reconciled: false,
		..top.clone()
	})];
	for line in children(data, id) {
		rows.push(Row::Transaction(Transaction {
			reconciled: false,
			..line.clone()
		}));
	}
	Ok(rows)
}

/// The payee named `name`, reused case-insensitively from the live ordinary
/// payees, or made with its mapping (`getPayeeByName`, `insertPayee`).
pub fn payee_named(data: &BudgetData, name: &str) -> Result<(Vec<Row>, PayeeId), CoreError> {
	let name = name.trim();
	if name.is_empty() {
		return Err(CoreError::Invalid("Enter a payee name.".to_owned()));
	}
	let lower = name.to_lowercase();
	if let Some(found) = data
		.payees
		.values()
		.find(|p| !p.tombstone && p.transfer_account.is_none() && p.name.to_lowercase() == lower)
	{
		return Ok((Vec::new(), found.id.clone()));
	}
	let id = PayeeId::new(veritas_store::new_id());
	Ok((
		vec![
			Row::Payee(new_payee(&id, name)),
			Row::PayeeMapping(PayeeMapping {
				from: id.clone(),
				to: id.clone(),
			}),
		],
		id,
	))
}

/// Rows as `v_transactions` orders them: date, newest first, then the
/// starting balance, then `sort_order` descending, then id.
pub(crate) fn register_order(a: &Transaction, b: &Transaction) -> std::cmp::Ordering {
	b.date
		.cmp(&a.date)
		.then(b.starting_balance.cmp(&a.starting_balance))
		.then(b.sort_order.total_cmp(&a.sort_order))
		.then_with(|| a.id.cmp(&b.id))
}

/// `updateCategoryRules` (transaction-rules.ts:952-1047), for the rows just
/// saved: a payee whose 5 latest rows include one of them, and agree on a
/// category at least 3 times, gets a rule that sets it.
pub fn learn_categories(
	data: &BudgetData,
	saved: &[TransactionId],
	today: CivilDate,
) -> Result<Vec<Row>, CoreError> {
	let saved_rows: Vec<&Transaction> = saved
		.iter()
		.filter_map(|id| data.transactions.get(id))
		.filter(|row| row.split != SplitRole::Parent && row.category.is_some())
		.collect();
	let Some(oldest) = saved_rows.iter().map(|row| row.date).min() else {
		return Ok(Vec::new());
	};
	let from = oldest
		.naive()
		.checked_sub_days(Days::new(180))
		.map(CivilDate::from);
	let until = today
		.naive()
		.checked_add_days(Days::new(180))
		.map(CivilDate::from);
	let (Some(from), Some(until)) = (from, until) else {
		return Ok(Vec::new());
	};
	let saved_ids: BTreeSet<&TransactionId> = saved.iter().collect();
	let mapped_payee = |row: &Transaction| {
		row.payee
			.as_ref()
			.map(|payee| data.resolve_payee(payee).unwrap_or(payee).clone())
	};
	let mapped_category = |row: &Transaction| {
		row.category
			.as_ref()
			.and_then(|category| data.resolve_category(category))
			.filter(|category| data.categories.get(*category).is_some_and(|c| !c.tombstone))
			.cloned()
	};
	let payees: BTreeSet<PayeeId> = saved_rows
		.iter()
		.filter_map(|row| mapped_payee(row))
		.collect();
	let mut register: Vec<&Transaction> = data
		.transactions
		.values()
		.filter(|row| alive(data, row) && row.split != SplitRole::Parent)
		.filter(|row| row.date >= from && row.date <= until)
		.filter(|row| data.accounts.get(&row.account).is_some_and(|a| !a.closed))
		.filter(|row| {
			mapped_payee(row)
				.and_then(|payee| data.payees.get(&payee))
				.is_some_and(|payee| payee.learn_categories)
		})
		.collect();
	register.sort_by(|a, b| register_order(a, b));
	let mut by_payee: BTreeMap<PayeeId, Vec<&Transaction>> = BTreeMap::new();
	for row in register {
		if let Some(payee) = mapped_payee(row)
			&& payees.contains(&payee)
		{
			by_payee.entry(payee).or_default().push(row);
		}
	}
	let mut rows = Vec::new();
	for (payee, latest) in by_payee {
		let latest: Vec<&Transaction> = latest.into_iter().take(5).collect();
		if !latest.iter().any(|row| saved_ids.contains(&row.id)) {
			continue;
		}
		let categories: Vec<Option<CategoryId>> =
			latest.iter().map(|row| mapped_category(row)).collect();
		if let Some(category) = probable_category(&categories) {
			rows.extend(category_rule(data, &payee, &category)?);
		}
	}
	Ok(rows)
}

/// `getProbableCategory`, quirks included: the first row's score is the bar,
/// so an uncategorized newest row stops anything being learned.
fn probable_category(categories: &[Option<CategoryId>]) -> Option<CategoryId> {
	let mut scores: BTreeMap<&CategoryId, usize> = BTreeMap::new();
	for category in categories.iter().flatten() {
		*scores.entry(category).or_default() += 1;
	}
	let score =
		|category: &Option<CategoryId>| category.as_ref().and_then(|c| scores.get(c).copied());
	let mut winner: Option<(Option<usize>, &Option<CategoryId>)> = None;
	for category in categories {
		let candidate = score(category);
		winner = match winner {
			None => Some((candidate, category)),
			Some((best, _))
				if candidate.is_some() && best.is_some_and(|best| candidate > Some(best)) =>
			{
				Some((candidate, category))
			}
			kept => kept,
		};
	}
	let (score, category) = winner?;
	(score? >= 3).then(|| category.clone()).flatten()
}

fn stored(rule: &Rule) -> Option<(Vec<Value>, Vec<Value>)> {
	let conditions: Vec<Value> = serde_json::from_str(&rule.conditions_json).ok()?;
	let actions: Vec<Value> = serde_json::from_str(&rule.actions_json).ok()?;
	Some((conditions, actions))
}

/// Updates every `payee is <payee> → set category` rule, or adds one unless
/// a `payee oneOf […]` rule already sets that category (§4.5).
fn category_rule(
	data: &BudgetData,
	payee: &PayeeId,
	category: &CategoryId,
) -> Result<Vec<Row>, CoreError> {
	let setter = |action: &Value| action["op"] == "set" && action["field"] == "category";
	let mut rows = Vec::new();
	let mut found = false;
	let mut one_of = false;
	for rule in data
		.rules
		.values()
		.filter(|r| !r.tombstone && r.stage == RuleStage::Default)
	{
		let Some((conditions, actions)) = stored(rule) else {
			continue;
		};
		let [condition] = conditions.as_slice() else {
			continue;
		};
		let [action] = actions.as_slice() else {
			continue;
		};
		if !setter(action) || condition["field"] != "description" {
			continue;
		}
		let op = condition["op"].as_str().unwrap_or_default();
		if (op == "is" || op == "isNot") && condition["value"] == payee.as_str() {
			found = true;
			if action["value"] != category.as_str() {
				let mut action = action.clone();
				action["value"] = Value::String(category.to_string());
				rows.push(Row::Rule(Rule {
					actions_json: Value::Array(vec![action]).to_string(),
					..rule.clone()
				}));
			}
		} else if op == "oneOf"
			&& action["value"] == category.as_str()
			&& condition["value"]
				.as_array()
				.is_some_and(|values| values.iter().any(|v| v == payee.as_str()))
		{
			one_of = true;
		}
	}
	if !found && !one_of {
		rows.push(Row::Rule(Rule {
			id: RuleId::new(veritas_store::new_id()),
			stage: RuleStage::Default,
			conditions_op: veritas_domain::ConditionsOp::And,
			conditions_json: serde_json::json!([
				{"op": "is", "field": "description", "value": payee.as_str(), "type": "id"}
			])
			.to_string(),
			actions_json: serde_json::json!([
				{"op": "set", "field": "category", "value": category.as_str(), "type": "id"}
			])
			.to_string(),
			tombstone: false,
		}));
	}
	Ok(rows)
}
