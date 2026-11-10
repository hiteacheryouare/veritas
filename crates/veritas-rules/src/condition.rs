//! Rule conditions: parsed the way `new Condition(...)` validates them and
//! evaluated the way `Condition.eval` does in memory
//! (reference/actual/packages/loot-core/src/server/rules/condition.ts).

use chrono::{Datelike, Days, NaiveDate};
use serde_json::Value;
use veritas_domain::YearMonth;

use crate::js;
use crate::recur::{RecurError, Recurrence};

/// Why a stored condition or action isn't one Veritas runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Gap {
	/// Actual's own constructor throws, so Actual drops the whole rule at load
	/// and never runs it either.
	RejectedByActual(String),
	/// Actual runs it; Veritas shows it read-only and keeps it as stored.
	Unsupported(String),
}

fn rejected<T>(reason: impl Into<String>) -> Result<T, Gap> {
	Err(Gap::RejectedByActual(reason.into()))
}

fn unsupported<T>(reason: impl Into<String>) -> Result<T, Gap> {
	Err(Gap::Unsupported(reason.into()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
	ImportedPayee,
	Payee,
	PayeeName,
	Account,
	Category,
	CategoryGroup,
	Notes,
	Amount,
	Date,
	Cleared,
	Reconciled,
	Transfer,
	Parent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
	Text,
	Id,
	Number,
	Date,
	Boolean,
}

impl Field {
	/// Reads a stored `field`, which is the internal column name for the
	/// renamed ones (`acct`, `description`, `imported_description`).
	pub fn from_stored(name: &str) -> Option<Field> {
		Some(match name {
			"imported_payee" | "imported_description" => Field::ImportedPayee,
			"payee" | "description" => Field::Payee,
			"payee_name" => Field::PayeeName,
			"account" | "acct" => Field::Account,
			"category" => Field::Category,
			"category_group" => Field::CategoryGroup,
			"notes" => Field::Notes,
			"amount" => Field::Amount,
			"date" => Field::Date,
			"cleared" => Field::Cleared,
			"reconciled" => Field::Reconciled,
			"transfer" => Field::Transfer,
			"parent" => Field::Parent,
			_ => return None,
		})
	}

	pub fn kind(self) -> FieldKind {
		match self {
			Field::ImportedPayee | Field::PayeeName | Field::Notes => FieldKind::Text,
			Field::Payee | Field::Account | Field::Category | Field::CategoryGroup => FieldKind::Id,
			Field::Amount => FieldKind::Number,
			Field::Date => FieldKind::Date,
			Field::Cleared | Field::Reconciled | Field::Transfer | Field::Parent => {
				FieldKind::Boolean
			}
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
	Is,
	IsNot,
	Contains,
	DoesNotContain,
	Matches,
	OneOf,
	NotOneOf,
	HasTags,
	HasAnyTag,
	IsApprox,
	IsBetween,
	Gt,
	Gte,
	Lt,
	Lte,
	OnBudget,
	OffBudget,
	/// Internal to Actual's query builder. `eval` has no case for it, so it is always false.
	And,
}

impl Op {
	pub fn from_stored(name: &str) -> Option<Op> {
		Some(match name {
			"is" => Op::Is,
			"isNot" => Op::IsNot,
			"contains" => Op::Contains,
			"doesNotContain" => Op::DoesNotContain,
			"matches" => Op::Matches,
			"oneOf" => Op::OneOf,
			"notOneOf" => Op::NotOneOf,
			"hasTags" => Op::HasTags,
			"hasAnyTag" => Op::HasAnyTag,
			"isapprox" => Op::IsApprox,
			"isbetween" => Op::IsBetween,
			"gt" => Op::Gt,
			"gte" => Op::Gte,
			"lt" => Op::Lt,
			"lte" => Op::Lte,
			"onBudget" => Op::OnBudget,
			"offBudget" => Op::OffBudget,
			"and" => Op::And,
			_ => return None,
		})
	}

	/// `isValidOp` (`shared/rules.ts:99-108`): the type's ops, minus the
	/// field's disallowed ones, plus its internal ones.
	fn valid_for(self, field: Field) -> bool {
		use Op::*;
		let by_kind = match field.kind() {
			FieldKind::Date => matches!(self, Is | IsApprox | Gt | Gte | Lt | Lte),
			FieldKind::Id => matches!(
				self,
				Is | Contains
					| Matches | OneOf
					| IsNot | DoesNotContain
					| NotOneOf | OnBudget
					| OffBudget
			),
			FieldKind::Text => matches!(
				self,
				Is | Contains
					| Matches | OneOf
					| IsNot | DoesNotContain
					| NotOneOf | HasTags
					| HasAnyTag
			),
			FieldKind::Number => matches!(self, Is | IsApprox | IsBetween | Gt | Gte | Lt | Lte),
			FieldKind::Boolean => self == Is,
		};
		let disallowed = match field {
			Field::ImportedPayee => matches!(self, HasTags | HasAnyTag),
			Field::Payee | Field::Category | Field::CategoryGroup => {
				matches!(self, OnBudget | OffBudget)
			}
			Field::Notes => matches!(self, OneOf | NotOneOf),
			_ => false,
		};
		let internal = matches!(field, Field::Category | Field::CategoryGroup) && self == And;
		(by_kind && !disallowed) || internal
	}
}

#[derive(Clone, Debug, PartialEq)]
pub enum DateValue {
	Day(NaiveDate),
	Month(YearMonth),
	Year(i32),
	Recur(Box<Recurrence>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ConditionValue {
	/// Lowercased, except for the tag ops, which keep the stored text.
	Text(String),
	/// `oneOf` on a string field: falsy entries dropped, the rest lowercased.
	TextList(Vec<String>),
	/// Ids stay as stored (after payee and category mappings); `None` is null.
	Id(Option<String>),
	IdList(Vec<Option<String>>),
	Number(f64),
	Between(f64, f64),
	Date(DateValue),
	Bool(bool),
	/// `onBudget`, `offBudget` and `and` don't look at the value.
	Ignored,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmountSign {
	Any,
	Inflow,
	Outflow,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Condition {
	pub field: Field,
	pub op: Op,
	pub value: ConditionValue,
	pub sign: AmountSign,
}

/// A field's value as `eval` sees it. `Absent` is a key that isn't on the
/// object at all, which fails every op; strings are lowercased by `eval`.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldValue {
	Absent,
	Null,
	Text(String),
	Number(f64),
	Bool(bool),
}

/// The JS string length, in UTF-16 units, that `parseDateString` switches on.
fn js_length(text: &str) -> usize {
	text.encode_utf16().count()
}

/// `parseDateString` (`rule-utils.ts:239-266`).
fn parse_date_string(text: &str) -> Result<DateValue, Gap> {
	let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
	let exotic_iso = text.starts_with(['+', '-']) || text.contains('W');
	match js_length(text) {
		10 => match js::strict_date(text) {
			Some(date) => Ok(DateValue::Day(date)),
			None if exotic_iso => {
				unsupported(format!("{text:?} is an ISO date form Veritas doesn't read"))
			}
			None => rejected(format!("{text:?} is not a date")),
		},
		7 => {
			let month = text
				.split_once('-')
				.filter(|(year, month)| {
					year.len() == 4 && month.len() == 2 && digits(year) && digits(month)
				})
				.and_then(|_| YearMonth::parse(text).ok());
			match month {
				Some(month) => Ok(DateValue::Month(month)),
				None if exotic_iso => unsupported(format!(
					"{text:?} is an ISO month form Veritas doesn't read"
				)),
				None => rejected(format!("{text:?} is not a month")),
			}
		}
		4 if digits(text) => text
			.parse()
			.map(DateValue::Year)
			.or_else(|_| rejected(format!("{text:?} is not a year"))),
		_ => rejected(format!("{text:?} is not a date")),
	}
}

pub(crate) fn recur_gap(error: RecurError) -> Gap {
	match error {
		RecurError::Rejected(reason) => Gap::RejectedByActual(reason),
		other => Gap::Unsupported(other.to_string()),
	}
}

/// Parses a date condition's value the way `CONDITION_TYPES.date.parse` does.
pub fn parse_date_value(op: Op, value: Option<&Value>) -> Result<DateValue, Gap> {
	let parsed = match value {
		None | Some(Value::Null) => return rejected("a date condition can't be empty"),
		Some(Value::String(text)) => parse_date_string(text)?,
		Some(object @ Value::Object(config))
			if config.get("frequency").is_some_and(|f| !f.is_null()) =>
		{
			DateValue::Recur(Box::new(Recurrence::parse(object).map_err(recur_gap)?))
		}
		Some(_) => return rejected("invalid date format"),
	};
	let fits = match op {
		Op::IsApprox => matches!(parsed, DateValue::Day(_) | DateValue::Recur(_)),
		Op::Gt | Op::Gte | Op::Lt | Op::Lte => matches!(parsed, DateValue::Day(_)),
		_ => true,
	};
	if !fits {
		return rejected(format!("invalid date value for {op:?}"));
	}
	Ok(parsed)
}

fn amount_sign(options: Option<&Value>) -> AmountSign {
	match options {
		Some(Value::Object(options)) if js::truthy(options.get("outflow")) => AmountSign::Outflow,
		Some(Value::Object(options)) if js::truthy(options.get("inflow")) => AmountSign::Inflow,
		_ => AmountSign::Any,
	}
}

impl Condition {
	/// Parses one stored condition item (`{op, field, value, type, options?}`).
	pub fn parse(item: &Value) -> Result<Condition, Gap> {
		let field_name = item
			.get("field")
			.and_then(Value::as_str)
			.unwrap_or_default();
		let Some(field) = Field::from_stored(field_name) else {
			return rejected(format!("invalid condition field {field_name:?}"));
		};
		let op_name = item.get("op").and_then(Value::as_str).unwrap_or_default();
		let op = Op::from_stored(op_name)
			.filter(|op| op.valid_for(field))
			.ok_or_else(|| {
				Gap::RejectedByActual(format!("invalid condition operator {op_name:?}"))
			})?;
		let value = item.get("value");
		let kind = field.kind();
		if matches!(
			kind,
			FieldKind::Date | FieldKind::Number | FieldKind::Boolean
		) && value.is_none_or(Value::is_null)
		{
			return rejected("field cannot be empty");
		}
		let value = match kind {
			FieldKind::Text => parse_text(op, value)?,
			FieldKind::Id => parse_id(op, value)?,
			FieldKind::Number => parse_number(op, value)?,
			FieldKind::Date => ConditionValue::Date(parse_date_value(op, value)?),
			FieldKind::Boolean => match value {
				Some(Value::Bool(flag)) => ConditionValue::Bool(*flag),
				_ => return rejected("value must be a boolean"),
			},
		};
		Ok(Condition {
			field,
			op,
			value,
			sign: amount_sign(item.get("options")),
		})
	}

	/// `Condition.eval`. A recurrence that can't be expanded is an error,
	/// as it would throw out of Actual's `runRules`.
	pub fn eval(
		&self,
		field_value: FieldValue,
		account_off_budget: Option<bool>,
	) -> Result<bool, RecurError> {
		let kind = self.field.kind();
		let mut value = match field_value {
			FieldValue::Absent | FieldValue::Null if kind == FieldKind::Text => {
				FieldValue::Text(String::new())
			}
			FieldValue::Absent => return Ok(false),
			FieldValue::Text(text) => FieldValue::Text(text.to_lowercase()),
			other => other,
		};
		if let FieldValue::Number(amount) = value {
			match self.sign {
				AmountSign::Outflow if amount > 0.0 => return Ok(false),
				AmountSign::Outflow => value = FieldValue::Number(-amount),
				AmountSign::Inflow if amount < 0.0 => return Ok(false),
				_ => {}
			}
		}

		use ConditionValue as V;
		use FieldValue as F;
		Ok(match (self.op, &value, &self.value) {
			(Op::Is | Op::IsApprox, F::Null, V::Date(_)) => false,
			(Op::Is | Op::IsApprox, F::Text(text), V::Date(date)) => {
				let Some(day) = js::parse_date(text) else {
					return Ok(false);
				};
				match (self.op, date) {
					(Op::IsApprox, DateValue::Recur(recurrence)) => recurrence.occurs_between(
						day.checked_sub_days(Days::new(2)).unwrap_or(day),
						day.checked_add_days(Days::new(2)).unwrap_or(day),
					)?,
					(_, DateValue::Recur(recurrence)) => recurrence.occurs_on(day)?,
					(Op::IsApprox, DateValue::Day(target)) => {
						let low = target.checked_sub_days(Days::new(2)).unwrap_or(*target);
						let high = target.checked_add_days(Days::new(2)).unwrap_or(*target);
						let text = text.as_str();
						text >= low.to_string().as_str() && text <= high.to_string().as_str()
					}
					(_, DateValue::Day(target)) => *text == target.to_string(),
					(_, DateValue::Month(month)) => {
						day.year() == month.year() && day.month() == month.month()
					}
					(_, DateValue::Year(year)) => day.year() == *year,
				}
			}
			(Op::IsApprox, F::Number(amount), V::Number(target)) => {
				let threshold = js::math_round(target.abs() * 0.075);
				*amount >= target - threshold && *amount <= target + threshold
			}
			(Op::Is | Op::IsApprox, _, expected) => strictly_equal(&value, expected),
			(Op::IsNot, _, expected) => !strictly_equal(&value, expected),
			(Op::IsBetween, F::Number(amount), V::Between(num1, num2)) => {
				let (low, high) = if num1 < num2 {
					(num1, num2)
				} else {
					(num2, num1)
				};
				amount >= low && amount <= high
			}
			(_, F::Null, _) if self.op != Op::OnBudget && self.op != Op::OffBudget => false,
			(Op::Contains, F::Text(text), V::Text(needle) | V::Id(Some(needle))) => {
				text.contains(needle.as_str())
			}
			(Op::DoesNotContain, F::Text(text), V::Text(needle) | V::Id(Some(needle))) => {
				!text.contains(needle.as_str())
			}
			(Op::OneOf, F::Text(text), V::TextList(list)) => list.iter().any(|entry| entry == text),
			(Op::NotOneOf, F::Text(text), V::TextList(list)) => {
				!list.iter().any(|entry| entry == text)
			}
			(Op::OneOf, F::Text(text), V::IdList(list)) => list
				.iter()
				.any(|entry| entry.as_deref() == Some(text.as_str())),
			(Op::NotOneOf, F::Text(text), V::IdList(list)) => !list
				.iter()
				.any(|entry| entry.as_deref() == Some(text.as_str())),
			(Op::HasTags, F::Text(text), V::Text(tags)) => extract_tags(tags)
				.iter()
				.all(|tag| has_tag(text, &tag.to_lowercase())),
			(Op::HasAnyTag, F::Text(text), V::Text(tags)) => extract_tags(tags)
				.iter()
				.any(|tag| has_tag(text, &tag.to_lowercase())),
			(
				Op::Gt | Op::Gte | Op::Lt | Op::Lte,
				F::Text(text),
				V::Date(DateValue::Day(target)),
			) => {
				let Some(day) = js::parse_date(text) else {
					return Ok(false);
				};
				let equal = *text == target.to_string();
				match self.op {
					Op::Gt => day > *target,
					Op::Gte => equal || day > *target,
					Op::Lt => day < *target,
					_ => equal || day < *target,
				}
			}
			(Op::Gt, F::Number(amount), V::Number(target)) => amount > target,
			(Op::Gte, F::Number(amount), V::Number(target)) => amount >= target,
			(Op::Lt, F::Number(amount), V::Number(target)) => amount < target,
			(Op::Lte, F::Number(amount), V::Number(target)) => amount <= target,
			(Op::OnBudget, _, _) => account_off_budget == Some(false),
			(Op::OffBudget, _, _) => account_off_budget == Some(true),
			_ => false,
		})
	}
}

/// JS `===` between a field value and a parsed condition value.
fn strictly_equal(field: &FieldValue, expected: &ConditionValue) -> bool {
	match (field, expected) {
		(FieldValue::Text(text), ConditionValue::Text(value)) => text == value,
		(FieldValue::Text(text), ConditionValue::Id(Some(value))) => text == value,
		(FieldValue::Null, ConditionValue::Id(None)) => true,
		(FieldValue::Number(amount), ConditionValue::Number(value)) => amount == value,
		(FieldValue::Bool(flag), ConditionValue::Bool(value)) => flag == value,
		_ => false,
	}
}

fn parse_text(op: Op, value: Option<&Value>) -> Result<ConditionValue, Gap> {
	if matches!(op, Op::OneOf | Op::NotOneOf) {
		let Some(Value::Array(entries)) = value else {
			return rejected("oneOf must have an array value");
		};
		let mut list = Vec::new();
		for entry in entries.iter().filter(|entry| js::truthy(Some(entry))) {
			match entry {
				Value::String(text) => list.push(text.to_lowercase()),
				_ => return unsupported("a oneOf entry that isn't text"),
			}
		}
		return Ok(ConditionValue::TextList(list));
	}
	let Some(Value::String(text)) = value else {
		return rejected("invalid string value");
	};
	let needs_text = matches!(
		op,
		Op::Contains | Op::Matches | Op::DoesNotContain | Op::HasTags | Op::HasAnyTag
	);
	if needs_text && text.is_empty() {
		return rejected(format!("{op:?} must have non-empty string"));
	}
	match op {
		Op::Matches => unsupported("regular expression conditions"),
		Op::HasTags | Op::HasAnyTag => Ok(ConditionValue::Text(text.clone())),
		_ => Ok(ConditionValue::Text(text.to_lowercase())),
	}
}

fn parse_id(op: Op, value: Option<&Value>) -> Result<ConditionValue, Gap> {
	let id = |value: &Value| match value {
		Value::String(id) => Ok(Some(id.clone())),
		Value::Null => Ok(None),
		_ => unsupported("an id that isn't text"),
	};
	match op {
		Op::OneOf | Op::NotOneOf | Op::And => {
			let Some(Value::Array(entries)) = value else {
				return rejected("oneOf must have an array value");
			};
			if op == Op::And {
				return Ok(ConditionValue::Ignored);
			}
			Ok(ConditionValue::IdList(
				entries.iter().map(id).collect::<Result<_, _>>()?,
			))
		}
		Op::OnBudget | Op::OffBudget => Ok(ConditionValue::Ignored),
		Op::Matches => unsupported("regular expression conditions"),
		Op::Contains | Op::DoesNotContain => match value {
			Some(Value::String(needle)) => Ok(ConditionValue::Id(Some(needle.clone()))),
			_ => unsupported("contains with a value that isn't text"),
		},
		_ => match value {
			Some(value) => Ok(ConditionValue::Id(id(value)?)),
			None => unsupported("an id condition without a value"),
		},
	}
}

fn parse_number(op: Op, value: Option<&Value>) -> Result<ConditionValue, Gap> {
	let parsed = match value {
		Some(Value::Number(number)) => number.as_f64().map(ConditionValue::Number),
		Some(Value::Object(between)) => match (between.get("num1"), between.get("num2")) {
			(Some(Value::Number(num1)), Some(Value::Number(num2))) => num1
				.as_f64()
				.zip(num2.as_f64())
				.map(|(num1, num2)| ConditionValue::Between(num1, num2)),
			_ => None,
		},
		_ => None,
	};
	match (op, parsed) {
		(_, None) => rejected("value must be a number or between amount"),
		(Op::IsBetween, Some(between @ ConditionValue::Between(..))) => Ok(between),
		(Op::IsBetween, _) => rejected("invalid between value"),
		(_, Some(literal @ ConditionValue::Number(_))) => Ok(literal),
		_ => rejected("invalid number value"),
	}
}

/// `extractTagsForFilter` (`shared/tags.ts:7-19`): runs of characters that
/// are neither `#` nor whitespace, each given one `#`, without repeats.
fn extract_tags(value: &str) -> Vec<String> {
	let mut tags: Vec<String> = Vec::new();
	for token in value.split(|c: char| c == '#' || js::is_js_whitespace(c)) {
		if token.is_empty() {
			continue;
		}
		let tag = format!("#{token}");
		if !tags.contains(&tag) {
			tags.push(tag);
		}
	}
	tags
}

/// The regex `(?<!#)<tag>([\s#]|$)` from `Condition.eval`, matched by hand.
fn has_tag(text: &str, tag: &str) -> bool {
	let mut from = 0;
	while let Some(found) = text[from..].find(tag) {
		let at = from + found;
		let after = &text[at + tag.len()..];
		let free_before = !text[..at].ends_with('#');
		let ends = after
			.chars()
			.next()
			.is_none_or(|c| c == '#' || js::is_js_whitespace(c));
		if free_before && ends {
			return true;
		}
		from = at + tag.chars().next().map_or(1, char::len_utf8);
	}
	false
}

#[cfg(test)]
mod tests {
	use serde_json::json;

	use super::*;

	fn condition(op: &str, field: &str, value: Value) -> Condition {
		Condition::parse(&json!({"op": op, "field": field, "value": value})).unwrap()
	}

	fn text(value: &str) -> FieldValue {
		FieldValue::Text(value.into())
	}

	fn eval(condition: &Condition, value: FieldValue) -> bool {
		condition.eval(value, None).unwrap()
	}

	fn parse_gap(op: &str, field: &str, value: Value) -> Gap {
		Condition::parse(&json!({"op": op, "field": field, "value": value})).unwrap_err()
	}

	// The vectors below are Actual's own, from
	// reference/actual/packages/loot-core/src/server/rules/index.test.ts.

	#[test]
	fn null_and_missing_fields() {
		assert!(!eval(
			&condition("contains", "notes", json!("foo")),
			FieldValue::Null
		));
		assert!(!eval(
			&condition("oneOf", "imported_payee", json!(["foo"])),
			FieldValue::Null
		));
		for op in ["gt", "gte", "lt", "lte", "isapprox"] {
			assert!(!eval(
				&condition(op, "date", json!("2020-01-01")),
				FieldValue::Null
			));
		}
		assert!(eval(
			&condition("is", "payee", json!(null)),
			FieldValue::Null
		));
		assert!(eval(&condition("is", "notes", json!("")), FieldValue::Null));
		assert!(!eval(
			&condition("is", "payee", json!(null)),
			FieldValue::Absent
		));
		assert!(!eval(
			&condition("contains", "notes", json!("foo")),
			FieldValue::Absent
		));
	}

	#[test]
	fn date_ops_and_their_value_shapes() {
		assert!(matches!(
			parse_gap("isapprox", "date", json!("2020-08")),
			Gap::RejectedByActual(_)
		));
		for op in ["gt", "gte", "lt", "lte"] {
			assert!(matches!(
				parse_gap(op, "date", json!("2020-08")),
				Gap::RejectedByActual(_)
			));
		}
		assert!(matches!(
			parse_gap("is", "date", json!("2020-14-01")),
			Gap::RejectedByActual(_)
		));
		assert!(matches!(
			parse_gap("is", "date", json!("2020-05-53")),
			Gap::RejectedByActual(_)
		));
		assert!(matches!(
			parse_gap("is", "date", json!("2020-0")),
			Gap::RejectedByActual(_)
		));

		let day = condition("is", "date", json!("2020-08-10"));
		assert!(!eval(&day, text("2020-08-05")));
		assert!(eval(&day, text("2020-08-10")));
		let month = condition("is", "date", json!("2020-08"));
		assert!(eval(&month, text("2020-08-05")));
		assert!(!eval(&month, text("2020-09-10")));
		let year = condition("is", "date", json!("2020"));
		assert!(eval(&year, text("2020-09-10")));
		assert!(!eval(&year, text("2019-09-10")));
		let approx = condition("isapprox", "date", json!("2020-08-07"));
		assert!(!eval(&approx, text("2020-08-04")));
		assert!(eval(&approx, text("2020-08-05")));
		assert!(eval(&approx, text("2020-08-09")));
		assert!(!eval(&approx, text("2020-08-10")));

		let gt = condition("gt", "date", json!("2020-08-10"));
		assert!(eval(&gt, text("2020-08-11")) && !eval(&gt, text("2020-08-10")));
		let gte = condition("gte", "date", json!("2020-08-10"));
		assert!(eval(&gte, text("2020-08-10")) && !eval(&gte, text("2020-08-09")));
		let lt = condition("lt", "date", json!("2020-08-10"));
		assert!(eval(&lt, text("2020-08-09")) && !eval(&lt, text("2020-08-10")));
		let lte = condition("lte", "date", json!("2020-08-10"));
		assert!(eval(&lte, text("2020-08-10")) && !eval(&lte, text("2020-08-11")));
	}

	#[test]
	fn recurring_dates_match_on_occurrences_and_within_two_days() {
		let monthly = json!({"start": "2019-01-01", "frequency": "monthly", "patterns": [{"type": "day", "value": 15}]});
		let is = condition("is", "date", monthly.clone());
		assert!(!eval(&is, text("2018-03-15")));
		assert!(eval(&is, text("2019-03-15")));
		assert!(eval(&is, text("2020-06-15")));
		assert!(!eval(&is, text("2020-06-10")));

		let quarterly = condition(
			"is",
			"date",
			json!({"start": "2018-01-12", "frequency": "monthly", "interval": 3}),
		);
		assert!(eval(&quarterly, text("2019-01-12")));
		assert!(eval(&quarterly, text("2019-04-12")));
		assert!(eval(&quarterly, text("2020-07-12")));
		assert!(!eval(&quarterly, text("2020-06-12")));

		let approx = condition("isapprox", "date", monthly);
		assert!(!eval(&approx, text("2019-03-12")));
		assert!(eval(&approx, text("2019-03-13")));
		assert!(eval(&approx, text("2019-03-17")));
		assert!(!eval(&approx, text("2019-03-18")));
		assert!(eval(&approx, text("2019-05-17")));
	}

	#[test]
	fn ids_compare_lowercased_fields_against_stored_values() {
		let is = condition("is", "payee", json!("foo"));
		assert!(eval(&is, text("foo")) && eval(&is, text("FOO")) && !eval(&is, text("foo2")));
		let one_of = condition("oneOf", "payee", json!(["foo", "bar"]));
		assert!(
			eval(&one_of, text("FOO"))
				&& eval(&one_of, text("Bar"))
				&& !eval(&one_of, text("bar2"))
		);
		// An id with capitals never matches, because only the field is lowercased.
		assert!(!eval(&condition("is", "payee", json!("Foo")), text("Foo")));
		// `contains` on an id looks at the id itself, not the payee's name.
		assert!(!eval(
			&condition("contains", "payee", json!("amzn")),
			text("6f1c-uuid")
		));
	}

	#[test]
	fn strings_match_case_insensitively() {
		let is = condition("is", "notes", json!("foo"));
		assert!(eval(&is, text("FOO")) && !eval(&is, text("foo2")));
		let one_of = condition("oneOf", "imported_payee", json!(["foo", "bar"]));
		assert!(eval(&one_of, text("Bar")) && !eval(&one_of, text("bar2")));
		let contains = condition("contains", "notes", json!("foo"));
		for hit in ["bar foo baz", "bar FOOb", "foo", "bfoo"] {
			assert!(eval(&contains, text(hit)), "{hit}");
		}
		for miss in ["bfo", "f o o"] {
			assert!(!eval(&contains, text(miss)), "{miss}");
		}
		assert!(matches!(
			parse_gap("matches", "notes", json!("^fo*$")),
			Gap::Unsupported(_)
		));
	}

	#[test]
	fn number_values_and_ops() {
		assert!(matches!(
			parse_gap("isapprox", "amount", json!("hello")),
			Gap::RejectedByActual(_)
		));
		assert!(matches!(
			parse_gap("is", "amount", json!({"num1": 0, "num2": 10})),
			Gap::RejectedByActual(_)
		));
		assert!(matches!(
			parse_gap("isbetween", "amount", json!(34.22)),
			Gap::RejectedByActual(_)
		));
		assert!(matches!(
			parse_gap("isbetween", "amount", json!({"num1": 0})),
			Gap::RejectedByActual(_)
		));

		let n = |x: f64| FieldValue::Number(x);
		let approx = condition("isapprox", "amount", json!(1535));
		assert!(eval(&approx, n(1540.0)) && eval(&approx, n(1650.0)));
		assert!(!eval(&approx, n(1300.0)) && !eval(&approx, n(1800.0)));
		// t = Math.round(100 × 0.075) = Math.round(7.5) = 8
		let hundred = condition("isapprox", "amount", json!(100));
		assert!(eval(&hundred, n(108.0)) && !eval(&hundred, n(109.0)));

		let between = condition("isbetween", "amount", json!({"num1": 32, "num2": 86}));
		assert!(!eval(&between, n(30.0)) && eval(&between, n(32.0)) && eval(&between, n(86.0)));
		let reversed = condition("isbetween", "amount", json!({"num1": -16, "num2": -20}));
		assert!(eval(&reversed, n(-18.0)) && !eval(&reversed, n(-12.0)));

		assert!(!eval(&condition("gt", "amount", json!(1.55)), n(1.55)));
		assert!(eval(&condition("gte", "amount", json!(1.55)), n(1.55)));
		assert!(eval(&condition("lt", "amount", json!(1.55)), n(1.5)));
		assert!(!eval(&condition("lte", "amount", json!(1.55)), n(1.67)));
	}

	#[test]
	fn inflow_and_outflow_both_accept_zero() {
		let n = |x: f64| FieldValue::Number(x);
		let outflow = Condition::parse(
			&json!({"op": "is", "field": "amount", "value": 500, "options": {"outflow": true}}),
		)
		.unwrap();
		assert!(eval(&outflow, n(-500.0)));
		assert!(!eval(&outflow, n(500.0)));
		let zero_out = Condition::parse(
			&json!({"op": "is", "field": "amount", "value": 0, "options": {"outflow": true}}),
		)
		.unwrap();
		let zero_in = Condition::parse(
			&json!({"op": "is", "field": "amount", "value": 0, "options": {"inflow": true}}),
		)
		.unwrap();
		assert!(eval(&zero_out, n(0.0)) && eval(&zero_in, n(0.0)));
		assert!(!eval(&zero_in, n(-1.0)));
	}

	#[test]
	fn booleans_need_a_boolean_and_a_present_field() {
		assert!(matches!(
			parse_gap("is", "cleared", json!("true")),
			Gap::RejectedByActual(_)
		));
		assert!(matches!(
			parse_gap("is", "cleared", json!(null)),
			Gap::RejectedByActual(_)
		));
		let cleared = condition("is", "cleared", json!(true));
		assert!(eval(&cleared, FieldValue::Bool(true)));
		assert!(!eval(&cleared, FieldValue::Bool(false)));
		assert!(!eval(&cleared, FieldValue::Absent));
	}

	#[test]
	fn tags_need_whole_tags_and_ignore_escaped_ones() {
		let has = condition("hasTags", "notes", json!("#food ##ignored"));
		assert!(eval(&has, text("lunch #Food #ignored")));
		assert!(!eval(&has, text("lunch #food")));
		assert!(!eval(&has, text("lunch #foods #ignored")));
		assert!(!eval(&has, text("lunch ##food #ignored")));
		let any = condition("hasAnyTag", "notes", json!("#a #b"));
		assert!(eval(&any, text("x #b")) && !eval(&any, text("x #c")));
		// Nothing that parses as a tag: hasTags is vacuously true, hasAnyTag false.
		assert!(eval(
			&condition("hasTags", "notes", json!("###")),
			text("anything")
		));
		assert!(!eval(
			&condition("hasAnyTag", "notes", json!("###")),
			text("anything")
		));
		assert!(matches!(
			parse_gap("hasTags", "imported_payee", json!("#a")),
			Gap::RejectedByActual(_)
		));
	}

	#[test]
	fn internal_names_and_invalid_combinations() {
		let account = condition("is", "acct", json!("a1"));
		assert_eq!(account.field, Field::Account);
		assert_eq!(
			condition("is", "description", json!("p1")).field,
			Field::Payee
		);
		assert!(matches!(
			parse_gap("oneOf", "notes", json!(["x"])),
			Gap::RejectedByActual(_)
		));
		assert!(matches!(
			parse_gap("onBudget", "payee", json!(null)),
			Gap::RejectedByActual(_)
		));
		assert!(matches!(
			parse_gap("is", "saved", json!("x")),
			Gap::RejectedByActual(_)
		));
		assert!(matches!(
			parse_gap("is", "nonsense", json!("x")),
			Gap::RejectedByActual(_)
		));
		assert!(matches!(
			parse_gap("isbetween", "date", json!("2020-01-01")),
			Gap::RejectedByActual(_)
		));
		assert!(!eval(&condition("and", "category", json!([])), text("c1")));
	}

	#[test]
	fn on_and_off_budget_read_the_original_account() {
		let on = condition("onBudget", "account", json!(null));
		assert!(on.eval(text("a1"), Some(false)).unwrap());
		assert!(!on.eval(text("a1"), Some(true)).unwrap());
		assert!(!on.eval(text("a1"), None).unwrap());
		let off = condition("offBudget", "account", json!(null));
		assert!(off.eval(text("a1"), Some(true)).unwrap());
	}
}
