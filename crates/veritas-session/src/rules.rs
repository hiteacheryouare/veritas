//! The rule editor's writes (R-RULE-2, R-RULE-3): create, edit and delete as
//! Actual's `rule-add`, `rule-update` and `rule-delete` store them
//! (rules/app.ts:100-127, transaction-rules.ts:222-248), and "apply to
//! existing" with its preview (`rule-apply-actions`, transaction-rules.ts:755-834).

use veritas_domain::{
	BudgetData, CoreError, PayeeId, PayeeMapping, Row, Rule, RuleId, SplitRole, Transaction,
	TransactionId,
};
use veritas_rules::{
	BudgetPayees, Draft, RuleActions, RuleBook, RuleTransaction, Value, matching, written,
};

use crate::transactions::{alive, new_payee, sync_transfer};
use crate::writes::applied;

/// A saved rule, or the editor's unsaved draft.
#[derive(Clone, Copy, Debug)]
pub enum Source<'a> {
	Saved(&'a RuleId),
	Draft(&'a Draft),
}

fn live_rule<'a>(data: &'a BudgetData, id: &RuleId) -> Result<&'a Rule, CoreError> {
	data.rules
		.get(id)
		.filter(|rule| !rule.tombstone)
		.ok_or_else(|| CoreError::NotFound {
			entity: "rule",
			id: id.to_string(),
		})
}

fn rule_of(data: &BudgetData, source: Source) -> Result<Rule, CoreError> {
	match source {
		Source::Saved(id) => live_rule(data, id).cloned(),
		Source::Draft(draft) => written(data, RuleId::new(""), draft, None),
	}
}

/// `insertRule`: a new row with a new id (`db.insertWithUUID`).
pub fn create(data: &BudgetData, draft: &Draft) -> Result<(Vec<Row>, RuleId), CoreError> {
	let id = RuleId::new(veritas_store::new_id());
	let rule = written(data, id.clone(), draft, None)?;
	Ok((vec![Row::Rule(rule)], id))
}

/// `updateRule`. A bill's rule, or one the editor can't show, is refused.
pub fn edit(data: &BudgetData, id: &RuleId, draft: &Draft) -> Result<Vec<Row>, CoreError> {
	let existing = live_rule(data, id)?;
	let rule = written(data, id.clone(), draft, Some(existing))?;
	Ok(if &rule == existing {
		Vec::new()
	} else {
		vec![Row::Rule(rule)]
	})
}

/// `deleteRule` (transaction-rules.ts:236-248): refused while any schedule
/// row names the rule, deleted or not; otherwise tombstoned.
pub fn delete(data: &BudgetData, id: &RuleId) -> Result<Vec<Row>, CoreError> {
	let rule = live_rule(data, id)?;
	if data.schedules.values().any(|schedule| &schedule.rule == id) {
		return Err(CoreError::Invalid(
			"This rule belongs to a bill. Delete the bill instead.".to_owned(),
		));
	}
	Ok(vec![Row::Rule(Rule {
		tombstone: true,
		..rule.clone()
	})])
}

/// A row as `select('*')` hands it to `applyActions`: the payee and category
/// come through their mappings.
fn as_ruled(data: &BudgetData, row: &Transaction) -> RuleTransaction {
	RuleTransaction {
		account: row.account.clone(),
		date: row.date,
		amount: row.amount,
		payee: row
			.payee
			.as_ref()
			.and_then(|payee| data.resolve_payee(payee))
			.cloned(),
		category: row
			.category
			.as_ref()
			.and_then(|category| data.resolve_category(category))
			.cloned(),
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

fn cannot_run(reason: &str) -> CoreError {
	CoreError::Invalid(format!(
		"Veritas can't run this rule's actions here because of {reason}. Apply it in Actual."
	))
}

/// `applyActions` on the chosen rows: only the rule's actions, overwriting
/// what's there, then `batchUpdateTransactions` with its transfer hooks. An
/// off-budget row loses its category. A split rule makes the row a split
/// parent with new lines built as `makeChild` builds them
/// (`shared/transactions.ts:33-59`, `:249-267`, `:362-391`). A run Veritas
/// can't finish writes nothing (D60).
pub fn apply(
	data: &BudgetData,
	source: Source,
	ids: &[TransactionId],
	now_ms: f64,
) -> Result<Vec<Row>, CoreError> {
	apply_rule(data, &rule_of(data, source)?, ids, now_ms)
}

fn apply_rule(
	data: &BudgetData,
	rule: &Rule,
	ids: &[TransactionId],
	now_ms: f64,
) -> Result<Vec<Row>, CoreError> {
	let actions = RuleActions::new(data, rule).map_err(|reason| cannot_run(&reason))?;
	let mut payees = BudgetPayees::new(data, || PayeeId::new(veritas_store::new_id()));
	let mut updated = Vec::new();
	for id in ids {
		let row = data
			.transactions
			.get(id)
			.filter(|row| alive(data, row))
			.ok_or_else(|| CoreError::NotFound {
				entity: "transaction",
				id: id.to_string(),
			})?;
		if row.split == SplitRole::Parent {
			return Err(CoreError::Invalid(
				"Rules apply to a split's lines, not the split itself.".to_owned(),
			));
		}
		let mut ruled = as_ruled(data, row);
		actions
			.apply(&mut ruled, &mut payees)
			.map_err(|reason| cannot_run(&reason))?;
		let off_budget = data
			.accounts
			.get(&ruled.account)
			.is_some_and(|account| account.off_budget);
		let split_error = ruled.split_difference()?.map(|difference| {
			format!(
				r#"{{"type":"SplitTransactionError","version":1,"difference":{}}}"#,
				difference.minor()
			)
		});
		let lines = std::mem::take(&mut ruled.splits);
		let split = !lines.is_empty();
		let top = Transaction {
			account: ruled.account,
			date: ruled.date,
			amount: ruled.amount,
			payee: ruled.payee,
			category: if off_budget || split {
				None
			} else {
				ruled.category
			},
			notes: ruled.notes,
			imported_payee: ruled.imported_payee,
			cleared: ruled.cleared.unwrap_or(row.cleared),
			reconciled: ruled.reconciled.unwrap_or(row.reconciled),
			schedule: ruled.schedule,
			tombstone: ruled.deleted,
			split: if split {
				SplitRole::Parent
			} else {
				row.split.clone()
			},
			split_error: if split {
				split_error
			} else {
				row.split_error.clone()
			},
			..row.clone()
		};
		// `splitTransaction` gives the first line `sort_order || -1`, and each
		// line after it one less; that first line is the scratch row Actual drops.
		let first = if row.sort_order == 0.0 {
			-1.0
		} else {
			row.sort_order
		};
		let children: Vec<Transaction> = lines
			.into_iter()
			.zip(1..)
			.map(|(line, n)| Transaction {
				id: TransactionId::new(veritas_store::new_id()),
				amount: line.amount,
				payee: line.payee,
				imported_payee: None,
				category: if off_budget { None } else { line.category },
				notes: line.notes,
				split: SplitRole::Child {
					parent: top.id.clone(),
				},
				transfer: None,
				schedule: line.schedule,
				sort_order: first - f64::from(n),
				tombstone: false,
				split_error: None,
				..top.clone()
			})
			.collect();
		updated.push(top);
		updated.extend(children);
	}
	let mut rows = Vec::new();
	for (id, name) in &payees.created {
		rows.push(Row::Payee(new_payee(id, name)));
		rows.push(Row::PayeeMapping(PayeeMapping {
			from: id.clone(),
			to: id.clone(),
		}));
	}
	rows.extend(updated.iter().cloned().map(Row::Transaction));
	let mut state = applied(data, &rows);
	let book = RuleBook::new(&state);
	for row in &updated {
		let Some(mut current) = state.transactions.get(&row.id).cloned() else {
			continue;
		};
		let hooks = sync_transfer(&state, &mut current, Some(&book), now_ms)?;
		let mut step = vec![Row::Transaction(current)];
		step.extend(hooks);
		state = applied(&state, &step);
		rows.extend(step);
	}
	Ok(rows)
}

/// A field of a matched row that applying the rule would change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangedField {
	Account,
	Date,
	Amount,
	Payee,
	Category,
	Notes,
	ImportedPayee,
	Cleared,
	Reconciled,
	Schedule,
	Deleted,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Change {
	pub field: ChangedField,
	pub before: Value,
	/// A payee the apply would make is its name, as `Value::Text`.
	pub after: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Match {
	pub transaction: Transaction,
	pub changes: Vec<Change>,
	/// The lines a split rule would give it.
	pub lines: Vec<Transaction>,
}

fn changes(
	data: &BudgetData,
	before: &Transaction,
	after: &Transaction,
	made: &[(PayeeId, String)],
) -> Vec<Change> {
	let id = |id: Option<&str>| Value::Id(id.map(str::to_owned));
	let text = |text: &Option<String>| Value::Text(text.clone().unwrap_or_default());
	let payee =
		|payee: &Option<PayeeId>| match made.iter().find(|(id, _)| Some(id) == payee.as_ref()) {
			Some((_, name)) => Value::Text(name.clone()),
			None => id(payee.as_ref().map(PayeeId::as_str)),
		};
	let mapped_payee = before
		.payee
		.as_ref()
		.and_then(|p| data.resolve_payee(p))
		.cloned();
	let mapped_category = before
		.category
		.as_ref()
		.and_then(|c| data.resolve_category(c))
		.cloned();
	let all = [
		(
			ChangedField::Account,
			id(Some(before.account.as_str())),
			id(Some(after.account.as_str())),
		),
		(
			ChangedField::Date,
			Value::Date(before.date.to_string()),
			Value::Date(after.date.to_string()),
		),
		(
			ChangedField::Amount,
			Value::Amount(before.amount.minor()),
			Value::Amount(after.amount.minor()),
		),
		(
			ChangedField::Payee,
			payee(&mapped_payee),
			payee(&after.payee),
		),
		(
			ChangedField::Category,
			id(mapped_category.as_ref().map(|c| c.as_str())),
			id(after.category.as_ref().map(|c| c.as_str())),
		),
		(ChangedField::Notes, text(&before.notes), text(&after.notes)),
		(
			ChangedField::ImportedPayee,
			text(&before.imported_payee),
			text(&after.imported_payee),
		),
		(
			ChangedField::Cleared,
			Value::Bool(before.cleared),
			Value::Bool(after.cleared),
		),
		(
			ChangedField::Reconciled,
			Value::Bool(before.reconciled),
			Value::Bool(after.reconciled),
		),
		(
			ChangedField::Schedule,
			id(before.schedule.as_ref().map(|s| s.as_str())),
			id(after.schedule.as_ref().map(|s| s.as_str())),
		),
		(
			ChangedField::Deleted,
			Value::Bool(before.tombstone),
			Value::Bool(after.tombstone),
		),
	];
	all.into_iter()
		.filter(|(_, before, after)| before != after)
		.map(|(field, before, after)| Change {
			field,
			before,
			after,
		})
		.collect()
}

/// The rows "apply to existing" would offer, newest first, each with what
/// applying the rule would change on it.
pub fn preview(data: &BudgetData, source: Source) -> Result<Vec<Match>, CoreError> {
	let rule = rule_of(data, source)?;
	let ids = matching(data, &rule).map_err(|reason| {
		CoreError::Invalid(format!(
			"Veritas can't search for this rule's matches because of {reason}. Use Actual for this one."
		))
	})?;
	let planned = apply_rule(data, &rule, &ids, 0.0)?;
	let made: Vec<(PayeeId, String)> = planned
		.iter()
		.filter_map(|row| match row {
			Row::Payee(payee) if !data.payees.contains_key(&payee.id) => {
				Some((payee.id.clone(), payee.name.clone()))
			}
			_ => None,
		})
		.collect();
	let after = applied(data, &planned);
	Ok(ids
		.iter()
		.filter_map(|id| {
			let before = data.transactions.get(id)?;
			let now = after.transactions.get(id)?;
			let mut lines: Vec<Transaction> = after
				.transactions
				.values()
				.filter(|line| {
					matches!(&line.split, SplitRole::Child { parent } if parent == id)
						&& !data.transactions.contains_key(&line.id)
				})
				.cloned()
				.collect();
			lines.sort_by(|a, b| b.sort_order.total_cmp(&a.sort_order));
			Some(Match {
				transaction: before.clone(),
				changes: changes(data, before, now, &made),
				lines,
			})
		})
		.collect())
}
