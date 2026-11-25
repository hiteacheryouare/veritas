use std::collections::BTreeSet;

use veritas_domain::{BudgetData, EntityRef, TransactionId};
use veritas_reports::{Classification, classify};

use crate::bus::ChangeSet;

/// Every row's kind for the loaded budget, kept between report calls so a
/// report reads it instead of classifying the whole history again (R-PERF-4).
/// A commit that writes only transactions leaves it in place, and the next
/// report updates just the rows those writes reach. A commit that touches
/// anything else classification reads drops it, and the next report
/// rebuilds it.
#[derive(Default)]
pub(crate) struct ClassificationCache {
	classification: Option<Classification>,
	/// Transactions written since `classification` was last brought up to date.
	written: BTreeSet<TransactionId>,
}

impl ClassificationCache {
	pub(crate) fn get(&mut self, data: &BudgetData) -> &Classification {
		let written = std::mem::take(&mut self.written);
		if let Some(classification) = &mut self.classification
			&& !written.is_empty()
		{
			classification.reclassify(data, &written);
		}
		self.classification.get_or_insert_with(|| classify(data))
	}

	pub(crate) fn clear(&mut self) {
		self.classification = None;
		self.written.clear();
	}

	pub(crate) fn refresh(&mut self, changed: &ChangeSet) {
		let rebuild = changed.everything
			|| changed.entities.iter().any(|entity| {
				affects_classification(entity) && !matches!(entity, EntityRef::Transaction(_))
			});
		if rebuild {
			self.clear();
			return;
		}
		if self.classification.is_some() {
			for entity in &changed.entities {
				if let EntityRef::Transaction(id) = entity {
					self.written.insert(id.clone());
				}
			}
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
			| EntityRef::Setting(_)
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
	fn budget_and_transaction_edits_keep_the_classification_and_others_drop_it() {
		let data = BudgetData::empty(BudgetType::Envelope);
		let mut cache = ClassificationCache::default();
		cache.get(&data);
		let month = YearMonth::new(2026, 11).unwrap();
		cache.refresh(&changed(vec![
			EntityRef::BudgetCell(month, CategoryId::new("food")),
			EntityRef::Hold(month),
		]));
		assert!(cache.classification.is_some());
		assert!(cache.written.is_empty());

		cache.refresh(&changed(vec![EntityRef::Transaction(TransactionId::new(
			"t",
		))]));
		assert!(cache.classification.is_some());
		assert_eq!(cache.written.len(), 1);
		cache.get(&data);
		assert!(cache.written.is_empty());

		cache.refresh(&changed(vec![
			EntityRef::Transaction(TransactionId::new("t")),
			EntityRef::Category(CategoryId::new("food")),
		]));
		assert!(cache.classification.is_none());
		assert!(cache.written.is_empty());

		cache.get(&data);
		cache.refresh(&ChangeSet {
			everything: true,
			..ChangeSet::default()
		});
		assert!(cache.classification.is_none());
	}
}
