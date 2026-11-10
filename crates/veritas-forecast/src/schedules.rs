use veritas_domain::{
	AccountId, AccountKind, CategoryId, CategoryKind, CivilDate, ConditionsOp, CoreError, Money,
	PayeeId, Schedule, ScheduleId, TransactionKind,
};
use veritas_reports::{ClassifiedRow, Period};
use veritas_rules::{
	Action, AmountSign, ConditionValue, DateValue, Field, Op, ParsedRule, SetField, schedule_dates,
};

use crate::flows::{FlowKind, KnownFlow, LoanPayment, ScheduleAmount};
use crate::history::balances;
use crate::math::round_half_up;
use crate::projection::Forecaster;

/// A schedule or loan the forecast couldn't turn into flows, and why. It is
/// listed next to the forecast, never silently dropped.
#[derive(Clone, Debug, PartialEq)]
pub struct Unforecast {
	pub schedule: Option<ScheduleId>,
	pub loan: Option<AccountId>,
	pub reason: String,
}

/// Known flows (FC-2) from the budget's bills, income schedules and loans.
#[derive(Clone, Debug, PartialEq)]
pub struct ScheduledFlows {
	pub flows: Vec<KnownFlow>,
	/// Unpaid occurrences dated today or earlier: Actual's next date is in
	/// the past, so the bill is due or missed. They are not in `flows`.
	pub overdue: Vec<(ScheduleId, CivilDate)>,
	pub unforecast: Vec<Unforecast>,
}

/// What a schedule's rule says the transaction looks like.
struct Bill {
	date: DateValue,
	account: AccountId,
	payee: Option<String>,
	amount: ScheduleAmount,
	category: Option<CategoryId>,
}

fn cents(value: f64) -> Result<Money, String> {
	let rounded = value.round();
	if !value.is_finite() || rounded != value || rounded.abs() > 9.0e15 {
		return Err(format!("the amount {value} isn't a whole number of cents"));
	}
	Ok(Money::from_minor(rounded as i64))
}

fn bill(rule: &ParsedRule) -> Result<Bill, String> {
	if let Some(reason) = &rule.unreadable {
		return Err(reason.clone());
	}
	if rule.conditions_op != ConditionsOp::And {
		return Err("its rule matches any condition, not all of them".to_owned());
	}
	let (mut date, mut account, mut payee, mut amount, mut category) =
		(None, None, None, None, None);
	for item in &rule.conditions {
		let Ok(condition) = &item.parsed else {
			return Err("one of its conditions can't be read".to_owned());
		};
		let signed = |value: f64| {
			cents(value).map(|amount| match condition.sign {
				AmountSign::Outflow => Money::from_minor(-amount.minor()),
				AmountSign::Inflow | AmountSign::Any => amount,
			})
		};
		match (condition.field, condition.op, &condition.value) {
			(Field::Date, _, ConditionValue::Date(value)) => date = Some(value.clone()),
			(Field::Account, Op::Is, ConditionValue::Id(Some(id))) => {
				account = Some(AccountId::new(id.clone()));
			}
			(Field::Payee, Op::Is, ConditionValue::Id(id)) => payee = id.clone(),
			(Field::Amount, Op::Is, ConditionValue::Number(value)) => {
				amount = Some(ScheduleAmount::Is(signed(*value)?));
			}
			(Field::Amount, Op::IsApprox, ConditionValue::Number(value)) => {
				amount = Some(ScheduleAmount::IsApprox(signed(*value)?));
			}
			(Field::Amount, Op::IsBetween, ConditionValue::Between(low, high)) => {
				amount = Some(ScheduleAmount::IsBetween(signed(*low)?, signed(*high)?));
			}
			_ => {}
		}
	}
	for item in &rule.actions {
		if let Ok(Action::Set(SetField::Category(Some(id)))) = &item.parsed {
			category = Some(id.clone());
		}
	}
	Ok(Bill {
		date: date.ok_or("it has no date")?,
		account: account.ok_or("it has no account")?,
		payee,
		amount: amount.ok_or("it has no amount")?,
		category,
	})
}

impl Forecaster<'_> {
	/// The last day FC-3 projects: the end of the 12th month after today's.
	pub fn horizon_end(today: CivilDate) -> Result<CivilDate, CoreError> {
		Ok(Period::month(today.year_month().offset(12)?).last())
	}

	/// Every active bill and income schedule expanded into dated flows after
	/// `today` through `through` with Actual's recurrence, plus loan payments
	/// from loan metadata where no schedule pays the loan.
	pub fn scheduled_flows(
		&self,
		today: CivilDate,
		through: CivilDate,
	) -> Result<ScheduledFlows, CoreError> {
		let mut result = ScheduledFlows {
			flows: Vec::new(),
			overdue: Vec::new(),
			unforecast: Vec::new(),
		};
		for schedule in self.budget.schedules.values() {
			if schedule.tombstone || schedule.completed {
				continue;
			}
			if let Err(reason) = self.schedule_flows(schedule, today, through, &mut result)? {
				result.unforecast.push(Unforecast {
					schedule: Some(schedule.id.clone()),
					loan: None,
					reason,
				});
			}
		}
		self.loan_flows(today, through, &mut result)?;
		result
			.flows
			.sort_by(|a, b| a.date.cmp(&b.date).then_with(|| a.name.cmp(&b.name)));
		Ok(result)
	}

	fn schedule_flows(
		&self,
		schedule: &Schedule,
		today: CivilDate,
		through: CivilDate,
		result: &mut ScheduledFlows,
	) -> Result<Result<(), String>, CoreError> {
		let Some(rule) = self
			.budget
			.rules
			.get(&schedule.rule)
			.filter(|rule| !rule.tombstone)
		else {
			return Ok(Err("its rule is missing".to_owned()));
		};
		let bill = match bill(&ParsedRule::parse(rule)) {
			Ok(bill) => bill,
			Err(reason) => return Ok(Err(reason)),
		};
		let tomorrow = today
			.naive()
			.succ_opt()
			.map(CivilDate::from)
			.ok_or(CoreError::Overflow)?;
		let start = schedule.next_date.unwrap_or(tomorrow);
		if start > through {
			return Ok(Ok(()));
		}
		let dates = match schedule_dates(&bill.date, start, Some(through), None) {
			Ok(dates) => dates,
			Err(error) => return Ok(Err(error.to_string())),
		};
		let (amount, certainty) = bill.amount.per_occurrence(self.budget, &schedule.id)?;
		let payee = bill.payee.as_ref().map(|id| PayeeId::new(id.clone()));
		let resolved = payee
			.as_ref()
			.and_then(|payee| self.budget.resolve_payee(payee))
			.and_then(|payee| self.budget.payees.get(payee));
		let kind = match (
			resolved.and_then(|payee| payee.transfer_account.clone()),
			&bill.category,
		) {
			(Some(counterpart), _) => FlowKind::Transfer { counterpart },
			(None, Some(category))
				if self.budget.category_kind(category) == CategoryKind::Income =>
			{
				FlowKind::Income
			}
			(None, _) => FlowKind::Expense,
		};
		let name = schedule
			.name
			.clone()
			.or_else(|| resolved.map(|payee| payee.name.clone()))
			.unwrap_or_default();
		for date in dates {
			if date <= today {
				result.overdue.push((schedule.id.clone(), date));
				continue;
			}
			result.flows.push(KnownFlow {
				date,
				account: bill.account.clone(),
				amount,
				kind: kind.clone(),
				category: bill.category.clone(),
				schedule: Some(schedule.id.clone()),
				name: name.clone(),
				certainty,
			});
		}
		Ok(Ok(()))
	}

	/// FC-2 loan payments for loans with a payment amount and day in their
	/// metadata, paid from the account and categories of the last payment.
	fn loan_flows(
		&self,
		today: CivilDate,
		through: CivilDate,
		result: &mut ScheduledFlows,
	) -> Result<(), CoreError> {
		let scheduled: Vec<AccountId> = result
			.flows
			.iter()
			.filter_map(|flow| match &flow.kind {
				FlowKind::Transfer { counterpart } => Some(counterpart.clone()),
				_ => None,
			})
			.collect();
		let owed = balances(self.budget, &self.classification, today)?;
		for (loan, meta) in &self.budget.account_meta {
			let (Some(payment), Some(day)) = (meta.loan_payment, meta.loan_payment_day) else {
				continue;
			};
			if meta.kind != Some(AccountKind::Liability) || scheduled.contains(loan) {
				continue;
			}
			let unforecast = |reason: &str| Unforecast {
				schedule: None,
				loan: Some(loan.clone()),
				reason: reason.to_owned(),
			};
			let Some(last) = self.classification.rows().iter().rev().find(|row| {
				row.date <= today
					&& row.is_primary()
					&& row.kind() == TransactionKind::DebtPrincipal
					&& row.transfer_account.as_ref() == Some(loan)
			}) else {
				result.unforecast.push(unforecast(
					"no payment to this loan is recorded yet, so the paying account is unknown",
				));
				continue;
			};
			let parts: Vec<&ClassifiedRow> = match &last.parent {
				Some(parent) => self
					.classification
					.rows()
					.iter()
					.filter(|row| {
						row.parent.as_ref() == Some(parent)
							&& row.kind() == TransactionKind::Expense
							&& row.id != last.id
					})
					.collect(),
				None => Vec::new(),
			};
			let (interest, escrow) = match (parts.as_slice(), meta.loan_rate_bps) {
				([], _) => (None, Vec::new()),
				([only], _) => (Some(*only), Vec::new()),
				(_, Some(rate)) => {
					let before = last
						.date
						.naive()
						.pred_opt()
						.map_or(last.date, CivilDate::from);
					let owed_then = balances(self.budget, &self.classification, before)?
						.get(loan)
						.copied()
						.unwrap_or(0);
					let estimate =
						round_half_up(i128::from(owed_then).abs() * i128::from(rate), 120_000);
					let closest = parts
						.iter()
						.min_by_key(|row| (i128::from(row.amount.minor()).abs() - estimate).abs())
						.copied();
					let rest = parts
						.iter()
						.filter(|row| Some(row.id.clone()) != closest.map(|row| row.id.clone()))
						.copied()
						.collect();
					(closest, rest)
				}
				(_, None) => {
					result.unforecast.push(unforecast(
						"the last payment has several expense parts and no rate tells interest from escrow",
					));
					continue;
				}
			};
			let magnitude = |rows: &[&ClassifiedRow]| -> Result<Money, CoreError> {
				Money::checked_sum(rows.iter().map(|row| row.amount))?.checked_neg()
			};
			let mut dates = Vec::new();
			let mut month = today.year_month();
			while month <= through.year_month() {
				let period = Period::month(month);
				let date = CivilDate::new(month.year(), month.month(), u32::from(day))
					.unwrap_or(period.last());
				if today < date && date <= through {
					dates.push(date);
				}
				month = month.offset(1)?;
			}
			let flows = LoanPayment {
				from: last.account.clone(),
				loan: loan.clone(),
				name: self
					.budget
					.accounts
					.get(loan)
					.map(|account| account.name.clone())
					.unwrap_or_default(),
				payment,
				rate_bps: meta.loan_rate_bps,
				last_interest: magnitude(interest.as_slice())?,
				escrow: magnitude(&escrow)?,
				outstanding: Money::from_minor(owed.get(loan).copied().unwrap_or(0))
					.checked_neg()?,
				interest_category: interest.and_then(|row| row.category.clone()),
				escrow_category: escrow.first().and_then(|row| row.category.clone()),
				principal_category: last.category.clone(),
			}
			.flows(&dates)?;
			result.flows.extend(flows);
		}
		Ok(())
	}
}
