//! Bills (R-BILL-1, R-BILL-4): the friendly model of an Actual schedule and
//! the rule it owns. A new bill's rule is written exactly as Actual's schedule
//! editor writes it (`dc/components/schedules/schedule-edit-utils.ts:45-58`,
//! keys in `RecurringSchedulePicker.tsx:74-95` order); an edit rewrites only
//! the four schedule conditions, as `updateConditions` and `updateActions`
//! do (`lc/server/schedules/app.ts:107-173`), so anything else on the rule
//! survives byte for byte.

use chrono::Weekday;
use serde_json::Value;
use veritas_domain::{
	AccountId, BudgetData, CategoryId, CivilDate, CoreError, Money, PayeeId, Rule, Schedule,
	ScheduleId,
};

use crate::condition::{DateValue, Field, Op, parse_date_value};
use crate::js;
use crate::jsontext::{array_items, string, with_member, write_array, write_object};
use crate::recur::Frequency;
use crate::schedule::{next_date_from, validate_schedule_date};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BillAmount {
	/// `amount is`: exactly this.
	Exact(Money),
	/// `amount isapprox`: about this, within `Math.round(|n| × 0.075)`.
	About(Money),
	/// `amount isbetween`; posts the rounded middle.
	Between(Money, Money),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepeatPattern {
	/// A day of the month, 1 to 31, or −1 for the last day.
	Day(i8),
	/// The nth weekday of the month, 1 to 5, or −1 for the last one.
	Weekday(Weekday, i8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeekendMove {
	Before,
	After,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepeatEnd {
	Never,
	AfterOccurrences(u32),
	OnDate(CivilDate),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repeat {
	pub start: CivilDate,
	pub interval: u32,
	pub frequency: Frequency,
	/// Monthly only; empty means the start's day of the month.
	pub patterns: Vec<RepeatPattern>,
	/// `None` leaves weekends alone.
	pub weekend: Option<WeekendMove>,
	pub end: RepeatEnd,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BillDate {
	Once(CivilDate),
	Repeat(Repeat),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bill {
	pub name: Option<String>,
	pub payee: Option<PayeeId>,
	pub account: Option<AccountId>,
	/// Negative for a bill, positive for income such as a paycheck (R-BILL-5).
	pub amount: BillAmount,
	pub date: BillDate,
	/// Kept as a plain `set category` action on the schedule's rule.
	pub category: Option<CategoryId>,
	/// R-BILL-2: auto-post, or confirm first.
	pub posts_transaction: bool,
}

/// A schedule read back as a bill. `custom_rule` says the rule also holds
/// conditions or actions the bill doesn't show; an edit keeps them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BillView {
	pub id: ScheduleId,
	pub bill: Bill,
	pub completed: bool,
	pub next_date: Option<CivilDate>,
	pub custom_rule: bool,
}

/// `schedules_json_paths`: where each schedule condition sits in the rule's
/// conditions. The store upserts these four columns by `schedule_id`, never
/// with `INSERT OR REPLACE` (decision 4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonPaths {
	pub payee: Option<String>,
	pub account: Option<String>,
	pub amount: Option<String>,
	pub date: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduleRule {
	pub conditions_json: String,
	pub actions_json: String,
	pub json_paths: JsonPaths,
}

/// What saving a bill over an existing schedule writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditedRule {
	pub conditions_json: String,
	/// `None` when the actions don't change.
	pub actions_json: Option<String>,
	pub json_paths: JsonPaths,
	/// The account or date changed, so the next date is recomputed from
	/// today with a reset write (`app.ts:461-477`).
	pub reset_next_date: Option<Option<CivilDate>>,
}

fn invalid(message: impl Into<String>) -> CoreError {
	CoreError::Invalid(message.into())
}

fn weekday_code(weekday: Weekday) -> &'static str {
	match weekday {
		Weekday::Sun => "SU",
		Weekday::Mon => "MO",
		Weekday::Tue => "TU",
		Weekday::Wed => "WE",
		Weekday::Thu => "TH",
		Weekday::Fri => "FR",
		Weekday::Sat => "SA",
	}
}

impl Repeat {
	/// The RecurConfig the schedule editor writes, every key present.
	pub fn to_json(&self) -> String {
		let frequency = match self.frequency {
			Frequency::Daily => "daily",
			Frequency::Weekly => "weekly",
			Frequency::Monthly => "monthly",
			Frequency::Yearly => "yearly",
		};
		let patterns = write_array(self.patterns.iter().map(|pattern| match pattern {
			RepeatPattern::Day(day) => {
				write_object([("value", day.to_string().as_str()), ("type", "\"day\"")])
			}
			RepeatPattern::Weekday(weekday, nth) => {
				let code = string(weekday_code(*weekday));
				write_object([("value", nth.to_string().as_str()), ("type", code.as_str())])
			}
		}));
		let (end_mode, occurrences, end_date) = match self.end {
			RepeatEnd::Never => ("never", 1, self.start),
			RepeatEnd::AfterOccurrences(count) => ("after_n_occurrences", count, self.start),
			RepeatEnd::OnDate(date) => ("on_date", 1, date),
		};
		let mode = match self.weekend {
			Some(WeekendMove::After) => "after",
			_ => "before",
		};
		write_object([
			("start", string(&self.start.to_string()).as_str()),
			("interval", self.interval.to_string().as_str()),
			("frequency", string(frequency).as_str()),
			("patterns", patterns.as_str()),
			(
				"skipWeekend",
				if self.weekend.is_some() {
					"true"
				} else {
					"false"
				},
			),
			("weekendSolveMode", string(mode).as_str()),
			("endMode", string(end_mode).as_str()),
			("endOccurrences", occurrences.to_string().as_str()),
			("endDate", string(&end_date.to_string()).as_str()),
		])
	}

	/// Reads a RecurConfig. Shapes a bill can't show come back as `None`, and
	/// the bill is then shown read-only.
	pub fn from_json(value: &Value) -> Option<Repeat> {
		let config = value.as_object()?;
		let start = config
			.get("start")
			.and_then(Value::as_str)
			.and_then(js::strict_date)?
			.into();
		let frequency = match config.get("frequency")?.as_str()? {
			"daily" => Frequency::Daily,
			"weekly" => Frequency::Weekly,
			"monthly" => Frequency::Monthly,
			"yearly" => Frequency::Yearly,
			_ => return None,
		};
		let interval = match config.get("interval") {
			Some(value) if js::truthy(Some(value)) => u32::try_from(js::integer(value)?)
				.ok()
				.filter(|n| *n >= 1)?,
			_ => 1,
		};
		let mut patterns = Vec::new();
		if frequency == Frequency::Monthly {
			for pattern in config
				.get("patterns")
				.and_then(Value::as_array)
				.into_iter()
				.flatten()
			{
				let value = i8::try_from(pattern.get("value").and_then(js::integer)?).ok()?;
				patterns.push(match pattern.get("type")?.as_str()? {
					"day" => RepeatPattern::Day(value),
					code => RepeatPattern::Weekday(
						["SU", "MO", "TU", "WE", "TH", "FR", "SA"]
							.iter()
							.position(|c| *c == code)
							.and_then(|n| {
								Weekday::try_from(u8::try_from((n + 6) % 7).ok()?).ok()
							})?,
						value,
					),
				});
			}
		}
		let weekend = if js::truthy(config.get("skipWeekend")) {
			match config.get("weekendSolveMode")?.as_str()? {
				"before" => Some(WeekendMove::Before),
				"after" => Some(WeekendMove::After),
				_ => return None,
			}
		} else {
			None
		};
		let end = match config.get("endMode").and_then(Value::as_str) {
			None | Some("never") => RepeatEnd::Never,
			Some("after_n_occurrences") => RepeatEnd::AfterOccurrences(
				u32::try_from(js::integer(config.get("endOccurrences")?)?).ok()?,
			),
			Some("on_date") => {
				RepeatEnd::OnDate(js::strict_date(config.get("endDate")?.as_str()?)?.into())
			}
			Some(_) => return None,
		};
		Some(Repeat {
			start,
			interval,
			frequency,
			patterns,
			weekend,
			end,
		})
	}
}

impl BillDate {
	fn to_json(&self) -> String {
		match self {
			BillDate::Once(date) => string(&date.to_string()),
			BillDate::Repeat(repeat) => repeat.to_json(),
		}
	}
}

impl BillAmount {
	fn op(self) -> &'static str {
		match self {
			BillAmount::Exact(_) => "is",
			BillAmount::About(_) => "isapprox",
			BillAmount::Between(..) => "isbetween",
		}
	}

	fn to_json(self) -> String {
		match self {
			BillAmount::Exact(amount) | BillAmount::About(amount) => amount.minor().to_string(),
			BillAmount::Between(num1, num2) => write_object([
				("num1", num1.minor().to_string().as_str()),
				("num2", num2.minor().to_string().as_str()),
			]),
		}
	}
}

impl Bill {
	/// R-BILL-4: a bill must make a schedule Actual reads exactly as Veritas
	/// does, so it passes Actual's own API checks, has a date to land on,
	/// and avoids the pattern mixes rschedule walks surprisingly.
	pub fn validate(&self, today: CivilDate) -> Result<(), CoreError> {
		let raw: Value = serde_json::from_str(&self.date.to_json())
			.map_err(|error| invalid(error.to_string()))?;
		validate_schedule_date(&raw).map_err(invalid)?;
		if let BillAmount::Between(low, high) = self.amount
			&& low == high
		{
			return Err(invalid("A range needs two different amounts"));
		}
		if let BillDate::Repeat(repeat) = &self.date {
			if repeat.frequency != Frequency::Monthly && !repeat.patterns.is_empty() {
				return Err(invalid("Only monthly bills can repeat on chosen days"));
			}
			let mut weekdays = 0;
			let mut fifth = false;
			for pattern in &repeat.patterns {
				match *pattern {
					RepeatPattern::Day(day) if day == -1 || (1..=31).contains(&day) => {}
					RepeatPattern::Weekday(_, nth) if nth == -1 || (1..=5).contains(&nth) => {
						weekdays += 1;
						fifth |= nth == 5;
					}
					_ => {
						return Err(invalid(
							"Choose a day from 1 to 31 or the last day, and a week from 1 to 5 or the last week",
						));
					}
				}
			}
			// docs/actual-format.md §6.3 finding 2: a fifth weekday spills into
			// the next month and makes Actual skip that month's other weekdays.
			if fifth && weekdays > 1 {
				return Err(invalid(
					"A fifth weekday can't be combined with other weekdays; Actual would skip some of them",
				));
			}
		}
		let date = self.date_value()?;
		match next_date_from(&date, today) {
			Ok(Some(_)) => Ok(()),
			Ok(None) => Err(invalid(
				"This bill never comes due: its end is before its first date",
			)),
			Err(error) => Err(invalid(format!("This bill never comes due: {error}"))),
		}
	}

	fn date_value(&self) -> Result<DateValue, CoreError> {
		let raw: Value = serde_json::from_str(&self.date.to_json())
			.map_err(|error| invalid(error.to_string()))?;
		parse_date_value(Op::IsApprox, Some(&raw)).map_err(|gap| invalid(format!("{gap:?}")))
	}

	fn id_json(id: Option<&str>) -> String {
		id.map_or_else(|| "null".to_owned(), string)
	}

	/// The editor's four conditions, in its order, with Actual's stored field names.
	fn conditions(&self) -> Vec<String> {
		let item = |op: &str, field: &str, value: String| {
			write_object([
				("op", string(op).as_str()),
				("field", string(field).as_str()),
				("value", value.as_str()),
			])
		};
		let mut conditions = vec![item(
			"is",
			"description",
			Self::id_json(self.payee.as_ref().map(PayeeId::as_str)),
		)];
		if let Some(account) = &self.account {
			conditions.push(item("is", "acct", string(account.as_str())));
		}
		conditions.push(item("isapprox", "date", self.date.to_json()));
		conditions.push(item(self.amount.op(), "amount", self.amount.to_json()));
		conditions
	}

	fn category_action(category: &CategoryId) -> String {
		write_object([
			("op", "\"set\""),
			("field", "\"category\""),
			("value", string(category.as_str()).as_str()),
			("type", "\"id\""),
		])
	}

	/// The rule `createSchedule` inserts for a new bill.
	pub fn new_rule(&self, schedule: &ScheduleId) -> ScheduleRule {
		let conditions_json = write_array(self.conditions());
		let mut actions = vec![write_object([
			("op", "\"link-schedule\""),
			("value", string(schedule.as_str()).as_str()),
		])];
		if let Some(category) = &self.category {
			actions.push(Self::category_action(category));
		}
		ScheduleRule {
			json_paths: json_paths(&conditions_json),
			conditions_json,
			actions_json: write_array(actions),
		}
	}

	/// Saving this bill over an existing schedule rule.
	pub fn edit_rule(&self, rule: &Rule, today: CivilDate) -> Result<EditedRule, CoreError> {
		let unreadable = || invalid("This bill's rule can't be read, so edit it in Actual");
		let old: Vec<&str> = array_items(&rule.conditions_json).ok_or_else(unreadable)?;
		let old_values: Vec<Value> = old
			.iter()
			.map(|item| serde_json::from_str(item).unwrap_or(Value::Null))
			.collect();
		let slots = schedule_slots(&old_values);
		let fresh = self.conditions();
		let fresh_by_field = |field: Field| {
			fresh.iter().find(|item| {
				let value: Value = serde_json::from_str(item).unwrap_or(Value::Null);
				field_of(&value) == Some(field)
			})
		};

		let mut conditions: Vec<String> = old.iter().map(|item| (*item).to_owned()).collect();
		let mut added = Vec::new();
		for (field, slot) in [
			(Field::Payee, slots.payee),
			(Field::Account, slots.account),
			(Field::Amount, slots.amount),
			(Field::Date, slots.date),
		] {
			let replacement = match (field, slot) {
				// The editor overwrites the amount condition outright.
				(Field::Amount, _) => fresh_by_field(Field::Amount).cloned(),
				(_, Some(index)) => {
					let value = self.field_value(field);
					with_member(old[index], "value", &value)
				}
				(_, None) => fresh_by_field(field).cloned(),
			};
			match (slot, replacement) {
				(Some(index), Some(item)) => conditions[index] = item,
				(None, Some(item)) => added.push(item),
				_ => {}
			}
		}
		conditions.extend(added);
		let conditions_json = write_array(conditions.iter().cloned());

		let new_values: Vec<Value> = conditions
			.iter()
			.map(|item| serde_json::from_str(item).unwrap_or(Value::Null))
			.collect();
		let actions_json = self.edit_actions(&rule.actions_json, &new_values);
		let changed = |field: Field| {
			let find = |values: &[Value]| {
				values
					.iter()
					.find(|value| field_of(value) == Some(field))
					.map(without_type)
			};
			find(&old_values) != find(&new_values)
		};
		let reset_next_date = if changed(Field::Account) || changed(Field::Date) {
			Some(
				next_date_from(&self.date_value()?, today)
					.map_err(|error| invalid(error.to_string()))?,
			)
		} else {
			None
		};
		Ok(EditedRule {
			json_paths: json_paths(&conditions_json),
			conditions_json,
			actions_json,
			reset_next_date,
		})
	}

	fn field_value(&self, field: Field) -> String {
		match field {
			Field::Payee => Self::id_json(self.payee.as_ref().map(PayeeId::as_str)),
			Field::Account => Self::id_json(self.account.as_ref().map(AccountId::as_str)),
			Field::Date => self.date.to_json(),
			_ => self.amount.to_json(),
		}
	}

	/// `updateActions` keeps plain `set amount` actions equal to the scheduled
	/// amount; Veritas also keeps the bill's category in a plain `set category`.
	fn edit_actions(&self, actions_json: &str, conditions: &[Value]) -> Option<String> {
		let items = array_items(actions_json)?;
		let amount = schedule_slots(conditions)
			.amount
			.and_then(|index| scheduled_amount(conditions[index].get("value")));
		let mut changed = false;
		let mut has_category = false;
		let mut updated = Vec::new();
		for item in items {
			let value: Value = serde_json::from_str(item).unwrap_or(Value::Null);
			let plain = value["op"] == "set"
				&& !js::truthy(value.pointer("/options/template"))
				&& !js::truthy(value.pointer("/options/formula"));
			let split = js::truthy(value.pointer("/options/splitIndex"));
			if plain
				&& value["field"] == "amount"
				&& let Some(amount) = amount
				&& value["value"] != amount
			{
				updated.push(with_member(item, "value", &amount.to_string())?);
				changed = true;
			} else if plain && !split && value["field"] == "category" && !has_category {
				has_category = true;
				match &self.category {
					Some(category) if value["value"] != category.as_str() => {
						updated.push(with_member(item, "value", &string(category.as_str()))?);
						changed = true;
					}
					Some(_) => updated.push(item.to_owned()),
					None => changed = true,
				}
			} else {
				updated.push(item.to_owned());
			}
		}
		if let (false, Some(category)) = (has_category, &self.category) {
			updated.push(Self::category_action(category));
			changed = true;
		}
		changed.then(|| write_array(updated))
	}
}

/// `getScheduledAmount`: the number, or the rounded middle of a range.
/// `None` for an amount that isn't whole cents, which Actual would post as
/// it is and Veritas can't hold.
pub fn scheduled_amount(value: Option<&Value>) -> Option<i64> {
	match value {
		None | Some(Value::Null) => Some(0),
		Some(Value::Number(number)) => js::integer(&Value::Number(number.clone())),
		Some(Value::Object(range)) => {
			let num1 = range.get("num1").and_then(Value::as_f64)?;
			let num2 = range.get("num2").and_then(Value::as_f64)?;
			let middle = js::math_round((num1 + num2) / 2.0);
			middle.is_finite().then_some(middle as i64)
		}
		_ => None,
	}
}

fn field_of(condition: &Value) -> Option<Field> {
	condition
		.get("field")
		.and_then(Value::as_str)
		.and_then(Field::from_stored)
}

fn without_type(condition: &Value) -> Value {
	let mut condition = condition.clone();
	if let Some(object) = condition.as_object_mut() {
		object.remove("type");
	}
	condition
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Slots {
	pub payee: Option<usize>,
	pub account: Option<usize>,
	pub amount: Option<usize>,
	pub date: Option<usize>,
}

/// `extractScheduleConds` (`lc/shared/schedules.ts:255-281`) as indexes.
pub(crate) fn schedule_slots(conditions: &[Value]) -> Slots {
	let find = |field: Field, ops: &[&str]| {
		conditions.iter().position(|condition| {
			field_of(condition) == Some(field)
				&& condition["op"].as_str().is_some_and(|op| ops.contains(&op))
		})
	};
	Slots {
		payee: find(Field::Payee, &["is"]),
		account: find(Field::Account, &["is"]),
		amount: find(Field::Amount, &["is", "isapprox", "isbetween"]),
		date: find(Field::Date, &["is", "isapprox"]),
	}
}

/// `onRuleUpdate`'s `schedules_json_paths` row for a rule's conditions.
pub fn json_paths(conditions_json: &str) -> JsonPaths {
	let values: Vec<Value> = serde_json::from_str(conditions_json).unwrap_or_default();
	let slots = schedule_slots(&values);
	let path = |slot: Option<usize>| slot.map(|index| format!("$[{index}]"));
	JsonPaths {
		payee: path(slots.payee),
		account: path(slots.account),
		amount: path(slots.amount),
		date: path(slots.date),
	}
}

/// Reads a schedule as a bill. `None` when its date or amount is a shape the
/// bill screen can't show; such a schedule is listed read-only.
pub fn bill_from_schedule(
	schedule: &Schedule,
	rule: &Rule,
	budget: &BudgetData,
) -> Option<BillView> {
	let conditions: Vec<Value> = serde_json::from_str(&rule.conditions_json).ok()?;
	let actions: Vec<Value> = serde_json::from_str(&rule.actions_json).ok()?;
	let slots = schedule_slots(&conditions);
	let value = |slot: Option<usize>| slot.map(|index| &conditions[index]["value"]);
	// `v_schedules._payee` is `payee_mapping.targetId`: no row, no payee.
	let payee = value(slots.payee)
		.and_then(Value::as_str)
		.and_then(|id| budget.payee_mapping.get(&PayeeId::new(id)).cloned());
	let account = value(slots.account)
		.and_then(Value::as_str)
		.map(AccountId::new);
	let amount_value = value(slots.amount)?;
	let amount = match conditions[slots.amount?]["op"].as_str()? {
		"isbetween" => BillAmount::Between(
			Money::from_minor(amount_value.get("num1").and_then(js::integer)?),
			Money::from_minor(amount_value.get("num2").and_then(js::integer)?),
		),
		"isapprox" => BillAmount::About(Money::from_minor(js::integer(amount_value)?)),
		_ => BillAmount::Exact(Money::from_minor(js::integer(amount_value)?)),
	};
	let date = match value(slots.date)? {
		Value::String(text) => BillDate::Once(js::strict_date(text)?.into()),
		config => BillDate::Repeat(Repeat::from_json(config)?),
	};
	let mut category = None;
	let mut custom = conditions.len()
		!= [slots.payee, slots.account, slots.amount, slots.date]
			.iter()
			.flatten()
			.count();
	for action in &actions {
		let plain_category = action["op"] == "set"
			&& action["field"] == "category"
			&& action.get("options").is_none_or(|o| !js::truthy(Some(o)));
		if action["op"] == "link-schedule" {
			continue;
		} else if plain_category && category.is_none() {
			category = action["value"].as_str().map(CategoryId::new);
		} else {
			custom = true;
		}
	}
	Some(BillView {
		id: schedule.id.clone(),
		bill: Bill {
			name: schedule.name.clone(),
			payee,
			account,
			amount,
			date,
			category,
			posts_transaction: schedule.posts_transaction,
		},
		completed: schedule.completed,
		next_date: schedule.next_date,
		custom_rule: custom,
	})
}

/// The name Actual stores: trimmed, and empty becomes no name
/// (`normalizeScheduleName`, `app.ts:299-302`). Live names must be unique.
pub fn schedule_name(
	name: Option<&str>,
	schedule: Option<&ScheduleId>,
	budget: &BudgetData,
) -> Result<Option<String>, CoreError> {
	let Some(name) = name.map(str::trim).filter(|name| !name.is_empty()) else {
		return Ok(None);
	};
	let taken = budget.schedules.values().any(|other| {
		!other.tombstone && other.name.as_deref() == Some(name) && Some(&other.id) != schedule
	});
	if taken {
		return Err(invalid(format!("Another bill is already called {name}")));
	}
	Ok(Some(name.to_owned()))
}

#[cfg(test)]
mod tests {
	use veritas_domain::{BudgetType, ConditionsOp, RuleId, RuleStage};

	use super::*;

	fn day(text: &str) -> CivilDate {
		CivilDate::from(js::strict_date(text).unwrap())
	}

	fn rent() -> Bill {
		Bill {
			name: Some("Rent".into()),
			payee: Some(PayeeId::new("landlord")),
			account: Some(AccountId::new("checking")),
			amount: BillAmount::About(Money::from_minor(-150_000)),
			date: BillDate::Repeat(Repeat {
				start: day("2026-08-15"),
				interval: 1,
				frequency: Frequency::Monthly,
				patterns: Vec::new(),
				weekend: None,
				end: RepeatEnd::Never,
			}),
			category: Some(CategoryId::new("housing")),
			posts_transaction: true,
		}
	}

	fn stored(bill: &Bill, id: &str) -> (Schedule, Rule) {
		let text = bill.new_rule(&ScheduleId::new(id));
		let rule = Rule {
			id: RuleId::new(format!("rule-{id}")),
			stage: RuleStage::Default,
			conditions_op: ConditionsOp::And,
			conditions_json: text.conditions_json,
			actions_json: text.actions_json,
			tombstone: false,
		};
		let schedule = Schedule {
			id: ScheduleId::new(id),
			name: bill.name.clone(),
			rule: rule.id.clone(),
			posts_transaction: bill.posts_transaction,
			completed: false,
			next_date: None,
			tombstone: false,
		};
		(schedule, rule)
	}

	#[test]
	fn a_new_rule_is_written_like_the_schedule_editor() {
		let text = rent().new_rule(&ScheduleId::new("s1"));
		assert_eq!(
			text.conditions_json,
			r#"[{"op":"is","field":"description","value":"landlord"},{"op":"is","field":"acct","value":"checking"},{"op":"isapprox","field":"date","value":{"start":"2026-08-15","interval":1,"frequency":"monthly","patterns":[],"skipWeekend":false,"weekendSolveMode":"before","endMode":"never","endOccurrences":1,"endDate":"2026-08-15"}},{"op":"isapprox","field":"amount","value":-150000}]"#
		);
		assert_eq!(
			text.actions_json,
			r#"[{"op":"link-schedule","value":"s1"},{"op":"set","field":"category","value":"housing","type":"id"}]"#
		);
		assert_eq!(
			text.json_paths,
			JsonPaths {
				payee: Some("$[0]".into()),
				account: Some("$[1]".into()),
				amount: Some("$[3]".into()),
				date: Some("$[2]".into())
			}
		);
		let mut no_account = rent();
		no_account.account = None;
		no_account.payee = None;
		let text = no_account.new_rule(&ScheduleId::new("s2"));
		assert!(text.conditions_json.starts_with(
			r#"[{"op":"is","field":"description","value":null},{"op":"isapprox","field":"date""#
		));
		assert_eq!(text.json_paths.account, None);
	}

	#[test]
	fn bills_round_trip_through_the_schedule_rule() {
		let budget = BudgetData::empty(BudgetType::Envelope);
		let mut bills = vec![rent()];
		let mut paycheck = rent();
		paycheck.amount = BillAmount::Exact(Money::from_minor(310_000));
		paycheck.category = None;
		paycheck.date = BillDate::Repeat(Repeat {
			start: day("2026-01-01"),
			interval: 2,
			frequency: Frequency::Monthly,
			patterns: vec![
				RepeatPattern::Day(1),
				RepeatPattern::Day(-1),
				RepeatPattern::Weekday(Weekday::Fri, 2),
			],
			weekend: Some(WeekendMove::Before),
			end: RepeatEnd::AfterOccurrences(24),
		});
		bills.push(paycheck);
		let mut gym = rent();
		gym.amount = BillAmount::Between(Money::from_minor(-5000), Money::from_minor(-4000));
		gym.date = BillDate::Once(day("2026-12-05"));
		bills.push(gym);
		for bill in bills {
			let (schedule, rule) = stored(&bill, "s");
			let view = bill_from_schedule(&schedule, &rule, &budget).unwrap();
			assert_eq!(
				view.bill.payee, None,
				"an unknown payee id has no _payee, as in v_schedules"
			);
			assert_eq!(
				Bill {
					payee: bill.payee.clone(),
					..view.bill
				},
				bill
			);
			assert!(!view.custom_rule);
		}
	}

	#[test]
	fn an_edit_rewrites_only_the_schedule_conditions() {
		// A rule customised in Actual: a type key, an extra condition and a
		// template action, all of which must survive.
		let rule = Rule {
			id: RuleId::new("r"),
			stage: RuleStage::Default,
			conditions_op: ConditionsOp::And,
			conditions_json: r#"[{"op":"is","field":"description","value":"landlord","type":"id"},{"op":"contains","field":"notes","value":"rent"},{"op":"isapprox","field":"date","value":{"start":"2026-08-15","frequency":"monthly"}},{"op":"is","field":"amount","value":-100}]"#.into(),
			actions_json: r#"[{"op":"link-schedule","value":"s"},{"op":"set","field":"amount","value":-100},{"op":"set","field":"notes","value":"","options":{"template":"{{payee}}"}}]"#.into(),
			tombstone: false,
		};
		let mut bill = rent();
		bill.category = None;
		bill.payee = Some(PayeeId::new("new-landlord"));
		let edited = bill.edit_rule(&rule, day("2026-11-09")).unwrap();
		let items = array_items(&edited.conditions_json).unwrap();
		assert_eq!(
			items[0],
			r#"{"op":"is","field":"description","value":"new-landlord","type":"id"}"#
		);
		assert_eq!(
			items[1],
			r#"{"op":"contains","field":"notes","value":"rent"}"#
		);
		assert_eq!(
			items[3],
			r#"{"op":"isapprox","field":"amount","value":-150000}"#
		);
		assert_eq!(
			items[4], r#"{"op":"is","field":"acct","value":"checking"}"#,
			"a missing condition is appended"
		);
		assert_eq!(
			edited.actions_json.as_deref(),
			Some(
				r#"[{"op":"link-schedule","value":"s"},{"op":"set","field":"amount","value":-150000},{"op":"set","field":"notes","value":"","options":{"template":"{{payee}}"}}]"#
			)
		);
		assert_eq!(edited.json_paths.account.as_deref(), Some("$[4]"));
		// The account and date changed, so Actual resets the next date from today.
		assert_eq!(edited.reset_next_date, Some(Some(day("2026-11-15"))));
		let budget = BudgetData::empty(BudgetType::Envelope);
		let (schedule, _) = stored(&rent(), "s");
		let view = bill_from_schedule(&schedule, &rule, &budget).unwrap();
		assert!(view.custom_rule);
	}

	#[test]
	fn an_edit_that_keeps_the_date_and_account_keeps_the_next_date() {
		let (_, rule) = stored(&rent(), "s");
		let mut cheaper = rent();
		cheaper.amount = BillAmount::About(Money::from_minor(-140_000));
		let edited = cheaper.edit_rule(&rule, day("2026-11-09")).unwrap();
		assert_eq!(edited.reset_next_date, None);
		assert_eq!(edited.actions_json, None);
	}

	#[test]
	fn validation_refuses_what_actual_would_misread() {
		let today = day("2026-11-09");
		assert!(rent().validate(today).is_ok());
		let with = |change: &dyn Fn(&mut Repeat)| {
			let mut bill = rent();
			if let BillDate::Repeat(repeat) = &mut bill.date {
				change(repeat);
			}
			bill.validate(today)
		};
		assert!(with(&|r| r.interval = 0).is_err());
		assert!(with(&|r| r.end = RepeatEnd::AfterOccurrences(0)).is_err());
		assert!(with(&|r| r.patterns = vec![RepeatPattern::Day(32)]).is_err());
		assert!(with(&|r| r.patterns = vec![RepeatPattern::Weekday(Weekday::Mon, 6)]).is_err());
		assert!(
			with(&|r| r.patterns = vec![
				RepeatPattern::Weekday(Weekday::Fri, 5),
				RepeatPattern::Weekday(Weekday::Tue, 1)
			])
			.is_err()
		);
		assert!(
			with(&|r| r.patterns = vec![
				RepeatPattern::Weekday(Weekday::Fri, 5),
				RepeatPattern::Day(3)
			])
			.is_ok()
		);
		assert!(with(&|r| r.end = RepeatEnd::OnDate(day("2026-01-01"))).is_err());
		assert!(
			with(&|r| {
				r.frequency = Frequency::Weekly;
				r.patterns = vec![RepeatPattern::Day(3)];
			})
			.is_err()
		);
		assert!(
			with(&|r| {
				r.interval = 12;
				r.start = day("2026-04-30");
				r.patterns = vec![RepeatPattern::Day(31)];
			})
			.is_err()
		);
	}

	#[test]
	fn names_are_trimmed_and_unique() {
		let mut budget = BudgetData::empty(BudgetType::Envelope);
		let (schedule, _) = stored(&rent(), "s");
		budget.schedules.insert(schedule.id.clone(), schedule);
		assert_eq!(schedule_name(Some("  "), None, &budget).unwrap(), None);
		assert!(schedule_name(Some(" Rent "), None, &budget).is_err());
		assert_eq!(
			schedule_name(Some(" Rent "), Some(&ScheduleId::new("s")), &budget)
				.unwrap()
				.as_deref(),
			Some("Rent")
		);
	}

	#[test]
	fn scheduled_amount_rounds_ranges_like_math_round() {
		assert_eq!(
			scheduled_amount(Some(&serde_json::json!({"num1": -1000, "num2": -2001}))),
			Some(-1500)
		);
		assert_eq!(
			scheduled_amount(Some(&serde_json::json!(-6999))),
			Some(-6999)
		);
		assert_eq!(scheduled_amount(None), Some(0));
		assert_eq!(
			scheduled_amount(Some(&serde_json::json!(-12.5))),
			None,
			"never truncated"
		);
	}
}
