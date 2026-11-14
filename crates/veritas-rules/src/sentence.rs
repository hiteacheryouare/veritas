//! A rule as the sentence editor shows it (R-RULE-2), read from the stored
//! JSON, and drafts written back the way Actual serializes a rule
//! (`Condition.serialize`, condition.ts:453-460; `Action.serialize`,
//! action.ts:403-410). An item the draft leaves unchanged keeps its stored
//! text byte for byte.

use serde_json::Value as Json;
use veritas_domain::{
	BudgetData, CategoryId, ConditionsOp, CoreError, PayeeId, Rule, RuleId, RuleStage, ScheduleId,
};

use crate::condition::{AmountSign, Field, FieldKind, Op};
use crate::js;
use crate::jsontext::{array_items, string, write_array, write_object};
use crate::rule::{ParsedRule, RuleSupport, rank};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
	Id(Option<String>),
	Ids(Vec<String>),
	Text(String),
	Texts(Vec<String>),
	/// Minor units.
	Amount(i64),
	/// `isbetween`'s `num1` and `num2`, in the order stored.
	AmountRange(i64, i64),
	/// `YYYY-MM-DD`.
	Date(String),
	/// `YYYY-MM`.
	Month(String),
	Year(i32),
	Bool(bool),
	/// `onBudget`, `offBudget`, `delete-transaction` and a remainder split.
	Nothing,
	/// Stored JSON the editor can't show, such as a repeating date or a template.
	Raw(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SentenceCondition {
	pub field: Field,
	pub op: Op,
	/// Inflow and outflow are `amount` with `options` (`shared/rules.ts:120-128`).
	pub sign: AmountSign,
	pub value: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionOp {
	Set,
	PrependNotes,
	AppendNotes,
	LinkSchedule,
	DeleteTransaction,
	SetSplitAmount,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SentenceAction {
	pub op: ActionOp,
	/// Only `set` has one; `None` for `set saved` too.
	pub field: Option<Field>,
	pub value: Value,
	/// `options.splitIndex`; 0 is the transaction itself.
	pub split: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Access {
	Editable,
	/// A bill's rule: it is edited through the bill.
	Bill(ScheduleId),
	/// Kept exactly as stored; edit it in Actual (R-RULE-3).
	EditInActual(Vec<String>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sentence {
	pub id: RuleId,
	pub stage: RuleStage,
	pub conditions_op: ConditionsOp,
	pub conditions: Vec<SentenceCondition>,
	pub actions: Vec<SentenceAction>,
	pub access: Access,
	/// Veritas evaluates and applies every condition and action.
	pub runs_in_veritas: bool,
}

/// What the editor saves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Draft {
	pub stage: RuleStage,
	pub conditions_op: ConditionsOp,
	pub conditions: Vec<SentenceCondition>,
	pub actions: Vec<SentenceAction>,
}

const ITEM_KEYS: [&str; 5] = ["op", "field", "value", "type", "options"];
const SPLITS: &str = "a split";
const UNSHOWN: &str = "a value the sentence editor can't show";
const EXTRAS: &str = "stored details the sentence editor can't keep";

fn invalid<T>(message: impl Into<String>) -> Result<T, CoreError> {
	Err(CoreError::Invalid(message.into()))
}

/// The id a merged payee or category now reads as, as `migrateIds` maps it
/// when Actual loads rules.
fn mapped(budget: &BudgetData, id: &str) -> String {
	budget
		.category_mapping
		.get(&CategoryId::new(id))
		.map(CategoryId::as_str)
		.or_else(|| {
			budget
				.payee_mapping
				.get(&PayeeId::new(id))
				.map(PayeeId::as_str)
		})
		.filter(|target| !target.is_empty())
		.unwrap_or(id)
		.to_owned()
}

fn only_keys(item: &Json, keys: &[&str]) -> bool {
	item.as_object()
		.is_some_and(|object| object.keys().all(|key| keys.contains(&key.as_str())))
}

fn options_hold_only(item: &Json, allowed: impl Fn(&str, &Json) -> bool) -> bool {
	match item.get("options") {
		None | Some(Json::Null) => true,
		Some(Json::Object(options)) => options.iter().all(|(key, value)| allowed(key, value)),
		Some(_) => false,
	}
}

fn text_of(value: Option<&Json>) -> Option<String> {
	value.and_then(Json::as_str).map(str::to_owned)
}

fn raw(value: Option<&Json>) -> Value {
	Value::Raw(value.map_or_else(|| "null".to_owned(), Json::to_string))
}

fn condition_value(
	budget: &BudgetData,
	field: Field,
	op: Op,
	value: Option<&Json>,
) -> Option<Value> {
	let ids = |budget: &BudgetData| -> Option<Vec<String>> {
		value?
			.as_array()?
			.iter()
			.map(|entry| entry.as_str().map(|id| mapped(budget, id)))
			.collect()
	};
	let texts = || -> Option<Vec<String>> {
		value?
			.as_array()?
			.iter()
			.map(|entry| entry.as_str().map(str::to_owned))
			.collect()
	};
	Some(match (field.kind(), op) {
		(_, Op::OnBudget | Op::OffBudget) => Value::Nothing,
		(_, Op::And) => return None,
		(FieldKind::Id, Op::Is | Op::IsNot) => match value? {
			Json::Null => Value::Id(None),
			Json::String(id) => Value::Id(Some(mapped(budget, id))),
			_ => return None,
		},
		(FieldKind::Id, Op::OneOf | Op::NotOneOf) => Value::Ids(ids(budget)?),
		(FieldKind::Text, Op::OneOf | Op::NotOneOf) => Value::Texts(texts()?),
		(FieldKind::Id | FieldKind::Text, _) => Value::Text(text_of(value)?),
		(FieldKind::Number, Op::IsBetween) => {
			let range = value?;
			Value::AmountRange(
				js::integer(range.get("num1")?)?,
				js::integer(range.get("num2")?)?,
			)
		}
		(FieldKind::Number, _) => Value::Amount(js::integer(value?)?),
		(FieldKind::Date, _) => date_value(value?.as_str()?)?,
		(FieldKind::Boolean, _) => Value::Bool(value?.as_bool()?),
	})
}

fn date_value(text: &str) -> Option<Value> {
	let digits =
		|part: &str, len: usize| part.len() == len && part.bytes().all(|b| b.is_ascii_digit());
	let parts: Vec<&str> = text.split('-').collect();
	match parts.as_slice() {
		[year, month, day] if digits(year, 4) && digits(month, 2) && digits(day, 2) => {
			js::strict_date(text).map(|_| Value::Date(text.to_owned()))
		}
		[year, month] if digits(year, 4) && digits(month, 2) => {
			veritas_domain::YearMonth::parse(text)
				.ok()
				.map(|_| Value::Month(text.to_owned()))
		}
		[year] if digits(year, 4) => year.parse().ok().map(Value::Year),
		_ => None,
	}
}

/// One stored condition, and why the editor can't take it as it is.
fn read_condition(
	budget: &BudgetData,
	item: &Json,
) -> Option<(SentenceCondition, Option<&'static str>)> {
	let field = Field::from_stored(item.get("field")?.as_str()?)?;
	let op = Op::from_stored(item.get("op")?.as_str()?)?;
	let sign = match item.get("options") {
		Some(Json::Object(options)) if js::truthy(options.get("outflow")) => AmountSign::Outflow,
		Some(Json::Object(options)) if js::truthy(options.get("inflow")) => AmountSign::Inflow,
		_ => AmountSign::Any,
	};
	let value = condition_value(budget, field, op, item.get("value"));
	let plain = only_keys(item, &ITEM_KEYS)
		&& options_hold_only(item, |key, value| {
			matches!(key, "inflow" | "outflow") && value.is_boolean()
		});
	let problem = match (&value, plain) {
		(None, _) => Some(UNSHOWN),
		(_, false) => Some(EXTRAS),
		_ => None,
	};
	let condition = SentenceCondition {
		field,
		op,
		sign: if field == Field::Amount {
			sign
		} else {
			AmountSign::Any
		},
		value: value.unwrap_or_else(|| raw(item.get("value"))),
	};
	Some((condition, problem))
}

fn read_action(budget: &BudgetData, item: &Json) -> Option<(SentenceAction, Option<&'static str>)> {
	let option = |key: &str| item.get("options").and_then(|options| options.get(key));
	let split = match option("splitIndex") {
		index if !js::truthy(index) => 0,
		Some(index) => js::integer(index).and_then(|n| u32::try_from(n).ok())?,
		None => 0,
	};
	let value = item.get("value");
	let id = |value: Option<&Json>| match value {
		Some(Json::Null) => Some(Value::Id(None)),
		Some(Json::String(id)) => Some(Value::Id(Some(mapped(budget, id)))),
		_ => None,
	};
	let (op, field, value) = match item.get("op")?.as_str()? {
		"set" => {
			let field = Field::from_stored(item.get("field")?.as_str()?);
			let value = if js::truthy(option("template")) || js::truthy(option("formula")) {
				None
			} else {
				match field {
					Some(Field::Payee | Field::Category | Field::Account) => id(value),
					Some(Field::Notes | Field::ImportedPayee | Field::PayeeName) => {
						text_of(value).map(Value::Text)
					}
					Some(Field::Amount) => value.and_then(js::integer).map(Value::Amount),
					Some(Field::Date) => value
						.and_then(Json::as_str)
						.and_then(js::strict_date)
						.map(|date| Value::Date(date.to_string())),
					Some(Field::Cleared | Field::Reconciled) => {
						value.and_then(Json::as_bool).map(Value::Bool)
					}
					_ => None,
				}
			};
			(ActionOp::Set, field, value)
		}
		"prepend-notes" => (
			ActionOp::PrependNotes,
			None,
			text_of(value).map(Value::Text),
		),
		"append-notes" => (ActionOp::AppendNotes, None, text_of(value).map(Value::Text)),
		"link-schedule" => (
			ActionOp::LinkSchedule,
			None,
			text_of(value).map(|id| Value::Id(Some(id))),
		),
		"delete-transaction" => (ActionOp::DeleteTransaction, None, Some(Value::Nothing)),
		"set-split-amount" => {
			let value = match option("method").and_then(Json::as_str) {
				Some("fixed-amount") => value.and_then(js::integer).map(Value::Amount),
				Some("remainder") => Some(Value::Nothing),
				_ => None,
			};
			(ActionOp::SetSplitAmount, None, value)
		}
		_ => return None,
	};
	let plain = only_keys(item, &ITEM_KEYS)
		&& options_hold_only(item, |key, value| {
			matches!(key, "splitIndex" | "template" | "formula") && !js::truthy(Some(value))
		});
	let problem = if split > 0 || op == ActionOp::SetSplitAmount {
		Some(SPLITS)
	} else if value.is_none() {
		Some(UNSHOWN)
	} else if !plain {
		Some(EXTRAS)
	} else {
		None
	};
	let action = SentenceAction {
		op,
		field,
		value: value.unwrap_or_else(|| Value::Raw(item.to_string())),
		split,
	};
	Some((action, problem))
}

type Read<T> = Vec<(String, T, Option<&'static str>)>;

/// Each stored item's text, model and editor problem. `None` when the text
/// isn't an array or holds an item Actual itself would refuse.
fn read_items<T>(
	text: &str,
	read: impl Fn(&Json) -> Option<(T, Option<&'static str>)>,
) -> Option<Read<T>> {
	array_items(text)?
		.into_iter()
		.map(|item| {
			let value: Json = serde_json::from_str(item).ok()?;
			let (model, problem) = read(&value)?;
			Some((item.to_owned(), model, problem))
		})
		.collect()
}

/// The bill a rule belongs to: the schedule it links, or a schedule that
/// names it.
fn owner(budget: &BudgetData, rule: &Rule, actions: &[SentenceAction]) -> Option<ScheduleId> {
	actions
		.iter()
		.find(|action| action.op == ActionOp::LinkSchedule)
		.and_then(|action| match &action.value {
			Value::Id(Some(id)) => Some(ScheduleId::new(id.as_str())),
			_ => None,
		})
		.or_else(|| {
			budget
				.schedules
				.values()
				.find(|schedule| !schedule.tombstone && schedule.rule == rule.id)
				.map(|schedule| schedule.id.clone())
		})
}

/// One live rule as the editor sees it. `None` for a rule Actual drops when
/// it loads rules, which its rules page never shows.
pub fn sentence(budget: &BudgetData, rule: &Rule) -> Option<Sentence> {
	let support = ParsedRule::parse(rule).support();
	if matches!(support, RuleSupport::IgnoredByActual(_)) {
		return None;
	}
	let conditions = read_items(&rule.conditions_json, |item| read_condition(budget, item))?;
	let actions = read_items(&rule.actions_json, |item| read_action(budget, item))?;
	let mut problems: Vec<String> = Vec::new();
	for problem in conditions
		.iter()
		.map(|(_, _, problem)| problem)
		.chain(actions.iter().map(|(_, _, problem)| problem))
		.flatten()
	{
		if !problems.iter().any(|known| known == problem) {
			problems.push((*problem).to_owned());
		}
	}
	let actions: Vec<SentenceAction> = actions.into_iter().map(|(_, model, _)| model).collect();
	let access = match (owner(budget, rule, &actions), &support) {
		(Some(schedule), _) => Access::Bill(schedule),
		(None, RuleSupport::ReadOnly(reasons)) => Access::EditInActual(reasons.clone()),
		(None, _) if !problems.is_empty() => Access::EditInActual(problems),
		(None, _) => Access::Editable,
	};
	Some(Sentence {
		id: rule.id.clone(),
		stage: rule.stage,
		conditions_op: rule.conditions_op,
		conditions: conditions.into_iter().map(|(_, model, _)| model).collect(),
		actions,
		access,
		runs_in_veritas: support == RuleSupport::Full,
	})
}

/// The rules page's list (`ManageRules.tsx:145-160`): `rules-get`, which is
/// `rankRules` over the loaded rules (`rules/app.ts:186-188`), without the
/// rules of completed bills.
pub fn sentences(budget: &BudgetData) -> Vec<Sentence> {
	let completed = |id: &RuleId| {
		budget
			.schedules
			.values()
			.any(|schedule| !schedule.tombstone && schedule.completed && &schedule.rule == id)
	};
	let parsed: Vec<ParsedRule> = budget
		.rules
		.values()
		.filter(|rule| !rule.tombstone && !completed(&rule.id))
		.map(ParsedRule::parse)
		.collect();
	rank(&parsed)
		.into_iter()
		.filter_map(|parsed| budget.rules.get(&parsed.id))
		.filter_map(|rule| sentence(budget, rule))
		.collect()
}

fn field_label(field: Field) -> &'static str {
	match field {
		Field::ImportedPayee => "imported payee",
		Field::Payee => "payee",
		Field::PayeeName => "payee name",
		Field::Account => "account",
		Field::Category => "category",
		Field::CategoryGroup => "category group",
		Field::Notes => "notes",
		Field::Amount => "amount",
		Field::Date => "date",
		Field::Cleared => "cleared",
		Field::Reconciled => "reconciled",
		Field::Transfer => "transfer",
		Field::Parent => "parent",
	}
}

fn live(budget: &BudgetData, field: Field, id: &str) -> Result<(), CoreError> {
	let found = match field {
		Field::Payee => budget
			.payees
			.get(&PayeeId::new(id))
			.is_some_and(|payee| !payee.tombstone),
		Field::Category => budget
			.categories
			.get(&CategoryId::new(id))
			.is_some_and(|category| !category.tombstone),
		Field::CategoryGroup => budget
			.category_groups
			.get(&veritas_domain::CategoryGroupId::new(id))
			.is_some_and(|group| !group.tombstone),
		Field::Account => budget
			.accounts
			.get(&veritas_domain::AccountId::new(id))
			.is_some_and(|account| !account.tombstone),
		_ => true,
	};
	if found {
		Ok(())
	} else {
		Err(CoreError::NotFound {
			entity: match field {
				Field::Payee => "payee",
				Field::Category => "category",
				Field::CategoryGroup => "category group",
				_ => "account",
			},
			id: id.to_owned(),
		})
	}
}

fn check_condition(budget: &BudgetData, condition: &SentenceCondition) -> Result<(), CoreError> {
	let SentenceCondition {
		field,
		op,
		sign,
		value,
	} = condition;
	let label = field_label(*field);
	if *op == Op::And || !op.valid_for(*field) {
		return invalid(format!("“{}” doesn't work with {label}.", op.stored()));
	}
	if *sign != AmountSign::Any && (*field != Field::Amount || *op == Op::IsBetween) {
		return invalid("Inflow and outflow need an amount compared to a single number.");
	}
	let needs_text = matches!(
		op,
		Op::Contains | Op::DoesNotContain | Op::Matches | Op::HasTags | Op::HasAnyTag
	);
	let fits = match (field.kind(), op, value) {
		(_, Op::OnBudget | Op::OffBudget, Value::Nothing) => true,
		(FieldKind::Id, Op::Is | Op::IsNot, Value::Id(id)) => {
			if let Some(id) = id {
				live(budget, *field, id)?;
			}
			true
		}
		(FieldKind::Id, Op::OneOf | Op::NotOneOf, Value::Ids(ids)) => {
			if ids.is_empty() {
				return invalid(format!("Choose at least one {label}."));
			}
			for id in ids {
				live(budget, *field, id)?;
			}
			true
		}
		(FieldKind::Text, Op::OneOf | Op::NotOneOf, Value::Texts(texts)) => {
			if texts.is_empty() || texts.iter().any(String::is_empty) {
				return invalid(format!("Enter at least one {label}, and leave none blank."));
			}
			true
		}
		(FieldKind::Id | FieldKind::Text, _, Value::Text(text)) if needs_text => {
			if text.is_empty() {
				return invalid(format!("Enter the text to look for in {label}."));
			}
			true
		}
		(FieldKind::Text, Op::Is | Op::IsNot, Value::Text(_)) => true,
		(FieldKind::Number, Op::IsBetween, Value::AmountRange(..)) => true,
		(FieldKind::Number, op, Value::Amount(_)) => *op != Op::IsBetween,
		(FieldKind::Date, Op::Is, Value::Date(text) | Value::Month(text)) => {
			date_value(text).is_some()
		}
		(FieldKind::Date, Op::Is, Value::Year(year)) => (1000..=9999).contains(year),
		(FieldKind::Date, _, Value::Date(text)) => date_value(text).is_some(),
		(FieldKind::Boolean, Op::Is, Value::Bool(_)) => true,
		_ => false,
	};
	if fits {
		Ok(())
	} else {
		invalid(format!("That value doesn't fit “{label} {}”.", op.stored()))
	}
}

fn check_action(budget: &BudgetData, action: &SentenceAction) -> Result<(), CoreError> {
	if action.split > 0 || action.op == ActionOp::SetSplitAmount {
		return invalid("Splitting rules can only be made in Actual for now.");
	}
	let fits = match (action.op, action.field, &action.value) {
		(ActionOp::LinkSchedule, ..) => {
			return invalid("Only a bill links its own rule. Edit the bill instead.");
		}
		(ActionOp::Set, Some(field @ (Field::Payee | Field::Category)), Value::Id(id)) => {
			if let Some(id) = id {
				live(budget, field, id)?;
			}
			true
		}
		(ActionOp::Set, Some(Field::Account), Value::Id(Some(id))) => {
			live(budget, Field::Account, id)?;
			true
		}
		(
			ActionOp::Set,
			Some(Field::Notes | Field::ImportedPayee | Field::PayeeName),
			Value::Text(_),
		)
		| (ActionOp::Set, Some(Field::Amount), Value::Amount(_))
		| (ActionOp::Set, Some(Field::Cleared | Field::Reconciled), Value::Bool(_)) => true,
		(ActionOp::Set, Some(Field::Date), Value::Date(text)) => js::strict_date(text).is_some(),
		(
			ActionOp::PrependNotes | ActionOp::AppendNotes,
			None | Some(Field::Notes),
			Value::Text(text),
		) => {
			if text.is_empty() {
				return invalid("Enter the text to add to the notes.");
			}
			true
		}
		(ActionOp::DeleteTransaction, None, Value::Nothing) => true,
		_ => false,
	};
	if fits {
		Ok(())
	} else {
		invalid(match action.field {
			Some(field) => format!("That value can't be set on {}.", field_label(field)),
			None => "That action needs a different value.".to_owned(),
		})
	}
}

fn id_json(id: &Option<String>) -> String {
	id.as_deref().map_or_else(|| "null".to_owned(), string)
}

fn list_json(items: &[String]) -> String {
	write_array(items.iter().map(|item| string(item)))
}

fn value_json(value: &Value) -> String {
	match value {
		Value::Id(id) => id_json(id),
		Value::Ids(items) | Value::Texts(items) => list_json(items),
		Value::Text(text) | Value::Date(text) | Value::Month(text) => string(text),
		Value::Amount(amount) => amount.to_string(),
		Value::AmountRange(num1, num2) => write_object([
			("num1", num1.to_string().as_str()),
			("num2", num2.to_string().as_str()),
		]),
		Value::Year(year) => string(&year.to_string()),
		Value::Bool(flag) => flag.to_string(),
		Value::Nothing => "null".to_owned(),
		Value::Raw(json) => json.clone(),
	}
}

fn write_condition(condition: &SentenceCondition) -> String {
	let value = value_json(&condition.value);
	let mut members = vec![
		("op", string(condition.op.stored())),
		("field", string(condition.field.stored())),
		("value", value),
		("type", string(condition.field.type_name())),
	];
	match condition.sign {
		AmountSign::Inflow => members.push(("options", r#"{"inflow":true}"#.to_owned())),
		AmountSign::Outflow => members.push(("options", r#"{"outflow":true}"#.to_owned())),
		AmountSign::Any => {}
	}
	write_object(members.iter().map(|(key, value)| (*key, value.as_str())))
}

/// `Action.serialize`: `prepend-notes` and `append-notes` get the field
/// `notes` and type `id` from the constructor, and `delete-transaction` a
/// null field and no type (action.ts:53-87).
fn write_action(action: &SentenceAction) -> String {
	let value = value_json(&action.value);
	let (op, field, kind) = match (action.op, action.field) {
		(ActionOp::PrependNotes, _) => ("prepend-notes", string("notes"), Some("id")),
		(ActionOp::AppendNotes, _) => ("append-notes", string("notes"), Some("id")),
		(ActionOp::DeleteTransaction, _) => ("delete-transaction", "null".to_owned(), None),
		(_, Some(field)) => ("set", string(field.stored()), Some(field.type_name())),
		(_, None) => ("set", "null".to_owned(), None),
	};
	let kind = kind.map(string);
	let mut members = vec![("op", string(op)), ("field", field), ("value", value)];
	if let Some(kind) = kind {
		members.push(("type", kind));
	}
	write_object(members.iter().map(|(key, value)| (*key, value.as_str())))
}

/// The draft's items as stored text: an item equal to one already stored
/// keeps that item's text, and a new or changed one is checked and written
/// fresh. The whole text is kept when nothing changed.
fn written_items<T: PartialEq>(
	draft: &[T],
	stored: Option<(&str, Read<T>)>,
	check: impl Fn(&T) -> Result<(), CoreError>,
	write: impl Fn(&T) -> String,
) -> Result<String, CoreError> {
	let (text, stored) = stored.unwrap_or_default();
	let mut used = vec![false; stored.len()];
	let mut items = Vec::new();
	for model in draft {
		let kept = stored
			.iter()
			.enumerate()
			.find(|(index, (_, old, _))| !used[*index] && old == model)
			.map(|(index, _)| index);
		match kept {
			Some(index) => {
				used[index] = true;
				items.push(stored[index].0.clone());
			}
			None => {
				check(model)?;
				items.push(write(model));
			}
		}
	}
	let same = items.len() == stored.len() && items.iter().zip(&stored).all(|(a, (b, ..))| a == b);
	Ok(if same && !text.is_empty() {
		text.to_owned()
	} else {
		write_array(items)
	})
}

/// The rule a draft saves as: a new rule with `id`, or `existing` edited.
/// Bills' rules and rules the editor can't show are refused.
pub fn written(
	budget: &BudgetData,
	id: RuleId,
	draft: &Draft,
	existing: Option<&Rule>,
) -> Result<Rule, CoreError> {
	if let Some(rule) = existing {
		match sentence(budget, rule).map(|sentence| sentence.access) {
			Some(Access::Editable) => {}
			Some(Access::Bill(_)) => {
				return invalid("This rule belongs to a bill. Edit the bill instead.");
			}
			Some(Access::EditInActual(reasons)) => {
				return invalid(format!(
					"Veritas can't edit this rule because it has {}. Edit it in Actual.",
					reasons.join(", ")
				));
			}
			None => {
				return invalid(
					"Actual can't read this rule, so it never runs. Fix or delete it in Actual.",
				);
			}
		}
	}
	if draft.conditions.is_empty() {
		return invalid("Add at least one condition.");
	}
	if draft.actions.is_empty() {
		return invalid("Add at least one action.");
	}
	let stored_conditions = existing.and_then(|rule| {
		read_items(&rule.conditions_json, |item| read_condition(budget, item))
			.map(|items| (rule.conditions_json.as_str(), items))
	});
	let stored_actions = existing.and_then(|rule| {
		read_items(&rule.actions_json, |item| read_action(budget, item))
			.map(|items| (rule.actions_json.as_str(), items))
	});
	let rule = Rule {
		id,
		stage: draft.stage,
		conditions_op: draft.conditions_op,
		conditions_json: written_items(
			&draft.conditions,
			stored_conditions,
			|condition| check_condition(budget, condition),
			write_condition,
		)?,
		actions_json: written_items(
			&draft.actions,
			stored_actions,
			|action| check_action(budget, action),
			write_action,
		)?,
		tombstone: false,
	};
	match ParsedRule::parse(&rule).support() {
		RuleSupport::Full => Ok(rule),
		RuleSupport::ReadOnly(reasons) => invalid(format!(
			"Veritas couldn't run a rule with {}, so it can't save it.",
			reasons.join(", ")
		)),
		RuleSupport::IgnoredByActual(reason) => {
			invalid(format!("Actual wouldn't accept this rule: {reason}."))
		}
	}
}

#[cfg(test)]
mod tests {
	use serde_json::json;
	use veritas_domain::{
		Account, AccountId, BudgetType, Category, CategoryGroup, CategoryGroupId, Payee, Schedule,
	};

	use super::*;

	fn budget() -> BudgetData {
		let mut budget = BudgetData::empty(BudgetType::Envelope);
		budget.accounts.insert(
			AccountId::new("checking"),
			Account {
				id: AccountId::new("checking"),
				name: "Checking".into(),
				off_budget: false,
				closed: false,
				group: None,
				sort_order: 0.0,
				tombstone: false,
				last_reconciled: None,
			},
		);
		budget.category_groups.insert(
			CategoryGroupId::new("food"),
			CategoryGroup {
				id: CategoryGroupId::new("food"),
				name: "Food".into(),
				is_income: false,
				hidden: false,
				sort_order: 0.0,
				tombstone: false,
			},
		);
		for id in ["shopping", "groceries"] {
			budget.categories.insert(
				CategoryId::new(id),
				Category {
					id: CategoryId::new(id),
					name: id.into(),
					group: CategoryGroupId::new("food"),
					is_income: false,
					hidden: false,
					sort_order: 0.0,
					tombstone: false,
				},
			);
			budget
				.category_mapping
				.insert(CategoryId::new(id), CategoryId::new(id));
		}
		budget.payees.insert(
			PayeeId::new("amazon"),
			Payee {
				id: PayeeId::new("amazon"),
				name: "Amazon".into(),
				transfer_account: None,
				favorite: false,
				learn_categories: true,
				tombstone: false,
			},
		);
		budget
	}

	fn stored(id: &str, conditions: Json, actions: Json) -> Rule {
		Rule {
			id: RuleId::new(id),
			stage: RuleStage::Default,
			conditions_op: ConditionsOp::And,
			conditions_json: conditions.to_string(),
			actions_json: actions.to_string(),
			tombstone: false,
		}
	}

	fn amzn() -> Draft {
		Draft {
			stage: RuleStage::Pre,
			conditions_op: ConditionsOp::And,
			conditions: vec![SentenceCondition {
				field: Field::ImportedPayee,
				op: Op::Contains,
				sign: AmountSign::Any,
				value: Value::Text("AMZN".into()),
			}],
			actions: vec![
				SentenceAction {
					op: ActionOp::Set,
					field: Some(Field::Payee),
					value: Value::Id(Some("amazon".into())),
					split: 0,
				},
				SentenceAction {
					op: ActionOp::Set,
					field: Some(Field::Category),
					value: Value::Id(Some("shopping".into())),
					split: 0,
				},
			],
		}
	}

	#[test]
	fn a_draft_is_written_as_actual_serializes_a_rule() {
		let rule = written(&budget(), RuleId::new("r"), &amzn(), None).unwrap();
		assert_eq!(
			rule.conditions_json,
			r#"[{"op":"contains","field":"imported_description","value":"AMZN","type":"string"}]"#
		);
		assert_eq!(
			rule.actions_json,
			r#"[{"op":"set","field":"description","value":"amazon","type":"id"},{"op":"set","field":"category","value":"shopping","type":"id"}]"#
		);
		let read = sentence(&budget(), &rule).unwrap();
		assert_eq!(read.access, Access::Editable);
		assert_eq!(read.conditions, amzn().conditions);
		assert_eq!(read.actions, amzn().actions);
	}

	#[test]
	fn inflow_notes_and_delete_are_written_with_actuals_shapes() {
		let mut draft = amzn();
		draft.conditions = vec![SentenceCondition {
			field: Field::Amount,
			op: Op::Gt,
			sign: AmountSign::Outflow,
			value: Value::Amount(5000),
		}];
		draft.actions = vec![
			SentenceAction {
				op: ActionOp::AppendNotes,
				field: None,
				value: Value::Text(" #big".into()),
				split: 0,
			},
			SentenceAction {
				op: ActionOp::DeleteTransaction,
				field: None,
				value: Value::Nothing,
				split: 0,
			},
		];
		let rule = written(&budget(), RuleId::new("r"), &draft, None).unwrap();
		assert_eq!(
			rule.conditions_json,
			r#"[{"op":"gt","field":"amount","value":5000,"type":"number","options":{"outflow":true}}]"#
		);
		assert_eq!(
			rule.actions_json,
			r#"[{"op":"append-notes","field":"notes","value":" #big","type":"id"},{"op":"delete-transaction","field":null,"value":null}]"#
		);
	}

	#[test]
	fn an_edit_keeps_the_text_of_what_it_didnt_change() {
		// The key order and the `splitIndex: 0` Actual's editor writes stay as stored.
		let rule = stored(
			"r",
			json!([{"type": "string", "field": "imported_payee", "op": "contains", "value": "AMZN"}]),
			json!([{"type": "id", "field": "category", "op": "set", "value": "shopping", "options": {"splitIndex": 0}}]),
		);
		let budget = budget();
		let mut draft = Draft {
			stage: RuleStage::Default,
			conditions_op: ConditionsOp::And,
			conditions: sentence(&budget, &rule).unwrap().conditions,
			actions: sentence(&budget, &rule).unwrap().actions,
		};
		let same = written(&budget, rule.id.clone(), &draft, Some(&rule)).unwrap();
		assert_eq!(same, rule);
		draft.actions[0].value = Value::Id(Some("groceries".into()));
		let edited = written(&budget, rule.id.clone(), &draft, Some(&rule)).unwrap();
		assert_eq!(edited.conditions_json, rule.conditions_json);
		assert_eq!(
			edited.actions_json,
			r#"[{"op":"set","field":"category","value":"groceries","type":"id"}]"#
		);
	}

	#[test]
	fn bills_templates_and_unshown_parts_are_read_only() {
		let mut budget = budget();
		budget.schedules.insert(
			ScheduleId::new("rent"),
			Schedule {
				id: ScheduleId::new("rent"),
				name: None,
				rule: RuleId::new("bill"),
				posts_transaction: false,
				completed: false,
				next_date: None,
				tombstone: false,
			},
		);
		let payee_is = json!([{"op": "is", "field": "description", "value": "amazon"}]);
		let bill = stored(
			"bill",
			payee_is.clone(),
			json!([{"op": "link-schedule", "value": "rent"}]),
		);
		let template = stored(
			"template",
			payee_is.clone(),
			json!([{"op": "set", "field": "notes", "value": "", "options": {"template": "{{imported_payee}}"}}]),
		);
		let named = stored(
			"named",
			json!([{"op": "is", "field": "description", "value": "amazon", "customName": "x"}]),
			json!([{"op": "set", "field": "category", "value": "shopping"}]),
		);
		let split = stored(
			"split",
			payee_is,
			json!([{"op": "set-split-amount", "value": null, "options": {"splitIndex": 1, "method": "remainder"}}]),
		);
		let access = |rule: &Rule| sentence(&budget, rule).unwrap().access;
		assert_eq!(access(&bill), Access::Bill(ScheduleId::new("rent")));
		assert_eq!(
			access(&template),
			Access::EditInActual(vec!["a template action".into()])
		);
		assert_eq!(access(&named), Access::EditInActual(vec![EXTRAS.into()]));
		assert_eq!(access(&split), Access::EditInActual(vec![SPLITS.into()]));
		assert!(!sentence(&budget, &template).unwrap().runs_in_veritas);
		let draft = amzn();
		for rule in [&bill, &template, &named, &split] {
			assert!(matches!(
				written(&budget, rule.id.clone(), &draft, Some(rule)),
				Err(CoreError::Invalid(_))
			));
		}
	}

	#[test]
	fn drafts_are_checked() {
		let budget = budget();
		let refuse = |change: &dyn Fn(&mut Draft)| {
			let mut draft = amzn();
			change(&mut draft);
			written(&budget, RuleId::new("r"), &draft, None).unwrap_err()
		};
		assert!(matches!(
			refuse(&|d| d.conditions.clear()),
			CoreError::Invalid(_)
		));
		assert!(matches!(
			refuse(&|d| d.actions.clear()),
			CoreError::Invalid(_)
		));
		assert!(matches!(
			refuse(&|d| d.conditions[0].op = Op::Gt),
			CoreError::Invalid(_)
		));
		assert!(matches!(
			refuse(&|d| d.conditions[0].value = Value::Text(String::new())),
			CoreError::Invalid(_)
		));
		assert!(matches!(
			refuse(&|d| d.actions[1].value = Value::Id(Some("nope".into()))),
			CoreError::NotFound {
				entity: "category",
				..
			}
		));
		assert!(matches!(
			refuse(&|d| d.actions[0].split = 1),
			CoreError::Invalid(_)
		));
		assert!(matches!(
			refuse(&|d| {
				d.conditions[0].op = Op::Matches;
				d.conditions[0].value = Value::Text("(?<=a)b".into());
			}),
			CoreError::Invalid(_)
		));
	}

	#[test]
	fn the_list_follows_the_rules_page() {
		let mut budget = budget();
		for (id, stage, op) in [
			("b", RuleStage::Default, "is"),
			("a", RuleStage::Default, "contains"),
			("c", RuleStage::Pre, "is"),
			("done", RuleStage::Default, "is"),
		] {
			let mut rule = stored(
				id,
				json!([{"op": op, "field": "notes", "value": "x"}]),
				json!([{"op": "set", "field": "cleared", "value": true}]),
			);
			rule.stage = stage;
			budget.rules.insert(rule.id.clone(), rule);
		}
		budget.schedules.insert(
			ScheduleId::new("s"),
			Schedule {
				id: ScheduleId::new("s"),
				name: None,
				rule: RuleId::new("done"),
				posts_transaction: false,
				completed: true,
				next_date: None,
				tombstone: false,
			},
		);
		let ignored = stored(
			"ignored",
			json!([{"op": "oneOf", "field": "notes", "value": ["x"]}]),
			json!([]),
		);
		budget.rules.insert(ignored.id.clone(), ignored);
		let order: Vec<String> = sentences(&budget)
			.iter()
			.map(|s| s.id.as_str().to_owned())
			.collect();
		assert_eq!(order, ["c", "a", "b"]);
	}
}
