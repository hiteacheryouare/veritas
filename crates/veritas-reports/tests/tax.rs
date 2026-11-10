//! R-REP-8: tax buckets per docs/finance/tax-buckets.md, worked examples T-1
//! to T-15, the TAX-5 CSV files and the TAX-6 invariants.

#![allow(clippy::unwrap_used)]

mod support;

use support::*;
use veritas_domain::{CategoryKind, CategoryMeta, PayeeId};
use veritas_reports::{Bucket, Source, TaxSummary, detail_csv, summary_csv, tax_summary};

fn bucket_of(book: &mut Book, category: &str, bucket: &str) {
	let meta = book
		.budget
		.category_meta
		.entry(cat(category))
		.or_insert(CategoryMeta {
			kind: None,
			tax_bucket: None,
			archived_on: None,
		});
	meta.tax_bucket = Some(bucket.to_owned());
}

fn tag(book: &mut Book, tag: &str, bucket: &str) {
	book.budget
		.tax_tags
		.insert(tag.to_owned(), bucket.to_owned());
}

fn summary(book: &Book, year: i32) -> TaxSummary {
	let summary = tax_summary(&book.budget, &book.classify(), year).unwrap();
	for total in &summary.buckets {
		let detail: i64 = summary
			.detail
			.iter()
			.filter(|row| row.bucket == Some(total.bucket))
			.map(|row| row.counted.unwrap().minor())
			.sum();
		assert_eq!(total.total.minor(), detail, "TAX-6 invariant 1");
	}
	summary
}

fn total(summary: &TaxSummary, bucket: Bucket) -> Option<(i64, usize)> {
	summary
		.bucket(bucket)
		.map(|total| (total.total.minor(), total.rows.len()))
}

fn book() -> Book {
	let mut book = master_book();
	book.group("Giving", false)
		.category("Donations", "Giving", None)
		.category("Interest", "Income", None)
		.group("Business", false)
		.category("Supplies", "Business", None)
		.category("Freelance", "Income", None);
	bucket_of(&mut book, "Donations", "charitable");
	tag(&mut book, "medical", "medical");
	tag(&mut book, "rx", "medical");
	tag(&mut book, "education", "education");
	tag(&mut book, "notax", "none");
	tag(&mut book, "hsa", "hsa");
	book
}

fn spend(book: &mut Book, id: &str, on: &str, amount: i64, category: &str, notes: Option<&str>) {
	let row = book.row(id, "CHK", on, amount);
	row.category = Some(cat(category));
	row.notes = notes.map(str::to_owned);
}

#[test]
fn tax7_t1_category_bucket_with_a_refund() {
	let mut book = book();
	spend(&mut book, "a", "2027-02-01", -10_000, "Donations", None);
	spend(&mut book, "b", "2027-03-01", -5_000, "Donations", None);
	spend(&mut book, "c", "2027-04-01", 2_000, "Donations", None);
	let summary = summary(&book, 2027);
	assert_eq!(total(&summary, Bucket::Charitable), Some((13_000, 3)));
	assert!(
		summary
			.detail
			.iter()
			.all(|row| row.source == Source::Category)
	);
}

#[test]
fn tax7_t2_tag_beats_the_category() {
	let mut book = book();
	spend(
		&mut book,
		"rx",
		"2027-02-01",
		-3_000,
		"Groceries",
		Some("pharmacy #medical"),
	);
	let summary = summary(&book, 2027);
	assert_eq!(total(&summary, Bucket::Medical), Some((3_000, 1)));
	assert_eq!(summary.detail[0].source, Source::Tag);
}

#[test]
fn tax7_t3_a_none_tag_removes_the_categorys_bucket() {
	let mut book = book();
	spend(
		&mut book,
		"g",
		"2027-02-01",
		-4_000,
		"Donations",
		Some("#notax"),
	);
	let summary = summary(&book, 2027);
	assert_eq!(total(&summary, Bucket::Charitable), None);
	assert!(summary.detail.is_empty());
}

#[test]
fn tax7_t4_two_buckets_is_a_conflict() {
	let mut book = book();
	spend(
		&mut book,
		"x",
		"2027-02-01",
		-2_500,
		"Groceries",
		Some("#medical #education"),
	);
	let summary = summary(&book, 2027);
	assert!(summary.buckets.is_empty());
	let conflicts: Vec<_> = summary.conflicts().collect();
	assert_eq!(conflicts.len(), 1);
	assert_eq!(conflicts[0].counted, None);
	assert_eq!(conflicts[0].bucket_id(), "conflict");
}

#[test]
fn tax7_t5_the_same_bucket_twice_counts_once() {
	let mut book = book();
	spend(
		&mut book,
		"x",
		"2027-02-01",
		-1_000,
		"Groceries",
		Some("#medical #rx"),
	);
	assert_eq!(
		total(&summary(&book, 2027), Bucket::Medical),
		Some((1_000, 1))
	);
}

#[test]
fn tax7_t6_hsa_transfer_counts_once_and_never_its_mirror() {
	let mut book = book();
	book.account("HSA", true);
	book.category("HSA contributions", "Saving", Some(CategoryKind::Savings));
	book.transfer("h", "CHK", "HSA", "2027-02-01", -20_000)
		.in_category("HSA contributions")
		.notes = Some("#hsa".to_owned());
	book.get("hm").notes = Some("#hsa".to_owned());
	let summary = summary(&book, 2027);
	assert_eq!(total(&summary, Bucket::Hsa), Some((20_000, 1)));
	assert!(summary.detail.iter().all(|row| row.row.as_str() != "hm"));
}

#[test]
fn tax7_t7_interest_income() {
	let mut book = book();
	bucket_of(&mut book, "Interest", "interest_income");
	spend(&mut book, "i", "2027-02-01", 1_234, "Interest", None);
	assert_eq!(
		total(&summary(&book, 2027), Bucket::InterestIncome),
		Some((1_234, 1))
	);
}

#[test]
fn tax7_t8_the_tax_year_is_the_calendar_year() {
	let mut book = book();
	spend(&mut book, "a", "2027-12-31", -5_000, "Donations", None);
	spend(&mut book, "b", "2028-01-01", -7_000, "Donations", None);
	assert_eq!(
		total(&summary(&book, 2027), Bucket::Charitable),
		Some((5_000, 1))
	);
	assert_eq!(
		total(&summary(&book, 2028), Bucket::Charitable),
		Some((7_000, 1))
	);
}

#[test]
fn tax7_t9_split_children_count_on_their_own_and_parent_tags_do_not_flow_down() {
	let mut book = book();
	book.parent("p", "CHK", "2027-02-01", -10_000).notes = Some("#medical".to_owned());
	book.row("p1", "CHK", "2027-02-01", -6_000)
		.in_category("Donations")
		.child_of("p");
	book.row("p2", "CHK", "2027-02-01", -4_000)
		.in_category("Groceries")
		.child_of("p");
	let summary = summary(&book, 2027);
	assert_eq!(total(&summary, Bucket::Charitable), Some((6_000, 1)));
	assert_eq!(total(&summary, Bucket::Medical), None);
}

#[test]
fn tax7_t10_a_negative_total_is_never_clamped() {
	let mut book = book();
	spend(&mut book, "r", "2027-02-01", 2_000, "Donations", None);
	assert_eq!(
		total(&summary(&book, 2027), Bucket::Charitable),
		Some((-2_000, 1))
	);
}

#[test]
fn tax7_t11_starting_balances_and_adjustments_are_never_counted() {
	let mut book = book();
	book.account("NEW", false);
	book.row("sb", "NEW", "2027-02-01", 50_000).notes = Some("#medical".to_owned());
	book.get("sb").starting_balance = true;
	book.row("rev", "BRK", "2027-02-01", 900).notes = Some("#medical".to_owned());
	book.transfer("card", "CHK", "VISA", "2027-02-02", -500)
		.notes = Some("#medical".to_owned());
	assert!(summary(&book, 2027).detail.is_empty());
}

#[test]
fn tax7_t13_tag_grammar_matches_actual() {
	let mut book = book();
	tag(&mut book, "medical", "medical");
	book.budget.tax_tags.remove("rx");
	spend(
		&mut book,
		"a",
		"2027-02-01",
		-1_000,
		"Groceries",
		Some("#medical#rx"),
	);
	spend(
		&mut book,
		"b",
		"2027-02-02",
		-1_000,
		"Groceries",
		Some("##medical"),
	);
	let summary = summary(&book, 2027);
	assert_eq!(total(&summary, Bucket::Medical), Some((1_000, 1)));
	assert_eq!(summary.detail[0].row.as_str(), "a");
}

#[test]
fn tax7_t14_none_beats_other_tags_and_is_not_a_conflict() {
	let mut book = book();
	spend(
		&mut book,
		"x",
		"2027-02-01",
		-1_000,
		"Groceries",
		Some("#medical #notax"),
	);
	let summary = summary(&book, 2027);
	assert!(summary.detail.is_empty());
}

#[test]
fn tax7_t15_gross_paycheck() {
	let mut book = book();
	book.account("401k", true).account("HSA", true);
	book.group("Taxes and deductions", false);
	for category in ["Federal tax", "State tax", "FICA", "Health premium"] {
		book.category(category, "Taxes and deductions", None);
	}
	for category in ["Federal tax", "State tax", "FICA"] {
		bucket_of(&mut book, category, "taxes_withheld");
	}
	bucket_of(&mut book, "Paycheck", "w2_income");
	book.category("Retirement", "Saving", Some(CategoryKind::Savings));
	bucket_of(&mut book, "Retirement", "retirement_contribution");
	book.category("HSA contributions", "Saving", Some(CategoryKind::Savings));
	bucket_of(&mut book, "HSA contributions", "hsa");
	book.parent("Pay", "CHK", "2027-01-15", 234_400);
	for (id, amount, category) in [
		("Gross", 400_000, "Paycheck"),
		("Fed", -50_000, "Federal tax"),
		("State", -20_000, "State tax"),
		("FICA", -30_600, "FICA"),
		("Premium", -15_000, "Health premium"),
	] {
		book.row(id, "CHK", "2027-01-15", amount)
			.in_category(category)
			.child_of("Pay");
	}
	book.transfer("Ret", "CHK", "401k", "2027-01-15", -40_000)
		.in_category("Retirement")
		.child_of("Pay");
	book.transfer("Hsa", "CHK", "HSA", "2027-01-15", -10_000)
		.in_category("HSA contributions")
		.child_of("Pay");
	let summary = summary(&book, 2027);
	assert_eq!(total(&summary, Bucket::W2Income), Some((400_000, 1)));
	assert_eq!(total(&summary, Bucket::TaxesWithheld), Some((100_600, 3)));
	assert_eq!(
		total(&summary, Bucket::RetirementContribution),
		Some((40_000, 1))
	);
	assert_eq!(total(&summary, Bucket::Hsa), Some((10_000, 1)));
	assert_eq!(
		summary.detail.len(),
		6,
		"the premium and both mirrors are absent"
	);
}

#[test]
fn tax7_t12_business_income_and_expense_never_net() {
	let mut book = book();
	bucket_of(&mut book, "Freelance", "business_income");
	bucket_of(&mut book, "Supplies", "business_expense");
	spend(&mut book, "f", "2027-02-01", 80_000, "Freelance", None);
	spend(&mut book, "s", "2027-02-02", -15_000, "Supplies", None);
	spend(&mut book, "r", "2027-02-03", 1_000, "Supplies", None);
	let summary = summary(&book, 2027);
	assert_eq!(total(&summary, Bucket::BusinessIncome), Some((80_000, 1)));
	assert_eq!(total(&summary, Bucket::BusinessExpense), Some((14_000, 2)));
}

#[test]
fn tax6_invariant_3_reclassifying_a_category_kind_keeps_totals() {
	let mut book = book();
	book.account("HSA", true);
	book.category("Health savings", "Saving", None);
	bucket_of(&mut book, "Health savings", "hsa");
	book.transfer("h", "CHK", "HSA", "2027-02-01", -20_000)
		.in_category("Health savings");
	let before = total(&summary(&book, 2027), Bucket::Hsa);
	book.budget
		.category_meta
		.get_mut(&cat("Health savings"))
		.unwrap()
		.kind = Some(CategoryKind::Savings);
	assert_eq!(total(&summary(&book, 2027), Bucket::Hsa), before);
	assert_eq!(before, Some((20_000, 1)));
}

#[test]
fn tax5_summary_and_detail_csv() {
	let mut book = book();
	bucket_of(&mut book, "Interest", "interest_income");
	spend(
		&mut book,
		"a",
		"2027-02-01",
		-10_000,
		"Donations",
		Some("=SUM(A1), \"gift\""),
	);
	spend(&mut book, "b", "2027-03-01", 2_050, "Donations", None);
	spend(
		&mut book,
		"c",
		"2027-04-01",
		1_234,
		"Interest",
		Some("@bank"),
	);
	book.get("a").payee = Some(PayeeId::new("Lender"));
	book.transfer("t", "CHK", "SAV", "2027-05-01", -500)
		.in_category("Groceries")
		.notes = Some("#medical#education".to_owned());
	let summary = summary(&book, 2027);
	assert_eq!(
		summary_csv(&summary),
		"tax_year,bucket,direction,total_cents,total,transaction_count\n\
		 2027,charitable,outflow,7950,79.50,2\n\
		 2027,interest_income,inflow,1234,12.34,1\n"
	);
	assert_eq!(
		detail_csv(&book.budget, &summary),
		"tax_year,bucket,source,date,account,payee,category,amount_cents,counted_cents,notes,transaction_id,parent_id\n\
		 2027,charitable,category,2027-02-01,CHK,Lender,Donations,-10000,10000,\"'=SUM(A1), \"\"gift\"\"\",a,\n\
		 2027,charitable,category,2027-03-01,CHK,,Donations,2050,-2050,,b,\n\
		 2027,conflict,tag,2027-05-01,CHK,SAV,Groceries,-500,,#medical#education,t,\n\
		 2027,interest_income,category,2027-04-01,CHK,,Interest,1234,1234,'@bank,c,\n"
	);
	assert!(!summary_csv(&summary).starts_with('\u{FEFF}'));
	assert_eq!(
		summary
			.detail
			.iter()
			.filter(|row| row.counted.is_some())
			.count(),
		3
	);
}
