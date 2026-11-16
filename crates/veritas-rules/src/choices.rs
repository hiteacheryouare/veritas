//! A port of the choices Actual's rule editor offers (v26.11.0), so the
//! sentence editor's pickers come from the same tables instead of a copy in
//! the app: `TYPE_INFO`, `FIELD_INFO` and `getValidOps` from
//! `packages/loot-core/src/shared/rules.ts` (lines 6-118); `conditionFields`,
//! `getActionFields`, `getSplitActionFields`, the action op list and the
//! inflow and outflow op filter from
//! `packages/desktop-client/src/components/rules/RuleEditor.tsx` (lines
//! 443-460, 508-521, 965-976 and 1004-1019); and `getAllocationMethods` from
//! `packages/desktop-client/src/util/rule.ts` (lines 26-33). Actual's
//! feature flags for action templating and formulas are taken as off.

use crate::condition::{AmountSign, Field, FieldKind, Op};
use crate::sentence::ActionOp;

/// The value a draft gives an op or a field, as `check_condition` and
/// `check_action` accept it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueKind {
	/// A payee, category, category group or account id.
	Id,
	/// One or more ids.
	Ids,
	Text,
	/// One or more texts, none blank.
	Texts,
	/// Minor units, in the sign the field states (see [`ConditionChoice`]).
	Amount,
	/// `isbetween`'s two ends.
	AmountRange,
	/// `YYYY-MM-DD`.
	Date,
	/// `YYYY-MM-DD`, `YYYY-MM` or `YYYY`: a date `is` matches a day, a month or a year.
	DateMonthOrYear,
	Bool,
	/// A split line's percentage of what the fixed amounts leave.
	Percent,
	Nothing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConditionChoice {
	pub field: Field,
	/// `Inflow` and `Outflow` are Actual's `amount-inflow` and
	/// `amount-outflow` pseudo-fields (`lc/shared/rules.ts:120-128`): a
	/// positive number that matches only money in, or only money out
	/// compared as its absolute value (`lc/server/rules/condition.ts:255-266`).
	pub sign: AmountSign,
	pub ops: Vec<(Op, ValueKind)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choices {
	pub conditions: Vec<ConditionChoice>,
	/// The ops on the transaction itself. `None` for `set`, whose value
	/// follows its field.
	pub action_ops: Vec<(ActionOp, Option<ValueKind>)>,
	pub action_fields: Vec<(Field, ValueKind)>,
	pub split_fields: Vec<(Field, ValueKind)>,
	/// A split line's `set-split-amount`: fixed amount, percent, remainder.
	pub split_amounts: Vec<ValueKind>,
}

/// The value an op takes on a field.
pub fn condition_value(field: Field, op: Op) -> ValueKind {
	match (field.kind(), op) {
		(_, Op::OnBudget | Op::OffBudget | Op::And) => ValueKind::Nothing,
		(FieldKind::Id, Op::Is | Op::IsNot) => ValueKind::Id,
		(FieldKind::Id, Op::OneOf | Op::NotOneOf) => ValueKind::Ids,
		(FieldKind::Text, Op::OneOf | Op::NotOneOf) => ValueKind::Texts,
		(FieldKind::Id | FieldKind::Text, _) => ValueKind::Text,
		(FieldKind::Number, Op::IsBetween) => ValueKind::AmountRange,
		(FieldKind::Number, _) => ValueKind::Amount,
		(FieldKind::Date, Op::Is) => ValueKind::DateMonthOrYear,
		(FieldKind::Date, _) => ValueKind::Date,
		(FieldKind::Boolean, _) => ValueKind::Bool,
	}
}

fn action_value(field: Field) -> ValueKind {
	match field.kind() {
		FieldKind::Id => ValueKind::Id,
		FieldKind::Text => ValueKind::Text,
		FieldKind::Number => ValueKind::Amount,
		FieldKind::Date => ValueKind::Date,
		FieldKind::Boolean => ValueKind::Bool,
	}
}

fn condition(field: Field, sign: AmountSign) -> ConditionChoice {
	ConditionChoice {
		field,
		sign,
		ops: Op::valid_ops(field)
			.into_iter()
			// Inflow and outflow drop `isbetween` (`RuleEditor.tsx:965-976`).
			.filter(|op| sign == AmountSign::Any || *op != Op::IsBetween)
			.map(|op| (op, condition_value(field, op)))
			.collect(),
	}
}

pub fn choices() -> Choices {
	use AmountSign::{Any, Inflow, Outflow};
	use Field::*;
	let conditions = [
		(ImportedPayee, Any),
		(Account, Any),
		(Category, Any),
		(CategoryGroup, Any),
		(Date, Any),
		(Payee, Any),
		(Notes, Any),
		(Amount, Any),
		(Amount, Inflow),
		(Amount, Outflow),
		(Cleared, Any),
	]
	.into_iter()
	.map(|(field, sign)| condition(field, sign))
	.collect();
	// `getActionFields` without `payee_name`, which the editor hides while
	// action templating and formulas are off (`RuleEditor.tsx:508-513`).
	let action_fields: Vec<(Field, ValueKind)> =
		[Category, Payee, Notes, Cleared, Account, Date, Amount]
			.into_iter()
			.map(|field| (field, action_value(field)))
			.collect();
	// `getSplitActionFields`: without the parent-only fields (`RuleEditor.tsx:455-460`).
	let split_fields = action_fields
		.iter()
		.copied()
		.filter(|(field, _)| !matches!(field, Amount | Cleared | Account | Date))
		.collect();
	Choices {
		conditions,
		action_ops: vec![
			(ActionOp::Set, None),
			(ActionOp::PrependNotes, Some(ValueKind::Text)),
			(ActionOp::AppendNotes, Some(ValueKind::Text)),
			(ActionOp::DeleteTransaction, Some(ValueKind::Nothing)),
		],
		action_fields,
		split_fields,
		split_amounts: vec![ValueKind::Amount, ValueKind::Percent, ValueKind::Nothing],
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn every_offered_op_is_one_a_draft_accepts() {
		for choice in choices().conditions {
			assert!(!choice.ops.is_empty());
			for (op, _) in choice.ops {
				assert!(op.valid_for(choice.field), "{:?} {op:?}", choice.field);
			}
		}
	}

	#[test]
	fn the_ops_follow_type_info_order() {
		assert_eq!(
			Op::valid_ops(Field::Payee),
			[
				Op::Is,
				Op::Contains,
				Op::Matches,
				Op::OneOf,
				Op::IsNot,
				Op::DoesNotContain,
				Op::NotOneOf
			]
		);
		assert_eq!(
			Op::valid_ops(Field::Notes),
			[
				Op::Is,
				Op::Contains,
				Op::Matches,
				Op::IsNot,
				Op::DoesNotContain,
				Op::HasTags,
				Op::HasAnyTag
			]
		);
	}
}
