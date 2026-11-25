use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::hash::{BuildHasherDefault, Hasher};
use std::num::NonZero;

use veritas_domain::{
	AccountId, BudgetData, CategoryId, CategoryKind, CivilDate, Money, Payee, PayeeId, SplitRole,
	Transaction, TransactionId, TransactionKind,
};

use crate::Period;

/// The notes Actual's reconcile flow writes on the adjustment it creates. It
/// is localized in Actual, so matching it is best effort.
pub const RECONCILIATION_NOTE: &str = "Reconciliation balance adjustment";

/// Which row of the classification decision table decided a row's kind. The
/// variants are in table order, starting at row 2 (split parents are never
/// classified).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Decision {
	StartingBalance,
	Reconciliation,
	OnBudgetTransfer,
	OffBudgetTransfer,
	DebtPrincipalTransfer,
	SavingsTransfer,
	CapitalizedExpense,
	CapitalizedIncome,
	UncategorizedTransfer,
	OffBudgetRevaluation,
	Income,
	Expense,
	MisfiledTransferCategory,
	Uncategorized,
}

impl Decision {
	pub fn table_row(self) -> u8 {
		match self {
			Decision::StartingBalance => 2,
			Decision::Reconciliation => 3,
			Decision::OnBudgetTransfer => 4,
			Decision::OffBudgetTransfer => 5,
			Decision::DebtPrincipalTransfer => 6,
			Decision::SavingsTransfer => 7,
			Decision::CapitalizedExpense => 8,
			Decision::CapitalizedIncome => 9,
			Decision::UncategorizedTransfer => 10,
			Decision::OffBudgetRevaluation => 11,
			Decision::Income => 12,
			Decision::Expense => 13,
			Decision::MisfiledTransferCategory => 14,
			Decision::Uncategorized => 15,
		}
	}

	pub fn kind(self) -> TransactionKind {
		match self {
			Decision::StartingBalance => TransactionKind::StartingBalance,
			Decision::Reconciliation | Decision::OffBudgetRevaluation => {
				TransactionKind::Adjustment
			}
			Decision::OnBudgetTransfer
			| Decision::OffBudgetTransfer
			| Decision::UncategorizedTransfer => TransactionKind::InternalTransfer,
			Decision::DebtPrincipalTransfer => TransactionKind::DebtPrincipal,
			Decision::SavingsTransfer => TransactionKind::SavingsContribution,
			Decision::CapitalizedIncome | Decision::Income => TransactionKind::Income,
			Decision::CapitalizedExpense
			| Decision::Expense
			| Decision::MisfiledTransferCategory
			| Decision::Uncategorized => TransactionKind::Expense,
		}
	}

	pub fn adjustment_source(self) -> Option<AdjustmentSource> {
		match self {
			Decision::Reconciliation => Some(AdjustmentSource::Reconciliation),
			Decision::OffBudgetRevaluation => Some(AdjustmentSource::OffBudgetRevaluation),
			_ => None,
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AdjustmentSource {
	Reconciliation,
	OffBudgetRevaluation,
}

/// A mirror leg is the off-budget side of an on↔off pair. It copies the
/// on-budget leg's kind, counts in balances and is skipped by every flow total.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Leg {
	Primary,
	Mirror,
}

/// Problems surfaced next to a row. They never change its kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Flags {
	pub needs_review: bool,
	pub misfiled: bool,
	pub kind_conflict: bool,
	pub unpaired: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClassifiedRow {
	pub id: TransactionId,
	pub parent: Option<TransactionId>,
	pub account: AccountId,
	pub date: CivilDate,
	pub amount: Money,
	/// After `category_mapping`, so merged categories report under their target.
	pub category: Option<CategoryId>,
	pub on_budget: bool,
	/// The other account of a transfer, from the payee's `transfer_acct`.
	pub transfer_account: Option<AccountId>,
	/// The other leg of a paired transfer. Its date may differ, since Actual
	/// does not keep the two legs' dates in sync.
	pub partner: Option<Partner>,
	pub decision: Decision,
	pub leg: Leg,
	pub flags: Flags,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Partner {
	pub id: TransactionId,
	pub date: CivilDate,
}

impl ClassifiedRow {
	pub fn kind(&self) -> TransactionKind {
		self.decision.kind()
	}

	pub fn adjustment_source(&self) -> Option<AdjustmentSource> {
		self.decision.adjustment_source()
	}

	pub fn is_primary(&self) -> bool {
		self.leg == Leg::Primary
	}

	/// A paired leg inside `period` whose partner is dated outside it.
	pub fn in_transit(&self, period: Period) -> bool {
		period.contains(self.date)
			&& self
				.partner
				.as_ref()
				.is_some_and(|partner| !period.contains(partner.date))
	}
}

/// Every live, non-parent row of a budget with its kind, sorted by date then id.
#[derive(Clone, Debug, Default)]
pub struct Classification {
	rows: Vec<ClassifiedRow>,
	/// Positions in `rows`, in id order, for lookup by id.
	by_id: Vec<usize>,
	orphans: Vec<TransactionId>,
}

impl Classification {
	pub fn rows(&self) -> &[ClassifiedRow] {
		&self.rows
	}

	pub fn get(&self, id: &TransactionId) -> Option<&ClassifiedRow> {
		let found = self.find(id)?;
		self.by_id
			.get(found)
			.and_then(|&position| self.rows.get(position))
	}

	/// Where `id` sits in `by_id`.
	fn find(&self, id: &TransactionId) -> Option<usize> {
		self.by_id
			.binary_search_by(|&position| {
				self.rows
					.get(position)
					.map_or(std::cmp::Ordering::Less, |row| row.id.cmp(id))
			})
			.ok()
	}

	/// Brings the classification up to date with `budget` after only the
	/// transactions in `changed` were written: added, edited or deleted.
	/// Everything else a classification reads (accounts, categories, payees,
	/// their mappings and transaction metadata) must be as it was; when it
	/// isn't, [`classify`] afresh. The result is what [`classify`] gives.
	pub fn reclassify(&mut self, budget: &BudgetData, changed: &BTreeSet<TransactionId>) {
		let affected = affected_rows(budget, changed);
		if affected.len() > RECLASSIFY_AT_MOST {
			*self = classify(budget);
			return;
		}
		let (rows, orphans) = classify_rows(budget, &affected);
		for id in &affected {
			self.remove(id);
		}
		for row in rows {
			self.insert(row);
		}
		for orphan in orphans {
			if let Err(at) = self.orphans.binary_search(&orphan) {
				self.orphans.insert(at, orphan);
			}
		}
	}

	fn remove(&mut self, id: &TransactionId) {
		if let Ok(at) = self.orphans.binary_search(id) {
			self.orphans.remove(at);
		}
		let Some(found) = self.find(id) else {
			return;
		};
		let position = self.by_id.remove(found);
		if position < self.rows.len() {
			self.rows.remove(position);
		}
		for slot in &mut self.by_id {
			if *slot > position {
				*slot -= 1;
			}
		}
	}

	fn insert(&mut self, row: ClassifiedRow) {
		let position = self
			.rows
			.partition_point(|other| (other.date, &other.id) < (row.date, &row.id));
		let found = self
			.by_id
			.partition_point(|&other| self.rows.get(other).is_some_and(|other| other.id < row.id));
		for slot in &mut self.by_id {
			if *slot >= position {
				*slot += 1;
			}
		}
		self.by_id.insert(found, position);
		self.rows.insert(position, row);
	}

	pub fn within(&self, period: Period) -> &[ClassifiedRow] {
		let start = self.rows.partition_point(|row| row.date < period.first());
		let end = self.rows.partition_point(|row| row.date <= period.last());
		self.rows.get(start..end).unwrap_or_default()
	}

	/// Every row dated on or before `date`: what an account balance on that
	/// date sums.
	pub fn through(&self, date: CivilDate) -> &[ClassifiedRow] {
		let end = self.rows.partition_point(|row| row.date <= date);
		self.rows.get(..end).unwrap_or_default()
	}

	pub fn before(&self, date: CivilDate) -> &[ClassifiedRow] {
		let end = self.rows.partition_point(|row| row.date < date);
		self.rows.get(..end).unwrap_or_default()
	}

	/// Live rows whose own account is missing from the budget. No account holds
	/// them, so they are never classified, count in no total, and each one
	/// needs review.
	pub fn orphans(&self) -> &[TransactionId] {
		&self.orphans
	}

	/// On↔off transfers counted as spending only because their category has
	/// no Veritas kind yet. The UI shows this count and the one-time wizard
	/// that assigns kinds to those categories.
	pub fn transfers_counted_as_spending<'a>(
		&'a self,
		budget: &'a BudgetData,
	) -> impl Iterator<Item = &'a ClassifiedRow> + 'a {
		self.rows.iter().filter(|row| {
			row.decision == Decision::CapitalizedExpense
				&& row.is_primary()
				&& row.category.as_ref().is_some_and(|category| {
					budget
						.category_meta
						.get(category)
						.and_then(|meta| meta.kind)
						.is_none()
				})
		})
	}

	/// CLS-4.2: each category that [`Self::transfers_counted_as_spending`]
	/// files under, with how many of those transfers it holds. These are the
	/// categories the one-time wizard asks about.
	pub fn categories_to_review(&self, budget: &BudgetData) -> BTreeMap<CategoryId, usize> {
		let mut counts = BTreeMap::new();
		for row in self.transfers_counted_as_spending(budget) {
			if let Some(category) = &row.category {
				*counts.entry(category.clone()).or_insert(0) += 1;
			}
		}
		counts
	}
}

/// Below this many transactions a classification runs on one thread.
const PARALLEL_FROM: usize = 8192;
const MOST_THREADS: usize = 4;

/// `work` over `items` in consecutive chunks, one per thread, with each
/// chunk's offset into `items`. The results come back in chunk order.
fn in_chunks<'i, T: Sync, R: Send>(
	items: &'i [T],
	threads: usize,
	work: impl Fn(usize, &'i [T]) -> R + Sync,
) -> Vec<R> {
	if threads <= 1 || items.is_empty() {
		return vec![work(0, items)];
	}
	let size = items.len().div_ceil(threads);
	let work = &work;
	std::thread::scope(|scope| {
		let running: Vec<_> = items
			.chunks(size)
			.enumerate()
			.map(|(index, chunk)| {
				let offset = index * size;
				(offset, chunk, scope.spawn(move || work(offset, chunk)))
			})
			.collect();
		running
			.into_iter()
			.map(|(offset, chunk, thread)| thread.join().unwrap_or_else(|_| work(offset, chunk)))
			.collect()
	})
}

pub fn classify(budget: &BudgetData) -> Classification {
	let threads = if budget.transactions.len() < PARALLEL_FROM {
		1
	} else {
		std::thread::available_parallelism()
			.map_or(1, NonZero::get)
			.min(MOST_THREADS)
	};
	classify_on(budget, threads)
}

/// [`classify`] with its row-by-row work split across `threads` threads.
/// The result is the same for any count.
pub fn classify_on(budget: &BudgetData, threads: usize) -> Classification {
	let transactions: Vec<&Transaction> = budget.transactions.values().collect();
	let mut orphans = Vec::new();
	let mut candidates = Vec::with_capacity(transactions.len());
	for (chunk_candidates, chunk_orphans) in in_chunks(&transactions, threads, |_, chunk| {
		let mut lookups = Lookups::new(budget);
		let mut candidates = Vec::with_capacity(chunk.len());
		let mut orphans = Vec::new();
		for &transaction in chunk {
			if !is_row(budget, transaction) {
				continue;
			}
			match Candidate::new(budget, &mut lookups, transaction) {
				Some(candidate) => candidates.push(candidate),
				None => orphans.push(transaction.id.clone()),
			}
		}
		(candidates, orphans)
	}) {
		candidates.extend(chunk_candidates);
		orphans.extend(chunk_orphans);
	}

	let mut decided: Vec<Option<ClassifiedRow>> = Vec::with_capacity(candidates.len());
	let mut mirrors = Vec::new();
	for (chunk_decided, chunk_mirrors) in in_chunks(&candidates, threads, |offset, chunk| {
		let mut decided = Vec::with_capacity(chunk.len());
		let mut mirrors = Vec::new();
		for (index, candidate) in chunk.iter().enumerate() {
			let partner = partner(&candidates, candidate);
			let partner_candidate = partner.and_then(|other| candidates.get(other));
			let row = ClassifiedRow {
				partner: partner_candidate.map(|other| Partner {
					id: other.transaction.id.clone(),
					date: other.transaction.date,
				}),
				..candidate.row()
			};
			match decide(candidate, partner_candidate) {
				Outcome::Decided(decision, flags) => decided.push(Some(ClassifiedRow {
					decision,
					flags,
					..row
				})),
				Outcome::Mirror => {
					decided.push(None);
					mirrors.push((offset + index, row, partner));
				}
			}
		}
		(decided, mirrors)
	}) {
		decided.extend(chunk_decided);
		mirrors.extend(chunk_mirrors);
	}
	for (position, row, primary) in mirrors {
		let primary = primary
			.and_then(|primary| decided.get(primary))
			.and_then(|primary| primary.as_ref().map(|primary| primary.decision));
		if let Some(slot) = decided.get_mut(position) {
			*slot = Some(mirrored(row, primary));
		}
	}

	// Rows arrive in id order, so sorting by date then position gives
	// date-then-id.
	let mut order: Vec<(Option<CivilDate>, usize)> = decided
		.iter()
		.enumerate()
		.map(|(position, row)| (row.as_ref().map(|row| row.date), position))
		.collect();
	order.sort_unstable();
	let mut by_id = vec![0; decided.len()];
	for (sorted, &(_, position)) in order.iter().enumerate() {
		if let Some(slot) = by_id.get_mut(position) {
			*slot = sorted;
		}
	}
	let mut slots: Vec<Option<ClassifiedRow>> = decided;
	let rows = order
		.iter()
		.filter_map(|&(_, position)| slots.get_mut(position).and_then(Option::take))
		.collect();
	Classification {
		rows,
		by_id,
		orphans,
	}
}

/// A multiply-rotate hash that takes ids eight bytes at a time. The maps
/// below are keyed by ids from the budget itself, and std's SipHash costs
/// more than the lookups they save.
#[derive(Default)]
struct IdHasher(u64);

impl Hasher for IdHasher {
	fn finish(&self) -> u64 {
		self.0
	}

	fn write(&mut self, bytes: &[u8]) {
		let (words, tail) = bytes.as_chunks::<8>();
		for &word in words {
			self.add(u64::from_le_bytes(word));
		}
		let tail = tail
			.iter()
			.fold(0, |tail, &byte| (tail << 8) | u64::from(byte));
		self.add(tail);
	}
}

impl IdHasher {
	fn add(&mut self, word: u64) {
		self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(0x517c_c1b7_2722_0a95);
	}
}

type IdMap<K, V> = HashMap<K, V, BuildHasherDefault<IdHasher>>;

/// A category after `category_mapping`, with its effective kind and kind
/// conflict (CLS-3.1).
type ResolvedCategory<'a> = (&'a CategoryId, (CategoryKind, bool));

/// A row's account, category and payee, resolved through the budget's
/// mappings. Thousands of rows share each one, so each id is resolved once
/// per classification instead of once per row.
struct Lookups<'a> {
	budget: &'a BudgetData,
	/// Whether each account is on budget. `None` when the budget lacks it.
	accounts: IdMap<&'a AccountId, Option<bool>>,
	categories: IdMap<&'a CategoryId, Option<ResolvedCategory<'a>>>,
	payees: IdMap<&'a PayeeId, Option<&'a Payee>>,
}

impl<'a> Lookups<'a> {
	fn new(budget: &'a BudgetData) -> Self {
		Lookups {
			budget,
			accounts: IdMap::default(),
			categories: IdMap::default(),
			payees: IdMap::default(),
		}
	}

	fn on_budget(&mut self, id: &'a AccountId) -> Option<bool> {
		let budget = self.budget;
		*self
			.accounts
			.entry(id)
			.or_insert_with(|| budget.accounts.get(id).map(|account| !account.off_budget))
	}

	/// The category a row reports under, after `category_mapping`, with its
	/// effective kind. `None` when it isn't one of the budget's categories.
	fn category(&mut self, id: &'a CategoryId) -> Option<ResolvedCategory<'a>> {
		let budget = self.budget;
		*self.categories.entry(id).or_insert_with(|| {
			budget
				.resolve_category(id)
				.filter(|category| budget.categories.contains_key(*category))
				.map(|category| (category, budget.effective_category_kind(category)))
		})
	}

	fn payee(&mut self, id: &'a PayeeId) -> Option<&'a Payee> {
		let budget = self.budget;
		*self.payees.entry(id).or_insert_with(|| {
			budget
				.resolve_payee(id)
				.and_then(|payee| budget.payees.get(payee))
		})
	}
}

/// Actual's `v_transactions_internal_alive` with `is_parent = 0`: not
/// tombstoned, and a child only while its parent exists and is not tombstoned.
fn is_row(budget: &BudgetData, transaction: &Transaction) -> bool {
	if transaction.tombstone {
		return false;
	}
	match &transaction.split {
		SplitRole::None => true,
		SplitRole::Parent => false,
		SplitRole::Child { parent } => budget
			.transactions
			.get(parent)
			.is_some_and(|parent| !parent.tombstone),
	}
}

struct Transfer {
	account: AccountId,
	/// A counterpart account missing from the budget counts as off-budget.
	other_on_budget: bool,
}

/// A live row and the facts the decision table reads, before pairing.
struct Candidate<'a> {
	transaction: &'a Transaction,
	category: Option<&'a CategoryId>,
	/// The category's effective kind and kind conflict (CLS-3.1).
	category_kind: Option<(CategoryKind, bool)>,
	on_budget: bool,
	transfer: Option<Transfer>,
	starting_balance: bool,
	reconciliation: bool,
}

impl<'a> Candidate<'a> {
	/// `None` when the row's own account is missing: an orphaned row.
	fn new(
		budget: &BudgetData,
		lookups: &mut Lookups<'a>,
		transaction: &'a Transaction,
	) -> Option<Self> {
		let on_budget = lookups.on_budget(&transaction.account)?;
		let category = transaction
			.category
			.as_ref()
			.and_then(|category| lookups.category(category));
		let payee = transaction
			.payee
			.as_ref()
			.and_then(|payee| lookups.payee(payee));
		let transfer = payee
			.and_then(|payee| payee.transfer_account.as_ref())
			.map(|account| Transfer {
				other_on_budget: lookups.on_budget(account).unwrap_or(false),
				account: account.clone(),
			});
		let reconciliation = budget
			.transaction_meta
			.get(&transaction.id)
			.is_some_and(|meta| meta.adjustment)
			|| (transaction.cleared
				&& payee.is_none()
				&& category.is_none()
				&& transaction.notes.as_deref() == Some(RECONCILIATION_NOTE));
		Some(Candidate {
			transaction,
			category: category.map(|(category, _)| category),
			category_kind: category.map(|(_, kind)| kind),
			on_budget,
			transfer,
			starting_balance: transaction.starting_balance,
			reconciliation,
		})
	}

	fn marked(&self) -> bool {
		self.starting_balance || self.reconciliation
	}

	fn row(&self) -> ClassifiedRow {
		let transaction = self.transaction;
		ClassifiedRow {
			id: transaction.id.clone(),
			parent: match &transaction.split {
				SplitRole::Child { parent } => Some(parent.clone()),
				SplitRole::None | SplitRole::Parent => None,
			},
			account: transaction.account.clone(),
			date: transaction.date,
			amount: transaction.amount,
			category: self.category.cloned(),
			on_budget: self.on_budget,
			transfer_account: self
				.transfer
				.as_ref()
				.map(|transfer| transfer.account.clone()),
			partner: None,
			decision: Decision::Uncategorized,
			leg: Leg::Primary,
			flags: Flags::default(),
		}
	}
}

/// The other leg, only when the link is reciprocal (each row points at the
/// other and sits in the other's transfer account) and the amounts net to
/// zero. Dates may differ.
/// `candidates` are in id order, as the budget's transactions are keyed.
fn partner(candidates: &[Candidate], candidate: &Candidate) -> Option<usize> {
	let id = partner_id(candidate)?;
	let position = candidates
		.binary_search_by(|other| other.transaction.id.cmp(id))
		.ok()?;
	pairs(candidate, candidates.get(position)?).then_some(position)
}

/// The row a transfer's `transfer` field names, unless it names itself.
fn partner_id<'c>(candidate: &'c Candidate) -> Option<&'c TransactionId> {
	candidate.transfer.as_ref()?;
	let own = candidate.transaction;
	own.transfer.as_ref().filter(|id| *id != &own.id)
}

/// Whether `other`, the row `candidate` names, is its other leg: the link is
/// reciprocal (each row points at the other and sits in the other's transfer
/// account) and the amounts net to zero. Dates may differ.
fn pairs(candidate: &Candidate, other: &Candidate) -> bool {
	let own = candidate.transaction;
	let Some(transfer) = &candidate.transfer else {
		return false;
	};
	let points_back = other.transaction.transfer.as_ref() == Some(&own.id);
	let other_points_here = other
		.transfer
		.as_ref()
		.is_some_and(|transfer| transfer.account == own.account);
	let nets_to_zero = own
		.amount
		.checked_add(other.transaction.amount)
		.is_ok_and(|sum| sum == Money::ZERO);
	points_back
		&& other_points_here
		&& nets_to_zero
		&& other.transaction.account == transfer.account
}

/// A mirror leg takes its primary leg's decision. Without one it is an
/// unpaired transfer for review.
fn mirrored(row: ClassifiedRow, primary: Option<Decision>) -> ClassifiedRow {
	match primary {
		Some(decision) => ClassifiedRow {
			decision,
			leg: Leg::Mirror,
			..row
		},
		None => ClassifiedRow {
			decision: Decision::UncategorizedTransfer,
			flags: Flags {
				needs_review: true,
				unpaired: true,
				..Flags::default()
			},
			..row
		},
	}
}

/// More rows than this changing at once cost more to splice in one at a
/// time than to classify afresh.
const RECLASSIFY_AT_MOST: usize = 32;

/// The rows whose classification can change when the transactions in
/// `changed` are written: those transactions, the split children whose
/// liveness follows them, and every row whose transfer names one of those.
/// A row reads nothing else from other transactions.
fn affected_rows(
	budget: &BudgetData,
	changed: &BTreeSet<TransactionId>,
) -> BTreeSet<TransactionId> {
	let mut named = changed.clone();
	for transaction in budget.transactions.values() {
		if let SplitRole::Child { parent } = &transaction.split
			&& changed.contains(parent)
		{
			named.insert(transaction.id.clone());
		}
	}
	let mut affected = named.clone();
	for transaction in budget.transactions.values() {
		if transaction
			.transfer
			.as_ref()
			.is_some_and(|to| named.contains(to))
		{
			affected.insert(transaction.id.clone());
		}
	}
	affected
}

/// What [`classify`] decides for `ids` in `budget`: their rows, and those
/// that are orphans. Ids that aren't live rows are left out.
fn classify_rows(
	budget: &BudgetData,
	ids: &BTreeSet<TransactionId>,
) -> (Vec<ClassifiedRow>, Vec<TransactionId>) {
	let mut lookups = Lookups::new(budget);
	let mut rows = Vec::new();
	let mut orphans = Vec::new();
	for id in ids {
		let Some(transaction) = budget
			.transactions
			.get(id)
			.filter(|transaction| is_row(budget, transaction))
		else {
			continue;
		};
		let Some(candidate) = Candidate::new(budget, &mut lookups, transaction) else {
			orphans.push(id.clone());
			continue;
		};
		let partner = partner_id(&candidate)
			.and_then(|other| budget.transactions.get(other))
			.filter(|other| is_row(budget, other))
			.and_then(|other| Candidate::new(budget, &mut lookups, other))
			.filter(|other| pairs(&candidate, other));
		let row = ClassifiedRow {
			partner: partner.as_ref().map(|other| Partner {
				id: other.transaction.id.clone(),
				date: other.transaction.date,
			}),
			..candidate.row()
		};
		rows.push(match decide(&candidate, partner.as_ref()) {
			Outcome::Decided(decision, flags) => ClassifiedRow {
				decision,
				flags,
				..row
			},
			// A mirror always has a partner, and the partner's own partner
			// is this row, as the pairing is reciprocal.
			Outcome::Mirror => {
				let primary =
					partner
						.as_ref()
						.and_then(|primary| match decide(primary, Some(&candidate)) {
							Outcome::Decided(decision, _) => Some(decision),
							Outcome::Mirror => None,
						});
				mirrored(row, primary)
			}
		});
	}
	(rows, orphans)
}

enum Outcome {
	Decided(Decision, Flags),
	Mirror,
}

fn decide(candidate: &Candidate, partner: Option<&Candidate>) -> Outcome {
	let mut flags = Flags::default();
	let category_kind = candidate.category_kind;

	let Some(transfer) = &candidate.transfer else {
		let decision = if candidate.starting_balance {
			Decision::StartingBalance
		} else if candidate.reconciliation {
			Decision::Reconciliation
		} else if !candidate.on_budget {
			Decision::OffBudgetRevaluation
		} else {
			match category_kind {
				None => {
					flags.needs_review = true;
					Decision::Uncategorized
				}
				Some((kind, conflict)) => {
					flags.kind_conflict = conflict;
					match kind {
						CategoryKind::Income => Decision::Income,
						CategoryKind::Expense => Decision::Expense,
						CategoryKind::DebtPrincipal | CategoryKind::Savings => {
							flags.misfiled = true;
							Decision::MisfiledTransferCategory
						}
					}
				}
			}
		};
		return Outcome::Decided(decision, flags);
	};

	let Some(partner) = partner else {
		flags.unpaired = true;
		if candidate.marked() {
			flags.needs_review = true;
			let decision = if candidate.starting_balance {
				Decision::StartingBalance
			} else {
				Decision::Reconciliation
			};
			return Outcome::Decided(decision, flags);
		}
		return Outcome::Decided(
			transfer_decision(candidate, transfer, category_kind, &mut flags),
			flags,
		);
	};

	if !candidate.on_budget && transfer.other_on_budget {
		return Outcome::Mirror;
	}
	flags.needs_review = candidate.marked() || partner.marked();
	Outcome::Decided(
		transfer_decision(candidate, transfer, category_kind, &mut flags),
		flags,
	)
}

/// Rows 4 to 10 for a transfer that is not a mirror leg.
fn transfer_decision(
	candidate: &Candidate,
	transfer: &Transfer,
	category_kind: Option<(CategoryKind, bool)>,
	flags: &mut Flags,
) -> Decision {
	match (candidate.on_budget, transfer.other_on_budget) {
		(true, true) => {
			flags.needs_review |= candidate.category.is_some();
			Decision::OnBudgetTransfer
		}
		(false, false) => {
			flags.needs_review |= candidate.category.is_some();
			Decision::OffBudgetTransfer
		}
		(false, true) => {
			flags.needs_review = true;
			Decision::UncategorizedTransfer
		}
		(true, false) => match category_kind {
			None => {
				flags.needs_review = true;
				Decision::UncategorizedTransfer
			}
			Some((kind, conflict)) => {
				flags.kind_conflict = conflict;
				match kind {
					CategoryKind::DebtPrincipal => Decision::DebtPrincipalTransfer,
					CategoryKind::Savings => Decision::SavingsTransfer,
					CategoryKind::Expense => Decision::CapitalizedExpense,
					CategoryKind::Income => Decision::CapitalizedIncome,
				}
			}
		},
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn every_decision_maps_to_its_table_row_and_kind() {
		use TransactionKind::*;
		let table = [
			(Decision::StartingBalance, 2, StartingBalance),
			(Decision::Reconciliation, 3, Adjustment),
			(Decision::OnBudgetTransfer, 4, InternalTransfer),
			(Decision::OffBudgetTransfer, 5, InternalTransfer),
			(Decision::DebtPrincipalTransfer, 6, DebtPrincipal),
			(Decision::SavingsTransfer, 7, SavingsContribution),
			(Decision::CapitalizedExpense, 8, Expense),
			(Decision::CapitalizedIncome, 9, Income),
			(Decision::UncategorizedTransfer, 10, InternalTransfer),
			(Decision::OffBudgetRevaluation, 11, Adjustment),
			(Decision::Income, 12, Income),
			(Decision::Expense, 13, Expense),
			(Decision::MisfiledTransferCategory, 14, Expense),
			(Decision::Uncategorized, 15, Expense),
		];
		for (decision, row, kind) in table {
			assert_eq!(decision.table_row(), row, "{decision:?}");
			assert_eq!(decision.kind(), kind, "{decision:?}");
		}
	}
}
