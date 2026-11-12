use veritas_domain::{
	AccountId, BudgetData, CategoryId, CivilDate, CoreError, Money, ScheduleId, SplitRole,
	YearMonth,
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

/// A payment date where the payment is below interest plus escrow. The
/// uncovered part is not recorded (DEBT-2.6); the screen warns (DEBT-2.2).
#[derive(Clone, Debug, PartialEq)]
pub struct Shortfall {
	pub date: CivilDate,
	pub loan: AccountId,
	pub uncovered: Money,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LoanSplit {
	pub flows: Vec<KnownFlow>,
	pub shortfalls: Vec<Shortfall>,
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
	/// The last payment's escrow (DEBT-3), repeated every payment.
	pub escrow: Money,
	/// `-balance` of the loan account before the first payment.
	pub outstanding: Money,
	pub interest_category: Option<CategoryId>,
	pub escrow_category: Option<CategoryId>,
	pub principal_category: Option<CategoryId>,
}

impl LoanPayment {
	/// FC-2's split of each payment, in DEBT-2.1a's order: the payment covers interest
	/// (`round_half_up(outstanding * rate_bps / 120000)`), then escrow, then
	/// principal, which is capped at what is still owed so the last payment is
	/// smaller. A shortfall never capitalizes (DEBT-2.2, DEBT-2.6): interest
	/// and escrow are booked up to what the payment covers, and the loan
	/// balance moves only by principal.
	pub fn flows(&self, dates: &[CivilDate]) -> Result<Vec<KnownFlow>, CoreError> {
		Ok(self.split(dates)?.flows)
	}

	/// The same flows plus the dates where the payment doesn't cover interest
	/// and escrow, which the screen warns about (FC-2, DEBT-2.2).
	pub fn split(&self, dates: &[CivilDate]) -> Result<LoanSplit, CoreError> {
		self.split_from(dates, None)
	}

	/// `split` when part of the first month is already paid: FC-2 starts
	/// that month from today's balance plus the principal already paid, and
	/// books only the month's interest not yet booked.
	pub fn split_from(
		&self,
		dates: &[CivilDate],
		so_far: Option<MonthSoFar>,
	) -> Result<LoanSplit, CoreError> {
		let payment = self.payment.minor();
		let escrow = self.escrow.minor();
		if payment < 0 || escrow < 0 {
			return Err(CoreError::Invalid(format!(
				"{}: a loan payment and its escrow can't be negative",
				self.name
			)));
		}
		let mut outstanding = self.outstanding.minor().max(0);
		let mut flows = Vec::new();
		let mut shortfalls = Vec::new();
		let mut start = 0;
		while let Some(first) = dates.get(start) {
			if outstanding == 0 {
				break;
			}
			let month = first.year_month();
			let count = dates[start..]
				.iter()
				.take_while(|date| date.year_month() == month)
				.count();
			let shares = self.month_interest(outstanding, month, count, so_far)?;
			for (date, interest) in dates[start..start + count].iter().zip(shares) {
				if outstanding == 0 {
					break;
				}
				let owed = interest.checked_add(escrow).ok_or(CoreError::Overflow)?;
				if payment < owed {
					shortfalls.push(Shortfall {
						date: *date,
						loan: self.loan.clone(),
						uncovered: Money::from_minor(owed - payment),
					});
				}
				let paid_interest = interest.min(payment);
				let paid_escrow = escrow.min(payment - paid_interest);
				let principal = (payment - paid_interest - paid_escrow).min(outstanding);
				outstanding = outstanding
					.checked_sub(principal)
					.ok_or(CoreError::Overflow)?;
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
				for (amount, category) in [
					(paid_interest, &self.interest_category),
					(paid_escrow, &self.escrow_category),
				] {
					if amount > 0 {
						flows.push(flow(amount, FlowKind::Expense, category));
					}
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
			start += count;
		}
		Ok(LoanSplit { flows, shortfalls })
	}

	/// FC-2: interest accrues per calendar month and is shared among that
	/// month's `count` payments, leftover cents to the earliest. With no rate
	/// each payment repeats the last payment's interest.
	fn month_interest(
		&self,
		outstanding: i64,
		month: YearMonth,
		count: usize,
		so_far: Option<MonthSoFar>,
	) -> Result<Vec<i64>, CoreError> {
		let Some(rate) = self.rate_bps else {
			return Ok(vec![self.last_interest.minor().max(0); count]);
		};
		let this_month = so_far.filter(|so_far| so_far.month == month);
		let start = match this_month {
			Some(so_far) => outstanding
				.checked_add(so_far.principal.minor())
				.ok_or(CoreError::Overflow)?,
			None => outstanding,
		};
		let total = i64::try_from(round_half_up(i128::from(start) * i128::from(rate), 120_000))
			.map_err(|_| CoreError::Overflow)?;
		let booked = this_month.map_or(0, |so_far| so_far.interest.minor());
		let total = total.checked_sub(booked).ok_or(CoreError::Overflow)?.max(0);
		let n = i64::try_from(count)
			.map_err(|_| CoreError::Overflow)?
			.max(1);
		let (share, extra) = (total / n, total % n);
		Ok((0..n).map(|k| share + i64::from(k < extra)).collect())
	}
}

/// What a loan has already been paid in the month a split starts in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MonthSoFar {
	pub month: YearMonth,
	pub interest: Money,
	pub principal: Money,
}
