//! v9: Move money, category kinds, loans and the register's history and tags
//! (R-XFER-1/4, R-DEBT-1/2, CLS-3.3, CLS-4.1, docs/finance/debt.md DEBT-1..6).

use veritas_domain::{
	AccountId, BudgetData, CategoryId, CivilDate, CoreError, Money, Row, TransactionId,
};
use veritas_engine::category_kinds::KindTarget as CoreKindTarget;
use veritas_rules::RuleBook;
use veritas_session::{kinds, loans, register, transfers};

use crate::budget::{civil_date, money};
use crate::session::Session;
use crate::{FfiError, NumberStyle};

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CategoryKind {
	Income,
	Expense,
	DebtPrincipal,
	Savings,
}

impl From<CategoryKind> for veritas_domain::CategoryKind {
	fn from(kind: CategoryKind) -> Self {
		match kind {
			CategoryKind::Income => veritas_domain::CategoryKind::Income,
			CategoryKind::Expense => veritas_domain::CategoryKind::Expense,
			CategoryKind::DebtPrincipal => veritas_domain::CategoryKind::DebtPrincipal,
			CategoryKind::Savings => veritas_domain::CategoryKind::Savings,
		}
	}
}

/// Where CLS-4.1 files a loan payment's principal or a savings move.
/// Visible categories always win; v12's hidden cases come only when none
/// has the kind.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum KindTarget {
	/// No category has the kind, visible or not: offer `createKindCategory`.
	None,
	One {
		category_id: String,
	},
	/// Several visible: the user picks.
	Several {
		category_ids: Vec<String>,
	},
	/// v12. One hidden match: confirm 'Show "{name}" again and use it?' (with
	/// the group when it is hidden too), then `reviveKindCategory`.
	OneHidden {
		category: HiddenCategoryMatch,
	},
	/// v12. One archived match: confirm 'Restore "{name}" and use it?', then
	/// `reviveKindCategory`; or "Create a new category" under a free name.
	OneArchived {
		category: HiddenCategoryMatch,
	},
	/// v12. Several hidden or archived matches, in budget order: list each
	/// with its group and "(hidden)" or "(archived)", plus "Create a new
	/// category". The pick is confirmed, then `reviveKindCategory`.
	SeveralHidden {
		categories: Vec<HiddenCategoryMatch>,
	},
}

/// v12. A category of the kind that isn't visible (CLS-4.1): hidden, in a
/// hidden group, or archived (`archived`, which reads "(archived)" over
/// "(hidden)"). Reviving it shows the category and its group again.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct HiddenCategoryMatch {
	pub category_id: String,
	pub name: String,
	pub group_id: String,
	pub group_name: String,
	pub group_is_hidden: bool,
	pub archived: bool,
}

impl From<veritas_engine::category_kinds::HiddenMatch> for HiddenCategoryMatch {
	fn from(found: veritas_engine::category_kinds::HiddenMatch) -> Self {
		HiddenCategoryMatch {
			category_id: found.category.to_string(),
			name: found.name,
			group_id: found.group.to_string(),
			group_name: found.group_name,
			group_is_hidden: found.group_hidden,
			archived: found.archived,
		}
	}
}

/// Where `createKindCategory` would put the new category. When
/// `group_is_hidden`, the shell asks before unhiding that group (CLS-4.1).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct KindCategoryPlan {
	pub group_name: String,
	pub group_is_hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BalanceAfter {
	pub account_id: String,
	/// The account's total, as `accounts()` shows it.
	pub now: i64,
	pub after: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TransferPurpose {
	None,
	/// On budget to off budget: ask what it was for (R-XFER-4, CLS-4.1).
	Ask,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TransferPreview {
	pub from: BalanceAfter,
	pub to: BalanceAfter,
	pub purpose: TransferPurpose,
	/// From the budget into a loan (ST-9.1): offer the payment wizard.
	pub to_loan: bool,
}

/// A loan's terms (R-DEBT-1). `rateBps` is the annual rate in basis points
/// (980 is 9.80%), `payment` cents, `paymentDay` 1 to 31, `lender` the
/// lender payee's name.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LoanTerms {
	pub rate_bps: Option<i64>,
	pub payment: Option<i64>,
	pub payment_day: Option<u8>,
	pub lender: Option<String>,
}

impl From<&LoanTerms> for veritas_domain::LoanTerms {
	fn from(terms: &LoanTerms) -> Self {
		veritas_domain::LoanTerms {
			rate_bps: terms.rate_bps,
			payment: terms.payment.map(Money::from_minor),
			payment_day: terms.payment_day,
			lender: terms.lender.clone(),
		}
	}
}

impl From<veritas_domain::LoanTerms> for LoanTerms {
	fn from(terms: veritas_domain::LoanTerms) -> Self {
		LoanTerms {
			rate_bps: terms.rate_bps,
			payment: terms.payment.map(Money::minor),
			payment_day: terms.payment_day,
			lender: terms.lender,
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum LoanPaymentProblem {
	/// v12. DEBT-2.2: interest, escrow and fees together are more than the
	/// payment, by `short_by`. Saving is refused. Was `InterestAbovePayment`.
	PaymentTooSmall { short_by: i64 },
	/// DEBT-2.3: "This pays off the loan and leaves a credit of $X". A warning.
	PaysOff { credit: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LoanPaymentPreview {
	/// What the other parts leave (DEBT-2.1).
	pub principal: i64,
	/// Owed the day before the payment (DEBT-1).
	pub outstanding: i64,
	/// "About $X of this is probably interest" (DEBT-4), when the rate is set.
	pub estimate: Option<i64>,
	pub rate_bps: Option<i64>,
	pub problem: Option<LoanPaymentProblem>,
	/// v12. What `fundPrincipal` would budget for this principal in the
	/// payment's month, on the budget scale: whole units in a 0-place budget,
	/// clamped to what To Budget holds, as Actual's `transferAvailable`
	/// moves it. 0 means nothing would be budgeted.
	pub principal_funding: i64,
	/// v12. `principal_funding` before the clamp to To Budget, on the same
	/// scale. When `principal_funding` is smaller, show "Funded {funded} of
	/// {asked}" (DEBT-6 A.6).
	pub principal_funding_asked: i64,
}

/// DEBT-3's pre-fill. Every field may be empty; fees are never pre-filled.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LoanPaymentDefaults {
	pub from_account_id: Option<String>,
	pub total: Option<i64>,
	pub interest: Option<i64>,
	pub escrow: Option<i64>,
	pub principal_category_id: Option<String>,
	pub interest_category_id: Option<String>,
	pub escrow_category_id: Option<String>,
}

/// What the wizard saves (DEBT-6). Amounts are positive cents. `id` names the
/// payment to replace when it is an existing split; otherwise it picks the
/// new payment's id, and `nil` lets the core pick. v12: a part's category is
/// needed only when the part isn't 0, interest's included, and
/// `fund_principal` also budgets the principal from To Budget into the
/// principal line's envelope in the payment's month, exactly
/// `LoanPaymentPreview.principal_funding` (A.6). It is `Invalid` on a draft
/// that replaces a payment.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LoanPaymentDraft {
	pub id: Option<String>,
	pub from_account_id: String,
	pub loan_account_id: String,
	pub date: String,
	pub total: i64,
	pub interest: i64,
	pub escrow: i64,
	pub fees: i64,
	pub principal_category_id: String,
	pub interest_category_id: Option<String>,
	pub escrow_category_id: Option<String>,
	pub fees_category_id: Option<String>,
	pub notes: Option<String>,
	pub cleared: bool,
	#[uniffi(default = false)]
	pub fund_principal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BalancePoint {
	pub date: String,
	pub balance: i64,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct BalanceHistory {
	pub points: Vec<BalancePoint>,
	pub change: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TagSummary {
	pub name: String,
	pub color: Option<String>,
	pub hidden: bool,
	pub count: u64,
}

fn parts(total: i64, interest: i64, escrow: i64, fees: i64) -> loans::Parts {
	loans::Parts {
		total: money(total),
		interest: money(interest),
		escrow: money(escrow),
		fees: money(fees),
	}
}

fn text(id: Option<impl ToString>) -> Option<String> {
	id.map(|id| id.to_string())
}

pub(crate) fn set_loan_terms(
	data: &BudgetData,
	account_id: &str,
	terms: &LoanTerms,
) -> Result<Vec<Row>, CoreError> {
	loans::set_loan_terms(data, &AccountId::new(account_id), &terms.into())
}

pub(crate) fn record_loan_payment(
	data: &BudgetData,
	draft: &LoanPaymentDraft,
	now_ms: f64,
) -> Result<Vec<Row>, CoreError> {
	let core = loans::PaymentDraft {
		id: draft.id.as_deref().map(TransactionId::new),
		from: AccountId::new(draft.from_account_id.as_str()),
		loan: AccountId::new(draft.loan_account_id.as_str()),
		date: civil_date(&draft.date)?,
		parts: parts(draft.total, draft.interest, draft.escrow, draft.fees),
		principal_category: CategoryId::new(draft.principal_category_id.as_str()),
		interest_category: draft.interest_category_id.as_deref().map(CategoryId::new),
		escrow_category: draft.escrow_category_id.as_deref().map(CategoryId::new),
		fees_category: draft.fees_category_id.as_deref().map(CategoryId::new),
		notes: draft.notes.clone(),
		cleared: draft.cleared,
		fund_principal: draft.fund_principal,
	};
	loans::record_payment(data, &core, &RuleBook::new(data), now_ms).map(|(rows, _)| rows)
}

fn date(text: &str) -> Result<CivilDate, CoreError> {
	civil_date(text)
}

/// v9. What was typed in a loan's rate field, in basis points: `"9.8"`,
/// `"9.80"` and `"9.80%"` are all 980. Arithmetic works as in an amount
/// field; `style` decides the decimal separator. Negative is `Invalid`.
#[uniffi::export]
pub fn evaluate_rate(text: String, style: NumberStyle) -> Result<i64, FfiError> {
	let trimmed = text.trim();
	let number = trimmed
		.strip_suffix('%')
		.unwrap_or(trimmed)
		.trim()
		.to_owned();
	let rate = crate::evaluate_amount(number, style)?;
	if rate < 0 {
		return Err(FfiError::Invalid {
			message: "The interest rate can't be negative.".to_owned(),
		});
	}
	Ok(rate)
}

#[uniffi::export]
impl Session {
	/// v9. Move money's sheet (R-XFER-1, R-XFER-4): both balances before and
	/// after moving `amount` (positive cents), whether to ask what the move
	/// was for, and whether it pays a loan.
	pub fn transfer_preview(
		&self,
		from_account_id: String,
		to_account_id: String,
		amount: i64,
	) -> Result<TransferPreview, FfiError> {
		self.core.call(|session| {
			let preview = transfers::transfer_preview(
				session.data()?,
				&AccountId::new(from_account_id.as_str()),
				&AccountId::new(to_account_id.as_str()),
				money(amount),
				session.today(),
			)?;
			let side = |side: transfers::BalanceAfter| BalanceAfter {
				account_id: side.account.to_string(),
				now: side.now.minor(),
				after: side.after.minor(),
			};
			Ok(TransferPreview {
				from: side(preview.from),
				to: side(preview.to),
				purpose: match preview.purpose {
					transfers::TransferPurpose::None => TransferPurpose::None,
					transfers::TransferPurpose::Ask => TransferPurpose::Ask,
				},
				to_loan: preview.to_loan,
			})
		})
	}

	/// v9. CLS-4.1: the visible categories of `kind`, which must be
	/// `debtPrincipal` or `savings`; v12 adds the hidden and archived ones
	/// when none is visible.
	pub fn kind_target(&self, kind: CategoryKind) -> Result<KindTarget, FfiError> {
		self.core.call(|session| {
			Ok(match kinds::kind_target(session.data()?, kind.into())? {
				CoreKindTarget::None => KindTarget::None,
				CoreKindTarget::One(id) => KindTarget::One {
					category_id: id.to_string(),
				},
				CoreKindTarget::Several(ids) => KindTarget::Several {
					category_ids: ids.iter().map(ToString::to_string).collect(),
				},
				CoreKindTarget::OneHidden(found) => KindTarget::OneHidden {
					category: found.into(),
				},
				CoreKindTarget::OneArchived(found) => KindTarget::OneArchived {
					category: found.into(),
				},
				CoreKindTarget::SeveralHidden(all) => KindTarget::SeveralHidden {
					categories: all.into_iter().map(Into::into).collect(),
				},
			})
		})
	}

	/// v9. CLS-4.1's confirm step before `createKindCategory`: the group the
	/// new category goes in, and whether it exists but is hidden.
	pub fn kind_category_plan(&self, kind: CategoryKind) -> Result<KindCategoryPlan, FfiError> {
		self.core.call(|session| {
			let plan = kinds::kind_category_plan(session.data()?, kind.into())?;
			Ok(KindCategoryPlan {
				group_name: plan.group_name,
				group_is_hidden: plan.group_is_hidden,
			})
		})
	}

	/// v9. CLS-3.3's confirm step: how many rows change classification if
	/// the category takes `kind`. Errors as `setCategoryKind` would.
	pub fn kind_change_preview(
		&self,
		category_id: String,
		kind: CategoryKind,
	) -> Result<u64, FfiError> {
		self.core.call(|session| {
			kinds::kind_change_preview(
				session.data()?,
				&CategoryId::new(category_id.as_str()),
				kind.into(),
			)
			.map(|count| count as u64)
		})
	}

	/// v9. A loan's terms, or `nil` when the account isn't in Loans (ST-9.1).
	pub fn loan_terms(&self, account_id: String) -> Result<Option<LoanTerms>, FfiError> {
		self.core.call(|session| {
			loans::loan_terms(
				session.data()?,
				&AccountId::new(account_id.as_str()),
				session.today(),
			)
			.map(|terms| terms.map(Into::into))
		})
	}

	/// v9. The wizard's live arithmetic (DEBT-2, DEBT-4).
	pub fn loan_payment_preview(
		&self,
		loan_account_id: String,
		date: String,
		total: i64,
		interest: i64,
		escrow: i64,
		fees: i64,
	) -> Result<LoanPaymentPreview, FfiError> {
		self.core.call(|session| {
			let preview = loans::payment_preview(
				session.data()?,
				&AccountId::new(loan_account_id.as_str()),
				self::date(&date)?,
				&parts(total, interest, escrow, fees),
			)?;
			Ok(LoanPaymentPreview {
				principal: preview.principal.minor(),
				outstanding: preview.outstanding.minor(),
				estimate: preview.estimate.map(Money::minor),
				rate_bps: preview.rate_bps,
				principal_funding: preview.principal_funding.minor(),
				principal_funding_asked: preview.principal_funding_asked.minor(),
				problem: preview.problem.map(|problem| match problem {
					loans::PaymentProblem::PaymentTooSmall { short_by } => {
						LoanPaymentProblem::PaymentTooSmall {
							short_by: short_by.minor(),
						}
					}
					loans::PaymentProblem::PaysOff { credit } => LoanPaymentProblem::PaysOff {
						credit: credit.minor(),
					},
				}),
			})
		})
	}

	/// v9. DEBT-3's pre-fill for a payment dated `today`, cap included.
	pub fn loan_payment_defaults(
		&self,
		loan_account_id: String,
		today: String,
	) -> Result<LoanPaymentDefaults, FfiError> {
		self.core.call(|session| {
			let defaults = loans::payment_defaults(
				session.data()?,
				&AccountId::new(loan_account_id.as_str()),
				date(&today)?,
			)?;
			Ok(LoanPaymentDefaults {
				from_account_id: text(defaults.from),
				total: defaults.total.map(Money::minor),
				interest: defaults.interest.map(Money::minor),
				escrow: defaults.escrow.map(Money::minor),
				principal_category_id: text(defaults.principal_category),
				interest_category_id: text(defaults.interest_category),
				escrow_category_id: text(defaults.escrow_category),
			})
		})
	}

	/// v9. End-of-day balances over `from...to` (`YYYY-MM-DD`, inclusive).
	pub fn balance_history(
		&self,
		account_id: String,
		from: String,
		to: String,
	) -> Result<BalanceHistory, FfiError> {
		self.core.call(|session| {
			let history = register::balance_history(
				session.data()?,
				&AccountId::new(account_id.as_str()),
				date(&from)?,
				date(&to)?,
			)?;
			Ok(BalanceHistory {
				points: history
					.points
					.iter()
					.map(|point| BalancePoint {
						date: point.date.to_string(),
						balance: point.balance.minor(),
					})
					.collect(),
				change: history.change,
			})
		})
	}

	/// v9. Every tag, most used first.
	pub fn tags(&self) -> Result<Vec<TagSummary>, FfiError> {
		self.core.call(|session| {
			Ok(register::tag_summaries(session.data()?)
				.into_iter()
				.map(|tag| TagSummary {
					name: tag.name,
					color: tag.color,
					hidden: tag.hidden,
					count: tag.count as u64,
				})
				.collect())
		})
	}
}
