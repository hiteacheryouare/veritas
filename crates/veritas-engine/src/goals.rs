//! Goal math from docs/finance/debt.md DEBT-8: the monthly suggestion, the
//! active → reached → closed lifecycle, releasing leftover on close, and the
//! target Veritas writes into Actual (D36). A goal's progress is its
//! envelope's available balance from the sheet; nothing here recomputes it.

use veritas_domain::{BudgetData, CategoryId, CoreError, Goal, GoalStatus, Money, YearMonth};

use crate::budget_edits::{BudgetEditor, BudgetWrites, CellWrite, Envelope, GoalColumns};
use crate::envelope::EnvelopeSheet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoalProgress {
	pub available: Money,
	pub target: Money,
	/// `max(0, target − available)`.
	pub remaining: Money,
}

/// DEBT-8.1 for month M.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Suggestion {
	/// Budget months from M through the target month, counting both, at least 1.
	pub months_left: u32,
	/// `max(0, target − carry)`, where carry excludes M's own budgeted amount.
	pub remaining: Money,
	/// `ceil(remaining / months_left)`.
	pub monthly: Money,
}

impl Suggestion {
	/// What each month contributes when the suggestion is followed: `monthly`
	/// until the last month, which takes what is left (DEBT-8.1).
	pub fn schedule(&self) -> Vec<Money> {
		let monthly = self.monthly.minor();
		let mut left = self.remaining.minor();
		(0..self.months_left)
			.map(|_| {
				let contribution = monthly.min(left).max(0);
				left -= contribution;
				Money::from_minor(contribution)
			})
			.collect()
	}
}

pub fn progress(sheet: &EnvelopeSheet, goal: &Goal, month: YearMonth) -> GoalProgress {
	let available = available(sheet, &goal.category, month);
	GoalProgress {
		available,
		target: goal.target,
		remaining: Money::from_minor((goal.target.minor() - available.minor()).max(0)),
	}
}

/// `None` without a target date: no suggestion (DEBT-8 "Months left").
pub fn suggestion(sheet: &EnvelopeSheet, goal: &Goal, month: YearMonth) -> Option<Suggestion> {
	let target_month = goal.target_date?.year_month();
	let cell = sheet.category(month, &goal.category);
	let carry = cell.map_or(0, |c| c.balance.minor() - c.budgeted.minor());
	let months_left = months_between(month, target_month).max(0) + 1;
	let months_left = u32::try_from(months_left).unwrap_or(u32::MAX);
	let remaining = (goal.target.minor() - carry).max(0);
	let monthly = remaining.unsigned_abs().div_ceil(u64::from(months_left)) as i64;
	Some(Suggestion {
		months_left,
		remaining: Money::from_minor(remaining),
		monthly: Money::from_minor(monthly),
	})
}

/// DEBT-8.2: an active goal whose available reaches its target in month M
/// becomes reached. Spending afterwards never reverts it. Returns the goal to
/// store, or `None` when nothing changes.
pub fn check_reached(sheet: &EnvelopeSheet, goal: &Goal, month: YearMonth) -> Option<Goal> {
	if goal.status != GoalStatus::Active || available(sheet, &goal.category, month) < goal.target {
		return None;
	}
	Some(Goal {
		status: GoalStatus::Reached,
		reached_month: Some(month),
		..goal.clone()
	})
}

/// DEBT-8.2 backfill for an imported goal: the first created month, up to
/// `through`, whose available is at least the target.
pub fn first_reached_month(
	sheet: &EnvelopeSheet,
	goal: &Goal,
	through: YearMonth,
) -> Option<YearMonth> {
	sheet
		.months()
		.iter()
		.copied()
		.take_while(|m| *m <= through)
		.find(|m| available(sheet, &goal.category, *m) >= goal.target)
}

/// DEBT-8.2 Reopen: back to reached if it ever was, else active. The caller
/// also unhides the category.
pub fn reopen(goal: &Goal) -> Goal {
	Goal {
		status: if goal.reached_month.is_some() {
			GoalStatus::Reached
		} else {
			GoalStatus::Active
		},
		..goal.clone()
	}
}

#[derive(Clone, Debug, PartialEq)]
pub struct GoalClosing {
	/// The release as budget moves in month M, with Actual's movement note.
	pub writes: BudgetWrites,
	pub goal: Goal,
	/// The goal's category, to hide in the same command.
	pub hide: CategoryId,
}

/// DEBT-8.3: release the goal's available at the end of M to `release_to`
/// (or pull an overspent goal back to 0 from it), then close and hide. Only
/// month M's budget amounts change; no transaction is touched (GOAL-1).
pub fn close(
	editor: &BudgetEditor<'_>,
	sheet: &EnvelopeSheet,
	goal: &Goal,
	month: YearMonth,
	release_to: &Envelope,
) -> Result<GoalClosing, CoreError> {
	if goal.status == GoalStatus::Closed {
		return Err(CoreError::Invalid(
			"This goal is already closed.".to_owned(),
		));
	}
	if *release_to == Envelope::Category(goal.category.clone()) {
		return Err(CoreError::Invalid(
			"Pick a different envelope to release this goal's money to.".to_owned(),
		));
	}
	let leftover = available(sheet, &goal.category, month);
	let writes = if leftover > Money::ZERO {
		editor.move_budget(month, leftover, &goal.category, release_to)?
	} else if leftover < Money::ZERO {
		let pulled = leftover.checked_neg()?;
		match release_to {
			Envelope::Category(source) => editor.move_budget(
				month,
				pulled,
				source,
				&Envelope::Category(goal.category.clone()),
			)?,
			Envelope::ToBudget => editor.fund_from_to_budget(month, pulled, &goal.category)?,
		}
	} else {
		BudgetWrites::default()
	};
	let released_to = match release_to {
		Envelope::Category(id) => Some(id.clone()),
		Envelope::ToBudget => None,
	};
	Ok(GoalClosing {
		writes,
		goal: Goal {
			status: GoalStatus::Closed,
			closed_month: Some(month),
			released_to,
			released: Some(leftover),
			..goal.clone()
		},
		hide: goal.category.clone(),
	})
}

/// D36: the target as Actual's long-term goal. The category note keeps one
/// `#goal <amount>` line (replacing an existing one), and every created month
/// from `from` on gets `goal = target`, `long_goal = 1`.
pub fn target_writes(
	budget: &BudgetData,
	sheet: &EnvelopeSheet,
	category: &CategoryId,
	target: Money,
	from: YearMonth,
) -> Result<BudgetWrites, CoreError> {
	if target <= Money::ZERO {
		return Err(CoreError::Invalid(
			"A goal's target has to be more than zero.".to_owned(),
		));
	}
	let line = format!("#goal {target}");
	let existing = budget
		.notes
		.get(category.as_str())
		.and_then(|n| n.as_deref())
		.unwrap_or("");
	let mut replaced = false;
	let mut lines: Vec<&str> = Vec::new();
	for current in existing.lines() {
		if is_goal_line(current) {
			if !replaced {
				lines.push(&line);
				replaced = true;
			}
		} else {
			lines.push(current);
		}
	}
	if !replaced {
		lines.push(&line);
	}
	let text = lines
		.into_iter()
		.skip_while(|l| l.is_empty())
		.collect::<Vec<_>>()
		.join("\n");
	let cells = sheet
		.months()
		.iter()
		.filter(|m| **m >= from)
		.map(|month| CellWrite {
			month: *month,
			category: category.clone(),
			amount: None,
			carryover: None,
			goal: Some(GoalColumns {
				goal: Some(target),
				long_goal: true,
			}),
		})
		.collect();
	Ok(BudgetWrites {
		cells,
		holds: Vec::new(),
		notes: vec![(category.as_str().to_owned(), text)],
	})
}

/// Actual reads a template line from its first `#`, case-sensitively
/// (`template-notes.ts:90-93`).
fn is_goal_line(line: &str) -> bool {
	line.find('#')
		.is_some_and(|start| line[start..].starts_with("#goal"))
}

fn available(sheet: &EnvelopeSheet, category: &CategoryId, month: YearMonth) -> Money {
	sheet
		.category(month, category)
		.map_or(Money::ZERO, |c| c.balance)
}

fn months_between(from: YearMonth, to: YearMonth) -> i64 {
	(i64::from(to.year()) - i64::from(from.year())) * 12 + i64::from(to.month())
		- i64::from(from.month())
}
