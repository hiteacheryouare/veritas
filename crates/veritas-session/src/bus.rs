use std::collections::BTreeSet;
use std::sync::Arc;

use veritas_domain::{BudgetData, CoreError, ReadOnlyReason};

use crate::change::{Change, EntityRef, Row};

/// A user action. `plan` validates against the loaded budget and returns the
/// rows that should exist afterwards, without touching anything.
pub trait Command {
	fn plan(&self, data: &BudgetData) -> Result<Vec<Row>, CoreError>;
}

impl<F> Command for F
where
	F: Fn(&BudgetData) -> Result<Vec<Row>, CoreError>,
{
	fn plan(&self, data: &BudgetData) -> Result<Vec<Row>, CoreError> {
		self(data)
	}
}

/// Durable storage for the loaded budget. One command is one transaction.
pub trait Store {
	type Transaction<'a>: StoreTransaction
	where
		Self: 'a;

	fn begin(&mut self) -> Result<Self::Transaction<'_>, CoreError>;
}

/// If `apply` or `commit` fails, nothing from this transaction may persist.
pub trait StoreTransaction {
	fn apply(&mut self, change: &Change) -> Result<(), CoreError>;
	fn commit(self) -> Result<(), CoreError>;
	fn rollback(self) -> Result<(), CoreError>;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangeSet {
	pub entities: BTreeSet<EntityRef>,
}

impl ChangeSet {
	fn of(changes: &[Change]) -> Self {
		ChangeSet {
			entities: changes.iter().map(Change::entity).collect(),
		}
	}

	pub fn is_empty(&self) -> bool {
		self.entities.is_empty()
	}
}

pub trait ChangeListener: Send + Sync {
	fn changed(&self, changes: &ChangeSet);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Subscription(u64);

/// Applies commands to memory and storage together, keeps unlimited undo and
/// redo, and tells subscribers which rows changed after every commit.
pub struct CommandBus<S: Store> {
	data: BudgetData,
	store: S,
	read_only: Option<ReadOnlyReason>,
	undo: Vec<Vec<Change>>,
	redo: Vec<Vec<Change>>,
	listeners: Vec<(Subscription, Arc<dyn ChangeListener>)>,
	next_subscription: u64,
}

#[expect(
	clippy::large_enum_variant,
	reason = "lives only while one command is applied"
)]
enum Intent<'a> {
	Put(Row),
	Undo(&'a Change),
	Redo(&'a Change),
}

impl Intent<'_> {
	fn against(self, data: &BudgetData) -> Option<Change> {
		match self {
			Intent::Put(row) => Some(Change::put(data, row)),
			Intent::Undo(change) => change.undo(data),
			Intent::Redo(change) => Some(change.redo(data)),
		}
	}
}

impl<S: Store> CommandBus<S> {
	pub fn new(data: BudgetData, store: S, read_only: Option<ReadOnlyReason>) -> Self {
		CommandBus {
			data,
			store,
			read_only,
			undo: Vec::new(),
			redo: Vec::new(),
			listeners: Vec::new(),
			next_subscription: 0,
		}
	}

	pub fn data(&self) -> &BudgetData {
		&self.data
	}

	pub fn store(&self) -> &S {
		&self.store
	}

	pub fn store_mut(&mut self) -> &mut S {
		&mut self.store
	}

	pub fn read_only(&self) -> Option<ReadOnlyReason> {
		self.read_only
	}

	pub fn can_undo(&self) -> bool {
		!self.undo.is_empty()
	}

	pub fn can_redo(&self) -> bool {
		!self.redo.is_empty()
	}

	pub fn perform<C: Command + ?Sized>(&mut self, command: &C) -> Result<ChangeSet, CoreError> {
		self.ensure_writable()?;
		let rows = command.plan(&self.data)?;
		let changes = transact(
			&mut self.data,
			&mut self.store,
			rows.into_iter().map(Intent::Put),
		)?;
		if changes.is_empty() {
			return Ok(ChangeSet::default());
		}
		let changed = ChangeSet::of(&changes);
		self.undo.push(changes);
		self.redo.clear();
		self.notify(&changed);
		Ok(changed)
	}

	pub fn undo(&mut self) -> Result<ChangeSet, CoreError> {
		self.ensure_writable()?;
		let Some(step) = self.undo.pop() else {
			return Ok(ChangeSet::default());
		};
		match transact(
			&mut self.data,
			&mut self.store,
			step.iter().rev().map(Intent::Undo),
		) {
			Ok(changes) => {
				let changed = ChangeSet::of(&changes);
				self.redo.push(step);
				self.notify(&changed);
				Ok(changed)
			}
			Err(error) => {
				self.undo.push(step);
				Err(error)
			}
		}
	}

	pub fn redo(&mut self) -> Result<ChangeSet, CoreError> {
		self.ensure_writable()?;
		let Some(step) = self.redo.pop() else {
			return Ok(ChangeSet::default());
		};
		match transact(
			&mut self.data,
			&mut self.store,
			step.iter().map(Intent::Redo),
		) {
			Ok(changes) => {
				let changed = ChangeSet::of(&changes);
				self.undo.push(step);
				self.notify(&changed);
				Ok(changed)
			}
			Err(error) => {
				self.redo.push(step);
				Err(error)
			}
		}
	}

	pub fn subscribe(&mut self, listener: Arc<dyn ChangeListener>) -> Subscription {
		let subscription = Subscription(self.next_subscription);
		self.next_subscription += 1;
		self.listeners.push((subscription, listener));
		subscription
	}

	pub fn unsubscribe(&mut self, subscription: Subscription) {
		self.listeners.retain(|(id, _)| *id != subscription);
	}

	fn ensure_writable(&self) -> Result<(), CoreError> {
		match self.read_only {
			Some(reason) => Err(CoreError::ReadOnly(reason)),
			None => Ok(()),
		}
	}

	fn notify(&self, changed: &ChangeSet) {
		if changed.is_empty() {
			return;
		}
		for (_, listener) in &self.listeners {
			listener.changed(changed);
		}
	}
}

/// Each change is built against memory as it stands, written to storage, then
/// applied to memory. Any failure puts memory back exactly as it was.
fn transact<'a, S: Store>(
	data: &mut BudgetData,
	store: &mut S,
	intents: impl IntoIterator<Item = Intent<'a>>,
) -> Result<Vec<Change>, CoreError> {
	let mut transaction = store.begin()?;
	let mut applied: Vec<Change> = Vec::new();
	for intent in intents {
		let Some(change) = intent.against(data) else {
			continue;
		};
		if change.is_noop() {
			continue;
		}
		if let Err(error) = transaction.apply(&change) {
			revert(data, &applied);
			// The write already failed; that error is the one worth reporting.
			let _ = transaction.rollback();
			return Err(error);
		}
		change.apply(data);
		applied.push(change);
	}
	if let Err(error) = transaction.commit() {
		revert(data, &applied);
		return Err(error);
	}
	Ok(applied)
}

fn revert(data: &mut BudgetData, applied: &[Change]) {
	for change in applied.iter().rev() {
		change.revert(data);
	}
}
