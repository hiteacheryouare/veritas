use std::collections::BTreeSet;
use std::sync::Arc;

use veritas_domain::{
	BudgetData, Change, CoreError, EntityRef, ReadOnlyReason, Row, Store, StoreTransaction,
};

use crate::lock::WriterLock;
use crate::snapshot::SnapshotReason;

/// A user action. `plan` validates against the loaded budget and returns the
/// rows that should exist afterwards, without touching anything.
pub trait Command {
	fn plan(&self, data: &BudgetData) -> Result<Vec<Row>, CoreError>;

	/// Bulk edits and rule applies return a reason, and the session snapshots
	/// the budget before running them.
	fn snapshot_first(&self) -> Option<SnapshotReason> {
		None
	}
}

impl<F> Command for F
where
	F: Fn(&BudgetData) -> Result<Vec<Row>, CoreError>,
{
	fn plan(&self, data: &BudgetData) -> Result<Vec<Row>, CoreError> {
		self(data)
	}
}

/// What a commit changed. `everything` is set when the whole budget was
/// reloaded, after a restore for example, and every view should refetch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangeSet {
	pub entities: BTreeSet<EntityRef>,
	pub everything: bool,
}

impl ChangeSet {
	fn of(changes: &[Change]) -> Self {
		ChangeSet {
			entities: changes.iter().map(Change::entity).collect(),
			everything: false,
		}
	}

	pub fn everything() -> Self {
		ChangeSet {
			entities: BTreeSet::new(),
			everything: true,
		}
	}

	pub fn is_empty(&self) -> bool {
		!self.everything && self.entities.is_empty()
	}
}

pub trait ChangeListener: Send + Sync {
	fn changed(&self, changes: &ChangeSet);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Subscription(u64);

/// The subscribers to one budget. They outlive any one bus, so they carry
/// over when the session reopens the budget.
#[derive(Default)]
pub struct Listeners {
	entries: Vec<(Subscription, Arc<dyn ChangeListener>)>,
	next: u64,
}

impl Listeners {
	pub fn subscribe(&mut self, listener: Arc<dyn ChangeListener>) -> Subscription {
		let subscription = Subscription(self.next);
		self.next += 1;
		self.entries.push((subscription, listener));
		subscription
	}

	pub fn unsubscribe(&mut self, subscription: Subscription) {
		self.entries.retain(|(id, _)| *id != subscription);
	}

	pub fn notify(&self, changed: &ChangeSet) {
		if changed.is_empty() {
			return;
		}
		for (_, listener) in &self.entries {
			listener.changed(changed);
		}
	}
}

/// What a bus may do. Writing takes the budget's lock, so only the process
/// that holds it can build a writable bus.
pub enum Permit<'a> {
	Write(&'a WriterLock),
	ReadOnly(ReadOnlyReason),
}

/// Applies commands to memory and storage together, keeps unlimited undo and
/// redo, and tells subscribers which rows changed after every commit.
pub struct CommandBus<S: Store> {
	data: BudgetData,
	store: S,
	read_only: Option<ReadOnlyReason>,
	undo: Vec<Vec<Change>>,
	redo: Vec<Vec<Change>>,
	listeners: Listeners,
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
	pub fn new(data: BudgetData, store: S, permit: Permit<'_>) -> Self {
		CommandBus::with_listeners(data, store, permit, Listeners::default())
	}

	pub fn with_listeners(
		data: BudgetData,
		store: S,
		permit: Permit<'_>,
		listeners: Listeners,
	) -> Self {
		CommandBus {
			data,
			store,
			read_only: match permit {
				Permit::Write(_) => None,
				Permit::ReadOnly(reason) => Some(reason),
			},
			undo: Vec::new(),
			redo: Vec::new(),
			listeners,
		}
	}

	/// Closes the store and hands back the subscribers.
	pub fn into_listeners(self) -> Listeners {
		self.listeners
	}

	pub fn listeners(&self) -> &Listeners {
		&self.listeners
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

	/// Stops all writes, for example when a background check finds damage.
	/// There is no way back without reopening with the lock.
	pub fn make_read_only(&mut self, reason: ReadOnlyReason) {
		self.read_only = Some(reason);
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
		self.listeners.subscribe(listener)
	}

	pub fn unsubscribe(&mut self, subscription: Subscription) {
		self.listeners.unsubscribe(subscription);
	}

	fn ensure_writable(&self) -> Result<(), CoreError> {
		match self.read_only {
			Some(reason) => Err(CoreError::ReadOnly(reason)),
			None => Ok(()),
		}
	}

	fn notify(&self, changed: &ChangeSet) {
		self.listeners.notify(changed);
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
