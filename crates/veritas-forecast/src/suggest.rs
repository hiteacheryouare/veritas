use std::collections::BTreeMap;

use veritas_domain::{
	CategoryId, CategoryKind, CivilDate, CoreError, GoalStatus, Money, TransactionKind, YearMonth,
};
use veritas_engine::EnvelopeSheet;
use veritas_reports::Period;

use crate::baseline::Baseline;
use crate::flows::{FlowKind, KnownFlow};
use crate::goal::month_name;
use crate::history::{Side, archived, balances, hidden};
use crate::math::{ceil_div, round_half_up, to_money};
use crate::projection::Forecaster;

/// What the envelope sheet (veritas-engine) says about a category.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Envelope {
	/// Money already in the envelope when next month starts, before its own
	/// budgeted amount: DEBT-8.1's carry for next month.
	pub carry_into_next: Money,
}

/// Every category's envelope for the month after `today`, from Actual's
/// math: DEBT-8.1's carry is that month's available minus its own budgeted
/// amount, so it already nets out spending and Actual's carryover rules.
pub fn next_month_envelopes(
	sheet: &EnvelopeSheet,
	today: CivilDate,
) -> Result<BTreeMap<CategoryId, Envelope>, CoreError> {
	let month = today.year_month().offset(1)?;
	let Some(sheet) = sheet.month(month) else {
		return Err(CoreError::Invalid(format!(
			"the envelope sheet has no {month}, so goal suggestions can't know what carries in"
		)));
	};
	sheet
		.categories
		.iter()
		.filter(|(_, cell)| !cell.is_income)
		.map(|(category, cell)| {
			Ok((
				category.clone(),
				Envelope {
					carry_into_next: cell.balance.checked_sub(cell.budgeted)?,
				},
			))
		})
		.collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Basis {
	Baseline,
	Seasonal,
	Bills,
	BillsAndBaseline,
	Goal,
	LoanPrincipal,
	NoHistory,
	NoTargetDate,
	/// GOAL-10: a reached goal gets no refill suggestion.
	GoalReached,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
	pub category: CategoryId,
	/// `None` keeps the current budgeted amount.
	pub amount: Option<Money>,
	pub basis: Basis,
	pub reason: String,
}

/// FC-5's next-month suggestions (R-FC-3).
#[derive(Clone, Debug, PartialEq)]
pub struct Suggestions {
	pub month: YearMonth,
	pub items: Vec<Suggestion>,
	/// Known income flows plus income baselines for the month.
	pub expected_income: Money,
	pub exceeds_income_by: Option<Money>,
	/// "Suggestions exceed expected income by $X", in whole dollars and never
	/// below $1 (FC-5).
	pub notice: Option<String>,
}

fn round_dollars(amount: Money) -> Result<Money, CoreError> {
	to_money(round_half_up(i128::from(amount.minor().max(0)), 100) * 100)
}

fn dollars(amount: Money) -> String {
	format!("${}", amount.minor() / 100)
}

fn cents(amount: Money) -> String {
	format!("${amount}")
}

impl Forecaster<'_> {
	/// `envelopes` needs an entry for each goal category.
	pub fn suggestions(
		&self,
		today: CivilDate,
		known: &[KnownFlow],
		envelopes: &BTreeMap<CategoryId, Envelope>,
	) -> Result<Suggestions, CoreError> {
		let month = today.year_month().offset(1)?;
		let period = Period::month(month);
		let in_month: Vec<&KnownFlow> = known
			.iter()
			.filter(|flow| period.contains(flow.date))
			.collect();
		let all_baselines = self.baselines(today, known)?;
		let baselines: BTreeMap<&CategoryId, &Baseline> = all_baselines
			.iter()
			.filter(|baseline| baseline.side == Side::Spending)
			.map(|baseline| (&baseline.category, baseline))
			.collect();

		let mut categories: Vec<_> = self
			.budget
			.categories
			.values()
			.filter(|category| {
				!category.tombstone
					&& !hidden(self.budget, &category.id)
					&& !archived(self.budget, &category.id)
			})
			.collect();
		categories.sort_by(|a, b| {
			let group = |id| {
				self.budget
					.category_groups
					.get(id)
					.map_or(f64::MAX, |group| group.sort_order)
			};
			group(&a.group)
				.total_cmp(&group(&b.group))
				.then_with(|| a.sort_order.total_cmp(&b.sort_order))
				.then_with(|| a.id.cmp(&b.id))
		});

		let mut items = Vec::new();
		for category in categories {
			let id = &category.id;
			let name = &category.name;
			let current = self
				.budget
				.budget_cells
				.get(&(month, id.clone()))
				.map_or(Money::ZERO, |cell| cell.budgeted);
			let kept = dollars(round_dollars(current)?);
			let keeping = |basis: Basis, text: &str| Suggestion {
				category: id.clone(),
				amount: None,
				basis,
				reason: format!("{text}, keeping {kept}"),
			};
			let goals = || {
				self.budget
					.goals
					.values()
					.filter(|goal| &goal.category == id)
			};
			let goal = goals()
				.find(|goal| goal.status == GoalStatus::Active)
				.or_else(|| goals().find(|goal| goal.status != GoalStatus::Closed));
			let item = match (self.budget.category_kind(id), goal) {
				(CategoryKind::Expense, Some(goal)) if goal.status == GoalStatus::Reached => {
					keeping(Basis::GoalReached, &format!("{name}: reached"))
				}
				(CategoryKind::Expense, Some(goal)) => match goal.target_date {
					None => keeping(Basis::NoTargetDate, &format!("{name}: no target date")),
					Some(target) => {
						let carry = envelopes
							.get(id)
							.map_or(Money::ZERO, |envelope| envelope.carry_into_next);
						let remaining = goal.target.checked_sub(carry)?.max(Money::ZERO);
						let months_left =
							(crate::baseline::months_between(month, target.year_month())? + 1)
								.max(1);
						let monthly = to_money(ceil_div(
							i128::from(remaining.minor()),
							i128::from(months_left),
						))?;
						Suggestion {
							category: id.clone(),
							amount: Some(monthly),
							basis: Basis::Goal,
							reason: format!(
								"{name} {}: {} left over {months_left} months",
								cents(monthly),
								cents(remaining)
							),
						}
					}
				},
				(CategoryKind::Expense, None) => self.expense_suggestion(
					id,
					name,
					month,
					&in_month,
					baselines.get(id).copied(),
					&keeping,
				)?,
				(CategoryKind::DebtPrincipal, _) => match self.last_principal(id, today)? {
					None => keeping(Basis::NoHistory, &format!("{name}: no history yet")),
					Some(principal) => {
						let amount = round_dollars(principal)?;
						Suggestion {
							category: id.clone(),
							amount: Some(amount),
							basis: Basis::LoanPrincipal,
							reason: format!(
								"{name} {}, principal of the last loan payment",
								dollars(amount)
							),
						}
					}
				},
				(CategoryKind::Income | CategoryKind::Savings, _) => continue,
			};
			items.push(item);
		}

		let mut expected_income = 0i64;
		for flow in in_month.iter().filter(|flow| flow.kind == FlowKind::Income) {
			expected_income = expected_income
				.checked_add(flow.amount.minor())
				.ok_or(CoreError::Overflow)?;
		}
		for baseline in &all_baselines {
			if baseline.side == Side::Income {
				expected_income = expected_income
					.checked_add(baseline.forecast_plus(month)?.minor())
					.ok_or(CoreError::Overflow)?;
			}
		}
		let suggested = Money::checked_sum(items.iter().filter_map(|item| item.amount))?;
		let excess = suggested.checked_sub(Money::from_minor(expected_income))?;
		let exceeds_income_by = (excess > Money::ZERO).then_some(excess);
		let notice = match exceeds_income_by {
			Some(by) => Some(format!(
				"Suggestions exceed expected income by {}",
				dollars(round_dollars(by)?.max(Money::from_minor(100)))
			)),
			None => None,
		};
		Ok(Suggestions {
			month,
			items,
			expected_income: Money::from_minor(expected_income),
			exceeds_income_by,
			notice,
		})
	}

	fn expense_suggestion(
		&self,
		id: &CategoryId,
		name: &str,
		month: YearMonth,
		in_month: &[&KnownFlow],
		baseline: Option<&Baseline>,
		keeping: &dyn Fn(Basis, &str) -> Suggestion,
	) -> Result<Suggestion, CoreError> {
		let bills: Vec<&&KnownFlow> = in_month
			.iter()
			.filter(|flow| flow.kind == FlowKind::Expense && flow.category.as_ref() == Some(id))
			.collect();
		let bill_total = Money::checked_sum(bills.iter().map(|flow| flow.amount))?.checked_neg()?;
		let has_history = baseline.is_some_and(Baseline::has_history);
		if bills.is_empty() && !has_history {
			return Ok(keeping(
				Basis::NoHistory,
				&format!("{name}: no history yet"),
			));
		}
		let forecast = match baseline {
			Some(baseline) if has_history => baseline.forecast_plus(month)?,
			_ => Money::ZERO,
		};
		let amount = round_dollars(bill_total.checked_add(forecast)?.max(Money::ZERO))?;
		let median = baseline.map(Baseline::label).unwrap_or_default();
		let (basis, reason) = if bills.is_empty() {
			let index = baseline.map_or(10_000, |baseline| baseline.index(month));
			if index == 10_000 {
				(
					Basis::Baseline,
					format!("{name} {}, {median}", dollars(amount)),
				)
			} else {
				let pct = crate::math::round_half_away(i128::from(index) - 10_000, 100);
				let sign = if pct < 0 { "-" } else { "+" };
				let month_only = month_name(month);
				let month_only = month_only.split(' ').next().unwrap_or_default();
				(
					Basis::Seasonal,
					format!(
						"{name} {}, {median}, adjusted {sign}{}% for {month_only}",
						dollars(amount),
						pct.abs()
					),
				)
			}
		} else if forecast == Money::ZERO {
			let mut names: Vec<&str> = Vec::new();
			for bill in &bills {
				if !names.contains(&bill.name.as_str()) {
					names.push(&bill.name);
				}
			}
			(
				Basis::Bills,
				format!(
					"{name} {}, scheduled: {}",
					dollars(amount),
					names.join(", ")
				),
			)
		} else {
			let bills_rounded = round_dollars(bill_total)?;
			let rest = amount.checked_sub(bills_rounded)?.max(Money::ZERO);
			(
				Basis::BillsAndBaseline,
				format!(
					"{name} {}: {} scheduled + {} {median}",
					dollars(amount),
					dollars(bills_rounded),
					dollars(rest)
				),
			)
		};
		Ok(Suggestion {
			category: id.clone(),
			amount: Some(amount),
			basis,
			reason,
		})
	}

	/// DEBT-3 source 1: the latest principal paid from this envelope, capped
	/// at what the loan still owes today.
	fn last_principal(
		&self,
		id: &CategoryId,
		today: CivilDate,
	) -> Result<Option<Money>, CoreError> {
		let Some(row) = self
			.classification
			.rows()
			.iter()
			.rev()
			.filter(|row| row.date <= today)
			.find(|row| {
				row.is_primary()
					&& row.kind() == TransactionKind::DebtPrincipal
					&& row.category.as_ref() == Some(id)
			})
		else {
			return Ok(None);
		};
		let principal = row.amount.checked_neg()?.max(Money::ZERO);
		let outstanding = match &row.transfer_account {
			Some(loan) => balances(self.budget, &self.classification, today)?
				.get(loan)
				.map(|balance| Money::from_minor(*balance).checked_neg())
				.transpose()?
				.map(|owed| owed.max(Money::ZERO)),
			None => None,
		};
		Ok(Some(
			outstanding.map_or(principal, |owed| principal.min(owed)),
		))
	}
}
