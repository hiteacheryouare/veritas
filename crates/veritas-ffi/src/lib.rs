//! The only surface the apps see: commands, queries and the change-event stream, exported through UniFFI.

mod accounts;
mod blocking;
mod budget;
mod commands;
mod goals;
mod records;
mod session;

use std::panic::{AssertUnwindSafe, catch_unwind};

use veritas_domain::{CoreError, YearMonth};

pub use accounts::{
	AccountDetail, AccountGroupDetail, AccountList, ClearedStatus, PayeeDetail, PayeeMemory,
	Reconciliation, RegisterFilter, RegisterLine, RegisterPage, RegisterRow, RegisterScope,
	RegisterSort, RegisterSortKey, SplitLineDraft, TransactionDraft,
};
pub use blocking::CancelToken;
pub use budget::{BudgetMonth, EnvelopeGroup, EnvelopeRow, MoneyTone};
pub use commands::Command;
pub use goals::{GoalClosePreview, GoalPace, GoalState, GoalSummary, MonthAmount};
pub use records::{
	AccountGroup, AccountGroupSummary, AccountSummary, AuditCheck, AuditFinding, BackupInfo,
	BudgetListing, CategoryHistory, Change, Currency, EntityKind, EntityRef, NewAccount,
	NewAccountKind, NewBudgetOptions, NumberFormat, NumberStyle, Overview, Retention, SessionState,
	SnapshotInfo, SnapshotKind,
};
pub use session::{ChangeListener, Session};

uniffi::setup_scaffolding!();

/// contracts/errors.md maps each variant to what the apps show.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FfiError {
	#[error("{entity} {id} doesn't exist")]
	NotFound { entity: String, id: String },
	#[error("{message}")]
	Invalid { message: String },
	#[error("read-only: {reason:?}")]
	ReadOnly { reason: ReadOnlyKind },
	#[error("open it in Actual {required} or newer first")]
	UpgradeInActualFirst { required: String },
	#[error("integrity audit found {violations} problems")]
	AuditFailed { violations: u64 },
	#[error("{message}")]
	NotAnActualBudget { message: String },
	#[error("{message}")]
	Storage { message: String },
	#[error("{message}")]
	Io { message: String },
	#[error("the amount is too large")]
	Overflow,
	#[error("cancelled")]
	Cancelled,
	#[error("{message}")]
	Internal { message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ReadOnlyKind {
	NewerActualVersion,
	TrackingBudget,
	OpenElsewhere,
	AuditFailed,
	NewerVeritasSchema,
}

impl From<CoreError> for FfiError {
	fn from(error: CoreError) -> Self {
		match error {
			CoreError::NotFound { entity, id } => FfiError::NotFound {
				entity: entity.to_owned(),
				id,
			},
			CoreError::Invalid(message) => FfiError::Invalid { message },
			CoreError::ReadOnly(reason) => FfiError::ReadOnly {
				reason: reason.into(),
			},
			CoreError::UpgradeInActualFirst { required } => {
				FfiError::UpgradeInActualFirst { required }
			}
			CoreError::AuditFailed { violations } => FfiError::AuditFailed {
				violations: violations as u64,
			},
			CoreError::NotAnActualBudget(message) => FfiError::NotAnActualBudget { message },
			CoreError::Storage(message) => FfiError::Storage { message },
			CoreError::Io(message) => FfiError::Io { message },
			CoreError::Overflow => FfiError::Overflow,
			CoreError::Cancelled => FfiError::Cancelled,
			CoreError::Internal(message) => FfiError::Internal { message },
		}
	}
}

/// Every export runs through here, so a bug in the core becomes an
/// `Internal` error the app can show instead of a crash (R-REL-7).
pub(crate) fn guard<T>(call: impl FnOnce() -> Result<T, CoreError>) -> Result<T, FfiError> {
	match catch_unwind(AssertUnwindSafe(call)) {
		Ok(result) => result.map_err(FfiError::from),
		Err(payload) => {
			let detail = payload
				.downcast_ref::<&str>()
				.map(|text| (*text).to_owned())
				.or_else(|| payload.downcast_ref::<String>().cloned())
				.unwrap_or_else(|| "unknown panic".to_owned());
			Err(FfiError::Internal {
				message: format!("the core hit a bug: {detail}"),
			})
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct Month {
	pub year: i32,
	pub month: u32,
}

#[uniffi::export]
pub fn core_version() -> Result<String, FfiError> {
	guard(|| Ok(env!("CARGO_PKG_VERSION").to_owned()))
}

/// Every budget folder in `budgets_dir`, most recently opened first.
#[uniffi::export]
pub fn list_budgets(budgets_dir: String) -> Result<Vec<BudgetListing>, FfiError> {
	guard(|| {
		veritas_session::list_budgets(std::path::Path::new(&budgets_dir))
			.map(|budgets| budgets.iter().map(BudgetListing::from).collect())
	})
}

/// Actual's currencies, in its order, for setup's currency picker.
#[uniffi::export]
pub fn currencies() -> Result<Vec<Currency>, FfiError> {
	guard(|| {
		Ok(veritas_domain::CURRENCIES
			.iter()
			.map(Currency::from)
			.collect())
	})
}

/// The text an amount field shows when it opens (R-BUD-3): grouping
/// separators, no currency symbol, a hyphen for negatives, so typing `+12.34`
/// after it adds to the value.
#[uniffi::export]
pub fn amount_edit_text(amount: i64, style: NumberStyle) -> Result<String, FfiError> {
	use veritas_engine::{NumberFormat as EngineFormat, edit_text};
	guard(|| {
		let format =
			EngineFormat::from_preference(veritas_domain::NumberStyle::from(style).actual_value());
		Ok(edit_text(
			veritas_domain::Money::from_minor(amount),
			format,
			false,
		))
	})
}

/// What a budget cell or goal amount field shows when it opens, at the
/// currency's `decimal_places` (D63, CLS-12): `amountEditText` for budget
/// amounts. In a 0-place budget 5000 shows `5,000`, and
/// `evaluateBudgetAmount` reads that back as 5000.
#[uniffi::export]
pub fn budget_amount_edit_text(
	amount: i64,
	style: NumberStyle,
	decimal_places: u8,
) -> Result<String, FfiError> {
	guard(|| {
		Ok(veritas_engine::edit_text_at(
			veritas_domain::Money::from_minor(amount),
			engine_format(style),
			false,
			decimal_places,
		))
	})
}

/// Actual's single-operator rule, for every keystroke in an amount field: when
/// the whole text is one operator typed over the selected value, it becomes
/// `original` followed by that operator, so `+40` adds 40.
#[uniffi::export]
pub fn amount_field_text(original: String, typed: String) -> Result<String, FfiError> {
	guard(|| Ok(veritas_engine::single_operator_rule(&original, &typed)))
}

/// Evaluates what was typed in an amount field, arithmetic included, the
/// way Actual does (R-BUD-3): `12.50+3` is 1550 cents. `style` decides which
/// separator is the decimal point. Text that isn't a storable amount comes
/// back as `Invalid` with a message to show beside the field, and the field
/// keeps its old value (D37).
/// Always two places, as Actual's register is (CLS-12, D63); budget cells use
/// `evaluate_budget_amount`.
#[uniffi::export]
pub fn evaluate_amount(text: String, style: NumberStyle) -> Result<i64, FfiError> {
	use veritas_engine::commit_amount;
	guard(|| {
		let edit = commit_amount(veritas_domain::Money::ZERO, &text, engine_format(style));
		match edit.rejection {
			None => Ok(edit.amount.minor()),
			Some(rejection) => Err(rejected(rejection)),
		}
	})
}

/// What was typed in a budget cell or goal amount, read at the currency's
/// `decimal_places` as Actual's budget does (`useFormat` `fromEdit`, CLS-12,
/// D63): letters are ignored, then arithmetic, then a plain currency amount.
/// Take the places from `currencies()` for the budget's currency, or 2 for a
/// code that isn't listed (CLS-12.7).
#[uniffi::export]
pub fn evaluate_budget_amount(
	text: String,
	style: NumberStyle,
	decimal_places: u8,
) -> Result<i64, FfiError> {
	use veritas_engine::{
		MAX_SAFE_MINOR, Rejection, currency_to_amount, eval_arithmetic, js_round,
	};
	guard(|| {
		let format = engine_format(style);
		let normalized: String = text
			.trim()
			.chars()
			.filter(|c| !is_bidi_control(*c) && !c.is_alphabetic())
			.collect();
		if text.trim().is_empty() {
			return Err(rejected(Rejection::Empty));
		}
		let value = eval_arithmetic(&normalized, format)
			.or_else(|| currency_to_amount(&normalized, format))
			.ok_or_else(|| rejected(Rejection::NotAnAmount))?;
		let minor = js_round(value * 10f64.powi(i32::from(decimal_places)));
		if !minor.is_finite() {
			return Err(rejected(Rejection::NotFinite));
		}
		if minor.abs() > MAX_SAFE_MINOR as f64 {
			return Err(CoreError::Overflow);
		}
		Ok(minor as i64)
	})
}

fn engine_format(style: NumberStyle) -> veritas_engine::NumberFormat {
	veritas_engine::NumberFormat::from_preference(
		veritas_domain::NumberStyle::from(style).actual_value(),
	)
}

fn rejected(rejection: veritas_engine::Rejection) -> CoreError {
	use veritas_engine::Rejection;
	match rejection {
		Rejection::Empty => CoreError::Invalid("Enter an amount.".to_owned()),
		Rejection::NotAnAmount => {
			CoreError::Invalid("That isn't an amount. Use numbers and + - * /.".to_owned())
		}
		Rejection::NotFinite => CoreError::Invalid(
			"That doesn't work out to an amount. Check for a division by zero.".to_owned(),
		),
		Rejection::TooLarge => CoreError::Overflow,
	}
}

/// The marks Actual strips before reading a budget amount
/// (`[\u200E\u200F\u202A-\u202E\u2066-\u2069]`).
fn is_bidi_control(c: char) -> bool {
	matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

#[uniffi::export]
pub fn parse_month(text: String) -> Result<Month, FfiError> {
	guard(|| {
		YearMonth::parse(&text).map(|month| Month {
			year: month.year(),
			month: month.month(),
		})
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_panic_inside_the_core_becomes_an_internal_error() {
		let result: Result<(), FfiError> = guard(|| panic!("index out of bounds"));
		match result {
			Err(FfiError::Internal { message }) => assert!(message.contains("index out of bounds")),
			other => panic!("expected an internal error, got {other:?}"),
		}
	}

	#[test]
	fn the_currency_list_is_actuals() {
		let all = currencies().unwrap();
		assert_eq!(all.len(), 55);
		let usd = all.iter().find(|currency| currency.code == "USD").unwrap();
		assert_eq!((usd.symbol.as_str(), usd.decimal_places), ("$", 2));
		assert!(usd.symbol_first);
		let rupee = all.iter().find(|currency| currency.code == "INR").unwrap();
		assert_eq!(rupee.number_format, NumberStyle::CommaDotIn);
	}

	#[test]
	fn amount_fields_evaluate_arithmetic_like_actual() {
		// R-BUD-3
		let evaluate = |text: &str, style| evaluate_amount(text.to_owned(), style);
		assert_eq!(evaluate("12.50+3", NumberStyle::CommaDot).unwrap(), 1_550);
		assert_eq!(evaluate("1,234.5", NumberStyle::CommaDot).unwrap(), 123_450);
		assert_eq!(evaluate("1.234,5", NumberStyle::DotComma).unwrap(), 123_450);
		assert_eq!(evaluate("-20*3", NumberStyle::CommaDot).unwrap(), -6_000);
		for bad in ["", "   ", "abc", "1/0"] {
			assert!(
				matches!(
					evaluate(bad, NumberStyle::CommaDot),
					Err(FfiError::Invalid { .. })
				),
				"{bad:?}"
			);
		}
		assert!(matches!(
			evaluate("99999999999999999999", NumberStyle::CommaDot),
			Err(FfiError::Overflow)
		));
	}

	#[test]
	fn budget_amounts_are_read_at_the_currencys_places() {
		// D63, CLS-12: 5000 yen in a budget cell is stored as 5000.
		let evaluate = |text: &str, places| {
			evaluate_budget_amount(text.to_owned(), NumberStyle::CommaDot, places)
		};
		assert_eq!(evaluate("5000", 0).unwrap(), 5_000);
		assert_eq!(evaluate("5000", 2).unwrap(), 500_000);
		assert_eq!(evaluate("2.5", 0).unwrap(), 3, "Math.round(2.5)");
		assert_eq!(evaluate("-2.5", 0).unwrap(), -2, "Math.round(-2.5)");
		assert_eq!(evaluate("12.50+3", 2).unwrap(), 1_550);
		assert_eq!(evaluate("1,234.567", 3).unwrap(), 1_234_567);
		assert_eq!(evaluate("¥5,000", 0).unwrap(), 5_000);
		assert_eq!(
			evaluate("5000 JPY", 0).unwrap(),
			5_000,
			"letters are ignored"
		);
		assert_eq!(evaluate("\u{200E}40", 2).unwrap(), 4_000);
		assert_eq!(
			evaluate_budget_amount("1.234,5".to_owned(), NumberStyle::DotComma, 2).unwrap(),
			123_450
		);
		for bad in ["", "  ", "abc", "1/0"] {
			assert!(
				matches!(evaluate(bad, 2), Err(FfiError::Invalid { .. })),
				"{bad:?}"
			);
		}
		assert!(matches!(
			evaluate("99999999999999999", 0),
			Err(FfiError::Overflow)
		));
	}

	#[test]
	fn a_budget_amount_round_trips_through_its_edit_text() {
		// D63: open the field, commit it unchanged, get the same integer back.
		for (amount, places, style, shown) in [
			(5_000, 0, NumberStyle::CommaDot, "5,000"),
			(-1_234_567, 0, NumberStyle::DotComma, "-1.234.567"),
			(1_234_567, 3, NumberStyle::CommaDot, "1,234.567"),
			(123_450, 2, NumberStyle::SpaceComma, "1\u{202f}234,50"),
		] {
			let text = budget_amount_edit_text(amount, style, places).unwrap();
			assert_eq!(text, shown);
			assert_eq!(evaluate_budget_amount(text, style, places).unwrap(), amount);
		}
	}

	#[test]
	fn core_errors_cross_the_boundary_with_their_details() {
		assert!(matches!(
			parse_month("2027-13".to_owned()),
			Err(FfiError::Invalid { message }) if message.contains("2027-13")
		));
		assert_eq!(
			parse_month("2027-03".to_owned()).ok(),
			Some(Month {
				year: 2027,
				month: 3
			})
		);
	}
}
