//! "Apply to existing" (R-RULE-2). The rule editor finds rows with Actual's
//! SQL evaluator, `conditionsToAQL` (transaction-rules.ts:493-753), over
//! `q('transactions')`: live rows that aren't split parents
//! (RuleEditor.tsx:1085-1103, aql/schema/executors.ts:228-247). Its semantics
//! differ from `Condition.eval`. Apply then runs only the rule's actions on
//! the chosen rows, as `applyActions` does (transaction-rules.ts:755-834).

use chrono::{Datelike, Days};
use veritas_domain::{
	BudgetData, CategoryId, ConditionsOp, PayeeId, Rule, SplitRole, Transaction, TransactionId,
};

use crate::action::{Action, RuleTransaction, SetField};
use crate::condition::{
	AmountSign, Condition, ConditionValue, DateValue, Field, FieldKind, Gap, Op, extract_tags,
	has_tag,
};
use crate::js;
use crate::rule::ParsedRule;
use crate::run::PayeeDirectory;

/// One lookup in the union of both mapping tables, as `getMappings` keeps them.
fn mapping(budget: &BudgetData, id: &str) -> Option<String> {
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
		.map(str::to_owned)
}

fn gap_reason(gap: &Gap) -> String {
	match gap {
		Gap::Unsupported(what) | Gap::RejectedByActual(what) => what.clone(),
	}
}

/// A row of `v_transactions_internal_alive`: the payee and category are read
/// through their mappings, and a parent has no category.
struct ViewRow<'a> {
	transaction: &'a Transaction,
	payee: Option<&'a PayeeId>,
	category: Option<&'a CategoryId>,
}

impl<'a> ViewRow<'a> {
	fn of(budget: &'a BudgetData, transaction: &'a Transaction) -> ViewRow<'a> {
		ViewRow {
			transaction,
			payee: transaction
				.payee
				.as_ref()
				.and_then(|payee| budget.resolve_payee(payee)),
			category: match transaction.split {
				SplitRole::Parent => None,
				_ => transaction
					.category
					.as_ref()
					.and_then(|category| budget.resolve_category(category)),
			},
		}
	}

	fn id(&self, budget: &BudgetData, field: Field) -> Option<String> {
		match field {
			Field::Payee => self.payee.map(|id| id.as_str().to_owned()),
			Field::Account => Some(self.transaction.account.as_str().to_owned()),
			Field::Category => self.category.map(|id| id.as_str().to_owned()),
			Field::CategoryGroup => self
				.category
				.and_then(|id| budget.categories.get(id))
				.map(|category| category.group.as_str().to_owned()),
			_ => None,
		}
	}

	/// `<field>.name` through the join: a transfer payee is named after its
	/// account (`v_payees`, aql/schema/index.ts:315-327).
	fn name(&self, budget: &BudgetData, field: Field) -> Option<String> {
		match field {
			Field::Payee => {
				let payee = budget.payees.get(self.payee?)?;
				match &payee.transfer_account {
					Some(account) => budget
						.accounts
						.get(account)
						.filter(|account| !account.tombstone)
						.map(|account| account.name.clone()),
					None => Some(payee.name.clone()),
				}
			}
			Field::Account => budget
				.accounts
				.get(&self.transaction.account)
				.map(|account| account.name.clone()),
			Field::Category => self
				.category
				.and_then(|id| budget.categories.get(id))
				.map(|category| category.name.clone()),
			Field::CategoryGroup => self
				.category
				.and_then(|id| budget.categories.get(id))
				.and_then(|category| budget.category_groups.get(&category.group))
				.map(|group| group.name.clone()),
			_ => None,
		}
	}

	fn text(&self, field: Field) -> Option<&'a str> {
		match field {
			Field::Notes => self.transaction.notes.as_deref(),
			Field::ImportedPayee => self.transaction.imported_payee.as_deref(),
			_ => None,
		}
	}
}

/// Actual's `getNormalisedString` (`shared/normalisation.ts:9-15`) for the
/// letters Latin text meets: lowercase, a few letters without a
/// decomposition, then accents removed.
fn normalise(text: &str) -> String {
	const PLAIN: &str = "aaaaaaæceeeeiiiiðnooooo÷øuuuuyþy";
	let mut out = String::new();
	for c in text.to_lowercase().chars() {
		match c {
			'ł' => out.push('l'),
			'ø' => out.push('o'),
			'ß' => out.push_str("ss"),
			'œ' => out.push_str("oe"),
			'\u{0300}'..='\u{036F}' => {}
			'\u{E0}'..='\u{FF}' => out.push(PLAIN.chars().nth(c as usize - 0xE0).unwrap_or(c)),
			c => out.push(c),
		}
	}
	out
}

/// `UNICODE_LIKE(%needle%)` (`unicodeLike.ts:7-31`): `%` is any run and `?`
/// one character, on both sides normalised.
fn like(needle: &str, text: &str) -> bool {
	fn glob(pattern: &[char], text: &[char]) -> bool {
		match pattern.split_first() {
			None => true,
			Some(('%', rest)) => {
				(0..=text.len()).any(|skip| glob(rest, text.get(skip..).unwrap_or_default()))
			}
			Some(('?', rest)) => !text.is_empty() && glob(rest, text.get(1..).unwrap_or_default()),
			Some((c, rest)) => {
				text.first() == Some(c) && glob(rest, text.get(1..).unwrap_or_default())
			}
		}
	}
	let pattern: Vec<char> = normalise(&format!("%{needle}%")).chars().collect();
	let text: Vec<char> = normalise(text).chars().collect();
	glob(&pattern, &text)
}

fn regexp(condition: &Condition, text: &str) -> Result<bool, String> {
	match &condition.value {
		ConditionValue::Pattern(pattern) => pattern
			.test(text)
			.map_err(|_| "text a regex reads differently in JavaScript".to_owned()),
		_ => Ok(false),
	}
}

fn compare(op: Op, actual: f64, target: f64) -> bool {
	match op {
		Op::Gt => actual > target,
		Op::Gte => actual >= target,
		Op::Lt => actual < target,
		Op::Lte => actual <= target,
		_ => actual == target,
	}
}

/// `apply` for a number (`:547-562`): inflow needs `amount > 0`, outflow
/// `amount < 0` and compares the negated amount.
fn amount_test(condition: &Condition, row: &ViewRow, op: Op, target: f64) -> bool {
	let amount = row.transaction.amount.minor() as f64;
	match condition.sign {
		AmountSign::Outflow => amount < 0.0 && compare(op, -amount, target),
		AmountSign::Inflow => amount > 0.0 && compare(op, amount, target),
		AmountSign::Any => compare(op, amount, target),
	}
}

fn matches(budget: &BudgetData, condition: &Condition, row: &ViewRow) -> Result<bool, String> {
	let t = row.transaction;
	use ConditionValue as V;
	let field = condition.field;
	Ok(match (field.kind(), condition.op, &condition.value) {
		// `conditionSpecialCases` (`:460-490`), on rows that are never parents here.
		(_, Op::Is, V::Id(None)) if field == Field::Category => {
			row.category.is_none() && t.transfer.is_none()
		}
		(_, Op::OnBudget, _) => budget
			.accounts
			.get(&t.account)
			.is_some_and(|a| !a.off_budget),
		(_, Op::OffBudget, _) => budget
			.accounts
			.get(&t.account)
			.is_some_and(|a| a.off_budget),
		(FieldKind::Boolean, Op::Is, V::Bool(flag)) => match field {
			Field::Cleared => t.cleared == *flag,
			Field::Reconciled => t.reconciled == *flag,
			Field::Transfer => t.transfer.is_some() == *flag,
			_ => !*flag,
		},
		(FieldKind::Id, Op::Is, V::Id(id)) => row.id(budget, field) == *id,
		(FieldKind::Id, Op::IsNot, V::Id(None)) => row.id(budget, field).is_some(),
		(FieldKind::Id, Op::IsNot, V::Id(Some(id))) => row.id(budget, field).as_ref() != Some(id),
		(FieldKind::Id, Op::OneOf, V::IdList(ids)) => {
			let actual = row.id(budget, field);
			ids.contains(&actual)
		}
		(FieldKind::Id, Op::NotOneOf, V::IdList(ids)) => {
			let actual = row.id(budget, field);
			!ids.is_empty()
				&& ids.iter().all(|id| match id {
					None => actual.is_some(),
					Some(id) => actual.as_ref() != Some(id),
				})
		}
		(FieldKind::Id, Op::Contains, V::Id(Some(needle))) => row
			.name(budget, field)
			.is_some_and(|name| like(needle, &name)),
		(FieldKind::Id, Op::DoesNotContain, V::Id(Some(needle))) => row
			.name(budget, field)
			.is_none_or(|name| !like(needle, &name)),
		(FieldKind::Id, Op::Matches, _) => {
			regexp(condition, &row.name(budget, field).unwrap_or_default())?
		}
		(FieldKind::Text, _, _) if field == Field::PayeeName => {
			return Err("a payee name condition, which Actual's query can't run".to_owned());
		}
		(FieldKind::Text, Op::Is, V::Text(value)) => {
			let actual = row.text(field).map(str::to_lowercase);
			match value.as_str() {
				"" => actual.is_none_or(|text| text.is_empty()),
				value => actual.as_deref() == Some(value),
			}
		}
		(FieldKind::Text, Op::IsNot, V::Text(value)) => row
			.text(field)
			.is_none_or(|text| text.to_lowercase() != *value),
		(FieldKind::Text, Op::OneOf, V::TextList(values)) => {
			let actual = row.text(field).map(str::to_lowercase);
			values.iter().any(|value| actual.as_ref() == Some(value))
		}
		(FieldKind::Text, Op::NotOneOf, V::TextList(values)) => {
			let actual = row.text(field).map(str::to_lowercase);
			!values.is_empty() && values.iter().all(|value| actual.as_ref() != Some(value))
		}
		(FieldKind::Text, Op::Contains, V::Text(needle)) => row
			.text(field)
			.is_some_and(|text| like(needle, &text.to_lowercase())),
		(FieldKind::Text, Op::DoesNotContain, V::Text(needle)) => row
			.text(field)
			.is_none_or(|text| !like(needle, &text.to_lowercase())),
		(FieldKind::Text, Op::Matches, _) => regexp(
			condition,
			&row.text(field).unwrap_or_default().to_lowercase(),
		)?,
		// `makeTagAqlRegex` (`shared/tags.ts:50-58`) is case-sensitive, and no tags match nothing.
		(FieldKind::Text, Op::HasTags | Op::HasAnyTag, V::Text(value)) => {
			let tags = extract_tags(value);
			let text = row.text(field).unwrap_or_default();
			let found = |tag: &String| has_tag(text, tag);
			!tags.is_empty()
				&& if condition.op == Op::HasTags {
					tags.iter().all(found)
				} else {
					tags.iter().any(found)
				}
		}
		(FieldKind::Number, Op::IsApprox, V::Number(target)) => {
			let threshold = js::math_round(target.abs() * 0.075);
			amount_test(condition, row, Op::Gte, target - threshold)
				&& amount_test(condition, row, Op::Lte, target + threshold)
		}
		(FieldKind::Number, op, V::Number(target)) => amount_test(condition, row, op, *target),
		(FieldKind::Number, Op::IsBetween, V::Between(num1, num2)) => {
			let amount = t.amount.minor() as f64;
			amount >= num1.min(*num2) && amount <= num1.max(*num2)
		}
		(FieldKind::Date, _, V::Date(DateValue::Recur(_))) => {
			return Err("a repeating date, which Veritas can't search for yet".to_owned());
		}
		(FieldKind::Date, op, V::Date(date)) => {
			let day = t.date.naive();
			match (op, date) {
				(Op::IsApprox, DateValue::Day(target)) => {
					let low = target.checked_sub_days(Days::new(2)).unwrap_or(*target);
					let high = target.checked_add_days(Days::new(2)).unwrap_or(*target);
					day >= low && day <= high
				}
				(Op::Is, DateValue::Day(target)) => day == *target,
				(Op::Is, DateValue::Month(month)) => {
					day.year() == month.year() && day.month() == month.month()
				}
				(Op::Is, DateValue::Year(year)) => day.year() == *year,
				(Op::Gt, DateValue::Day(target)) => day > *target,
				(Op::Gte, DateValue::Day(target)) => day >= *target,
				(Op::Lt, DateValue::Day(target)) => day < *target,
				(Op::Lte, DateValue::Day(target)) => day <= *target,
				_ => false,
			}
		}
		_ => false,
	})
}

/// The live rows that aren't split parents and that the rule's conditions
/// find, newest first. A condition Actual can't build is dropped, as
/// `conditionsToAQL` drops it, and with none left nothing is found. A rule
/// with split actions skips split lines (`is_child: false`). `Err` names
/// what Veritas can't search for.
pub fn matching(budget: &BudgetData, rule: &Rule) -> Result<Vec<TransactionId>, String> {
	let mut parsed = ParsedRule::parse(rule);
	if let Some(reason) = &parsed.unreadable {
		return Err(reason.clone());
	}
	parsed.migrate_ids(&|id| mapping(budget, id));
	let mut conditions = Vec::new();
	for item in &parsed.conditions {
		match &item.parsed {
			Ok(condition) => conditions.push(condition),
			Err(Gap::RejectedByActual(_)) => {}
			Err(gap) => return Err(gap_reason(gap)),
		}
	}
	if conditions.is_empty() {
		return Ok(Vec::new());
	}
	let splits = parsed
		.actions
		.iter()
		.any(|item| item.parsed.as_ref().is_ok_and(|action| action.split() > 0));
	let any = rule.conditions_op == ConditionsOp::Or;
	let mut found = Vec::new();
	for transaction in budget.transactions.values() {
		let alive = !transaction.tombstone
			&& match &transaction.split {
				SplitRole::Parent => false,
				SplitRole::Child { parent } => {
					!splits
						&& budget
							.transactions
							.get(parent)
							.is_some_and(|p| !p.tombstone)
				}
				SplitRole::None => true,
			};
		if !alive {
			continue;
		}
		let row = ViewRow::of(budget, transaction);
		let mut verdict = !any;
		for condition in &conditions {
			let hit = matches(budget, condition, &row)?;
			if hit == any {
				verdict = any;
				break;
			}
		}
		if verdict {
			found.push(transaction);
		}
	}
	found.sort_by(|a, b| {
		b.date
			.cmp(&a.date)
			.then(b.sort_order.total_cmp(&a.sort_order))
			.then_with(|| a.id.cmp(&b.id))
	});
	Ok(found.into_iter().map(|t| t.id.clone()).collect())
}

/// One rule's actions, parsed with merged ids mapped, for `applyActions`.
#[derive(Clone, Debug)]
pub struct RuleActions {
	actions: Vec<Action>,
}

impl RuleActions {
	/// `Err` says why Veritas can't run them: an action it doesn't support,
	/// or a split, which Veritas doesn't apply to existing rows yet.
	pub fn new(budget: &BudgetData, rule: &Rule) -> Result<RuleActions, String> {
		let mut parsed = ParsedRule::parse(rule);
		if let Some(reason) = &parsed.unreadable {
			return Err(reason.clone());
		}
		parsed.migrate_ids(&|id| mapping(budget, id));
		let actions = parsed
			.actions
			.into_iter()
			.map(|item| item.parsed.map_err(|gap| gap_reason(&gap)))
			.collect::<Result<Vec<Action>, String>>()?;
		if actions.iter().any(|action| action.split() > 0) {
			return Err("a split, which Veritas can't apply to existing transactions yet".into());
		}
		Ok(RuleActions { actions })
	}

	/// `execActions` then `finalizeTransactionForRules`: every action
	/// overwrites its field, and a payee named by `set payee_name` is found
	/// or made once they have all run. `Err` is a name two payees share.
	pub fn apply(
		&self,
		transaction: &mut RuleTransaction,
		payees: &mut dyn PayeeDirectory,
	) -> Result<(), String> {
		let mut name: Option<Option<String>> = None;
		for action in &self.actions {
			match action {
				Action::SetPayeeName(next) => name = Some(next.clone()),
				Action::Set(SetField::Payee(_)) => {
					action.apply(transaction);
					name = None;
				}
				other => other.apply(transaction),
			}
		}
		if let Some(name) = name {
			transaction.payee = match name.filter(|name| !name.is_empty()) {
				Some(name) => Some(payees.find_or_create(&name).ok_or_else(|| {
					format!("a payee name, {name}, that more than one payee shares")
				})?),
				None => None,
			};
		}
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use serde_json::{Value, json};
	use veritas_domain::{
		Account, AccountId, BudgetType, CivilDate, Money, Payee, RuleId, RuleStage,
	};

	use super::*;
	use crate::run::BudgetPayees;

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
		for (id, name) in [("amazon", "Amazon Café"), ("old", "Old")] {
			budget.payees.insert(
				PayeeId::new(id),
				Payee {
					id: PayeeId::new(id),
					name: name.into(),
					transfer_account: None,
					favorite: false,
					learn_categories: true,
					tombstone: false,
				},
			);
		}
		budget
			.payee_mapping
			.insert(PayeeId::new("amazon"), PayeeId::new("amazon"));
		budget
			.payee_mapping
			.insert(PayeeId::new("old"), PayeeId::new("amazon"));
		for (id, payee, amount, notes, split) in [
			("t1", Some("amazon"), -500, None, SplitRole::None),
			("t2", Some("old"), 700, Some("Lunch #Food"), SplitRole::None),
			("t3", None, -100, Some("lunch"), SplitRole::Parent),
			(
				"t4",
				None,
				-100,
				Some("lunch"),
				SplitRole::Child {
					parent: TransactionId::new("t3"),
				},
			),
		] {
			budget.transactions.insert(
				TransactionId::new(id),
				Transaction {
					id: TransactionId::new(id),
					account: AccountId::new("checking"),
					date: CivilDate::new(2026, 11, 2).unwrap(),
					amount: Money::from_minor(amount),
					payee: payee.map(PayeeId::new),
					imported_payee: None,
					category: None,
					notes: notes.map(str::to_owned),
					cleared: false,
					reconciled: false,
					split,
					transfer: None,
					schedule: None,
					starting_balance: false,
					sort_order: 0.0,
					tombstone: false,
				},
			);
		}
		budget
	}

	fn found(conditions: Value) -> Vec<String> {
		let rule = Rule {
			id: RuleId::new("r"),
			stage: RuleStage::Default,
			conditions_op: ConditionsOp::And,
			conditions_json: conditions.to_string(),
			actions_json: json!([{"op": "set", "field": "cleared", "value": true}]).to_string(),
			tombstone: false,
		};
		matching(&budget(), &rule)
			.unwrap()
			.iter()
			.map(|id| id.as_str().to_owned())
			.collect()
	}

	#[test]
	fn the_query_reads_names_and_mappings_and_skips_parents() {
		// `payee contains` matches the payee's name here, never in memory.
		assert_eq!(
			found(json!([{"op": "contains", "field": "description", "value": "cafe"}])),
			["t1", "t2"]
		);
		assert_eq!(
			found(json!([{"op": "is", "field": "description", "value": "amazon"}])),
			["t1", "t2"]
		);
		assert_eq!(
			found(json!([{"op": "isNot", "field": "notes", "value": "lunch"}])),
			["t1", "t2"]
		);
		assert_eq!(
			found(json!([{"op": "is", "field": "notes", "value": "LUNCH"}])),
			["t4"]
		);
		assert_eq!(
			found(
				json!([{"op": "gte", "field": "amount", "value": 500, "options": {"outflow": true}}])
			),
			["t1"]
		);
		// Tags are case-sensitive in the query.
		assert!(found(json!([{"op": "hasTags", "field": "notes", "value": "#food"}])).is_empty());
		assert_eq!(
			found(json!([{"op": "hasTags", "field": "notes", "value": "#Food"}])),
			["t2"]
		);
		// A condition Actual can't build is dropped, and none left finds nothing.
		assert!(found(json!([{"op": "oneOf", "field": "notes", "value": ["x"]}])).is_empty());
	}

	#[test]
	fn actions_overwrite_and_name_a_payee_once_they_have_run() {
		let budget = budget();
		let rule = Rule {
			id: RuleId::new("r"),
			stage: RuleStage::Default,
			conditions_op: ConditionsOp::And,
			conditions_json: "[]".into(),
			actions_json: json!([
				{"op": "set", "field": "payee_name", "value": "Brand new"},
				{"op": "set", "field": "notes", "value": "x"},
				{"op": "prepend-notes", "value": "> "}
			])
			.to_string(),
			tombstone: false,
		};
		let actions = RuleActions::new(&budget, &rule).unwrap();
		let mut transaction = RuleTransaction {
			account: AccountId::new("checking"),
			date: CivilDate::new(2026, 11, 2).unwrap(),
			amount: Money::from_minor(-500),
			payee: Some(PayeeId::new("amazon")),
			category: None,
			notes: Some("old".into()),
			imported_payee: None,
			cleared: Some(false),
			reconciled: Some(false),
			schedule: None,
			deleted: false,
			splits: Vec::new(),
			is_child: false,
		};
		let mut payees = BudgetPayees::new(&budget, || PayeeId::new("made"));
		actions.apply(&mut transaction, &mut payees).unwrap();
		assert_eq!(transaction.notes.as_deref(), Some("> x"));
		assert_eq!(transaction.payee, Some(PayeeId::new("made")));
		assert_eq!(
			payees.created,
			[(PayeeId::new("made"), "Brand new".to_owned())]
		);
	}
}
