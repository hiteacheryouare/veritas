//! The sentence-style rule editor (R-RULE-2, R-RULE-3): the rules list, the
//! models drafts are saved from, and "apply to existing" with its preview.

use std::sync::Arc;

use veritas_domain::{BudgetData, ConditionsOp, CoreError, Row, RuleId, TransactionId};
use veritas_reports::{self as reports, Json};
use veritas_rules::{AmountSign, Field, Op};
use veritas_session::rules::{self as session_rules, ChangedField, Source};

use crate::FfiError;
use crate::blocking::{CancelToken as CancelHandle, run};
use crate::session::Session;

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum RuleStage {
	Pre,
	Default,
	Post,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum RuleConditionsOp {
	And,
	Or,
}

/// A transaction field. `AmountInflow` and `AmountOutflow` are conditions
/// only: Actual stores them as `amount` with an inflow or outflow option.
/// `Saved` is a report or dashboard filter only: a saved filter, by id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum RuleField {
	ImportedPayee,
	Payee,
	PayeeName,
	Account,
	Category,
	CategoryGroup,
	Notes,
	Amount,
	AmountInflow,
	AmountOutflow,
	Date,
	Cleared,
	Reconciled,
	Transfer,
	Parent,
	Saved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum RuleConditionOp {
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
	/// Actual's internal "all of"; it never matches when rules run. Read only.
	And,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum RuleActionOp {
	Set,
	PrependNotes,
	AppendNotes,
	/// A bill's rule. Read only.
	LinkSchedule,
	DeleteTransaction,
	/// A split line's amount: `.amount` fixed, `.percent` of what the fixed
	/// amounts leave, or `.nothing` for an equal share of the remainder.
	SetSplitAmount,
}

#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum RuleValue {
	/// A payee, category, category group, account or schedule id; `None` is empty.
	Id {
		id: Option<String>,
	},
	Ids {
		ids: Vec<String>,
	},
	Text {
		text: String,
	},
	Texts {
		texts: Vec<String>,
	},
	/// Minor units.
	Amount {
		amount: i64,
	},
	AmountRange {
		num1: i64,
		num2: i64,
	},
	/// `YYYY-MM-DD`.
	Date {
		date: String,
	},
	/// `YYYY-MM`.
	Month {
		month: String,
	},
	Year {
		year: i32,
	},
	Bool {
		value: bool,
	},
	/// A split line's percentage of what the fixed amounts leave.
	Percent {
		percent: f64,
	},
	Nothing,
	/// Stored JSON the sentence can't show. Read only.
	Raw {
		json: String,
	},
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RuleCondition {
	pub field: RuleField,
	pub op: RuleConditionOp,
	pub value: RuleValue,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RuleAction {
	pub op: RuleActionOp,
	/// Only `Set` has one.
	pub field: Option<RuleField>,
	pub value: RuleValue,
	/// 0 is the transaction itself; 1 and up are split lines.
	pub split_index: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum RuleAccess {
	Editable,
	/// The rule of a bill; edit the bill.
	Bill {
		schedule_id: String,
	},
	/// Shown read only and kept byte for byte: "Edit in Actual". `reasons`
	/// are noun phrases, such as "a template action".
	EditInActual {
		reasons: Vec<String>,
	},
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RuleSentence {
	pub id: String,
	pub stage: RuleStage,
	pub conditions_op: RuleConditionsOp,
	pub conditions: Vec<RuleCondition>,
	pub actions: Vec<RuleAction>,
	pub access: RuleAccess,
	/// Veritas evaluates and applies all of it. When false, transactions it
	/// would touch get "may need Actual".
	pub runs_in_veritas: bool,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RuleDraft {
	pub stage: RuleStage,
	pub conditions_op: RuleConditionsOp,
	pub conditions: Vec<RuleCondition>,
	pub actions: Vec<RuleAction>,
}

/// A saved rule, or the editor's unsaved draft.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum RuleSource {
	Saved { rule_id: String },
	Draft { draft: RuleDraft },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum RuleChangedField {
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

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RuleFieldChange {
	pub field: RuleChangedField,
	pub before: RuleValue,
	/// A payee the apply would create is `Text` with its name.
	pub after: RuleValue,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RuleMatch {
	pub transaction_id: String,
	/// Set for a split line.
	pub parent_id: Option<String>,
	pub date: String,
	pub account_id: String,
	pub amount: i64,
	pub payee_id: Option<String>,
	pub category_id: Option<String>,
	pub notes: Option<String>,
	pub changes: Vec<RuleFieldChange>,
	/// The lines a split rule would give it, in order.
	pub split_lines: Vec<RuleSplitLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RuleSplitLine {
	pub amount: i64,
	pub payee_id: Option<String>,
	pub category_id: Option<String>,
	pub notes: Option<String>,
}

fn stage(stage: veritas_domain::RuleStage) -> RuleStage {
	match stage {
		veritas_domain::RuleStage::Pre => RuleStage::Pre,
		veritas_domain::RuleStage::Default => RuleStage::Default,
		veritas_domain::RuleStage::Post => RuleStage::Post,
	}
}

fn stored_stage(stage: RuleStage) -> veritas_domain::RuleStage {
	match stage {
		RuleStage::Pre => veritas_domain::RuleStage::Pre,
		RuleStage::Default => veritas_domain::RuleStage::Default,
		RuleStage::Post => veritas_domain::RuleStage::Post,
	}
}

fn field(field: Field, sign: AmountSign) -> RuleField {
	match (field, sign) {
		(Field::Amount, AmountSign::Inflow) => RuleField::AmountInflow,
		(Field::Amount, AmountSign::Outflow) => RuleField::AmountOutflow,
		(Field::ImportedPayee, _) => RuleField::ImportedPayee,
		(Field::Payee, _) => RuleField::Payee,
		(Field::PayeeName, _) => RuleField::PayeeName,
		(Field::Account, _) => RuleField::Account,
		(Field::Category, _) => RuleField::Category,
		(Field::CategoryGroup, _) => RuleField::CategoryGroup,
		(Field::Notes, _) => RuleField::Notes,
		(Field::Amount, _) => RuleField::Amount,
		(Field::Date, _) => RuleField::Date,
		(Field::Cleared, _) => RuleField::Cleared,
		(Field::Reconciled, _) => RuleField::Reconciled,
		(Field::Transfer, _) => RuleField::Transfer,
		(Field::Parent, _) => RuleField::Parent,
	}
}

fn core_field(field: RuleField) -> Result<(Field, AmountSign), CoreError> {
	Ok(match field {
		RuleField::AmountInflow => (Field::Amount, AmountSign::Inflow),
		RuleField::AmountOutflow => (Field::Amount, AmountSign::Outflow),
		RuleField::ImportedPayee => (Field::ImportedPayee, AmountSign::Any),
		RuleField::Payee => (Field::Payee, AmountSign::Any),
		RuleField::PayeeName => (Field::PayeeName, AmountSign::Any),
		RuleField::Account => (Field::Account, AmountSign::Any),
		RuleField::Category => (Field::Category, AmountSign::Any),
		RuleField::CategoryGroup => (Field::CategoryGroup, AmountSign::Any),
		RuleField::Notes => (Field::Notes, AmountSign::Any),
		RuleField::Amount => (Field::Amount, AmountSign::Any),
		RuleField::Date => (Field::Date, AmountSign::Any),
		RuleField::Cleared => (Field::Cleared, AmountSign::Any),
		RuleField::Reconciled => (Field::Reconciled, AmountSign::Any),
		RuleField::Transfer => (Field::Transfer, AmountSign::Any),
		RuleField::Parent => (Field::Parent, AmountSign::Any),
		RuleField::Saved => {
			return Err(CoreError::Invalid(
				"A rule can't use a saved filter.".to_owned(),
			));
		}
	})
}

macro_rules! same_variants {
	($from:ty => $to:ty, $name:ident, [$($variant:ident),*]) => {
		fn $name(value: $from) -> $to {
			match value {
				$(<$from>::$variant => <$to>::$variant,)*
			}
		}
	};
}

same_variants!(Op => RuleConditionOp, condition_op, [
	Is, IsNot, Contains, DoesNotContain, Matches, OneOf, NotOneOf, HasTags, HasAnyTag, IsApprox,
	IsBetween, Gt, Gte, Lt, Lte, OnBudget, OffBudget, And
]);
same_variants!(RuleConditionOp => Op, core_condition_op, [
	Is, IsNot, Contains, DoesNotContain, Matches, OneOf, NotOneOf, HasTags, HasAnyTag, IsApprox,
	IsBetween, Gt, Gte, Lt, Lte, OnBudget, OffBudget, And
]);
same_variants!(veritas_rules::ActionOp => RuleActionOp, action_op, [
	Set, PrependNotes, AppendNotes, LinkSchedule, DeleteTransaction, SetSplitAmount
]);
same_variants!(RuleActionOp => veritas_rules::ActionOp, core_action_op, [
	Set, PrependNotes, AppendNotes, LinkSchedule, DeleteTransaction, SetSplitAmount
]);
same_variants!(ChangedField => RuleChangedField, changed_field, [
	Account, Date, Amount, Payee, Category, Notes, ImportedPayee, Cleared, Reconciled, Schedule,
	Deleted
]);

fn value(value: veritas_rules::Value) -> RuleValue {
	use veritas_rules::Value as V;
	match value {
		V::Id(id) => RuleValue::Id { id },
		V::Ids(ids) => RuleValue::Ids { ids },
		V::Text(text) => RuleValue::Text { text },
		V::Texts(texts) => RuleValue::Texts { texts },
		V::Amount(amount) => RuleValue::Amount { amount },
		V::AmountRange(num1, num2) => RuleValue::AmountRange { num1, num2 },
		V::Date(date) => RuleValue::Date { date },
		V::Month(month) => RuleValue::Month { month },
		V::Year(year) => RuleValue::Year { year },
		V::Bool(value) => RuleValue::Bool { value },
		V::Percent(percent) => RuleValue::Percent { percent },
		V::Nothing => RuleValue::Nothing,
		V::Raw(json) => RuleValue::Raw { json },
	}
}

fn core_value(value: &RuleValue) -> Result<veritas_rules::Value, CoreError> {
	use veritas_rules::Value as V;
	Ok(match value.clone() {
		RuleValue::Id { id } => V::Id(id),
		RuleValue::Ids { ids } => V::Ids(ids),
		RuleValue::Text { text } => V::Text(text),
		RuleValue::Texts { texts } => V::Texts(texts),
		RuleValue::Amount { amount } => V::Amount(amount),
		RuleValue::AmountRange { num1, num2 } => V::AmountRange(num1, num2),
		RuleValue::Date { date } => V::Date(date),
		RuleValue::Month { month } => V::Month(month),
		RuleValue::Year { year } => V::Year(year),
		RuleValue::Bool { value } => V::Bool(value),
		RuleValue::Percent { percent } => V::Percent(percent),
		RuleValue::Nothing => V::Nothing,
		RuleValue::Raw { .. } => {
			return Err(CoreError::Invalid(
				"That part of the rule can only be changed in Actual.".to_owned(),
			));
		}
	})
}

fn sentence(sentence: veritas_rules::Sentence) -> RuleSentence {
	RuleSentence {
		id: sentence.id.to_string(),
		stage: stage(sentence.stage),
		conditions_op: match sentence.conditions_op {
			ConditionsOp::And => RuleConditionsOp::And,
			ConditionsOp::Or => RuleConditionsOp::Or,
		},
		conditions: sentence
			.conditions
			.into_iter()
			.map(|condition| RuleCondition {
				field: field(condition.field, condition.sign),
				op: condition_op(condition.op),
				value: value(condition.value),
			})
			.collect(),
		actions: sentence
			.actions
			.into_iter()
			.map(|action| RuleAction {
				op: action_op(action.op),
				field: action.field.map(|f| field(f, AmountSign::Any)),
				value: value(action.value),
				split_index: action.split,
			})
			.collect(),
		access: match sentence.access {
			veritas_rules::Access::Editable => RuleAccess::Editable,
			veritas_rules::Access::Bill(schedule) => RuleAccess::Bill {
				schedule_id: schedule.to_string(),
			},
			veritas_rules::Access::EditInActual(reasons) => RuleAccess::EditInActual { reasons },
		},
		runs_in_veritas: sentence.runs_in_veritas,
	}
}

pub(crate) fn draft(draft: &RuleDraft) -> Result<veritas_rules::Draft, CoreError> {
	let conditions = draft
		.conditions
		.iter()
		.map(|condition| {
			let (field, sign) = core_field(condition.field)?;
			Ok(veritas_rules::SentenceCondition {
				field,
				op: core_condition_op(condition.op),
				sign,
				value: core_value(&condition.value)?,
			})
		})
		.collect::<Result<Vec<_>, CoreError>>()?;
	let actions = draft
		.actions
		.iter()
		.map(|action| {
			let field = match action.field.map(core_field).transpose()? {
				Some((_, AmountSign::Inflow | AmountSign::Outflow)) => {
					return Err(CoreError::Invalid(
						"An action sets the amount, not an inflow or outflow.".to_owned(),
					));
				}
				other => other.map(|(field, _)| field),
			};
			Ok(veritas_rules::SentenceAction {
				op: core_action_op(action.op),
				field,
				value: core_value(&action.value)?,
				split: action.split_index,
			})
		})
		.collect::<Result<Vec<_>, CoreError>>()?;
	Ok(veritas_rules::Draft {
		stage: stored_stage(draft.stage),
		conditions_op: match draft.conditions_op {
			RuleConditionsOp::And => ConditionsOp::And,
			RuleConditionsOp::Or => ConditionsOp::Or,
		},
		conditions,
		actions,
	})
}

/// Actual's name for a filter's field (`FIELD_INFO`, shared/rules.ts:62-88).
fn filter_field(field: RuleField) -> &'static str {
	match field {
		RuleField::ImportedPayee => "imported_payee",
		RuleField::Payee => "payee",
		RuleField::PayeeName => "payee_name",
		RuleField::Account => "account",
		RuleField::Category => "category",
		RuleField::CategoryGroup => "category_group",
		RuleField::Notes => "notes",
		RuleField::Amount | RuleField::AmountInflow | RuleField::AmountOutflow => "amount",
		RuleField::Date => "date",
		RuleField::Cleared => "cleared",
		RuleField::Reconciled => "reconciled",
		RuleField::Transfer => "transfer",
		RuleField::Parent => "parent",
		RuleField::Saved => "saved",
	}
}

fn serde(stored: &Json) -> Option<serde_json::Value> {
	serde_json::from_str(&stored.stringify()).ok()
}

/// A stored report or dashboard filter, typed. `None` when Actual defines
/// no such field or op.
pub(crate) fn filter_condition(budget: &BudgetData, stored: &Json) -> Option<RuleCondition> {
	if stored.get("field").and_then(Json::as_str) == Some("saved") {
		let op = Op::from_stored(stored.get("op")?.as_str()?)?;
		return Some(RuleCondition {
			field: RuleField::Saved,
			op: condition_op(op),
			value: match stored.get("value") {
				Some(Json::String(id)) => RuleValue::Id {
					id: Some(id.clone()),
				},
				other => RuleValue::Raw {
					json: other.unwrap_or(&Json::Null).stringify(),
				},
			},
		});
	}
	let condition = veritas_rules::filter_condition(budget, &serde(stored)?)?;
	Some(RuleCondition {
		field: field(condition.field, condition.sign),
		op: condition_op(condition.op),
		value: value(condition.value),
	})
}

pub(crate) fn filter_conditions(budget: &BudgetData, stored: &[Json]) -> Vec<RuleCondition> {
	stored
		.iter()
		.filter_map(|condition| filter_condition(budget, condition))
		.collect()
}

/// A new filter as Actual's filter editor saves it: inflow and outflow as
/// `amount` with an option, and a month or year date with `month` or `year`
/// (`subfieldToOptions.ts`).
fn new_filter(condition: &RuleCondition) -> Result<Json, CoreError> {
	let value = match &condition.value {
		RuleValue::Raw { .. } => {
			return Err(CoreError::Invalid(
				"That filter can only be changed in Actual.".to_owned(),
			));
		}
		RuleValue::Id { id } if condition.field == RuleField::Saved => id.as_deref().map_or_else(
			|| "null".to_owned(),
			|id| Json::String(id.to_owned()).stringify(),
		),
		other => veritas_rules::stored_value(&core_value(other)?),
	};
	let option = match (&condition.field, &condition.value) {
		(RuleField::AmountInflow, _) => Some("inflow"),
		(RuleField::AmountOutflow, _) => Some("outflow"),
		(RuleField::Date, RuleValue::Month { .. }) => Some("month"),
		(RuleField::Date, RuleValue::Year { .. }) => Some("year"),
		_ => None,
	};
	let parse = |text: &str| {
		Json::parse(text).map_err(|reason| CoreError::Invalid(format!("a filter: {reason}")))
	};
	reports::condition(
		filter_field(condition.field),
		core_condition_op(condition.op).stored(),
		parse(&value)?,
		option
			.map(|option| parse(&format!(r#"{{"{option}":true}}"#)))
			.transpose()?,
	)
}

/// Pairs each condition with a stored filter it equals, in order and each
/// at most once: the longest such pairing, as a diff would find it.
fn kept_pairs(
	conditions: &[RuleCondition],
	typed: &[(usize, RuleCondition)],
) -> Vec<(usize, usize)> {
	let (rows, columns) = (typed.len(), conditions.len());
	let mut longest = vec![vec![0_usize; columns + 1]; rows + 1];
	for row in (0..rows).rev() {
		for column in (0..columns).rev() {
			longest[row][column] = if typed[row].1 == conditions[column] {
				longest[row + 1][column + 1] + 1
			} else {
				longest[row + 1][column].max(longest[row][column + 1])
			};
		}
	}
	let mut pairs = Vec::new();
	let (mut row, mut column) = (0, 0);
	while row < rows && column < columns {
		if typed[row].1 == conditions[column] {
			pairs.push((typed[row].0, column));
			row += 1;
			column += 1;
		} else if longest[row + 1][column] >= longest[row][column + 1] {
			row += 1;
		} else {
			column += 1;
		}
	}
	pairs
}

/// The filters to store, as Actual's filter editor keeps them. A condition
/// paired with an equal stored filter keeps that filter's exact JSON, even
/// when two stored filters type the same (one with a `customName`, say).
/// Between kept filters, a changed condition takes the place of the stored
/// filter it replaces, and a stored filter Veritas can't type stays after
/// the filter it followed.
pub(crate) fn stored_filters(
	budget: &BudgetData,
	conditions: &[RuleCondition],
	stored: &[Json],
) -> Result<Vec<Json>, CoreError> {
	let typed: Vec<(usize, RuleCondition)> = stored
		.iter()
		.enumerate()
		.filter_map(|(index, old)| filter_condition(budget, old).map(|typed| (index, typed)))
		.collect();
	let untyped = |index: usize| typed.iter().all(|(at, _)| *at != index);
	let mut written = Vec::with_capacity(conditions.len() + stored.len());
	let (mut next_stored, mut next_condition) = (0, 0);
	let pairs = kept_pairs(conditions, &typed);
	let ends = pairs
		.iter()
		.map(|&(at, column)| (at, column, true))
		.chain(std::iter::once((stored.len(), conditions.len(), false)));
	for (kept_at, kept_column, kept) in ends {
		for (index, old) in stored.iter().enumerate().take(kept_at).skip(next_stored) {
			if untyped(index) {
				written.push(old.clone());
			} else if next_condition < kept_column {
				written.push(new_filter(&conditions[next_condition])?);
				next_condition += 1;
			}
		}
		for condition in &conditions[next_condition..kept_column] {
			written.push(new_filter(condition)?);
		}
		if kept {
			written.push(stored[kept_at].clone());
		}
		next_stored = kept_at + 1;
		next_condition = kept_column + 1;
	}
	Ok(written)
}

/// Runs `with` on the source as the session reads it.
fn with_source<T>(
	source: &RuleSource,
	with: impl FnOnce(Source) -> Result<T, CoreError>,
) -> Result<T, CoreError> {
	match source {
		RuleSource::Saved { rule_id } => with(Source::Saved(&RuleId::new(rule_id.as_str()))),
		RuleSource::Draft { draft: model } => with(Source::Draft(&draft(model)?)),
	}
}

fn preview(budget: &BudgetData, source: &RuleSource) -> Result<Vec<RuleMatch>, CoreError> {
	let matches = with_source(source, |source| session_rules::preview(budget, source))?;
	Ok(matches
		.into_iter()
		.map(|found| {
			let row = found.transaction;
			RuleMatch {
				transaction_id: row.id.to_string(),
				parent_id: match row.split {
					veritas_domain::SplitRole::Child { parent } => Some(parent.to_string()),
					_ => None,
				},
				date: row.date.to_string(),
				account_id: row.account.to_string(),
				amount: row.amount.minor(),
				payee_id: row.payee.map(|p| p.to_string()),
				category_id: row.category.map(|c| c.to_string()),
				notes: row.notes,
				split_lines: found
					.lines
					.into_iter()
					.map(|line| RuleSplitLine {
						amount: line.amount.minor(),
						payee_id: line.payee.map(|p| p.to_string()),
						category_id: line.category.map(|c| c.to_string()),
						notes: line.notes,
					})
					.collect(),
				changes: found
					.changes
					.into_iter()
					.map(|change| RuleFieldChange {
						field: changed_field(change.field),
						before: value(change.before),
						after: value(change.after),
					})
					.collect(),
			}
		})
		.collect())
}

/// `Command::CreateRule`, `EditRule`, `DeleteRule` and `ApplyRule`.
pub(crate) fn create(data: &BudgetData, model: &RuleDraft) -> Result<Vec<Row>, CoreError> {
	session_rules::create(data, &draft(model)?).map(|(rows, _)| rows)
}

pub(crate) fn edit(data: &BudgetData, id: &str, model: &RuleDraft) -> Result<Vec<Row>, CoreError> {
	session_rules::edit(data, &RuleId::new(id), &draft(model)?)
}

pub(crate) fn delete(data: &BudgetData, id: &str) -> Result<Vec<Row>, CoreError> {
	session_rules::delete(data, &RuleId::new(id))
}

pub(crate) fn apply(
	data: &BudgetData,
	source: &RuleSource,
	ids: &[String],
	now_ms: f64,
) -> Result<Vec<Row>, CoreError> {
	let ids: Vec<TransactionId> = ids
		.iter()
		.map(|id| TransactionId::new(id.as_str()))
		.collect();
	with_source(source, |source| {
		session_rules::apply(data, source, &ids, now_ms)
	})
}

#[uniffi::export]
impl Session {
	/// Every rule as the rules page lists it: ranked as Actual runs them
	/// (stage, then specificity, then id), without completed bills' rules
	/// or rules Actual can't read.
	pub fn rules(&self) -> Result<Vec<RuleSentence>, FfiError> {
		self.core.call(|session| {
			session.data().map(|budget| {
				veritas_rules::sentences(budget)
					.into_iter()
					.map(sentence)
					.collect()
			})
		})
	}

	/// "Apply to existing": the live rows that aren't split parents and that
	/// the rule's conditions find the way Actual's rule editor searches,
	/// newest first, each with what applying it would change.
	pub async fn rule_preview(
		&self,
		source: RuleSource,
		cancel: Arc<CancelHandle>,
	) -> Result<Vec<RuleMatch>, FfiError> {
		let core = self.core.clone();
		run(&cancel, move |cancel| {
			core.report(cancel, |budget, _| preview(budget, &source))
		})
		.await
	}
}

#[cfg(test)]
mod tests {
	use serde_json::{Value, json};

	use super::*;
	use crate::Command;
	use crate::session::Core;
	use crate::session::tests::{
		actual_apply, actual_apply_text, actual_reads, data, imported_from, table, this_month,
		transfer_payee_of, two_account_base,
	};

	fn condition(field: RuleField, op: RuleConditionOp, value: RuleValue) -> RuleCondition {
		RuleCondition { field, op, value }
	}

	#[test]
	fn filters_that_type_alike_keep_their_own_stored_text() {
		let budget = BudgetData::empty(veritas_domain::BudgetType::Envelope);
		let stored: Vec<Json> = [
			r#"{"field":"notes","op":"contains","value":"trip","type":"string","customName":"Trips"}"#,
			r#"{"field":"notes","op":"contains","value":"trip","type":"string"}"#,
			r#"{"field":"mood","op":"is","value":"happy","type":"string"}"#,
			r#"{"field":"amount","op":"gt","value":-500,"type":"number"}"#,
		]
		.iter()
		.map(|text| Json::parse(text).unwrap())
		.collect();
		let typed = filter_conditions(&budget, &stored);
		assert_eq!(typed.len(), 3);
		assert_eq!(typed[0], typed[1]);
		let text = |filters: &[Json]| filters.iter().map(Json::stringify).collect::<Vec<_>>();
		assert_eq!(
			text(&stored_filters(&budget, &typed, &stored).unwrap()),
			text(&stored)
		);
		let mut edited = typed.clone();
		edited[2].value = RuleValue::Amount { amount: -600 };
		let written = text(&stored_filters(&budget, &edited, &stored).unwrap());
		assert_eq!(written[..3], text(&stored)[..3]);
		assert_eq!(
			written[3],
			r#"{"field":"amount","op":"gt","value":-600,"type":"number"}"#
		);
	}

	fn set(field: RuleField, value: RuleValue) -> RuleAction {
		RuleAction {
			op: RuleActionOp::Set,
			field: Some(field),
			value,
			split_index: 0,
		}
	}

	fn id(id: &str) -> RuleValue {
		RuleValue::Id {
			id: Some(id.to_owned()),
		}
	}

	fn stored_rule(core: &Core<veritas_store::Store>, id: &str) -> Value {
		let rule = &data(core).rules[&RuleId::new(id)];
		json!({
			"stage": match rule.stage {
				veritas_domain::RuleStage::Pre => Some("pre"),
				veritas_domain::RuleStage::Post => Some("post"),
				veritas_domain::RuleStage::Default => None,
			},
			"conditions_op": match rule.conditions_op {
				ConditionsOp::And => "and",
				ConditionsOp::Or => "or",
			},
			"conditions": rule.conditions_json,
			"actions": rule.actions_json,
			"tombstone": i64::from(rule.tombstone),
		})
	}

	fn actual_rule(dump: &Value, id: &str) -> Value {
		let row = table(dump, "rules")
			.iter()
			.find(|row| row["id"] == id)
			.unwrap();
		json!({
			"stage": row["stage"],
			"conditions_op": row["conditions_op"],
			"conditions": row["conditions"],
			"actions": row["actions"],
			"tombstone": row["tombstone"],
		})
	}

	/// A createRule or updateRule op carrying a rule as Veritas stored it,
	/// under the public field names the API takes. Actual stores items in
	/// the order its caller gives them, with `field` renamed in place.
	fn rule_op(op: &str, id: &str, stored: &Value) -> String {
		let public = |text: &Value| {
			text.as_str()
				.unwrap()
				.replace(r#""field":"description""#, r#""field":"payee""#)
				.replace(
					r#""field":"imported_description""#,
					r#""field":"imported_payee""#,
				)
				.replace(r#""field":"acct""#, r#""field":"account""#)
		};
		format!(
			r#"{{"op":"{op}","id":"{id}","stage":{},"conditionsOp":{},"conditions":{},"actions":{}}}"#,
			stored["stage"],
			stored["conditions_op"],
			public(&stored["conditions"]),
			public(&stored["actions"]),
		)
	}

	#[test]
	fn a_rule_is_created_edited_and_deleted_as_actuals_api_stores_it() {
		// R-RULE-2 against Actual's createRule, updateRule and deleteRule on the same base.
		let (_, today) = this_month();
		let (scratch, base, _, savings, category) = two_account_base("rule-crud-oracle");
		let core = imported_from(&scratch, &base, "veritas");
		let to_savings = transfer_payee_of(&core, &savings);
		let created = RuleDraft {
			stage: RuleStage::Pre,
			conditions_op: RuleConditionsOp::And,
			conditions: vec![condition(
				RuleField::ImportedPayee,
				RuleConditionOp::Contains,
				RuleValue::Text {
					text: "AMZN Mktp".to_owned(),
				},
			)],
			actions: vec![
				set(RuleField::Payee, id(&to_savings)),
				set(RuleField::Category, id(&category)),
			],
		};
		let change = core
			.perform(Command::CreateRule {
				draft: created.clone(),
			})
			.unwrap();
		let rule_id = change
			.entities
			.iter()
			.find(|entity| entity.kind == crate::EntityKind::Rule)
			.and_then(|entity| entity.id.clone())
			.unwrap();
		let listed = core
			.call(|session| session.data().map(veritas_rules::sentences))
			.unwrap();
		assert_eq!(listed.len(), 1);
		assert_eq!(listed[0].access, veritas_rules::Access::Editable);
		let after_create = stored_rule(&core, &rule_id);

		let mut edited = created;
		edited.stage = RuleStage::Default;
		edited.conditions_op = RuleConditionsOp::Or;
		edited.conditions.push(condition(
			RuleField::AmountOutflow,
			RuleConditionOp::Gt,
			RuleValue::Amount { amount: 10_000 },
		));
		edited.actions[1] = RuleAction {
			op: RuleActionOp::AppendNotes,
			field: None,
			value: RuleValue::Text {
				text: " #shopping".to_owned(),
			},
			split_index: 0,
		};
		core.perform(Command::EditRule {
			rule_id: rule_id.clone(),
			draft: edited,
		})
		.unwrap();
		let after_edit = stored_rule(&core, &rule_id);
		core.perform(Command::DeleteRule {
			rule_id: rule_id.clone(),
		})
		.unwrap();
		let after_delete = stored_rule(&core, &rule_id);

		let rule_op = |op: &str, stored: &Value| format!("[{}]", rule_op(op, &rule_id, stored));
		let first = scratch.0.join("created.zip");
		if !actual_apply_text(&base, &rule_op("createRule", &after_create), &first, &today) {
			return;
		}
		assert_eq!(
			actual_rule(&actual_reads(&first, &today).unwrap(), &rule_id),
			after_create
		);

		let second = scratch.0.join("edited.zip");
		assert!(actual_apply_text(
			&first,
			&rule_op("updateRule", &after_edit),
			&second,
			&today
		));
		assert_eq!(
			actual_rule(&actual_reads(&second, &today).unwrap(), &rule_id),
			after_edit
		);

		let third = scratch.0.join("deleted.zip");
		let ops = json!([{"op": "deleteRule", "id": rule_id}]);
		assert!(actual_apply(&second, ops, &third, &today));
		assert_eq!(
			actual_rule(&actual_reads(&third, &today).unwrap(), &rule_id),
			after_delete
		);
	}

	fn by_notes<'a>(rows: &'a [Value], account: &str, notes: &str) -> &'a Value {
		rows.iter()
			.find(|row| {
				row["acct"] == account
					&& row["tombstone"] == 0
					&& row["notes"].as_str().is_some_and(|n| n.starts_with(notes))
			})
			.unwrap()
	}

	#[test]
	fn apply_to_existing_writes_what_actuals_rule_editor_writes() {
		// R-RULE-2's apply against Actual's rule-apply-actions, with the transfer
		// hook it runs: setting a transfer payee makes the other leg.
		let (_, today) = this_month();
		let (scratch, base, checking, savings, category) = two_account_base("rule-apply-oracle");
		let probe = imported_from(&scratch, &base, "probe");
		let to_savings = transfer_payee_of(&probe, &savings);
		probe.close().unwrap();
		let seeded = scratch.0.join("seeded.zip");
		let ops = json!([
			{"op": "addTransactions", "account": checking, "transactions": [
				{"date": "2026-11-10", "amount": -2500, "notes": "AMZN order 1", "category": category},
				{"date": "2026-11-11", "amount": -4000, "notes": "amzn order 2"},
				{"date": "2026-11-12", "amount": -900, "notes": "coffee"}
			]},
			{"op": "createRule", "id": "move-amzn", "conditions": [{"op": "contains", "field": "notes", "value": "amzn", "type": "string"}],
			 "actions": [
				{"op": "set", "field": "payee", "value": to_savings, "type": "id"},
				{"op": "append-notes", "field": "notes", "value": " (moved)", "type": "id"},
				{"op": "set", "field": "cleared", "value": true, "type": "boolean"}
			 ]},
		]);
		if !actual_apply(&base, ops, &seeded, &today) {
			return;
		}
		let core = imported_from(&scratch, &seeded, "veritas");
		let ids = |core: &Core<veritas_store::Store>| {
			let data = data(core);
			let find = |notes: &str| {
				data.transactions
					.values()
					.find(|t| t.notes.as_deref() == Some(notes))
					.unwrap()
					.id
					.to_string()
			};
			(find("AMZN order 1"), find("amzn order 2"))
		};
		let (first, second) = ids(&core);
		let source = RuleSource::Saved {
			rule_id: "move-amzn".to_owned(),
		};
		let preview = core
			.call(|session| preview(session.data()?, &source))
			.unwrap();
		let found: Vec<&str> = preview.iter().map(|m| m.transaction_id.as_str()).collect();
		assert_eq!(found, [second.as_str(), first.as_str()]);
		assert!(
			preview[0]
				.changes
				.iter()
				.any(|c| c.field == RuleChangedField::Payee && c.after == id(&to_savings))
		);
		assert!(preview[1].changes.iter().any(
			|c| c.field == RuleChangedField::Category && c.after == RuleValue::Id { id: None }
		));
		core.perform(Command::ApplyRule {
			source,
			transaction_ids: vec![first.clone(), second.clone()],
		})
		.unwrap();
		let snapshots = core.call(|session| session.snapshots()).unwrap();
		assert!(
			snapshots
				.iter()
				.any(|s| s.reason == veritas_session::SnapshotReason::RuleApply)
		);
		let ours = data(&core);
		let view = |t: &veritas_domain::Transaction| {
			json!({
				"acct": t.account.as_str(),
				"amount": t.amount.minor(),
				"description": t.payee.as_ref().map(|p| p.as_str()),
				"notes": t.notes,
				"cleared": i64::from(t.cleared),
				"category": t.category.as_ref().map(|c| c.as_str()),
				"has_transfer": t.transfer.is_some(),
			})
		};
		let ours: Vec<(Value, Value)> = [&first, &second]
			.iter()
			.map(|id| {
				let row = &ours.transactions[&TransactionId::new(id.as_str())];
				let leg = &ours.transactions[row.transfer.as_ref().unwrap()];
				(view(row), view(leg))
			})
			.collect();

		let out = scratch.0.join("applied.zip");
		let ops =
			json!([{"op": "applyActions", "rule": "move-amzn", "transactions": [first, second]}]);
		assert!(actual_apply(&seeded, ops, &out, &today));
		let dump = actual_reads(&out, &today).unwrap();
		let rows = table(&dump, "transactions");
		let view = |r: &Value| {
			json!({
				"acct": r["acct"], "amount": r["amount"], "description": r["description"],
				"notes": r["notes"], "cleared": r["cleared"], "category": r["category"],
				"has_transfer": r["transferred_id"].is_string(),
			})
		};
		let theirs: Vec<(Value, Value)> = ["AMZN order 1", "amzn order 2"]
			.iter()
			.map(|notes| {
				let row = by_notes(rows, &checking, notes);
				let leg = rows
					.iter()
					.find(|r| r["id"] == row["transferred_id"])
					.unwrap();
				(view(row), view(leg))
			})
			.collect();
		assert_eq!(ours, theirs);
	}

	#[test]
	fn a_split_rule_is_stored_and_applied_as_actual_does() {
		// R-RULE-2 splits: createRule stores Veritas's text, and
		// rule-apply-actions makes the same parent, lines and transfer leg.
		let (_, today) = this_month();
		let (scratch, base, checking, savings, category) = two_account_base("rule-split-oracle");
		let seeded = scratch.0.join("seeded.zip");
		let ops = json!([{"op": "addTransactions", "account": checking, "transactions": [
			{"date": "2026-11-10", "amount": -10_000, "notes": "split me", "category": category}
		]}]);
		if !actual_apply(&base, ops, &seeded, &today) {
			return;
		}
		let core = imported_from(&scratch, &seeded, "veritas");
		let to_savings = transfer_payee_of(&core, &savings);
		let line = |split_index, op, field, value| RuleAction {
			op,
			field,
			value,
			split_index,
		};
		let draft = RuleDraft {
			stage: RuleStage::Default,
			conditions_op: RuleConditionsOp::And,
			conditions: vec![condition(
				RuleField::Notes,
				RuleConditionOp::Is,
				RuleValue::Text {
					text: "split me".to_owned(),
				},
			)],
			actions: vec![
				line(
					1,
					RuleActionOp::SetSplitAmount,
					None,
					RuleValue::Amount { amount: -2500 },
				),
				line(
					1,
					RuleActionOp::Set,
					Some(RuleField::Category),
					id(&category),
				),
				line(
					2,
					RuleActionOp::SetSplitAmount,
					None,
					RuleValue::Percent { percent: 50.0 },
				),
				line(
					2,
					RuleActionOp::Set,
					Some(RuleField::Notes),
					RuleValue::Text {
						text: "half".to_owned(),
					},
				),
				line(3, RuleActionOp::SetSplitAmount, None, RuleValue::Nothing),
				line(
					3,
					RuleActionOp::Set,
					Some(RuleField::Payee),
					id(&to_savings),
				),
			],
		};
		let change = core.perform(Command::CreateRule { draft }).unwrap();
		let rule_id = change
			.entities
			.iter()
			.find(|entity| entity.kind == crate::EntityKind::Rule)
			.and_then(|entity| entity.id.clone())
			.unwrap();
		let stored = stored_rule(&core, &rule_id);
		let parent_id = data(&core)
			.transactions
			.values()
			.find(|t| t.notes.as_deref() == Some("split me"))
			.unwrap()
			.id
			.to_string();
		let source = RuleSource::Saved {
			rule_id: rule_id.clone(),
		};
		let preview = core
			.call(|session| preview(session.data()?, &source))
			.unwrap();
		assert_eq!(preview.len(), 1);
		let amounts: Vec<i64> = preview[0].split_lines.iter().map(|l| l.amount).collect();
		assert_eq!(amounts, [-2500, -3750, -3750]);
		core.perform(Command::ApplyRule {
			source,
			transaction_ids: vec![parent_id.clone()],
		})
		.unwrap();
		let ours = data(&core);
		let row = |t: &veritas_domain::Transaction| {
			json!({
				"amount": t.amount.minor(),
				"description": t.payee.as_ref().map(|p| p.as_str()),
				"category": t.category.as_ref().map(|c| c.as_str()),
				"notes": t.notes,
				"cleared": i64::from(t.cleared),
				"sort_order": t.sort_order,
				"isParent": i64::from(t.split == veritas_domain::SplitRole::Parent),
				"isChild": i64::from(matches!(t.split, veritas_domain::SplitRole::Child { .. })),
				"error": t.split_error,
				"leg": t.transfer.as_ref().map(|leg| {
					let leg = &ours.transactions[leg];
					json!([leg.account.as_str(), leg.amount.minor(), leg.payee.as_ref().map(|p| p.as_str()), leg.notes])
				}),
			})
		};
		let parent = &ours.transactions[&TransactionId::new(parent_id.as_str())];
		let mut lines: Vec<&veritas_domain::Transaction> = ours
			.transactions
			.values()
			.filter(|t| {
				matches!(&t.split, veritas_domain::SplitRole::Child { parent } if parent.as_str() == parent_id)
			})
			.collect();
		lines.sort_by(|a, b| b.sort_order.total_cmp(&a.sort_order));
		let ours: Vec<Value> = std::iter::once(parent).chain(lines).map(row).collect();

		let out = scratch.0.join("applied.zip");
		let ops = format!(
			r#"[{},{{"op":"applyActions","rule":"{rule_id}","transactions":["{parent_id}"]}}]"#,
			rule_op("createRule", &rule_id, &stored)
		);
		assert!(actual_apply_text(&seeded, &ops, &out, &today));
		let dump = actual_reads(&out, &today).unwrap();
		assert_eq!(actual_rule(&dump, &rule_id), stored);
		let rows = table(&dump, "transactions");
		let row = |r: &Value| {
			let leg = rows
				.iter()
				.find(|leg| r["transferred_id"].is_string() && leg["id"] == r["transferred_id"]);
			json!({
				"amount": r["amount"], "description": r["description"], "category": r["category"],
				"notes": r["notes"], "cleared": r["cleared"], "sort_order": r["sort_order"].as_f64(),
				"isParent": r["isParent"], "isChild": r["isChild"], "error": r["error"],
				"leg": leg.map(|leg| json!([leg["acct"], leg["amount"], leg["description"], leg["notes"]])),
			})
		};
		let parent = rows.iter().find(|r| r["id"] == parent_id.as_str()).unwrap();
		let mut lines: Vec<&Value> = rows
			.iter()
			.filter(|r| r["parent_id"] == parent_id.as_str() && r["tombstone"] == 0)
			.collect();
		lines.sort_by(|a, b| {
			b["sort_order"]
				.as_f64()
				.unwrap()
				.total_cmp(&a["sort_order"].as_f64().unwrap())
		});
		let theirs: Vec<Value> = std::iter::once(parent).chain(lines).map(row).collect();
		assert_eq!(ours, theirs);
	}
}
