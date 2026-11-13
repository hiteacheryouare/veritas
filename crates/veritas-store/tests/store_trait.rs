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
		last_reconciled: None,
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
		split_error: None,
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
				text: Some("Vet visit".to_owned()),
			}),
			Row::TransactionMeta(TransactionMetaRow {
				transaction: purchase.id.clone(),
				meta: TransactionMeta {
					adjustment: false,
					needs_actual: Some("a rule set notes with a template".to_owned()),
				},
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

#[test]
fn dropping_a_transaction_without_committing_rolls_it_back() {
	let scratch = Scratch::new("trait-drop");
	let db = fixture_db("small", &scratch);
	let mut store = Store::open(&db, Access::ReadWrite).unwrap();
	let data = store.load().unwrap().data;
	let edited = Transaction {
		amount: Money::from_minor(1),
		..data.transactions.values().next().unwrap().clone()
	};
	{
		let mut tx = store.begin().unwrap();
		tx.apply(&Change::put(&data, Row::Transaction(edited)))
			.unwrap();
	}
	assert_eq!(store.load().unwrap().data, data);
}

/// Names every Change variant, so a new one fails to compile here until the
/// test below writes it too.
fn variant(change: &Change) -> &'static str {
	match change {
		Change::Account(_) => "Account",
		Change::AccountGroup(_) => "AccountGroup",
		Change::CategoryGroup(_) => "CategoryGroup",
		Change::Category(_) => "Category",
		Change::Payee(_) => "Payee",
		Change::Transaction(_) => "Transaction",
		Change::Rule(_) => "Rule",
		Change::Schedule(_) => "Schedule",
		Change::Tag(_) => "Tag",
		Change::BudgetCell(_) => "BudgetCell",
		Change::Hold(_) => "Hold",
		Change::Note(_) => "Note",
		Change::CategoryMapping(_) => "CategoryMapping",
		Change::PayeeMapping(_) => "PayeeMapping",
		Change::CategoryMeta(_) => "CategoryMeta",
		Change::AccountMeta(_) => "AccountMeta",
		Change::TransactionMeta(_) => "TransactionMeta",
		Change::TaxTag(_) => "TaxTag",
		Change::Goal(_) => "Goal",
	}
}

const EVERY_VARIANT: [&str; 19] = [
	"Account",
	"AccountGroup",
	"CategoryGroup",
	"Category",
	"Payee",
	"Transaction",
	"Rule",
	"Schedule",
	"Tag",
	"BudgetCell",
	"Hold",
	"Note",
	"CategoryMapping",
	"PayeeMapping",
	"CategoryMeta",
	"AccountMeta",
	"TransactionMeta",
	"TaxTag",
	"Goal",
];

#[test]
fn every_change_variant_is_created_then_edited_and_reloads_as_memory() {
	use veritas_domain::{
		AccountGroup, AccountGroupId, AccountKind, AccountMeta, AccountMetaRow, CategoryGroup,
		CategoryGroupId, CategoryKind, CategoryMeta, CategoryMetaRow, ConditionsOp, Rule, RuleId,
		RuleStage, Schedule, ScheduleId, Tag, TagId,
	};
	let scratch = Scratch::new("trait-every-variant");
	let db = fixture_db("small", &scratch);
	let mut store = Store::open(&db, Access::ReadWrite).unwrap();
	store.adopt().unwrap();
	let mut data = store.load().unwrap().data;
	let month = YearMonth::parse("2026-11").unwrap();

	let account = Account {
		last_reconciled: None,
		id: AccountId::new("v-acct"),
		name: "Brokerage".to_owned(),
		off_budget: true,
		closed: false,
		group: Some(AccountGroupId::new("v-group")),
		sort_order: 1.0,
		tombstone: false,
	};
	let payee = Payee {
		id: PayeeId::new("v-payee"),
		name: String::new(),
		transfer_account: Some(account.id.clone()),
		favorite: false,
		learn_categories: true,
		tombstone: false,
	};
	let group = CategoryGroup {
		id: CategoryGroupId::new("v-cat-group"),
		name: "Goals".to_owned(),
		is_income: false,
		hidden: false,
		sort_order: 1.0,
		tombstone: false,
	};
	let category = Category {
		id: CategoryId::new("v-cat"),
		name: "Car".to_owned(),
		group: group.id.clone(),
		is_income: false,
		hidden: false,
		sort_order: 1.0,
		tombstone: false,
	};
	let rule = Rule {
		id: RuleId::new("v-rule"),
		stage: RuleStage::Pre,
		conditions_op: ConditionsOp::And,
		conditions_json: "[]".to_owned(),
		actions_json: "[]".to_owned(),
		tombstone: false,
	};
	let schedule = Schedule {
		id: ScheduleId::new("v-schedule"),
		name: Some("Insurance".to_owned()),
		rule: rule.id.clone(),
		posts_transaction: false,
		completed: false,
		next_date: Some(CivilDate::new(2026, 12, 1).unwrap()),
		tombstone: false,
	};
	let transaction = Transaction {
		id: TransactionId::new("v-txn"),
		account: account.id.clone(),
		date: CivilDate::new(2026, 11, 5).unwrap(),
		amount: Money::from_minor(-1_000),
		payee: None,
		imported_payee: Some("ACME".to_owned()),
		category: None,
		notes: None,
		cleared: true,
		reconciled: false,
		split: SplitRole::None,
		transfer: None,
		schedule: Some(schedule.id.clone()),
		starting_balance: false,
		sort_order: 2.0,
		tombstone: false,
		split_error: None,
	};
	let created = vec![
		Row::AccountGroup(AccountGroup {
			id: AccountGroupId::new("v-group"),
			name: "Investments".to_owned(),
			sort_order: 1.0,
			tombstone: false,
		}),
		Row::Account(account.clone()),
		Row::Payee(payee.clone()),
		Row::PayeeMapping(PayeeMapping {
			from: payee.id.clone(),
			to: payee.id.clone(),
		}),
		Row::CategoryGroup(group.clone()),
		Row::Category(category.clone()),
		Row::CategoryMapping(CategoryMapping {
			from: category.id.clone(),
			to: category.id.clone(),
		}),
		Row::Rule(rule.clone()),
		Row::Schedule(schedule.clone()),
		Row::Transaction(transaction.clone()),
		Row::Tag(Tag {
			id: TagId::new("v-tag"),
			name: "car".to_owned(),
			color: None,
			description: None,
			hidden: false,
			tombstone: false,
		}),
		Row::BudgetCell(BudgetCell {
			month,
			category: category.id.clone(),
			budgeted: Money::from_minor(5_000),
			carryover: false,
			goal: None,
			long_goal: false,
		}),
		Row::Hold(Hold {
			month,
			amount: Some(Money::from_minor(100)),
		}),
		Row::Note(Note {
			id: "v-note".to_owned(),
			text: Some("Renew in May".to_owned()),
		}),
		Row::CategoryMeta(CategoryMetaRow {
			category: category.id.clone(),
			meta: CategoryMeta {
				kind: Some(CategoryKind::Savings),
				tax_bucket: None,
				archived_on: None,
			},
		}),
		Row::AccountMeta(AccountMetaRow {
			account: account.id.clone(),
			meta: AccountMeta {
				kind: Some(AccountKind::Asset),
				..AccountMeta::default()
			},
		}),
		Row::TransactionMeta(TransactionMetaRow {
			transaction: transaction.id.clone(),
			meta: TransactionMeta {
				adjustment: true,
				needs_actual: None,
			},
		}),
		Row::TaxTag(veritas_domain::TaxTag {
			tag: "giving".to_owned(),
			bucket: "charitable".to_owned(),
		}),
		Row::Goal(Goal {
			id: GoalId::new("v-goal"),
			category: category.id.clone(),
			target: Money::from_minor(100_000),
			target_date: None,
			icon: None,
			color: None,
			status: GoalStatus::Active,
			reached_month: None,
			closed_month: None,
			released_to: None,
			released: None,
		}),
	];
	let mut seen = std::collections::BTreeSet::new();
	let mut run = |data: &mut BudgetData, rows: Vec<Row>| {
		let mut tx = store.begin().unwrap();
		let mut planned = data.clone();
		for row in rows {
			let change = Change::put(&planned, row);
			seen.insert(variant(&change));
			tx.apply(&change).unwrap();
			change.apply(&mut planned);
		}
		tx.commit().unwrap();
		*data = planned;
	};
	run(&mut data, created.clone());

	// Every row again with a field changed, so each one takes the update path.
	// The mappings are repointed, as a merge would.
	let other_category = data
		.categories
		.keys()
		.find(|id| **id != category.id)
		.unwrap()
		.clone();
	let other_payee = data
		.payees
		.keys()
		.find(|id| **id != payee.id)
		.unwrap()
		.clone();
	let edited: Vec<Row> = created
		.into_iter()
		.map(|row| match row {
			Row::AccountGroup(row) => Row::AccountGroup(AccountGroup {
				name: "Brokerage".to_owned(),
				..row
			}),
			Row::Account(row) => Row::Account(Account {
				closed: true,
				..row
			}),
			Row::Payee(row) => Row::Payee(Payee {
				favorite: true,
				..row
			}),
			Row::CategoryGroup(row) => Row::CategoryGroup(CategoryGroup {
				hidden: true,
				..row
			}),
			Row::Category(row) => Row::Category(Category {
				name: "Car fund".to_owned(),
				..row
			}),
			Row::CategoryMapping(row) => Row::CategoryMapping(CategoryMapping {
				to: other_category.clone(),
				..row
			}),
			Row::PayeeMapping(row) => Row::PayeeMapping(PayeeMapping {
				to: other_payee.clone(),
				..row
			}),
			Row::Rule(row) => Row::Rule(Rule {
				conditions_op: ConditionsOp::Or,
				..row
			}),
			Row::Schedule(row) => Row::Schedule(Schedule {
				next_date: Some(CivilDate::new(2027, 1, 1).unwrap()),
				completed: true,
				..row
			}),
			Row::Transaction(row) => Row::Transaction(Transaction {
				notes: Some("Six months".to_owned()),
				..row
			}),
			Row::Tag(row) => Row::Tag(Tag {
				color: Some("#336699".to_owned()),
				..row
			}),
			Row::BudgetCell(row) => Row::BudgetCell(BudgetCell {
				carryover: true,
				long_goal: true,
				..row
			}),
			Row::Hold(row) => Row::Hold(Hold {
				amount: None,
				..row
			}),
			Row::Note(row) => Row::Note(Note { text: None, ..row }),
			Row::CategoryMeta(row) => Row::CategoryMeta(CategoryMetaRow {
				meta: CategoryMeta {
					tax_bucket: Some("hsa".to_owned()),
					..row.meta
				},
				..row
			}),
			Row::AccountMeta(row) => Row::AccountMeta(AccountMetaRow {
				meta: AccountMeta {
					lender: Some("Credit union".to_owned()),
					..row.meta
				},
				..row
			}),
			Row::TransactionMeta(row) => Row::TransactionMeta(TransactionMetaRow {
				meta: TransactionMeta {
					adjustment: true,
					needs_actual: Some("formula action".to_owned()),
				},
				..row
			}),
			Row::TaxTag(row) => Row::TaxTag(veritas_domain::TaxTag {
				bucket: "medical".to_owned(),
				..row
			}),
			Row::Goal(row) => Row::Goal(Goal {
				status: GoalStatus::Reached,
				reached_month: Some(month),
				..row
			}),
		})
		.collect();
	run(&mut data, edited);

	assert_eq!(seen.into_iter().collect::<Vec<_>>(), {
		let mut all = EVERY_VARIANT.to_vec();
		all.sort_unstable();
		all
	});
	assert_eq!(store.load().unwrap().data, data);
}
