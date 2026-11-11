//! R-FC-1 baselines: every FC-1 worked example (F-1 to F-9) plus the
//! bill-linked exclusion and the 25-month seasonal rule.

#![allow(clippy::unwrap_used, clippy::panic)]

mod support;

use support::*;
use veritas_forecast::{Baseline, Forecaster, Side};

fn baseline(book: &Book, today: &str, category: &str) -> Baseline {
	Forecaster::new(&book.budget)
		.unwrap()
		.baselines(date(today), &[])
		.unwrap()
		.into_iter()
		.find(|baseline| baseline.category == cat(category))
		.unwrap()
}

fn months(first: &str, values: &[i64]) -> Vec<(String, i64)> {
	values
		.iter()
		.enumerate()
		.map(|(step, x)| (ym(first).offset(step as i32).unwrap().to_string(), *x))
		.collect()
}

fn book_with(category: &str, first: &str, values: &[i64]) -> Book {
	let mut book = Book::household();
	let series = months(first, values);
	let series: Vec<(&str, i64)> = series.iter().map(|(m, x)| (m.as_str(), *x)).collect();
	book.monthly(category, &series);
	book
}

#[test]
fn f_1_even_median_of_six_months() {
	let book = book_with(
		"Groceries",
		"2026-07",
		&[42000, 42000, 43000, 43000, 44000, 44000],
	);
	let groceries = baseline(&book, "2027-01-15", "Groceries");
	assert_eq!(groceries.level, cents(43000));
	assert_eq!(groceries.side, Side::Spending);
	assert_eq!(groceries.label(), "6-month median");
}

#[test]
fn f_2_half_cent_rounds_up() {
	let book = book_with("Groceries", "2026-09", &[42000, 42001, 41000, 45000]);
	let groceries = baseline(&book, "2027-01-15", "Groceries");
	assert_eq!(
		(groceries.level, groceries.window_months),
		(cents(42001), 4)
	);
}

#[test]
fn f_3_short_history_is_labeled() {
	let book = book_with("Groceries", "2026-11", &[10000, 20000]);
	let groceries = baseline(&book, "2027-01-15", "Groceries");
	assert_eq!(groceries.level, cents(15000));
	assert_eq!(groceries.label(), "2-month median");
}

#[test]
fn f_4_zero_history() {
	let book = Book::household();
	let fun = baseline(&book, "2027-01-15", "Fun");
	assert_eq!(fun.level, cents(0));
	assert!(!fun.has_history());
	assert_eq!(fun.label(), "No history yet");
}

#[test]
fn f_5_zero_months_inside_the_window_count() {
	let book = book_with("Gifts", "2026-06", &[5000, 0, 0, 30000, 0, 0, 30000]);
	let gifts = baseline(&book, "2027-01-15", "Gifts");
	assert_eq!((gifts.level, gifts.window_months), (cents(0), 6));
}

#[test]
fn f_6_new_category_window_starts_at_its_first_row() {
	let book = book_with("Fuel", "2026-11", &[5000, 7000]);
	let fuel = baseline(&book, "2027-01-15", "Fuel");
	assert_eq!((fuel.level, fuel.window_months), (cents(6000), 2));
}

#[test]
fn f_7_seasonality_after_13_months() {
	// Nov 2026 opens the history; Dec 2026 to Nov 2027 is the 12-month series.
	let mut values = vec![5000, 8000];
	values.extend([5000; 11]);
	let book = book_with("Gifts", "2026-11", &values);
	let gifts = baseline(&book, "2027-12-05", "Gifts");
	assert_eq!(gifts.history_months, 13);
	let index = gifts.seasonal_index.unwrap();
	assert_eq!(index[11], 12619);
	assert!(index[..11].iter().all(|bp| *bp == 9762), "{index:?}");
	assert_eq!(gifts.deseasonalized_level, Some(cents(5122)));
	assert_eq!(gifts.forecast(ym("2027-12")).unwrap(), cents(6463));
	assert_eq!(gifts.level, cents(5000));
}

#[test]
fn f_7_without_enough_history_there_is_no_index() {
	let mut values = vec![8000];
	values.extend([5000; 11]);
	let book = book_with("Gifts", "2026-12", &values);
	let gifts = baseline(&book, "2027-12-05", "Gifts");
	assert_eq!(gifts.seasonal_index, None);
	assert_eq!(gifts.forecast(ym("2027-12")).unwrap(), cents(5000));
}

#[test]
fn f_8_sparse_category_gets_no_index() {
	let mut values = vec![5000];
	values.extend([9000, 0, 0, 9000, 0, 0, 9000, 0, 0, 9000, 0, 0]);
	let book = book_with("Gifts", "2026-11", &values);
	let gifts = baseline(&book, "2027-12-05", "Gifts");
	assert_eq!(gifts.seasonal_index, None);
	assert_eq!(gifts.forecast(ym("2027-12")).unwrap(), gifts.level);
}

#[test]
fn f_9_negative_months_are_used_as_is() {
	let book = book_with("Fun", "2026-11", &[-8000, 2000]);
	let fun = baseline(&book, "2027-01-15", "Fun");
	assert_eq!(fun.level, cents(-3000));
}

#[test]
fn twenty_five_months_average_both_windows_without_shrinkage() {
	let year: Vec<i64> = [8000].into_iter().chain([5000; 11]).collect();
	let mut values = vec![5000];
	values.extend(&year);
	values.extend(&year);
	let book = book_with("Gifts", "2025-11", &values);
	let gifts = baseline(&book, "2027-12-05", "Gifts");
	assert_eq!(gifts.history_months, 25);
	let index = gifts.seasonal_index.unwrap();
	assert_eq!((index[11], index[0]), (15238, 9524));
}

#[test]
fn twenty_five_months_with_a_sparse_previous_year_shrink_the_latest() {
	let mut values = vec![5000];
	values.extend([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 9000]);
	values.push(8000);
	values.extend([5000; 11]);
	let book = book_with("Gifts", "2025-11", &values);
	let index = baseline(&book, "2027-12-05", "Gifts")
		.seasonal_index
		.unwrap();
	assert_eq!(index[11], 12619);
}

#[test]
fn bill_linked_rows_are_known_flows_not_baseline() {
	let mut book = book_with("Electric", "2026-07", &[2000; 6]);
	for month in [
		"2026-07", "2026-08", "2026-09", "2026-10", "2026-11", "2026-12",
	] {
		book.spend("Electric", &format!("{month}-02"), -9000)
			.bill("electric");
	}
	let electric = baseline(&book, "2027-01-15", "Electric");
	assert_eq!(electric.level, cents(2000));
}

#[test]
fn month_to_date_leaves_the_remainder() {
	let mut book = book_with("Groceries", "2026-07", &[43000; 6]);
	book.spend("Groceries", "2027-01-10", -20000);
	book.spend("Groceries", "2027-01-20", -5000);
	let groceries = baseline(&book, "2027-01-15", "Groceries");
	assert_eq!(groceries.month_to_date, cents(20000));
	assert_eq!(groceries.remainder(ym("2027-01")).unwrap(), cents(23000));
}

#[test]
fn hidden_debt_and_scheduled_income_categories_get_no_baseline() {
	let mut book = book_with("Fun", "2026-07", &[1000; 6]);
	book.hide("Fun");
	let forecaster = Forecaster::new(&book.budget).unwrap();
	let paycheck = monthly_flows(
		"2027-02",
		1,
		"Paycheck",
		300000,
		veritas_forecast::FlowKind::Income,
	);
	let categories: Vec<_> = forecaster
		.baselines(date("2027-01-15"), &paycheck)
		.unwrap()
		.into_iter()
		.map(|baseline| baseline.category)
		.collect();
	assert!(!categories.contains(&cat("Fun")));
	assert!(!categories.contains(&cat("Loan principal")));
	assert!(!categories.contains(&cat("Paycheck")));
	assert!(categories.contains(&cat("Side gigs")));
}

#[test]
fn r_rel_7_a_month_too_large_to_sum_is_an_overflow_not_a_clamp() {
	let mut book = Book::household();
	book.spend("Groceries", "2026-12-01", -(i64::MAX / 2 + 1));
	book.row("VISA", "2026-12-02", -(i64::MAX / 2 + 1)).category = Some(cat("Groceries"));
	let forecaster = Forecaster::new(&book.budget).unwrap();
	assert!(matches!(
		forecaster.baselines(date("2027-01-15"), &[]),
		Err(veritas_domain::CoreError::Overflow)
	));
}

#[test]
fn cls_3_1_a_kind_conflict_baselines_by_actuals_flag() {
	let mut book = book_with("Fun", "2026-07", &[1000; 6]);
	book.kind("Fun", veritas_domain::CategoryKind::Income);
	let fun = baseline(&book, "2027-01-15", "Fun");
	assert_eq!((fun.side, fun.level), (Side::Spending, cents(1000)));
}
