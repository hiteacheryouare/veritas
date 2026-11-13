use std::collections::BTreeSet;

use veritas_domain::{BudgetCell, BudgetData, CategoryId, Change, Hold, Note, Row, YearMonth};
use veritas_engine::BudgetWrites;

/// The rows that hold `writes` once applied: each touched budget cell, hold
/// and note as it ends up.
pub fn rows(data: &BudgetData, writes: &BudgetWrites) -> Vec<Row> {
	let cells: BTreeSet<(YearMonth, CategoryId)> = writes
		.cells
		.iter()
		.map(|cell| (cell.month, cell.category.clone()))
		.collect();
	let holds: BTreeSet<YearMonth> = writes.holds.iter().map(|(month, _)| *month).collect();
	let notes: BTreeSet<&String> = writes.notes.iter().map(|(id, _)| id).collect();
	// Writes only touch these three tables, so the rest of a large budget isn't
	// cloned (R-PERF-2).
	let mut after = BudgetData::empty(data.budget_type);
	for key in &cells {
		if let Some(cell) = data.budget_cells.get(key) {
			after.budget_cells.insert(key.clone(), cell.clone());
		}
	}
	for month in &holds {
		if let Some(hold) = data.holds.get(month) {
			after.holds.insert(*month, *hold);
		}
	}
	for id in &notes {
		if let Some(note) = data.notes.get(*id) {
			after.notes.insert((*id).clone(), note.clone());
		}
	}
	writes.apply_to(&mut after);
	let mut rows = Vec::new();
	for key in cells {
		if let Some(cell) = after.budget_cells.get(&key) {
			rows.push(Row::BudgetCell(BudgetCell::clone(cell)));
		}
	}
	for month in holds {
		rows.push(Row::Hold(Hold {
			month,
			amount: after.holds.get(&month).copied().flatten(),
		}));
	}
	for id in notes {
		rows.push(Row::Note(Note {
			id: id.clone(),
			text: after.notes.get(id).cloned().flatten(),
		}));
	}
	rows
}

/// `data` with `rows` applied, for planning a command in steps.
pub fn applied(data: &BudgetData, rows: &[Row]) -> BudgetData {
	let mut after = data.clone();
	for row in rows {
		Change::put(&after, row.clone()).apply(&mut after);
	}
	after
}
