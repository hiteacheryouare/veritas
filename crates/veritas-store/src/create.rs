use veritas_domain::{
	Account, AccountId, AccountKind, AccountMeta, AccountMetaRow, BudgetData, Category,
	CategoryGroup, CategoryGroupId, CategoryId, CategoryKind, CategoryMapping, CategoryMeta,
	CategoryMetaRow, CivilDate, CoreError, Money, NewAccountKind, Payee, PayeeId, PayeeMapping,
	Row, SplitRole, Transaction, TransactionId,
};

use crate::audit::TAX_BUCKETS;
use crate::starter::StarterGroup;

/// The db of a brand-new Actual budget, made by `cargo xtask template`.
pub const TEMPLATE_DB: &[u8] = include_bytes!("../template/actual-26.11.0.sqlite");

/// Actual's gap between neighbouring `sort_order` values (db/sort.ts).
const SORT_INCREMENT: f64 = 16384.0;

/// An id in the shape Actual generates: a lowercase UUIDv4.
pub fn new_id() -> String {
	uuid::Uuid::new_v4().to_string()
}

fn invalid(message: impl Into<String>) -> CoreError {
	CoreError::Invalid(message.into())
}

/// The rows that turn a budget's categories into `groups`, in order: seed
/// rows are renamed, moved and re-sorted with plain updates, and every other
/// group and category is inserted with its identity mapping, as Actual's own
/// create does. Each category also gets its kind and tax bucket.
pub fn plan_starter_categories(
	data: &BudgetData,
	groups: &[StarterGroup],
) -> Result<Vec<Row>, CoreError> {
	let mut rows = Vec::new();
	let mut category_index = 0.0;
	for (group_index, starter) in groups.iter().enumerate() {
		let sort_order = (group_index as f64 + 1.0) * SORT_INCREMENT;
		let group_id = match &starter.seed {
			Some(seed) => {
				let existing = data
					.category_groups
					.get(seed)
					.ok_or_else(|| invalid(format!("the template has no group {seed}")))?;
				if existing.is_income != starter.is_income {
					return Err(invalid(format!(
						"the template's group {seed} can't change between income and expense"
					)));
				}
				rows.push(Row::CategoryGroup(CategoryGroup {
					name: starter.name.clone(),
					sort_order,
					..existing.clone()
				}));
				seed.clone()
			}
			None => {
				let id = CategoryGroupId::new(new_id());
				rows.push(Row::CategoryGroup(CategoryGroup {
					id: id.clone(),
					name: starter.name.clone(),
					is_income: starter.is_income,
					hidden: false,
					sort_order,
					tombstone: false,
				}));
				id
			}
		};
		for category in &starter.categories {
			if (category.kind == CategoryKind::Income) != starter.is_income {
				return Err(invalid(format!(
					"{} must be an income category exactly when its group is the income group",
					category.name
				)));
			}
			if let Some(bucket) = &category.tax_bucket
				&& !TAX_BUCKETS.contains(&bucket.as_str())
			{
				return Err(invalid(format!("{bucket} isn't a tax bucket")));
			}
			category_index += 1.0;
			let sort_order = category_index * SORT_INCREMENT;
			let id = match &category.seed {
				Some(seed) => {
					let existing = data
						.categories
						.get(seed)
						.ok_or_else(|| invalid(format!("the template has no category {seed}")))?;
					rows.push(Row::Category(Category {
						name: category.name.clone(),
						group: group_id.clone(),
						is_income: starter.is_income,
						sort_order,
						..existing.clone()
					}));
					seed.clone()
				}
				None => {
					let id = CategoryId::new(new_id());
					rows.push(Row::Category(Category {
						id: id.clone(),
						name: category.name.clone(),
						group: group_id.clone(),
						is_income: starter.is_income,
						hidden: false,
						sort_order,
						tombstone: false,
					}));
					rows.push(Row::CategoryMapping(CategoryMapping {
						from: id.clone(),
						to: id.clone(),
					}));
					id
				}
			};
			rows.push(Row::CategoryMeta(CategoryMetaRow {
				category: id,
				meta: CategoryMeta {
					kind: Some(category.kind),
					tax_bucket: category.tax_bucket.clone(),
					archived_on: None,
				},
			}));
		}
	}
	Ok(rows)
}

#[derive(Clone, Debug, PartialEq)]
/// An account setup creates. A liability's `starting_balance` is what is
/// owed, as a positive amount, and is stored negative as Actual expects.
pub struct NewAccount {
	pub name: String,
	pub kind: NewAccountKind,
	pub starting_balance: Money,
}

impl NewAccount {
	fn off_budget(&self) -> bool {
		self.kind.off_budget()
	}

	fn opening_amount(&self) -> Result<Money, CoreError> {
		match self.kind.kind() {
			AccountKind::Asset => Ok(self.starting_balance),
			AccountKind::Liability => self.starting_balance.checked_neg(),
		}
	}
}

/// D56: off-budget accounts for a 401(k) and an HSA, when setup's boxes are
/// checked, so gross-paycheck contributions classify as savings.
pub fn paycheck_accounts(retirement_401k: bool, hsa: bool) -> Vec<NewAccount> {
	let mut accounts = Vec::new();
	if retirement_401k {
		accounts.push(NewAccount {
			name: "401(k)".to_owned(),
			kind: NewAccountKind::Retirement,
			starting_balance: Money::ZERO,
		});
	}
	if hsa {
		accounts.push(NewAccount {
			name: "HSA".to_owned(),
			kind: NewAccountKind::Investment,
			starting_balance: Money::ZERO,
		});
	}
	accounts
}

/// `getStartingBalancePayee`: the income category named "Starting Balances",
/// else the first live income category.
fn starting_balance_category(data: &BudgetData) -> Option<CategoryId> {
	let live_income = || {
		data.categories
			.values()
			.filter(|c| c.is_income && !c.tombstone)
	};
	live_income()
		.find(|c| c.name.to_lowercase() == "starting balances")
		.or_else(|| live_income().next())
		.map(|c| c.id.clone())
}

/// The rows Actual's `createAccount` writes, plus the account's kind in
/// `veritas_account_meta`: the account, its transfer payee and mapping, and
/// for a non-zero balance a starting-balance transaction
/// dated `today` with the "Starting Balance" payee, created if needed.
pub fn plan_new_account(
	data: &BudgetData,
	account: &NewAccount,
	today: CivilDate,
	now_ms: f64,
) -> Result<Vec<Row>, CoreError> {
	if account.name.trim().is_empty() {
		return Err(invalid("An account needs a name."));
	}
	let last = data
		.accounts
		.values()
		.filter(|a| a.off_budget == account.off_budget())
		.map(|a| a.sort_order)
		.fold(0.0, f64::max);
	let id = AccountId::new(new_id());
	let transfer_payee = PayeeId::new(new_id());
	let mut rows = vec![
		Row::Account(Account {
			id: id.clone(),
			name: account.name.clone(),
			off_budget: account.off_budget(),
			closed: false,
			group: None,
			sort_order: last + SORT_INCREMENT,
			tombstone: false,
		}),
		Row::Payee(Payee {
			id: transfer_payee.clone(),
			name: String::new(),
			transfer_account: Some(id.clone()),
			favorite: false,
			learn_categories: true,
			tombstone: false,
		}),
		Row::PayeeMapping(PayeeMapping {
			from: transfer_payee.clone(),
			to: transfer_payee,
		}),
		Row::AccountMeta(AccountMetaRow {
			account: id.clone(),
			meta: AccountMeta {
				kind: Some(account.kind.kind()),
				..AccountMeta::default()
			},
		}),
	];
	let opening = account.opening_amount()?;
	if opening != Money::ZERO {
		let existing = data
			.payees
			.values()
			.find(|p| !p.tombstone && p.name.to_lowercase() == "starting balance")
			.map(|p| p.id.clone());
		let payee = match existing {
			Some(payee) => payee,
			None => {
				let payee = PayeeId::new(new_id());
				rows.push(Row::Payee(Payee {
					id: payee.clone(),
					name: "Starting Balance".to_owned(),
					transfer_account: None,
					favorite: false,
					learn_categories: true,
					tombstone: false,
				}));
				rows.push(Row::PayeeMapping(PayeeMapping {
					from: payee.clone(),
					to: payee.clone(),
				}));
				payee
			}
		};
		rows.push(Row::Transaction(Transaction {
			id: TransactionId::new(new_id()),
			account: id,
			date: today,
			amount: opening,
			payee: Some(payee),
			imported_payee: None,
			category: if account.off_budget() {
				None
			} else {
				starting_balance_category(data)
			},
			notes: None,
			cleared: true,
			reconciled: false,
			split: SplitRole::None,
			transfer: None,
			schedule: None,
			starting_balance: true,
			sort_order: now_ms,
			tombstone: false,
		}));
	}
	Ok(rows)
}
