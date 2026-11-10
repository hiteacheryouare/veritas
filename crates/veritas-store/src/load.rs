use std::collections::BTreeMap;

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OptionalExtension, Row};
use veritas_domain::{
	Account, AccountGroup, AccountGroupId, AccountId, AccountKind, AccountMeta, BudgetCell,
	BudgetData, BudgetType, Category, CategoryGroup, CategoryGroupId, CategoryId, CategoryKind,
	CategoryMeta, CivilDate, ConditionsOp, CoreError, Goal, GoalId, GoalStatus, Money, Payee,
	PayeeId, Rule, RuleId, RuleStage, Schedule, ScheduleId, SplitRole, Tag, TagId, Transaction,
	TransactionId, TransactionMeta,
};

use crate::cells::{
	Cell, date, date_from_int, db_month, flag, integer, is_zero, money, required_text, sort_order,
	text, text_month, tombstone, transaction_tombstone,
};
use crate::connection::storage;

/// A row Veritas couldn't represent. It stays in the file untouched; the
/// invariant audit reports it. Carries no amounts, names or notes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkippedRow {
	pub table: &'static str,
	pub id: Option<String>,
	pub reason: &'static str,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoadReport {
	pub skipped: Vec<SkippedRow>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
	pub data: BudgetData,
	pub report: LoadReport,
}

pub(crate) const DUPLICATE_BUDGET_CELL: &str =
	"repeats an earlier row's month and category, and replaces it";

fn at<'r>(row: &'r Row<'_>, index: usize) -> Cell<ValueRef<'r>> {
	row.get_ref(index).map_err(|_| "a column is missing")
}

fn each_row(
	conn: &Connection,
	table: &'static str,
	sql: &str,
	skipped: &mut Vec<SkippedRow>,
	mut read: impl FnMut(&Row<'_>) -> Cell<()>,
) -> Result<(), CoreError> {
	let mut statement = conn.prepare(sql).map_err(storage)?;
	let mut rows = statement.query([]).map_err(storage)?;
	while let Some(row) = rows.next().map_err(storage)? {
		if let Err(reason) = read(row) {
			let id = match row.get_ref(0) {
				Ok(ValueRef::Text(bytes)) => Some(String::from_utf8_lossy(bytes).into_owned()),
				_ => None,
			};
			skipped.push(SkippedRow { table, id, reason });
		}
	}
	Ok(())
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool, CoreError> {
	conn.query_row(
		"SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
		[table],
		|row| row.get(0),
	)
	.map_err(storage)
}

/// Actual treats a file as tracking only when `budgetType` is exactly
/// `'tracking'`; a missing row or any other value is envelope.
pub(crate) fn budget_type(conn: &Connection) -> Result<BudgetType, CoreError> {
	if !table_exists(conn, "preferences")? {
		return Ok(BudgetType::Envelope);
	}
	let value: Option<Option<String>> = conn
		.query_row(
			"SELECT CAST(value AS TEXT) FROM preferences WHERE id = 'budgetType'",
			[],
			|row| row.get(0),
		)
		.optional()
		.map_err(storage)?;
	Ok(match value.flatten().as_deref() {
		Some("tracking") => BudgetType::Tracking,
		_ => BudgetType::Envelope,
	})
}

fn id(row: &Row<'_>, index: usize) -> Cell<String> {
	required_text(at(row, index)?, "the row has no id")
}

pub fn load(conn: &Connection) -> Result<Loaded, CoreError> {
	let mut data = BudgetData::empty(budget_type(conn)?);
	let mut skipped = Vec::new();

	each_row(
		conn,
		"accounts",
		"SELECT id, name, offbudget, closed, account_group_id, sort_order, tombstone FROM accounts",
		&mut skipped,
		|row| {
			let id = AccountId::new(id(row, 0)?);
			let account = Account {
				id: id.clone(),
				name: text(at(row, 1)?)?.unwrap_or_default(),
				off_budget: flag(at(row, 2)?),
				closed: flag(at(row, 3)?),
				group: text(at(row, 4)?)?.map(AccountGroupId::new),
				sort_order: sort_order(at(row, 5)?)?,
				tombstone: tombstone(at(row, 6)?),
			};
			data.accounts.insert(id, account);
			Ok(())
		},
	)?;

	each_row(
		conn,
		"account_groups",
		"SELECT id, name, sort_order, tombstone FROM account_groups",
		&mut skipped,
		|row| {
			let id = AccountGroupId::new(id(row, 0)?);
			let group = AccountGroup {
				id: id.clone(),
				name: text(at(row, 1)?)?.unwrap_or_default(),
				sort_order: sort_order(at(row, 2)?)?,
				tombstone: tombstone(at(row, 3)?),
			};
			data.account_groups.insert(id, group);
			Ok(())
		},
	)?;

	each_row(
		conn,
		"category_groups",
		"SELECT id, name, is_income, hidden, sort_order, tombstone FROM category_groups",
		&mut skipped,
		|row| {
			let id = CategoryGroupId::new(id(row, 0)?);
			let group = CategoryGroup {
				id: id.clone(),
				name: text(at(row, 1)?)?.unwrap_or_default(),
				is_income: flag(at(row, 2)?),
				hidden: flag(at(row, 3)?),
				sort_order: sort_order(at(row, 4)?)?,
				tombstone: tombstone(at(row, 5)?),
			};
			data.category_groups.insert(id, group);
			Ok(())
		},
	)?;

	each_row(
		conn,
		"categories",
		"SELECT id, name, cat_group, is_income, hidden, sort_order, tombstone FROM categories",
		&mut skipped,
		|row| {
			let id = CategoryId::new(id(row, 0)?);
			let category = Category {
				id: id.clone(),
				name: text(at(row, 1)?)?.unwrap_or_default(),
				group: CategoryGroupId::new(required_text(
					at(row, 2)?,
					"the category has no group",
				)?),
				is_income: flag(at(row, 3)?),
				hidden: flag(at(row, 4)?),
				sort_order: sort_order(at(row, 5)?)?,
				tombstone: tombstone(at(row, 6)?),
			};
			data.categories.insert(id, category);
			Ok(())
		},
	)?;

	each_row(
		conn,
		"payees",
		"SELECT id, name, transfer_acct, favorite, learn_categories, tombstone FROM payees",
		&mut skipped,
		|row| {
			let id = PayeeId::new(id(row, 0)?);
			let payee = Payee {
				id: id.clone(),
				name: text(at(row, 1)?)?.unwrap_or_default(),
				transfer_account: text(at(row, 2)?)?.map(AccountId::new),
				favorite: flag(at(row, 3)?),
				learn_categories: flag(at(row, 4)?),
				tombstone: tombstone(at(row, 5)?),
			};
			data.payees.insert(id, payee);
			Ok(())
		},
	)?;

	each_row(
		conn,
		"transactions",
		"SELECT id, acct, date, amount, description, category, notes, cleared, reconciled, \
		 isParent, isChild, parent_id, transferred_id, schedule, starting_balance_flag, \
		 sort_order, tombstone, imported_description FROM transactions",
		&mut skipped,
		|row| {
			let transaction = transaction(row)?;
			data.transactions
				.insert(transaction.id.clone(), transaction);
			Ok(())
		},
	)?;

	let budget_table = match data.budget_type {
		BudgetType::Envelope => "zero_budgets",
		BudgetType::Tracking => "reflect_budgets",
	};
	each_row(
		conn,
		budget_table,
		&format!(
			"SELECT id, month, category, amount, carryover, goal, long_goal FROM {budget_table} ORDER BY rowid"
		),
		&mut skipped,
		|row| {
			let cell = BudgetCell {
				month: db_month(at(row, 1)?)?.ok_or("the budget row has no month")?,
				category: CategoryId::new(required_text(
					at(row, 2)?,
					"the budget row has no category",
				)?),
				budgeted: money(at(row, 3)?)?.unwrap_or(Money::ZERO),
				carryover: flag(at(row, 4)?),
				goal: money(at(row, 5)?)?,
				long_goal: flag(at(row, 6)?),
			};
			// Rows are matched by (month, category) and never by id, because
			// Actual wrote two id formats over the years. A repeated pair is
			// corrupt; the later row wins, as in Actual's sheet, and is the
			// one reported.
			let shadowed = data
				.budget_cells
				.insert((cell.month, cell.category.clone()), cell);
			match shadowed {
				Some(_) => Err(DUPLICATE_BUDGET_CELL),
				None => Ok(()),
			}
		},
	)?;

	// Holds are an envelope feature; Actual doesn't read them for tracking
	// budgets.
	if data.budget_type == BudgetType::Envelope {
		each_row(
			conn,
			"zero_budget_months",
			"SELECT id, buffered FROM zero_budget_months",
			&mut skipped,
			|row| {
				let month = text_month(at(row, 0)?)?.ok_or("the row has no id")?;
				let held = money(at(row, 1)?)?;
				data.holds.insert(month, held);
				Ok(())
			},
		)?;
	}

	each_row(
		conn,
		"rules",
		"SELECT id, stage, conditions_op, conditions, actions, tombstone FROM rules",
		&mut skipped,
		|row| {
			let id = RuleId::new(id(row, 0)?);
			let rule = Rule {
				id: id.clone(),
				stage: match text(at(row, 1)?)?.as_deref() {
					// Actual's loadRules still reads two retired stage names as pre.
					Some("pre" | "cleanup" | "modify") => RuleStage::Pre,
					Some("post") => RuleStage::Post,
					_ => RuleStage::Default,
				},
				conditions_op: match text(at(row, 2)?)?.as_deref() {
					Some("or") => ConditionsOp::Or,
					_ => ConditionsOp::And,
				},
				conditions_json: required_text(at(row, 3)?, "the rule has no conditions")?,
				actions_json: required_text(at(row, 4)?, "the rule has no actions")?,
				tombstone: tombstone(at(row, 5)?),
			};
			data.rules.insert(id, rule);
			Ok(())
		},
	)?;

	let next_dates = next_dates(conn, &mut skipped)?;
	each_row(
		conn,
		"schedules",
		"SELECT id, name, rule, posts_transaction, completed, tombstone FROM schedules",
		&mut skipped,
		|row| {
			let id = ScheduleId::new(id(row, 0)?);
			let schedule = Schedule {
				next_date: next_dates.get(&id).and_then(|next| next.date),
				id: id.clone(),
				name: text(at(row, 1)?)?,
				rule: RuleId::new(required_text(at(row, 2)?, "the schedule has no rule")?),
				posts_transaction: flag(at(row, 3)?),
				completed: flag(at(row, 4)?),
				tombstone: tombstone(at(row, 5)?),
			};
			data.schedules.insert(id, schedule);
			Ok(())
		},
	)?;

	each_row(
		conn,
		"tags",
		"SELECT id, tag, color, description, hidden, tombstone FROM tags",
		&mut skipped,
		|row| {
			let id = TagId::new(id(row, 0)?);
			let tag = Tag {
				id: id.clone(),
				name: required_text(at(row, 1)?, "the tag has no text")?,
				color: text(at(row, 2)?)?,
				description: text(at(row, 3)?)?,
				hidden: flag(at(row, 4)?),
				tombstone: tombstone(at(row, 5)?),
			};
			data.tags.insert(id, tag);
			Ok(())
		},
	)?;

	each_row(
		conn,
		"notes",
		"SELECT id, note FROM notes",
		&mut skipped,
		|row| {
			let id = id(row, 0)?;
			data.notes.insert(id, text(at(row, 1)?)?);
			Ok(())
		},
	)?;

	each_row(
		conn,
		"category_mapping",
		"SELECT id, transferId FROM category_mapping",
		&mut skipped,
		|row| {
			let from = CategoryId::new(id(row, 0)?);
			let to = required_text(at(row, 1)?, "the mapping has no target")?;
			data.category_mapping.insert(from, CategoryId::new(to));
			Ok(())
		},
	)?;

	each_row(
		conn,
		"payee_mapping",
		"SELECT id, targetId FROM payee_mapping",
		&mut skipped,
		|row| {
			let from = PayeeId::new(id(row, 0)?);
			let to = required_text(at(row, 1)?, "the mapping has no target")?;
			data.payee_mapping.insert(from, PayeeId::new(to));
			Ok(())
		},
	)?;

	load_veritas_tables(conn, &mut data, &mut skipped)?;

	Ok(Loaded {
		data,
		report: LoadReport { skipped },
	})
}

fn transaction(row: &Row<'_>) -> Cell<Transaction> {
	let id = TransactionId::new(id(row, 0)?);
	let is_parent = flag(at(row, 9)?);
	// Actual's views keep a row only when isChild = 0, or when it is 1 and
	// its parent is alive, so any other value never reaches a total.
	let is_child = match at(row, 10)? {
		ValueRef::Null => return Err("the split-child flag is NULL"),
		value if flag(value) => true,
		value if is_zero(value) => false,
		_ => return Err("the split-child flag isn't 0 or 1"),
	};
	let split = match (is_parent, is_child) {
		(true, true) => return Err("the row is marked both split parent and split child"),
		(_, true) => SplitRole::Child {
			parent: TransactionId::new(required_text(
				at(row, 11)?,
				"the split child has no parent",
			)?),
		},
		(true, false) => SplitRole::Parent,
		(false, false) => SplitRole::None,
	};
	Ok(Transaction {
		id,
		account: AccountId::new(required_text(
			at(row, 1)?,
			"the transaction has no account",
		)?),
		date: date(at(row, 2)?)?.ok_or("the transaction has no date")?,
		amount: money(at(row, 3)?)?.unwrap_or(Money::ZERO),
		payee: text(at(row, 4)?)?.map(PayeeId::new),
		imported_payee: text(at(row, 17)?)?,
		// Actual always reads a split parent's category as NULL.
		category: if is_parent {
			None
		} else {
			text(at(row, 5)?)?.map(CategoryId::new)
		},
		notes: text(at(row, 6)?)?,
		cleared: flag(at(row, 7)?),
		reconciled: flag(at(row, 8)?),
		split,
		transfer: text(at(row, 12)?)?.map(TransactionId::new),
		schedule: text(at(row, 13)?)?.map(ScheduleId::new),
		starting_balance: flag(at(row, 14)?),
		sort_order: sort_order(at(row, 15)?)?,
		tombstone: transaction_tombstone(at(row, 16)?),
	})
}

struct NextDate {
	date: Option<CivilDate>,
	tombstone: bool,
}

/// `v_schedules.next_date`: the local date when both timestamps are set and
/// equal, otherwise the base date.
fn next_dates(
	conn: &Connection,
	skipped: &mut Vec<SkippedRow>,
) -> Result<BTreeMap<ScheduleId, NextDate>, CoreError> {
	let mut dates: BTreeMap<ScheduleId, NextDate> = BTreeMap::new();
	each_row(
		conn,
		"schedules_next_date",
		"SELECT id, schedule_id, local_next_date, local_next_date_ts, base_next_date, \
		 base_next_date_ts, tombstone FROM schedules_next_date ORDER BY rowid",
		skipped,
		|row| {
			let schedule =
				ScheduleId::new(required_text(at(row, 1)?, "the row names no schedule")?);
			let local_ts = integer(at(row, 3)?)?;
			let base_ts = integer(at(row, 5)?)?;
			let chosen = if local_ts.is_some() && local_ts == base_ts {
				at(row, 2)?
			} else {
				at(row, 4)?
			};
			let next = NextDate {
				date: integer(chosen)?.map(date_from_int).transpose()?,
				tombstone: tombstone(at(row, 6)?),
			};
			let replace = match dates.get(&schedule) {
				None => true,
				Some(existing) => existing.tombstone && !next.tombstone,
			};
			if replace {
				dates.insert(schedule, next);
			}
			Ok(())
		},
	)?;
	Ok(dates)
}

fn category_kind(text: &str) -> Cell<CategoryKind> {
	match text {
		"income" => Ok(CategoryKind::Income),
		"expense" => Ok(CategoryKind::Expense),
		"debt_principal" => Ok(CategoryKind::DebtPrincipal),
		"savings" => Ok(CategoryKind::Savings),
		_ => Err("unknown category kind"),
	}
}

fn load_veritas_tables(
	conn: &Connection,
	data: &mut BudgetData,
	skipped: &mut Vec<SkippedRow>,
) -> Result<(), CoreError> {
	if table_exists(conn, "veritas_category_meta")? {
		each_row(
			conn,
			"veritas_category_meta",
			"SELECT category_id, kind, tax_bucket, archived_on FROM veritas_category_meta",
			skipped,
			|row| {
				let category = CategoryId::new(id(row, 0)?);
				let meta = CategoryMeta {
					kind: text(at(row, 1)?)?
						.map(|kind| category_kind(&kind))
						.transpose()?,
					tax_bucket: text(at(row, 2)?)?,
					archived_on: date(at(row, 3)?)?,
				};
				data.category_meta.insert(category, meta);
				Ok(())
			},
		)?;
	}

	if table_exists(conn, "veritas_account_meta")? {
		each_row(
			conn,
			"veritas_account_meta",
			"SELECT account_id, kind, loan_rate_bps, loan_payment, loan_payment_day, lender \
			 FROM veritas_account_meta",
			skipped,
			|row| {
				let account = AccountId::new(id(row, 0)?);
				let meta = AccountMeta {
					kind: match text(at(row, 1)?)?.as_deref() {
						None => None,
						Some("asset") => Some(AccountKind::Asset),
						Some("liability") => Some(AccountKind::Liability),
						Some(_) => return Err("unknown account kind"),
					},
					loan_rate_bps: integer(at(row, 2)?)?,
					loan_payment: money(at(row, 3)?)?,
					loan_payment_day: integer(at(row, 4)?)?
						.map(|day| {
							u8::try_from(day)
								.ok()
								.filter(|day| (1..=31).contains(day))
								.ok_or("the loan payment day isn't 1 to 31")
						})
						.transpose()?,
					lender: text(at(row, 5)?)?,
				};
				data.account_meta.insert(account, meta);
				Ok(())
			},
		)?;
	}

	if table_exists(conn, "veritas_transaction_meta")? {
		each_row(
			conn,
			"veritas_transaction_meta",
			"SELECT transaction_id, adjustment, needs_actual FROM veritas_transaction_meta",
			skipped,
			|row| {
				let transaction = TransactionId::new(id(row, 0)?);
				let meta = TransactionMeta {
					adjustment: flag(at(row, 1)?),
					needs_actual: text(at(row, 2)?)?,
				};
				data.transaction_meta.insert(transaction, meta);
				Ok(())
			},
		)?;
	}

	if table_exists(conn, "veritas_tax_tag")? {
		each_row(
			conn,
			"veritas_tax_tag",
			"SELECT tag, bucket FROM veritas_tax_tag",
			skipped,
			|row| {
				let tag = required_text(at(row, 0)?, "the row has no tag")?;
				let bucket = required_text(at(row, 1)?, "the tag has no bucket")?;
				data.tax_tags.insert(tag, bucket);
				Ok(())
			},
		)?;
	}

	if table_exists(conn, "veritas_goal")? {
		each_row(
			conn,
			"veritas_goal",
			"SELECT id, category_id, target_cents, target_date, icon, color, status, \
			 reached_month, closed_month, released_to_category_id, released_cents FROM veritas_goal",
			skipped,
			|row| {
				let id = GoalId::new(id(row, 0)?);
				let goal = Goal {
					id: id.clone(),
					category: CategoryId::new(required_text(
						at(row, 1)?,
						"the goal has no category",
					)?),
					target: money(at(row, 2)?)?.ok_or("the goal has no target")?,
					target_date: date(at(row, 3)?)?,
					icon: text(at(row, 4)?)?,
					color: text(at(row, 5)?)?,
					status: match required_text(at(row, 6)?, "the goal has no status")?.as_str() {
						"active" => GoalStatus::Active,
						"reached" => GoalStatus::Reached,
						"closed" => GoalStatus::Closed,
						_ => return Err("unknown goal status"),
					},
					reached_month: text_month(at(row, 7)?)?,
					closed_month: text_month(at(row, 8)?)?,
					released_to: text(at(row, 9)?)?.map(CategoryId::new),
					released: money(at(row, 10)?)?,
				};
				data.goals.insert(id, goal);
				Ok(())
			},
		)?;
	}
	Ok(())
}
