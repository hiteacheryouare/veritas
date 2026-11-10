//! Where rules run and how their result is merged (docs/actual-format.md §6.2).
//! Rules run on new transactions only: manual entry, imports and bank sync,
//! schedule posts, transfer counterparts, the reconciliation adjustment and
//! the "Run rules" action. Editing an existing transaction never runs them.

use veritas_domain::AccountId;

use crate::action::RuleTransaction;
use crate::condition::Field;
use crate::run::RunOutcome;

/// What `addTransactions` keeps of the rules' result (`sync.ts:1059-1064`):
/// the account is the one being added to, and `cleared` defaults to true.
/// This is the path for imports through the API and for schedule posts.
pub fn added_transaction(mut ruled: RuleTransaction, account: &AccountId) -> RuleTransaction {
	ruled.account = account.clone();
	ruled.cleared = Some(ruled.cleared.unwrap_or(true));
	ruled
}

/// The other leg of a new transfer only takes `notes`, `cleared` and
/// `schedule` from its rules, and keeps the first leg's schedule when the
/// rules give none (`lc/server/transactions/transfer.ts:61-79`). `mirror` is
/// the leg as Actual builds it before the rules: the other account, the
/// negated amount, the first account's transfer payee, the same date, notes
/// and schedule, and `cleared: false`.
pub fn counterpart(mirror: &RuleTransaction, ruled: &RuleTransaction) -> RuleTransaction {
	RuleTransaction {
		notes: ruled.notes.clone(),
		cleared: ruled.cleared,
		schedule: ruled.schedule.clone().or_else(|| mirror.schedule.clone()),
		..mirror.clone()
	}
}

/// `isEmptyRuleTarget`: null, `''`, `0` and `false` are fields a rule may fill.
fn empty_text(value: &Option<String>) -> bool {
	value.as_deref().is_none_or(str::is_empty)
}

/// `shouldApplyRuleChange` for notes: an append or prepend that keeps the
/// user's note verbatim, and isn't already there, comes through.
fn wraps_note(current: &str, next: &str) -> bool {
	if next == current {
		return false;
	}
	let Some(index) = next.find(current) else {
		return false;
	};
	let prepended = &next[..index];
	let appended = &next[index + current.len()..];
	let already = (prepended.is_empty() || current.starts_with(prepended))
		&& (appended.is_empty() || current.ends_with(appended));
	!already
}

/// Manual entry on desktop and mobile (`applyRulesToTransaction`,
/// `dc/components/transactions/applyRulesToTransaction.ts:44-82`): each field
/// the rules changed is taken only when the user left it empty and didn't
/// clear it on this row; notes may also be wrapped by append or prepend.
pub fn merge_manual_entry(
	entered: &RuleTransaction,
	ruled: &RuleTransaction,
	cleared_by_user: &[Field],
) -> RuleTransaction {
	let allowed = |field: Field, empty: bool| !cleared_by_user.contains(&field) && empty;
	let mut merged = entered.clone();
	if ruled.payee != entered.payee && allowed(Field::Payee, entered.payee.is_none()) {
		merged.payee = ruled.payee.clone();
	}
	if ruled.category != entered.category && allowed(Field::Category, entered.category.is_none()) {
		merged.category = ruled.category.clone();
	}
	if ruled.amount != entered.amount && allowed(Field::Amount, entered.amount.minor() == 0) {
		merged.amount = ruled.amount;
	}
	if ruled.imported_payee != entered.imported_payee
		&& allowed(Field::ImportedPayee, empty_text(&entered.imported_payee))
	{
		merged.imported_payee = ruled.imported_payee.clone();
	}
	if ruled.cleared != entered.cleared && allowed(Field::Cleared, entered.cleared != Some(true)) {
		merged.cleared = ruled.cleared;
	}
	if ruled.reconciled != entered.reconciled
		&& allowed(Field::Reconciled, entered.reconciled != Some(true))
	{
		merged.reconciled = ruled.reconciled;
	}
	if ruled.notes != entered.notes && !cleared_by_user.contains(&Field::Notes) {
		let take = match (entered.notes.as_deref(), ruled.notes.as_deref()) {
			(None | Some(""), _) => true,
			(Some(current), Some(next)) => wraps_note(current, next),
			(Some(_), None) => false,
		};
		if take {
			merged.notes = ruled.notes.clone();
		}
	}
	// The account and date are never empty on a row being entered, so rules
	// can't move them. The schedule and delete flag aren't fields the user
	// clears, so a rule's value is taken when the row had none.
	if ruled.schedule != entered.schedule && entered.schedule.is_none() {
		merged.schedule = ruled.schedule.clone();
	}
	if ruled.deleted && !entered.deleted {
		merged.deleted = true;
	}
	// A split rule's lines fill a row that had none; the user's own payee
	// stays on the parent, since a split only empties it on the rule's copy.
	if entered.splits.is_empty() && !ruled.splits.is_empty() {
		merged.splits = ruled.splits.clone();
	}
	merged
}

/// The note the UI shows on a transaction when rules Actual would have run
/// couldn't run in Veritas (Architect decision: apply the rest, never drop
/// silently).
pub fn may_need_actual(outcome: &RunOutcome) -> Option<String> {
	let reasons: Vec<&str> = outcome
		.undetermined
		.iter()
		.filter_map(|rule| outcome.reasons.get(rule).map(String::as_str))
		.collect();
	let why = if reasons.is_empty() {
		String::new()
	} else {
		format!(" ({})", reasons.join("; "))
	};
	match outcome.undetermined.len() {
		0 => None,
		1 => Some(format!(
			"1 rule couldn't run in Veritas{why}. Open this transaction in Actual to apply it."
		)),
		n => Some(format!(
			"{n} rules couldn't run in Veritas{why}. Open this transaction in Actual to apply them."
		)),
	}
}

#[cfg(test)]
mod tests {
	use veritas_domain::{CategoryId, CivilDate, Money, PayeeId, RuleId, ScheduleId};

	use super::*;

	fn entered() -> RuleTransaction {
		RuleTransaction {
			account: AccountId::new("checking"),
			date: CivilDate::new(2026, 11, 9).unwrap(),
			amount: Money::from_minor(-1250),
			payee: Some(PayeeId::new("cafe")),
			category: None,
			notes: Some("lunch".into()),
			imported_payee: None,
			cleared: Some(false),
			reconciled: None,
			schedule: None,
			deleted: false,
			splits: Vec::new(),
			is_child: false,
		}
	}

	#[test]
	fn rules_fill_only_empty_fields_on_manual_entry() {
		let mut ruled = entered();
		ruled.payee = Some(PayeeId::new("other"));
		ruled.category = Some(CategoryId::new("dining"));
		ruled.amount = Money::from_minor(-999);
		ruled.cleared = Some(true);
		ruled.account = AccountId::new("savings");
		let merged = merge_manual_entry(&entered(), &ruled, &[]);
		assert_eq!(merged.payee, Some(PayeeId::new("cafe")));
		assert_eq!(merged.category, Some(CategoryId::new("dining")));
		assert_eq!(merged.amount, Money::from_minor(-1250));
		assert_eq!(merged.cleared, Some(true), "false is empty to a rule");
		assert_eq!(merged.account, AccountId::new("checking"));
		let merged = merge_manual_entry(&entered(), &ruled, &[Field::Category]);
		assert_eq!(
			merged.category, None,
			"a field the user cleared stays empty"
		);
	}

	#[test]
	fn a_split_from_rules_fills_a_new_row_but_keeps_its_payee() {
		let mut ruled = entered();
		ruled.payee = None;
		ruled.splits = vec![crate::action::SplitLine {
			amount: Money::from_minor(-1250),
			payee: Some(PayeeId::new("cafe")),
			category: Some(CategoryId::new("dining")),
			notes: None,
		}];
		let merged = merge_manual_entry(&entered(), &ruled, &[]);
		assert_eq!(merged.splits, ruled.splits);
		assert_eq!(merged.payee, Some(PayeeId::new("cafe")));
	}

	#[test]
	fn notes_take_appends_and_prepends_once() {
		// dc/components/transactions/table/utils.ts:120-175
		let note = |entered_note: &str, ruled_note: &str| {
			let mut row = entered();
			row.notes = Some(entered_note.into());
			let mut ruled = row.clone();
			ruled.notes = Some(ruled_note.into());
			merge_manual_entry(&row, &ruled, &[]).notes.unwrap()
		};
		assert_eq!(note("lunch", "lunch #food"), "lunch #food");
		assert_eq!(note("lunch #food", "lunch #food #food"), "lunch #food");
		assert_eq!(note("lunch", "Work: lunch"), "Work: lunch");
		assert_eq!(note("lunch", "dinner"), "lunch");
	}

	#[test]
	fn add_transactions_forces_the_account_and_defaults_cleared() {
		let mut ruled = entered();
		ruled.account = AccountId::new("elsewhere");
		ruled.cleared = None;
		let added = added_transaction(ruled, &AccountId::new("checking"));
		assert_eq!(added.account, AccountId::new("checking"));
		assert_eq!(added.cleared, Some(true));
	}

	#[test]
	fn a_counterpart_takes_only_notes_cleared_and_schedule() {
		let mut mirror = entered();
		mirror.schedule = Some(ScheduleId::new("rent"));
		let mut ruled = mirror.clone();
		ruled.notes = Some("moved".into());
		ruled.category = Some(CategoryId::new("dining"));
		ruled.cleared = Some(true);
		ruled.schedule = None;
		let leg = counterpart(&mirror, &ruled);
		assert_eq!(leg.notes.as_deref(), Some("moved"));
		assert_eq!(leg.category, None);
		assert_eq!(leg.cleared, Some(true));
		assert_eq!(leg.schedule, Some(ScheduleId::new("rent")));
	}

	#[test]
	fn the_note_names_how_many_rules_are_missing() {
		let mut outcome = RunOutcome {
			transaction: Some(entered()),
			partial: entered(),
			applied: Vec::new(),
			undetermined: Vec::new(),
			reasons: Default::default(),
		};
		assert_eq!(may_need_actual(&outcome), None);
		outcome.undetermined = vec![RuleId::new("a"), RuleId::new("b")];
		assert!(
			may_need_actual(&outcome)
				.unwrap()
				.starts_with("2 rules couldn't run in Veritas. ")
		);
		outcome.undetermined = vec![RuleId::new("a")];
		outcome
			.reasons
			.insert(RuleId::new("a"), "a template action".into());
		assert_eq!(
			may_need_actual(&outcome).unwrap(),
			"1 rule couldn't run in Veritas (a template action). Open this transaction in Actual to apply it."
		);
	}
}
