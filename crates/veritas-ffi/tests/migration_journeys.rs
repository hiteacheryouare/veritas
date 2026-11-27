#![allow(
	clippy::unwrap_used,
	clippy::expect_used,
	clippy::panic,
	reason = "integration tests fail by panicking"
)]

//! Upgrade to Veritas features through the exported API
//! (docs/finance/migration.md): MIG-7.3's lossless export and undo, and
//! MIG-7.4's Revert after a reopen, on the all-features fixture.

#[path = "support/children.rs"]
mod children;

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::Thread;

use serde_json::Value;
use veritas_ffi::{
	CancelToken, Command, MigrationConfidence, MigrationConversion, MigrationEdits, MigrationItem,
	MigrationRevert, MigrationSelection, Month, Session,
};

const TODAY: &str = "2026-11-13";
const NOVEMBER: Month = Month {
	year: 2026,
	month: 11,
};
const GROCERIES: &str = "fc093a41-2d11-4d98-a211-e5db92205f23";

struct Unpark(Thread);

impl Wake for Unpark {
	fn wake(self: Arc<Self>) {
		self.0.unpark();
	}
}

fn block_on<F: Future>(future: F) -> F::Output {
	let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
	let mut context = Context::from_waker(&waker);
	let mut future = pin!(future);
	loop {
		if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
			return output;
		}
		std::thread::park();
	}
}

struct Scratch(PathBuf);

impl Scratch {
	fn new(label: &str) -> Self {
		static NEXT: AtomicUsize = AtomicUsize::new(0);
		let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
			"ffi-migration-{label}-{}-{}",
			std::process::id(),
			NEXT.fetch_add(1, Ordering::Relaxed)
		));
		let _ = std::fs::remove_dir_all(&path);
		std::fs::create_dir_all(&path).unwrap();
		Scratch(path)
	}

	fn budget(&self) -> String {
		self.0.join("budget").display().to_string()
	}

	fn join(&self, name: &str) -> String {
		self.0.join(name).display().to_string()
	}
}

impl Drop for Scratch {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.0);
	}
}

fn import(scratch: &Scratch) -> Arc<Session> {
	let fixture =
		Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/all-features.zip");
	let session = block_on(Session::import_actual_zip(
		fixture.display().to_string(),
		scratch.budget(),
		CancelToken::new(),
	))
	.unwrap();
	session.budget_month(NOVEMBER, TODAY.to_owned()).unwrap();
	session
}

fn reopen(scratch: &Scratch, session: Arc<Session>) -> Arc<Session> {
	session.close().unwrap();
	let session = children::while_no_child_starts(|| {
		block_on(Session::open(scratch.budget(), CancelToken::new())).unwrap()
	});
	session.budget_month(NOVEMBER, TODAY.to_owned()).unwrap();
	session
}

/// Every Actual table of an export, row by row, keyed by primary key.
fn actual_tables(session: &Session, zip: &str) -> BTreeMap<String, BTreeMap<String, Value>> {
	block_on(session.export_actual_zip(zip.to_owned(), CancelToken::new())).unwrap();
	let dump: Value = serde_json::from_slice(
		&veritas_interop::canonical_dump_of_actual_tables(Path::new(zip)).unwrap(),
	)
	.unwrap();
	let mut tables = BTreeMap::new();
	for (name, table) in dump["tables"].as_object().unwrap() {
		let keys: Vec<String> = table["primaryKey"]
			.as_array()
			.map(|keys| {
				keys.iter()
					.filter_map(|key| key.as_str().map(str::to_owned))
					.collect()
			})
			.unwrap_or_default();
		let rows = table["rows"]
			.as_array()
			.unwrap()
			.iter()
			.map(|row| {
				let key = if keys.is_empty() {
					row.to_string()
				} else {
					keys.iter()
						.map(|key| row[key].to_string())
						.collect::<Vec<_>>()
						.join("|")
				};
				(key, row.clone())
			})
			.collect();
		tables.insert(name.clone(), rows);
	}
	tables
}

/// (table, row key, changed column) for every difference. A row only in
/// `after` reads as column `+`, only in `before` as `-`.
fn differences(
	before: &BTreeMap<String, BTreeMap<String, Value>>,
	after: &BTreeMap<String, BTreeMap<String, Value>>,
) -> BTreeSet<(String, String, String)> {
	let mut found = BTreeSet::new();
	let names: BTreeSet<&String> = before.keys().chain(after.keys()).collect();
	let empty = BTreeMap::new();
	for name in names {
		let (old, new) = (
			before.get(name).unwrap_or(&empty),
			after.get(name).unwrap_or(&empty),
		);
		let keys: BTreeSet<&String> = old.keys().chain(new.keys()).collect();
		for key in keys {
			match (old.get(key), new.get(key)) {
				(Some(_), None) => {
					found.insert((name.clone(), key.clone(), "-".to_owned()));
				}
				(None, Some(_)) => {
					found.insert((name.clone(), key.clone(), "+".to_owned()));
				}
				(Some(old), Some(new)) => {
					for (column, value) in new.as_object().unwrap() {
						if old.get(column) != Some(value) {
							found.insert((name.clone(), key.clone(), column.clone()));
						}
					}
				}
				(None, None) => {}
			}
		}
	}
	found
}

fn quoted(id: &str) -> String {
	format!("\"{id}\"")
}

#[test]
fn mig_7_3_apply_changes_only_the_allowed_actual_rows_and_undo_puts_them_back() {
	let scratch = Scratch::new("lossless");
	let session = import(&scratch);
	session
		.perform(Command::SetCategoryNote {
			category_id: GROCERIES.to_owned(),
			text: "#goal 500".to_owned(),
		})
		.unwrap();
	let before = actual_tables(&session, &scratch.join("before.zip"));

	let suggestions = session.migration_suggestions(TODAY.to_owned()).unwrap();
	let goal_key = format!("goal:{GROCERIES}");
	let goal = suggestions
		.iter()
		.find(|suggestion| suggestion.key == goal_key)
		.expect("the #goal line makes Groceries a goal suggestion");
	assert_eq!(goal.confidence, MigrationConfidence::PreTicked);
	assert_eq!(goal.proposal.target, Some(50_000));
	assert!(goal.target_in_actual);
	let bills: Vec<&str> = suggestions
		.iter()
		.filter(|suggestion| {
			suggestion.conversion == MigrationConversion::Bill
				&& suggestion.confidence == MigrationConfidence::PreTicked
		})
		.map(|suggestion| suggestion.key.as_str())
		.collect();
	let mut items = vec![MigrationItem {
		key: goal_key.clone(),
		edits: MigrationEdits::default(),
	}];
	items.extend(bills.iter().map(|key| MigrationItem {
		key: (*key).to_owned(),
		edits: MigrationEdits::default(),
	}));
	session
		.apply_migration(
			MigrationSelection {
				items,
				consent_show_goals_group: true,
			},
			TODAY.to_owned(),
		)
		.unwrap();

	let after = actual_tables(&session, &scratch.join("after.zip"));
	let groceries = quoted(GROCERIES);
	for (table, key, column) in differences(&before, &after) {
		let allowed = match table.as_str() {
			"categories" => {
				key == groceries && (column == "cat_group" || column == "sort_order")
					|| column == "sort_order"
			}
			"category_groups" => column == "+" || column == "hidden",
			_ => false,
		};
		assert!(allowed, "{table} {key} {column} isn't a MIG-2.1 write");
	}
	assert_ne!(
		before["categories"][&groceries]["cat_group"],
		after["categories"][&groceries]["cat_group"]
	);

	session.undo().unwrap();
	let undone = actual_tables(&session, &scratch.join("undone.zip"));
	for (table, key, column) in differences(&before, &undone) {
		let retired = table == "category_groups"
			&& column == "+"
			&& undone["category_groups"][&key]["tombstone"] == 1;
		assert!(retired, "after undo, {table} {key} {column} differs");
	}
	assert!(
		session.applied_migrations().unwrap().is_empty(),
		"undo removes the records"
	);
}

#[test]
fn mig_7_4_a_goal_reverts_after_a_reopen_from_its_record_alone() {
	let scratch = Scratch::new("revert");
	let session = import(&scratch);
	session
		.perform(Command::SetCategoryNote {
			category_id: GROCERIES.to_owned(),
			text: "#goal 500".to_owned(),
		})
		.unwrap();
	let before = actual_tables(&session, &scratch.join("before.zip"));
	let key = format!("goal:{GROCERIES}");
	session
		.apply_migration(
			MigrationSelection {
				items: vec![MigrationItem {
					key: key.clone(),
					edits: MigrationEdits::default(),
				}],
				consent_show_goals_group: true,
			},
			TODAY.to_owned(),
		)
		.unwrap();
	let session = reopen(&scratch, session);
	assert!(
		!session.state().unwrap().can_undo,
		"undo lives in memory only"
	);
	let applied = session.applied_migrations().unwrap();
	assert_eq!(
		applied
			.iter()
			.map(|item| item.key.as_str())
			.collect::<Vec<_>>(),
		[key.as_str()]
	);
	match session
		.revert_migration(key.clone(), None, false, TODAY.to_owned())
		.unwrap()
	{
		MigrationRevert::Reverted { notes, .. } => assert!(notes.is_empty()),
		other => panic!("{other:?}"),
	}
	let reverted = actual_tables(&session, &scratch.join("reverted.zip"));
	let groceries = quoted(GROCERIES);
	assert_eq!(
		before["categories"][&groceries]["cat_group"],
		reverted["categories"][&groceries]["cat_group"]
	);
	assert!(session.applied_migrations().unwrap().is_empty());
	assert!(
		session
			.revert_migration(key, None, false, TODAY.to_owned())
			.is_err(),
		"a second Revert finds nothing"
	);
}

#[test]
fn mig_1_8_offering_and_dismissing_leave_no_undo_step() {
	let scratch = Scratch::new("settings");
	let session = import(&scratch);
	session
		.perform(Command::SetCategoryNote {
			category_id: GROCERIES.to_owned(),
			text: "#goal 500".to_owned(),
		})
		.unwrap();
	session.undo().unwrap();
	assert!(!session.migration_offered().unwrap());
	session.mark_migration_offered().unwrap();
	assert!(session.migration_offered().unwrap());
	session.redo().unwrap();
	let key = format!("goal:{GROCERIES}");
	session.set_migration_dismissed(key.clone(), true).unwrap();
	session.undo().unwrap();
	assert!(session.migration_offered().unwrap(), "undo skips the offer");
	session.redo().unwrap();
	let dismissed = session
		.migration_suggestions(TODAY.to_owned())
		.unwrap()
		.into_iter()
		.find(|suggestion| suggestion.key == key)
		.unwrap();
	assert!(dismissed.dismissed, "and the dismissal");
	let session = reopen(&scratch, session);
	assert!(session.migration_offered().unwrap(), "it survives a reopen");
}

/// MIG-7.5 (and MIG-2.3): undo leaves a `zero_budgets` cell the command
/// created with no goal figure, while a cell that existed gets its goal back
/// exactly, as `createGoal` and `retargetGoal` write them.
#[test]
fn mig_7_5_undo_clears_a_created_cells_goal_and_restores_an_existing_one() {
	let scratch = Scratch::new("cells");
	let session = import(&scratch);
	let before = actual_tables(&session, &scratch.join("before.zip"));
	session
		.perform(Command::CreateGoal {
			name: "Boat".to_owned(),
			target: 120_000,
			target_date: None,
			icon: None,
			color: None,
		})
		.unwrap();
	session.undo().unwrap();
	let undone = actual_tables(&session, &scratch.join("undone.zip"));
	let created: Vec<&Value> = undone["zero_budgets"]
		.iter()
		.filter(|(key, _)| !before["zero_budgets"].contains_key(*key))
		.map(|(_, row)| row)
		.collect();
	assert!(!created.is_empty(), "the goal wrote cells of its own");
	for row in created {
		assert_eq!(row["goal"], Value::Null, "{row}");
		assert_eq!(row["long_goal"], 0, "{row}");
		assert_eq!(row["amount"], 0, "{row}");
	}
	for (key, row) in &before["zero_budgets"] {
		assert_eq!(
			&undone["zero_budgets"][key], row,
			"an existing cell is as it was"
		);
	}

	session.redo().unwrap();
	let goal = session
		.goals(TODAY.to_owned())
		.unwrap()
		.goals
		.into_iter()
		.find(|goal| goal.name == "Boat")
		.unwrap();
	let created = actual_tables(&session, &scratch.join("created.zip"));
	session
		.perform(Command::RetargetGoal {
			goal_id: goal.id.clone(),
			target: 150_000,
			target_date: None,
		})
		.unwrap();
	session.undo().unwrap();
	let retargeted_back = actual_tables(&session, &scratch.join("retargeted.zip"));
	assert_eq!(
		created["zero_budgets"], retargeted_back["zero_budgets"],
		"undoing a re-target puts every existing cell's goal back"
	);
}
