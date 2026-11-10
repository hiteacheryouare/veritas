use veritas_domain::{AccountId, AccountKind, AccountMeta, CategoryKind, PayeeId};
use veritas_reports::RECONCILIATION_NOTE;

use super::{Book, RowExt};

pub const GROSS: i64 = 312_500;
pub const FEDERAL: i64 = 37_500;
pub const STATE: i64 = 15_600;
pub const FICA: i64 = 23_906;
pub const RETIREMENT: i64 = 18_750;
pub const NET_DEPOSIT: i64 = 216_744;

pub fn paycheck(book: &mut Book, id: &str, on: &str) {
	book.parent(id, "CHK", on, NET_DEPOSIT).payee = Some(PayeeId::new("Employer"));
	for (part, category, amount) in [
		("gross", "Paycheck", GROSS),
		("federal", "Federal tax", -FEDERAL),
		("state", "State tax", -STATE),
		("fica", "Social Security and Medicare", -FICA),
	] {
		book.row(&format!("{id}-{part}"), "CHK", on, amount)
			.in_category(category)
			.child_of(id)
			.payee = Some(PayeeId::new("Employer"));
	}
	book.transfer_with_mirror(
		&format!("{id}-401k"),
		&format!("{id}-401k-deposit"),
		"CHK",
		"401K",
		on,
		-RETIREMENT,
	)
	.in_category("Retirement")
	.child_of(id);
}

pub fn loan_payment(book: &mut Book, id: &str, on: &str, principal: i64, interest: i64) {
	book.parent(id, "CHK", on, -(principal + interest)).payee = Some(PayeeId::new("Servicer"));
	book.transfer_with_mirror(
		&format!("{id}-principal"),
		&format!("{id}-credited"),
		"CHK",
		"LOAN",
		on,
		-principal,
	)
	.in_category("Student loan principal")
	.child_of(id);
	book.row(&format!("{id}-interest"), "CHK", on, -interest)
		.in_category("Student loan interest")
		.child_of(id)
		.payee = Some(PayeeId::new("Servicer"));
}

pub fn spend(book: &mut Book, id: &str, account: &str, on: &str, category: &str, amount: i64) {
	book.row(id, account, on, amount).in_category(category);
}

pub fn household() -> Book {
	let mut book = Book::new();
	book.account("CHK", false)
		.account("VISA", false)
		.account("HYSA", true)
		.account("401K", true)
		.account("LOAN", true)
		.payee("Employer")
		.payee("Servicer")
		.payee("Bank")
		.payee("Starting Balance")
		.group("Income", true)
		.group("Taxes", false)
		.group("Bills", false)
		.group("Everyday", false)
		.group("Debt", false)
		.group("Saving", false)
		.group("Subscriptions", false)
		.group("Moving", false)
		.group("Car", false)
		.category("Starting Balances", "Income", None)
		.category("Paycheck", "Income", None)
		.category("Interest", "Income", None)
		.category("Federal tax", "Taxes", None)
		.category("State tax", "Taxes", None)
		.category("Social Security and Medicare", "Taxes", None)
		.category("Rent", "Bills", None)
		.category("Phone", "Bills", None)
		.category("Groceries", "Everyday", None)
		.category("Dining", "Everyday", None)
		.category("Coffee", "Everyday", None)
		.category("Clothing", "Everyday", None)
		.category("Gym", "Everyday", None)
		.category(
			"Student loan principal",
			"Debt",
			Some(CategoryKind::DebtPrincipal),
		)
		.category("Student loan interest", "Debt", None)
		.category("Savings", "Saving", Some(CategoryKind::Savings))
		.category("Retirement", "Saving", Some(CategoryKind::Savings))
		.category("Streaming", "Subscriptions", None)
		.category("Moving costs", "Moving", None)
		.category("Car repair", "Car", None);

	for (id, account, balance, category) in [
		("open-CHK", "CHK", 350_000, Some("Starting Balances")),
		("open-VISA", "VISA", -42_000, Some("Starting Balances")),
		("open-HYSA", "HYSA", 1_200_000, None),
		("open-401K", "401K", 3_100_000, None),
		("open-LOAN", "LOAN", -2_400_000, None),
	] {
		let row = book.row(id, account, "2026-08-01", balance);
		row.starting_balance = true;
		row.payee = Some(PayeeId::new("Starting Balance"));
		if let Some(category) = category {
			row.in_category(category);
		}
	}

	spend(&mut book, "aug-rent", "CHK", "2026-08-01", "Rent", -165_000);
	spend(
		&mut book,
		"aug-movers",
		"VISA",
		"2026-08-03",
		"Moving costs",
		-89_000,
	);
	spend(
		&mut book,
		"aug-coffee-1",
		"VISA",
		"2026-08-04",
		"Coffee",
		-650,
	);
	loan_payment(&mut book, "aug-loan", "2026-08-05", 19_500, 11_500);
	spend(
		&mut book,
		"aug-groceries-1",
		"VISA",
		"2026-08-06",
		"Groceries",
		-14_237,
	);
	spend(&mut book, "aug-gym", "CHK", "2026-08-08", "Gym", -4_000);
	spend(
		&mut book,
		"aug-streaming",
		"VISA",
		"2026-08-10",
		"Streaming",
		-1_599,
	);
	spend(
		&mut book,
		"aug-coffee-2",
		"VISA",
		"2026-08-12",
		"Coffee",
		-575,
	);
	paycheck(&mut book, "aug-pay-1", "2026-08-14");
	book.transfer("aug-save", "CHK", "HYSA", "2026-08-16", -50_000)
		.in_category("Savings");
	spend(
		&mut book,
		"aug-dining",
		"VISA",
		"2026-08-18",
		"Dining",
		-4_250,
	);
	book.transfer("aug-card", "CHK", "VISA", "2026-08-20", -42_000);
	spend(
		&mut book,
		"aug-groceries-2",
		"VISA",
		"2026-08-22",
		"Groceries",
		-11_863,
	);
	spend(
		&mut book,
		"aug-phone",
		"VISA",
		"2026-08-25",
		"Phone",
		-6_500,
	);
	paycheck(&mut book, "aug-pay-2", "2026-08-31");
	book.row("aug-hysa-interest", "HYSA", "2026-08-31", 4_100)
		.payee = Some(PayeeId::new("Bank"));
	book.row("aug-chk-interest", "CHK", "2026-08-31", 118)
		.in_category("Interest")
		.payee = Some(PayeeId::new("Bank"));

	spend(&mut book, "sep-rent", "CHK", "2026-09-01", "Rent", -165_000);
	spend(
		&mut book,
		"sep-groceries-1",
		"VISA",
		"2026-09-04",
		"Groceries",
		-12_450,
	);
	loan_payment(&mut book, "sep-loan", "2026-09-05", 19_593, 11_407);
	spend(
		&mut book,
		"sep-jacket-and-shoes",
		"VISA",
		"2026-09-05",
		"Clothing",
		-12_000,
	);
	spend(&mut book, "sep-gym", "CHK", "2026-09-08", "Gym", -4_000);
	spend(
		&mut book,
		"sep-streaming",
		"VISA",
		"2026-09-10",
		"Streaming",
		-1_599,
	);
	paycheck(&mut book, "sep-pay-1", "2026-09-15");
	book.transfer("sep-save", "CHK", "HYSA", "2026-09-16", -50_000)
		.in_category("Savings");
	spend(
		&mut book,
		"sep-dining",
		"VISA",
		"2026-09-19",
		"Dining",
		-6_820,
	);
	book.transfer("sep-card", "CHK", "VISA", "2026-09-20", -128_674);
	spend(
		&mut book,
		"sep-shoes-returned",
		"VISA",
		"2026-09-20",
		"Clothing",
		4_500,
	);
	spend(
		&mut book,
		"sep-groceries-2",
		"VISA",
		"2026-09-23",
		"Groceries",
		-13_115,
	);
	spend(
		&mut book,
		"sep-phone",
		"VISA",
		"2026-09-25",
		"Phone",
		-6_500,
	);
	paycheck(&mut book, "sep-pay-2", "2026-09-30");
	book.row("sep-hysa-interest", "HYSA", "2026-09-30", 4_350)
		.payee = Some(PayeeId::new("Bank"));
	book.row("sep-chk-interest", "CHK", "2026-09-30", 131)
		.in_category("Interest")
		.payee = Some(PayeeId::new("Bank"));
	book.row("sep-reconcile", "CHK", "2026-09-30", -1_234).notes =
		Some(RECONCILIATION_NOTE.to_owned());

	spend(&mut book, "oct-rent", "CHK", "2026-10-01", "Rent", -165_000);
	spend(
		&mut book,
		"oct-jacket-returned",
		"VISA",
		"2026-10-03",
		"Clothing",
		7_500,
	);
	loan_payment(&mut book, "oct-loan", "2026-10-05", 19_687, 11_313);
	spend(
		&mut book,
		"oct-groceries",
		"VISA",
		"2026-10-07",
		"Groceries",
		-15_022,
	);
	spend(
		&mut book,
		"oct-brakes",
		"VISA",
		"2026-10-09",
		"Car repair",
		-61_480,
	);
	book.transfer("oct-from-savings", "CHK", "HYSA", "2026-10-12", 30_000)
		.in_category("Savings");
	paycheck(&mut book, "oct-pay-1", "2026-10-15");
	book.transfer("oct-save", "CHK", "HYSA", "2026-10-16", -50_000)
		.in_category("Savings");
	book.transfer("oct-card", "CHK", "VISA", "2026-10-20", -47_984);
	spend(
		&mut book,
		"oct-dining",
		"VISA",
		"2026-10-24",
		"Dining",
		-5_310,
	);
	spend(
		&mut book,
		"oct-phone",
		"VISA",
		"2026-10-25",
		"Phone",
		-6_500,
	);
	paycheck(&mut book, "oct-pay-2", "2026-10-30");
	book.row("oct-hysa-interest", "HYSA", "2026-10-31", 4_290)
		.payee = Some(PayeeId::new("Bank"));
	book.row("oct-chk-interest", "CHK", "2026-10-31", 97)
		.in_category("Interest")
		.payee = Some(PayeeId::new("Bank"));

	book.delete_category_into("Coffee", "Dining")
		.hide_category("Gym")
		.archive("Streaming", "2026-09-30")
		.hide_group("Moving");
	book
}

fn car_loan_payment(book: &mut Book, id: &str, on: &str, principal: i64, interest: i64) {
	book.parent(id, "CHK", on, -(principal + interest)).payee = Some(PayeeId::new("Auto lender"));
	book.transfer_with_mirror(
		&format!("{id}-principal"),
		&format!("{id}-credited"),
		"CHK",
		"AUTO",
		on,
		-principal,
	)
	.in_category("Car loan principal")
	.child_of(id);
	book.row(&format!("{id}-interest"), "CHK", on, -interest)
		.in_category("Car loan interest")
		.child_of(id)
		.payee = Some(PayeeId::new("Auto lender"));
}

/// The household carried through November 2026 to January 2027: a car loan
/// paid off with a $5 overpayment (an asset for a month, then refunded and
/// closed), a brokerage revalued each month end, an old savings account
/// closed with $10 left in it, a credit union account opened with a
/// categorized starting balance, a card payment and a savings transfer whose
/// legs post on either side of a month end, and a bank fee booked through
/// Veritas's reconcile flow.
pub fn through_year_end() -> Book {
	let mut book = household();
	book.account("AUTO", true)
		.account("BRK", true)
		.account("OLDSAV", true)
		.account("CU", false)
		.payee("Auto lender")
		.group("Gifts", false)
		.category(
			"Car loan principal",
			"Debt",
			Some(CategoryKind::DebtPrincipal),
		)
		.category("Car loan interest", "Debt", None)
		.category("Gifts", "Gifts", None);
	book.budget.account_meta.insert(
		AccountId::new("AUTO"),
		AccountMeta {
			kind: Some(AccountKind::Liability),
			..AccountMeta::default()
		},
	);
	for account in ["AUTO", "OLDSAV"] {
		book.budget
			.accounts
			.get_mut(&AccountId::new(account))
			.unwrap()
			.closed = true;
	}
	for (id, account, on, balance, category) in [
		("open-AUTO", "AUTO", "2026-11-01", -60_000, None),
		("open-BRK", "BRK", "2026-11-01", 800_000, None),
		("open-OLDSAV", "OLDSAV", "2026-11-01", 15_000, None),
		(
			"open-CU",
			"CU",
			"2026-12-01",
			25_000,
			Some("Starting Balances"),
		),
	] {
		let row = book.row(id, account, on, balance);
		row.starting_balance = true;
		row.payee = Some(PayeeId::new("Starting Balance"));
		if let Some(category) = category {
			row.in_category(category);
		}
	}

	spend(&mut book, "nov-rent", "CHK", "2026-11-01", "Rent", -165_000);
	loan_payment(&mut book, "nov-loan", "2026-11-05", 19_782, 11_218);
	car_loan_payment(&mut book, "nov-car", "2026-11-05", 30_000, 400);
	spend(
		&mut book,
		"nov-groceries",
		"VISA",
		"2026-11-08",
		"Groceries",
		-14_820,
	);
	paycheck(&mut book, "nov-pay-1", "2026-11-14");
	book.transfer("nov-save", "CHK", "HYSA", "2026-11-16", -50_000)
		.in_category("Savings");
	book.transfer("nov-close-oldsav", "CHK", "OLDSAV", "2026-11-20", 14_000)
		.in_category("Savings");
	spend(
		&mut book,
		"nov-dining",
		"VISA",
		"2026-11-22",
		"Dining",
		-7_215,
	);
	spend(
		&mut book,
		"nov-phone",
		"VISA",
		"2026-11-25",
		"Phone",
		-6_500,
	);
	paycheck(&mut book, "nov-pay-2", "2026-11-30");
	book.transfer("nov-card", "CHK", "VISA", "2026-11-30", -80_812);
	book.get("nov-cardm").date = super::date("2026-12-01");
	book.row("nov-hysa-interest", "HYSA", "2026-11-30", 4_180)
		.payee = Some(PayeeId::new("Bank"));
	book.row("nov-brk-value", "BRK", "2026-11-30", 12_345);
	book.row("nov-chk-interest", "CHK", "2026-11-30", 105)
		.in_category("Interest")
		.payee = Some(PayeeId::new("Bank"));

	spend(&mut book, "dec-rent", "CHK", "2026-12-01", "Rent", -165_000);
	loan_payment(&mut book, "dec-loan", "2026-12-05", 19_877, 11_123);
	car_loan_payment(&mut book, "dec-car-payoff", "2026-12-05", 30_500, 120);
	spend(
		&mut book,
		"dec-groceries",
		"VISA",
		"2026-12-10",
		"Groceries",
		-21_404,
	);
	spend(
		&mut book,
		"dec-gifts",
		"VISA",
		"2026-12-12",
		"Gifts",
		-35_000,
	);
	paycheck(&mut book, "dec-pay-1", "2026-12-15");
	book.transfer("dec-save", "CHK", "HYSA", "2026-12-16", -50_000)
		.in_category("Savings");
	book.transfer("dec-card", "CHK", "VISA", "2026-12-20", -28_535);
	paycheck(&mut book, "dec-pay-2", "2026-12-31");
	book.transfer("dec-year-end-save", "CHK", "HYSA", "2026-12-31", -50_000)
		.in_category("Savings");
	book.get("dec-year-end-savem").date = super::date("2027-01-04");
	book.row("dec-hysa-interest", "HYSA", "2026-12-31", 4_310)
		.payee = Some(PayeeId::new("Bank"));
	book.row("dec-brk-value", "BRK", "2026-12-31", -8_210);
	book.row("dec-chk-interest", "CHK", "2026-12-31", 142)
		.in_category("Interest")
		.payee = Some(PayeeId::new("Bank"));

	spend(&mut book, "jan-rent", "CHK", "2027-01-01", "Rent", -165_000);
	loan_payment(&mut book, "jan-loan", "2027-01-05", 19_972, 11_028);
	book.transfer("jan-car-refund", "CHK", "AUTO", "2027-01-08", 500)
		.in_category("Car loan principal");
	spend(
		&mut book,
		"jan-groceries",
		"VISA",
		"2027-01-09",
		"Groceries",
		-13_377,
	);
	paycheck(&mut book, "jan-pay-1", "2027-01-15");
	book.transfer("jan-save", "CHK", "HYSA", "2027-01-16", -50_000)
		.in_category("Savings");
	book.transfer("jan-card", "CHK", "VISA", "2027-01-20", -56_404);
	paycheck(&mut book, "jan-pay-2", "2027-01-29");
	book.row("jan-hysa-interest", "HYSA", "2027-01-31", 4_420)
		.payee = Some(PayeeId::new("Bank"));
	book.row("jan-brk-value", "BRK", "2027-01-31", 20_000);
	book.row("jan-chk-interest", "CHK", "2027-01-31", 133)
		.in_category("Interest")
		.payee = Some(PayeeId::new("Bank"));
	book.row("jan-cu-fee", "CU", "2027-01-31", -75);
	book.mark_adjustment("jan-cu-fee");
	book
}
