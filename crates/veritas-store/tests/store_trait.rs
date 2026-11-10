//! veritas-store as the command bus's Store: after any list of changes,
//! reloading the file gives exactly the memory the changes produced.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Scratch, fixture_db};
use veritas_domain::{
	Account, AccountId, BudgetCell, BudgetData, Category, CategoryId, CategoryMapping, Change,
	CivilDate, CoreError, Goal, GoalId, GoalStatus, Hold, Money, Note, Payee, PayeeId,
	PayeeMapping, Row, SplitRole, Store as _, StoreTransaction, Transaction, TransactionId,
	TransactionMeta, TransactionMetaRow, YearMonth,
};
use veritas_store::{Access, Store};

fn perform(store: &mut Store, data: &mut BudgetData, rows: Vec<Row>) -> Result<(), CoreError> {
	let mut tx = store.begin()?;
	let mut changes: Vec<Change> = Vec::new();
	let mut planned = data.clone();
	for row in rows {
		let change = Change::put(&planned, row);
		tx.apply(&change)?;
		change.apply(&mut planned);
		changes.push(change);
	}
	tx.commit()?;
	*data = planned;
	Ok(())
}

#[test]
fn reloading_after_every_kind_of_change_matches_memory_r_rel_1() {
	let scratch = Scratch::new("trait-memory");
	let db = fixture_db("small", &scratch);
	let mut store = Store::open(&db, Access::ReadWrite).unwrap();
	store.adopt().unwrap();
	let mut data = store.load().unwrap().data;

	let account = Account {
		id: AccountId::new("acct-cu"),
		name: "Credit union".to_owned(),
		off_budget: false,
		closed: false,
		group: None,
		sort_order: 9000.0,
		tombstone: false,
	};
	let transfer_payee = Payee {
		id: PayeeId::new("payee-cu"),
		name: String::new(),
		transfer_account: Some(account.id.clone()),
		favorite: false,
		learn_categories: true,
		tombstone: false,
	};
	let group = data
		.categories
		.values()
		.find(|c| !c.is_income)
		.unwrap()
		.group
		.clone();
	let category = Category {
		id: CategoryId::new("cat-pets"),
		name: "Pets".to_owned(),
		group,
		is_income: false,
		hidden: false,
		sort_order: 9000.0,
		tombstone: false,
	};
	perform(
		&mut store,
		&mut data,
		vec![
			Row::Account(account.clone()),
			Row::Payee(transfer_payee.clone()),
			Row::PayeeMapping(PayeeMapping {
				from: transfer_payee.id.clone(),
				to: transfer_payee.id.clone(),
			}),
			Row::Category(category.clone()),
			Row::CategoryMapping(CategoryMapping {
				from: category.id.clone(),
				to: category.id.clone(),
			}),
		],
	)
	.unwrap();

	let purchase = Transaction {
		id: TransactionId::new("txn-vet"),
		account: account.id.clone(),
		date: CivilDate::new(2026, 11, 3).unwrap(),
		amount: Money::from_minor(-8_500),
		payee: None,
		imported_payee: None,
		category: Some(category.id.clone()),
		notes: Some("Annual checkup".to_owned()),
		cleared: false,
		reconciled: false,
		split: SplitRole::None,
		transfer: None,
		schedule: None,
		starting_balance: false,
		sort_order: 1_794_225_600_001.0,
		tombstone: false,
	};
	let november = YearMonth::parse("2026-11").unwrap();
	perform(
		&mut store,
		&mut data,
		vec![
			Row::Transaction(purchase.clone()),
			Row::BudgetCell(BudgetCell {
				month: november,
				category: category.id.clone(),
				budgeted: Money::from_minor(10_000),
				carryover: false,
				goal: None,
				long_goal: false,
			}),
			Row::Hold(Hold {
				month: november,
				amount: Some(Money::from_minor(2_500)),
			}),
			Row::Note(Note {
				id: format!("budget-{november}"),
				text: "Vet visit".to_owned(),
			}),
			Row::TransactionMeta(TransactionMetaRow {
				transaction: purchase.id.clone(),
				meta: TransactionMeta { adjustment: false },
			}),
			Row::Goal(Goal {
				id: GoalId::new("goal-pets"),
				category: category.id.clone(),
				target: Money::from_minor(50_000),
				target_date: Some(CivilDate::new(2027, 6, 1).unwrap()),
				icon: None,
				color: None,
				status: GoalStatus::Active,
				reached_month: None,
				closed_month: None,
				released_to: None,
				released: None,
			}),
		],
	)
	.unwrap();

	perform(
		&mut store,
		&mut data,
		vec![
			Row::Transaction(Transaction {
				amount: Money::from_minor(-9_250),
				cleared: true,
				..purchase.clone()
			}),
			Row::Hold(Hold {
				month: november,
				amount: None,
			}),
			Row::Account(Account {
				name: "Credit union checking".to_owned(),
				..account.clone()
			}),
		],
	)
	.unwrap();
	let deleted = Transaction {
		tombstone: true,
		..data.transactions[&purchase.id].clone()
	};
	perform(&mut store, &mut data, vec![Row::Transaction(deleted)]).unwrap();

	assert_eq!(store.load().unwrap().data, data);
	let (_, report) = store.open_audit().unwrap();
	assert!(report.is_clean(), "{report:?}");
}

#[test]
fn a_failed_change_leaves_nothing_behind_r_rel_1() {
	let scratch = Scratch::new("trait-rollback");
	let db = fixture_db("small", &scratch);
	let mut store = Store::open(&db, Access::ReadWrite).unwrap();
	let mut data = store.load().unwrap().data;
	let original = data.clone();
	let group = data.categories.values().next().unwrap().group.clone();
	let orphan = Category {
		id: CategoryId::new("cat-orphan"),
		name: "No mapping".to_owned(),
		group,
		is_income: false,
		hidden: false,
		sort_order: 1.0,
		tombstone: false,
	};
	let edited = Transaction {
		amount: Money::from_minor(1),
		..data.transactions.values().next().unwrap().clone()
	};
	let result = perform(
		&mut store,
		&mut data,
		vec![Row::Transaction(edited), Row::Category(orphan)],
	);
	assert!(matches!(result, Err(CoreError::Invalid(_))));
	assert_eq!(data, original);
	assert_eq!(store.load().unwrap().data, original);
}
