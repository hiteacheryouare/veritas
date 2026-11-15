//! Rule actions: parsed the way `new Action(...)` validates them and applied
//! the way `Action.exec` does (reference/actual/packages/loot-core/src/server/rules/action.ts).

use serde_json::Value;
use veritas_domain::{AccountId, CategoryId, CivilDate, CoreError, Money, PayeeId, ScheduleId};

use crate::condition::{Field, Gap};
use crate::js;

#[derive(Clone, Debug, PartialEq)]
pub enum SetField {
	Payee(Option<PayeeId>),
	Category(Option<CategoryId>),
	Account(AccountId),
	Notes(Option<String>),
	ImportedPayee(Option<String>),
	Amount(Money),
	Date(CivilDate),
	Cleared(bool),
	Reconciled(bool),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
	Set(SetField),
	/// `set payee_name`: the payee by that name, made if missing, once the
	/// rule finishes (`resolvePayeeNameForRules`). An empty name clears it.
	SetPayeeName(Option<String>),
	LinkSchedule(ScheduleId),
	PrependNotes(String),
	AppendNotes(String),
	DeleteTransaction,
	/// `set-split-amount`. On split 0, the transaction itself, only a fixed
	/// amount does anything (`action.ts:201-242`).
	SplitAmount {
		split: u32,
		method: SplitMethod,
	},
	/// An action on split line `split`, numbered from 1 as Actual stores
	/// `options.splitIndex`.
	InSplit {
		split: u32,
		action: Box<Action>,
	},
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SplitMethod {
	FixedAmount(Money),
	/// A percentage of what the fixed amounts leave.
	FixedPercent(f64),
	/// An equal share of what the fixed amounts and percentages leave.
	Remainder,
}

/// One child of a split, as rules build it. The session stores it the way
/// `makeSplitTransaction` does (`sync.ts:45-58`): the parent's account, date,
/// cleared and reconciled, and `sort_order` 0, −1, −2… in this order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SplitLine {
	pub amount: Money,
	pub payee: Option<PayeeId>,
	pub category: Option<CategoryId>,
	pub notes: Option<String>,
	/// Rules never give a line a schedule. It is set only when the line's
	/// transfer leg links one, which Actual writes back (`Transfers::link_back`).
	pub schedule: Option<ScheduleId>,
}

/// The fields a rule reads and writes on a transaction while it runs.
#[derive(Clone, Debug, PartialEq)]
pub struct RuleTransaction {
	pub account: AccountId,
	pub date: CivilDate,
	pub amount: Money,
	pub payee: Option<PayeeId>,
	pub category: Option<CategoryId>,
	pub notes: Option<String>,
	pub imported_payee: Option<String>,
	/// `None` is a transaction object without the key, which no condition matches.
	pub cleared: Option<bool>,
	pub reconciled: Option<bool>,
	pub schedule: Option<ScheduleId>,
	pub deleted: bool,
	/// A split's lines; a transaction with any is a split parent.
	pub splits: Vec<SplitLine>,
	/// A split child run on its own, as the "Run rules" action can. Split
	/// actions skip it (`rule.ts:118-121`).
	pub is_child: bool,
}

impl RuleTransaction {
	/// `recalculateSplit`'s error: what the lines leave of the parent's
	/// amount, `None` when they add up. `Overflow` for lines whose total
	/// doesn't fit (only possible with lines Veritas didn't build).
	pub fn split_difference(&self) -> Result<Option<Money>, CoreError> {
		if self.splits.is_empty() {
			return Ok(None);
		}
		let total = Money::checked_sum(self.splits.iter().map(|line| line.amount))?;
		let difference = self.amount.checked_sub(total)?;
		Ok((difference != Money::ZERO).then_some(difference))
	}
}

fn rejected<T>(reason: impl Into<String>) -> Result<T, Gap> {
	Err(Gap::RejectedByActual(reason.into()))
}

fn unsupported<T>(reason: impl Into<String>) -> Result<T, Gap> {
	Err(Gap::Unsupported(reason.into()))
}

fn text_or_null(value: Option<&Value>, what: &str) -> Result<Option<String>, Gap> {
	match value {
		Some(Value::String(text)) => Ok(Some(text.clone())),
		Some(Value::Null) => Ok(None),
		_ => unsupported(format!("a new {what} that isn't text")),
	}
}

impl Action {
	/// Parses one stored action item (`{op, field, value, type, options?}`).
	pub fn parse(item: &Value) -> Result<Action, Gap> {
		let op = item.get("op").and_then(Value::as_str).unwrap_or_default();
		let field_name = item.get("field").and_then(Value::as_str);
		let field = field_name.and_then(Field::from_stored);
		let value = item.get("value");
		let options = item.get("options");
		let option = |key: &str| options.and_then(|options| options.get(key));

		match op {
			"set" if field.is_none() && field_name != Some("saved") => {
				return rejected(format!("invalid field for action: {field_name:?}"));
			}
			"set" | "set-split-amount" | "link-schedule" | "prepend-notes" | "append-notes"
			| "delete-transaction" => {}
			_ => return rejected(format!("invalid action operation {op:?}")),
		}
		if field == Some(Field::Account) && !js::truthy(value) {
			return rejected("field cannot be empty: account");
		}
		let split = match option("splitIndex") {
			index if !js::truthy(index) => 0,
			Some(index) => js::integer(index)
				.and_then(|n| u32::try_from(n).ok())
				.ok_or_else(|| Gap::Unsupported("a split line number that isn't whole".into()))?,
			None => 0,
		};
		if op == "set-split-amount" {
			let method = match option("method").and_then(Value::as_str) {
				Some("fixed-amount") => match value.and_then(js::integer) {
					Some(amount) => SplitMethod::FixedAmount(Money::from_minor(amount)),
					None => return unsupported("a fixed split amount that isn't whole cents"),
				},
				Some("fixed-percent") => match value.and_then(Value::as_f64) {
					Some(percent) => SplitMethod::FixedPercent(percent),
					None => return unsupported("a split percentage that isn't a number"),
				},
				Some("remainder") => SplitMethod::Remainder,
				Some("formula") => return unsupported("a formula split amount"),
				_ => return unsupported("a split amount without a known method"),
			};
			return Ok(Action::SplitAmount { split, method });
		}
		let action = Self::parse_plain(op, field, value, option("template"), option("formula"))?;
		if split == 0 {
			return Ok(action);
		}
		match action {
			Action::Set(
				SetField::Payee(_)
				| SetField::Category(_)
				| SetField::Notes(_)
				| SetField::Amount(_),
			)
			| Action::PrependNotes(_)
			| Action::AppendNotes(_) => Ok(Action::InSplit {
				split,
				action: Box::new(action),
			}),
			_ => unsupported("an action Veritas can't run on a split line"),
		}
	}

	fn parse_plain(
		op: &str,
		field: Option<Field>,
		value: Option<&Value>,
		template: Option<&Value>,
		formula: Option<&Value>,
	) -> Result<Action, Gap> {
		match op {
			"set" => {
				if js::truthy(formula) {
					return unsupported("a formula action");
				}
				if js::truthy(template) {
					return unsupported("a template action");
				}
				let Some(field) = field else {
					return unsupported("a change to the saved field");
				};
				if field == Field::PayeeName {
					return Ok(Action::SetPayeeName(text_or_null(value, "payee name")?));
				}
				Ok(Action::Set(parse_set(field, value)?))
			}
			"link-schedule" => match value {
				Some(Value::String(id)) => Ok(Action::LinkSchedule(ScheduleId::new(id.clone()))),
				_ => unsupported("a schedule link that isn't text"),
			},
			"prepend-notes" | "append-notes" => {
				let Some(Value::String(text)) = value else {
					return unsupported("notes that aren't text");
				};
				Ok(if op == "prepend-notes" {
					Action::PrependNotes(text.clone())
				} else {
					Action::AppendNotes(text.clone())
				})
			}
			_ => Ok(Action::DeleteTransaction),
		}
	}

	/// Points `set` on an id field at its merge target, like `migrateIds`.
	pub(crate) fn migrate_ids(&mut self, map: &impl Fn(&str) -> Option<String>) {
		let remap = |id: &str| map(id).filter(|target| !target.is_empty());
		match self {
			Action::Set(SetField::Payee(Some(id))) => {
				if let Some(target) = remap(id.as_str()) {
					*id = PayeeId::new(target);
				}
			}
			Action::Set(SetField::Category(Some(id))) => {
				if let Some(target) = remap(id.as_str()) {
					*id = CategoryId::new(target);
				}
			}
			Action::Set(SetField::Account(id)) => {
				if let Some(target) = remap(id.as_str()) {
					*id = AccountId::new(target);
				}
			}
			Action::InSplit { action, .. } => action.migrate_ids(map),
			_ => {}
		}
	}

	/// The split line an action belongs to; 0 is the transaction itself.
	pub fn split(&self) -> u32 {
		match self {
			Action::SplitAmount { split, .. } | Action::InSplit { split, .. } => *split,
			_ => 0,
		}
	}

	/// `Action.exec` on a split line.
	pub(crate) fn apply_to_line(&self, line: &mut SplitLine) {
		let wrap = |notes: Option<String>, text: &str, before: bool| {
			Some(match notes.filter(|n| !n.is_empty()) {
				Some(notes) if before => format!("{text}{notes}"),
				Some(notes) => format!("{notes}{text}"),
				None => text.to_owned(),
			})
		};
		match self {
			Action::InSplit { action, .. } => action.apply_to_line(line),
			Action::SplitAmount {
				method: SplitMethod::FixedAmount(amount),
				..
			} => line.amount = *amount,
			Action::Set(SetField::Payee(payee)) => line.payee = payee.clone(),
			Action::Set(SetField::Category(category)) => line.category = category.clone(),
			Action::Set(SetField::Notes(notes)) => line.notes = notes.clone(),
			Action::Set(SetField::Amount(amount)) => line.amount = *amount,
			Action::PrependNotes(text) => line.notes = wrap(line.notes.take(), text, true),
			Action::AppendNotes(text) => line.notes = wrap(line.notes.take(), text, false),
			_ => {}
		}
	}

	/// `Action.exec` on the running transaction.
	pub fn apply(&self, transaction: &mut RuleTransaction) {
		match self {
			Action::Set(set) => match set.clone() {
				SetField::Payee(payee) => transaction.payee = payee,
				SetField::Category(category) => transaction.category = category,
				SetField::Account(account) => transaction.account = account,
				SetField::Notes(notes) => transaction.notes = notes,
				SetField::ImportedPayee(imported) => transaction.imported_payee = imported,
				SetField::Amount(amount) => transaction.amount = amount,
				SetField::Date(date) => transaction.date = date,
				SetField::Cleared(cleared) => transaction.cleared = Some(cleared),
				SetField::Reconciled(reconciled) => transaction.reconciled = Some(reconciled),
			},
			Action::LinkSchedule(schedule) => transaction.schedule = Some(schedule.clone()),
			Action::PrependNotes(text) => {
				transaction.notes =
					Some(match transaction.notes.take().filter(|n| !n.is_empty()) {
						Some(notes) => format!("{text}{notes}"),
						None => text.clone(),
					});
			}
			Action::AppendNotes(text) => {
				transaction.notes =
					Some(match transaction.notes.take().filter(|n| !n.is_empty()) {
						Some(notes) => format!("{notes}{text}"),
						None => text.clone(),
					});
			}
			Action::DeleteTransaction => transaction.deleted = true,
			Action::SplitAmount {
				method: SplitMethod::FixedAmount(amount),
				..
			} => transaction.amount = *amount,
			Action::SetPayeeName(_) | Action::SplitAmount { .. } | Action::InSplit { .. } => {}
		}
	}
}

fn parse_set(field: Field, value: Option<&Value>) -> Result<SetField, Gap> {
	Ok(match field {
		Field::Payee => SetField::Payee(text_or_null(value, "payee")?.map(PayeeId::new)),
		Field::Category => {
			SetField::Category(text_or_null(value, "category")?.map(CategoryId::new))
		}
		Field::Account => match value {
			Some(Value::String(id)) => SetField::Account(AccountId::new(id.clone())),
			_ => return unsupported("a new account that isn't text"),
		},
		Field::Notes => SetField::Notes(text_or_null(value, "notes")?),
		Field::ImportedPayee => SetField::ImportedPayee(text_or_null(value, "imported payee")?),
		Field::Amount => match value.and_then(js::integer) {
			Some(amount) => SetField::Amount(Money::from_minor(amount)),
			None => return unsupported("a new amount that isn't whole cents"),
		},
		Field::Date => match value.and_then(Value::as_str).and_then(js::strict_date) {
			Some(date) => SetField::Date(date.into()),
			None => return unsupported("a new date that isn't YYYY-MM-DD"),
		},
		Field::Cleared | Field::Reconciled => {
			let Some(Value::Bool(flag)) = value else {
				return unsupported("a flag set to something other than true or false");
			};
			if field == Field::Cleared {
				SetField::Cleared(*flag)
			} else {
				SetField::Reconciled(*flag)
			}
		}
		Field::PayeeName | Field::CategoryGroup | Field::Transfer | Field::Parent => {
			return unsupported("a change to a field Veritas doesn't keep on transactions");
		}
	})
}

#[cfg(test)]
mod tests {
	use serde_json::json;

	use super::*;

	fn transaction() -> RuleTransaction {
		RuleTransaction {
			account: AccountId::new("checking"),
			date: CivilDate::new(2026, 11, 9).unwrap(),
			amount: Money::from_minor(-1250),
			payee: None,
			category: None,
			notes: None,
			imported_payee: Some("AMZN Mktp".into()),
			cleared: Some(false),
			reconciled: None,
			schedule: None,
			deleted: false,
			splits: Vec::new(),
			is_child: false,
		}
	}

	fn action(item: Value) -> Action {
		Action::parse(&item).unwrap()
	}

	#[test]
	fn set_writes_the_field() {
		// reference/actual/packages/loot-core/src/server/rules/index.test.ts:326-331
		let mut txn = transaction();
		txn.notes = Some("Sarah".into());
		action(json!({"op": "set", "field": "notes", "value": "James"})).apply(&mut txn);
		assert_eq!(txn.notes.as_deref(), Some("James"));
		action(json!({"op": "set", "field": "description", "value": "amazon"})).apply(&mut txn);
		assert_eq!(txn.payee, Some(PayeeId::new("amazon")));
		action(json!({"op": "set", "field": "category", "value": null})).apply(&mut txn);
		assert_eq!(txn.category, None);
		action(json!({"op": "set", "field": "cleared", "value": true})).apply(&mut txn);
		assert_eq!(txn.cleared, Some(true));
	}

	#[test]
	fn notes_are_prepended_and_appended_without_a_separator() {
		let mut txn = transaction();
		action(json!({"op": "append-notes", "value": " #food"})).apply(&mut txn);
		assert_eq!(txn.notes.as_deref(), Some(" #food"));
		action(json!({"op": "prepend-notes", "value": "Lunch"})).apply(&mut txn);
		assert_eq!(txn.notes.as_deref(), Some("Lunch #food"));
	}

	#[test]
	fn link_schedule_and_delete() {
		let mut txn = transaction();
		action(json!({"op": "link-schedule", "value": "rent"})).apply(&mut txn);
		action(json!({"op": "delete-transaction", "value": null})).apply(&mut txn);
		assert_eq!(txn.schedule, Some(ScheduleId::new("rent")));
		assert!(txn.deleted);
	}

	#[test]
	fn what_actual_rejects_and_what_veritas_leaves_to_actual() {
		let gap = |item: Value| Action::parse(&item).unwrap_err();
		assert!(matches!(
			gap(json!({"op": "rename", "field": "notes", "value": "x"})),
			Gap::RejectedByActual(_)
		));
		assert!(matches!(
			gap(json!({"op": "set", "field": "nonsense", "value": "x"})),
			Gap::RejectedByActual(_)
		));
		assert!(matches!(
			gap(json!({"op": "set", "field": "account", "value": ""})),
			Gap::RejectedByActual(_)
		));
		for item in [
			json!({"op": "set", "field": "notes", "value": "", "options": {"template": "{{imported_payee}}"}}),
			json!({"op": "set", "field": "amount", "value": 0, "options": {"formula": "=BALANCE_OF(\"x\")"}}),
			json!({"op": "set-split-amount", "value": null, "options": {"splitIndex": 1, "method": "formula", "formula": "=1"}}),
			json!({"op": "set", "field": "cleared", "value": true, "options": {"splitIndex": 2}}),
			json!({"op": "set", "field": "amount", "value": 12.5}),
		] {
			assert!(matches!(gap(item.clone()), Gap::Unsupported(_)), "{item}");
		}
		// An empty template is no template in Actual, so this is a plain set.
		assert!(
			Action::parse(
				&json!({"op": "set", "field": "notes", "value": "x", "options": {"template": ""}})
			)
			.is_ok()
		);
	}
}
