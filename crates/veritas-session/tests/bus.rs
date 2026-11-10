#![allow(
	clippy::unwrap_used,
	clippy::expect_used,
	clippy::panic,
	reason = "integration tests fail by panicking"
)]

mod support;

use std::sync::{Arc, Mutex};

use proptest::prelude::*;
use support::TempDir;
use veritas_domain::{
	Account, AccountGroup, AccountGroupId, AccountId, AccountKind, AccountMeta, AccountMetaRow,
	BudgetCell, BudgetData, BudgetType, Category, CategoryGroup, CategoryGroupId, CategoryId,
	CategoryKind, CategoryMapping, CategoryMeta, CategoryMetaRow, Change, CivilDate, ConditionsOp,
	CoreError, EntityRef, Goal, GoalId, GoalStatus, Hold, Money, Note, Payee, PayeeId,
	PayeeMapping, ReadOnlyReason, Row, Rule, RuleId, RuleStage, Schedule, ScheduleId, SplitRole,
	Store, StoreTransaction, Tag, TagId, Transaction, TransactionId, TransactionMeta,
	TransactionMetaRow, YearMonth,
};
use veritas_session::{Access, ChangeListener, ChangeSet, CommandBus, Permit, acquire};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
	OnApply(usize),
	OnCommit,
}

/// Stands in for SQLite: a committed copy of the budget that only changes on
/// commit. It checks every change starts from what it holds, so a wrong
/// `before` from the bus fails loudly, and it can fail on purpose.
struct MirrorStore {
	committed: BudgetData,
	commits: usize,
	fail_next: Option<Failure>,
}

impl MirrorStore {
	fn new(data: &BudgetData) -> Self {
		MirrorStore {
			committed: data.clone(),
			commits: 0,
			fail_next: None,
		}
	}

	fn fail_next(&mut self, failure: Failure) {
		self.fail_next = Some(failure);
	}

	fn disarm(&mut self) {
		self.fail_next = None;
	}
}

struct MirrorTransaction<'a> {
	store: &'a mut MirrorStore,
	pending: BudgetData,
	applied: usize,
	failure: Option<Failure>,
}

impl Store for MirrorStore {
	type Transaction<'a> = MirrorTransaction<'a>;

	fn begin(&mut self) -> Result<MirrorTransaction<'_>, CoreError> {
		let pending = self.committed.clone();
		let failure = self.fail_next.take();
		Ok(MirrorTransaction {
			store: self,
			pending,
			applied: 0,
			failure,
		})
	}
}

impl StoreTransaction for MirrorTransaction<'_> {
	fn apply(&mut self, change: &Change) -> Result<(), CoreError> {
		if self.failure == Some(Failure::OnApply(self.applied)) {
			return Err(CoreError::Storage("disk I/O error".to_owned()));
		}
		if !change.starts_from(&self.pending) {
			return Err(CoreError::Internal(format!(
				"stale before for {:?}",
				change.entity()
			)));
		}
		change.apply(&mut self.pending);
		self.applied += 1;
		Ok(())
	}

	fn commit(self) -> Result<(), CoreError> {
		if self.failure == Some(Failure::OnCommit) {
			return Err(CoreError::Storage("database is locked".to_owned()));
		}
		self.store.committed = self.pending;
		self.store.commits += 1;
		Ok(())
	}

	fn rollback(self) -> Result<(), CoreError> {
		Ok(())
	}
}

type Bus = CommandBus<MirrorStore>;

fn account(id: &str, name: &str) -> Account {
	Account {
		id: AccountId::new(id),
		name: name.to_owned(),
		off_budget: false,
		closed: false,
		group: None,
		sort_order: 0.0,
		tombstone: false,
	}
}

fn category(id: &str, name: &str) -> Category {
	Category {
		id: CategoryId::new(id),
		name: name.to_owned(),
		group: CategoryGroupId::new("bills"),
		is_income: false,
		hidden: false,
		sort_order: 0.0,
		tombstone: false,
	}
}

fn transaction(id: &str, amount: i64) -> Transaction {
	Transaction {
		id: TransactionId::new(id),
		account: AccountId::new("checking"),
		date: CivilDate::new(2027, 3, 14).unwrap(),
		amount: Money::from_minor(amount),
		payee: None,
		imported_payee: None,
		category: Some(CategoryId::new("groceries")),
		notes: None,
		cleared: false,
		reconciled: false,
		split: SplitRole::None,
		transfer: None,
		schedule: None,
		starting_balance: false,
		sort_order: 0.0,
		tombstone: false,
	}
}

fn march() -> YearMonth {
	YearMonth::new(2027, 3).unwrap()
}

fn cell(category: &str, budgeted: i64) -> BudgetCell {
	BudgetCell {
		month: march(),
		category: CategoryId::new(category),
		budgeted: Money::from_minor(budgeted),
		carryover: true,
		goal: Some(Money::from_minor(50_000)),
		long_goal: false,
	}
}

fn goal(id: &str) -> Goal {
	Goal {
		id: GoalId::new(id),
		category: CategoryId::new("vacation"),
		target: Money::from_minor(300_000),
		target_date: None,
		icon: None,
		color: None,
		status: GoalStatus::Active,
		reached_month: None,
		closed_month: None,
		released_to: None,
		released: None,
	}
}

fn budget() -> BudgetData {
	let mut data = BudgetData::empty(BudgetType::Envelope);
	for row in [account("checking", "Checking"), account("visa", "Visa")] {
		data.accounts.insert(row.id.clone(), row);
	}
	data.category_groups.insert(
		CategoryGroupId::new("bills"),
		CategoryGroup {
			id: CategoryGroupId::new("bills"),
			name: "Bills".to_owned(),
			is_income: false,
			hidden: false,
			sort_order: 0.0,
			tombstone: false,
		},
	);
	for row in [
		category("groceries", "Groceries"),
		category("rent", "Rent"),
		category("vacation", "Vacation"),
	] {
		data.categories.insert(row.id.clone(), row);
	}
	for row in [transaction("t1", -4_512), transaction("t2", -165_000)] {
		data.transactions.insert(row.id.clone(), row);
	}
	let groceries = cell("groceries", 60_000);
	data.budget_cells
		.insert((groceries.month, groceries.category.clone()), groceries);
	data
}

/// A bus over `budget()`, built with a writer lock on a throwaway folder.
fn bus() -> Bus {
	let folder = TempDir::new("bus");
	let Access::Writer(lock) = acquire(folder.path()).unwrap() else {
		panic!("a fresh folder is always free to lock");
	};
	let data = budget();
	let store = MirrorStore::new(&data);
	CommandBus::new(data, store, Permit::Write(&lock))
}

fn rows(rows: Vec<Row>) -> impl Fn(&BudgetData) -> Result<Vec<Row>, CoreError> {
	move |_: &BudgetData| Ok(rows.clone())
}

fn renamed(data: &BudgetData, id: &str, name: &str) -> Row {
	let mut row = data.accounts[&AccountId::new(id)].clone();
	row.name = name.to_owned();
	Row::Account(row)
}

fn budgeted(category: &str, amount: i64) -> Row {
	Row::BudgetCell(cell(category, amount))
}

fn assert_in_sync(bus: &Bus) {
	assert_eq!(bus.data(), &bus.store().committed);
}

#[derive(Default)]
struct Recorder(Mutex<Vec<ChangeSet>>);

impl ChangeListener for Recorder {
	fn changed(&self, changes: &ChangeSet) {
		self.0.lock().unwrap().push(changes.clone());
	}
}

impl Recorder {
	fn take(&self) -> Vec<ChangeSet> {
		std::mem::take(&mut self.0.lock().unwrap())
	}
}

fn ids(set: &ChangeSet) -> Vec<EntityRef> {
	set.entities.iter().cloned().collect()
}

#[test]
fn a_command_lands_in_memory_and_storage_as_one_transaction() {
	// R-REL-1
	let mut bus = bus();
	let rename = renamed(bus.data(), "checking", "Everyday");
	let changed = bus
		.perform(&rows(vec![
			rename,
			budgeted("rent", 165_000),
			Row::Transaction(transaction("t3", -2_250)),
		]))
		.unwrap();

	assert_in_sync(&bus);
	assert_eq!(bus.store().commits, 1);
	assert_eq!(
		bus.data().accounts[&AccountId::new("checking")].name,
		"Everyday"
	);
	assert_eq!(
		ids(&changed),
		[
			EntityRef::Account(AccountId::new("checking")),
			EntityRef::Transaction(TransactionId::new("t3")),
			EntityRef::BudgetCell(march(), CategoryId::new("rent")),
		]
	);
}

#[test]
fn a_storage_failure_mid_command_leaves_memory_exactly_as_it_was() {
	// R-REL-1
	for failure in [
		Failure::OnApply(0),
		Failure::OnApply(1),
		Failure::OnApply(2),
		Failure::OnCommit,
	] {
		let mut bus = bus();
		let listener = Arc::new(Recorder::default());
		bus.subscribe(listener.clone());
		let before = bus.data().clone();
		let rename = renamed(bus.data(), "checking", "Everyday");
		bus.store_mut().fail_next(failure);

		let result = bus.perform(&rows(vec![
			rename,
			budgeted("groceries", 75_000),
			Row::Transaction(transaction("t3", -2_250)),
		]));

		assert!(
			matches!(result, Err(CoreError::Storage(_))),
			"{failure:?}: {result:?}"
		);
		assert_eq!(bus.data(), &before, "{failure:?}");
		assert_in_sync(&bus);
		assert!(!bus.can_undo(), "{failure:?}");
		assert!(listener.take().is_empty(), "{failure:?}");
	}
}

#[test]
fn a_rejected_command_touches_nothing() {
	let mut bus = bus();
	let before = bus.data().clone();
	let reject = |_: &BudgetData| -> Result<Vec<Row>, CoreError> {
		Err(CoreError::Invalid("enter an amount".to_owned()))
	};
	assert!(matches!(bus.perform(&reject), Err(CoreError::Invalid(_))));
	assert_eq!(bus.data(), &before);
	assert_eq!(bus.store().commits, 0);
	assert!(!bus.can_undo());
}

#[test]
fn a_command_that_changes_nothing_is_not_an_undo_step() {
	let mut bus = bus();
	let same = renamed(bus.data(), "checking", "Checking");
	let changed = bus.perform(&rows(vec![same])).unwrap();
	assert!(changed.is_empty());
	assert!(!bus.can_undo());
}

#[test]
fn undo_and_redo_walk_back_and_forth_through_every_edit() {
	// R-REL-5: no limit on depth.
	let mut bus = bus();
	let initial = bus.data().clone();
	for step in 0..200 {
		let name = format!("Checking {step}");
		let rename = renamed(bus.data(), "checking", &name);
		bus.perform(&rows(vec![rename, budgeted("groceries", step * 100)]))
			.unwrap();
	}
	let edited = bus.data().clone();

	while bus.can_undo() {
		bus.undo().unwrap();
		assert_in_sync(&bus);
	}
	assert_eq!(bus.data(), &initial);

	while bus.can_redo() {
		bus.redo().unwrap();
	}
	assert_eq!(bus.data(), &edited);
	assert_in_sync(&bus);
}

#[test]
fn undoing_a_creation_retires_rows_the_way_actual_does() {
	// R-REL-5. reference/actual/packages/loot-core/src/server/undo.ts:161-203
	// and redo's resurrections at :236-280.
	let mut bus = bus();
	let meta = CategoryMetaRow {
		category: CategoryId::new("vacation"),
		meta: CategoryMeta {
			kind: Some(CategoryKind::Savings),
			tax_bucket: None,
			archived_on: None,
		},
	};
	bus.perform(&rows(vec![
		Row::Transaction(transaction("t3", -2_250)),
		budgeted("rent", 165_000),
		Row::Note(Note {
			id: "rent".to_owned(),
			text: Some("Due on the 1st".to_owned()),
		}),
		Row::CategoryMeta(meta),
		Row::Goal(goal("trip")),
		Row::Hold(Hold {
			month: march(),
			amount: Some(Money::from_minor(25_000)),
		}),
		Row::TransactionMeta(TransactionMetaRow {
			transaction: TransactionId::new("t3"),
			meta: TransactionMeta {
				adjustment: true,
				needs_actual: None,
			},
		}),
		Row::CategoryMapping(CategoryMapping {
			from: CategoryId::new("vacation"),
			to: CategoryId::new("vacation"),
		}),
	]))
	.unwrap();
	let created = bus.data().clone();

	bus.undo().unwrap();
	let data = bus.data();
	assert!(data.transactions[&TransactionId::new("t3")].tombstone);
	let rent = &data.budget_cells[&(march(), CategoryId::new("rent"))];
	assert_eq!((rent.budgeted, rent.carryover), (Money::ZERO, false));
	assert_eq!(rent.goal, Some(Money::from_minor(50_000)));
	assert_eq!(
		data.notes["rent"], None,
		"undo clears the note and keeps its row"
	);
	assert!(
		!data
			.category_meta
			.contains_key(&CategoryId::new("vacation"))
	);
	assert!(!data.goals.contains_key(&GoalId::new("trip")));
	assert_eq!(data.holds.get(&march()), Some(&Some(Money::ZERO)));
	assert!(
		!data
			.transaction_meta
			.contains_key(&TransactionId::new("t3"))
	);
	assert_eq!(
		data.category_mapping.get(&CategoryId::new("vacation")),
		Some(&CategoryId::new("vacation"))
	);
	assert_in_sync(&bus);

	bus.redo().unwrap();
	assert_eq!(bus.data(), &created);
	assert_in_sync(&bus);
}

#[test]
fn a_new_command_clears_redo() {
	let mut bus = bus();
	bus.perform(&rows(vec![budgeted("groceries", 1)])).unwrap();
	bus.undo().unwrap();
	assert!(bus.can_redo());
	bus.perform(&rows(vec![budgeted("groceries", 2)])).unwrap();
	assert!(!bus.can_redo());
}

#[test]
fn a_failed_undo_or_redo_keeps_its_step_for_a_retry() {
	// R-REL-1, R-REL-5
	let mut bus = bus();
	bus.perform(&rows(vec![budgeted("groceries", 1), budgeted("rent", 2)]))
		.unwrap();
	let edited = bus.data().clone();

	bus.store_mut().fail_next(Failure::OnApply(1));
	assert!(bus.undo().is_err());
	assert_eq!(bus.data(), &edited);
	assert!(bus.can_undo() && !bus.can_redo());
	bus.undo().unwrap();
	let undone = bus.data().clone();

	bus.store_mut().fail_next(Failure::OnCommit);
	assert!(bus.redo().is_err());
	assert_eq!(bus.data(), &undone);
	assert!(bus.can_redo());
	bus.redo().unwrap();
	assert_eq!(bus.data(), &edited);
	assert_in_sync(&bus);
}

#[test]
fn a_budget_open_elsewhere_refuses_every_write() {
	// R-REL-6
	let data = budget();
	let store = MirrorStore::new(&data);
	let mut bus = CommandBus::new(data, store, Permit::ReadOnly(ReadOnlyReason::OpenElsewhere));
	let read_only = |result: Result<ChangeSet, CoreError>| {
		matches!(
			result,
			Err(CoreError::ReadOnly(ReadOnlyReason::OpenElsewhere))
		)
	};
	assert!(read_only(bus.perform(&rows(vec![budgeted("rent", 1)]))));
	assert!(read_only(bus.undo()));
	assert!(read_only(bus.redo()));
	assert_eq!(bus.store().commits, 0);
}

#[test]
fn subscribers_hear_which_rows_changed_after_each_commit() {
	let mut bus = bus();
	let listener = Arc::new(Recorder::default());
	let subscription = bus.subscribe(listener.clone());
	let rent = EntityRef::BudgetCell(march(), CategoryId::new("rent"));

	bus.perform(&rows(vec![budgeted("rent", 1)])).unwrap();
	bus.undo().unwrap();
	bus.redo().unwrap();
	let heard: Vec<_> = listener.take().iter().map(ids).collect();
	assert_eq!(heard, vec![vec![rent.clone()]; 3]);

	bus.store_mut().fail_next(Failure::OnCommit);
	assert!(bus.perform(&rows(vec![budgeted("rent", 2)])).is_err());
	assert!(listener.take().is_empty());

	bus.unsubscribe(subscription);
	bus.perform(&rows(vec![budgeted("rent", 3)])).unwrap();
	assert!(listener.take().is_empty());
}

/// One new row of every kind the bus writes, as a "set up my budget" command
/// would create them.
struct NewRows {
	account: Account,
	account_group: AccountGroup,
	category_group: CategoryGroup,
	category: Category,
	payee: Payee,
	transaction: Transaction,
	rule: Rule,
	schedule: Schedule,
	tag: Tag,
	cell: BudgetCell,
	hold: Hold,
	note: Note,
	category_mapping: CategoryMapping,
	payee_mapping: PayeeMapping,
	category_meta: CategoryMetaRow,
	account_meta: AccountMetaRow,
	transaction_meta: TransactionMetaRow,
	goal: Goal,
}

impl NewRows {
	fn new() -> Self {
		NewRows {
			account: Account {
				group: Some(AccountGroupId::new("cash")),
				..account("savings", "High-yield savings")
			},
			account_group: AccountGroup {
				id: AccountGroupId::new("cash"),
				name: "Cash".to_owned(),
				sort_order: 1.0,
				tombstone: false,
			},
			category_group: CategoryGroup {
				id: CategoryGroupId::new("wants"),
				name: "Wants".to_owned(),
				is_income: false,
				hidden: false,
				sort_order: 2.0,
				tombstone: false,
			},
			category: Category {
				group: CategoryGroupId::new("wants"),
				..category("dining", "Dining out")
			},
			payee: Payee {
				id: PayeeId::new("landlord"),
				name: "Landlord LLC".to_owned(),
				transfer_account: None,
				favorite: false,
				learn_categories: true,
				tombstone: false,
			},
			transaction: Transaction {
				payee: Some(PayeeId::new("landlord")),
				category: Some(CategoryId::new("rent")),
				..transaction("t-rent", -165_000)
			},
			rule: Rule {
				id: RuleId::new("rule-rent"),
				stage: RuleStage::Default,
				conditions_op: ConditionsOp::And,
				conditions_json: r#"[{"op":"is","field":"payee","value":"landlord"}]"#.to_owned(),
				actions_json: r#"[{"op":"set","field":"category","value":"rent"}]"#.to_owned(),
				tombstone: false,
			},
			schedule: Schedule {
				id: ScheduleId::new("sched-rent"),
				name: Some("Rent".to_owned()),
				rule: RuleId::new("rule-rent"),
				posts_transaction: false,
				completed: false,
				next_date: CivilDate::new(2027, 4, 1).ok(),
				tombstone: false,
			},
			tag: Tag {
				id: TagId::new("tag-house"),
				name: "house".to_owned(),
				color: Some("#5b8def".to_owned()),
				description: None,
				hidden: false,
				tombstone: false,
			},
			cell: cell("rent", 165_000),
			hold: Hold {
				month: march(),
				amount: Some(Money::from_minor(25_000)),
			},
			note: Note {
				id: "rent".to_owned(),
				text: Some("Due on the 1st".to_owned()),
			},
			category_mapping: CategoryMapping {
				from: CategoryId::new("dining"),
				to: CategoryId::new("dining"),
			},
			payee_mapping: PayeeMapping {
				from: PayeeId::new("landlord"),
				to: PayeeId::new("landlord"),
			},
			category_meta: CategoryMetaRow {
				category: CategoryId::new("dining"),
				meta: CategoryMeta {
					kind: Some(CategoryKind::Expense),
					tax_bucket: None,
					archived_on: None,
				},
			},
			account_meta: AccountMetaRow {
				account: AccountId::new("savings"),
				meta: AccountMeta {
					kind: Some(AccountKind::Asset),
					..AccountMeta::default()
				},
			},
			transaction_meta: TransactionMetaRow {
				transaction: TransactionId::new("t-rent"),
				meta: TransactionMeta {
					adjustment: false,
					needs_actual: None,
				},
			},
			goal: goal("trip"),
		}
	}

	/// The same rows after a second command edits a field of each one.
	fn edited() -> Self {
		let mut rows = NewRows::new();
		rows.account.name = "Emergency fund".to_owned();
		rows.account_group.name = "Cash & savings".to_owned();
		rows.category_group.hidden = true;
		rows.category.name = "Restaurants".to_owned();
		rows.payee.favorite = true;
		rows.transaction.amount = Money::from_minor(-170_000);
		rows.rule.stage = RuleStage::Pre;
		rows.schedule.next_date = CivilDate::new(2027, 5, 1).ok();
		rows.tag.hidden = true;
		rows.cell.budgeted = Money::from_minor(170_000);
		rows.hold.amount = Some(Money::from_minor(30_000));
		rows.note.text = Some("Due on the 1st, late after the 5th".to_owned());
		rows.category_mapping.to = CategoryId::new("groceries");
		rows.payee_mapping.to = PayeeId::new("property-co");
		rows.category_meta.meta.tax_bucket = Some("schedule-c".to_owned());
		rows.account_meta.meta.lender = Some("Ally".to_owned());
		rows.transaction_meta.meta.adjustment = true;
		rows.goal.target = Money::from_minor(450_000);
		rows
	}

	fn rows(&self) -> Vec<Row> {
		vec![
			Row::AccountGroup(self.account_group.clone()),
			Row::Account(self.account.clone()),
			Row::CategoryGroup(self.category_group.clone()),
			Row::Category(self.category.clone()),
			Row::Payee(self.payee.clone()),
			Row::Transaction(self.transaction.clone()),
			Row::Rule(self.rule.clone()),
			Row::Schedule(self.schedule.clone()),
			Row::Tag(self.tag.clone()),
			Row::BudgetCell(self.cell.clone()),
			Row::Hold(self.hold.clone()),
			Row::Note(self.note.clone()),
			Row::CategoryMapping(self.category_mapping.clone()),
			Row::PayeeMapping(self.payee_mapping.clone()),
			Row::CategoryMeta(self.category_meta.clone()),
			Row::AccountMeta(self.account_meta.clone()),
			Row::TransactionMeta(self.transaction_meta.clone()),
			Row::Goal(self.goal.clone()),
		]
	}
}

#[test]
fn every_kind_of_row_undoes_and_redoes_exactly() {
	// R-REL-5 "covering every command": one command creates a row of every
	// kind, a second edits each of them. Undoing the edit restores every row
	// exactly; undoing the creation retires each the way Actual does
	// (reference/actual/packages/loot-core/src/server/undo.ts:161-203); redo
	// brings both back.
	let mut bus = bus();
	let initial = bus.data().clone();
	let created = NewRows::new();
	let edited = NewRows::edited();

	let changed = bus.perform(&rows(created.rows())).unwrap();
	assert_eq!(changed.entities.len(), 18);
	let after_create = bus.data().clone();
	let changed = bus.perform(&rows(edited.rows())).unwrap();
	assert_eq!(changed.entities.len(), 18);
	let after_edit = bus.data().clone();

	bus.undo().unwrap();
	assert_eq!(bus.data(), &after_create);
	assert_in_sync(&bus);

	bus.undo().unwrap();
	let mut retired = initial.clone();
	macro_rules! tombstoned {
		($($field:ident: $row:ident),*) => {$(
			retired.$field.insert(
				created.$row.id.clone(),
				{
					let mut row = created.$row.clone();
					row.tombstone = true;
					row
				},
			);
		)*};
	}
	tombstoned!(
		accounts: account,
		account_groups: account_group,
		category_groups: category_group,
		categories: category,
		payees: payee,
		transactions: transaction,
		rules: rule,
		schedules: schedule,
		tags: tag
	);
	retired.budget_cells.insert(
		(march(), CategoryId::new("rent")),
		BudgetCell {
			budgeted: Money::ZERO,
			carryover: false,
			..created.cell.clone()
		},
	);
	retired.holds.insert(march(), Some(Money::ZERO));
	retired.notes.insert("rent".to_owned(), None);
	retired
		.category_mapping
		.insert(CategoryId::new("dining"), CategoryId::new("dining"));
	retired
		.payee_mapping
		.insert(PayeeId::new("landlord"), PayeeId::new("landlord"));
	assert_eq!(bus.data(), &retired);
	assert_in_sync(&bus);

	bus.redo().unwrap();
	assert_eq!(bus.data(), &after_create);
	bus.redo().unwrap();
	assert_eq!(bus.data(), &after_edit);
	assert_in_sync(&bus);
	assert!(!bus.can_redo());
}

#[test]
fn a_command_that_writes_one_row_twice_undoes_to_the_original() {
	// R-REL-1, R-REL-5: covering overspending moves money into the same
	// category twice in one command. Undo returns the original amount, and a
	// failure on the second write leaves the first unapplied too.
	let mut bus = bus();
	let original = bus.data().clone();
	let twice = rows(vec![
		budgeted("groceries", 70_000),
		budgeted("groceries", 71_234),
	]);

	bus.store_mut().fail_next(Failure::OnApply(1));
	assert!(bus.perform(&twice).is_err());
	assert_eq!(bus.data(), &original);
	assert_in_sync(&bus);

	bus.perform(&twice).unwrap();
	let groceries = (march(), CategoryId::new("groceries"));
	assert_eq!(
		bus.data().budget_cells[&groceries].budgeted,
		Money::from_minor(71_234)
	);
	assert_in_sync(&bus);

	bus.undo().unwrap();
	assert_eq!(bus.data(), &original);
	assert_in_sync(&bus);
	bus.redo().unwrap();
	assert_eq!(
		bus.data().budget_cells[&groceries].budgeted,
		Money::from_minor(71_234)
	);
}

#[derive(Clone, Debug)]
enum Op {
	Budget { category: usize, amount: i64 },
	Rename { account: usize, name: u8 },
	Add { id: u8, amount: i64 },
	Undo,
	Redo,
}

fn op() -> impl Strategy<Value = Op> {
	prop_oneof![
		(0usize..3, -100_000i64..100_000)
			.prop_map(|(category, amount)| Op::Budget { category, amount }),
		(0usize..2, any::<u8>()).prop_map(|(account, name)| Op::Rename { account, name }),
		(any::<u8>(), -100_000i64..100_000).prop_map(|(id, amount)| Op::Add { id, amount }),
		Just(Op::Undo),
		Just(Op::Redo),
	]
}

fn failure() -> impl Strategy<Value = Option<Failure>> {
	prop_oneof![
		3 => Just(None),
		1 => (0usize..3).prop_map(|n| Some(Failure::OnApply(n))),
		1 => Just(Some(Failure::OnCommit)),
	]
}

fn run(bus: &mut Bus, op: &Op) -> Result<ChangeSet, CoreError> {
	const CATEGORIES: [&str; 3] = ["groceries", "rent", "vacation"];
	const ACCOUNTS: [&str; 2] = ["checking", "visa"];
	match op {
		Op::Budget { category, amount } => {
			bus.perform(&rows(vec![budgeted(CATEGORIES[*category], *amount)]))
		}
		Op::Rename { account, name } => {
			let rename = renamed(bus.data(), ACCOUNTS[*account], &format!("Account {name}"));
			bus.perform(&rows(vec![rename, budgeted("groceries", i64::from(*name))]))
		}
		Op::Add { id, amount } => bus.perform(&rows(vec![
			Row::Transaction(transaction(&format!("new-{id}"), *amount)),
			budgeted("vacation", *amount),
		])),
		Op::Undo => bus.undo(),
		Op::Redo => bus.redo(),
	}
}

proptest! {
	// R-REL-1, R-REL-5: any sequence of edits, undos, redos and injected
	// storage failures keeps memory equal to what storage committed, a failed
	// step changes nothing, and undoing everything then redoing everything
	// lands back on the same budget.
	#[test]
	fn memory_and_storage_never_disagree(steps in prop::collection::vec((op(), failure()), 1..60)) {
		let mut bus = bus();
		for (op, failure) in &steps {
			let before = bus.data().clone();
			if let Some(failure) = failure {
				bus.store_mut().fail_next(*failure);
			}
			let result = run(&mut bus, op);
			bus.store_mut().disarm();
			if result.is_err() {
				prop_assert_eq!(bus.data(), &before);
			}
			prop_assert_eq!(bus.data(), &bus.store().committed);
		}

		let latest = bus.data().clone();
		let mut undone = 0;
		while bus.can_undo() {
			bus.undo().unwrap();
			undone += 1;
		}
		for _ in 0..undone {
			bus.redo().unwrap();
		}
		prop_assert_eq!(bus.data(), &latest);
		prop_assert_eq!(bus.data(), &bus.store().committed);
	}
}
