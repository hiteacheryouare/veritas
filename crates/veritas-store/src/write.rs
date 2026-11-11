use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::types::Value;
use rusqlite::{OptionalExtension, Transaction as SqlTransaction, params_from_iter};
use veritas_domain::{
	Account, AccountGroup, AccountId, AccountKind, AccountMeta, BudgetCell, BudgetType, Category,
	CategoryGroup, CategoryId, CategoryKind, CategoryMeta, CivilDate, CoreError, Goal, GoalStatus,
	Money, Payee, PayeeId, Rule, RuleStage, Schedule, SplitRole, Tag, Transaction, TransactionId,
	TransactionMeta, YearMonth,
};

use crate::connection::storage;
use crate::load::{BUDGET_CELL_PICK, NEXT_DATE_PICK};

/// A row as the columns Veritas models, in a fixed order per table.
type Columns = Vec<(&'static str, Value)>;

fn flag(value: bool) -> Value {
	Value::Integer(i64::from(value))
}

fn text(value: Option<&str>) -> Value {
	value.map_or(Value::Null, |text| Value::Text(text.to_owned()))
}

fn owned(value: &str) -> Value {
	Value::Text(value.to_owned())
}

fn money(value: Option<Money>) -> Value {
	value.map_or(Value::Null, |amount| Value::Integer(amount.minor()))
}

fn date(value: Option<CivilDate>) -> Value {
	value.map_or(Value::Null, |date| {
		Value::Integer(
			i64::from(date.year()) * 10_000 + i64::from(date.month()) * 100 + i64::from(date.day()),
		)
	})
}

fn db_month(month: YearMonth) -> i64 {
	i64::from(month.year()) * 100 + i64::from(month.month())
}

fn text_month(value: Option<YearMonth>) -> Value {
	value.map_or(Value::Null, |month| Value::Text(month.to_string()))
}

fn now_ms() -> i64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
		.unwrap_or_default()
}

fn account_columns(row: &Account) -> Columns {
	vec![
		("name", owned(&row.name)),
		("offbudget", flag(row.off_budget)),
		("closed", flag(row.closed)),
		(
			"account_group_id",
			text(row.group.as_ref().map(|id| id.as_str())),
		),
		("sort_order", Value::Real(row.sort_order)),
		("tombstone", flag(row.tombstone)),
	]
}

fn account_group_columns(row: &AccountGroup) -> Columns {
	vec![
		("name", owned(&row.name)),
		("sort_order", Value::Real(row.sort_order)),
		("tombstone", flag(row.tombstone)),
	]
}

fn category_group_columns(row: &CategoryGroup) -> Columns {
	vec![
		("name", owned(&row.name)),
		("is_income", flag(row.is_income)),
		("hidden", flag(row.hidden)),
		("sort_order", Value::Real(row.sort_order)),
		("tombstone", flag(row.tombstone)),
	]
}

fn category_columns(row: &Category) -> Columns {
	vec![
		("name", owned(&row.name)),
		("cat_group", owned(row.group.as_str())),
		("is_income", flag(row.is_income)),
		("hidden", flag(row.hidden)),
		("sort_order", Value::Real(row.sort_order)),
		("tombstone", flag(row.tombstone)),
	]
}

fn payee_columns(row: &Payee) -> Columns {
	vec![
		("name", owned(&row.name)),
		(
			"transfer_acct",
			text(row.transfer_account.as_ref().map(|id| id.as_str())),
		),
		("favorite", flag(row.favorite)),
		("learn_categories", flag(row.learn_categories)),
		("tombstone", flag(row.tombstone)),
	]
}

fn transaction_columns(row: &Transaction) -> Columns {
	let (is_parent, is_child, parent) = match &row.split {
		SplitRole::None => (false, false, None),
		SplitRole::Parent => (true, false, None),
		SplitRole::Child { parent } => (false, true, Some(parent.as_str())),
	};
	vec![
		("acct", owned(row.account.as_str())),
		("date", date(Some(row.date))),
		("amount", Value::Integer(row.amount.minor())),
		(
			"description",
			text(row.payee.as_ref().map(|id| id.as_str())),
		),
		("imported_description", text(row.imported_payee.as_deref())),
		(
			"category",
			text(row.category.as_ref().map(|id| id.as_str())),
		),
		("notes", text(row.notes.as_deref())),
		("cleared", flag(row.cleared)),
		("reconciled", flag(row.reconciled)),
		("isParent", flag(is_parent)),
		("isChild", flag(is_child)),
		("parent_id", text(parent)),
		(
			"transferred_id",
			text(row.transfer.as_ref().map(|id| id.as_str())),
		),
		(
			"schedule",
			text(row.schedule.as_ref().map(|id| id.as_str())),
		),
		("starting_balance_flag", flag(row.starting_balance)),
		("sort_order", Value::Real(row.sort_order)),
		("tombstone", flag(row.tombstone)),
	]
}

fn rule_columns(row: &Rule) -> Columns {
	vec![
		(
			"stage",
			match row.stage {
				RuleStage::Pre => owned("pre"),
				RuleStage::Default => Value::Null,
				RuleStage::Post => owned("post"),
			},
		),
		(
			"conditions_op",
			owned(match row.conditions_op {
				veritas_domain::ConditionsOp::And => "and",
				veritas_domain::ConditionsOp::Or => "or",
			}),
		),
		("conditions", owned(&row.conditions_json)),
		("actions", owned(&row.actions_json)),
		("tombstone", flag(row.tombstone)),
	]
}

fn schedule_columns(row: &Schedule) -> Columns {
	vec![
		("name", text(row.name.as_deref())),
		("rule", owned(row.rule.as_str())),
		("posts_transaction", flag(row.posts_transaction)),
		("completed", flag(row.completed)),
		("tombstone", flag(row.tombstone)),
	]
}

fn tag_columns(row: &Tag) -> Columns {
	vec![
		("tag", owned(&row.name)),
		("color", text(row.color.as_deref())),
		("description", text(row.description.as_deref())),
		("hidden", flag(row.hidden)),
		("tombstone", flag(row.tombstone)),
	]
}

fn budget_cell_columns(row: &BudgetCell) -> Columns {
	vec![
		("amount", Value::Integer(row.budgeted.minor())),
		("carryover", flag(row.carryover)),
		("goal", money(row.goal)),
		("long_goal", flag(row.long_goal)),
	]
}

fn category_kind(kind: CategoryKind) -> &'static str {
	match kind {
		CategoryKind::Income => "income",
		CategoryKind::Expense => "expense",
		CategoryKind::DebtPrincipal => "debt_principal",
		CategoryKind::Savings => "savings",
	}
}

fn category_meta_columns(row: &CategoryMeta) -> Columns {
	vec![
		("kind", text(row.kind.map(category_kind))),
		("tax_bucket", text(row.tax_bucket.as_deref())),
		("archived_on", date(row.archived_on)),
	]
}

fn account_meta_columns(row: &AccountMeta) -> Columns {
	vec![
		(
			"kind",
			text(row.kind.map(|kind| match kind {
				AccountKind::Asset => "asset",
				AccountKind::Liability => "liability",
			})),
		),
		(
			"loan_rate_bps",
			row.loan_rate_bps.map_or(Value::Null, Value::Integer),
		),
		("loan_payment", money(row.loan_payment)),
		(
			"loan_payment_day",
			row.loan_payment_day
				.map_or(Value::Null, |day| Value::Integer(i64::from(day))),
		),
		("lender", text(row.lender.as_deref())),
	]
}

fn transaction_meta_columns(row: &TransactionMeta) -> Columns {
	vec![
		("adjustment", flag(row.adjustment)),
		("needs_actual", text(row.needs_actual.as_deref())),
	]
}

fn goal_columns(row: &Goal) -> Columns {
	vec![
		("category_id", owned(row.category.as_str())),
		("target_cents", Value::Integer(row.target.minor())),
		("target_date", date(row.target_date)),
		("icon", text(row.icon.as_deref())),
		("color", text(row.color.as_deref())),
		(
			"status",
			owned(match row.status {
				GoalStatus::Active => "active",
				GoalStatus::Reached => "reached",
				GoalStatus::Closed => "closed",
			}),
		),
		("reached_month", text_month(row.reached_month)),
		("closed_month", text_month(row.closed_month)),
		(
			"released_to_category_id",
			text(row.released_to.as_ref().map(|id| id.as_str())),
		),
		("released_cents", money(row.released)),
	]
}

fn quote(name: &str) -> String {
	format!("\"{}\"", name.replace('"', "\"\""))
}

/// One SQLite transaction of row writes, in Actual's style: an insert names
/// only the columns Veritas has a value for, so every other column keeps its
/// SQL default, and an update sets only the columns that changed, so columns
/// Veritas doesn't know about survive. Actual rows are never deleted
/// or replaced; a delete is `tombstone = 1`.
pub struct Writer<'conn> {
	tx: SqlTransaction<'conn>,
	budget_type: BudgetType,
	new_categories: BTreeSet<CategoryId>,
	new_payees: BTreeSet<PayeeId>,
	new_accounts: BTreeSet<AccountId>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Nulls {
	/// Leave NULL columns out of an insert, as Actual's `convertForInsert`
	/// does, so the SQL default applies.
	Skip,
	/// Write NULL explicitly, for rows whose NULL means something.
	Write,
}

fn invalid(message: &str) -> CoreError {
	CoreError::Invalid(message.to_owned())
}

impl<'conn> Writer<'conn> {
	pub(crate) fn new(tx: SqlTransaction<'conn>, budget_type: BudgetType) -> Self {
		Writer {
			tx,
			budget_type,
			new_categories: BTreeSet::new(),
			new_payees: BTreeSet::new(),
			new_accounts: BTreeSet::new(),
		}
	}

	fn insert(
		&self,
		table: &str,
		key: &[(&str, Value)],
		columns: &Columns,
		nulls: Nulls,
	) -> Result<(), CoreError> {
		let written: Vec<&(&str, Value)> = key
			.iter()
			.chain(columns.iter())
			.filter(|(_, value)| nulls == Nulls::Write || *value != Value::Null)
			.collect();
		let names = written
			.iter()
			.map(|(name, _)| quote(name))
			.collect::<Vec<_>>()
			.join(", ");
		let marks = vec!["?"; written.len()].join(", ");
		self.tx
			.execute(
				&format!("INSERT INTO {} ({names}) VALUES ({marks})", quote(table)),
				params_from_iter(written.iter().map(|(_, value)| value)),
			)
			.map_err(storage)?;
		Ok(())
	}

	/// Sets the columns whose value differs between `before` and `after`.
	fn update(
		&self,
		table: &str,
		key: &[(&str, Value)],
		before: &Columns,
		after: &Columns,
	) -> Result<(), CoreError> {
		let changed: Vec<&(&str, Value)> = after
			.iter()
			.zip(before)
			.filter(|(new, old)| new.1 != old.1)
			.map(|(new, _)| new)
			.collect();
		if changed.is_empty() {
			return Ok(());
		}
		let sets = changed
			.iter()
			.map(|(name, _)| format!("{} = ?", quote(name)))
			.collect::<Vec<_>>()
			.join(", ");
		let wheres = key
			.iter()
			.map(|(name, _)| format!("{} = ?", quote(name)))
			.collect::<Vec<_>>()
			.join(" AND ");
		let values = changed
			.iter()
			.map(|(_, value)| value)
			.chain(key.iter().map(|(_, value)| value));
		let updated = self
			.tx
			.execute(
				&format!("UPDATE {} SET {sets} WHERE {wheres}", quote(table)),
				params_from_iter(values),
			)
			.map_err(storage)?;
		if updated == 1 {
			Ok(())
		} else {
			Err(CoreError::NotFound {
				entity: "row",
				id: format!(
					"{table} {}",
					key.first()
						.map(|(_, value)| format!("{value:?}"))
						.unwrap_or_default()
				),
			})
		}
	}

	/// The shared path for Actual's tombstoned tables.
	fn put_actual<T>(
		&self,
		table: &str,
		id: &str,
		before: Option<&T>,
		after: Option<&T>,
		columns: fn(&T) -> Columns,
	) -> Result<bool, CoreError> {
		let key = [("id", owned(id))];
		match (before, after) {
			(_, None) => Err(invalid("Actual rows are tombstoned, never deleted")),
			(None, Some(after)) => self
				.insert(table, &key, &columns(after), Nulls::Skip)
				.map(|()| true),
			(Some(before), Some(after)) => self
				.update(table, &key, &columns(before), &columns(after))
				.map(|()| false),
		}
	}

	/// For veritas_* rows, which Veritas owns outright and may delete.
	fn put_veritas<T: ?Sized>(
		&self,
		table: &str,
		key_column: &'static str,
		id: &str,
		before: Option<&T>,
		after: Option<&T>,
		columns: fn(&T) -> Columns,
	) -> Result<(), CoreError> {
		let key = [(key_column, owned(id))];
		match (before, after) {
			(None, None) => Ok(()),
			(Some(_), None) => self
				.tx
				.execute(
					&format!(
						"DELETE FROM {} WHERE {} = ?1",
						quote(table),
						quote(key_column)
					),
					[id],
				)
				.map(|_| ())
				.map_err(storage),
			(None, Some(after)) => self.insert(table, &key, &columns(after), Nulls::Write),
			(Some(before), Some(after)) => {
				self.update(table, &key, &columns(before), &columns(after))
			}
		}
	}

	pub fn put_account(
		&mut self,
		before: Option<&Account>,
		after: Option<&Account>,
	) -> Result<(), CoreError> {
		let id = before
			.or(after)
			.map(|row| row.id.clone())
			.ok_or_else(|| invalid("no account"))?;
		if self.put_actual("accounts", id.as_str(), before, after, account_columns)? {
			self.new_accounts.insert(id);
		}
		Ok(())
	}

	pub fn put_account_group(
		&mut self,
		before: Option<&AccountGroup>,
		after: Option<&AccountGroup>,
	) -> Result<(), CoreError> {
		let id = before
			.or(after)
			.map(|row| row.id.as_str().to_owned())
			.unwrap_or_default();
		self.put_actual("account_groups", &id, before, after, account_group_columns)
			.map(|_| ())
	}

	pub fn put_category_group(
		&mut self,
		before: Option<&CategoryGroup>,
		after: Option<&CategoryGroup>,
	) -> Result<(), CoreError> {
		let id = before
			.or(after)
			.map(|row| row.id.as_str().to_owned())
			.unwrap_or_default();
		self.put_actual(
			"category_groups",
			&id,
			before,
			after,
			category_group_columns,
		)
		.map(|_| ())
	}

	pub fn put_category(
		&mut self,
		before: Option<&Category>,
		after: Option<&Category>,
	) -> Result<(), CoreError> {
		let id = before
			.or(after)
			.map(|row| row.id.clone())
			.ok_or_else(|| invalid("no category"))?;
		let key = [("id", owned(id.as_str()))];
		match (before, after) {
			(None, Some(after)) => {
				let mut columns = category_columns(after);
				// The AQL default, which differs from the SQL one by a space.
				columns.push(("template_settings", owned("{\"source\":\"notes\"}")));
				self.insert("categories", &key, &columns, Nulls::Skip)?;
				self.new_categories.insert(id);
				Ok(())
			}
			_ => self
				.put_actual("categories", id.as_str(), before, after, category_columns)
				.map(|_| ()),
		}
	}

	pub fn put_payee(
		&mut self,
		before: Option<&Payee>,
		after: Option<&Payee>,
	) -> Result<(), CoreError> {
		let id = before
			.or(after)
			.map(|row| row.id.clone())
			.ok_or_else(|| invalid("no payee"))?;
		if self.put_actual("payees", id.as_str(), before, after, payee_columns)? {
			self.new_payees.insert(id);
		}
		Ok(())
	}

	pub fn put_transaction(
		&mut self,
		before: Option<&Transaction>,
		after: Option<&Transaction>,
	) -> Result<(), CoreError> {
		let id = before
			.or(after)
			.map(|row| row.id.as_str().to_owned())
			.unwrap_or_default();
		self.put_actual("transactions", &id, before, after, transaction_columns)?;
		if let (Some(before), Some(after)) = (before, after)
			&& before.split == SplitRole::Parent
			&& after.split != SplitRole::Parent
		{
			// Memory reads a parent's category as NULL, the way Actual's views
			// mask it, so the column may still hold an old value. Once the row
			// stops being a split it must hold what memory says.
			self.tx
				.execute(
					"UPDATE transactions SET category = ?1 WHERE id = ?2",
					params_from_iter([
						text(after.category.as_ref().map(|id| id.as_str())),
						owned(&id),
					]),
				)
				.map_err(storage)?;
		}
		Ok(())
	}

	pub fn put_rule(
		&mut self,
		before: Option<&Rule>,
		after: Option<&Rule>,
	) -> Result<(), CoreError> {
		let id = before
			.or(after)
			.map(|row| row.id.as_str().to_owned())
			.unwrap_or_default();
		self.put_actual("rules", &id, before, after, rule_columns)
			.map(|_| ())
	}

	/// `next_date` lives in `schedules_next_date`. A changed date is written the
	/// way Actual's `setNextDate` writes one without `reset`: into the local
	/// pair, with the base timestamp, so `v_schedules` picks it.
	pub fn put_schedule(
		&mut self,
		before: Option<&Schedule>,
		after: Option<&Schedule>,
	) -> Result<(), CoreError> {
		let id = before
			.or(after)
			.map(|row| row.id.as_str().to_owned())
			.unwrap_or_default();
		self.put_actual("schedules", &id, before, after, schedule_columns)?;
		let Some(after) = after else {
			return Ok(());
		};
		if before.map(|row| row.next_date) == Some(after.next_date) {
			return Ok(());
		}
		let next = date(after.next_date);
		let existing: Option<(String, Option<i64>)> = self
			.tx
			.query_row(
				&format!(
					"SELECT id, base_next_date_ts FROM schedules_next_date WHERE schedule_id = ?1 \
					 ORDER BY {NEXT_DATE_PICK} LIMIT 1"
				),
				[&id],
				|row| Ok((row.get(0)?, row.get(1)?)),
			)
			.optional()
			.map_err(storage)?;
		match existing {
			Some((row_id, Some(base_ts))) => self
				.tx
				.execute(
					"UPDATE schedules_next_date SET local_next_date = ?1, local_next_date_ts = ?2 WHERE id = ?3",
					params_from_iter([&next, &Value::Integer(base_ts), &owned(&row_id)]),
				)
				.map(|_| ())
				.map_err(storage),
			Some((row_id, None)) => {
				let now = Value::Integer(now_ms());
				self.tx
					.execute(
						"UPDATE schedules_next_date SET local_next_date = ?1, local_next_date_ts = ?2, \
						 base_next_date = ?1, base_next_date_ts = ?2 WHERE id = ?3",
						params_from_iter([&next, &now, &owned(&row_id)]),
					)
					.map(|_| ())
					.map_err(storage)
			}
			None => {
				let row_id: String = self
					.tx
					.query_row(
						"SELECT lower(substr(h, 1, 8) || '-' || substr(h, 9, 4) || '-4' || substr(h, 14, 3) || '-' || \
						 substr('89ab', 1 + (abs(random()) % 4), 1) || substr(h, 18, 3) || '-' || substr(h, 21, 12)) \
						 FROM (SELECT hex(randomblob(16)) AS h)",
						[],
						|row| row.get(0),
					)
					.map_err(storage)?;
				let now = Value::Integer(now_ms());
				self.insert(
					"schedules_next_date",
					&[("id", owned(&row_id))],
					&vec![
						("schedule_id", owned(&id)),
						("local_next_date", next.clone()),
						("local_next_date_ts", now.clone()),
						("base_next_date", next),
						("base_next_date_ts", now),
					],
					Nulls::Skip,
				)
			}
		}
	}

	pub fn put_tag(&mut self, before: Option<&Tag>, after: Option<&Tag>) -> Result<(), CoreError> {
		let id = before
			.or(after)
			.map(|row| row.id.as_str().to_owned())
			.unwrap_or_default();
		self.put_actual("tags", &id, before, after, tag_columns)
			.map(|_| ())
	}

	/// Budget rows are found by (month, category), never by id, because
	/// Actual wrote two id formats. New rows get today's format.
	pub fn put_budget_cell(
		&mut self,
		before: Option<&BudgetCell>,
		after: Option<&BudgetCell>,
	) -> Result<(), CoreError> {
		let Some(after) = after else {
			return Err(invalid("budget rows are never deleted"));
		};
		let table = match self.budget_type {
			BudgetType::Envelope => "zero_budgets",
			BudgetType::Tracking => "reflect_budgets",
		};
		let month = db_month(after.month);
		let existing: Option<String> = self
			.tx
			.query_row(
				&format!(
					"SELECT id FROM {table} WHERE month = ?1 AND category = ?2 \
					 ORDER BY {BUDGET_CELL_PICK} LIMIT 1"
				),
				params_from_iter([Value::Integer(month), owned(after.category.as_str())]),
				|row| row.get(0),
			)
			.optional()
			.map_err(storage)?;
		match (existing, before) {
			(Some(id), Some(before)) => self.update(
				table,
				&[("id", owned(&id))],
				&budget_cell_columns(before),
				&budget_cell_columns(after),
			),
			(Some(_), None) => Err(invalid("that budget row already exists")),
			(None, _) => {
				// Actual's setBudget writes only the amount, so a goal-less
				// row keeps long_goal NULL, which reads as false.
				let mut columns = budget_cell_columns(after);
				columns.retain(|(name, value)| !(*name == "long_goal" && *value == flag(false)));
				self.insert(
					table,
					&[("id", owned(&format!("{month}-{}", after.category)))],
					&[
						vec![
							("month", Value::Integer(month)),
							("category", owned(after.category.as_str())),
						],
						columns,
					]
					.concat(),
					Nulls::Skip,
				)
			}
		}
	}

	pub fn put_hold(
		&mut self,
		month: YearMonth,
		before: Option<Option<Money>>,
		after: Option<Option<Money>>,
	) -> Result<(), CoreError> {
		if self.budget_type == BudgetType::Tracking {
			return Err(invalid("a tracking budget has no hold for next month"));
		}
		let key = [("id", owned(&month.to_string()))];
		let columns = |held: Option<Money>| vec![("buffered", money(held))];
		match (before, after) {
			(_, None) => Err(invalid("holds are never deleted")),
			(None, Some(after)) => {
				self.insert("zero_budget_months", &key, &columns(after), Nulls::Write)
			}
			(Some(before), Some(after)) => self.update(
				"zero_budget_months",
				&key,
				&columns(before),
				&columns(after),
			),
		}
	}

	/// Actual clears a note by writing NULL, so a removed note becomes NULL.
	/// `None` text is a NULL note, which is how Actual clears one.
	pub fn put_note(
		&mut self,
		id: &str,
		before: Option<Option<&str>>,
		after: Option<Option<&str>>,
	) -> Result<(), CoreError> {
		let key = [("id", owned(id))];
		let columns = |note: Option<&str>| vec![("note", text(note))];
		match (before, after) {
			(_, None) => Err(invalid("notes are cleared, never deleted")),
			(None, Some(note)) => self.insert("notes", &key, &columns(note), Nulls::Write),
			(Some(old), Some(note)) => self.update("notes", &key, &columns(old), &columns(note)),
		}
	}

	pub fn put_category_mapping(
		&mut self,
		from: &CategoryId,
		before: Option<&CategoryId>,
		after: Option<&CategoryId>,
	) -> Result<(), CoreError> {
		self.put_mapping(
			"category_mapping",
			"transferId",
			from.as_str(),
			before.map(CategoryId::as_str),
			after.map(CategoryId::as_str),
		)
	}

	pub fn put_payee_mapping(
		&mut self,
		from: &PayeeId,
		before: Option<&PayeeId>,
		after: Option<&PayeeId>,
	) -> Result<(), CoreError> {
		self.put_mapping(
			"payee_mapping",
			"targetId",
			from.as_str(),
			before.map(PayeeId::as_str),
			after.map(PayeeId::as_str),
		)
	}

	fn put_mapping(
		&self,
		table: &str,
		target: &'static str,
		from: &str,
		before: Option<&str>,
		after: Option<&str>,
	) -> Result<(), CoreError> {
		let key = [("id", owned(from))];
		match (before, after) {
			(_, None) => Err(invalid("mapping rows are never deleted")),
			(None, Some(to)) => self.insert(table, &key, &vec![(target, owned(to))], Nulls::Write),
			(Some(old), Some(to)) => self.update(
				table,
				&key,
				&vec![(target, owned(old))],
				&vec![(target, owned(to))],
			),
		}
	}

	pub fn put_category_meta(
		&mut self,
		category: &CategoryId,
		before: Option<&CategoryMeta>,
		after: Option<&CategoryMeta>,
	) -> Result<(), CoreError> {
		self.put_veritas(
			"veritas_category_meta",
			"category_id",
			category.as_str(),
			before,
			after,
			category_meta_columns,
		)
	}

	pub fn put_account_meta(
		&mut self,
		account: &AccountId,
		before: Option<&AccountMeta>,
		after: Option<&AccountMeta>,
	) -> Result<(), CoreError> {
		self.put_veritas(
			"veritas_account_meta",
			"account_id",
			account.as_str(),
			before,
			after,
			account_meta_columns,
		)
	}

	pub fn put_transaction_meta(
		&mut self,
		transaction: &TransactionId,
		before: Option<&TransactionMeta>,
		after: Option<&TransactionMeta>,
	) -> Result<(), CoreError> {
		self.put_veritas(
			"veritas_transaction_meta",
			"transaction_id",
			transaction.as_str(),
			before,
			after,
			transaction_meta_columns,
		)
	}

	/// A synced preference, such as `defaultCurrencyCode` or `numberFormat`.
	/// Actual stores every value as text.
	pub fn put_preference(&mut self, id: &str, value: &str) -> Result<(), CoreError> {
		self.tx
			.execute(
				"INSERT INTO preferences (id, value) VALUES (?1, ?2) \
				 ON CONFLICT (id) DO UPDATE SET value = excluded.value",
				[id, value],
			)
			.map(|_| ())
			.map_err(storage)
	}

	/// A `veritas_tax_tag` row: which tax bucket an Actual tag means.
	pub fn put_tax_tag(
		&mut self,
		tag: &str,
		before: Option<&str>,
		after: Option<&str>,
	) -> Result<(), CoreError> {
		self.put_veritas(
			"veritas_tax_tag",
			"tag",
			tag,
			before,
			after,
			|bucket: &str| vec![("bucket", owned(bucket))],
		)
	}

	pub fn put_goal(
		&mut self,
		before: Option<&Goal>,
		after: Option<&Goal>,
	) -> Result<(), CoreError> {
		let id = before
			.or(after)
			.map(|row| row.id.as_str().to_owned())
			.unwrap_or_default();
		if before.is_none()
			&& let Some(goal) = after
		{
			let mut columns = goal_columns(goal);
			columns.push(("created_at", Value::Integer(now_ms())));
			return self.insert(
				"veritas_goal",
				&[("id", owned(&id))],
				&columns,
				Nulls::Write,
			);
		}
		self.put_veritas("veritas_goal", "id", &id, before, after, goal_columns)
	}

	fn exists(&self, sql: &str, id: &str) -> Result<bool, CoreError> {
		self.tx
			.query_row(&format!("SELECT EXISTS ({sql})"), [id], |row| row.get(0))
			.map_err(storage)
	}

	/// Actual writes a self-mapping with every new category and payee, and a
	/// transfer payee (plus its mapping) with every new account. Commands plan
	/// those rows explicitly so memory sees them too; this refuses a
	/// transaction that forgot one.
	fn check_side_rows(&self) -> Result<(), CoreError> {
		for id in &self.new_categories {
			if !self.exists("SELECT 1 FROM category_mapping WHERE id = ?1", id.as_str())? {
				return Err(invalid(
					"a new category needs its category_mapping row in the same change",
				));
			}
		}
		for id in &self.new_payees {
			if !self.exists("SELECT 1 FROM payee_mapping WHERE id = ?1", id.as_str())? {
				return Err(invalid(
					"a new payee needs its payee_mapping row in the same change",
				));
			}
		}
		for id in &self.new_accounts {
			let has_payee = self.exists(
				"SELECT 1 FROM payees p JOIN payee_mapping m ON m.id = p.id \
				 WHERE p.transfer_acct = ?1 AND p.tombstone = 0",
				id.as_str(),
			)?;
			if !has_payee {
				return Err(invalid(
					"a new account needs its transfer payee in the same change",
				));
			}
		}
		Ok(())
	}

	pub fn commit(self) -> Result<(), CoreError> {
		self.check_side_rows()?;
		self.tx.commit().map_err(storage)
	}

	pub fn rollback(self) -> Result<(), CoreError> {
		self.tx.rollback().map_err(storage)
	}
}
