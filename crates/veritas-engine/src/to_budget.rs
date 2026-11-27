//! docs/finance/to-budget.md: the quick actions that spend To Budget.
//! Assign to underfunded (TB-3) covers overspending, then tops envelopes up
//! to Actual's stored monthly goal. Fund goals' monthly (TB-4) budgets each
//! Veritas goal's DEBT-8.1 suggestion. Both only add budget, are capped by
//! the To Budget left after each write (TB-2.2), and plan each write from
//! the state the writes before it left (TB-10.3).

use std::collections::{BTreeMap, BTreeSet};

use veritas_domain::{CategoryId, CoreError, GoalId, GoalStatus, Money, YearMonth};

use crate::budget_edits::{
	BudgetEditor, BudgetWrites, Destination, Envelope, append_line, month_note_id,
};
use crate::goals::suggestion;

/// Why an envelope gets budget.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FillReason {
	/// Pass 1: its available was below 0 (TB-3.2).
	Overspent,
	/// Pass 2: its budgeted amount was below Actual's monthly goal (TB-3.3).
	GoalShortfall,
	/// A Veritas goal's monthly suggestion (TB-4.2).
	GoalMonthly(GoalId),
}

/// One write of a plan: what the envelope needed and what it gets, which is
/// less when To Budget runs out (TB-2.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fill {
	pub category: CategoryId,
	pub reason: FillReason,
	pub need: Money,
	pub amount: Money,
}

/// A goal Fund goals' monthly leaves out, and why (TB-4.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Skip {
	NoTargetDate,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToBudgetPlan {
	/// In order of writing; only fills that write something.
	pub fills: Vec<Fill>,
	/// Hidden envelopes that would otherwise qualify (TB-2.3).
	pub hidden_left_out: u32,
	pub skipped: Vec<(GoalId, Skip)>,
	pub writes: BudgetWrites,
}

impl ToBudgetPlan {
	pub fn total(&self) -> Result<Money, CoreError> {
		Money::checked_sum(self.fills.iter().map(|fill| fill.amount))
	}

	/// The envelopes written, each counted once though it can be filled by
	/// both passes.
	pub fn envelopes(&self) -> usize {
		self.fills
			.iter()
			.map(|fill| &fill.category)
			.collect::<BTreeSet<_>>()
			.len()
	}
}

/// An envelope the automatic actions may consider, in Budget screen order.
struct Candidate {
	category: CategoryId,
	hidden: bool,
}

/// The working state a plan writes into, so each write reads what the ones
/// before it left (TB-10.3).
struct Working<'e, 'a> {
	editor: &'e BudgetEditor<'a>,
	month: YearMonth,
	left: i64,
	budgeted: BTreeMap<CategoryId, i64>,
	note: Option<String>,
	plan: ToBudgetPlan,
	hidden: BTreeSet<CategoryId>,
}

impl<'e, 'a> Working<'e, 'a> {
	fn new(editor: &'e BudgetEditor<'a>, month: YearMonth) -> Self {
		let note = editor
			.budget()
			.notes
			.get(&month_note_id(month))
			.cloned()
			.flatten();
		Working {
			editor,
			month,
			left: editor.available_to_budget(month),
			budgeted: BTreeMap::new(),
			note,
			plan: ToBudgetPlan::default(),
			hidden: BTreeSet::new(),
		}
	}

	fn budgeted(&self, category: &CategoryId) -> i64 {
		self.budgeted
			.get(category)
			.copied()
			.unwrap_or_else(|| self.editor.budgeted(self.month, category))
	}

	/// Adds `min(need, To Budget left)` to the envelope's budget. Returns the
	/// amount written, 0 once To Budget is used up.
	fn fill(&mut self, category: &CategoryId, reason: FillReason, need: i64) -> Result<i64, CoreError> {
		let amount = need.min(self.left).max(0);
		if amount == 0 {
			return Ok(0);
		}
		let budgeted = self
			.budgeted(category)
			.checked_add(amount)
			.ok_or(CoreError::Overflow)?;
		self.budgeted.insert(category.clone(), budgeted);
		self.left = self.left.checked_sub(amount).ok_or(CoreError::Overflow)?;
		self.plan.writes.amount(self.month, category, budgeted)?;
		self.plan.fills.push(Fill {
			category: category.clone(),
			reason,
			need: Money::from_minor(need),
			amount: Money::from_minor(amount),
		});
		Ok(amount)
	}

	/// `addMovementNotes` after a cover from To Budget, appended to the note
	/// as it stands after the covers before it.
	fn note_cover(&mut self, category: &CategoryId, amount: i64) -> Result<(), CoreError> {
		let line = self.editor.movement_line(
			amount,
			&Envelope::ToBudget,
			&Destination::Category(category.clone()),
		)?;
		self.note = Some(append_line(self.note.as_deref(), &line));
		Ok(())
	}

	fn finish(mut self) -> ToBudgetPlan {
		let original = self
			.editor
			.budget()
			.notes
			.get(&month_note_id(self.month))
			.cloned()
			.flatten();
		if let Some(note) = self.note.filter(|note| Some(note) != original.as_ref()) {
			self.plan.writes.notes.push((month_note_id(self.month), note));
		}
		self.plan.hidden_left_out = u32::try_from(self.hidden.len()).unwrap_or(u32::MAX);
		self.plan
	}
}

impl BudgetEditor<'_> {
	/// TB-2.3: live, non-income envelopes in Budget screen order (group order,
	/// then category order, then id), each marked hidden when it or its group is.
	fn candidates(&self) -> Vec<Candidate> {
		let budget = self.budget();
		let mut candidates = Vec::new();
		for (group_id, categories) in self.sheet().group_order() {
			let Some(group) = budget.category_groups.get(group_id).filter(|g| !g.tombstone && !g.is_income) else {
				continue;
			};
			for id in categories {
				let Some(category) = budget.categories.get(id).filter(|c| !c.tombstone && !c.is_income) else {
					continue;
				};
				candidates.push(Candidate {
					category: id.clone(),
					hidden: group.hidden || category.hidden,
				});
			}
		}
		candidates
	}

	/// TB-3 Assign to underfunded for month `month`: pass 1 covers each
	/// overspent envelope from To Budget with Actual's movement note, then
	/// pass 2 raises each envelope to Actual's stored monthly goal
	/// (`long_goal` not set) with no note. Hidden envelopes are counted, not
	/// filled.
	pub fn assign_to_underfunded(&self, month: YearMonth) -> Result<ToBudgetPlan, CoreError> {
		let candidates = self.candidates();
		let mut working = Working::new(self, month);
		for candidate in &candidates {
			let balance = self.balance(month, &candidate.category);
			if balance >= 0 {
				continue;
			}
			if candidate.hidden {
				working.hidden.insert(candidate.category.clone());
				continue;
			}
			let need = balance.checked_neg().ok_or(CoreError::Overflow)?;
			let covered = working.fill(&candidate.category, FillReason::Overspent, need)?;
			if covered > 0 {
				working.note_cover(&candidate.category, covered)?;
			}
		}
		for candidate in &candidates {
			let Some(goal) = self
				.budget()
				.budget_cells
				.get(&(month, candidate.category.clone()))
				.filter(|cell| !cell.long_goal)
				.and_then(|cell| cell.goal)
			else {
				continue;
			};
			let need = goal
				.minor()
				.checked_sub(working.budgeted(&candidate.category))
				.ok_or(CoreError::Overflow)?;
			if need <= 0 {
				continue;
			}
			if candidate.hidden {
				working.hidden.insert(candidate.category.clone());
				continue;
			}
			working.fill(&candidate.category, FillReason::GoalShortfall, need)?;
		}
		Ok(working.finish())
	}

	/// TB-4 Fund goals' monthly for month `month`: each active Veritas goal
	/// with a live category, in Budget screen order, raised to its DEBT-8.1
	/// suggestion for the month. A goal with no target date is skipped. The
	/// caller offers this for the current month only (TB-4.1).
	pub fn fund_goals_monthly(&self, month: YearMonth) -> Result<ToBudgetPlan, CoreError> {
		let budget = self.budget();
		let mut goals: Vec<_> = budget
			.goals
			.values()
			.filter(|goal| goal.status == GoalStatus::Active)
			.collect();
		goals.sort_by(|a, b| a.id.cmp(&b.id));
		let mut working = Working::new(self, month);
		for candidate in self.candidates() {
			for goal in goals.iter().filter(|goal| goal.category == candidate.category) {
				let Some(suggested) = suggestion(self.sheet(), goal, month)? else {
					if !candidate.hidden {
						working.plan.skipped.push((goal.id.clone(), Skip::NoTargetDate));
					}
					continue;
				};
				let need = suggested
					.monthly
					.minor()
					.checked_sub(working.budgeted(&candidate.category))
					.ok_or(CoreError::Overflow)?
					.max(0);
				if need == 0 {
					continue;
				}
				if candidate.hidden {
					working.hidden.insert(candidate.category.clone());
					continue;
				}
				working.fill(&candidate.category, FillReason::GoalMonthly(goal.id.clone()), need)?;
			}
		}
		Ok(working.finish())
	}
}
