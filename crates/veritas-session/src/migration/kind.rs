//! (c) MIG-5: a category used for debt principal or savings transfers gets
//! its kind, the one-time wizard CLS-4.2 promises.

use std::collections::{BTreeSet, HashMap};

use veritas_domain::{
	AccountId, BudgetData, CategoryId, CategoryKind, CivilDate, CoreError, GoalStatus, Money,
	TransactionId, YearMonth,
};
use veritas_engine::category_kinds::{self, KindTarget};
use veritas_reports::{
	Classification, ClassifiedRow, Decision, Flags, Period, Side, account_balances, classify,
	net_profit,
};

use super::{
	Confidence, Conversion, Effect, Inputs, KindChange, Suggestion, figure, last_complete_month,
	signal,
};

/// MIG-5.1's window: the 12 calendar months ending with today's.
fn window(today: CivilDate) -> Result<Period, CoreError> {
	Period::new(today.year_month().offset(-11)?.first_day(), today)
}

fn primary_rows<'a>(
	classification: &'a Classification,
	category: &'a CategoryId,
) -> impl Iterator<Item = &'a ClassifiedRow> {
	classification
		.rows()
		.iter()
		.filter(move |row| row.is_primary() && row.category.as_ref() == Some(category))
}

/// Categories with a row-8 transfer (a primary on-to-off transfer counted as
/// spending) in the window. MIG-5.5: these are never offered as goals.
pub(super) fn transfer_categories(inputs: &Inputs<'_>) -> Result<BTreeSet<CategoryId>, CoreError> {
	let window = window(inputs.today)?;
	Ok(inputs
		.classification
		.rows()
		.iter()
		.filter(|row| row.is_primary() && row.decision == Decision::CapitalizedExpense)
		.filter(|row| window.contains(row.date))
		.filter_map(|row| row.category.clone())
		.collect())
}

fn candidate(data: &BudgetData, id: &CategoryId) -> bool {
	let Some(category) = data.categories.get(id) else {
		return false;
	};
	let income_group = data
		.category_groups
		.get(&category.group)
		.is_some_and(|group| group.is_income);
	let open_goal = data
		.goals
		.values()
		.any(|goal| &goal.category == id && goal.status != GoalStatus::Closed);
	!category.tombstone
		&& !category.is_income
		&& !income_group
		&& !open_goal
		&& data.category_kind(id) == CategoryKind::Expense
		&& data
			.category_meta
			.get(id)
			.and_then(|meta| meta.kind)
			.is_none()
}

pub(super) fn suggestions(inputs: &Inputs<'_>) -> Result<Vec<Suggestion>, CoreError> {
	let data = inputs.data;
	let window = window(inputs.today)?;
	let sides: HashMap<AccountId, Side> =
		account_balances(data, inputs.classification, inputs.today)?
			.into_iter()
			.map(|balance| (balance.account, balance.side))
			.collect();
	let mut out = Vec::new();
	for id in transfer_categories(inputs)? {
		if !candidate(data, &id) {
			continue;
		}
		let rows: Vec<&ClassifiedRow> = primary_rows(inputs.classification, &id)
			.filter(|row| window.contains(row.date))
			.collect();
		let transfers: Vec<&ClassifiedRow> = rows
			.iter()
			.copied()
			.filter(|row| row.decision == Decision::CapitalizedExpense)
			.collect();
		let t = transfers.len();
		let n = rows.len();
		let months: BTreeSet<YearMonth> =
			transfers.iter().map(|row| row.date.year_month()).collect();
		let counterparties: BTreeSet<&AccountId> = transfers
			.iter()
			.filter_map(|row| row.transfer_account.as_ref())
			.collect();
		if counterparties.is_empty() || t < 2 || t * 100 < n * 80 {
			continue;
		}
		let side = |account: &&AccountId| sides.get(*account).copied().unwrap_or(Side::Asset);
		let kind = if counterparties
			.iter()
			.all(|account| side(account) == Side::Liability)
		{
			CategoryKind::DebtPrincipal
		} else if counterparties
			.iter()
			.all(|account| side(account) == Side::Asset)
		{
			CategoryKind::Savings
		} else {
			continue;
		};
		let Some(category) = data.categories.get(&id) else {
			continue;
		};
		let saving_accounts = counterparties.iter().all(|account| {
			data.account_types
				.get(*account)
				.is_some_and(|kind| kind == "savings" || kind == "investment")
		});
		let strict = t >= 3
			&& months.len() >= 3
			&& t * 100 >= n * 90
			&& !category.hidden
			&& (kind == CategoryKind::DebtPrincipal || saving_accounts);
		let Ok(change) = change_to(inputs, &id, Some(kind)) else {
			continue;
		};
		let mut suggestion = Suggestion::new(Conversion::Kind, id.as_str(), &category.name);
		let names: Vec<&str> = counterparties
			.iter()
			.filter_map(|account| data.accounts.get(*account))
			.map(|account| account.name.as_str())
			.collect();
		let what = match kind {
			CategoryKind::DebtPrincipal => "a debt",
			_ => "an off-budget asset",
		};
		suggestion.signals.push(signal(
			"transfers",
			format!(
				"{t} transfers to {} ({what}) in {} of the last 12 months, {}% of this category's rows",
				names.join(", "),
				months.len(),
				t * 100 / n.max(1),
			),
			vec![
				figure("transfers", t as i64),
				figure("months", months.len() as i64),
				figure("rows", n as i64),
			],
		));
		suggestion.proposed.kind = Some(kind);
		suggestion.confidence = if strict {
			Confidence::PreTicked
		} else {
			Confidence::Shown
		};
		if let Some(other) = visible_with(data, kind)? {
			suggestion.confidence = Confidence::Shown;
			suggestion
				.conflicts
				.push(format!("You already use {other} as {}.", kind_words(kind)));
		}
		suggestion.kind_change = Some(change.0);
		suggestion.effects = change.1;
		out.push(suggestion);
	}
	Ok(out)
}

/// A visible category that already has `kind` (CLS-4.1).
fn visible_with(data: &BudgetData, kind: CategoryKind) -> Result<Option<String>, CoreError> {
	let found = match category_kinds::kind_target(data, kind)? {
		KindTarget::One(id) => Some(id),
		KindTarget::Several(ids) => ids.into_iter().next(),
		_ => None,
	};
	Ok(found
		.and_then(|id| data.categories.get(&id))
		.map(|category| category.name.clone()))
}

pub(super) fn kind_words(kind: CategoryKind) -> &'static str {
	match kind {
		CategoryKind::DebtPrincipal => "debt principal",
		CategoryKind::Savings => "savings",
		CategoryKind::Income => "income",
		CategoryKind::Expense => "spending",
	}
}

/// The budget with `category`'s stored kind replaced: `None` removes it.
pub(super) fn with_kind(
	data: &BudgetData,
	category: &CategoryId,
	kind: Option<CategoryKind>,
) -> BudgetData {
	let mut after = data.clone();
	let mut meta =
		after
			.category_meta
			.get(category)
			.cloned()
			.unwrap_or(veritas_domain::CategoryMeta {
				kind: None,
				tax_bucket: None,
				archived_on: None,
			});
	meta.kind = kind;
	if meta.kind.is_none() && meta.tax_bucket.is_none() && meta.archived_on.is_none() {
		after.category_meta.remove(category);
	} else {
		after.category_meta.insert(category.clone(), meta);
	}
	after
}

/// MIG-5.4 over all history, and the report figures for the last complete
/// month, if the category's stored kind becomes `kind`.
pub(super) fn change_to(
	inputs: &Inputs<'_>,
	category: &CategoryId,
	kind: Option<CategoryKind>,
) -> Result<(KindChange, Vec<Effect>), CoreError> {
	let after_data = with_kind(inputs.data, category, kind);
	let after = classify(&after_data);
	let before: HashMap<&TransactionId, (Decision, Flags)> = inputs
		.classification
		.rows()
		.iter()
		.filter(|row| row.is_primary())
		.map(|row| (&row.id, (row.decision, row.flags)))
		.collect();
	let mut rows = 0u32;
	let mut transfers = 0u32;
	let mut amount = Money::ZERO;
	let mut purchases = 0u32;
	let paid = |decision: Decision| {
		matches!(
			decision,
			Decision::DebtPrincipalTransfer | Decision::SavingsTransfer
		)
	};
	for row in after.rows().iter().filter(|row| row.is_primary()) {
		let Some(&(decision, flags)) = before.get(&row.id) else {
			continue;
		};
		if (decision, flags) == (row.decision, row.flags) {
			continue;
		}
		rows += 1;
		if row.category.as_ref() != Some(category) {
			continue;
		}
		let moved = (decision == Decision::CapitalizedExpense && paid(row.decision))
			|| (paid(decision) && row.decision == Decision::CapitalizedExpense);
		if moved {
			transfers += 1;
			amount = amount.checked_sub(row.amount)?;
		} else if flags.misfiled != row.flags.misfiled {
			purchases += 1;
		}
	}
	let stored = inputs
		.data
		.category_meta
		.get(category)
		.and_then(|meta| meta.kind);
	let involved = kind.or(stored).unwrap_or(CategoryKind::Expense);
	let month = last_complete_month(inputs.today)?;
	let period = Period::month(month);
	let was = net_profit(inputs.data, inputs.classification, period)?;
	let will = net_profit(&after_data, &after, period)?;
	let paid_line = |report: &veritas_reports::NetProfit| match involved {
		CategoryKind::Savings => report.savings.total,
		_ => report.debt_paydown.total,
	};
	let effect = |name: &str, before: Money, after: Money| Effect {
		report: name.to_owned(),
		month: Some(month),
		before,
		after,
	};
	let paid_name = match involved {
		CategoryKind::Savings => "Savings",
		_ => "Debt paydown",
	};
	let effects = vec![
		effect("Expenses", was.expenses.total, will.expenses.total),
		effect(paid_name, paid_line(&was), paid_line(&will)),
		effect("Net Profit", was.net_profit.total, will.net_profit.total),
		effect(
			"Net cash flow",
			was.net_cash_flow.total,
			will.net_cash_flow.total,
		),
	];
	Ok((
		KindChange {
			rows,
			transfers,
			amount,
			purchases,
			kind: involved,
		},
		effects,
	))
}
