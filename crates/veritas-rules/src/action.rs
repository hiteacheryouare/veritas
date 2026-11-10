//! Rule actions: parsed the way `new Action(...)` validates them and applied
//! the way `Action.exec` does (reference/actual/packages/loot-core/src/server/rules/action.ts).

use serde_json::Value;
use veritas_domain::{AccountId, CategoryId, CivilDate, Money, PayeeId, ScheduleId};

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
	LinkSchedule(ScheduleId),
	PrependNotes(String),
	AppendNotes(String),
	DeleteTransaction,
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
		_ => unsupported(format!("setting {what} to something that isn't text")),
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
		if op == "set-split-amount" || js::truthy(option("splitIndex")) {
			return unsupported("split actions");
		}

		match op {
			"set" => {
				if js::truthy(option("formula")) {
					return unsupported("formula actions");
				}
				if js::truthy(option("template")) {
					return unsupported("template actions");
				}
				let Some(field) = field else {
					return unsupported("setting saved");
				};
				Ok(Action::Set(parse_set(field, value)?))
			}
			"link-schedule" => match value {
				Some(Value::String(id)) => Ok(Action::LinkSchedule(ScheduleId::new(id.clone()))),
				_ => unsupported("linking a schedule id that isn't text"),
			},
			"prepend-notes" | "append-notes" => {
				let Some(Value::String(text)) = value else {
					return unsupported("notes text that isn't text");
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
			_ => return unsupported("setting account to something that isn't text"),
		},
		Field::Notes => SetField::Notes(text_or_null(value, "notes")?),
		Field::ImportedPayee => SetField::ImportedPayee(text_or_null(value, "imported payee")?),
		Field::Amount => match value.and_then(js::integer) {
			Some(amount) => SetField::Amount(Money::from_minor(amount)),
			None => return unsupported("setting amount to something that isn't whole cents"),
		},
		Field::Date => match value.and_then(Value::as_str).and_then(js::strict_date) {
			Some(date) => SetField::Date(date.into()),
			None => return unsupported("setting date to something that isn't YYYY-MM-DD"),
		},
		Field::Cleared | Field::Reconciled => {
			let Some(Value::Bool(flag)) = value else {
				return unsupported("setting a flag to something that isn't true or false");
			};
			if field == Field::Cleared {
				SetField::Cleared(*flag)
			} else {
				SetField::Reconciled(*flag)
			}
		}
		Field::PayeeName => {
			return unsupported("renaming to a payee by name, which creates payees");
		}
		Field::CategoryGroup | Field::Transfer | Field::Parent => {
			return unsupported("setting a field Veritas doesn't keep on transactions");
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
			json!({"op": "set", "field": "payee_name", "value": "Amazon"}),
			json!({"op": "set-split-amount", "value": 500, "options": {"splitIndex": 1, "method": "fixed-amount"}}),
			json!({"op": "set", "field": "category", "value": "food", "options": {"splitIndex": 2}}),
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
