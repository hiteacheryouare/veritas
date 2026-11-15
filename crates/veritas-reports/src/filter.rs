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
		self.passes(transaction.date.naive(), &|condition| {
			condition_matches(condition, &row)
		})
	}

	/// Whether a transaction the balance forecast made up passes, judged in
	/// memory the way `matchesForecastFilters` judges the AQL that
	/// `conditionsToAQL` builds (`forecast-filters.ts:186-299`). That differs
	/// from SQL only on text and names: a missing text never passes `$like`
	/// or `$notlike`, and `%` doesn't reach across a line break.
	pub(crate) fn matches_object(&self, object: &FilterObject) -> Result<bool, CoreError> {
		self.passes(object.date, &|condition| object_matches(condition, object))
	}

	fn passes(
		&self,
		date: NaiveDate,
		condition: &dyn Fn(&Condition) -> Result<bool, CoreError>,
	) -> Result<bool, CoreError> {
		for clause in &self.clauses {
			let passed = clause_matches(clause, date, condition)?;
			if passed == self.any {
				return Ok(passed);
			}
		}
		Ok(!self.any)
	}
}

/// A transaction the balance forecast made up, as its in-memory filter reads
/// it (`ForecastFilterObject`): the account only when the forecast covers it,
/// and the payee, category and group only when they're live.
#[derive(Clone, Debug)]
pub(crate) struct FilterObject {
	pub amount: i64,
	pub date: NaiveDate,
	pub notes: Option<String>,
	pub imported_payee: Option<String>,
	pub cleared: bool,
	pub reconciled: bool,
	pub transfer: bool,
	pub account: Option<ObjectAccount>,
	pub payee: Option<Named>,
	pub category: Option<ObjectCategory>,
}

#[derive(Clone, Debug)]
pub(crate) struct Named {
	pub id: String,
	pub name: String,
}

#[derive(Clone, Debug)]
pub(crate) struct ObjectAccount {
	pub id: String,
	pub name: String,
	pub off_budget: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct ObjectCategory {
	pub id: String,
	pub name: String,
	pub group: Option<Named>,
}

/// What `getValueByPath` finds: a path through a null object is
/// `undefined`, which equals nothing, not even null.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Found<'a> {
	Undefined,
	Null,
	Text(&'a str),
}

impl<'a> Found<'a> {
	fn of(value: Option<&'a str>) -> Found<'a> {
		value.map_or(Found::Null, Found::Text)
	}

	fn equals(self, expected: Option<&str>) -> bool {
		match (self, expected) {
			(Found::Null, None) => true,
			(Found::Text(text), Some(expected)) => text == expected,
			_ => false,
		}
	}

	fn text(self) -> Option<&'a str> {
		match self {
			Found::Text(text) => Some(text),
			_ => None,
		}
	}
}

impl FilterObject {
	fn id(&self, field: Field) -> Found<'_> {
		match field {
			Field::Payee => Found::of(self.payee.as_ref().map(|payee| payee.id.as_str())),
			Field::Category => Found::of(self.category.as_ref().map(|c| c.id.as_str())),
			Field::Account => Found::of(self.account.as_ref().map(|a| a.id.as_str())),
			Field::CategoryGroup => match &self.category {
				None => Found::Undefined,
				Some(category) => Found::of(category.group.as_ref().map(|g| g.id.as_str())),
			},
			_ => Found::Undefined,
		}
	}

	fn name(&self, field: Field) -> Found<'_> {
		fn named(value: Option<&str>) -> Found<'_> {
			value.map_or(Found::Undefined, Found::Text)
		}
		match field {
			Field::Payee => named(self.payee.as_ref().map(|payee| payee.name.as_str())),
			Field::Category => named(self.category.as_ref().map(|c| c.name.as_str())),
			Field::Account => named(self.account.as_ref().map(|a| a.name.as_str())),
			Field::CategoryGroup => named(
				self.category
					.as_ref()
					.and_then(|category| category.group.as_ref())
					.map(|group| group.name.as_str()),
			),
			_ => Found::Undefined,
		}
	}

	fn text(&self, field: Field) -> Option<&str> {
		match field {
			Field::Notes => self.notes.as_deref(),
			Field::ImportedPayee => self.imported_payee.as_deref(),
			_ => None,
		}
	}
}

/// JS's `i`-flag `Canonicalize` outside Unicode mode: the upper case when
/// it is one character and doesn't turn non-ASCII into ASCII.
fn fold(c: char) -> char {
	let mut upper = c.to_uppercase();
	match (upper.next(), upper.next()) {
		(Some(single), None) if c.is_ascii() || !single.is_ascii() => single,
		_ => c,
	}
}

/// `likeToRegex('%' + needle + '%').test(text)`: `%` is `.*`, which stops
/// at a line break, and the rest is literal and case-insensitive.
fn memory_like(needle: &str, text: &str) -> bool {
	let text: Vec<char> = text.chars().collect();
	let tokens = std::iter::once(None)
		.chain(needle.chars().map(|c| (c != '%').then(|| fold(c))))
		.chain(std::iter::once(None));
	let mut reached = vec![false; text.len() + 1];
	reached[0] = true;
	for token in tokens {
		match token {
			None => {
				for j in 1..=text.len() {
					if reached[j - 1]
						&& !matches!(text[j - 1], '\n' | '\r' | '\u{2028}' | '\u{2029}')
					{
						reached[j] = true;
					}
				}
			}
			Some(c) => {
				let mut next = vec![false; text.len() + 1];
				for j in 1..=text.len() {
					next[j] = reached[j - 1] && fold(text[j - 1]) == c;
				}
				reached = next;
			}
		}
	}
	reached[text.len()]
}

fn memory_regex(condition: &Condition, text: Option<&str>) -> Result<bool, CoreError> {
	match text {
		Some(text) => regex_test(condition, Some(text)),
		None => Ok(false),
	}
}

fn object_matches(condition: &Condition, object: &FilterObject) -> Result<bool, CoreError> {
	use ConditionValue as V;
	let field = condition.field;
	let op = condition.op;
	match field.kind() {
		FieldKind::Id => {
			let id = object.id(field);
			Ok(match (op, &condition.value) {
				(Op::Is, V::Id(value)) => id.equals(value.as_deref()),
				(Op::IsNot, V::Id(value)) => !id.equals(value.as_deref()),
				(Op::OneOf, V::IdList(values)) => values.iter().any(|v| id.equals(v.as_deref())),
				(Op::NotOneOf, V::IdList(values)) => {
					!values.is_empty() && values.iter().all(|v| !id.equals(v.as_deref()))
				}
				(Op::Contains, V::Id(Some(needle))) => object
					.name(field)
					.text()
					.is_some_and(|name| memory_like(needle, name)),
				(Op::DoesNotContain, V::Id(Some(needle))) => object
					.name(field)
					.text()
					.is_some_and(|name| !memory_like(needle, name)),
				(Op::Matches, _) => return memory_regex(condition, object.name(field).text()),
				(Op::OnBudget, _) => object.account.as_ref().is_some_and(|a| !a.off_budget),
				(Op::OffBudget, _) => object.account.as_ref().is_some_and(|a| a.off_budget),
				_ => {
					return Err(unsupported(format!("the filter {field:?} {op:?}")));
				}
			})
		}
		FieldKind::Text => {
			let raw = object.text(field);
			let lower = raw.map(str::to_lowercase);
			let column = lower.as_deref();
			Ok(match (op, &condition.value) {
				(Op::Is, V::Text(value)) if value.is_empty() => column.is_none_or(str::is_empty),
				(Op::Is, V::Text(value)) => column == Some(value.as_str()),
				(Op::IsNot, V::Text(value)) => column != Some(value.as_str()),
				(Op::OneOf, V::TextList(values)) => {
					values.iter().any(|value| column == Some(value.as_str()))
				}
				(Op::NotOneOf, V::TextList(values)) => {
					!values.is_empty() && values.iter().all(|value| column != Some(value.as_str()))
				}
				(Op::Contains, V::Text(needle)) => {
					column.is_some_and(|text| memory_like(needle, text))
				}
				(Op::DoesNotContain, V::Text(needle)) => {
					column.is_some_and(|text| !memory_like(needle, text))
				}
				(Op::Matches, _) => return memory_regex(condition, column),
				(Op::HasTags | Op::HasAnyTag, V::Text(value)) => {
					let tags = extract_tags(value);
					let found = |tag: &String| raw.is_some_and(|text| has_tag(text, tag));
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
		FieldKind::Number => number_matches(condition, object.amount),
		FieldKind::Date => date_matches(condition, object.date),
		FieldKind::Boolean => {
			let V::Bool(wanted) = condition.value else {
				return Ok(false);
			};
			let actual = match field {
				Field::Cleared => object.cleared,
				Field::Reconciled => object.reconciled,
				Field::Transfer => object.transfer,
				_ => false,
			};
			Ok(actual == wanted)
		}
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

fn clause_matches(
	clause: &Clause,
	date: NaiveDate,
	condition: &dyn Fn(&Condition) -> Result<bool, CoreError>,
) -> Result<bool, CoreError> {
	match clause {
		Clause::Condition(single) => condition(single),
		Clause::All(conditions) => {
			for single in conditions {
				if !condition(single)? {
					return Ok(false);
				}
			}
			Ok(true)
		}
		Clause::Days { days, approx } => Ok(days.iter().any(|day| {
			if *approx {
				within_two_days(date, *day)
			} else {
				date == *day
			}
		})),
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
		FieldKind::Number => number_matches(condition, row.transaction.amount.minor()),
		FieldKind::Date => date_matches(condition, row.transaction.date.naive()),
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

/// AQL's number filters, which read the same in SQL and in memory.
fn number_matches(condition: &Condition, amount: i64) -> Result<bool, CoreError> {
	use ConditionValue as V;
	let field = condition.field;
	let op = condition.op;
	#[allow(clippy::cast_precision_loss)]
	let amount = amount as f64;
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
		(Op::Is | Op::Gt | Op::Gte | Op::Lt | Op::Lte, V::Number(number)) => signed(op, *number),
		_ => {
			return Err(unsupported(format!("the filter {field:?} {op:?}")));
		}
	})
}

/// AQL's date filters, which compare `yyyy-MM-dd` text the same in SQL and
/// in memory.
fn date_matches(condition: &Condition, date: NaiveDate) -> Result<bool, CoreError> {
	use ConditionValue as V;
	let field = condition.field;
	let op = condition.op;
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

	#[test]
	fn the_forecast_like_is_a_case_blind_regex_that_stops_at_line_breaks() {
		assert!(memory_like("gym", "Iron GYM"));
		assert!(memory_like("a%c", "abc"));
		assert!(!memory_like("a_c", "abc"));
		assert!(!memory_like("b", "a\nb"));
		assert!(memory_like("\nb", "a\nb"));
	}
}
