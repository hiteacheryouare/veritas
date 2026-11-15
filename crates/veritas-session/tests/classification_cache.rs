#![allow(
	clippy::unwrap_used,
	clippy::expect_used,
	clippy::panic,
	reason = "integration tests fail by panicking"
)]

//! The session's classification cache (R-PERF-4) for a commit no FFI
//! command makes alone yet: a category's kind changing by itself, as the
//! kind wizard does. Every report reads the cache, so it must follow.

mod support;

use std::path::Path;

use support::TempDir;
use veritas_domain::{BudgetData, CategoryKind, CategoryMetaRow, CoreError, Row};
use veritas_reports::{ClassifiedRow, classify};
use veritas_session::{Session, SessionOptions};

fn cached_and_fresh(
	session: &mut Session<veritas_store::Store>,
) -> (Vec<ClassifiedRow>, Vec<ClassifiedRow>) {
	session
		.with_classification(|data, cached| {
			Ok((cached.rows().to_vec(), classify(data).rows().to_vec()))
		})
		.unwrap()
}

#[test]
fn a_category_kind_changing_alone_reclassifies_its_rows() {
	let dir = TempDir::new("classification-kind");
	let zip =
		Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/all-features.zip");
	let mut session = Session::<veritas_store::Store>::import_actual_zip(
		&zip,
		&dir.path().join("budget"),
		SessionOptions::default(),
	)
	.unwrap();
	let (mut before, fresh) = cached_and_fresh(&mut session);
	assert_eq!(before, fresh);

	let expenses: Vec<_> = session
		.data()
		.unwrap()
		.categories
		.values()
		.filter(|c| !c.tombstone && !c.is_income)
		.map(|c| c.id.clone())
		.collect();
	let mut moved = 0;
	for category in &expenses {
		for kind in [CategoryKind::Savings, CategoryKind::DebtPrincipal] {
			let set_kind = |data: &BudgetData| -> Result<Vec<Row>, CoreError> {
				let write =
					veritas_engine::category_kinds::set_category_kind(data, category, kind)?;
				Ok(vec![Row::CategoryMeta(CategoryMetaRow {
					category: write.category,
					meta: write.meta,
				})])
			};
			session.perform(&set_kind).unwrap();
			let (cached, fresh) = cached_and_fresh(&mut session);
			assert_eq!(cached, fresh, "{category} as {kind:?}");
			moved += usize::from(cached != before);
			before = cached;
		}
	}
	assert!(
		moved > 0,
		"some kind change moved a transfer between debt paydown, savings and spending"
	);
}
