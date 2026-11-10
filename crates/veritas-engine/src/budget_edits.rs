//! Actual's envelope budget actions (`packages/loot-core/src/server/budget/
//! actions.ts`, v26.11.0). Each action reads the current sheet the way Actual
//! reads its cells and returns the rows it writes; nothing here touches
//! storage. Apply the writes to memory and SQLite in one command, then tell
//! the sheet which months changed.

use veritas_domain::{BudgetCell, BudgetData, CategoryId, CivilDate, CoreError, Money, YearMonth};

use crate::amount_field::{edit_text, js_round};
use crate::arithmetic::NumberFormat;
use crate::envelope::{EnvelopeSheet, counted_category, safe};

/// One `zero_budgets` row, written column by column like Actual's upserts:
/// `None` leaves a column as it is (or at its default on a new row).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellWrite {
	pub month: YearMonth,
	pub category: CategoryId,
	pub amount: Option<Money>,
	pub carryover: Option<bool>,
	pub goal: Option<GoalColumns>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoalColumns {
	pub goal: Option<Money>,
	pub long_goal: bool,
}

/// The rows a budget action writes, in the order Actual writes them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BudgetWrites {
	pub cells: Vec<CellWrite>,
	/// `zero_budget_months.buffered`, keyed by month.
	pub holds: Vec<(YearMonth, Money)>,
	/// `notes` rows by id, with their full new text.
	pub notes: Vec<(String, String)>,
}

impl BudgetWrites {
	pub fn is_empty(&self) -> bool {
		self.cells.is_empty() && self.holds.is_empty() && self.notes.is_empty()
	}

	/// The earliest month whose sheet values these writes can change.
	pub fn first_month(&self) -> Option<YearMonth> {
		let cells = self.cells.iter().map(|c| c.month);
		let holds = self.holds.iter().map(|(m, _)| *m);
		cells.chain(holds).min()
	}

	/// Applies the writes to the in-memory budget; later writes to the same
	/// row win, as they do in Actual.
	pub fn apply_to(&self, budget: &mut BudgetData) {
		for write in &self.cells {
			let cell = budget
				.budget_cells
				.entry((write.month, write.category.clone()))
				.or_insert_with(|| BudgetCell {
					month: write.month,
					category: write.category.clone(),
					budgeted: Money::ZERO,
					carryover: false,
					goal: None,
					long_goal: false,
				});
			if let Some(amount) = write.amount {
				cell.budgeted = amount;
			}
			if let Some(carryover) = write.carryover {
				cell.carryover = carryover;
			}
			if let Some(goal) = write.goal {
				cell.goal = goal.goal;
				cell.long_goal = goal.long_goal;
			}
		}
		for (month, amount) in &self.holds {
			budget.holds.insert(*month, Some(*amount));
		}
		for (id, text) in &self.notes {
			budget.notes.insert(id.clone(), text.clone());
		}
	}

	fn amount(
		&mut self,
		month: YearMonth,
		category: &CategoryId,
		amount: i64,
	) -> Result<(), CoreError> {
		self.cells.push(CellWrite {
			month,
			category: category.clone(),
			amount: Some(Money::from_minor(safe(amount)?)),
			carryover: None,
			goal: None,
		});
		Ok(())
	}

	fn carryover(&mut self, month: YearMonth, category: &CategoryId, flag: bool) {
		self.cells.push(CellWrite {
			month,
			category: category.clone(),
			amount: None,
			carryover: Some(flag),
			goal: None,
		});
	}
}

/// A source or destination for moving budget: an envelope or To Budget.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Envelope {
	Category(CategoryId),
	ToBudget,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AverageSpan {
	Three = 3,
	Six = 6,
	Twelve = 12,
}

/// Reads the budget and its up-to-date sheet the way Actual's actions read
/// sheet cells. `format` and `hide_fraction` shape amounts in movement notes.
#[derive(Clone, Copy, Debug)]
pub struct BudgetEditor<'a> {
	budget: &'a BudgetData,
	sheet: &'a EnvelopeSheet,
	format: NumberFormat,
	hide_fraction: bool,
}

impl<'a> BudgetEditor<'a> {
	pub fn new(
		budget: &'a BudgetData,
		sheet: &'a EnvelopeSheet,
		format: NumberFormat,
		hide_fraction: bool,
	) -> Self {
		BudgetEditor {
			budget,
			sheet,
			format,
			hide_fraction,
		}
	}

	/// `budget/budget-amount`.
	pub fn set_budget(
		&self,
		month: YearMonth,
		category: &CategoryId,
		amount: Money,
	) -> Result<BudgetWrites, CoreError> {
		let mut writes = BudgetWrites::default();
		writes.amount(month, category, amount.minor())?;
		Ok(writes)
	}

	/// `budget/copy-previous-month`: every budget row of last month for a live
	/// category, except income categories and hidden categories or groups.
	pub fn copy_previous_month(&self, month: YearMonth) -> Result<BudgetWrites, CoreError> {
		let previous = month.offset(-1)?;
		let mut writes = BudgetWrites::default();
		for ((_, id), cell) in self.cells_of(previous)? {
			let Some(category) = self.budget.categories.get(id).filter(|c| !c.tombstone) else {
				continue;
			};
			let group_hidden = self
				.budget
				.category_groups
				.get(&category.group)
				.map(|g| g.hidden);
			if category.is_income || category.hidden || group_hidden == Some(true) {
				continue;
			}
			writes.amount(month, id, cell.budgeted.minor())?;
		}
		Ok(writes)
	}

	/// `budget/copy-single-month`.
	pub fn copy_single_previous_month(
		&self,
		month: YearMonth,
		category: &CategoryId,
	) -> Result<BudgetWrites, CoreError> {
		let mut writes = BudgetWrites::default();
		writes.amount(month, category, self.budgeted(month.offset(-1)?, category))?;
		Ok(writes)
	}

	/// `budget/set-zero`: every live expense category, hidden ones included.
	pub fn set_zero(&self, month: YearMonth) -> Result<BudgetWrites, CoreError> {
		let mut writes = BudgetWrites::default();
		for category in self.budget.categories.values() {
			if !category.tombstone && !category.is_income {
				writes.amount(month, &category.id, 0)?;
			}
		}
		Ok(writes)
	}

	/// `budget/set-{3,6,12}month-avg`: visible live expense categories in
	/// visible groups get minus their average activity.
	pub fn set_average(
		&self,
		month: YearMonth,
		span: AverageSpan,
	) -> Result<BudgetWrites, CoreError> {
		let mut writes = BudgetWrites::default();
		for category in self.budget.categories.values() {
			let group_visible = self
				.budget
				.category_groups
				.get(&category.group)
				.map(|g| !g.hidden);
			if category.tombstone
				|| category.hidden
				|| group_visible != Some(true)
				|| category.is_income
			{
				continue;
			}
			let average = self.category_average(month, &category.id, span)?;
			writes.amount(month, &category.id, -average)?;
		}
		Ok(writes)
	}

	/// `budget/set-n-month-avg` for one category. Unlike the bulk version it
	/// doesn't skip income categories; their average is kept positive.
	pub fn set_category_average(
		&self,
		month: YearMonth,
		category: &CategoryId,
		span: AverageSpan,
	) -> Result<BudgetWrites, CoreError> {
		let average = self.category_average(month, category, span)?;
		let is_income = self
			.budget
			.categories
			.get(category)
			.is_some_and(|c| c.is_income);
		let mut writes = BudgetWrites::default();
		writes.amount(month, category, if is_income { average } else { -average })?;
		Ok(writes)
	}

	/// `getCategoryAverage`: the rounded mean of up to N months of activity,
	/// ending last month (or the month before today's, for future months) and
	/// never reaching back before the category's first activity.
	pub fn category_average(
		&self,
		month: YearMonth,
		category: &CategoryId,
		span: AverageSpan,
	) -> Result<i64, CoreError> {
		let current = self.sheet.today().year_month();
		let mut first = month.offset(-1)?;
		if first >= current {
			first = current.offset(-1)?;
		}
		let first_activity = self.first_activity(category, first);
		let mut total: i64 = 0;
		let mut count: i64 = 0;
		let mut cursor = first;
		for _ in 0..span as usize {
			if first_activity.is_some_and(|start| cursor < start) {
				break;
			}
			let spent = self
				.sheet
				.category(cursor, category)
				.map_or(0, |c| c.spent.minor());
			total = add(total, spent)?;
			count += 1;
			cursor = cursor.offset(-1)?;
		}
		if count == 0 {
			return Ok(0);
		}
		Ok(js_round(total as f64 / count as f64) as i64)
	}

	/// `budget/copy-until-year-end`: this month's amount into every later
	/// created month of the same year.
	pub fn copy_until_year_end(
		&self,
		month: YearMonth,
		category: &CategoryId,
	) -> Result<BudgetWrites, CoreError> {
		let amount = self.budgeted(month, category);
		let mut writes = BudgetWrites::default();
		for later in self.sheet.months() {
			if *later > month && later.year() == month.year() {
				writes.amount(*later, category, amount)?;
			}
		}
		Ok(writes)
	}

	/// `budget/hold-for-next-month`. `None` when To Budget isn't positive, which
	/// is Actual's `false`. Holds add up and are clamped to `[0, To Budget]`.
	pub fn hold_for_next_month(
		&self,
		month: YearMonth,
		amount: Money,
	) -> Result<Option<BudgetWrites>, CoreError> {
		let to_budget = self.available_to_budget(month);
		if to_budget <= 0 {
			return Ok(None);
		}
		let held = self
			.budget
			.holds
			.get(&month)
			.copied()
			.flatten()
			.map_or(0, Money::minor);
		let change = amount.minor().max(-held).min(to_budget.max(0));
		let mut writes = BudgetWrites::default();
		writes
			.holds
			.push((month, Money::from_minor(add(held, change)?)));
		Ok(Some(writes))
	}

	/// `budget/reset-hold`.
	pub fn reset_hold(&self, month: YearMonth) -> BudgetWrites {
		BudgetWrites {
			holds: vec![(month, Money::ZERO)],
			..BudgetWrites::default()
		}
	}

	/// `budget/transfer-available`: To Budget into an envelope, clamped to what
	/// To Budget holds. Actual writes no movement note for it.
	pub fn transfer_available(
		&self,
		month: YearMonth,
		amount: Money,
		category: &CategoryId,
	) -> Result<BudgetWrites, CoreError> {
		let moved = amount.minor().min(self.available_to_budget(month)).max(0);
		let mut writes = BudgetWrites::default();
		writes.amount(month, category, add(self.budgeted(month, category), moved)?)?;
		Ok(writes)
	}

	/// Veritas only: puts `amount` from To Budget into an envelope without
	/// Actual's clamp, with the movement note Actual's moves write. Closing an
	/// overspent goal against To Budget uses it (DEBT-8.3).
	pub fn fund_from_to_budget(
		&self,
		month: YearMonth,
		amount: Money,
		category: &CategoryId,
	) -> Result<BudgetWrites, CoreError> {
		let mut writes = BudgetWrites::default();
		writes.amount(
			month,
			category,
			add(self.budgeted(month, category), amount.minor())?,
		)?;
		let to = Destination::Category(category.clone());
		writes
			.notes
			.push(self.movement_note(month, amount.minor(), &Envelope::ToBudget, to)?);
		Ok(writes)
	}

	/// Move budget (`budget/transfer-category`): no clamping, so a negative
	/// amount moves the other way. Moving to To Budget only lowers `from`.
	pub fn move_budget(
		&self,
		month: YearMonth,
		amount: Money,
		from: &CategoryId,
		to: &Envelope,
	) -> Result<BudgetWrites, CoreError> {
		let amount = amount.minor();
		let mut writes = BudgetWrites::default();
		writes.amount(month, from, sub(self.budgeted(month, from), amount)?)?;
		if let Envelope::Category(to) = to {
			writes.amount(month, to, add(self.budgeted(month, to), amount)?)?;
		}
		let from = Envelope::Category(from.clone());
		writes
			.notes
			.push(self.movement_note(month, amount, &from, Destination::from(to))?);
		Ok(writes)
	}

	/// `budget/cover-overspending`: fills `to`'s negative balance from `from`,
	/// all of it when `amount` is `None` or zero, never more than `from` has.
	pub fn cover_overspending(
		&self,
		month: YearMonth,
		to: &CategoryId,
		from: &Envelope,
		amount: Option<Money>,
	) -> Result<BudgetWrites, CoreError> {
		let need = match amount.filter(|a| *a != Money::ZERO) {
			Some(amount) => -amount.minor(),
			None => self.balance(month, to),
		};
		let available = match from {
			Envelope::ToBudget => self.available_to_budget(month),
			Envelope::Category(from) => self.balance(month, from),
		};
		let mut writes = BudgetWrites::default();
		if need >= 0 || available <= 0 {
			return Ok(writes);
		}
		let covered = need.unsigned_abs().min(available.unsigned_abs()) as i64;
		if let Envelope::Category(from) = from {
			writes.amount(month, from, sub(self.budgeted(month, from), covered)?)?;
		}
		writes.amount(month, to, add(self.budgeted(month, to), covered)?)?;
		let destination = Destination::Category(to.clone());
		writes
			.notes
			.push(self.movement_note(month, covered, from, destination)?);
		Ok(writes)
	}

	/// `budget/cover-overbudgeted`: fills a negative To Budget from an envelope.
	pub fn cover_overbudgeted(
		&self,
		month: YearMonth,
		category: &CategoryId,
		amount: Option<Money>,
	) -> Result<BudgetWrites, CoreError> {
		let need = match amount.filter(|a| *a != Money::ZERO) {
			Some(amount) => -amount.minor(),
			None => self.available_to_budget(month),
		};
		let available = self.balance(month, category);
		let mut writes = BudgetWrites::default();
		if need >= 0 || available <= 0 {
			return Ok(writes);
		}
		let covered = need.unsigned_abs().min(available.unsigned_abs()) as i64;
		writes.amount(
			month,
			category,
			sub(self.budgeted(month, category), covered)?,
		)?;
		let from = Envelope::Category(category.clone());
		writes
			.notes
			.push(self.movement_note(month, covered, &from, Destination::Overbudgeted)?);
		Ok(writes)
	}

	/// `budget/set-carryover`: the flag on every month from `start` through the
	/// last created month. Months created later get no row, so the flag lapses.
	/// Like Actual's `rangeInclusive`, a start past the last month still writes
	/// the start month.
	pub fn set_carryover(
		&self,
		start: YearMonth,
		category: &CategoryId,
		flag: bool,
	) -> Result<BudgetWrites, CoreError> {
		let mut writes = BudgetWrites::default();
		let last = self.sheet.months().last().copied().unwrap_or(start);
		let mut month = start;
		while month < last {
			writes.carryover(month, category, flag);
			month = month.offset(1)?;
		}
		writes.carryover(month, category, flag);
		Ok(writes)
	}

	/// `budget/reset-income-carryover`: auto hold off for this month only, on
	/// every live income category.
	pub fn reset_income_carryover(&self, month: YearMonth) -> BudgetWrites {
		let mut writes = BudgetWrites::default();
		for category in self.budget.categories.values() {
			if category.is_income && !category.tombstone {
				writes.carryover(month, &category.id, false);
			}
		}
		writes
	}

	/// The income menu's auto-hold toggle: turning it on resets the manual
	/// hold first (`IncomeMenu.tsx:38-45`).
	pub fn set_income_auto_hold(
		&self,
		month: YearMonth,
		category: &CategoryId,
		on: bool,
	) -> Result<BudgetWrites, CoreError> {
		let mut writes = if on {
			self.reset_hold(month)
		} else {
			BudgetWrites::default()
		};
		writes
			.cells
			.extend(self.set_carryover(month, category, on)?.cells);
		Ok(writes)
	}

	/// `budget-<category>` as Actual's sheet holds it: the stored row for a
	/// live category, in any month, else 0.
	pub fn budgeted(&self, month: YearMonth, category: &CategoryId) -> i64 {
		let live = self
			.budget
			.categories
			.get(category)
			.is_some_and(|c| !c.tombstone);
		if !live {
			return 0;
		}
		self.budget
			.budget_cells
			.get(&(month, category.clone()))
			.map_or(0, |cell| cell.budgeted.minor())
	}

	fn balance(&self, month: YearMonth, category: &CategoryId) -> i64 {
		self.sheet
			.category(month, category)
			.map_or(0, |c| c.balance.minor())
	}

	fn available_to_budget(&self, month: YearMonth) -> i64 {
		self.sheet.summary(month).map_or(0, |s| s.to_budget.minor())
	}

	fn cells_of(
		&self,
		month: YearMonth,
	) -> Result<impl Iterator<Item = (&'a (YearMonth, CategoryId), &'a BudgetCell)>, CoreError> {
		let from = (month, CategoryId::new(""));
		let to = (month.offset(1)?, CategoryId::new(""));
		Ok(self.budget.budget_cells.range(from..to))
	}

	/// `getFirstActivityMonth`: the earliest budget row or counted transaction
	/// of the category up to `through`.
	fn first_activity(&self, category: &CategoryId, through: YearMonth) -> Option<YearMonth> {
		let rows = self
			.budget
			.budget_cells
			.keys()
			.filter(|(month, id)| id == category && *month <= through)
			.map(|(month, _)| *month);
		let transactions = self
			.budget
			.transactions
			.values()
			.filter(|t| counted_category(self.budget, t) == Some(category))
			.map(|t| t.date.year_month())
			.filter(|month| *month <= through);
		rows.chain(transactions).min()
	}

	/// `addMovementNotes`: appends "- Reassigned …" to the month's budget note.
	fn movement_note(
		&self,
		month: YearMonth,
		amount: i64,
		from: &Envelope,
		to: Destination,
	) -> Result<(String, String), CoreError> {
		let id = format!("budget-{month}");
		let display = edit_text(
			Money::from_minor(safe(amount)?),
			self.format,
			self.hide_fraction,
		);
		let name = |category: &CategoryId| {
			self.budget
				.categories
				.get(category)
				.filter(|c| !c.tombstone)
				.map_or_else(|| "undefined".to_owned(), |c| c.name.clone())
		};
		let from = match from {
			Envelope::ToBudget => "To Budget".to_owned(),
			Envelope::Category(category) => name(category),
		};
		let to = match &to {
			Destination::ToBudget => "To Budget".to_owned(),
			Destination::Overbudgeted => "Overbudgeted".to_owned(),
			Destination::Category(category) => name(category),
		};
		let day = note_day(self.sheet.today());
		let line = format!("- Reassigned {display} from {from} \u{2192} {to} on {day}");
		let text = match self.budget.notes.get(&id).filter(|n| !n.is_empty()) {
			Some(existing) => format!("{existing}\n{line}"),
			None => line,
		};
		Ok((id, text))
	}
}

enum Destination {
	Category(CategoryId),
	ToBudget,
	Overbudgeted,
}

impl From<&Envelope> for Destination {
	fn from(envelope: &Envelope) -> Self {
		match envelope {
			Envelope::Category(id) => Destination::Category(id.clone()),
			Envelope::ToBudget => Destination::ToBudget,
		}
	}
}

/// date-fns `MMMM dd` in English, e.g. "November 09".
fn note_day(today: CivilDate) -> String {
	const MONTHS: [&str; 12] = [
		"January",
		"February",
		"March",
		"April",
		"May",
		"June",
		"July",
		"August",
		"September",
		"October",
		"November",
		"December",
	];
	let name = MONTHS
		.get(today.month() as usize - 1)
		.copied()
		.unwrap_or_default();
	format!("{name} {:02}", today.day())
}

fn add(a: i64, b: i64) -> Result<i64, CoreError> {
	a.checked_add(b).ok_or(CoreError::Overflow)
}

fn sub(a: i64, b: i64) -> Result<i64, CoreError> {
	a.checked_sub(b).ok_or(CoreError::Overflow)
}
