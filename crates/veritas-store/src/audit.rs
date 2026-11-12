use std::collections::{BTreeMap, BTreeSet};

use veritas_domain::{BudgetData, Money, SplitRole, Transaction, TransactionId};

use crate::load::Loaded;

/// What an audit finding is about. Findings name rows by id only, never by
/// amount, name or note, because the report can end up in logs.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Check {
	/// `PRAGMA integrity_check` found damage. The text is SQLite's.
	Integrity(String),
	/// The load couldn't represent the row; the reason says why.
	Unrepresentable(&'static str),
	/// A transfer row whose link isn't reciprocal, live and between the two
	/// accounts its payees name.
	UnpairedTransfer,
	/// A row links to a counterpart but its payee isn't a transfer payee.
	StrayTransferLink,
	/// A reciprocal pair whose amounts don't cancel.
	TransferAmountMismatch,
	/// A live split parent whose live children don't sum to it.
	SplitSumMismatch,
	/// A live split parent with no live children.
	EmptySplit,
	/// A split child whose parent row exists but isn't a split parent.
	ChildOfNonParent,
	/// A live row references a row that doesn't exist. `related` names it.
	MissingReference(&'static str),
	/// A live row references a tombstoned row. `related` names it.
	DeletedReference(&'static str),
	/// A mapping points at an id that maps on to somewhere else. Actual keeps
	/// the tables flattened, and its views follow only one step.
	MappingChain,
	/// Following the mapping comes back to where it started.
	MappingCycle,
	/// Two budget rows for the same month and category.
	DuplicateBudgetCell,
	/// A tax bucket id that isn't one of the known buckets, written by
	/// something other than this build. `related` names the id.
	UnknownTaxBucket,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Violation {
	pub check: Check,
	pub table: &'static str,
	pub id: Option<String>,
	pub related: Option<String>,
}

/// The invariant audit. It reports and never repairs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AuditReport {
	pub violations: Vec<Violation>,
}

impl AuditReport {
	/// Damage SQLite itself found. A budget like this is never adopted.
	pub fn is_corrupt(&self) -> bool {
		self.violations
			.iter()
			.any(|violation| matches!(violation.check, Check::Integrity(_)))
	}

	pub fn is_clean(&self) -> bool {
		self.violations.is_empty()
	}

	pub fn from_integrity(problems: Vec<String>) -> AuditReport {
		AuditReport {
			violations: problems
				.into_iter()
				.map(|problem| Violation {
					check: Check::Integrity(problem),
					table: "sqlite",
					id: None,
					related: None,
				})
				.collect(),
		}
	}
}

struct Findings(Vec<Violation>);

impl Findings {
	fn push(&mut self, check: Check, table: &'static str, id: &str, related: Option<&str>) {
		self.0.push(Violation {
			check,
			table,
			id: Some(id.to_owned()),
			related: related.map(str::to_owned),
		});
	}
}

/// A transaction Actual's alive view would return: not tombstoned, and a child
/// only while its parent exists and isn't tombstoned.
fn is_live(data: &BudgetData, transaction: &Transaction) -> bool {
	if transaction.tombstone {
		return false;
	}
	match &transaction.split {
		SplitRole::Child { parent } => data
			.transactions
			.get(parent)
			.is_some_and(|parent| !parent.tombstone),
		_ => true,
	}
}

pub fn audit(loaded: &Loaded) -> AuditReport {
	let data = &loaded.data;
	let mut findings = Findings(Vec::new());
	for skipped in &loaded.report.skipped {
		let check = if skipped.reason == crate::load::DUPLICATE_BUDGET_CELL {
			Check::DuplicateBudgetCell
		} else {
			Check::Unrepresentable(skipped.reason)
		};
		findings.0.push(Violation {
			check,
			table: skipped.table,
			id: skipped.id.clone(),
			related: None,
		});
	}
	transfers(data, &mut findings);
	tax_buckets(data, &mut findings);
	splits(data, &mut findings);
	references(data, &mut findings);
	mappings(
		"category_mapping",
		data.category_mapping
			.iter()
			.map(|(from, to)| (from.as_str(), to.as_str())),
		&mut findings,
	);
	mappings(
		"payee_mapping",
		data.payee_mapping
			.iter()
			.map(|(from, to)| (from.as_str(), to.as_str())),
		&mut findings,
	);
	let mut violations = findings.0;
	violations.sort();
	violations.dedup();
	AuditReport { violations }
}

/// The known tax buckets. `none` is only a tag override, never a category bucket.
const TAX_BUCKETS: [&str; 10] = [
	"w2_income",
	"interest_income",
	"business_income",
	"business_expense",
	"charitable",
	"medical",
	"education",
	"hsa",
	"retirement_contribution",
	"taxes_withheld",
];

fn tax_buckets(data: &BudgetData, findings: &mut Findings) {
	for (category, meta) in &data.category_meta {
		if let Some(bucket) = &meta.tax_bucket
			&& !TAX_BUCKETS.contains(&bucket.as_str())
		{
			findings.push(
				Check::UnknownTaxBucket,
				"veritas_category_meta",
				category.as_str(),
				Some(bucket),
			);
		}
	}
	for (tag, bucket) in &data.tax_tags {
		if bucket != "none" && !TAX_BUCKETS.contains(&bucket.as_str()) {
			findings.push(
				Check::UnknownTaxBucket,
				"veritas_tax_tag",
				tag,
				Some(bucket),
			);
		}
	}
}

fn transfer_account<'a>(data: &'a BudgetData, transaction: &Transaction) -> Option<&'a str> {
	let payee = data.resolve_payee(transaction.payee.as_ref()?)?;
	data.payees
		.get(payee)?
		.transfer_account
		.as_ref()
		.map(|account| account.as_str())
}

fn transfers(data: &BudgetData, findings: &mut Findings) {
	for transaction in data.transactions.values() {
		if !is_live(data, transaction) || transaction.split == SplitRole::Parent {
			continue;
		}
		let id = transaction.id.as_str();
		let Some(counterpart_account) = transfer_account(data, transaction) else {
			if let Some(link) = &transaction.transfer {
				findings.push(
					Check::StrayTransferLink,
					"transactions",
					id,
					Some(link.as_str()),
				);
			}
			continue;
		};
		let partner = transaction
			.transfer
			.as_ref()
			.and_then(|link| data.transactions.get(link))
			.filter(|partner| is_live(data, partner) && partner.split != SplitRole::Parent);
		let paired = partner.filter(|partner| {
			partner.transfer.as_ref() == Some(&transaction.id)
				&& partner.account.as_str() == counterpart_account
				&& transfer_account(data, partner) == Some(transaction.account.as_str())
		});
		match paired {
			None => findings.push(
				Check::UnpairedTransfer,
				"transactions",
				id,
				transaction.transfer.as_ref().map(TransactionId::as_str),
			),
			Some(partner) => {
				let cancels =
					transaction.amount.checked_add(partner.amount).ok() == Some(Money::ZERO);
				if !cancels && transaction.id < partner.id {
					findings.push(
						Check::TransferAmountMismatch,
						"transactions",
						id,
						Some(partner.id.as_str()),
					);
				}
			}
		}
	}
}

fn splits(data: &BudgetData, findings: &mut Findings) {
	let mut children: BTreeMap<&TransactionId, Vec<Money>> = BTreeMap::new();
	for transaction in data.transactions.values() {
		let SplitRole::Child { parent } = &transaction.split else {
			continue;
		};
		if transaction.tombstone {
			continue;
		}
		match data.transactions.get(parent) {
			None => findings.push(
				Check::MissingReference("parent transaction"),
				"transactions",
				transaction.id.as_str(),
				Some(parent.as_str()),
			),
			Some(row) if row.split != SplitRole::Parent => findings.push(
				Check::ChildOfNonParent,
				"transactions",
				transaction.id.as_str(),
				Some(parent.as_str()),
			),
			Some(_) => children.entry(parent).or_default().push(transaction.amount),
		}
	}
	for parent in data.transactions.values() {
		if parent.split != SplitRole::Parent || parent.tombstone {
			continue;
		}
		match children.get(&parent.id) {
			None => findings.push(Check::EmptySplit, "transactions", parent.id.as_str(), None),
			Some(amounts) => {
				if Money::checked_sum(amounts.iter().copied()).ok() != Some(parent.amount) {
					findings.push(
						Check::SplitSumMismatch,
						"transactions",
						parent.id.as_str(),
						None,
					);
				}
			}
		}
	}
}

/// Records a reference from a live row: missing, or pointing at a tombstone.
fn reference(
	findings: &mut Findings,
	table: &'static str,
	id: &str,
	what: &'static str,
	target: &str,
	target_tombstone: Option<bool>,
) {
	match target_tombstone {
		None => findings.push(Check::MissingReference(what), table, id, Some(target)),
		Some(true) => findings.push(Check::DeletedReference(what), table, id, Some(target)),
		Some(false) => {}
	}
}

fn references(data: &BudgetData, findings: &mut Findings) {
	let account = |id: &str| {
		data.accounts
			.get(&veritas_domain::AccountId::new(id))
			.map(|row| row.tombstone)
	};
	let category = |id: &str| {
		data.categories
			.get(&veritas_domain::CategoryId::new(id))
			.map(|row| row.tombstone)
	};

	for transaction in data.transactions.values() {
		if !is_live(data, transaction) {
			continue;
		}
		let id = transaction.id.as_str();
		let row_account = transaction.account.as_str();
		reference(
			findings,
			"transactions",
			id,
			"account",
			row_account,
			account(row_account),
		);
		if let Some(payee) = &transaction.payee {
			match data.resolve_payee(payee) {
				None => findings.push(
					Check::MissingReference("payee mapping"),
					"transactions",
					id,
					Some(payee.as_str()),
				),
				Some(target) => {
					let exists = data.payees.get(target).map(|_| false);
					reference(
						findings,
						"transactions",
						id,
						"payee",
						target.as_str(),
						exists,
					);
				}
			}
		}
		if let Some(row_category) = &transaction.category {
			match data.resolve_category(row_category) {
				None => findings.push(
					Check::MissingReference("category mapping"),
					"transactions",
					id,
					Some(row_category.as_str()),
				),
				Some(target) => {
					let exists = data.categories.get(target).map(|_| false);
					reference(
						findings,
						"transactions",
						id,
						"category",
						target.as_str(),
						exists,
					);
				}
			}
		}
		if let Some(schedule) = &transaction.schedule {
			let exists = data.schedules.get(schedule).map(|_| false);
			reference(
				findings,
				"transactions",
				id,
				"schedule",
				schedule.as_str(),
				exists,
			);
		}
	}

	for row in data.accounts.values().filter(|row| !row.tombstone) {
		if let Some(group) = &row.group {
			let exists = data.account_groups.get(group).map(|group| group.tombstone);
			reference(
				findings,
				"accounts",
				row.id.as_str(),
				"account group",
				group.as_str(),
				exists,
			);
		}
	}
	for row in data.categories.values().filter(|row| !row.tombstone) {
		let exists = data
			.category_groups
			.get(&row.group)
			.map(|group| group.tombstone);
		reference(
			findings,
			"categories",
			row.id.as_str(),
			"category group",
			row.group.as_str(),
			exists,
		);
	}
	for row in data.payees.values().filter(|row| !row.tombstone) {
		if let Some(target) = &row.transfer_account {
			let exists = account(target.as_str()).map(|_| false);
			reference(
				findings,
				"payees",
				row.id.as_str(),
				"transfer account",
				target.as_str(),
				exists,
			);
		}
	}
	for row in data.schedules.values().filter(|row| !row.tombstone) {
		let exists = data.rules.get(&row.rule).map(|rule| rule.tombstone);
		reference(
			findings,
			"schedules",
			row.id.as_str(),
			"rule",
			row.rule.as_str(),
			exists,
		);
	}
	for (month, row_category) in data.budget_cells.keys() {
		if category(row_category.as_str()).is_none() {
			let id = format!("{month}/{row_category}");
			findings.push(
				Check::MissingReference("category"),
				"budget",
				&id,
				Some(row_category.as_str()),
			);
		}
	}

	// Veritas's own rows may outlive what Actual deletes.
	for row_category in data.category_meta.keys() {
		let exists = category(row_category.as_str()).map(|_| false);
		reference(
			findings,
			"veritas_category_meta",
			row_category.as_str(),
			"category",
			row_category.as_str(),
			exists,
		);
	}
	for row_account in data.account_meta.keys() {
		let exists = account(row_account.as_str()).map(|_| false);
		reference(
			findings,
			"veritas_account_meta",
			row_account.as_str(),
			"account",
			row_account.as_str(),
			exists,
		);
	}
	for transaction in data.transaction_meta.keys() {
		let exists = data.transactions.get(transaction).map(|_| false);
		reference(
			findings,
			"veritas_transaction_meta",
			transaction.as_str(),
			"transaction",
			transaction.as_str(),
			exists,
		);
	}
	for goal in data.goals.values() {
		let id = goal.id.as_str();
		reference(
			findings,
			"veritas_goal",
			id,
			"category",
			goal.category.as_str(),
			category(goal.category.as_str()),
		);
		if let Some(released) = &goal.released_to {
			let exists = category(released.as_str()).map(|_| false);
			reference(
				findings,
				"veritas_goal",
				id,
				"released-to category",
				released.as_str(),
				exists,
			);
		}
	}
}

fn mappings<'a>(
	table: &'static str,
	rows: impl Iterator<Item = (&'a str, &'a str)>,
	findings: &mut Findings,
) {
	let map: BTreeMap<&str, &str> = rows.collect();
	for (&from, &to) in &map {
		if from == to {
			continue;
		}
		let mut seen = BTreeSet::from([from]);
		let mut current = to;
		let mut check = None;
		while let Some(&next) = map.get(current) {
			if next == current {
				break;
			}
			if !seen.insert(current) {
				check = Some(Check::MappingCycle);
				break;
			}
			check = Some(Check::MappingChain);
			current = next;
		}
		if let Some(check) = check {
			findings.push(check, table, from, Some(to));
		}
	}
}
