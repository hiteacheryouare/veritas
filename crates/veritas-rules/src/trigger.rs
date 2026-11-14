//! Where rules run and how their result is merged (docs/actual-format.md §6.2).
//! Rules run on new transactions only: manual entry, imports and bank sync,
//! schedule posts, transfer counterparts, the reconciliation adjustment and
//! the "Run Rules" action. Editing an existing transaction never runs them.

use veritas_domain::{AccountId, BudgetData, CategoryId, CivilDate, Money, PayeeId, ScheduleId};

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

/// One transfer leg the writer creates for a new transaction.
#[derive(Clone, Debug, PartialEq)]
pub struct TransferLeg {
	/// `None` for the transaction itself, `Some(n)` for split line `n`.
	pub line: Option<usize>,
	/// The account the money goes to or comes from.
	pub to: AccountId,
	/// The other leg as Actual builds it before its rules run. Run it through
	/// `RuleBook::run` and keep what `counterpart` keeps.
	pub mirror: RuleTransaction,
}

/// A new transaction as the writer stores it, and its transfer legs.
#[derive(Clone, Debug, PartialEq)]
pub struct Transfers {
	pub transaction: RuleTransaction,
	pub legs: Vec<TransferLeg>,
}

/// What `batchUpdateTransactions` does with `runTransfers` for each new row
/// (`transfer.ts:47-97`). A split parent gets no leg; each of its lines with
/// a transfer payee gets one, sized to the line. When both accounts are on
/// budget, or both off, the row's category is cleared (`clearCategory`), on a
/// split line as much as on a plain transaction, and the new leg has none.
pub fn transfers(added: &RuleTransaction, budget: &BudgetData) -> Transfers {
	let transfer_to = |payee: &Option<PayeeId>| {
		let account = payee
			.as_ref()
			.and_then(|payee| budget.payees.get(payee))?
			.transfer_account
			.clone()?;
		budget
			.accounts
			.get(&account)
			.filter(|account| !account.tombstone)
			.map(|account| account.id.clone())
	};
	let from_payee = budget
		.payees
		.values()
		.find(|payee| payee.transfer_account.as_ref() == Some(&added.account))
		.map(|payee| payee.id.clone());
	let same_budget = |to: &AccountId| {
		let off = |id: &AccountId| budget.accounts.get(id).map(|account| account.off_budget);
		off(&added.account) == off(to)
	};
	let mirror = |to: &AccountId,
	              amount: Money,
	              notes: &Option<String>,
	              schedule: Option<ScheduleId>,
	              date: CivilDate| {
		RuleTransaction {
			account: to.clone(),
			date,
			amount: Money::from_minor(-amount.minor()),
			payee: from_payee.clone(),
			category: None,
			notes: notes.clone().filter(|notes| !notes.is_empty()),
			imported_payee: None,
			cleared: Some(false),
			reconciled: None,
			schedule,
			deleted: false,
			splits: Vec::new(),
			is_child: false,
		}
	};
	let mut transaction = added.clone();
	let mut legs = Vec::new();
	if transaction.splits.is_empty() {
		if let Some(to) = transfer_to(&transaction.payee) {
			if same_budget(&to) {
				transaction.category = None::<CategoryId>;
			}
			let leg = mirror(
				&to,
				transaction.amount,
				&transaction.notes,
				transaction.schedule.clone(),
				transaction.date,
			);
			legs.push(TransferLeg {
				line: None,
				to,
				mirror: leg,
			});
		}
	} else {
		let date = transaction.date;
		for (n, line) in transaction.splits.iter_mut().enumerate() {
			if let Some(to) = transfer_to(&line.payee) {
				if same_budget(&to) {
					line.category = None;
				}
				// A split line carries no schedule of its own (`makeChild`).
				let leg = mirror(&to, line.amount, &line.notes, None, date);
				legs.push(TransferLeg {
					line: Some(n),
					to,
					mirror: leg,
				});
			}
		}
	}
	Transfers { transaction, legs }
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

/// What the UI says when rules Actual would have run couldn't run here.
/// Under D60 Veritas saves nothing from such a run, so the note says so and
/// sends Ryan to Actual to add the transaction there.
pub fn may_need_actual(outcome: &RunOutcome) -> Option<String> {
	let (rules, why) = missing_rules(outcome)?;
	Some(format!(
		"Veritas didn't save this: {rules} couldn't run here{why}. Add it in Actual."
	))
}

/// "1 rule" or "N rules", and the reasons in parentheses; `None` when every
/// rule ran.
fn missing_rules(outcome: &RunOutcome) -> Option<(String, String)> {
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
	let rules = match outcome.undetermined.len() {
		0 => return None,
		1 => "1 rule".to_owned(),
		n => format!("{n} rules"),
	};
	Some((rules, why))
}

/// A transaction Ryan typed in, ready to save, and the note to show on it.
#[derive(Clone, Debug, PartialEq)]
pub struct ManualEntry {
	pub transaction: RuleTransaction,
	pub note: Option<String>,
}

/// Saving a row typed by hand (D64). When every rule ran, the rules fill
/// what he left empty (`merge_manual_entry`). When one couldn't run, the row
/// is saved exactly as typed with no rule effects and marked "may need
/// Actual": D60's "write nothing" covers what rules would have written, never
/// what he typed. Actual never runs rules on an existing transaction by
/// itself, so the note points to its "Run Rules" action.
pub fn manual_entry(
	entered: &RuleTransaction,
	outcome: &RunOutcome,
	cleared_by_user: &[Field],
) -> ManualEntry {
	match (&outcome.transaction, missing_rules(outcome)) {
		(Some(ruled), _) => ManualEntry {
			transaction: merge_manual_entry(entered, ruled, cleared_by_user),
			note: None,
		},
		(None, missing) => {
			let (rules, why) = missing.unwrap_or_else(|| ("A rule".to_owned(), String::new()));
			let them = if outcome.undetermined.len() == 1 {
				"it"
			} else {
				"them"
			};
			ManualEntry {
				transaction: entered.clone(),
				note: Some(format!(
					"Saved as you typed it. {rules} couldn't run here{why}. To apply {them}, select this transaction in Actual and choose Run Rules."
				)),
			}
		}
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

	fn transfer_budget() -> BudgetData {
		use veritas_domain::{Account, BudgetType, Payee};
		let mut budget = BudgetData::empty(BudgetType::Envelope);
		for (id, off_budget) in [("checking", false), ("savings", false), ("brokerage", true)] {
			budget.accounts.insert(
				AccountId::new(id),
				Account {
					last_reconciled: None,
					id: AccountId::new(id),
					name: id.into(),
					off_budget,
					closed: false,
					group: None,
					sort_order: 0.0,
					tombstone: false,
				},
			);
			let payee = PayeeId::new(format!("to-{id}"));
			budget.payees.insert(
				payee.clone(),
				Payee {
					id: payee,
					name: String::new(),
					transfer_account: Some(AccountId::new(id)),
					favorite: false,
					learn_categories: false,
					tombstone: false,
				},
			);
		}
		budget
	}

	#[test]
	fn split_lines_that_are_transfers_get_their_own_legs() {
		// transfer.ts:47-97: a leg per line, sized to the line; on budget to
		// on budget clears the line's category, on budget to off keeps it.
		let line = |amount: i64, payee: &str, category: &str| crate::action::SplitLine {
			amount: Money::from_minor(amount),
			payee: Some(PayeeId::new(payee)),
			category: Some(CategoryId::new(category)),
			notes: Some("monthly".into()),
		};
		let mut paycheck = entered();
		paycheck.amount = Money::from_minor(-3_000);
		paycheck.payee = None;
		paycheck.splits = vec![
			line(-1_000, "to-savings", "saving"),
			line(-1_500, "to-brokerage", "investing"),
			line(-500, "cafe", "dining"),
		];
		let done = transfers(&paycheck, &transfer_budget());
		let categories: Vec<Option<&str>> = done
			.transaction
			.splits
			.iter()
			.map(|l| l.category.as_ref().map(CategoryId::as_str))
			.collect();
		assert_eq!(categories, [None, Some("investing"), Some("dining")]);
		assert_eq!(
			done.legs.len(),
			2,
			"the parent gets no leg, the cafe line isn't a transfer"
		);
		let savings = &done.legs[0];
		assert_eq!((savings.line, savings.to.as_str()), (Some(0), "savings"));
		assert_eq!(savings.mirror.amount, Money::from_minor(1_000));
		assert_eq!(savings.mirror.payee, Some(PayeeId::new("to-checking")));
		assert_eq!(savings.mirror.notes.as_deref(), Some("monthly"));
		assert_eq!(
			(
				savings.mirror.cleared,
				savings.mirror.category.clone(),
				savings.mirror.schedule.clone()
			),
			(Some(false), None, None)
		);
		assert_eq!(done.legs[1].line, Some(1));

		let mut plain = entered();
		plain.payee = Some(PayeeId::new("to-savings"));
		plain.category = Some(CategoryId::new("saving"));
		plain.schedule = Some(ScheduleId::new("rent"));
		let done = transfers(&plain, &transfer_budget());
		assert_eq!(done.transaction.category, None);
		assert_eq!(done.legs[0].line, None);
		assert_eq!(done.legs[0].mirror.schedule, Some(ScheduleId::new("rent")));
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
	fn a_typed_row_is_saved_as_typed_when_a_rule_cannot_run() {
		// D64: no rule effects, the row as typed, and a note that it was saved.
		let mut ruled = entered();
		ruled.category = Some(CategoryId::new("dining"));
		let mut outcome = RunOutcome {
			transaction: None,
			partial: ruled.clone(),
			applied: vec![RuleId::new("category")],
			undetermined: vec![RuleId::new("template")],
			reasons: Default::default(),
		};
		outcome
			.reasons
			.insert(RuleId::new("template"), "a template action".into());
		let saved = manual_entry(&entered(), &outcome, &[]);
		assert_eq!(
			saved.transaction,
			entered(),
			"the rule that did run isn't applied either"
		);
		assert_eq!(
			saved.note.as_deref(),
			Some(
				"Saved as you typed it. 1 rule couldn't run here (a template action). To apply it, select this transaction in Actual and choose Run Rules."
			)
		);

		outcome.transaction = Some(ruled);
		outcome.undetermined.clear();
		let ruled_entry = manual_entry(&entered(), &outcome, &[]);
		assert_eq!(
			ruled_entry.transaction.category,
			Some(CategoryId::new("dining"))
		);
		assert_eq!(ruled_entry.note, None);
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
		assert_eq!(
			may_need_actual(&outcome).unwrap(),
			"Veritas didn't save this: 2 rules couldn't run here. Add it in Actual."
		);
		outcome.undetermined = vec![RuleId::new("a")];
		outcome
			.reasons
			.insert(RuleId::new("a"), "a template action".into());
		assert_eq!(
			may_need_actual(&outcome).unwrap(),
			"Veritas didn't save this: 1 rule couldn't run here (a template action). Add it in Actual."
		);
	}
}
