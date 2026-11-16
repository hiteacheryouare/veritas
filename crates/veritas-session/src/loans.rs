//! Loans (R-DEBT-1, R-DEBT-2, docs/finance/debt.md DEBT-1..6): their terms,
//! the payment wizard's arithmetic and pre-fill, and the split a payment is
//! stored as.

use chrono::Days;
use veritas_domain::{
	AccountId, AccountKind, AccountMeta, AccountMetaRow, BudgetData, CategoryId, CategoryKind,
	CivilDate, CoreError, LoanTerms, Money, Row, SplitRole, Transaction, TransactionId,
};
use veritas_engine::{BudgetEditor, EnvelopeSheet, NumberFormat, register_currency_text};
use veritas_rules::RuleBook;

use crate::accounts::live_account;
use crate::transactions::{
	self, Draft, Line, Planned, alive, children, payee_named, register_order, storable,
	transfer_payee,
};

/// Terms as stored: amounts storable, the day 1 to 31, a blank lender none.
pub fn checked_terms(terms: &LoanTerms) -> Result<LoanTerms, CoreError> {
	if terms.rate_bps.is_some_and(|rate| rate < 0) {
		return Err(CoreError::Invalid(
			"The interest rate can't be negative.".to_owned(),
		));
	}
	if let Some(payment) = terms.payment {
		if payment.is_negative() {
			return Err(CoreError::Invalid(
				"The payment can't be negative.".to_owned(),
			));
		}
		storable(payment)?;
	}
	if terms
		.payment_day
		.is_some_and(|day| !(1..=31).contains(&day))
	{
		return Err(CoreError::Invalid(
			"The payment day is a day of the month, 1 to 31.".to_owned(),
		));
	}
	Ok(LoanTerms {
		lender: terms
			.lender
			.as_deref()
			.map(str::trim)
			.filter(|lender| !lender.is_empty())
			.map(str::to_owned),
		..terms.clone()
	})
}

/// The account's balance at the end of `date`: live rows, split parents left out.
pub fn balance_on(
	data: &BudgetData,
	account: &AccountId,
	date: CivilDate,
) -> Result<Money, CoreError> {
	let mut balance = Money::ZERO;
	for row in data.transactions.values() {
		if &row.account == account
			&& row.date <= date
			&& row.split != SplitRole::Parent
			&& alive(data, row)
		{
			balance = balance.checked_add(row.amount)?;
		}
	}
	Ok(balance)
}

/// Off budget and a liability by docs/finance/statements.md ST-9.1, as of
/// `today`: the sidebar's Loans group (ST-9.2).
pub fn in_loans(
	data: &BudgetData,
	account: &AccountId,
	today: CivilDate,
) -> Result<bool, CoreError> {
	let found = live_account(data, account)?;
	if !found.off_budget {
		return Ok(false);
	}
	let balance = balance_on(data, account, today)?;
	Ok(crate::overview::placement(data, account, balance).0)
}

/// The loan's terms, or `None` when ST-9.1 doesn't place the account in Loans.
pub fn loan_terms(
	data: &BudgetData,
	account: &AccountId,
	today: CivilDate,
) -> Result<Option<LoanTerms>, CoreError> {
	if !in_loans(data, account, today)? {
		return Ok(None);
	}
	let meta = data.account_meta.get(account).cloned().unwrap_or_default();
	Ok(Some(LoanTerms {
		rate_bps: meta.loan_rate_bps,
		payment: meta.loan_payment,
		payment_day: meta.loan_payment_day,
		lender: meta.lender,
	}))
}

/// Replaces the loan's terms; a `None` field clears it. The account's kind
/// is kept. An on-budget account, or one pinned as an asset, takes no terms.
pub fn set_loan_terms(
	data: &BudgetData,
	account: &AccountId,
	terms: &LoanTerms,
) -> Result<Vec<Row>, CoreError> {
	let found = live_account(data, account)?;
	let meta = data.account_meta.get(account).cloned().unwrap_or_default();
	if !found.off_budget || meta.kind == Some(AccountKind::Asset) {
		return Err(CoreError::Invalid(
			"Only an off-budget loan account has loan terms.".to_owned(),
		));
	}
	let terms = checked_terms(terms)?;
	Ok(vec![Row::AccountMeta(AccountMetaRow {
		account: account.clone(),
		meta: AccountMeta {
			kind: meta.kind,
			loan_rate_bps: terms.rate_bps,
			loan_payment: terms.payment,
			loan_payment_day: terms.payment_day,
			lender: terms.lender,
		},
	})])
}

fn day_before(date: CivilDate) -> Result<CivilDate, CoreError> {
	date.naive()
		.checked_sub_days(Days::new(1))
		.map(CivilDate::from)
		.ok_or_else(|| CoreError::Invalid("That date is out of range.".to_owned()))
}

/// DEBT-1: what is owed the day before `date`, over all live rows, never
/// below 0 (an overpaid loan owes nothing).
pub fn outstanding(
	data: &BudgetData,
	loan: &AccountId,
	date: CivilDate,
) -> Result<Money, CoreError> {
	let owed = balance_on(data, loan, day_before(date)?)?.checked_neg()?;
	Ok(owed.max(Money::ZERO))
}

/// DEBT-4: `round_half_up(outstanding * rate_bps / 120000)`, a hint only.
pub fn interest_estimate(outstanding: Money, rate_bps: i64) -> Result<Money, CoreError> {
	let product = i128::from(outstanding.minor()) * i128::from(rate_bps);
	let rounded = (product + 60_000).div_euclid(120_000);
	i64::try_from(rounded)
		.map(Money::from_minor)
		.map_err(|_| CoreError::Overflow)
}

/// One payment's parts as typed (DEBT-2): all at least 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parts {
	pub total: Money,
	pub interest: Money,
	pub escrow: Money,
	pub fees: Money,
}

impl Parts {
	/// DEBT-2.1: the remainder, so no cent goes missing. Negative is DEBT-2.2.
	pub fn principal(&self) -> Result<Money, CoreError> {
		for part in [self.total, self.interest, self.escrow, self.fees] {
			if part.is_negative() {
				return Err(CoreError::Invalid(
					"Enter each part of the payment as a positive amount.".to_owned(),
				));
			}
			storable(part)?;
		}
		self.total
			.checked_sub(self.interest)?
			.checked_sub(self.escrow)?
			.checked_sub(self.fees)
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaymentProblem {
	/// DEBT-2.2: interest, escrow and fees together are more than the
	/// payment. Blocks saving.
	PaymentTooSmall { short_by: Money },
	/// DEBT-2.3: principal is more than what is owed. A warning.
	PaysOff { credit: Money },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaymentPreview {
	pub principal: Money,
	pub outstanding: Money,
	/// DEBT-4's hint, when the rate is set.
	pub estimate: Option<Money>,
	pub rate_bps: Option<i64>,
	pub problem: Option<PaymentProblem>,
	/// What `fund_principal` would budget for this principal (A.6).
	pub principal_funding: Money,
}

fn loan_account(data: &BudgetData, loan: &AccountId, date: CivilDate) -> Result<(), CoreError> {
	if !in_loans(data, loan, date)? {
		return Err(CoreError::Invalid(
			"That account isn't a loan. Pick an off-budget account that is owed.".to_owned(),
		));
	}
	Ok(())
}

pub fn payment_preview(
	data: &BudgetData,
	loan: &AccountId,
	date: CivilDate,
	parts: &Parts,
) -> Result<PaymentPreview, CoreError> {
	loan_account(data, loan, date)?;
	let principal = parts.principal()?;
	let outstanding = outstanding(data, loan, date)?;
	let rate_bps = data.account_meta.get(loan).and_then(|m| m.loan_rate_bps);
	let estimate = rate_bps
		.map(|rate| interest_estimate(outstanding, rate))
		.transpose()?;
	let problem = if principal.is_negative() {
		Some(PaymentProblem::PaymentTooSmall {
			short_by: principal.checked_neg()?,
		})
	} else if principal > outstanding {
		Some(PaymentProblem::PaysOff {
			credit: principal.checked_sub(outstanding)?,
		})
	} else {
		None
	};
	Ok(PaymentPreview {
		principal,
		outstanding,
		estimate,
		rate_bps,
		problem,
		principal_funding: principal_funding(data, date, principal)?,
	})
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaymentDefaults {
	pub from: Option<AccountId>,
	pub total: Option<Money>,
	pub interest: Option<Money>,
	pub escrow: Option<Money>,
	pub principal_category: Option<CategoryId>,
	pub interest_category: Option<CategoryId>,
	pub escrow_category: Option<CategoryId>,
}

/// The last payment into `loan` (DEBT-3 source 1): its parent, principal
/// line and the expense lines after it in Actual's split order.
struct LastPayment<'a> {
	parent: &'a Transaction,
	principal: &'a Transaction,
	expenses: Vec<&'a Transaction>,
}

fn kind_of(data: &BudgetData, row: &Transaction) -> Option<CategoryKind> {
	let category = row.category.as_ref()?;
	let category = data.resolve_category(category).unwrap_or(category);
	data.categories
		.get(category)
		.filter(|c| !c.tombstone)
		.map(|c| data.category_kind(&c.id))
}

fn last_payment<'a>(data: &'a BudgetData, loan: &AccountId) -> Option<LastPayment<'a>> {
	let mut legs: Vec<&Transaction> = data
		.transactions
		.values()
		.filter(|row| &row.account == loan && row.transfer.is_some() && alive(data, row))
		.collect();
	legs.sort_by(|a, b| register_order(a, b));
	legs.into_iter().find_map(|leg| {
		let principal = data
			.transactions
			.get(leg.transfer.as_ref()?)
			.filter(|line| alive(data, line))?;
		let SplitRole::Child { parent } = &principal.split else {
			return None;
		};
		if kind_of(data, principal) != Some(CategoryKind::DebtPrincipal) {
			return None;
		}
		let parent = data.transactions.get(parent)?;
		let on_budget = data
			.accounts
			.get(&parent.account)
			.is_some_and(|a| !a.off_budget);
		if !on_budget {
			return None;
		}
		let expenses = children(data, &parent.id)
			.into_iter()
			.filter(|line| line.id != principal.id && line.transfer.is_none())
			.filter(|line| kind_of(data, line) == Some(CategoryKind::Expense))
			.collect();
		Some(LastPayment {
			parent,
			principal,
			expenses,
		})
	})
}

/// DEBT-3: from the last payment (principal line, then the first two expense
/// lines as interest and escrow; the total is their sum, so a fee never
/// lands in principal), else the loan's payment with interest left blank
/// (DEBT-4's estimate is only a hint, never a default), else nothing. Then
/// principal is capped at what is owed the day before `today`, a blank
/// interest counting as 0, shrinking the total by the same amount.
pub fn payment_defaults(
	data: &BudgetData,
	loan: &AccountId,
	today: CivilDate,
) -> Result<PaymentDefaults, CoreError> {
	loan_account(data, loan, today)?;
	let owed = outstanding(data, loan, today)?;
	let mut defaults = match last_payment(data, loan) {
		Some(last) => {
			let principal = last.principal.amount.checked_neg()?;
			let interest = match last.expenses.first() {
				Some(line) => line.amount.checked_neg()?,
				None => Money::ZERO,
			};
			let escrow = match last.expenses.get(1) {
				Some(line) => line.amount.checked_neg()?,
				None => Money::ZERO,
			};
			PaymentDefaults {
				from: Some(last.parent.account.clone()),
				total: Some(principal.checked_add(interest)?.checked_add(escrow)?),
				interest: Some(interest),
				escrow: Some(escrow),
				principal_category: last.principal.category.clone(),
				interest_category: last.expenses.first().and_then(|l| l.category.clone()),
				escrow_category: last.expenses.get(1).and_then(|l| l.category.clone()),
			}
		}
		None => {
			let payment = data.account_meta.get(loan).and_then(|m| m.loan_payment);
			match payment {
				Some(payment) => PaymentDefaults {
					total: Some(payment),
					escrow: Some(Money::ZERO),
					..PaymentDefaults::default()
				},
				None => PaymentDefaults::default(),
			}
		}
	};
	if let Some(total) = defaults.total {
		let principal = total
			.checked_sub(defaults.interest.unwrap_or(Money::ZERO))?
			.checked_sub(defaults.escrow.unwrap_or(Money::ZERO))?;
		if principal > owed {
			defaults.total = Some(total.checked_sub(principal.checked_sub(owed)?)?);
		}
	}
	Ok(defaults)
}

/// What the wizard saves (DEBT-6). `id` names the payment to replace when it
/// is a live split; otherwise it is the new payment's id. A part's category
/// is needed only when the part isn't 0. `fund_principal` also budgets the
/// principal from To Budget into its envelope (A.6, `principal_funding`);
/// it is refused when the draft replaces a payment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaymentDraft {
	pub id: Option<TransactionId>,
	pub from: AccountId,
	pub loan: AccountId,
	pub date: CivilDate,
	pub parts: Parts,
	pub principal_category: CategoryId,
	pub interest_category: Option<CategoryId>,
	pub escrow_category: Option<CategoryId>,
	pub fees_category: Option<CategoryId>,
	pub notes: Option<String>,
	pub cleared: bool,
	pub fund_principal: bool,
}

fn category_of_kind(
	data: &BudgetData,
	category: &CategoryId,
	kind: CategoryKind,
	what: &str,
) -> Result<(), CoreError> {
	data.categories
		.get(category)
		.filter(|c| !c.tombstone)
		.ok_or_else(|| CoreError::NotFound {
			entity: "category",
			id: category.to_string(),
		})?;
	if data.category_kind(category) != kind {
		let wanted = match kind {
			CategoryKind::DebtPrincipal => "a debt principal category",
			_ => "an expense category",
		};
		return Err(CoreError::Invalid(format!("Put the {what} in {wanted}.")));
	}
	Ok(())
}

/// The DEBT-2.2 shortfall as copy shows it: when whole units are shown, a
/// part of a unit rounds up, so the message never names less than is
/// missing (5050 short reads "$51", never "$50").
fn shortfall_shown(short_by: Money, data: &BudgetData) -> Result<Money, CoreError> {
	if !(data.format.hide_fraction || data.format.decimal_places() == 0) {
		return Ok(short_by);
	}
	let whole = short_by
		.minor()
		.checked_add(99)
		.ok_or(CoreError::Overflow)?
		/ 100;
	whole
		.checked_mul(100)
		.map(Money::from_minor)
		.ok_or(CoreError::Overflow)
}

/// DEBT-6's rows: a split from `from` whose payee is the lender, a principal
/// line that transfers to the loan, and expense lines for interest, escrow
/// and fees (parts of 0 get no line). A payment smaller than its other parts
/// is refused (DEBT-2.2). Replacing a payment replaces its lines and their
/// counterparts in the same step (A.2).
pub fn record_payment(
	data: &BudgetData,
	draft: &PaymentDraft,
	book: &RuleBook,
	now_ms: f64,
) -> Result<(Vec<Row>, TransactionId), CoreError> {
	let principal = draft.parts.principal()?;
	if principal.is_negative() {
		return Err(CoreError::Invalid(format!(
			"This payment is {} less than its interest, escrow and fees. Record unpaid interest as an adjustment on the loan instead.",
			register_currency_text(
				shortfall_shown(principal.checked_neg()?, data)?,
				&data.format
			)
		)));
	}
	if draft.parts.total == Money::ZERO {
		return Err(CoreError::Invalid("Enter the payment total.".to_owned()));
	}
	let from = live_account(data, &draft.from)?;
	if from.off_budget {
		return Err(CoreError::Invalid(
			"Pay the loan from an on-budget account.".to_owned(),
		));
	}
	loan_account(data, &draft.loan, draft.date)?;
	let mut lines = Vec::new();
	let mut line = |amount: Money, category: &CategoryId, payee| -> Result<(), CoreError> {
		if amount != Money::ZERO {
			lines.push(Line {
				id: None,
				amount: amount.checked_neg()?,
				payee,
				category: Some(category.clone()),
				notes: None,
			});
		}
		Ok(())
	};
	if principal != Money::ZERO {
		category_of_kind(
			data,
			&draft.principal_category,
			CategoryKind::DebtPrincipal,
			"principal",
		)?;
		let to_loan = transfer_payee(data, &draft.loan).ok_or_else(|| CoreError::NotFound {
			entity: "transfer payee",
			id: draft.loan.to_string(),
		})?;
		line(principal, &draft.principal_category, Some(to_loan))?;
	}
	for (amount, category, what) in [
		(draft.parts.interest, &draft.interest_category, "interest"),
		(draft.parts.escrow, &draft.escrow_category, "escrow"),
		(draft.parts.fees, &draft.fees_category, "fees"),
	] {
		if amount == Money::ZERO {
			continue;
		}
		let category = category
			.as_ref()
			.ok_or_else(|| CoreError::Invalid(format!("Choose a category for the {what}.")))?;
		category_of_kind(data, category, CategoryKind::Expense, what)?;
		line(amount, category, None)?;
	}
	let mut rows = Vec::new();
	let lender = data
		.account_meta
		.get(&draft.loan)
		.and_then(|meta| meta.lender.clone());
	let payee = match lender {
		Some(name) => {
			let (payee_rows, payee) = payee_named(data, &name)?;
			rows.extend(payee_rows);
			Some(payee)
		}
		None => None,
	};
	let with_payee = crate::writes::applied(data, &rows);
	let replacing = draft
		.id
		.as_ref()
		.filter(|id| {
			data.transactions
				.get(*id)
				.is_some_and(|row| alive(data, row) && row.split == SplitRole::Parent)
		})
		.cloned();
	if draft.fund_principal && replacing.is_some() {
		return Err(CoreError::Invalid(
			"Budget the principal when you record a payment, not when you edit one. Move the budget by hand instead.".to_owned(),
		));
	}
	let split = Draft {
		id: if replacing.is_some() {
			None
		} else {
			draft.id.clone()
		},
		account: draft.from.clone(),
		date: draft.date,
		amount: draft.parts.total.checked_neg()?,
		payee,
		category: None,
		notes: draft.notes.clone().filter(|notes| !notes.trim().is_empty()),
		cleared: draft.cleared,
		splits: lines,
	};
	let id = match replacing {
		Some(id) => {
			rows.extend(transactions::edit(
				&with_payee,
				&id,
				&split,
				Some(book),
				now_ms,
			)?);
			id
		}
		None => {
			let Planned {
				rows: written, id, ..
			} = transactions::insert(&with_payee, &split, Some(book), now_ms)?;
			rows.extend(written);
			id
		}
	};
	if draft.fund_principal {
		let with_payment = crate::writes::applied(data, &rows);
		rows.extend(fund_principal(&with_payment, &id, draft.date, principal)?);
	}
	Ok((rows, id))
}

/// DEBT-6 A.6: what funding `principal` budgets from To Budget in the
/// month of `date`, as Actual's `transferAvailable` moves it
/// (`budget/actions.ts:578-593`): the principal on the budget scale, whole
/// units rounded half up in a 0-place budget (CLS-12.11), then clamped to
/// `max(0, min(amount, to_budget))`.
pub fn principal_funding(
	data: &BudgetData,
	date: CivilDate,
	principal: Money,
) -> Result<Money, CoreError> {
	if principal <= Money::ZERO {
		return Ok(Money::ZERO);
	}
	let amount = principal.register_to_budget_scale(data.format.decimal_places())?;
	let sheet = EnvelopeSheet::compute(data, date)?;
	let to_budget = sheet
		.summary(date.year_month())
		.map_or(Money::ZERO, |month| month.to_budget);
	Ok(amount.min(to_budget).max(Money::ZERO))
}

/// Budgets the principal line actually written (rules may have moved it to
/// another category) through `transferAvailable`, which writes no movement
/// note. Nothing is written when the amount comes to 0.
fn fund_principal(
	data: &BudgetData,
	payment: &TransactionId,
	date: CivilDate,
	principal: Money,
) -> Result<Vec<Row>, CoreError> {
	let category = children(data, payment)
		.into_iter()
		.find(|line| line.transfer.is_some())
		.and_then(|line| line.category.clone())
		.and_then(|category| data.resolve_category(&category).cloned());
	let Some(category) = category else {
		return Ok(Vec::new());
	};
	let amount = principal_funding(data, date, principal)?;
	if amount == Money::ZERO {
		return Ok(Vec::new());
	}
	let sheet = EnvelopeSheet::compute(data, date)?;
	let format = NumberFormat::from_preference(data.format.number_format.as_deref().unwrap_or(""));
	let editor = BudgetEditor::new(data, &sheet, format, data.format.hide_fraction);
	let writes = editor.transfer_available(date.year_month(), amount, &category)?;
	Ok(crate::writes::rows(data, &writes))
}

#[cfg(test)]
mod tests {
	use super::*;

	/// DEBT-5 L-2 and L-3: the estimate rounds half up and is exact when it can be.
	#[test]
	fn the_interest_hint_rounds_half_up() {
		let estimate = |owed, rate| interest_estimate(Money::from_minor(owed), rate).unwrap();
		assert_eq!(estimate(980_000, 980), Money::from_minor(8_003));
		assert_eq!(estimate(948_000, 980), Money::from_minor(7_742));
		assert_eq!(estimate(60_000, 1), Money::from_minor(1), "0.5 rounds up");
		assert_eq!(estimate(59_999, 1), Money::ZERO);
		assert_eq!(estimate(0, 980), Money::ZERO);
	}

	/// DEBT-2.2: with whole units shown, the named shortfall rounds up, so
	/// it is never less than what is missing.
	#[test]
	fn a_shortfall_shown_in_whole_units_rounds_up() {
		let mut data = BudgetData::empty(veritas_domain::BudgetType::Envelope);
		let shown = |data: &BudgetData, minor| {
			shortfall_shown(Money::from_minor(minor), data)
				.unwrap()
				.minor()
		};
		assert_eq!(shown(&data, 5_050), 5_050, "cents shown: exact");
		data.format.hide_fraction = true;
		assert_eq!(shown(&data, 5_050), 5_100);
		assert_eq!(shown(&data, 5_001), 5_100);
		assert_eq!(shown(&data, 5_000), 5_000);
		data.format.hide_fraction = false;
		data.format.currency_code = Some("JPY".to_owned());
		assert_eq!(shown(&data, 2_001), 2_100, "0 places: 21 yen, not 20");
	}

	/// DEBT-2.1 and DEBT-5 L-1, L-4, L-5, L-9.
	#[test]
	fn principal_is_what_the_other_parts_leave() {
		let parts = |total, interest, escrow, fees| Parts {
			total: Money::from_minor(total),
			interest: Money::from_minor(interest),
			escrow: Money::from_minor(escrow),
			fees: Money::from_minor(fees),
		};
		let principal = |p: Parts| p.principal().unwrap().minor();
		assert_eq!(principal(parts(40_000, 8_000, 0, 0)), 32_000);
		assert_eq!(principal(parts(200_000, 100_000, 40_000, 0)), 60_000);
		assert_eq!(principal(parts(42_500, 8_000, 0, 2_500)), 32_000);
		assert_eq!(principal(parts(5_000, 8_000, 0, 0)), -3_000);
		assert!(parts(40_000, -1, 0, 0).principal().is_err());
	}
}
