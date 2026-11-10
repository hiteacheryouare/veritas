//! R-BUD-4: Actual's budget actions (docs/actual-format.md §5.8), including
//! the "Reassigned …" movement note Actual appends (decision 6). Every case
//! starts from `small.zip` as its spec builds it, with today 2026-11-09.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;
use veritas_domain::{CoreError, Money};
use veritas_engine::{
	AverageSpan, BudgetEditor, BudgetWrites, Envelope, NumberFormat, SheetChange,
};

fn budgeted(budget: &Budget, month: &str, category: &str) -> Money {
	budget
		.0
		.budget_cells
		.get(&(ym(month), cat(category)))
		.map_or(Money::ZERO, |c| c.budgeted)
}

fn to(category: &str) -> Envelope {
	Envelope::Category(cat(category))
}

fn note(budget: &Budget, month: &str) -> String {
	budget
		.0
		.notes
		.get(&format!("budget-{month}"))
		.cloned()
		.unwrap_or_default()
}

#[test]
fn replays_the_oracle_apply_test_through_the_editor() {
	let mut budget = small_fixture();
	budget.edit(|e| {
		e.set_carryover(ym("2026-10"), &cat("groceries"), true)
			.unwrap()
	});
	budget.edit(|e| {
		e.set_budget(ym("2026-10"), &cat("groceries"), cents(10_000))
			.unwrap()
	});
	budget.txn(
		"late-dinner",
		"checking",
		"2026-10-30",
		-1_000,
		Some("dining"),
	);
	budget.edit(|e| {
		e.hold_for_next_month(ym("2026-10"), cents(50_000))
			.unwrap()
			.unwrap()
	});
	let sheet = budget.sheet(TODAY);
	let october = sheet.summary(ym("2026-10")).unwrap();
	assert_eq!(october.to_budget, cents(622_686));
	assert_eq!(
		sheet
			.category(ym("2026-11"), &cat("groceries"))
			.unwrap()
			.balance,
		cents(-2_657)
	);
	assert!(budget.0.budget_cells[&(ym("2027-11"), cat("groceries"))].carryover);
}

#[test]
fn moving_budget_between_envelopes_writes_both_amounts_and_a_note() {
	let mut budget = small_fixture();
	let writes = budget.edit(|e| {
		e.move_budget(
			ym("2026-10"),
			cents(2_000),
			&cat("groceries"),
			&to("dining"),
		)
		.unwrap()
	});
	assert_eq!(writes.first_month(), Some(ym("2026-10")));
	assert_eq!(budgeted(&budget, "2026-10", "groceries"), cents(10_000));
	assert_eq!(budgeted(&budget, "2026-10", "dining"), cents(7_000));
	assert_eq!(
		note(&budget, "2026-10"),
		"- Reassigned 20.00 from groceries \u{2192} dining on November 09"
	);

	budget.edit(|e| {
		e.move_budget(ym("2026-10"), cents(-500), &cat("groceries"), &to("dining"))
			.unwrap()
	});
	assert_eq!(
		budgeted(&budget, "2026-10", "groceries"),
		cents(10_500),
		"a negative move goes the other way"
	);
	assert_eq!(
		note(&budget, "2026-10"),
		"- Reassigned 20.00 from groceries \u{2192} dining on November 09\n\
		 - Reassigned -5.00 from groceries \u{2192} dining on November 09"
	);
}

#[test]
fn moving_budget_to_to_budget_only_lowers_the_source() {
	let mut budget = small_fixture();
	let before = budget
		.sheet(TODAY)
		.summary(ym("2026-10"))
		.unwrap()
		.to_budget;
	let writes = budget.edit(|e| {
		e.move_budget(
			ym("2026-10"),
			cents(1_000),
			&cat("rent"),
			&Envelope::ToBudget,
		)
		.unwrap()
	});
	assert_eq!(writes.cells.len(), 1);
	assert_eq!(budgeted(&budget, "2026-10", "rent"), cents(149_000));
	assert_eq!(
		budget
			.sheet(TODAY)
			.summary(ym("2026-10"))
			.unwrap()
			.to_budget,
		cents(before.minor() + 1_000)
	);
	assert!(note(&budget, "2026-10").ends_with("from rent \u{2192} To Budget on November 09"));
}

#[test]
fn movement_notes_follow_the_number_format_and_name_deleted_categories_like_actual() {
	let budget = small_fixture();
	let sheet = budget.sheet(TODAY);
	let dot_comma = BudgetEditor::new(&budget.0, &sheet, NumberFormat::DotComma, false);
	let writes = dot_comma
		.move_budget(
			ym("2026-10"),
			cents(123_456),
			&cat("groceries"),
			&to("dining"),
		)
		.unwrap();
	assert!(writes.notes[0].1.contains("Reassigned 1.234,56 from"));
	let hidden_cents = BudgetEditor::new(&budget.0, &sheet, NumberFormat::CommaDot, true);
	let writes = hidden_cents
		.move_budget(
			ym("2026-10"),
			cents(2_050),
			&cat("groceries"),
			&to("dining"),
		)
		.unwrap();
	assert!(writes.notes[0].1.contains("Reassigned 21 from"));

	let mut budget = small_fixture();
	budget.category("dining", "everyday", false).tombstone = true;
	budget.edit(|e| {
		e.move_budget(ym("2026-10"), cents(100), &cat("groceries"), &to("dining"))
			.unwrap()
	});
	assert!(note(&budget, "2026-10").contains("from groceries \u{2192} undefined on"));
}

#[test]
fn covering_overspending_moves_at_most_what_the_source_has() {
	let mut budget = small_fixture();
	let sheet = budget.sheet(TODAY);
	assert_eq!(
		sheet
			.category(ym("2026-10"), &cat("groceries"))
			.unwrap()
			.balance,
		cents(-657)
	);
	assert_eq!(
		sheet
			.category(ym("2026-10"), &cat("dining"))
			.unwrap()
			.balance,
		cents(570)
	);

	budget.edit(|e| {
		e.cover_overspending(ym("2026-10"), &cat("groceries"), &to("dining"), None)
			.unwrap()
	});
	assert_eq!(budgeted(&budget, "2026-10", "groceries"), cents(12_570));
	assert_eq!(budgeted(&budget, "2026-10", "dining"), cents(4_430));
	assert!(note(&budget, "2026-10").contains("Reassigned 5.70 from dining \u{2192} groceries"));
	let groceries = budget
		.sheet(TODAY)
		.category(ym("2026-10"), &cat("groceries"))
		.copied()
		.unwrap();
	assert_eq!(groceries.balance, cents(-87), "dining only had 5.70");

	let writes = budget.edit(|e| {
		e.cover_overspending(ym("2026-10"), &cat("groceries"), &to("dining"), None)
			.unwrap()
	});
	assert!(writes.is_empty(), "a source with nothing left does nothing");
}

#[test]
fn covering_overspending_takes_a_partial_amount_or_to_budget() {
	let mut budget = small_fixture();
	budget.edit(|e| {
		e.cover_overspending(
			ym("2026-10"),
			&cat("groceries"),
			&to("dining"),
			Some(cents(300)),
		)
		.unwrap()
	});
	assert_eq!(budgeted(&budget, "2026-10", "groceries"), cents(12_300));
	assert_eq!(budgeted(&budget, "2026-10", "dining"), cents(4_700));

	let writes = budget.edit(|e| {
		e.cover_overspending(ym("2026-10"), &cat("groceries"), &Envelope::ToBudget, None)
			.unwrap()
	});
	assert_eq!(writes.cells.len(), 1);
	assert_eq!(budgeted(&budget, "2026-10", "groceries"), cents(12_657));
	assert!(
		note(&budget, "2026-10")
			.ends_with("- Reassigned 3.57 from To Budget \u{2192} groceries on November 09")
	);

	let writes = budget.edit(|e| {
		e.cover_overspending(ym("2026-10"), &cat("rent"), &to("dining"), None)
			.unwrap()
	});
	assert!(
		writes.is_empty(),
		"an envelope that isn't overspent needs no cover"
	);
}

#[test]
fn covering_a_negative_to_budget_from_an_envelope() {
	let mut budget = small_fixture();
	budget.budget("2026-11", "rent", 700_000);
	let sheet = budget.sheet(TODAY);
	assert_eq!(
		sheet.summary(ym("2026-11")).unwrap().to_budget,
		cents(-29_971)
	);
	assert_eq!(
		sheet
			.category(ym("2026-11"), &cat("utilities"))
			.unwrap()
			.balance,
		cents(1_731)
	);
	budget.edit(|e| {
		e.cover_overbudgeted(ym("2026-11"), &cat("utilities"), None)
			.unwrap()
	});
	assert_eq!(budgeted(&budget, "2026-11", "utilities"), cents(-1_731));
	assert!(
		note(&budget, "2026-11").contains("Reassigned 17.31 from utilities \u{2192} Overbudgeted")
	);
	assert_eq!(
		budget
			.sheet(TODAY)
			.summary(ym("2026-11"))
			.unwrap()
			.to_budget,
		cents(-28_240)
	);

	let writes = budget.edit(|e| {
		e.cover_overbudgeted(ym("2026-11"), &cat("utilities"), None)
			.unwrap()
	});
	assert!(writes.is_empty());
}

#[test]
fn holds_add_up_and_stay_between_zero_and_to_budget() {
	let mut budget = small_fixture();
	let october = ym("2026-10");
	let to_budget = budget
		.sheet(TODAY)
		.summary(october)
		.unwrap()
		.to_budget
		.minor();
	assert_eq!(to_budget, 670_686);
	let held = |b: &Budget| b.0.holds[&october].unwrap();
	budget.edit(|e| {
		e.hold_for_next_month(october, cents(50_000))
			.unwrap()
			.unwrap()
	});
	budget.edit(|e| {
		e.hold_for_next_month(october, cents(30_000))
			.unwrap()
			.unwrap()
	});
	assert_eq!(held(&budget), cents(80_000));
	budget.edit(|e| {
		e.hold_for_next_month(october, cents(-1_000_000))
			.unwrap()
			.unwrap()
	});
	assert_eq!(held(&budget), Money::ZERO, "a hold never goes negative");
	budget.edit(|e| {
		e.hold_for_next_month(october, cents(10_000_000))
			.unwrap()
			.unwrap()
	});
	assert_eq!(held(&budget), cents(to_budget), "nor above To Budget");
	budget.edit(|e| e.reset_hold(october));
	assert_eq!(held(&budget), Money::ZERO);

	let empty = Budget::actual_seed();
	let sheet = empty.sheet(TODAY);
	let editor = BudgetEditor::new(&empty.0, &sheet, NumberFormat::CommaDot, false);
	assert_eq!(
		editor
			.hold_for_next_month(ym("2026-11"), cents(100))
			.unwrap(),
		None
	);
}

#[test]
fn transfer_available_is_clamped_to_to_budget_and_writes_no_note() {
	let mut budget = small_fixture();
	budget.edit(|e| {
		e.transfer_available(ym("2026-10"), cents(1_000_000), &cat("dining"))
			.unwrap()
	});
	assert_eq!(
		budgeted(&budget, "2026-10", "dining"),
		cents(5_000 + 670_686)
	);
	assert!(budget.0.notes.is_empty());
	budget.edit(|e| {
		e.transfer_available(ym("2026-09"), cents(-500), &cat("dining"))
			.unwrap()
	});
	assert_eq!(budgeted(&budget, "2026-09", "dining"), cents(5_000));
}

#[test]
fn carryover_is_written_from_the_start_month_through_the_last_created_month() {
	let budget = small_fixture();
	let sheet = budget.sheet(TODAY);
	let editor = BudgetEditor::new(&budget.0, &sheet, NumberFormat::CommaDot, false);
	let writes = editor
		.set_carryover(ym("2026-10"), &cat("dining"), true)
		.unwrap();
	assert_eq!(writes.cells.len(), 14);
	assert_eq!(writes.cells.last().unwrap().month, ym("2027-11"));
	assert!(
		writes
			.cells
			.iter()
			.all(|c| c.carryover == Some(true) && c.amount.is_none())
	);
	let late = editor
		.set_carryover(ym("2028-02"), &cat("dining"), true)
		.unwrap();
	assert_eq!(
		late.cells.len(),
		1,
		"Actual's rangeInclusive still yields the start month"
	);
}

#[test]
fn copying_last_month_skips_income_hidden_and_deleted_categories() {
	let mut budget = small_fixture();
	budget.category("hidden", "everyday", false).hidden = true;
	budget.budget("2026-10", "hidden", 777);
	budget.budget("2026-10", "income", 999);
	budget.category("gone", "everyday", false).tombstone = true;
	budget.budget("2026-10", "gone", 555);
	let writes = budget.edit(|e| e.copy_previous_month(ym("2026-11")).unwrap());
	let copied: Vec<_> = writes
		.cells
		.iter()
		.map(|c| (c.category.as_str(), c.amount.unwrap().minor()))
		.collect();
	assert_eq!(copied.len(), 4);
	assert!(copied.contains(&("rent", 150_000)) && copied.contains(&("dining", 5_000)));
	assert_eq!(budgeted(&budget, "2026-11", "hidden"), Money::ZERO);

	budget.edit(|e| {
		e.copy_single_previous_month(ym("2026-12"), &cat("groceries"))
			.unwrap()
	});
	assert_eq!(budgeted(&budget, "2026-12", "groceries"), cents(12_000));
	budget.edit(|e| {
		e.copy_until_year_end(ym("2026-10"), &cat("utilities"))
			.unwrap()
	});
	assert_eq!(budgeted(&budget, "2026-12", "utilities"), cents(9_000));
	assert_eq!(budgeted(&budget, "2027-01", "utilities"), Money::ZERO);
}

#[test]
fn set_zero_clears_every_live_expense_category_hidden_ones_too() {
	let mut budget = small_fixture();
	budget.category("hidden", "everyday", false).hidden = true;
	let writes = budget.edit(|e| e.set_zero(ym("2026-10")).unwrap());
	assert_eq!(
		writes.cells.len(),
		10,
		"five seed, four fixture and one hidden category"
	);
	assert_eq!(budgeted(&budget, "2026-10", "rent"), Money::ZERO);
}

#[test]
fn averages_walk_back_from_last_month_to_the_first_activity() {
	let budget = small_fixture();
	let sheet = budget.sheet(TODAY);
	let editor = BudgetEditor::new(&budget.0, &sheet, NumberFormat::CommaDot, false);
	let average = |month: &str, category: &str, span| {
		editor
			.category_average(ym(month), &cat(category), span)
			.unwrap()
	};
	assert_eq!(average("2026-11", "groceries", AverageSpan::Three), -12_657);
	assert_eq!(
		average("2026-11", "groceries", AverageSpan::Twelve),
		-12_657,
		"stops at August"
	);
	assert_eq!(
		average("2027-03", "dining", AverageSpan::Three),
		-4_810,
		"future months average up to last month"
	);
	assert_eq!(average("2026-11", "food", AverageSpan::Six), 0);
	assert_eq!(average("2026-11", "income", AverageSpan::Six), 400_000);

	let writes = editor
		.set_average(ym("2026-11"), AverageSpan::Three)
		.unwrap();
	let groceries = writes
		.cells
		.iter()
		.find(|c| c.category == cat("groceries"))
		.unwrap();
	assert_eq!(groceries.amount, Some(cents(12_657)));
	assert!(writes.cells.iter().all(|c| c.category != cat("income")));
	let income = editor
		.set_category_average(ym("2026-11"), &cat("income"), AverageSpan::Three)
		.unwrap();
	assert_eq!(income.cells[0].amount, Some(cents(400_000)));
}

#[test]
fn averages_round_halves_toward_positive_infinity_like_math_round() {
	let mut budget = Budget::actual_seed();
	budget.account("a", false);
	budget.txn("sep", "a", "2026-09-03", -2, Some("food"));
	budget.txn("oct", "a", "2026-10-03", -1, Some("food"));
	let sheet = budget.sheet(TODAY);
	let editor = BudgetEditor::new(&budget.0, &sheet, NumberFormat::CommaDot, false);
	assert_eq!(
		editor
			.category_average(ym("2026-11"), &cat("food"), AverageSpan::Three)
			.unwrap(),
		-1
	);
}

#[test]
fn income_auto_hold_and_its_resets() {
	let mut budget = small_fixture();
	budget.hold("2026-10", 1_000);
	let writes = budget.edit(|e| {
		e.set_income_auto_hold(ym("2026-10"), &cat("income"), true)
			.unwrap()
	});
	assert_eq!(writes.holds, vec![(ym("2026-10"), Money::ZERO)]);
	let october = *budget.sheet(TODAY).summary(ym("2026-10")).unwrap();
	assert_eq!(october.for_next_month, cents(400_000));

	let writes = budget.edit(|e| e.reset_income_carryover(ym("2026-10")));
	let ids: Vec<_> = writes.cells.iter().map(|c| c.category.as_str()).collect();
	assert_eq!(ids, ["income", "starting-balances"]);
	assert_eq!(
		budget
			.sheet(TODAY)
			.summary(ym("2026-10"))
			.unwrap()
			.auto_hold,
		Money::ZERO
	);
	assert!(
		budget.0.budget_cells[&(ym("2026-11"), cat("income"))].carryover,
		"later months keep auto hold"
	);
}

/// Actual would add the amount to the envelope; Veritas refuses instead.
#[test]
fn moving_an_envelope_to_itself_is_refused() {
	let budget = small_fixture();
	let sheet = budget.sheet(TODAY);
	let editor = BudgetEditor::new(&budget.0, &sheet, NumberFormat::CommaDot, false);
	let result = editor.move_budget(ym("2026-10"), cents(100), &cat("rent"), &to("rent"));
	assert!(matches!(result, Err(CoreError::Invalid(_))));
}

#[test]
fn amounts_beyond_actuals_safe_range_are_refused() {
	let budget = small_fixture();
	let sheet = budget.sheet(TODAY);
	let editor = BudgetEditor::new(&budget.0, &sheet, NumberFormat::CommaDot, false);
	let too_big = cents(1 << 51);
	assert!(matches!(
		editor.set_budget(ym("2026-10"), &cat("rent"), too_big),
		Err(CoreError::Overflow)
	));
	assert!(matches!(
		editor.move_budget(ym("2026-10"), too_big, &cat("rent"), &to("dining")),
		Err(CoreError::Overflow)
	));
}

type Edit = Box<dyn Fn(&BudgetEditor<'_>) -> BudgetWrites>;

#[test]
fn the_sheet_catches_up_from_the_first_month_an_edit_touches() {
	let mut budget = small_fixture();
	let mut sheet = budget.sheet(TODAY);
	let edits: Vec<Edit> = vec![
		Box::new(|e| {
			e.move_budget(ym("2026-09"), cents(4_000), &cat("rent"), &to("groceries"))
				.unwrap()
		}),
		Box::new(|e| {
			e.cover_overspending(ym("2026-09"), &cat("rent"), &Envelope::ToBudget, None)
				.unwrap()
		}),
		Box::new(|e| {
			e.set_carryover(ym("2026-08"), &cat("groceries"), true)
				.unwrap()
		}),
		Box::new(|e| {
			e.hold_for_next_month(ym("2026-11"), cents(5_000))
				.unwrap()
				.unwrap()
		}),
		Box::new(|e| e.set_average(ym("2027-02"), AverageSpan::Six).unwrap()),
	];
	for edit in edits {
		let writes = edit(&BudgetEditor::new(
			&budget.0,
			&sheet,
			NumberFormat::CommaDot,
			false,
		));
		writes.apply_to(&mut budget.0);
		sheet
			.apply(&budget.0, SheetChange::Month(writes.first_month().unwrap()))
			.unwrap();
		let fresh = budget.sheet(TODAY);
		for month in fresh.months() {
			assert_eq!(sheet.month(*month), fresh.month(*month), "{month}");
		}
	}
}
