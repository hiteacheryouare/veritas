use veritas_domain::{BudgetData, EntityRef};
use veritas_reports::{Classification, classify};

use crate::bus::ChangeSet;

/// Every row's kind for the loaded budget, kept between report calls so a
/// report reads it instead of classifying the whole history again (R-PERF-4).
/// A commit that touches anything classification reads drops it, and the
/// next report rebuilds it.
#[derive(Default)]
pub(crate) struct ClassificationCache {
	classification: Option<Classification>,
}

impl ClassificationCache {
	pub(crate) fn get(&mut self, data: &BudgetData) -> &Classification {
		self.classification.get_or_insert_with(|| classify(data))
	}

	pub(crate) fn clear(&mut self) {
		self.classification = None;
	}

	pub(crate) fn refresh(&mut self, changed: &ChangeSet) {
		if changed.everything || changed.entities.iter().any(affects_classification) {
			self.classification = None;
		}
	}
}

/// Rows a classification never reads. Anything else, including entities
/// added later, drops the cache.
fn affects_classification(entity: &EntityRef) -> bool {
	!matches!(
		entity,
		EntityRef::BudgetCell(..)
			| EntityRef::Hold(_)
			| EntityRef::Note(_)
			| EntityRef::Rule(_)
			| EntityRef::Schedule(_)
			| EntityRef::Tag(_)
			| EntityRef::TaxTag(_)
			| EntityRef::Goal(_)
			| EntityRef::CustomReport(_)
			| EntityRef::DashboardPage(_)
			| EntityRef::Widget(_)
	)
}

#[cfg(test)]
mod tests {
	use veritas_domain::{BudgetType, CategoryId, TransactionId, YearMonth};

	use super::*;

	fn changed(entities: Vec<EntityRef>) -> ChangeSet {
		ChangeSet {
			entities: entities.into_iter().collect(),
			..ChangeSet::default()
		}
	}

	#[test]
	fn budget_edits_keep_the_classification_and_transaction_edits_drop_it() {
		let data = BudgetData::empty(BudgetType::Envelope);
		let mut cache = ClassificationCache::default();
		cache.get(&data);
		let month = YearMonth::new(2026, 11).unwrap();
		cache.refresh(&changed(vec![
			EntityRef::BudgetCell(month, CategoryId::new("food")),
			EntityRef::Hold(month),
		]));
		assert!(cache.classification.is_some());

		cache.refresh(&changed(vec![EntityRef::Transaction(TransactionId::new(
			"t",
		))]));
		assert!(cache.classification.is_none());

		cache.get(&data);
		cache.refresh(&ChangeSet {
			everything: true,
			..ChangeSet::default()
		});
		assert!(cache.classification.is_none());
	}
}
