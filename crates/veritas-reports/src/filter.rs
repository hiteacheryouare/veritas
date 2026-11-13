//! Filter conditions over the `transactions` table the way Actual's reports
//! and widgets run them: `conditionsToAQL` turns each condition into an AQL
//! filter (`server/transactions/transaction-rules.ts:493-753`), and the AQL
//! compiler gives it SQL's null rules (`server/aql/compiler.ts:647-751`).
//! That differs from a rule's in-memory `Condition.eval` in places: an id's
//! `contains` reads the joined name, `isNot` keeps nulls, and tags match the
//! notes as typed.

use chrono::{Datelike, Days, NaiveDate};
use veritas_domain::{BudgetData, CoreError, SplitRole, Transaction};
use veritas_rules::{
	AmountSign, Condition, ConditionValue, DateValue, Field, FieldKind, Gap, Op, Recurrence,
};

use crate::json::Json;
use crate::report_query::{like, normalise, unsupported};

/// How many occurrences of a repeating date Actual expands (`recurDateBounds`).
const RECUR_DATE_BOUNDS: usize = 100;

#[derive(Clone, Debug)]
enum Clause {
	Condition(Condition),
	/// `conditionSpecialCases` for a category that `is` or `isNot` null.
	All(Vec<Condition>),
	/// A repeating date, already expanded to the days Actual lists.
	Days {
		days: Vec<NaiveDate>,
		approx: bool,
	},
}

/// A list of stored conditions joined by `and` or `or`, as a report or a
/// widget keeps them.
#[derive(Clone, Debug, Default)]
pub struct TransactionFilter {
	clauses: Vec<Clause>,
	any: bool,
}

/// What a condition can read of one transaction row: AQL's columns, plus the
/// joins its `.name` and `account.offbudget` paths make.
struct Row<'a> {
	budget: &'a BudgetData,
	transaction: &'a Transaction,
}

impl Row<'_> {
	fn id(&self, field: Field) -> Option<String> {
		let budget = self.budget;
		let transaction = self.transaction;
		match field {
			Field::Payee => transaction
				.payee
				.as_ref()
				.and_then(|id| budget.resolve_payee(id))
				.map(ToString::to_string),
			Field::Category if transaction.split == SplitRole::Parent => None,
			Field::Category => transaction
				.category
				.as_ref()
				.and_then(|id| budget.resolve_category(id))
				.map(ToString::to_string),
			Field::Account => Some(transaction.account.to_string()),
			Field::CategoryGroup => self
				.live_category()
				.map(|category| category.group.to_string()),
			_ => None,
		}
	}

	fn live_category(&self) -> Option<&veritas_domain::Category> {
		if self.transaction.split == SplitRole::Parent {
			return None;
		}
		self.transaction
			.category
			.as_ref()
			.and_then(|id| self.budget.resolve_category(id))
			.and_then(|id| self.budget.categories.get(id))
			.filter(|category| !category.tombstone)
	}

	fn live_account(&self) -> Option<&veritas_domain::Account> {
		self.budget
			.accounts
			.get(&self.transaction.account)
			.filter(|account| !account.tombstone)
	}

	/// The `name` a joined id reads as. A transfer payee is named after its
	/// account (`v_payees`), and one whose account is gone isn't joined.
	fn name(&self, field: Field) -> Option<String> {
		let budget = self.budget;
		match field {
			Field::Payee => {
				let payee = self
					.transaction
					.payee
					.as_ref()
					.and_then(|id| budget.resolve_payee(id))
					.and_then(|id| budget.payees.get(id))
					.filter(|payee| !payee.tombstone)?;
				match &payee.transfer_account {
					None => Some(payee.name.clone()),
					Some(account) => budget
						.accounts
						.get(account)
						.filter(|account| !account.tombstone)
						.map(|account| account.name.clone()),
				}
			}
			Field::Category => self.live_category().map(|category| category.name.clone()),
			Field::Account => self.live_account().map(|account| account.name.clone()),
			Field::CategoryGroup => self
				.live_category()
				.and_then(|category| budget.category_groups.get(&category.group))
				.filter(|group| !group.tombstone)
				.map(|group| group.name.clone()),
			_ => None,
		}
	}

	fn text(&self, field: Field) -> Option<&str> {
		match field {
			Field::Notes => self.transaction.notes.as_deref(),
			Field::ImportedPayee => self.transaction.imported_payee.as_deref(),
			_ => None,
		}
	}
}

fn rejected_or_error(gap: Gap) -> Result<Option<Condition>, CoreError> {
	match gap {
		Gap::RejectedByActual(_) => Ok(None),
		Gap::Unsupported(reason) => Err(unsupported(format!("a filter condition: {reason}"))),
	}
}

fn recur_days(recurrence: &Recurrence) -> Result<Vec<NaiveDate>, CoreError> {
	let fail = |_| unsupported("a repeating date in a filter".to_owned());
	let mut days = Vec::new();
	let mut from = NaiveDate::MIN;
	while days.len() < RECUR_DATE_BOUNDS {
		let Some(day) = recurrence.first_on_or_after(from).map_err(fail)? else {
			break;
		};
		days.push(day);
		let Some(next) = day.succ_opt() else {
			break;
		};
		from = next;
	}
	Ok(days)
}

impl TransactionFilter {
	/// Parses the stored conditions. Conditions Actual's constructor throws
	/// on are dropped, as `conditionsToAQL` drops them; ones with a
	/// `customName` are left out, as every report page leaves them out.
	pub fn new(conditions: &[Json], any: bool) -> Result<TransactionFilter, CoreError> {
		let mut clauses = Vec::new();
		for json in conditions {
			if json
				.get("customName")
				.is_some_and(|name| !matches!(name, Json::Null))
			{
				continue;
			}
			let value: serde_json::Value = serde_json::from_str(&json.stringify())
				.map_err(|error| CoreError::Invalid(format!("a filter condition: {error}")))?;
			let parsed = match Condition::parse(&value) {
				Ok(condition) => Some(condition),
				Err(gap) => rejected_or_error(gap)?,
			};
			let Some(condition) = parsed else {
				continue;
			};
			if condition.field == Field::PayeeName {
				return Err(unsupported(
					"a payee_name condition, which Actual's query can't run".to_owned(),
				));
			}
			clauses.push(special_case(condition)?);
		}
		Ok(TransactionFilter { clauses, any })
	}

	/// True when the filter would let every row through.
	pub fn is_empty(&self) -> bool {
		self.clauses.is_empty()
	}

	/// Whether the row passes. The caller has already decided the row is one
	/// AQL's `transactions` table shows.
	pub fn matches(
		&self,
		budget: &BudgetData,
		transaction: &Transaction,
	) -> Result<bool, CoreError> {
		if self.clauses.is_empty() {
			return Ok(true);
		}
		let row = Row {
			budget,
			transaction,
		};
		for clause in &self.clauses {
			let passed = clause_matches(clause, &row)?;
			if passed == self.any {
				return Ok(passed);
			}
		}
		Ok(!self.any)
	}
}

fn special_case(condition: Condition) -> Result<Clause, CoreError> {
	let flag = |field: Field| Condition {
		field,
		op: Op::Is,
		value: ConditionValue::Bool(false),
		sign: AmountSign::Any,
	};
	let null_category =
		condition.field == Field::Category && condition.value == ConditionValue::Id(None);
	Ok(match condition.op {
		Op::Is if null_category => {
			Clause::All(vec![condition, flag(Field::Transfer), flag(Field::Parent)])
		}
		Op::IsNot if null_category => Clause::All(vec![condition, flag(Field::Parent)]),
		Op::Is | Op::IsApprox => match &condition.value {
			ConditionValue::Date(DateValue::Recur(recurrence)) => Clause::Days {
				days: recur_days(recurrence)?,
				approx: condition.op == Op::IsApprox,
			},
			_ => Clause::Condition(condition),
		},
		_ => Clause::Condition(condition),
	})
}

fn clause_matches(clause: &Clause, row: &Row) -> Result<bool, CoreError> {
	match clause {
		Clause::Condition(condition) => condition_matches(condition, row),
		Clause::All(conditions) => {
			for condition in conditions {
				if !condition_matches(condition, row)? {
					return Ok(false);
				}
			}
			Ok(true)
		}
		Clause::Days { days, approx } => {
			let date = row.transaction.date.naive();
			Ok(days.iter().any(|day| {
				if *approx {
					within_two_days(date, *day)
				} else {
					date == *day
				}
			}))
		}
	}
}

fn within_two_days(date: NaiveDate, target: NaiveDate) -> bool {
	let low = target.checked_sub_days(Days::new(2)).unwrap_or(target);
	let high = target.checked_add_days(Days::new(2)).unwrap_or(target);
	(low..=high).contains(&date)
}

/// JS `Math.round`: halves go up, toward positive infinity.
fn math_round(x: f64) -> f64 {
	let floor = x.floor();
	if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

/// SQL `=` and `!=` between a nullable column and a value: `$eq` with null
/// is `IS NULL`, and `$ne` with a value keeps rows where the column is null.
fn sql_eq(column: Option<&str>, value: Option<&str>) -> bool {
	match value {
		None => column.is_none(),
		Some(value) => column == Some(value),
	}
}

fn sql_ne(column: Option<&str>, value: Option<&str>) -> bool {
	match value {
		None => column.is_some(),
		Some(value) => column != Some(value),
	}
}

/// `UNICODE_LIKE('%needle%', NORMALISE(column))`.
pub(crate) fn contains(needle: &str, column: Option<&str>) -> bool {
	let text = column.map(normalise).unwrap_or_default();
	like(&normalise(&format!("%{needle}%")), &text)
}

fn regex_test(condition: &Condition, column: Option<&str>) -> Result<bool, CoreError> {
	let ConditionValue::Pattern(pattern) = &condition.value else {
		return Ok(false);
	};
	pattern
		.test(column.unwrap_or_default())
		.map_err(|_| unsupported("a regex filter on text JavaScript reads differently".to_owned()))
}

/// JavaScript's `\s`, outside Unicode mode.
fn is_js_space(c: char) -> bool {
	matches!(
		c,
		'\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
			..='\u{200A}'
				| '\u{2028}' | '\u{2029}'
				| '\u{202F}' | '\u{205F}'
				| '\u{3000}' | '\u{FEFF}'
	)
}

/// `extractTagsForFilter` (`shared/tags.ts:7-19`).
fn extract_tags(value: &str) -> Vec<String> {
	let mut tags: Vec<String> = Vec::new();
	for token in value.split(|c: char| c == '#' || is_js_space(c)) {
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

/// `makeTagAqlRegex`'s `(^|[^#])<tag>([\s#]|$)`, case-sensitive on the
/// notes as stored.
fn has_tag(text: &str, tag: &str) -> bool {
	let mut from = 0;
	while let Some(found) = text[from..].find(tag) {
		let at = from + found;
		let free_before = !text[..at].ends_with('#');
		let ends = text[at + tag.len()..]
			.chars()
			.next()
			.is_none_or(|c| c == '#' || is_js_space(c));
		if free_before && ends {
			return true;
		}
		from = at + 1;
	}
	false
}

fn compare(op: Op, left: f64, right: f64) -> bool {
	match op {
		Op::Gt => left > right,
		Op::Gte => left >= right,
		Op::Lt => left < right,
		Op::Lte => left <= right,
		_ => left == right,
	}
}

fn condition_matches(condition: &Condition, row: &Row) -> Result<bool, CoreError> {
	use ConditionValue as V;
	let field = condition.field;
	let op = condition.op;
	match field.kind() {
		FieldKind::Id => {
			let column = row.id(field);
			let column = column.as_deref();
			Ok(match (op, &condition.value) {
				(Op::Is, V::Id(value)) => sql_eq(column, value.as_deref()),
				(Op::IsNot, V::Id(value)) => sql_ne(column, value.as_deref()),
				(Op::OneOf, V::IdList(values)) => {
					values.iter().any(|value| sql_eq(column, value.as_deref()))
				}
				(Op::NotOneOf, V::IdList(values)) => {
					!values.is_empty()
						&& values.iter().all(|value| sql_ne(column, value.as_deref()))
				}
				(Op::Contains, V::Id(Some(needle))) => contains(needle, row.name(field).as_deref()),
				(Op::DoesNotContain, V::Id(Some(needle))) => {
					let name = row.name(field);
					name.is_none() || !contains(needle, name.as_deref())
				}
				(Op::Matches, _) => return regex_test(condition, row.name(field).as_deref()),
				(Op::OnBudget, _) => row
					.live_account()
					.is_some_and(|account| !account.off_budget),
				(Op::OffBudget, _) => row.live_account().is_some_and(|account| account.off_budget),
				_ => {
					return Err(unsupported(format!("the filter {field:?} {op:?}")));
				}
			})
		}
		FieldKind::Text => {
			let raw = row.text(field);
			let lower = raw.map(str::to_lowercase);
			let column = lower.as_deref();
			Ok(match (op, &condition.value) {
				(Op::Is, V::Text(value)) if value.is_empty() => column.is_none_or(str::is_empty),
				(Op::Is, V::Text(value)) => column == Some(value.as_str()),
				(Op::IsNot, V::Text(value)) => sql_ne(column, Some(value)),
				(Op::OneOf, V::TextList(values)) => {
					values.iter().any(|value| column == Some(value.as_str()))
				}
				(Op::NotOneOf, V::TextList(values)) => {
					!values.is_empty() && values.iter().all(|value| sql_ne(column, Some(value)))
				}
				(Op::Contains, V::Text(needle)) => contains(needle, column),
				(Op::DoesNotContain, V::Text(needle)) => {
					column.is_none() || !contains(needle, column)
				}
				(Op::Matches, _) => return regex_test(condition, column),
				(Op::HasTags | Op::HasAnyTag, V::Text(value)) => {
					let tags = extract_tags(value);
					let text = raw.unwrap_or_default();
					let found = |tag: &String| has_tag(text, tag);
					if tags.is_empty() {
						false
					} else if op == Op::HasTags {
						tags.iter().all(found)
					} else {
						tags.iter().any(found)
					}
				}
				_ => {
					return Err(unsupported(format!("the filter {field:?} {op:?}")));
				}
			})
		}
		FieldKind::Number => {
			#[allow(clippy::cast_precision_loss)]
			let amount = row.transaction.amount.minor() as f64;
			let signed = |op: Op, value: f64| match condition.sign {
				AmountSign::Outflow => amount < 0.0 && compare(op, -amount, value),
				AmountSign::Inflow => amount > 0.0 && compare(op, amount, value),
				AmountSign::Any => compare(op, amount, value),
			};
			Ok(match (op, &condition.value) {
				(Op::IsApprox, V::Number(number)) => {
					let threshold = math_round(number.abs() * 0.075);
					signed(Op::Gte, number - threshold) && signed(Op::Lte, number + threshold)
				}
				(Op::IsBetween, V::Between(a, b)) => {
					let (low, high) = if a < b { (*a, *b) } else { (*b, *a) };
					amount >= low && amount <= high
				}
				(Op::Is | Op::Gt | Op::Gte | Op::Lt | Op::Lte, V::Number(number)) => {
					signed(op, *number)
				}
				_ => {
					return Err(unsupported(format!("the filter {field:?} {op:?}")));
				}
			})
		}
		FieldKind::Date => {
			let date = row.transaction.date.naive();
			Ok(match (op, &condition.value) {
				(Op::Is, V::Date(DateValue::Day(day))) => date == *day,
				(Op::Is, V::Date(DateValue::Month(month))) => {
					date.year() == month.year() && date.month() == month.month()
				}
				(Op::Is, V::Date(DateValue::Year(year))) => date.year() == *year,
				(Op::IsApprox, V::Date(DateValue::Day(day))) => within_two_days(date, *day),
				(Op::Gt, V::Date(DateValue::Day(day))) => date > *day,
				(Op::Gte, V::Date(DateValue::Day(day))) => date >= *day,
				(Op::Lt, V::Date(DateValue::Day(day))) => date < *day,
				(Op::Lte, V::Date(DateValue::Day(day))) => date <= *day,
				_ => {
					return Err(unsupported(format!("the filter {field:?} {op:?}")));
				}
			})
		}
		FieldKind::Boolean => {
			let V::Bool(wanted) = condition.value else {
				return Ok(false);
			};
			let transaction = row.transaction;
			let actual = match field {
				Field::Cleared => transaction.cleared,
				Field::Reconciled => transaction.reconciled,
				Field::Transfer => transaction.transfer.is_some(),
				Field::Parent => transaction.split == SplitRole::Parent,
				_ => false,
			};
			Ok(actual == wanted)
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn tags_match_case_sensitively_and_never_inside_a_longer_tag() {
		assert!(has_tag("dinner #food", "#food"));
		assert!(has_tag("#food#trip", "#food"));
		assert!(!has_tag("##food", "#food"));
		assert!(!has_tag("#foods", "#food"));
		assert!(!has_tag("#Food", "#food"));
		assert_eq!(extract_tags("#a ##b #a"), vec!["#a", "#b"]);
	}
}
