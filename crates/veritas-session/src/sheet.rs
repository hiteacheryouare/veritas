use std::collections::BTreeSet;

use veritas_domain::{BudgetData, CivilDate, CoreError, EntityRef, TransactionId, YearMonth};
use veritas_engine::{EnvelopeSheet, SheetChange};

use crate::bus::ChangeSet;

/// Actual's envelope sheet for the loaded budget, kept between calls so the
/// Budget screen and budget commands don't recompute it (R-PERF-2). Each
/// commit tells it what changed, and it recomputes only from there.
#[derive(Default)]
pub(crate) struct SheetCache {
	sheet: Option<EnvelopeSheet>,
}

impl SheetCache {
	/// The sheet as of `today`, whose month decides the months Actual creates.
	pub(crate) fn get(
		&mut self,
		data: &BudgetData,
		today: CivilDate,
	) -> Result<&EnvelopeSheet, CoreError> {
		let stale = self
			.sheet
			.as_ref()
			.is_none_or(|sheet| sheet.today() != today);
		if stale {
			self.sheet = Some(EnvelopeSheet::compute(data, today)?);
		}
		self.sheet
			.as_ref()
			.ok_or_else(|| CoreError::Internal("the envelope sheet went missing".to_owned()))
	}

	pub(crate) fn clear(&mut self) {
		self.sheet = None;
	}

	/// Brings the sheet up to date with a commit, undo or redo. If that fails,
	/// the sheet is dropped and rebuilt on the next read.
	pub(crate) fn refresh(&mut self, data: &BudgetData, changed: &ChangeSet) {
		let Some(sheet) = self.sheet.as_mut() else {
			return;
		};
		if changed.everything {
			self.sheet = None;
			return;
		}
		let mut structure = false;
		let mut first_month: Option<YearMonth> = None;
		let mut transactions: BTreeSet<TransactionId> = BTreeSet::new();
		for entity in &changed.entities {
			match entity {
				EntityRef::Account(_)
				| EntityRef::Category(_)
				| EntityRef::CategoryGroup(_)
				| EntityRef::CategoryMapping(_) => structure = true,
				EntityRef::BudgetCell(month, _) | EntityRef::Hold(month) => {
					first_month = Some(first_month.map_or(*month, |m| m.min(*month)));
				}
				EntityRef::Transaction(id) => {
					transactions.insert(id.clone());
				}
				_ => {}
			}
		}
		let updated = if structure {
			sheet.apply(data, SheetChange::Structure)
		} else {
			let ids: Vec<TransactionId> = transactions.into_iter().collect();
			let by_transactions = if ids.is_empty() {
				Ok(())
			} else {
				sheet.apply(data, SheetChange::Transactions(&ids))
			};
			by_transactions.and_then(|()| match first_month {
				Some(month) => sheet.apply(data, SheetChange::Month(month)),
				None => Ok(()),
			})
		};
		if updated.is_err() {
			self.sheet = None;
		}
	}
}
