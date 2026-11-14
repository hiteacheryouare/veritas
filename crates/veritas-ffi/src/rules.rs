//! The sentence-style rule editor (R-RULE-2, R-RULE-3): the rules list, the
//! models drafts are saved from, and "apply to existing" with its preview.

use std::sync::Arc;

use veritas_domain::{BudgetData, ConditionsOp, CoreError, Row, RuleId, TransactionId};
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
	/// A split line's amount. Read only.
	SetSplitAmount,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
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
	Nothing,
	/// Stored JSON the sentence can't show. Read only.
	Raw {
		json: String,
	},
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RuleCondition {
	pub field: RuleField,
	pub op: RuleConditionOp,
	pub value: RuleValue,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RuleAction {
	pub op: RuleActionOp,
	/// Only `Set` has one.
	pub field: Option<RuleField>,
	pub value: RuleValue,
	/// 0 is the transaction itself; 1 and up are split lines (read only).
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

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
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

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RuleDraft {
	pub stage: RuleStage,
	pub conditions_op: RuleConditionsOp,
	pub conditions: Vec<RuleCondition>,
	pub actions: Vec<RuleAction>,
}

/// A saved rule, or the editor's unsaved draft.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
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

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RuleFieldChange {
	pub field: RuleChangedField,
	pub before: RuleValue,
	/// A payee the apply would create is `Text` with its name.
	pub after: RuleValue,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
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

fn core_field(field: RuleField) -> (Field, AmountSign) {
	match field {
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
	}
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
			let (field, sign) = core_field(condition.field);
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
			let field = match action.field.map(core_field) {
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
