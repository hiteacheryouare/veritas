//! R-PERF-4 on perf-100k: a typical report within 100 ms and a year-long
//! Sankey within 500 ms, each timed from the loaded budget, classification
//! included, because any edit makes the session classify again.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::hint::black_box;
use std::path::Path;

use criterion::{Criterion, criterion_group, criterion_main};
use veritas_domain::{BudgetData, CivilDate, YearMonth};
use veritas_reports::{
	Period, balance_sheet, cash_flow, classify, net_profit, net_worth_by_month, owners_equity,
	sankey, tax_summary,
};

fn load_perf_100k() -> BudgetData {
	let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
	let scratch =
		Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("perf-100k-{}", std::process::id()));
	let _ = std::fs::remove_dir_all(&scratch);
	let budget = veritas_interop::import_actual_zip(
		&root.join("fixtures/synthetic/perf-100k.zip"),
		&scratch,
	)
	.unwrap();
	let mut store = veritas_store::Store::open(&budget.db_path).unwrap();
	store.adopt().unwrap();
	let data = store.load().unwrap().data;
	drop(store);
	let _ = std::fs::remove_dir_all(&scratch);
	data
}

fn reports(c: &mut Criterion) {
	let budget = load_perf_100k();
	let last = budget
		.transactions
		.values()
		.map(|row| row.date)
		.max()
		.expect("perf-100k has rows");
	let year = last.year() - 1;
	let full_year = Period::new(
		CivilDate::new(year, 1, 1).unwrap(),
		CivilDate::new(year, 12, 31).unwrap(),
	)
	.unwrap();
	let month = Period::month(YearMonth::new(year, 6).unwrap());
	let first_month = budget
		.transactions
		.values()
		.map(|row| row.date)
		.min()
		.unwrap()
		.year_month();

	let mut group = c.benchmark_group("perf-100k");
	group.sample_size(20);
	group.bench_function("classify", |b| b.iter(|| classify(black_box(&budget))));
	group.bench_function("net profit, one month", |b| {
		b.iter(|| net_profit(&budget, &classify(&budget), month).unwrap())
	});
	group.bench_function("net profit, one year", |b| {
		b.iter(|| net_profit(&budget, &classify(&budget), full_year).unwrap())
	});
	group.bench_function("balance sheet", |b| {
		b.iter(|| balance_sheet(&budget, &classify(&budget), last).unwrap())
	});
	group.bench_function("owner's equity, one year", |b| {
		b.iter(|| owners_equity(&budget, &classify(&budget), full_year).unwrap())
	});
	group.bench_function("cash flow, one year", |b| {
		b.iter(|| cash_flow(&budget, &classify(&budget), full_year).unwrap())
	});
	group.bench_function("net worth, every month", |b| {
		b.iter(|| {
			net_worth_by_month(&budget, &classify(&budget), first_month, last.year_month()).unwrap()
		})
	});
	group.bench_function("tax summary, one year", |b| {
		b.iter(|| tax_summary(&budget, &classify(&budget), year).unwrap())
	});
	group.bench_function("sankey, one year", |b| {
		b.iter(|| sankey(&budget, &classify(&budget), full_year).unwrap())
	});
	let cached = classify(&budget);
	group.bench_function("net profit, one month, classification cached", |b| {
		b.iter(|| net_profit(&budget, &cached, month).unwrap())
	});
	group.bench_function("sankey, one year, classification cached", |b| {
		b.iter(|| sankey(&budget, &cached, full_year).unwrap())
	});
	group.finish();
}

criterion_group!(benches, reports);
criterion_main!(benches);
