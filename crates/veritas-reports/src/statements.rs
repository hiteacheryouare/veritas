use std::collections::BTreeMap;

use veritas_domain::{
	AccountGroupId, AccountId, BudgetData, CivilDate, CoreError, Money, TransactionKind,
};

use crate::balances::{self, AccountBalance, Side};
use crate::{Classification, ClassifiedRow, Decision, Line, Period, Section, net_profit};

#[derive(Clone, Debug, PartialEq)]
pub struct SheetLine {
	pub account: AccountId,
	/// As displayed: an asset's balance, or what a liability owes.
	pub amount: Money,
	pub closed: bool,
	pub deleted: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AccountGroupSection {
	/// `None` holds the accounts outside any account group.
	pub group: Option<AccountGroupId>,
	pub accounts: Vec<SheetLine>,
	pub total: Money,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BalanceSheet {
	pub as_of: CivilDate,
	pub assets: Vec<AccountGroupSection>,
	pub total_assets: Money,
	pub liabilities: Vec<AccountGroupSection>,
	pub total_liabilities: Money,
	pub net_worth: Money,
}

pub fn balance_sheet(
	budget: &BudgetData,
	classification: &Classification,
	as_of: CivilDate,
) -> Result<BalanceSheet, CoreError> {
	let balances = balances::account_balances(budget, classification, as_of)?;
	let (total_assets, total_liabilities) = balances::totals(&balances)?;
	let shown: Vec<&AccountBalance> = balances
		.iter()
		.filter(|line| !(line.closed || line.deleted) || line.balance != Money::ZERO)
		.collect();
	Ok(BalanceSheet {
		as_of,
		assets: sections(budget, &shown, Side::Asset)?,
		total_assets,
		liabilities: sections(budget, &shown, Side::Liability)?,
		total_liabilities,
		net_worth: total_assets.checked_sub(total_liabilities)?,
	})
}

fn sections(
	budget: &BudgetData,
	balances: &[&AccountBalance],
	side: Side,
) -> Result<Vec<AccountGroupSection>, CoreError> {
	let mut groups: BTreeMap<Option<AccountGroupId>, Vec<SheetLine>> = BTreeMap::new();
	for line in balances.iter().filter(|line| line.side == side) {
		let amount = match side {
			Side::Asset => line.balance,
			Side::Liability => line.balance.checked_neg()?,
		};
		groups
			.entry(line.group.clone())
			.or_default()
			.push(SheetLine {
				account: line.account.clone(),
				amount,
				closed: line.closed,
				deleted: line.deleted,
			});
	}
	let mut sections = groups
		.into_iter()
		.map(|(group, accounts)| {
			Ok(AccountGroupSection {
				total: Money::checked_sum(accounts.iter().map(|line| line.amount))?,
				group,
				accounts,
			})
		})
		.collect::<Result<Vec<_>, CoreError>>()?;
	let order = |group: &Option<AccountGroupId>| {
		group.as_ref().map(|id| {
			budget
				.account_groups
				.get(id)
				.map_or(f64::MAX, |group| group.sort_order)
		})
	};
	sections.sort_by(|a, b| match (order(&a.group), order(&b.group)) {
		(None, None) => std::cmp::Ordering::Equal,
		(None, Some(_)) => std::cmp::Ordering::Greater,
		(Some(_), None) => std::cmp::Ordering::Less,
		(Some(x), Some(y)) => x.total_cmp(&y).then_with(|| a.group.cmp(&b.group)),
	});
	Ok(sections)
}

/// The Statement of Owner's Equity. Its lines are the only reconciling items
/// there are: if `unexplained` is ever non-zero a row is misclassified, and
/// the audit reports it. No balancing line is ever added.
#[derive(Clone, Debug, PartialEq)]
pub struct OwnersEquity {
	pub period: Period,
	/// Net worth on the day before the period starts.
	pub opening: Money,
	pub net_income: Line,
	pub capitalized_transfers: Line,
	pub starting_balances: Line,
	pub reconciliation_adjustments: Line,
	pub revaluations: Line,
	pub unpaired_transfers: Line,
	/// Paired legs dated in the period whose partner is dated outside it.
	pub in_transit: Line,
	/// Net worth on the last day of the period.
	pub closing: Money,
}

impl OwnersEquity {
	pub fn lines(&self) -> [&Line; 7] {
		[
			&self.net_income,
			&self.capitalized_transfers,
			&self.starting_balances,
			&self.reconciliation_adjustments,
			&self.revaluations,
			&self.unpaired_transfers,
			&self.in_transit,
		]
	}

	pub fn unexplained(&self) -> Result<Money, CoreError> {
		let explained = Money::checked_sum(self.lines().iter().map(|line| line.total))?;
		self.closing
			.checked_sub(self.opening)?
			.checked_sub(explained)
	}
}

pub fn owners_equity(
	budget: &BudgetData,
	classification: &Classification,
	period: Period,
) -> Result<OwnersEquity, CoreError> {
	let mut capitalized = Lines::default();
	let mut starting = Lines::default();
	let mut reconciliation = Lines::default();
	let mut revaluations = Lines::default();
	let mut unpaired = Lines::default();
	let mut in_transit = Lines::default();
	for row in classification.within(period) {
		let kind = row.kind();
		if row.in_transit(period) {
			in_transit.add(row, row.amount)?;
		}
		if row.is_primary()
			&& !row.flags.unpaired
			&& matches!(
				row.decision,
				Decision::CapitalizedExpense | Decision::CapitalizedIncome
			) {
			capitalized.add(row, row.amount.checked_neg()?)?;
		}
		match row.decision {
			Decision::Reconciliation => reconciliation.add(row, row.amount)?,
			Decision::OffBudgetRevaluation => revaluations.add(row, row.amount)?,
			_ => {}
		}
		if kind == TransactionKind::StartingBalance {
			starting.add(row, row.amount)?;
		}
		if row.flags.unpaired
			&& matches!(
				kind,
				TransactionKind::InternalTransfer
					| TransactionKind::DebtPrincipal
					| TransactionKind::SavingsContribution
			) {
			unpaired.add(row, row.amount)?;
		}
	}
	Ok(OwnersEquity {
		period,
		opening: Money::checked_sum(
			classification
				.before(period.first())
				.iter()
				.map(|row| row.amount),
		)?,
		net_income: net_profit(budget, classification, period)?.net_profit,
		capitalized_transfers: capitalized.0,
		starting_balances: starting.0,
		reconciliation_adjustments: reconciliation.0,
		revaluations: revaluations.0,
		unpaired_transfers: unpaired.0,
		in_transit: in_transit.0,
		closing: balances::net_worth(classification, period.last())?,
	})
}

/// The Cash Flow Statement over the on-budget accounts. Amounts are signed as
/// they move budget cash: debt paydown and savings are negative when money
/// left the budget.
#[derive(Clone, Debug, PartialEq)]
pub struct CashFlow {
	pub period: Period,
	pub opening: Money,
	pub net_profit: Line,
	pub debt_paydown: Line,
	pub savings: Line,
	pub net_cash_flow: Line,
	pub unreviewed_transfers: Line,
	pub change_from_operations: Money,
	/// On↔on legs dated in the period whose partner is dated outside it.
	pub in_transit: Line,
	pub starting_balances: Line,
	pub reconciliation_adjustments: Line,
	pub closing: Money,
}

impl CashFlow {
	pub fn unexplained(&self) -> Result<Money, CoreError> {
		let explained = self
			.change_from_operations
			.checked_add(self.in_transit.total)?
			.checked_add(self.starting_balances.total)?
			.checked_add(self.reconciliation_adjustments.total)?;
		self.closing
			.checked_sub(self.opening)?
			.checked_sub(explained)
	}
}

pub fn cash_flow(
	budget: &BudgetData,
	classification: &Classification,
	period: Period,
) -> Result<CashFlow, CoreError> {
	let report = net_profit(budget, classification, period)?;
	let mut starting = Lines::default();
	let mut reconciliation = Lines::default();
	let mut in_transit = Lines::default();
	for row in classification
		.within(period)
		.iter()
		.filter(|row| row.on_budget)
	{
		if row.decision == Decision::OnBudgetTransfer && row.in_transit(period) {
			in_transit.add(row, row.amount)?;
		}
		match row.kind() {
			TransactionKind::StartingBalance => starting.add(row, row.amount)?,
			TransactionKind::Adjustment => reconciliation.add(row, row.amount)?,
			_ => {}
		}
	}
	let negated = |line: Line| -> Result<Line, CoreError> {
		Ok(Line {
			total: line.total.checked_neg()?,
			rows: line.rows,
		})
	};
	let unreviewed = negated(report.unreviewed)?;
	let change_from_operations = report.net_cash_flow.total.checked_add(unreviewed.total)?;
	let on_budget = |rows: &[ClassifiedRow]| {
		Money::checked_sum(
			rows.iter()
				.filter(|row| row.on_budget)
				.map(|row| row.amount),
		)
	};
	Ok(CashFlow {
		period,
		opening: on_budget(classification.before(period.first()))?,
		net_profit: report.net_profit,
		debt_paydown: negated(section_line(report.debt_paydown))?,
		savings: negated(section_line(report.savings))?,
		net_cash_flow: report.net_cash_flow,
		unreviewed_transfers: unreviewed,
		change_from_operations,
		in_transit: in_transit.0,
		starting_balances: starting.0,
		reconciliation_adjustments: reconciliation.0,
		closing: on_budget(classification.through(period.last()))?,
	})
}

fn section_line(section: Section) -> Line {
	Line {
		total: section.total,
		rows: section.rows,
	}
}

#[derive(Default)]
struct Lines(Line);

impl Lines {
	fn add(&mut self, row: &ClassifiedRow, amount: Money) -> Result<(), CoreError> {
		self.0.total = self.0.total.checked_add(amount)?;
		self.0.rows.push(row.id.clone());
		Ok(())
	}
}
