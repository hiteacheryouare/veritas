//! `runRules` (reference/actual/packages/loot-core/src/server/transactions/transaction-rules.ts:323-449):
//! candidate rules from the two indexers, ranked, run in a chain where each
//! rule sees the changes of the ones before it.

use std::collections::{BTreeMap, BTreeSet};

use veritas_domain::{BudgetData, CategoryId, ConditionsOp, Money, PayeeId, RuleId, ScheduleId};

use crate::action::{Action, RuleTransaction, SetField, SplitLine, SplitMethod};
use crate::condition::{EvalError, Field, FieldValue, Gap};
use crate::js;
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
	/// The transaction with every rule applied, ready to write. `None` when
	/// any rule is undetermined: Veritas then doesn't know what Actual would
	/// write, so nothing from this run may be written. Leave the transaction
	/// for Actual and show the "may need Actual" note instead.
	pub transaction: Option<RuleTransaction>,
	/// What the rules Veritas could run make of the transaction. It is for
	/// showing, never for writing: with an undetermined rule it is only part
	/// of Actual's result.
	pub partial: RuleTransaction,
	/// Rules whose actions were applied, in the order they ran.
	pub applied: Vec<RuleId>,
	/// Rules Actual would have evaluated or applied here but Veritas can't
	/// (a template action, a regex it can't read the JS way). Their effect
	/// is missing.
	pub undetermined: Vec<RuleId>,
	/// Why each undetermined rule couldn't run, in words for the person
	/// looking at the transaction.
	pub reasons: BTreeMap<RuleId, String>,
}

/// Where `set payee_name` finds or makes its payee. `None` when more than
/// one live payee has the name: Actual's `getPayeeByName` has no `ORDER BY`,
/// so which one it takes depends on SQLite's row order, and the rule is
/// reported as undetermined rather than guessed.
pub trait PayeeDirectory {
	fn find_or_create(&mut self, name: &str) -> Option<PayeeId>;
	/// How many payees the directory has made so far.
	fn made(&self) -> usize;
	/// Forgets the payees made after the first `count`. A run that ends
	/// undetermined writes nothing, its payees included (D60).
	fn forget_made_after(&mut self, count: usize);
}

/// Finds payees in the budget the way `getPayeeByName` does (a live payee
/// whose lowercased name matches, `db/index.ts:723-728`) and records the ones
/// a rule creates. The store inserts each like `insertPayee` (`:576-586`): a
/// `payees` row with just the name, plus its identity `payee_mapping` row.
/// Names are compared as stored: a transfer payee's stored name is empty.
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
	fn find_or_create(&mut self, name: &str) -> Option<PayeeId> {
		let wanted = name.to_lowercase();
		let mut matches = self
			.budget
			.payees
			.values()
			.filter(|payee| !payee.tombstone && payee.name.to_lowercase() == wanted)
			.map(|payee| payee.id.clone())
			.chain(
				self.created
					.iter()
					.filter(|(_, created)| created.to_lowercase() == wanted)
					.map(|(id, _)| id.clone()),
			);
		match (matches.next(), matches.next()) {
			(Some(id), None) => Some(id),
			(Some(_), Some(_)) => None,
			(None, _) => {
				let id = (self.new_id)();
				self.created.push((id.clone(), name.to_owned()));
				Some(id)
			}
		}
	}

	fn made(&self) -> usize {
		self.created.len()
	}

	fn forget_made_after(&mut self, count: usize) {
		self.created.truncate(count);
	}
}

/// Three-valued outcome of a rule's conditions: a condition Veritas can't
/// evaluate only matters when the others don't settle the answer.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Verdict {
	Match,
	NoMatch,
	Unknown(String),
}

/// A gap's text is already a noun phrase ("a template action"), which the
/// "may need Actual" note lists as is.
fn gap_reason(gap: &Gap) -> String {
	match gap {
		Gap::Unsupported(what) | Gap::RejectedByActual(what) => what.clone(),
	}
}

fn eval_reason(error: &EvalError) -> String {
	match error {
		EvalError::Recurrence(_) => {
			"a repeating date Veritas can't expand the way Actual does".into()
		}
		other => other.to_string(),
	}
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
		let made_before = payees.made();
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
		let mut outcome = RunOutcome {
			transaction: None,
			partial: transaction.clone(),
			applied: Vec::new(),
			undetermined: Vec::new(),
			reasons: BTreeMap::new(),
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
				Verdict::Unknown(reason) => {
					outcome.undetermined.push(rule.id.clone());
					outcome.reasons.insert(rule.id.clone(), reason);
				}
				Verdict::Match => {
					let actions: Result<Vec<&Action>, String> = rule
						.actions
						.iter()
						.map(|item| item.parsed.as_ref().map_err(gap_reason))
						.collect();
					let result = actions.and_then(|actions| {
						let mut next = working.clone();
						let mut next_name = payee_name.clone();
						let mut new_payee = false;
						for action in actions.iter().filter(|action| action.split() == 0) {
							match action {
								Action::SetPayeeName(name) => {
									next_name = name.clone();
									new_payee = true;
								}
								Action::Set(SetField::Payee(_)) => {
									action.apply(&mut next);
									new_payee = false;
								}
								other => other.apply(&mut next),
							}
						}
						let lines: Vec<&Action> = actions
							.iter()
							.copied()
							.filter(|action| action.split() > 0)
							.collect();
						if !lines.is_empty() && !next.is_child {
							// A payee still waiting on its name would be copied into
							// the new lines unresolved, which Actual then stores.
							if new_payee {
								return Err("a split while naming a new payee, which Actual stores wrongly".into());
							}
							split(&mut next, &lines).ok_or_else(|| {
								"split lines numbered past the end, which make Actual stop with an error".to_string()
							})?;
						}
						if new_payee {
							next.payee = match next_name.as_deref().filter(|name| !name.is_empty())
							{
								Some(name) => Some(payees.find_or_create(name).ok_or_else(|| {
									format!(
										"a payee name, {name}, that more than one payee shares, so Actual's pick depends on how its database orders them"
									)
								})?),
								None => None,
							};
						}
						Ok((next, next_name))
					});
					match result {
						Ok((next, name)) => {
							working = next;
							payee_name = name;
							outcome.applied.push(rule.id.clone());
						}
						Err(reason) => {
							outcome.undetermined.push(rule.id.clone());
							outcome.reasons.insert(rule.id.clone(), reason);
						}
					}
				}
			}
		}
		outcome.transaction = outcome.undetermined.is_empty().then(|| working.clone());
		if outcome.transaction.is_none() {
			payees.forget_made_after(made_before);
		}
		outcome.partial = working;
		outcome
	}
}

/// `execSplitActions` (`rule.ts:26-101`). A transaction that isn't split yet
/// becomes a parent with no payee and one line copying its payee and
/// category; each action adds at most one line (copying the last line's
/// payee) when its number is past the end. Fixed amounts apply first, then
/// percentages of what they leave, then equal remainders, with the last
/// remainder line taking the rounding. The first line is a scratch row that
/// is dropped at the end, even on a transaction that was already split, as
/// in Actual. `None` is a rule Actual would crash on (a line number that
/// skips ahead).
fn split(parent: &mut RuleTransaction, actions: &[&Action]) -> Option<()> {
	let line = |payee: Option<PayeeId>, category: Option<CategoryId>| SplitLine {
		amount: Money::ZERO,
		payee,
		category,
		notes: None,
	};
	let mut lines = if parent.splits.is_empty() {
		let first = line(parent.payee.take(), parent.category.clone());
		vec![first]
	} else {
		parent.splits.clone()
	};
	for action in actions {
		let index = usize::try_from(action.split()).ok()?;
		if index >= lines.len() {
			let payee = lines
				.last()
				.and_then(|last| last.payee.clone())
				.or_else(|| parent.payee.clone());
			lines.push(line(payee, parent.category.clone()));
		}
		action.apply_to_line(lines.get_mut(index)?);
	}
	let left = |lines: &[SplitLine]| {
		parent.amount.minor() - lines.iter().map(|l| l.amount.minor()).sum::<i64>()
	};
	let after_fixed = left(&lines) as f64;
	let mut remainders = Vec::new();
	for action in actions {
		let index = usize::try_from(action.split()).ok()?;
		match action {
			Action::SplitAmount {
				method: SplitMethod::FixedPercent(percent),
				..
			} => {
				lines[index].amount =
					Money::from_minor(js::math_round(after_fixed * (percent / 100.0)) as i64);
			}
			Action::SplitAmount {
				method: SplitMethod::Remainder,
				..
			} => remainders.push(index),
			_ => {}
		}
	}
	if let Some(last) = remainders.iter().copied().max() {
		let share =
			Money::from_minor(js::math_round(left(&lines) as f64 / remainders.len() as f64) as i64);
		for index in &remainders {
			lines[*index].amount = share;
		}
		lines[last].amount = Money::from_minor(lines[last].amount.minor() + left(&lines));
	}
	lines.remove(0);
	parent.splits = lines;
	Some(())
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
	let mut unknown: Option<String> = None;
	for item in &rule.conditions {
		let result = match &item.parsed {
			Ok(condition) => condition
				.eval(
					field_value(condition.field, transaction, payee_name, budget),
					account_off_budget,
				)
				.map_err(|error| eval_reason(&error)),
			Err(gap) => Err(gap_reason(gap)),
		};
		match result {
			Ok(true) if any => return Verdict::Match,
			Ok(false) if !any => return Verdict::NoMatch,
			Err(reason) => {
				unknown.get_or_insert(reason);
			}
			_ => {}
		}
	}
	match (unknown, any) {
		(Some(reason), _) => Verdict::Unknown(reason),
		(None, true) => Verdict::NoMatch,
		(None, false) => Verdict::Match,
	}
}

#[cfg(test)]
mod tests {
	use serde_json::{Value, json};
	use veritas_domain::{
		Account, AccountId, BudgetType, Category, CategoryGroupId, CivilDate, Payee, Rule,
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
			splits: Vec::new(),
			is_child: false,
		}
	}

	fn run(budget: &BudgetData, txn: &RuleTransaction) -> RunOutcome {
		RuleBook::new(budget).run(
			txn,
			budget,
			&mut BudgetPayees::new(budget, || PayeeId::new("made")),
		)
	}

	fn split_rule(budget: &mut BudgetData, id: &str, stage: RuleStage, actions: Value) {
		add_rule(
			budget,
			id,
			stage,
			ConditionsOp::And,
			json!([{"op": "contains", "field": "imported_payee", "value": "split"}]),
			actions,
		);
	}

	fn line(split: u32, method: &str, value: Value) -> Value {
		json!({"op": "set-split-amount", "value": value, "options": {"splitIndex": split, "method": method}})
	}

	fn amounts(outcome: &RunOutcome) -> Vec<i64> {
		outcome
			.partial
			.splits
			.iter()
			.map(|l| l.amount.minor())
			.collect()
	}

	#[test]
	fn splits_apply_fixed_then_percent_then_remainder() {
		// rule.ts:26-101 on -4210: 1000 fixed, 10% of the -5210 left, then two
		// equal remainders with the last one taking the odd cent.
		let mut budget = budget();
		split_rule(
			&mut budget,
			"split",
			RuleStage::Default,
			json!([
				line(1, "fixed-amount", json!(1000)),
				line(2, "fixed-percent", json!(10)),
				line(3, "remainder", Value::Null),
				{"op": "set", "field": "category", "value": "rent", "options": {"splitIndex": 3}},
				line(4, "remainder", Value::Null),
			]),
		);
		let mut txn = transaction("split me");
		txn.payee = Some(PayeeId::new("amazon"));
		let outcome = run(&budget, &txn);
		assert_eq!(amounts(&outcome), [1000, -521, -2344, -2345]);
		assert_eq!(outcome.partial.split_difference(), None);
		assert_eq!(
			outcome.partial.payee, None,
			"the parent loses its payee to the lines"
		);
		assert_eq!(
			outcome.partial.splits[0].payee,
			Some(PayeeId::new("amazon"))
		);
		assert_eq!(
			outcome.partial.splits[2].category,
			Some(CategoryId::new("rent"))
		);
	}

	#[test]
	fn a_second_split_rule_drops_the_first_line_as_actual_does() {
		let mut budget = budget();
		split_rule(
			&mut budget,
			"a",
			RuleStage::Default,
			json!([
				line(1, "fixed-percent", json!(50)),
				line(2, "remainder", Value::Null)
			]),
		);
		split_rule(
			&mut budget,
			"b",
			RuleStage::Post,
			json!([line(1, "fixed-amount", json!(-100))]),
		);
		let outcome = run(&budget, &transaction("split me"));
		assert_eq!(amounts(&outcome), [-100]);
		assert_eq!(
			outcome.partial.split_difference().map(Money::minor),
			Some(-4110)
		);
	}

	#[test]
	fn splits_actual_would_crash_on_or_store_wrong_are_undetermined() {
		let mut budget = budget();
		// Line 2 before line 1 exists: Actual throws out of runRules.
		split_rule(
			&mut budget,
			"skips",
			RuleStage::Default,
			json!([line(2, "remainder", Value::Null)]),
		);
		let outcome = run(&budget, &transaction("split me"));
		assert_eq!(
			outcome
				.undetermined
				.iter()
				.map(RuleId::as_str)
				.collect::<Vec<_>>(),
			["skips"]
		);
		assert!(outcome.partial.splits.is_empty());
		assert!(outcome.reasons[&RuleId::new("skips")].contains("past the end"));

		let mut budget = self::budget();
		split_rule(
			&mut budget,
			"named",
			RuleStage::Default,
			json!([
				{"op": "set", "field": "payee_name", "value": "Corner"},
				line(1, "remainder", Value::Null),
			]),
		);
		let named = run(&budget, &transaction("split me"));
		assert_eq!(named.undetermined.len(), 1);
		assert!(named.reasons[&RuleId::new("named")].contains("new payee"));
	}

	#[test]
	fn a_child_run_on_its_own_takes_only_the_plain_actions() {
		let mut budget = budget();
		split_rule(
			&mut budget,
			"split",
			RuleStage::Default,
			json!([
				{"op": "set", "field": "notes", "value": "seen"},
				line(1, "remainder", Value::Null),
			]),
		);
		let mut child = transaction("split me");
		child.is_child = true;
		let outcome = run(&budget, &child);
		assert_eq!(outcome.partial.notes.as_deref(), Some("seen"));
		assert!(outcome.partial.splits.is_empty());
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
		assert_eq!(outcome.partial.payee, Some(PayeeId::new("amazon")));
		assert_eq!(outcome.partial.category, Some(CategoryId::new("groceries")));
		let outcome = book.run(&transaction("Corner shop"), &budget, &mut payees);
		assert_eq!(outcome.partial.payee, Some(PayeeId::new("made")));
		assert_eq!(
			payees.created,
			[(PayeeId::new("made"), "Corner Store".to_owned())]
		);
		book.run(&transaction("corner again"), &budget, &mut payees);
		assert_eq!(payees.created.len(), 1);
	}

	#[test]
	fn an_undetermined_run_makes_no_payees() {
		// D60: the run writes nothing, so the payee its rename made goes too.
		let mut budget = budget();
		add_rule(
			&mut budget,
			"rename",
			RuleStage::Pre,
			ConditionsOp::And,
			json!([{"op": "contains", "field": "imported_payee", "value": "corner"}]),
			json!([{"op": "set", "field": "payee_name", "value": "Corner Store"}]),
		);
		add_rule(
			&mut budget,
			"template",
			RuleStage::Post,
			ConditionsOp::And,
			json!([{"op": "contains", "field": "imported_payee", "value": "late"}]),
			json!([{"op": "set", "field": "notes", "value": "", "options": {"template": "{{date}}"}}]),
		);
		let book = RuleBook::new(&budget);
		let mut payees = BudgetPayees::new(&budget, || PayeeId::new("made"));
		let kept = book.run(&transaction("Corner shop"), &budget, &mut payees);
		assert!(kept.transaction.is_some());
		assert_eq!(payees.created.len(), 1);
		let dropped = book.run(&transaction("Corner shop, late"), &budget, &mut payees);
		assert_eq!(dropped.undetermined, [RuleId::new("template")]);
		assert_eq!(dropped.transaction, None);
		assert_eq!(payees.created.len(), 1, "the first run's payee stays");

		let mut fresh = BudgetPayees::new(&budget, || PayeeId::new("made"));
		book.run(&transaction("Corner shop, late"), &budget, &mut fresh);
		assert!(
			fresh.created.is_empty(),
			"nothing from an undetermined run is made"
		);
	}

	#[test]
	fn a_name_two_payees_share_is_undetermined() {
		// getPayeeByName has no ORDER BY, so Actual's pick depends on row order.
		let mut budget = budget();
		let mut twin = budget.payees[&PayeeId::new("amazon")].clone();
		twin.id = PayeeId::new("amazon-2");
		twin.name = "AMAZON".into();
		budget.payees.insert(twin.id.clone(), twin);
		add_rule(
			&mut budget,
			"rename",
			RuleStage::Pre,
			ConditionsOp::And,
			json!([{"op": "contains", "field": "imported_payee", "value": "amzn"}]),
			json!([{"op": "set", "field": "payee_name", "value": "amazon"}, {"op": "set", "field": "notes", "value": "renamed"}]),
		);
		let outcome = run(&budget, &transaction("AMZN Mktp"));
		assert_eq!(outcome.undetermined, [RuleId::new("rename")]);
		assert_eq!(
			outcome.transaction, None,
			"nothing from an undetermined run may be written"
		);
		assert_eq!(outcome.partial.payee, None);
		assert_eq!(
			outcome.partial.notes, None,
			"nothing from the rule is applied"
		);
		assert!(
			outcome.reasons[&RuleId::new("rename")]
				.starts_with("a payee name, amazon, that more than one payee shares")
		);
	}

	#[test]
	fn an_emoji_in_the_text_a_regex_reads_says_so() {
		let mut budget = budget();
		add_rule(
			&mut budget,
			"regex",
			RuleStage::Default,
			ConditionsOp::And,
			json!([{"op": "matches", "field": "imported_payee", "value": "^coffee"}]),
			json!([{"op": "set", "field": "notes", "value": "coffee"}]),
		);
		let outcome = run(&budget, &transaction("☕️ Coffee 🍩"));
		assert_eq!(outcome.undetermined, [RuleId::new("regex")]);
		assert!(
			outcome.reasons[&RuleId::new("regex")].contains("emoji"),
			"{:?}",
			outcome.reasons
		);

		let mut budget = self::budget();
		add_rule(
			&mut budget,
			"template",
			RuleStage::Default,
			ConditionsOp::And,
			json!([{"op": "contains", "field": "imported_payee", "value": "x"}]),
			json!([{"op": "set", "field": "notes", "value": "", "options": {"template": "{{date}}"}}]),
		);
		let templated = run(&budget, &transaction("x"));
		assert_eq!(
			templated.reasons[&RuleId::new("template")],
			"a template action"
		);
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
		assert_eq!(outcome.partial.payee, Some(PayeeId::new("amazon")));
		assert_eq!(outcome.partial.category, Some(CategoryId::new("groceries")));
		assert_eq!(outcome.partial.notes.as_deref(), Some(" #food"));
		assert_eq!(
			outcome.transaction.as_ref(),
			Some(&outcome.partial),
			"a fully run outcome is ready to write"
		);
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
		assert_eq!(outcome.partial.cleared, Some(true));
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
		assert_eq!(outcome.partial.notes.as_deref(), Some("rent-rule"));
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
		assert_eq!(outcome.partial.category, Some(CategoryId::new("groceries")));
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
		assert_eq!(outcome.partial.account, AccountId::new("brokerage"));
		assert_eq!(outcome.partial.notes.as_deref(), Some("still on budget"));
		assert_eq!(outcome.partial.cleared, Some(false));
	}
}
