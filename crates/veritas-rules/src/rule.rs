//! A stored rule parsed into conditions and actions, with Actual's ranking
//! and candidate indexing (reference/actual/packages/loot-core/src/server/rules/).

use std::collections::BTreeSet;

use serde_json::Value;
use veritas_domain::{ConditionsOp, Rule, RuleId, RuleStage};

use crate::action::Action;
use crate::condition::{Condition, ConditionValue, Field, Gap, Op};

/// One stored item. `raw` is the parsed JSON for display; the rule's text in
/// `veritas_domain::Rule` is never rewritten, so it round-trips byte for byte.
#[derive(Clone, Debug, PartialEq)]
pub struct Item<T> {
	pub raw: Value,
	pub parsed: Result<T, Gap>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleSupport {
	/// Veritas evaluates and applies every condition and action.
	Full,
	/// Actual runs it but Veritas can't, so it's shown read-only (R-RULE-3).
	ReadOnly(Vec<String>),
	/// Actual drops it when it loads rules, so it never runs anywhere.
	IgnoredByActual(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedRule {
	pub id: RuleId,
	pub stage: RuleStage,
	pub conditions_op: ConditionsOp,
	pub conditions: Vec<Item<Condition>>,
	pub actions: Vec<Item<Action>>,
	/// Set when the stored text isn't a JSON array at all.
	pub unreadable: Option<String>,
}

/// A rule indexer key (`rule-indexer.ts`). `Any` is Actual's `'*'`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum IndexKey {
	Any,
	Text(String),
	/// JS `value[0]` on a character outside the BMP is a lone high surrogate.
	Surrogate(u16),
}

impl IndexKey {
	pub(crate) fn first_char(text: &str) -> Option<IndexKey> {
		let first = text.chars().next()?;
		let mut units = [0u16; 2];
		Some(match first.encode_utf16(&mut units) {
			[unit] => IndexKey::Text(
				char::from_u32(u32::from(*unit))
					.map_or_else(String::new, |c| c.to_lowercase().collect()),
			),
			[high, ..] => IndexKey::Surrogate(*high),
			[] => return None,
		})
	}

	pub(crate) fn whole(text: &str) -> Option<IndexKey> {
		(!text.is_empty()).then(|| IndexKey::Text(text.to_lowercase()))
	}
}

fn parse_items<T>(
	json: &str,
	parse: impl Fn(&Value) -> Result<T, Gap>,
) -> Result<Vec<Item<T>>, String> {
	if json.is_empty() {
		return Ok(Vec::new());
	}
	match serde_json::from_str::<Value>(json) {
		Ok(Value::Array(items)) => Ok(items
			.into_iter()
			.map(|raw| Item {
				parsed: parse(&raw),
				raw,
			})
			.collect()),
		Ok(_) => Err("rule json must be an array".into()),
		Err(_) => Err("cannot parse rule json".into()),
	}
}

impl ParsedRule {
	pub fn parse(rule: &Rule) -> ParsedRule {
		let conditions = parse_items(&rule.conditions_json, Condition::parse);
		let actions = parse_items(&rule.actions_json, Action::parse);
		let unreadable = conditions
			.as_ref()
			.err()
			.or(actions.as_ref().err())
			.cloned();
		ParsedRule {
			id: rule.id.clone(),
			stage: rule.stage,
			conditions_op: rule.conditions_op,
			conditions: conditions.unwrap_or_default(),
			actions: actions.unwrap_or_default(),
			unreadable,
		}
	}

	pub fn support(&self) -> RuleSupport {
		if let Some(reason) = &self.unreadable {
			return RuleSupport::IgnoredByActual(reason.clone());
		}
		let gaps = self
			.conditions
			.iter()
			.filter_map(|item| item.parsed.as_ref().err())
			.chain(
				self.actions
					.iter()
					.filter_map(|item| item.parsed.as_ref().err()),
			);
		let mut unsupported = Vec::new();
		for gap in gaps {
			match gap {
				Gap::RejectedByActual(reason) => {
					return RuleSupport::IgnoredByActual(reason.clone());
				}
				Gap::Unsupported(reason) => unsupported.push(reason.clone()),
			}
		}
		if unsupported.is_empty() {
			RuleSupport::Full
		} else {
			RuleSupport::ReadOnly(unsupported)
		}
	}

	pub(crate) fn migrate_ids(&mut self, map: &impl Fn(&str) -> Option<String>) {
		for item in &mut self.conditions {
			if let Ok(condition) = &mut item.parsed {
				condition.migrate_ids(map);
			}
		}
		for item in &mut self.actions {
			if let Ok(action) = &mut item.parsed {
				action.migrate_ids(map);
			}
		}
	}

	/// `computeScore` (`rule-utils.ts:38-61`): the sum of the op points, an
	/// unknown op resetting the running total, doubled when every op is an
	/// exact-ish one.
	pub fn specificity(&self) -> i64 {
		let ops: Vec<&str> = self
			.conditions
			.iter()
			.map(|item| {
				item.raw
					.get("op")
					.and_then(Value::as_str)
					.unwrap_or_default()
			})
			.collect();
		let total = ops.iter().fold(0, |score, op| match *op {
			"is" | "isNot" => score + 10,
			"oneOf" | "notOneOf" => score + 9,
			"isapprox" | "isbetween" => score + 5,
			"gt" | "gte" | "lt" | "lte" => score + 1,
			"contains" | "doesNotContain" | "matches" | "hasTags" | "hasAnyTag" | "onBudget"
			| "offBudget" => score,
			_ => 0,
		});
		let exact = ops
			.iter()
			.all(|op| matches!(*op, "is" | "isNot" | "isapprox" | "oneOf" | "notOneOf"));
		if exact { total * 2 } else { total }
	}

	/// The keys `RuleIndexer.getIndexes` files this rule under for `field`:
	/// its first condition on that field decides, and only the is-family ops
	/// narrow it. An empty `oneOf` files it nowhere.
	pub(crate) fn index_keys(
		&self,
		field: Field,
		key: fn(&str) -> Option<IndexKey>,
	) -> BTreeSet<IndexKey> {
		let first = self.conditions.iter().find(|item| {
			item.raw
				.get("field")
				.and_then(Value::as_str)
				.and_then(Field::from_stored)
				== Some(field)
		});
		let Some(Ok(condition)) = first.map(|item| &item.parsed) else {
			return BTreeSet::from([IndexKey::Any]);
		};
		let of = |value: Option<&str>| value.and_then(key).unwrap_or(IndexKey::Any);
		match (condition.op, &condition.value) {
			(Op::Is | Op::IsNot, ConditionValue::Text(text)) => BTreeSet::from([of(Some(text))]),
			(Op::Is | Op::IsNot, ConditionValue::Id(id)) => BTreeSet::from([of(id.as_deref())]),
			(Op::OneOf | Op::NotOneOf, ConditionValue::TextList(list)) => {
				list.iter().map(|text| of(Some(text))).collect()
			}
			(Op::OneOf | Op::NotOneOf, ConditionValue::IdList(list)) => {
				list.iter().map(|id| of(id.as_deref())).collect()
			}
			_ => BTreeSet::from([IndexKey::Any]),
		}
	}
}

/// `rankRules`: stage pre, then the default stage, then post; within a stage
/// ascending specificity, then id. More specific rules run later and win.
pub(crate) fn rank<'a>(rules: impl IntoIterator<Item = &'a ParsedRule>) -> Vec<&'a ParsedRule> {
	let stage = |rule: &ParsedRule| match rule.stage {
		RuleStage::Pre => 0,
		RuleStage::Default => 1,
		RuleStage::Post => 2,
	};
	let mut ranked: Vec<(u8, i64, &ParsedRule)> = rules
		.into_iter()
		.map(|rule| (stage(rule), rule.specificity(), rule))
		.collect();
	ranked.sort_by(|a, b| {
		(a.0, a.1)
			.cmp(&(b.0, b.1))
			.then_with(|| a.2.id.as_str().cmp(b.2.id.as_str()))
	});
	ranked.into_iter().map(|(_, _, rule)| rule).collect()
}

#[cfg(test)]
mod tests {
	use serde_json::json;

	use super::*;

	pub(crate) fn rule(id: &str, stage: RuleStage, conditions: Value, actions: Value) -> Rule {
		Rule {
			id: RuleId::new(id),
			stage,
			conditions_op: ConditionsOp::And,
			conditions_json: conditions.to_string(),
			actions_json: actions.to_string(),
			tombstone: false,
		}
	}

	fn parsed(conditions: Value) -> ParsedRule {
		ParsedRule::parse(&rule("r", RuleStage::Default, conditions, json!([])))
	}

	#[test]
	fn specificity_follows_compute_score() {
		let score = |conditions: Value| parsed(conditions).specificity();
		assert_eq!(
			score(json!([{"op": "is", "field": "payee", "value": "p"}])),
			20
		);
		assert_eq!(
			score(
				json!([{"op": "is", "field": "payee", "value": "p"}, {"op": "contains", "field": "notes", "value": "x"}])
			),
			10
		);
		assert_eq!(
			score(
				json!([{"op": "oneOf", "field": "payee", "value": ["p"]}, {"op": "isapprox", "field": "amount", "value": 5}])
			),
			28
		);
		assert_eq!(
			score(json!([{"op": "isbetween", "field": "amount", "value": {"num1": 1, "num2": 2}}])),
			5
		);
		assert_eq!(
			score(json!([{"op": "gt", "field": "amount", "value": 1}])),
			1
		);
		// The internal `and` op isn't scored, so it resets the total so far.
		assert_eq!(
			score(
				json!([{"op": "is", "field": "payee", "value": "p"}, {"op": "and", "field": "category", "value": []}, {"op": "gt", "field": "amount", "value": 1}])
			),
			1
		);
		assert_eq!(score(json!([])), 0);
	}

	#[test]
	fn ranking_orders_stage_then_score_then_id() {
		// reference/actual/packages/loot-core/src/server/rules/index.test.ts rankRules cases.
		let payee_is = json!([{"op": "is", "field": "payee", "value": "p"}]);
		let contains = json!([{"op": "contains", "field": "imported_payee", "value": "x"}]);
		let rules = [
			rule("b", RuleStage::Default, payee_is.clone(), json!([])),
			rule("a", RuleStage::Default, payee_is.clone(), json!([])),
			rule("c", RuleStage::Default, contains.clone(), json!([])),
			rule("z", RuleStage::Post, contains.clone(), json!([])),
			rule("y", RuleStage::Pre, payee_is, json!([])),
		];
		let parsed: Vec<ParsedRule> = rules.iter().map(ParsedRule::parse).collect();
		let order: Vec<&str> = rank(&parsed).iter().map(|rule| rule.id.as_str()).collect();
		assert_eq!(order, ["y", "c", "a", "b", "z"]);
	}

	#[test]
	fn support_classifies_each_rule() {
		let full =
			parsed(json!([{"op": "contains", "field": "imported_description", "value": "amzn"}]));
		assert_eq!(full.support(), RuleSupport::Full);
		let regex = parsed(json!([{"op": "matches", "field": "notes", "value": "^a"}]));
		assert!(matches!(regex.support(), RuleSupport::ReadOnly(_)));
		let broken = parsed(json!([{"op": "oneOf", "field": "notes", "value": ["a"]}]));
		assert!(matches!(broken.support(), RuleSupport::IgnoredByActual(_)));
		let mut garbage = rule("g", RuleStage::Default, json!([]), json!([]));
		garbage.conditions_json = "{not json".into();
		assert!(matches!(
			ParsedRule::parse(&garbage).support(),
			RuleSupport::IgnoredByActual(_)
		));
	}

	#[test]
	fn index_keys_come_from_the_first_condition_on_the_field() {
		let keys = |conditions: Value| {
			parsed(conditions).index_keys(Field::ImportedPayee, IndexKey::first_char)
		};
		assert_eq!(
			keys(json!([{"op": "is", "field": "imported_payee", "value": "Amazon"}])),
			BTreeSet::from([IndexKey::Text("a".into())])
		);
		assert_eq!(
			keys(json!([{"op": "oneOf", "field": "imported_payee", "value": ["b", "C", ""]}])),
			BTreeSet::from([IndexKey::Text("b".into()), IndexKey::Text("c".into())])
		);
		assert_eq!(
			keys(
				json!([{"op": "contains", "field": "imported_payee", "value": "a"}, {"op": "is", "field": "imported_payee", "value": "b"}])
			),
			BTreeSet::from([IndexKey::Any])
		);
		assert_eq!(
			keys(json!([{"op": "oneOf", "field": "imported_payee", "value": []}])),
			BTreeSet::new()
		);
		assert_eq!(keys(json!([])), BTreeSet::from([IndexKey::Any]));
		assert_eq!(
			IndexKey::first_char("😀x"),
			Some(IndexKey::Surrogate(0xd83d))
		);
	}
}
