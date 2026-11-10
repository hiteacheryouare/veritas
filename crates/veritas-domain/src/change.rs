use std::collections::BTreeMap;
use std::fmt::Debug;

use crate::{
	Account, AccountGroup, AccountGroupId, AccountId, AccountMeta, BudgetCell, BudgetData,
	Category, CategoryGroup, CategoryGroupId, CategoryId, CategoryMeta, Goal, GoalId, Money, Payee,
	PayeeId, Rule, RuleId, Schedule, ScheduleId, Tag, TagId, Transaction, TransactionId,
	TransactionMeta, YearMonth,
};

/// One row a subscriber may need to refetch after a commit.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EntityRef {
	Account(AccountId),
	AccountGroup(AccountGroupId),
	CategoryGroup(CategoryGroupId),
	Category(CategoryId),
	Payee(PayeeId),
	Transaction(TransactionId),
	BudgetCell(YearMonth, CategoryId),
	Hold(YearMonth),
	Rule(RuleId),
	Schedule(ScheduleId),
	Tag(TagId),
	Note(String),
	CategoryMapping(CategoryId),
	PayeeMapping(PayeeId),
	CategoryMeta(CategoryId),
	AccountMeta(AccountId),
	TransactionMeta(TransactionId),
	Goal(GoalId),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hold {
	pub month: YearMonth,
	pub amount: Option<Money>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
	pub id: String,
	pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CategoryMapping {
	pub from: CategoryId,
	pub to: CategoryId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PayeeMapping {
	pub from: PayeeId,
	pub to: PayeeId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CategoryMetaRow {
	pub category: CategoryId,
	pub meta: CategoryMeta,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AccountMetaRow {
	pub account: AccountId,
	pub meta: AccountMeta,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransactionMetaRow {
	pub transaction: TransactionId,
	pub meta: TransactionMeta,
}

/// What undo writes for a row that a command created.
pub enum Retired<T> {
	Becomes(T),
	Removed,
	Kept,
}

/// A row kind the command bus can write: how to find it in memory, put it
/// back, and retire it on undo.
pub trait Record: Clone + PartialEq + Debug + Sized {
	type Key: Clone + PartialEq + Debug;

	fn key(&self) -> Self::Key;
	fn read(data: &BudgetData, key: &Self::Key) -> Option<Self>;
	fn write(data: &mut BudgetData, key: &Self::Key, value: Option<Self>);
	fn retired(created: &Self) -> Retired<Self>;
	fn entity(key: &Self::Key) -> EntityRef;
}

/// One row before and after a command. `before` is always what memory and
/// storage held when the change was made, so a store can update only the
/// columns that differ. `after: None` means the row goes away, which only
/// happens on undo for notes (Actual writes a NULL note) and `veritas_*` rows.
#[derive(Clone, Debug, PartialEq)]
pub struct Delta<T: Record> {
	pub key: T::Key,
	pub before: Option<T>,
	pub after: Option<T>,
}

impl<T: Record> Delta<T> {
	fn against(data: &BudgetData, key: T::Key, after: Option<T>) -> Self {
		let before = T::read(data, &key);
		Delta { key, before, after }
	}

	fn undo(&self, data: &BudgetData) -> Option<Self> {
		let target = match (&self.before, &self.after) {
			(Some(before), _) => Some(before.clone()),
			(None, Some(created)) => match T::retired(created) {
				Retired::Becomes(row) => Some(row),
				Retired::Removed => None,
				Retired::Kept => return None,
			},
			(None, None) => return None,
		};
		Some(Delta::against(data, self.key.clone(), target))
	}

	fn redo(&self, data: &BudgetData) -> Self {
		Delta::against(data, self.key.clone(), self.after.clone())
	}

	fn apply(&self, data: &mut BudgetData) {
		T::write(data, &self.key, self.after.clone());
	}

	fn revert(&self, data: &mut BudgetData) {
		T::write(data, &self.key, self.before.clone());
	}

	fn starts_from(&self, data: &BudgetData) -> bool {
		T::read(data, &self.key) == self.before
	}

	fn is_noop(&self) -> bool {
		self.before == self.after
	}
}

macro_rules! rows {
	($($variant:ident($ty:ty)),* $(,)?) => {
		/// A row a command wants to exist after it runs.
		#[derive(Clone, Debug, PartialEq)]
		pub enum Row {
			$($variant($ty),)*
		}

		/// What a store writes: one row, before and after.
		#[derive(Clone, Debug, PartialEq)]
		pub enum Change {
			$($variant(Delta<$ty>),)*
		}

		impl Change {
			/// The change that makes `row` exist, starting from what `data` holds.
			pub fn put(data: &BudgetData, row: Row) -> Change {
				match row {
					$(Row::$variant(after) => {
						Change::$variant(Delta::against(data, after.key(), Some(after)))
					})*
				}
			}

			/// The change that takes this one back, or `None` when Actual keeps the row.
			pub fn undo(&self, data: &BudgetData) -> Option<Change> {
				match self {
					$(Change::$variant(delta) => delta.undo(data).map(Change::$variant),)*
				}
			}

			pub fn redo(&self, data: &BudgetData) -> Change {
				match self {
					$(Change::$variant(delta) => Change::$variant(delta.redo(data)),)*
				}
			}

			/// Puts `before` back exactly, for rolling back a failed transaction.
			pub fn revert(&self, data: &mut BudgetData) {
				match self {
					$(Change::$variant(delta) => delta.revert(data),)*
				}
			}

			pub fn is_noop(&self) -> bool {
				match self {
					$(Change::$variant(delta) => delta.is_noop(),)*
				}
			}

			pub fn apply(&self, data: &mut BudgetData) {
				match self {
					$(Change::$variant(delta) => delta.apply(data),)*
				}
			}

			/// True when `before` is what `data` holds for this row.
			pub fn starts_from(&self, data: &BudgetData) -> bool {
				match self {
					$(Change::$variant(delta) => delta.starts_from(data),)*
				}
			}

			pub fn entity(&self) -> EntityRef {
				match self {
					$(Change::$variant(delta) => <$ty as Record>::entity(&delta.key),)*
				}
			}
		}
	};
}

rows!(
	Account(Account),
	AccountGroup(AccountGroup),
	CategoryGroup(CategoryGroup),
	Category(Category),
	Payee(Payee),
	Transaction(Transaction),
	Rule(Rule),
	Schedule(Schedule),
	Tag(Tag),
	BudgetCell(BudgetCell),
	Hold(Hold),
	Note(Note),
	CategoryMapping(CategoryMapping),
	PayeeMapping(PayeeMapping),
	CategoryMeta(CategoryMetaRow),
	AccountMeta(AccountMetaRow),
	TransactionMeta(TransactionMetaRow),
	Goal(Goal),
);

fn put<K: Ord + Clone, V>(map: &mut BTreeMap<K, V>, key: &K, value: Option<V>) {
	match value {
		Some(value) => {
			map.insert(key.clone(), value);
		}
		None => {
			map.remove(key);
		}
	}
}

// Undo of a created row follows Actual's undoMessage:
// reference/actual/packages/loot-core/src/server/undo.ts:161-203.
macro_rules! tombstoned {
	($($ty:ident in $field:ident by $key:ty),* $(,)?) => {$(
		impl Record for $ty {
			type Key = $key;

			fn key(&self) -> $key {
				self.id.clone()
			}

			fn read(data: &BudgetData, key: &$key) -> Option<Self> {
				data.$field.get(key).cloned()
			}

			fn write(data: &mut BudgetData, key: &$key, value: Option<Self>) {
				put(&mut data.$field, key, value);
			}

			fn retired(created: &Self) -> Retired<Self> {
				Retired::Becomes($ty {
					tombstone: true,
					..created.clone()
				})
			}

			fn entity(key: &$key) -> EntityRef {
				EntityRef::$ty(key.clone())
			}
		}
	)*};
}

tombstoned!(
	Account in accounts by AccountId,
	AccountGroup in account_groups by AccountGroupId,
	CategoryGroup in category_groups by CategoryGroupId,
	Category in categories by CategoryId,
	Payee in payees by PayeeId,
	Transaction in transactions by TransactionId,
	Rule in rules by RuleId,
	Schedule in schedules by ScheduleId,
	Tag in tags by TagId,
);

impl Record for BudgetCell {
	type Key = (YearMonth, CategoryId);

	fn key(&self) -> Self::Key {
		(self.month, self.category.clone())
	}

	fn read(data: &BudgetData, key: &Self::Key) -> Option<Self> {
		data.budget_cells.get(key).cloned()
	}

	fn write(data: &mut BudgetData, key: &Self::Key, value: Option<Self>) {
		put(&mut data.budget_cells, key, value);
	}

	fn retired(created: &Self) -> Retired<Self> {
		Retired::Becomes(BudgetCell {
			budgeted: Money::ZERO,
			carryover: false,
			..created.clone()
		})
	}

	fn entity(key: &Self::Key) -> EntityRef {
		EntityRef::BudgetCell(key.0, key.1.clone())
	}
}

impl Record for Hold {
	type Key = YearMonth;

	fn key(&self) -> YearMonth {
		self.month
	}

	fn read(data: &BudgetData, key: &YearMonth) -> Option<Self> {
		data.holds.get(key).map(|&amount| Hold {
			month: *key,
			amount,
		})
	}

	fn write(data: &mut BudgetData, key: &YearMonth, value: Option<Self>) {
		put(&mut data.holds, key, value.map(|hold| hold.amount));
	}

	fn retired(created: &Self) -> Retired<Self> {
		Retired::Becomes(Hold {
			month: created.month,
			amount: Some(Money::ZERO),
		})
	}

	fn entity(key: &YearMonth) -> EntityRef {
		EntityRef::Hold(*key)
	}
}

impl Record for Note {
	type Key = String;

	fn key(&self) -> String {
		self.id.clone()
	}

	fn read(data: &BudgetData, key: &String) -> Option<Self> {
		data.notes.get(key).map(|text| Note {
			id: key.clone(),
			text: text.clone(),
		})
	}

	fn write(data: &mut BudgetData, key: &String, value: Option<Self>) {
		put(&mut data.notes, key, value.map(|note| note.text));
	}

	fn retired(_: &Self) -> Retired<Self> {
		Retired::Removed
	}

	fn entity(key: &String) -> EntityRef {
		EntityRef::Note(key.clone())
	}
}

impl Record for CategoryMapping {
	type Key = CategoryId;

	fn key(&self) -> CategoryId {
		self.from.clone()
	}

	fn read(data: &BudgetData, key: &CategoryId) -> Option<Self> {
		data.category_mapping.get(key).map(|to| CategoryMapping {
			from: key.clone(),
			to: to.clone(),
		})
	}

	fn write(data: &mut BudgetData, key: &CategoryId, value: Option<Self>) {
		put(&mut data.category_mapping, key, value.map(|row| row.to));
	}

	fn retired(_: &Self) -> Retired<Self> {
		Retired::Kept
	}

	fn entity(key: &CategoryId) -> EntityRef {
		EntityRef::CategoryMapping(key.clone())
	}
}

impl Record for PayeeMapping {
	type Key = PayeeId;

	fn key(&self) -> PayeeId {
		self.from.clone()
	}

	fn read(data: &BudgetData, key: &PayeeId) -> Option<Self> {
		data.payee_mapping.get(key).map(|to| PayeeMapping {
			from: key.clone(),
			to: to.clone(),
		})
	}

	fn write(data: &mut BudgetData, key: &PayeeId, value: Option<Self>) {
		put(&mut data.payee_mapping, key, value.map(|row| row.to));
	}

	fn retired(_: &Self) -> Retired<Self> {
		Retired::Kept
	}

	fn entity(key: &PayeeId) -> EntityRef {
		EntityRef::PayeeMapping(key.clone())
	}
}

impl Record for CategoryMetaRow {
	type Key = CategoryId;

	fn key(&self) -> CategoryId {
		self.category.clone()
	}

	fn read(data: &BudgetData, key: &CategoryId) -> Option<Self> {
		data.category_meta.get(key).map(|meta| CategoryMetaRow {
			category: key.clone(),
			meta: meta.clone(),
		})
	}

	fn write(data: &mut BudgetData, key: &CategoryId, value: Option<Self>) {
		put(&mut data.category_meta, key, value.map(|row| row.meta));
	}

	fn retired(_: &Self) -> Retired<Self> {
		Retired::Removed
	}

	fn entity(key: &CategoryId) -> EntityRef {
		EntityRef::CategoryMeta(key.clone())
	}
}

impl Record for AccountMetaRow {
	type Key = AccountId;

	fn key(&self) -> AccountId {
		self.account.clone()
	}

	fn read(data: &BudgetData, key: &AccountId) -> Option<Self> {
		data.account_meta.get(key).map(|meta| AccountMetaRow {
			account: key.clone(),
			meta: meta.clone(),
		})
	}

	fn write(data: &mut BudgetData, key: &AccountId, value: Option<Self>) {
		put(&mut data.account_meta, key, value.map(|row| row.meta));
	}

	fn retired(_: &Self) -> Retired<Self> {
		Retired::Removed
	}

	fn entity(key: &AccountId) -> EntityRef {
		EntityRef::AccountMeta(key.clone())
	}
}

impl Record for TransactionMetaRow {
	type Key = TransactionId;

	fn key(&self) -> TransactionId {
		self.transaction.clone()
	}

	fn read(data: &BudgetData, key: &TransactionId) -> Option<Self> {
		data.transaction_meta
			.get(key)
			.map(|&meta| TransactionMetaRow {
				transaction: key.clone(),
				meta,
			})
	}

	fn write(data: &mut BudgetData, key: &TransactionId, value: Option<Self>) {
		put(&mut data.transaction_meta, key, value.map(|row| row.meta));
	}

	fn retired(_: &Self) -> Retired<Self> {
		Retired::Removed
	}

	fn entity(key: &TransactionId) -> EntityRef {
		EntityRef::TransactionMeta(key.clone())
	}
}

impl Record for Goal {
	type Key = GoalId;

	fn key(&self) -> GoalId {
		self.id.clone()
	}

	fn read(data: &BudgetData, key: &GoalId) -> Option<Self> {
		data.goals.get(key).cloned()
	}

	fn write(data: &mut BudgetData, key: &GoalId, value: Option<Self>) {
		put(&mut data.goals, key, value);
	}

	fn retired(_: &Self) -> Retired<Self> {
		Retired::Removed
	}

	fn entity(key: &GoalId) -> EntityRef {
		EntityRef::Goal(key.clone())
	}
}
