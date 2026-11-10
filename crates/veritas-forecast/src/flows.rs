use veritas_domain::{
	AccountId, BudgetData, CategoryId, CivilDate, CoreError, Money, ScheduleId, SplitRole,
};

use crate::math::{median, round_half_away, round_half_up, to_money};

/// How a known flow moves net worth (FC-2, FC-3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlowKind {
	Income,
	Expense,
	/// Money between two of the user's accounts: `account` moves by `amount`,
	/// `counterpart` by `-amount`, and net worth doesn't move. Card payments,
	/// savings moves and loan principal are transfers.
	Transfer {
		counterpart: AccountId,
	},
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Certainty {
	/// An exact bill, exact income or a loan payment.
	Exact,
	/// An `isapprox` or `isbetween` amount.
	Estimate,
}

/// One dated cash flow the forecast knows about ahead of time: a bill or
/// income schedule occurrence, a loan payment part. Built by the caller from
/// the recurrence engine; the forecast only reads the list.
#[derive(Clone, Debug, PartialEq)]
pub struct KnownFlow {
	pub date: CivilDate,
	pub account: AccountId,
	/// Actual's sign, seen from `account`.
	pub amount: Money,
	pub kind: FlowKind,
	pub category: Option<CategoryId>,
	pub schedule: Option<ScheduleId>,
	pub name: String,
	pub certainty: Certainty,
}

impl KnownFlow {
	/// Its effect on net worth: the amount for income and expenses, 0 for transfers.
	pub fn net_income(&self) -> Money {
		match self.kind {
			FlowKind::Income | FlowKind::Expense => self.amount,
			FlowKind::Transfer { .. } => Money::ZERO,
		}
	}
}

/// A schedule's amount condition (`amount` with `is`, `isapprox` or `isbetween`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleAmount {
	Is(Money),
	IsApprox(Money),
	IsBetween(Money, Money),
}

impl ScheduleAmount {
	/// FC-2's amount per occurrence. `isapprox` uses the median of the last
	/// up to `WINDOW` transactions linked to the schedule, else the stated amount.
	pub fn per_occurrence(
		self,
		budget: &BudgetData,
		schedule: &ScheduleId,
	) -> Result<(Money, Certainty), CoreError> {
		match self {
			ScheduleAmount::Is(amount) => Ok((amount, Certainty::Exact)),
			ScheduleAmount::IsBetween(low, high) => Ok((
				to_money(round_half_away(
					i128::from(low.minor()) + i128::from(high.minor()),
					2,
				))?,
				Certainty::Estimate,
			)),
			ScheduleAmount::IsApprox(stated) => {
				let mut linked: Vec<_> = budget
					.transactions
					.values()
					.filter(|row| {
						!row.tombstone
							&& row.schedule.as_ref() == Some(schedule)
							&& !matches!(row.split, SplitRole::Child { .. })
					})
					.map(|row| (row.date, row.id.clone(), row.amount.minor()))
					.collect();
				linked.sort();
				let recent: Vec<i64> = linked
					.iter()
					.rev()
					.take(usize::try_from(crate::baseline::WINDOW).unwrap_or(6))
					.map(|(_, _, amount)| *amount)
					.collect();
				let amount = median(&recent).map_or(stated, Money::from_minor);
				Ok((amount, Certainty::Estimate))
			}
		}
	}
}

/// A loan payment schedule to expand into known flows (FC-2).
#[derive(Clone, Debug, PartialEq)]
pub struct LoanPayment {
	pub from: AccountId,
	pub loan: AccountId,
	pub name: String,
	/// The whole payment, a positive magnitude.
	pub payment: Money,
	pub rate_bps: Option<i64>,
	/// The last payment's interest, used when there's no rate.
	pub last_interest: Money,
	/// `-balance` of the loan account before the first payment.
	pub outstanding: Money,
	pub interest_category: Option<CategoryId>,
	pub principal_category: Option<CategoryId>,
}

impl LoanPayment {
	/// An interest expense and a principal transfer per date. Interest is
	/// `round_half_up(outstanding * rate_bps / 120000)`; principal is the rest,
	/// capped at what is still owed, so the last payment is smaller.
	pub fn flows(&self, dates: &[CivilDate]) -> Result<Vec<KnownFlow>, CoreError> {
		let mut outstanding = self.outstanding.minor().max(0);
		let mut flows = Vec::new();
		for date in dates {
			if outstanding == 0 {
				break;
			}
			let interest = match self.rate_bps {
				Some(rate) => i64::try_from(round_half_up(
					i128::from(outstanding) * i128::from(rate),
					120_000,
				))
				.map_err(|_| CoreError::Overflow)?,
				None => self.last_interest.minor(),
			}
			.max(0);
			let principal = (self.payment.minor() - interest).clamp(0, outstanding);
			outstanding -= principal;
			let flow = |amount: i64, kind: FlowKind, category: &Option<CategoryId>| KnownFlow {
				date: *date,
				account: self.from.clone(),
				amount: Money::from_minor(-amount),
				kind,
				category: category.clone(),
				schedule: None,
				name: self.name.clone(),
				certainty: Certainty::Exact,
			};
			if interest > 0 {
				flows.push(flow(interest, FlowKind::Expense, &self.interest_category));
			}
			if principal > 0 {
				flows.push(flow(
					principal,
					FlowKind::Transfer {
						counterpart: self.loan.clone(),
					},
					&self.principal_category,
				));
			}
		}
		Ok(flows)
	}
}
