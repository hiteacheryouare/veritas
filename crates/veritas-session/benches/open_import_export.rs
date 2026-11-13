#![allow(
	clippy::unwrap_used,
	clippy::expect_used,
	reason = "a benchmark fails by panicking"
)]

//! R-PERF-1 (core portion: open a 100k-transaction budget until it takes
//! commands), R-PERF-2 (core portion: a budget cell edit, committed, then the
//! month as the Budget screen reads it) and R-PERF-5 (import and export of the
//! same budget as an Actual zip), on fixtures/synthetic/perf-100k. Measure on an idle machine:
//! `cargo bench -p veritas-session`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};
use veritas_domain::{BudgetData, CategoryId, CivilDate, CoreError, Money, Row};
use veritas_engine::{BudgetEditor, EnvelopeSheet, NumberFormat};
use veritas_session::{Command, Session, SessionOptions};

type BenchSession = Session<veritas_store::Store>;

fn fixture() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/perf-100k.zip")
}

fn scratch() -> PathBuf {
	static NEXT: AtomicUsize = AtomicUsize::new(0);
	let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("../../target/tmp/bench")
		.join(format!(
			"{}-{}",
			std::process::id(),
			NEXT.fetch_add(1, Ordering::Relaxed)
		));
	let _ = std::fs::remove_dir_all(&dir);
	std::fs::create_dir_all(&dir).unwrap();
	dir
}

fn benches(c: &mut Criterion) {
	let root = scratch();
	let budget = root.join("budget");
	veritas_interop::import_actual_zip(&fixture(), &budget).unwrap();

	let mut group = c.benchmark_group("perf-100k");
	group.sample_size(10);
	group.measurement_time(Duration::from_secs(20));

	// Each iteration times one step, then waits for the background check and
	// deletes what it wrote outside the timing, so a run doesn't fill the disk
	// with 58 MB copies.
	group.bench_function("R-PERF-1 open until editable", |b| {
		b.iter_custom(|iters| {
			let mut total = Duration::ZERO;
			for _ in 0..iters {
				let started = Instant::now();
				let mut session = BenchSession::open(&budget, SessionOptions::default()).unwrap();
				total += started.elapsed();
				session.wait_for_open_check();
			}
			total
		});
	});

	group.bench_function("R-PERF-5 import an Actual zip", |b| {
		b.iter_custom(|iters| {
			let mut total = Duration::ZERO;
			for _ in 0..iters {
				let into = scratch();
				let started = Instant::now();
				veritas_interop::import_actual_zip(&fixture(), &into.join("budget")).unwrap();
				total += started.elapsed();
				std::fs::remove_dir_all(&into).unwrap();
			}
			total
		});
	});

	let mut session = BenchSession::open(&budget, SessionOptions::default()).unwrap();
	session.wait_for_open_check();
	let snapshots = budget.join(veritas_session::SNAPSHOTS_DIR);
	group.bench_function("R-PERF-5 export an Actual zip", |b| {
		b.iter_custom(|iters| {
			let mut total = Duration::ZERO;
			for _ in 0..iters {
				let to = root.join("export.zip");
				let started = Instant::now();
				session.export_actual_zip(&to).unwrap();
				total += started.elapsed();
				std::fs::remove_file(&to).unwrap();
				std::fs::remove_dir_all(&snapshots).unwrap();
			}
			total
		});
	});

	// The current month of the fixture's clock, so the edit lands in a month
	// with 100k transactions of history before it.
	let today = CivilDate::new(2026, 11, 15).unwrap();
	let category = session
		.with_sheet(today, |data, _| {
			Ok(data
				.categories
				.values()
				.find(|c| !c.tombstone && !c.is_income)
				.unwrap()
				.id
				.clone())
		})
		.unwrap();
	let mut amount = 0;
	group.bench_function(
		"R-PERF-2 budget cell edit until the month reads back",
		|b| {
			b.iter(|| {
				amount += 100;
				session
					.perform(&SetBudget {
						category: category.clone(),
						amount: Money::from_minor(amount),
					})
					.unwrap();
				session
					.with_sheet(today, |_, sheet| Ok(sheet.month(today.year_month())))
					.unwrap()
			});
		},
	);
	group.finish();
	drop(session);
	std::fs::remove_dir_all(&root).unwrap();
}

/// What the app's SetBudget command plans, without the FFI crate.
struct SetBudget {
	category: CategoryId,
	amount: Money,
}

impl Command for SetBudget {
	fn plan(&self, _: &BudgetData) -> Result<Vec<Row>, CoreError> {
		Err(CoreError::Internal("planned with the sheet".to_owned()))
	}

	fn reads_sheet(&self) -> bool {
		true
	}

	fn plan_with_sheet(
		&self,
		data: &BudgetData,
		sheet: &EnvelopeSheet,
	) -> Result<Vec<Row>, CoreError> {
		let editor = BudgetEditor::new(
			data,
			sheet,
			NumberFormat::from_preference("comma-dot"),
			false,
		);
		let writes = editor.set_budget(sheet.today().year_month(), &self.category, self.amount)?;
		Ok(veritas_session::writes::rows(data, &writes))
	}
}

criterion_group!(open_import_export, benches);
criterion_main!(open_import_export);
