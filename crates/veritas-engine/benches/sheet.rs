//! R-PERF-2: a budget cell edit, its recompute and the month's screen model
//! within one frame (16 ms), on PLAN §9 #19's scale: 100k transactions,
//! 15+ years of months and 500 categories.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use veritas_domain::{
	Account, AccountId, BudgetData, BudgetType, Category, CategoryGroup, CategoryGroupId,
	CategoryId, CivilDate, Money, SplitRole, Transaction, TransactionId, YearMonth,
};
use veritas_engine::{AverageSpan, BudgetEditor, EnvelopeSheet, NumberFormat, SheetChange};

const CATEGORIES: usize = 500;
const GROUPS: usize = 25;
const TRANSACTIONS: usize = 100_000;

fn budget() -> BudgetData {
	let mut budget = BudgetData::empty(BudgetType::Envelope);
	let group = |i: usize| CategoryGroupId::new(format!("group-{i}"));
	for i in 0..=GROUPS {
		budget.category_groups.insert(
			group(i),
			CategoryGroup {
				id: group(i),
				name: format!("Group {i}"),
				is_income: i == GROUPS,
				hidden: false,
				sort_order: i as f64,
				tombstone: false,
			},
		);
	}
	for c in 0..CATEGORIES {
		let id = CategoryId::new(format!("category-{c}"));
		budget.category_mapping.insert(id.clone(), id.clone());
		let is_income = c < 5;
		budget.categories.insert(
			id.clone(),
			Category {
				id,
				name: format!("Category {c}"),
				group: if is_income {
					group(GROUPS)
				} else {
					group(c % GROUPS)
				},
				is_income,
				hidden: false,
				sort_order: c as f64,
				tombstone: false,
			},
		);
	}
	let account = AccountId::new("checking");
	budget.accounts.insert(
		account.clone(),
		Account {
			last_reconciled: None,
			id: account.clone(),
			name: "Checking".to_owned(),
			off_budget: false,
			closed: false,
			group: None,
			sort_order: 0.0,
			tombstone: false,
		},
	);
	for t in 0..TRANSACTIONS {
		let id = TransactionId::new(format!("t-{t}"));
		let date = CivilDate::new(
			2011 + (t % 16) as i32,
			1 + (t % 12) as u32,
			1 + (t % 28) as u32,
		)
		.unwrap();
		let category = t % CATEGORIES;
		let amount = if category < 5 {
			250_000
		} else {
			-((t as i64 * 37) % 20_000)
		};
		budget.transactions.insert(
			id.clone(),
			Transaction {
				id,
				account: account.clone(),
				date,
				amount: Money::from_minor(amount),
				payee: None,
				imported_payee: None,
				category: Some(CategoryId::new(format!("category-{category}"))),
				notes: None,
				cleared: true,
				reconciled: false,
				split: SplitRole::None,
				transfer: None,
				schedule: None,
				starting_balance: false,
				sort_order: t as f64,
				tombstone: false,
				split_error: None,
			},
		);
	}
	budget
}

fn benches(c: &mut Criterion) {
	let today = CivilDate::new(2026, 11, 9).unwrap();
	let current = today.year_month();
	let mut budget = budget();
	let mut sheet = EnvelopeSheet::compute(&budget, today).unwrap();
	let first = *sheet.months().first().unwrap();
	assert!(sheet.months().len() >= 180, "15+ years of months");

	c.bench_function("compute the whole sheet", |b| {
		b.iter(|| EnvelopeSheet::compute(black_box(&budget), today).unwrap())
	});

	let mut edit = |month: YearMonth, step: i64| {
		let editor = BudgetEditor::new(&budget, &sheet, NumberFormat::CommaDot, false);
		let writes = editor
			.set_budget(
				month,
				&CategoryId::new("category-7"),
				Money::from_minor(step),
			)
			.unwrap();
		writes.apply_to(&mut budget);
		sheet.apply(&budget, SheetChange::Month(month)).unwrap();
		black_box(sheet.month(current).unwrap());
	};
	let mut step = 0;
	c.bench_function(
		"R-PERF-2 budget edit in the first month, recompute, screen model",
		|b| {
			b.iter(|| {
				step += 1;
				edit(first, step);
			})
		},
	);
	c.bench_function(
		"R-PERF-2 budget edit in the current month, recompute, screen model",
		|b| {
			b.iter(|| {
				step += 1;
				edit(current, step);
			})
		},
	);

	let id = [TransactionId::new("t-12345")];
	let mut amount = 0;
	c.bench_function("transaction edit, recompute, screen model", |b| {
		b.iter(|| {
			amount -= 1;
			budget.transactions.get_mut(&id[0]).unwrap().amount = Money::from_minor(amount);
			sheet
				.apply(&budget, SheetChange::Transactions(&id))
				.unwrap();
			black_box(sheet.month(current).unwrap());
		})
	});

	let mut average = |all: bool| {
		let editor = BudgetEditor::new(&budget, &sheet, NumberFormat::CommaDot, false);
		let writes = if all {
			editor.set_average(current, AverageSpan::Three).unwrap()
		} else {
			editor
				.set_category_average(current, &CategoryId::new("category-7"), AverageSpan::Three)
				.unwrap()
		};
		writes.apply_to(&mut budget);
		sheet.apply(&budget, SheetChange::Month(current)).unwrap();
		black_box(sheet.month(current).unwrap());
	};
	c.bench_function(
		"R-PERF-2 set one envelope to its 3-month average, recompute, screen model",
		|b| b.iter(|| average(false)),
	);
	let mut slow = c.benchmark_group("bulk");
	slow.sample_size(10);
	slow.bench_function(
		"set every envelope to its 3-month average, recompute, screen model",
		|b| b.iter(|| average(true)),
	);
	slow.finish();
}

criterion_group!(sheet, benches);
criterion_main!(sheet);
