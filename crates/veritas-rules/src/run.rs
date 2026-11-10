//! `runRules` (reference/actual/packages/loot-core/src/server/transactions/transaction-rules.ts:323-449):
//! candidate rules from the two indexers, ranked, run in a chain where each
//! rule sees the changes of the ones before it.

use std::collections::{BTreeMap, BTreeSet};

use veritas_domain::{BudgetData, CategoryId, ConditionsOp, PayeeId, RuleId, ScheduleId};

use crate::action::{Action, RuleTransaction, SetField};
use crate::condition::{Field, FieldValue};
use crate::rule::{IndexKey, ParsedRule, RuleSupport, rank};

/// The budget's live rules, parsed once, with merged ids mapped to their
/// targets the way Actual does when it loads rules.
#[derive(Clone, Debug)]
pub struct RuleBook {
	rules: Vec<ParsedRule>,
	imported_payee_keys: BTreeMap<RuleId, BTreeSet<IndexKey>>,
	payee_keys: BTreeMap<RuleId, BTreeSet<IndexKey>>,
	schedule_rules: BTreeMap<ScheduleId, RuleId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RunOutcome {
	pub transaction: RuleTransaction,
	/// Rules whose actions were applied, in the order they ran.
	pub applied: Vec<RuleId>,
	/// Rules Actual would have evaluated or applied here but Veritas can't
	/// (a regex condition, a template action). Their effect is missing.
	pub undetermined: Vec<RuleId>,
}

/// Where `set payee_name` finds or makes its payee.
pub trait PayeeDirectory {
	fn find_or_create(&mut self, name: &str) -> PayeeId;
}

/// Finds payees in the budget the way `getPayeeByName` does (a live payee
/// whose lowercased name matches, `db/index.ts:723-728`) and records the ones
/// a rule creates. The store inserts each like `insertPayee` (`:576-586`): a
/// `payees` row with just the name, plus its identity `payee_mapping` row.
/// Transfer payees never match, since their stored name is empty.
pub struct BudgetPayees<'a> {
	budget: &'a BudgetData,
	new_id: Box<dyn FnMut() -> PayeeId + 'a>,
	pub created: Vec<(PayeeId, String)>,
}

impl<'a> BudgetPayees<'a> {
	pub fn new(budget: &'a BudgetData, new_id: impl FnMut() -> PayeeId + 'a) -> Self {
		BudgetPayees {
			budget,
			new_id: Box::new(new_id),
			created: Vec::new(),
		}
	}
}

impl PayeeDirectory for BudgetPayees<'_> {
	fn find_or_create(&mut self, name: &str) -> PayeeId {
		let wanted = name.to_lowercase();
		let existing = self
			.budget
			.payees
			.values()
			.find(|payee| {
				!payee.tombstone
					&& payee.transfer_account.is_none()
					&& payee.name.to_lowercase() == wanted
			})
			.map(|payee| payee.id.clone())
			.or_else(|| {
				self.created
					.iter()
					.find(|(_, created)| created.to_lowercase() == wanted)
					.map(|(id, _)| id.clone())
			});
		existing.unwrap_or_else(|| {
			let id = (self.new_id)();
			self.created.push((id.clone(), name.to_owned()));
			id
		})
	}
}

/// Three-valued outcome of a rule's conditions: a condition Veritas can't
/// evaluate only matters when the others don't settle the answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verdict {
	Match,
	NoMatch,
	Unknown,
}

impl RuleBook {
	pub fn new(budget: &BudgetData) -> RuleBook {
		// One lookup in the union of both mapping tables, as `getMappings` keeps them.
		let map = |id: &str| -> Option<String> {
			let category = budget.category_mapping.get(&CategoryId::new(id));
			let target = category.map(CategoryId::as_str).or_else(|| {
				budget
					.payee_mapping
					.get(&PayeeId::new(id))
					.map(PayeeId::as_str)
			});
			target.map(str::to_owned)
		};
		let mut rules = Vec::new();
		let mut imported_payee_keys = BTreeMap::new();
		let mut payee_keys = BTreeMap::new();
		for rule in budget.rules.values().filter(|rule| !rule.tombstone) {
			let mut parsed = ParsedRule::parse(rule);
			if matches!(parsed.support(), RuleSupport::IgnoredByActual(_)) {
				continue;
			}
			parsed.migrate_ids(&map);
			imported_payee_keys.insert(
				parsed.id.clone(),
				parsed.index_keys(Field::ImportedPayee, IndexKey::first_char),
			);
			payee_keys.insert(
				parsed.id.clone(),
				parsed.index_keys(Field::Payee, IndexKey::whole),
			);
			rules.push(parsed);
		}
		let schedule_rules = budget
			.schedules
			.values()
			.map(|schedule| (schedule.id.clone(), schedule.rule.clone()))
			.collect();
		RuleBook {
			rules,
			imported_payee_keys,
			payee_keys,
			schedule_rules,
		}
	}

	/// The rules Actual would consider for `transaction`: a rule is skipped
	/// without being evaluated only when both indexers filed it under keys the
	/// transaction doesn't have.
	fn candidates(&self, transaction: &RuleTransaction) -> Vec<&ParsedRule> {
		let first_char = transaction
			.imported_payee
			.as_deref()
			.and_then(IndexKey::first_char);
		let payee = transaction
			.payee
			.as_ref()
			.and_then(|payee| IndexKey::whole(payee.as_str()));
		let hit = |keys: Option<&BTreeSet<IndexKey>>, key: &Option<IndexKey>| {
			keys.is_some_and(|keys| {
				keys.contains(&IndexKey::Any) || key.as_ref().is_some_and(|key| keys.contains(key))
			})
		};
		self.rules
			.iter()
			.filter(|rule| {
				hit(self.imported_payee_keys.get(&rule.id), &first_char)
					|| hit(self.payee_keys.get(&rule.id), &payee)
			})
			.collect()
	}

	pub fn run(
		&self,
		transaction: &RuleTransaction,
		budget: &BudgetData,
		payees: &mut dyn PayeeDirectory,
	) -> RunOutcome {
		let mut payee_name = transaction
			.payee
			.as_ref()
			.and_then(|payee| budget.payees.get(payee))
			.map(|payee| payee.name.clone());
		let account_off_budget = budget
			.accounts
			.get(&transaction.account)
			.map(|account| account.off_budget);
		let schedule_rule = transaction
			.schedule
			.as_ref()
			.and_then(|schedule| self.schedule_rules.get(schedule));
		let other_schedule_rules: BTreeSet<&RuleId> = match schedule_rule {
			Some(own) => self
				.schedule_rules
				.values()
				.filter(|rule| *rule != own)
				.collect(),
			None => BTreeSet::new(),
		};

		let mut working = transaction.clone();
		let mut new_payee = false;
		let mut outcome = RunOutcome {
			transaction: transaction.clone(),
			applied: Vec::new(),
			undetermined: Vec::new(),
		};
		for rule in rank(self.candidates(transaction)) {
			let verdict = if schedule_rule == Some(&rule.id) {
				Verdict::Match
			} else if other_schedule_rules.contains(&rule.id) {
				continue;
			} else {
				evaluate(
					rule,
					&working,
					payee_name.as_deref(),
					account_off_budget,
					budget,
				)
			};
			match verdict {
				Verdict::NoMatch => {}
				Verdict::Unknown => outcome.undetermined.push(rule.id.clone()),
				Verdict::Match => {
					let actions: Option<Vec<&Action>> = rule
						.actions
						.iter()
						.map(|item| item.parsed.as_ref().ok())
						.collect();
					match actions {
						Some(actions) => {
							for action in actions {
								match action {
									Action::SetPayeeName(name) => {
										payee_name = name.clone();
										new_payee = true;
									}
									Action::Set(SetField::Payee(_)) => {
										action.apply(&mut working);
										new_payee = false;
									}
									other => other.apply(&mut working),
								}
							}
							outcome.applied.push(rule.id.clone());
						}
						None => outcome.undetermined.push(rule.id.clone()),
					}
				}
			}
			if new_payee {
				working.payee = payee_name
					.as_deref()
					.filter(|name| !name.is_empty())
					.map(|name| payees.find_or_create(name));
				new_payee = false;
			}
		}
		outcome.transaction = working;
		outcome
	}
}

/// What `Condition.eval` reads for `field` on the object `runRules` builds:
/// `payee_name` is fixed from the starting payee, `category_group` follows
/// the current category, and `transfer` and `parent` are never on the object.
fn field_value(
	field: Field,
	transaction: &RuleTransaction,
	payee_name: Option<&str>,
	budget: &BudgetData,
) -> FieldValue {
	let text = |value: Option<&str>| {
		value.map_or(FieldValue::Null, |text| FieldValue::Text(text.to_owned()))
	};
	match field {
		Field::ImportedPayee => text(transaction.imported_payee.as_deref()),
		Field::Payee => text(transaction.payee.as_ref().map(|payee| payee.as_str())),
		Field::PayeeName => text(payee_name),
		Field::Account => FieldValue::Text(transaction.account.as_str().to_owned()),
		Field::Category => text(
			transaction
				.category
				.as_ref()
				.map(|category| category.as_str()),
		),
		Field::CategoryGroup => transaction
			.category
			.as_ref()
			.filter(|category| !category.as_str().is_empty())
			.and_then(|category| budget.categories.get(category))
			.map_or(FieldValue::Absent, |category| {
				FieldValue::Text(category.group.as_str().to_owned())
			}),
		Field::Notes => text(transaction.notes.as_deref()),
		Field::Amount => FieldValue::Number(transaction.amount.minor() as f64),
		Field::Date => FieldValue::Text(transaction.date.to_string()),
		Field::Cleared => transaction
			.cleared
			.map_or(FieldValue::Absent, FieldValue::Bool),
		Field::Reconciled => transaction
			.reconciled
			.map_or(FieldValue::Absent, FieldValue::Bool),
		Field::Transfer | Field::Parent => FieldValue::Absent,
	}
}

/// `Rule.evalConditions`: no conditions never matches, `or` is any and
/// anything else is all.
fn evaluate(
	rule: &ParsedRule,
	transaction: &RuleTransaction,
	payee_name: Option<&str>,
	account_off_budget: Option<bool>,
	budget: &BudgetData,
) -> Verdict {
	if rule.conditions.is_empty() {
		return Verdict::NoMatch;
	}
	let any = rule.conditions_op == ConditionsOp::Or;
	let mut unknown = false;
	for item in &rule.conditions {
		let result = match &item.parsed {
			Ok(condition) => condition
				.eval(
					field_value(condition.field, transaction, payee_name, budget),
					account_off_budget,
				)
				.ok(),
			Err(_) => None,
		};
		match result {
			Some(true) if any => return Verdict::Match,
			Some(false) if !any => return Verdict::NoMatch,
			None => unknown = true,
			_ => {}
		}
	}
	match (unknown, any) {
		(true, _) => Verdict::Unknown,
		(false, true) => Verdict::NoMatch,
		(false, false) => Verdict::Match,
	}
}

#[cfg(test)]
mod tests {
	use serde_json::{Value, json};
	use veritas_domain::{
		Account, AccountId, BudgetType, Category, CategoryGroupId, CivilDate, Money, Payee, Rule,
		RuleStage, Schedule,
	};

	use super::*;

	fn budget() -> BudgetData {
		let mut budget = BudgetData::empty(BudgetType::Envelope);
		for (id, off_budget) in [("checking", false), ("brokerage", true)] {
			budget.accounts.insert(
				AccountId::new(id),
				Account {
					id: AccountId::new(id),
					name: id.into(),
					off_budget,
					closed: false,
					group: None,
					sort_order: 0.0,
					tombstone: false,
				},
			);
		}
		for (id, group) in [("groceries", "food"), ("dining", "food"), ("rent", "home")] {
			budget.categories.insert(
				CategoryId::new(id),
				Category {
					id: CategoryId::new(id),
					name: id.into(),
					group: CategoryGroupId::new(group),
					is_income: false,
					hidden: false,
					sort_order: 0.0,
					tombstone: false,
				},
			);
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

	fn add_rule(
		budget: &mut BudgetData,
		id: &str,
		stage: RuleStage,
		op: ConditionsOp,
		conditions: Value,
		actions: Value,
	) {
		budget.rules.insert(
			RuleId::new(id),
			Rule {
				id: RuleId::new(id),
				stage,
				conditions_op: op,
				conditions_json: conditions.to_string(),
				actions_json: actions.to_string(),
				tombstone: false,
			},
		);
	}

	fn transaction(imported_payee: &str) -> RuleTransaction {
		RuleTransaction {
			account: AccountId::new("checking"),
			date: CivilDate::new(2026, 11, 9).unwrap(),
			amount: Money::from_minor(-4210),
			payee: None,
			category: None,
			notes: None,
			imported_payee: Some(imported_payee.into()),
			cleared: Some(false),
			reconciled: Some(false),
			schedule: None,
			deleted: false,
		}
	}

	fn run(budget: &BudgetData, txn: &RuleTransaction) -> RunOutcome {
		RuleBook::new(budget).run(
			txn,
			budget,
			&mut BudgetPayees::new(budget, || PayeeId::new("made")),
		)
	}

	#[test]
	fn set_payee_name_finds_or_makes_the_payee_after_each_rule() {
		// transaction-rules.ts:1210-1227: `payee` becomes the payee by that
		// name once the rule finishes, so the next rule sees the id.
		let mut budget = budget();
		add_rule(
			&mut budget,
			"rename",
			RuleStage::Pre,
			ConditionsOp::And,
			json!([{"op": "contains", "field": "imported_payee", "value": "amzn"}]),
			json!([{"op": "set", "field": "payee_name", "value": "AMAZON"}]),
		);
		add_rule(
			&mut budget,
			"after",
			RuleStage::Default,
			ConditionsOp::And,
			json!([{"op": "is", "field": "payee", "value": "amazon"}, {"op": "is", "field": "payee_name", "value": "amazon"}]),
			json!([{"op": "set", "field": "category", "value": "groceries"}]),
		);
		add_rule(
			&mut budget,
			"new",
			RuleStage::Default,
			ConditionsOp::And,
			json!([{"op": "contains", "field": "imported_payee", "value": "corner"}]),
			json!([{"op": "set", "field": "payee_name", "value": "Corner Store"}]),
		);
		let book = RuleBook::new(&budget);
		let mut payees = BudgetPayees::new(&budget, || PayeeId::new("made"));
		let outcome = book.run(&transaction("AMZN Mktp"), &budget, &mut payees);
		assert_eq!(outcome.transaction.payee, Some(PayeeId::new("amazon")));
		assert_eq!(
			outcome.transaction.category,
			Some(CategoryId::new("groceries"))
		);
		let outcome = book.run(&transaction("Corner shop"), &budget, &mut payees);
		assert_eq!(outcome.transaction.payee, Some(PayeeId::new("made")));
		assert_eq!(
			payees.created,
			[(PayeeId::new("made"), "Corner Store".to_owned())]
		);
		book.run(&transaction("corner again"), &budget, &mut payees);
		assert_eq!(payees.created.len(), 1);
	}

	#[test]
	fn rules_chain_through_stages_and_specificity() {
		// R-RULE-1: the pre rule renames, the default rules see the new payee,
		// and the more specific one runs later and wins.
		let mut budget = budget();
		add_rule(
			&mut budget,
			"rename",
			RuleStage::Pre,
			ConditionsOp::And,
			json!([{"op": "contains", "field": "imported_description", "value": "amzn"}]),
			json!([{"op": "set", "field": "description", "value": "amazon"}]),
		);
		add_rule(
			&mut budget,
			"broad",
			RuleStage::Default,
			ConditionsOp::And,
			json!([{"op": "is", "field": "description", "value": "amazon"}, {"op": "lt", "field": "amount", "value": 0}]),
			json!([{"op": "set", "field": "category", "value": "dining"}]),
		);
		add_rule(
			&mut budget,
			"narrow",
			RuleStage::Default,
			ConditionsOp::And,
			json!([{"op": "is", "field": "description", "value": "amazon"}, {"op": "isapprox", "field": "amount", "value": -4200}]),
			json!([{"op": "set", "field": "category", "value": "groceries"}]),
		);
		add_rule(
			&mut budget,
			"tag",
			RuleStage::Post,
			ConditionsOp::And,
			json!([{"op": "is", "field": "category_group", "value": "food"}]),
			json!([{"op": "append-notes", "value": " #food"}]),
		);
		let outcome = run(&budget, &transaction("AMZN Mktp US"));
		assert_eq!(
			outcome
				.applied
				.iter()
				.map(RuleId::as_str)
				.collect::<Vec<_>>(),
			["rename", "broad", "narrow", "tag"]
		);
		assert_eq!(outcome.transaction.payee, Some(PayeeId::new("amazon")));
		assert_eq!(
			outcome.transaction.category,
			Some(CategoryId::new("groceries"))
		);
		assert_eq!(outcome.transaction.notes.as_deref(), Some(" #food"));
	}

	#[test]
	fn a_rule_without_conditions_never_matches() {
		let mut budget = budget();
		add_rule(
			&mut budget,
			"empty",
			RuleStage::Default,
			ConditionsOp::And,
			json!([]),
			json!([{"op": "set", "field": "notes", "value": "x"}]),
		);
		assert!(run(&budget, &transaction("anything")).applied.is_empty());
	}

	#[test]
	fn the_indexers_prune_is_not_rules_keyed_on_both_fields() {
		// The quirk in docs/actual-format.md §6.1: filed under 'x' and payee
		// "other", this rule is never evaluated for "Amazon" with no payee,
		// though its conditions would match.
		let mut budget = budget();
		let actions = json!([{"op": "set", "field": "notes", "value": "pruned?"}]);
		add_rule(
			&mut budget,
			"both",
			RuleStage::Default,
			ConditionsOp::And,
			json!([{"op": "isNot", "field": "imported_payee", "value": "xyz"}, {"op": "isNot", "field": "payee", "value": "other"}]),
			actions.clone(),
		);
		assert!(run(&budget, &transaction("Amazon")).applied.is_empty());
		assert_eq!(run(&budget, &transaction("Xerox")).applied.len(), 1);
		add_rule(
			&mut budget,
			"one",
			RuleStage::Default,
			ConditionsOp::And,
			json!([{"op": "isNot", "field": "imported_payee", "value": "xyz"}]),
			actions,
		);
		assert_eq!(
			run(&budget, &transaction("Amazon"))
				.applied
				.iter()
				.map(RuleId::as_str)
				.collect::<Vec<_>>(),
			["one"]
		);
	}

	#[test]
	fn unsupported_conditions_only_matter_when_they_decide() {
		let mut budget = budget();
		add_rule(
			&mut budget,
			"regex-and",
			RuleStage::Default,
			ConditionsOp::And,
			json!([{"op": "matches", "field": "notes", "value": "(?=a)"}, {"op": "is", "field": "amount", "value": 1}]),
			json!([{"op": "set", "field": "notes", "value": "x"}]),
		);
		add_rule(
			&mut budget,
			"regex-or",
			RuleStage::Default,
			ConditionsOp::Or,
			json!([{"op": "matches", "field": "notes", "value": "(?=a)"}, {"op": "is", "field": "amount", "value": -4210}]),
			json!([{"op": "set", "field": "cleared", "value": true}]),
		);
		add_rule(
			&mut budget,
			"template",
			RuleStage::Default,
			ConditionsOp::And,
			json!([{"op": "is", "field": "amount", "value": -4210}]),
			json!([{"op": "set", "field": "notes", "value": "", "options": {"template": "{{imported_payee}}"}}]),
		);
		add_rule(
			&mut budget,
			"unknown",
			RuleStage::Default,
			ConditionsOp::And,
			json!([{"op": "matches", "field": "notes", "value": "(?=a)"}]),
			json!([{"op": "set", "field": "cleared", "value": true}]),
		);
		let outcome = run(&budget, &transaction("Shop"));
		assert_eq!(
			outcome
				.applied
				.iter()
				.map(RuleId::as_str)
				.collect::<Vec<_>>(),
			["regex-or"]
		);
		let mut undetermined: Vec<&str> = outcome.undetermined.iter().map(RuleId::as_str).collect();
		undetermined.sort();
		assert_eq!(undetermined, ["template", "unknown"]);
		assert_eq!(outcome.transaction.cleared, Some(true));
	}

	#[test]
	fn rules_actual_drops_never_run() {
		let mut budget = budget();
		add_rule(
			&mut budget,
			"invalid",
			RuleStage::Default,
			ConditionsOp::And,
			json!([{"op": "oneOf", "field": "notes", "value": ["a"]}, {"op": "is", "field": "amount", "value": -4210}]),
			json!([{"op": "set", "field": "notes", "value": "x"}]),
		);
		let outcome = run(&budget, &transaction("Shop"));
		assert!(outcome.applied.is_empty() && outcome.undetermined.is_empty());
	}

	#[test]
	fn a_schedule_post_runs_its_own_rule_unconditionally_and_skips_other_schedules() {
		let mut budget = budget();
		for (schedule, rule) in [("rent", "rent-rule"), ("gym", "gym-rule")] {
			add_rule(
				&mut budget,
				rule,
				RuleStage::Default,
				ConditionsOp::And,
				json!([{"op": "is", "field": "date", "value": "1999-01-01"}]),
				json!([{"op": "link-schedule", "value": schedule}, {"op": "set", "field": "notes", "value": rule}]),
			);
			budget.schedules.insert(
				ScheduleId::new(schedule),
				Schedule {
					id: ScheduleId::new(schedule),
					name: None,
					rule: RuleId::new(rule),
					posts_transaction: true,
					completed: false,
					next_date: None,
					tombstone: false,
				},
			);
		}
		add_rule(
			&mut budget,
			"everyone",
			RuleStage::Default,
			ConditionsOp::And,
			json!([{"op": "lt", "field": "amount", "value": 0}]),
			json!([{"op": "set", "field": "category", "value": "rent"}]),
		);
		let mut txn = transaction("");
		txn.schedule = Some(ScheduleId::new("rent"));
		let outcome = run(&budget, &txn);
		let mut applied: Vec<&str> = outcome.applied.iter().map(RuleId::as_str).collect();
		applied.sort();
		assert_eq!(applied, ["everyone", "rent-rule"]);
		assert_eq!(outcome.transaction.notes.as_deref(), Some("rent-rule"));
	}

	#[test]
	fn merged_payees_and_categories_are_followed_once() {
		let mut budget = budget();
		budget
			.payee_mapping
			.insert(PayeeId::new("old-amazon"), PayeeId::new("amazon"));
		budget
			.category_mapping
			.insert(CategoryId::new("food"), CategoryId::new("groceries"));
		add_rule(
			&mut budget,
			"merged",
			RuleStage::Default,
			ConditionsOp::And,
			json!([{"op": "is", "field": "description", "value": "old-amazon"}]),
			json!([{"op": "set", "field": "category", "value": "food"}]),
		);
		let mut txn = transaction("");
		txn.payee = Some(PayeeId::new("amazon"));
		let outcome = run(&budget, &txn);
		assert_eq!(
			outcome.transaction.category,
			Some(CategoryId::new("groceries"))
		);
	}

	#[test]
	fn derived_fields_follow_actual() {
		// payee_name reads the starting payee; onBudget reads the starting
		// account even after a rule moves the transaction.
		let mut budget = budget();
		add_rule(
			&mut budget,
			"move",
			RuleStage::Pre,
			ConditionsOp::And,
			json!([{"op": "is", "field": "payee_name", "value": "AMAZON"}]),
			json!([{"op": "set", "field": "account", "value": "brokerage"}]),
		);
		add_rule(
			&mut budget,
			"on-budget",
			RuleStage::Default,
			ConditionsOp::And,
			json!([{"op": "onBudget", "field": "account", "value": null}]),
			json!([{"op": "set", "field": "notes", "value": "still on budget"}]),
		);
		add_rule(
			&mut budget,
			"transfer",
			RuleStage::Default,
			ConditionsOp::And,
			json!([{"op": "is", "field": "transfer", "value": false}]),
			json!([{"op": "set", "field": "cleared", "value": true}]),
		);
		let mut txn = transaction("");
		txn.payee = Some(PayeeId::new("amazon"));
		let outcome = run(&budget, &txn);
		assert_eq!(outcome.transaction.account, AccountId::new("brokerage"));
		assert_eq!(
			outcome.transaction.notes.as_deref(),
			Some("still on budget")
		);
		assert_eq!(outcome.transaction.cleared, Some(false));
	}
}
