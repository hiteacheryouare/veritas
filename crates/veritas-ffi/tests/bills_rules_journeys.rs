#![allow(
	clippy::unwrap_used,
	clippy::expect_used,
	clippy::panic,
	reason = "integration tests fail by panicking"
)]

//! Bills and the daily schedule service (R-BILL-1 to R-BILL-5), and a rules
//! session in the sentence editor (R-RULE-2, R-RULE-3), driven through the
//! exported API over several days, with Actual's own service, rule search and
//! reading of the same file as the verdict wherever the oracle is installed.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::Thread;

use serde_json::{Value, json};
use veritas_ffi::{
	BillAmount, BillDate, BillDraft, BillFrequency, BillRepeat, BillRepeatEnd, BillState,
	BillStatus, BillWeekend, CancelToken, Change, Command, EntityKind, FfiError, RuleAccess,
	RuleAction, RuleActionOp, RuleCondition, RuleConditionOp, RuleConditionsOp, RuleDraft,
	RuleField, RuleSource, RuleStage, RuleValue, Session, TransactionDraft,
};

const CHECKING: &str = "79664b3b-072e-442d-9082-0c1e02c242c0";
const SAVINGS_TRANSFER: &str = "6f3425f0-a50b-4997-a827-f445037e53f9";
const LANDLORD: &str = "8adebb90-d2c2-481b-8403-3d35ae4fa1b9";
const CITY_POWER: &str = "91c8df64-c2b7-4d28-a462-f481a46e1cc2";
const ACME: &str = "9d6b94a1-40db-4e98-adef-909218936e39";
const RENT: &str = "6a2a7900-aeac-482b-a592-ce709c1f4b4b";
const UTILITIES: &str = "e4b52b78-abc8-4d7d-9d50-a6ed7900e41c";
const INCOME: &str = "3c1699a5-522a-435e-86dc-93d900a14f0e";
const CORNER_GROCER: &str = "836cf416-110a-4fdd-8190-abff53421c2c";
const GROCERIES: &str = "3eb3739d-9384-490c-8e5f-73a396bf02e4";
const DINING_OUT: &str = "4668e3ba-d3bd-4b9d-9e21-1e232be57c69";

struct Unpark(Thread);

impl Wake for Unpark {
	fn wake(self: Arc<Self>) {
		self.0.unpark();
	}
}

fn block_on<F: Future>(future: F) -> F::Output {
	let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
	let mut context = Context::from_waker(&waker);
	let mut future = pin!(future);
	loop {
		if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
			return output;
		}
		std::thread::park();
	}
}

struct Scratch(PathBuf);

impl Scratch {
	fn new(label: &str) -> Self {
		static NEXT: AtomicUsize = AtomicUsize::new(0);
		let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
			"ffi-bills-{label}-{}-{}",
			std::process::id(),
			NEXT.fetch_add(1, Ordering::Relaxed)
		));
		let _ = std::fs::remove_dir_all(&path);
		std::fs::create_dir_all(&path).unwrap();
		Scratch(path)
	}

	fn join(&self, name: &str) -> PathBuf {
		self.0.join(name)
	}
}

impl Drop for Scratch {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.0);
	}
}

fn small_zip() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/small.zip")
}

fn text(path: &Path) -> String {
	path.display().to_string()
}

fn import(scratch: &Scratch, zip: &Path, name: &str) -> Arc<Session> {
	block_on(Session::import_actual_zip(
		text(zip),
		text(&scratch.join(name)),
		CancelToken::new(),
	))
	.unwrap()
}

fn reopen(scratch: &Scratch, name: &str) -> Arc<Session> {
	block_on(Session::open(text(&scratch.join(name)), CancelToken::new())).unwrap()
}

fn export(session: &Session, zip: &Path) {
	block_on(session.export_actual_zip(text(zip), CancelToken::new())).unwrap();
}

/// The day the shell last read the budget as of, which commands plan against.
fn see(session: &Session, day: &str) {
	let month = veritas_ffi::parse_month(day[..7].to_owned()).unwrap();
	session.budget_month(month, day.to_owned()).unwrap();
}

fn perform(session: &Session, command: Command) -> Change {
	session.perform(command).unwrap()
}

fn created(change: &Change, kind: EntityKind) -> String {
	change
		.entities
		.iter()
		.find(|e| e.kind == kind && e.id.is_some())
		.and_then(|e| e.id.clone())
		.unwrap()
}

fn last_run(scratch: &Scratch, name: &str) -> Option<String> {
	let text = std::fs::read_to_string(scratch.join(name).join("metadata.json")).unwrap();
	let value: Value = serde_json::from_str(&text).unwrap();
	value["lastScheduleRun"].as_str().map(str::to_owned)
}

fn sqlite(scratch: &Scratch, name: &str) -> rusqlite::Connection {
	rusqlite::Connection::open(scratch.join(name).join("db.sqlite")).unwrap()
}

fn monthly(start: &str, weekend: Option<BillWeekend>) -> BillDate {
	repeat(
		start,
		1,
		BillFrequency::Monthly,
		weekend,
		BillRepeatEnd::Never,
	)
}

fn repeat(
	start: &str,
	interval: u32,
	frequency: BillFrequency,
	weekend: Option<BillWeekend>,
	end: BillRepeatEnd,
) -> BillDate {
	BillDate::Repeat {
		repeat: BillRepeat {
			start: start.to_owned(),
			interval,
			frequency,
			patterns: Vec::new(),
			weekend,
			end,
		},
	}
}

fn bill(
	name: &str,
	payee: &str,
	amount: BillAmount,
	date: BillDate,
	category: Option<&str>,
	auto_post: bool,
) -> BillDraft {
	BillDraft {
		name: Some(name.to_owned()),
		payee_id: Some(payee.to_owned()),
		account_id: Some(CHECKING.to_owned()),
		amount,
		date,
		category_id: category.map(str::to_owned),
		auto_post,
	}
}

fn create(session: &Session, id: &str, draft: BillDraft) {
	perform(
		session,
		Command::CreateBill {
			bill_id: Some(id.to_owned()),
			bill: draft,
		},
	);
}

/// A household's recurring money, set up on 31 October.
fn household_bills(session: &Session) {
	// 1 November is a Sunday, so rent posts on Monday the 2nd.
	create(
		session,
		"rent",
		bill(
			"Rent",
			LANDLORD,
			BillAmount::Exact { amount: -150_000 },
			monthly("2026-11-01", Some(BillWeekend::MondayAfter)),
			Some(RENT),
			true,
		),
	);
	// Confirm-first: the service never posts it.
	create(
		session,
		"power",
		bill(
			"Power",
			CITY_POWER,
			BillAmount::About { amount: -8_000 },
			monthly("2026-11-15", Some(BillWeekend::FridayBefore)),
			Some(UTILITIES),
			false,
		),
	);
	// Income every other Friday (R-BILL-5).
	create(
		session,
		"paycheck",
		bill(
			"Paycheck",
			ACME,
			BillAmount::Exact { amount: 250_000 },
			repeat(
				"2026-11-06",
				2,
				BillFrequency::Weekly,
				None,
				BillRepeatEnd::Never,
			),
			Some(INCOME),
			true,
		),
	);
	// A transfer to savings, which posts its other leg too.
	create(
		session,
		"savings",
		bill(
			"Savings",
			SAVINGS_TRANSFER,
			BillAmount::Exact { amount: -20_000 },
			monthly("2026-11-20", None),
			None,
			true,
		),
	);
	// A one-off membership fee that ends after its first charge.
	create(
		session,
		"gym",
		bill(
			"Gym joining fee",
			ACME,
			BillAmount::Exact { amount: -4_000 },
			repeat(
				"2026-11-10",
				1,
				BillFrequency::Monthly,
				None,
				BillRepeatEnd::AfterOccurrences { count: 1 },
			),
			None,
			true,
		),
	);
}

/// small.zip with the household's bills, exported as the base every
/// timeline starts from.
fn household_base(scratch: &Scratch) -> PathBuf {
	let session = import(scratch, &small_zip(), "setup");
	see(&session, "2026-10-31");
	household_bills(&session);
	let base = scratch.join("base.zip");
	export(&session, &base);
	session.close().unwrap();
	base
}

const DAYS: [&str; 9] = [
	"2026-11-01",
	"2026-11-02",
	"2026-11-06",
	"2026-11-10",
	"2026-11-13",
	"2026-11-20",
	"2026-11-21",
	"2026-12-01",
	"2026-12-02",
];

fn oracle() -> Option<PathBuf> {
	let oracle = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/actual-oracle");
	if oracle.join("node_modules").exists() {
		return Some(oracle);
	}
	assert!(
		std::env::var_os("VERITAS_REQUIRE_ORACLE").is_none(),
		"VERITAS_REQUIRE_ORACLE is set but tools/actual-oracle/node_modules is missing"
	);
	eprintln!("skipping the Actual side: run `cargo xtask oracle setup`");
	None
}

fn node(oracle: &Path, args: &[&str]) -> Vec<u8> {
	let output = std::process::Command::new("node")
		.current_dir(oracle)
		.arg("src/main.ts")
		.args(args)
		.output()
		.expect("node runs");
	assert!(
		output.status.success(),
		"the oracle failed on {args:?}: {}",
		String::from_utf8_lossy(&output.stderr)
	);
	output.stdout
}

/// Actual imports and loads `zip` as of `today` and dumps its tables and its
/// own computed schedules.
fn actual_reads(oracle: &Path, zip: &Path, today: &str) -> Value {
	serde_json::from_slice(&node(oracle, &["dump", &text(zip), "--today", today])).unwrap()
}

/// Runs Actual's own handlers for `ops` on `zip` as of `today`. Chained runs
/// need distinct seeds, or Actual hands out the same ids again.
fn actual_apply(oracle: &Path, zip: &Path, ops: Value, out: &Path, today: &str, seed: u32) {
	let file = out.with_extension("ops.json");
	std::fs::write(&file, json!({"version": 1, "ops": ops}).to_string()).unwrap();
	node(
		oracle,
		&[
			"apply",
			&text(zip),
			&text(&file),
			"--out",
			&text(out),
			"--today",
			today,
			"--seed",
			&seed.to_string(),
		],
	);
}

fn rows<'a>(dump: &'a Value, table: &str) -> &'a Vec<Value> {
	dump["tables"][table]["rows"].as_array().unwrap()
}

/// Every live transaction a bill made, as Actual stores it, without ids and
/// sort orders. A transfer's other leg is paired by its schedule's leg.
fn posted(dump: &Value) -> Vec<Value> {
	let transactions = rows(dump, "transactions");
	let mut posted: Vec<Value> = transactions
		.iter()
		.filter(|row| row["tombstone"] != 1)
		.filter_map(|row| {
			let schedule = row["schedule"].as_str().map(str::to_owned).or_else(|| {
				transactions
					.iter()
					.find(|other| other["id"] == row["transferred_id"])
					.and_then(|other| other["schedule"].as_str())
					.map(|s| format!("{s} (other leg)"))
			})?;
			Some(json!({
				"schedule": schedule,
				"acct": row["acct"],
				"date": row["date"],
				"amount": row["amount"],
				"payee": row["description"],
				"category": row["category"],
				"notes": row["notes"],
				"cleared": row["cleared"],
				"isParent": row["isParent"],
				"isChild": row["isChild"],
				"transfer": row["transferred_id"].is_string(),
			}))
		})
		.collect();
	posted.sort_by_key(|row| row.to_string());
	posted
}

fn count(dump: &Value, schedule: &str) -> usize {
	posted(dump)
		.iter()
		.filter(|row| row["schedule"] == schedule)
		.count()
}

fn schedules(dump: &Value) -> Value {
	dump["computed"]["schedules"].clone()
}

#[test]
fn a_month_of_bills_posts_the_same_rows_whether_veritas_actual_or_both_open_the_budget() {
	// R-BILL-1, R-BILL-2, R-BILL-5, §10: a month of auto-post bills, with the
	// service run on the days the budget is opened. Veritas alone (with an
	// undo and a reopen along the way), Actual alone, and the two taking
	// turns all end with each occurrence posted exactly once.
	let scratch = Scratch::new("month");
	let base = household_base(&scratch);
	let Some(oracle) = oracle() else { return };

	// Veritas alone, left open across the days, as the app's timer does.
	let veritas = import(&scratch, &base, "veritas");
	let mut veritas = veritas;
	for day in DAYS {
		see(&veritas, day);
		veritas.maintain(day.to_owned()).unwrap();
		match day {
			"2026-11-02" => {
				veritas.undo().unwrap();
				assert!(
					veritas
						.maintain(day.to_owned())
						.unwrap()
						.entities
						.is_empty(),
					"undoing the run leaves the day marked done, so nothing reposts today"
				);
				let rent = veritas
					.bills(day.to_owned())
					.unwrap()
					.into_iter()
					.find(|b| b.id == "rent")
					.unwrap();
				assert_eq!(
					rent.status,
					BillStatus::Due,
					"rent is unpaid again, on its due day"
				);
			}
			"2026-11-13" => {
				veritas.close().unwrap();
				veritas = reopen(&scratch, "veritas");
				see(&veritas, day);
				veritas.maintain(day.to_owned()).unwrap();
			}
			"2026-11-20" => {
				assert!(
					veritas
						.maintain(day.to_owned())
						.unwrap()
						.entities
						.is_empty(),
					"once a day"
				);
			}
			_ => {}
		}
		assert_eq!(last_run(&scratch, "veritas").as_deref(), Some(day));
	}
	let ours = scratch.join("veritas-alone.zip");
	export(&veritas, &ours);
	veritas.close().unwrap();

	// Actual alone, opening the same base on the same days.
	let mut zip = base.clone();
	for (seed, day) in DAYS.iter().enumerate() {
		let out = scratch.join(&format!("actual-{day}.zip"));
		actual_apply(
			&oracle,
			&zip,
			json!([{"op": "openBudgetSchedules"}]),
			&out,
			day,
			seed as u32 + 10,
		);
		zip = out;
	}
	let theirs = zip;

	// Taking turns: Veritas on odd days, Actual on even ones, each opening
	// the file the other left.
	let mut zip = base.clone();
	for (step, day) in DAYS.iter().enumerate() {
		let out = scratch.join(&format!("turns-{day}.zip"));
		if step % 2 == 0 {
			let session = import(&scratch, &zip, &format!("turns-{day}"));
			see(&session, day);
			session.maintain((*day).to_owned()).unwrap();
			export(&session, &out);
			session.close().unwrap();
		} else {
			actual_apply(
				&oracle,
				&zip,
				json!([{"op": "openBudgetSchedules"}]),
				&out,
				day,
				step as u32 + 100,
			);
		}
		zip = out;
	}
	let turns = zip;

	let last = DAYS[DAYS.len() - 1];
	let (ours, theirs, turns) = (
		actual_reads(&oracle, &ours, last),
		actual_reads(&oracle, &theirs, last),
		actual_reads(&oracle, &turns, last),
	);
	assert_eq!(
		posted(&ours),
		posted(&theirs),
		"Veritas alone matches Actual alone"
	);
	assert_eq!(
		posted(&turns),
		posted(&theirs),
		"taking turns matches Actual alone"
	);
	assert_eq!(
		count(&ours, "rent"),
		2,
		"2 Nov (Monday after the 1st) and 1 Dec"
	);
	assert_eq!(count(&ours, "paycheck"), 2, "6 and 20 Nov");
	assert_eq!(
		count(&ours, "savings"),
		2,
		"the transfer and its other leg, both linked to the bill"
	);
	assert_eq!(count(&ours, "gym"), 1, "it ends after one charge");
	assert_eq!(
		count(&ours, "power"),
		0,
		"confirm-first bills wait for Paid"
	);
	assert_eq!(schedules(&ours), schedules(&theirs));
	assert_eq!(schedules(&turns), schedules(&theirs));
}

fn bill_on(session: &Session, id: &str, day: &str) -> veritas_ffi::BillSummary {
	session
		.bills(day.to_owned())
		.unwrap()
		.into_iter()
		.find(|b| b.id == id)
		.unwrap()
}

/// The live transaction Actual posted for `schedule` on `date`.
fn posted_id(dump: &Value, schedule: &str, date: i64) -> String {
	rows(dump, "transactions")
		.iter()
		.find(|row| row["schedule"] == schedule && row["date"] == date && row["tombstone"] != 1)
		.unwrap()["id"]
		.as_str()
		.unwrap()
		.to_owned()
}

#[test]
fn paid_with_an_edited_amount_and_date_writes_what_actual_writes_when_you_post_and_edit() {
	// R-BILL-3: Paid on the confirm-first power bill, due Friday 13 Nov,
	// paid on the 12th for 83.42. In Actual that's "Post transaction", then
	// editing the row's amount and date.
	let scratch = Scratch::new("paid");
	let base = household_base(&scratch);
	let Some(oracle) = oracle() else { return };
	let day = "2026-11-12";

	let session = import(&scratch, &base, "veritas");
	see(&session, day);
	let before = bill_on(&session, "power", day);
	assert_eq!(before.next_date.as_deref(), Some("2026-11-13"));
	assert_eq!(before.scheduled_amount, Some(-8_000));
	perform(
		&session,
		Command::PayBill {
			bill_id: "power".to_owned(),
			amount: -8_342,
			date: day.to_owned(),
		},
	);
	let paid = bill_on(&session, "power", day);
	assert_eq!(
		paid.status,
		BillStatus::Paid,
		"Actual counts the 12th as paying the 13th"
	);
	let ours = scratch.join("ours.zip");
	export(&session, &ours);
	session.close().unwrap();

	let posted_zip = scratch.join("posted.zip");
	actual_apply(
		&oracle,
		&base,
		json!([{"op": "postSchedule", "id": "power", "today": false}]),
		&posted_zip,
		day,
		1,
	);
	let row = posted_id(
		&actual_reads(&oracle, &posted_zip, day),
		"power",
		20_261_113,
	);
	let theirs = scratch.join("theirs.zip");
	actual_apply(
		&oracle,
		&posted_zip,
		json!([{"op": "updateTransaction", "id": row, "fields": {"amount": -8_342, "date": day}}]),
		&theirs,
		day,
		2,
	);

	let (ours_dump, theirs_dump) = (
		actual_reads(&oracle, &ours, day),
		actual_reads(&oracle, &theirs, day),
	);
	assert_eq!(posted(&ours_dump), posted(&theirs_dump));
	assert_eq!(count(&ours_dump, "power"), 1);
	assert_eq!(
		schedules(&ours_dump)["power"],
		schedules(&theirs_dump)["power"],
		"Actual reads the same next date and upcoming dates"
	);

	// A few days on, both sides move the bill on the same way.
	let (ours_later, theirs_later) = (
		scratch.join("ours-later.zip"),
		scratch.join("theirs-later.zip"),
	);
	let open = json!([{"op": "openBudgetSchedules"}]);
	actual_apply(&oracle, &ours, open.clone(), &ours_later, "2026-11-16", 3);
	actual_apply(&oracle, &theirs, open, &theirs_later, "2026-11-16", 4);
	let (ours_later, theirs_later) = (
		actual_reads(&oracle, &ours_later, "2026-11-16"),
		actual_reads(&oracle, &theirs_later, "2026-11-16"),
	);
	assert_eq!(
		schedules(&ours_later)["power"],
		schedules(&theirs_later)["power"]
	);
	assert_eq!(posted(&ours_later), posted(&theirs_later));
}

#[test]
fn paying_an_auto_post_bill_by_hand_never_lets_either_app_post_it_again() {
	// R-BILL-2, R-BILL-3, R-BILL-5: Paid on a due auto-post bill before the
	// day's run, Paid early on next month's rent, and an income bill paid a
	// day early. Neither Veritas's service nor Actual's posts any of them again.
	let scratch = Scratch::new("paid-auto");
	let base = household_base(&scratch);
	let session = import(&scratch, &base, "veritas");

	see(&session, "2026-11-02");
	assert_eq!(
		bill_on(&session, "rent", "2026-11-02").status,
		BillStatus::Due
	);
	perform(
		&session,
		Command::PayBill {
			bill_id: "rent".to_owned(),
			amount: -151_000,
			date: "2026-11-02".to_owned(),
		},
	);
	session.maintain("2026-11-02".to_owned()).unwrap();

	see(&session, "2026-11-05");
	let paycheck = bill_on(&session, "paycheck", "2026-11-05");
	assert!(paycheck.is_income);
	assert_eq!(paycheck.scheduled_amount, Some(250_000));
	let tray = session.upcoming(7, "2026-11-05".to_owned()).unwrap();
	assert!(
		tray.iter()
			.any(|u| u.bill_id == "paycheck" && u.is_income && u.date == "2026-11-06")
	);
	perform(
		&session,
		Command::PayBill {
			bill_id: "paycheck".to_owned(),
			amount: 251_250,
			date: "2026-11-05".to_owned(),
		},
	);
	session.maintain("2026-11-06".to_owned()).unwrap();

	see(&session, "2026-11-21");
	session.maintain("2026-11-21".to_owned()).unwrap();
	assert_eq!(
		bill_on(&session, "rent", "2026-11-21").next_date.as_deref(),
		Some("2026-12-01")
	);
	perform(
		&session,
		Command::PayBill {
			bill_id: "rent".to_owned(),
			amount: -150_000,
			date: "2026-11-21".to_owned(),
		},
	);
	assert_eq!(
		bill_on(&session, "rent", "2026-11-21").next_date.as_deref(),
		Some("2027-01-01"),
		"paid outside the window Actual counts, so the bill moves on to January"
	);
	session.maintain("2026-12-01".to_owned()).unwrap();
	let ours = scratch.join("ours.zip");
	export(&session, &ours);
	let rent_rows = || {
		let mut rows: Vec<(String, i64)> = session
			.register(
				veritas_ffi::RegisterScope::All,
				veritas_ffi::RegisterFilter::default(),
				veritas_ffi::RegisterSort {
					key: veritas_ffi::RegisterSortKey::Date,
					ascending: true,
				},
				0,
				1_000,
			)
			.unwrap()
			.rows
			.into_iter()
			.filter(|r| r.schedule_id.as_deref() == Some("rent"))
			.map(|r| (r.date, r.amount))
			.collect();
		rows.sort();
		rows
	};
	assert_eq!(
		rent_rows(),
		[
			("2026-11-02".to_owned(), -151_000),
			("2026-11-21".to_owned(), -150_000)
		],
		"the paid amounts, and no service post on 2 Nov or 1 Dec"
	);
	session.close().unwrap();

	let Some(oracle) = oracle() else { return };
	let mut zip = ours;
	for (seed, day) in ["2026-11-02", "2026-11-06", "2026-12-01", "2026-12-02"]
		.iter()
		.enumerate()
	{
		let out = scratch.join(&format!("actual-{day}.zip"));
		let mut ops = vec![json!({"op": "openBudgetSchedules"})];
		if *day == "2026-11-02" {
			ops.push(json!({"op": "runSchedules"}));
		}
		actual_apply(
			&oracle,
			&zip,
			Value::Array(ops),
			&out,
			day,
			seed as u32 + 20,
		);
		zip = out;
	}
	let dump = actual_reads(&oracle, &zip, "2026-12-02");
	assert_eq!(
		count(&dump, "rent"),
		2,
		"Actual posts neither rent again: {:?}",
		posted(&dump)
	);
	assert_eq!(
		count(&dump, "paycheck"),
		2,
		"5 Nov paid by hand, then Actual posts 20 Nov"
	);
	assert!(posted(&dump).iter().any(|row| row["schedule"] == "paycheck"
		&& row["date"] == 20_261_105
		&& row["amount"] == 251_250));
	assert_eq!(schedules(&dump)["rent"]["nextDate"], "2027-01-01");
}

fn conditions_of(dump: &Value, schedule: &str) -> Value {
	let rule = rows(dump, "schedules")
		.iter()
		.find(|row| row["id"] == schedule)
		.unwrap()["rule"]
		.clone();
	let stored = rows(dump, "rules")
		.iter()
		.find(|row| row["id"] == rule)
		.unwrap();
	serde_json::from_str(stored["conditions"].as_str().unwrap()).unwrap()
}

#[test]
fn pausing_resuming_skipping_editing_and_ending_bills_matches_actual_over_a_month() {
	// R-BILL-1, R-BILL-3: each day the budget is opened first (the day's
	// run), then one person's change, in Veritas and in Actual on the same base.
	let scratch = Scratch::new("lifecycle");
	let base = household_base(&scratch);
	let session = import(&scratch, &base, "veritas");
	let id = |bill: &str| bill.to_owned();

	let mut steps: Vec<(&str, Option<Value>)> = Vec::new();
	let day_of =
		|session: &Session, day: &'static str, command: Option<Command>, op: Option<Value>| {
			see(session, day);
			session.maintain(day.to_owned()).unwrap();
			if let Some(command) = command {
				perform(session, command);
			}
			(day, op)
		};
	steps.push(day_of(
		&session,
		"2026-11-05",
		Some(Command::PauseBill {
			bill_id: id("paycheck"),
		}),
		Some(json!({"op": "updateSchedule", "id": "paycheck", "fields": {"completed": true}})),
	));
	steps.push(day_of(&session, "2026-11-06", None, None));
	assert_eq!(
		bill_on(&session, "paycheck", "2026-11-06").state,
		BillState::Paused
	);
	steps.push(day_of(&session, "2026-11-09", Some(Command::ResumeBill { bill_id: id("paycheck") }),
		Some(json!({"op": "updateSchedule", "id": "paycheck", "fields": {"completed": false}, "resetNextDate": true}))));
	assert_eq!(
		bill_on(&session, "paycheck", "2026-11-09")
			.next_date
			.as_deref(),
		Some("2026-11-20"),
		"Restart picks up from today; the paused 6 Nov isn't caught up"
	);

	let mut power = bill(
		"Power",
		CITY_POWER,
		BillAmount::About { amount: -9_000 },
		monthly("2026-11-16", None),
		Some(UTILITIES),
		false,
	);
	power.auto_post = true;
	see(&session, "2026-11-10");
	session.maintain("2026-11-10".to_owned()).unwrap();
	perform(
		&session,
		Command::EditBill {
			bill_id: id("power"),
			bill: power,
		},
	);
	let edited = scratch.join("edited.zip");
	export(&session, &edited);
	steps.push(("2026-11-10", Some(json!({"op": "editPower"}))));
	assert_eq!(
		bill_on(&session, "power", "2026-11-10")
			.next_date
			.as_deref(),
		Some("2026-11-16")
	);
	assert_eq!(
		last_run(&scratch, "veritas"),
		None,
		"saving an auto-post bill lets the service run again today"
	);

	steps.push(day_of(&session, "2026-11-16", None, None));
	steps.push(day_of(
		&session,
		"2026-11-21",
		Some(Command::EndBill {
			bill_id: id("savings"),
		}),
		Some(json!({"op": "updateSchedule", "id": "savings", "fields": {"completed": true}})),
	));
	assert_eq!(
		bill_on(&session, "savings", "2026-11-21").state,
		BillState::Ended
	);
	steps.push(day_of(
		&session,
		"2026-11-25",
		Some(Command::SkipBill {
			bill_id: id("rent"),
		}),
		Some(json!({"op": "skipSchedule", "id": "rent"})),
	));
	assert_eq!(
		bill_on(&session, "rent", "2026-11-25").next_date.as_deref(),
		Some("2027-01-01")
	);
	steps.push(day_of(&session, "2026-12-01", None, None));
	steps.push(day_of(&session, "2026-12-02", None, None));
	assert!(
		matches!(
			session.perform(Command::ResumeBill {
				bill_id: id("rent")
			}),
			Err(FfiError::Invalid { .. })
		),
		"resuming an active bill is refused"
	);
	let ours = scratch.join("ours.zip");
	export(&session, &ours);
	session.close().unwrap();

	let Some(oracle) = oracle() else { return };
	let edit_conditions = conditions_of(&actual_reads(&oracle, &edited, "2026-11-10"), "power");
	let mut zip = base.clone();
	for (seed, (day, op)) in steps.iter().enumerate() {
		let mut ops = vec![json!({"op": "openBudgetSchedules"})];
		match op {
			Some(op) if op["op"] == "editPower" => ops.push(json!({
				"op": "updateSchedule", "id": "power",
				"fields": {"name": "Power", "posts_transaction": true},
				"conditions": edit_conditions, "resetNextDate": true,
			})),
			Some(op) => {
				ops.push(json!({"op": "tick"}));
				ops.push(op.clone());
			}
			None => {}
		}
		let out = scratch.join(&format!("actual-{seed}.zip"));
		actual_apply(
			&oracle,
			&zip,
			Value::Array(ops),
			&out,
			day,
			seed as u32 + 40,
		);
		zip = out;
	}
	let (ours, theirs) = (
		actual_reads(&oracle, &ours, "2026-12-02"),
		actual_reads(&oracle, &zip, "2026-12-02"),
	);
	assert_eq!(posted(&ours), posted(&theirs));
	assert_eq!(schedules(&ours), schedules(&theirs));
	assert_eq!(
		count(&ours, "paycheck"),
		1,
		"only 20 Nov: 6 Nov fell in the pause"
	);
	assert_eq!(
		count(&ours, "power"),
		1,
		"auto-post from the edit on, 16 Nov"
	);
	assert_eq!(
		count(&ours, "savings"),
		2,
		"November's transfer and its leg, none after the end"
	);
	assert_eq!(count(&ours, "rent"), 1, "December skipped");
}

#[test]
fn a_failed_service_commit_or_bill_save_never_touches_last_schedule_run() {
	// §10, contract v8: lastScheduleRun is written after the commit, so a
	// commit that fails leaves the day to run again, and a bill save that
	// fails doesn't clear it. A failed run never fails `maintain`: the shell
	// reads it from state() until a later run works.
	let scratch = Scratch::new("failed-commit");
	let base = household_base(&scratch);
	let session = import(&scratch, &base, "veritas");
	session.close().unwrap();
	assert_eq!(last_run(&scratch, "veritas"), None);

	sqlite(&scratch, "veritas")
		.execute_batch(
			"CREATE TRIGGER cowboy_veto BEFORE INSERT ON transactions
			 BEGIN SELECT RAISE(ABORT, 'the disk said no'); END;",
		)
		.unwrap();
	let session = reopen(&scratch, "veritas");
	see(&session, "2026-11-02");
	let failed = session.maintain("2026-11-02".to_owned()).unwrap();
	assert!(failed.entities.is_empty(), "{failed:?}");
	let problem = session.state().unwrap().schedule_problem;
	assert!(
		problem
			.as_deref()
			.is_some_and(|problem| problem.contains("the disk said no")),
		"{problem:?}"
	);
	assert_eq!(
		last_run(&scratch, "veritas"),
		None,
		"a failed run doesn't mark the day done"
	);
	let again = session.maintain("2026-11-02".to_owned()).unwrap();
	assert!(again.entities.is_empty());
	assert!(
		session.state().unwrap().schedule_problem.is_some(),
		"each tick tries again and keeps the problem while it lasts"
	);
	assert_eq!(
		bill_on(&session, "rent", "2026-11-02").status,
		BillStatus::Due
	);

	sqlite(&scratch, "veritas")
		.execute_batch("DROP TRIGGER cowboy_veto")
		.unwrap();
	session.maintain("2026-11-02".to_owned()).unwrap();
	assert_eq!(session.state().unwrap().schedule_problem, None);
	assert_eq!(last_run(&scratch, "veritas").as_deref(), Some("2026-11-02"));
	assert_eq!(
		bill_on(&session, "rent", "2026-11-02").status,
		BillStatus::Paid
	);

	sqlite(&scratch, "veritas")
		.execute_batch(
			"CREATE TRIGGER cowboy_veto_new BEFORE INSERT ON schedules
			 BEGIN SELECT RAISE(ABORT, 'the disk said no'); END;
			 CREATE TRIGGER cowboy_veto_edit BEFORE UPDATE ON rules
			 BEGIN SELECT RAISE(ABORT, 'the disk said no'); END;",
		)
		.unwrap();
	let phone = bill(
		"Phone",
		CITY_POWER,
		BillAmount::Exact { amount: -5_500 },
		monthly("2026-11-18", None),
		None,
		true,
	);
	let failed = session.perform(Command::CreateBill {
		bill_id: Some("phone".to_owned()),
		bill: phone,
	});
	assert!(
		matches!(failed, Err(FfiError::Storage { .. })),
		"{failed:?}"
	);
	assert_eq!(
		last_run(&scratch, "veritas").as_deref(),
		Some("2026-11-02"),
		"a failed create leaves the day done"
	);
	let mut gym = bill(
		"Gym joining fee",
		ACME,
		BillAmount::Exact { amount: -4_500 },
		monthly("2026-11-11", None),
		None,
		true,
	);
	gym.payee_id = Some(ACME.to_owned());
	let failed = session.perform(Command::EditBill {
		bill_id: "gym".to_owned(),
		bill: gym,
	});
	assert!(
		matches!(failed, Err(FfiError::Storage { .. })),
		"{failed:?}"
	);
	assert_eq!(
		last_run(&scratch, "veritas").as_deref(),
		Some("2026-11-02"),
		"a failed edit leaves the day done"
	);
	assert!(
		session
			.bills("2026-11-02".to_owned())
			.unwrap()
			.iter()
			.all(|b| b.id != "phone")
	);
	session.close().unwrap();
}

fn condition(field: RuleField, op: RuleConditionOp, value: RuleValue) -> RuleCondition {
	RuleCondition { field, op, value }
}

fn set(field: RuleField, value: RuleValue) -> RuleAction {
	RuleAction {
		op: RuleActionOp::Set,
		field: Some(field),
		value,
		split_index: 0,
	}
}

fn line(split_index: u32, share: RuleValue, category: &str) -> [RuleAction; 2] {
	[
		RuleAction {
			op: RuleActionOp::SetSplitAmount,
			field: None,
			value: share,
			split_index,
		},
		RuleAction {
			op: RuleActionOp::Set,
			field: Some(RuleField::Category),
			value: id(category),
			split_index,
		},
	]
}

fn id(id: &str) -> RuleValue {
	RuleValue::Id {
		id: Some(id.to_owned()),
	}
}

fn words(text: &str) -> RuleValue {
	RuleValue::Text {
		text: text.to_owned(),
	}
}

fn draft(
	op: RuleConditionsOp,
	conditions: Vec<RuleCondition>,
	actions: Vec<RuleAction>,
) -> RuleDraft {
	RuleDraft {
		stage: RuleStage::Default,
		conditions_op: op,
		conditions,
		actions,
	}
}

fn create_rule(session: &Session, draft: RuleDraft) -> String {
	created(
		&perform(session, Command::CreateRule { draft }),
		EntityKind::Rule,
	)
}

fn preview(session: &Session, rule: &str) -> Vec<veritas_ffi::RuleMatch> {
	block_on(session.rule_preview(
		RuleSource::Saved {
			rule_id: rule.to_owned(),
		},
		CancelToken::new(),
	))
	.unwrap()
}

fn spend(
	id: &str,
	date: &str,
	amount: i64,
	payee: Option<&str>,
	payee_name: Option<&str>,
	notes: Option<&str>,
) -> Command {
	Command::AddTransaction {
		transaction: TransactionDraft {
			id: Some(id.to_owned()),
			account_id: CHECKING.to_owned(),
			date: date.to_owned(),
			amount,
			payee_id: payee.map(str::to_owned),
			payee_name: payee_name.map(str::to_owned),
			category_id: None,
			notes: notes.map(str::to_owned),
			cleared: false,
			splits: Vec::new(),
		},
	}
}

/// A template action and a repeating-date condition, which the sentence
/// editor can't show, made by Actual's own createRule.
fn actual_only_rules() -> Value {
	json!([
		{"op": "createRule", "id": "actual-template", "conditions": [
			{"field": "imported_payee", "op": "contains", "value": "PAYPAL", "type": "string"}
		], "actions": [
			{"op": "set", "field": "notes", "value": "", "type": "string",
			 "options": {"template": "{{imported_payee}} via PayPal"}}
		]},
		{"op": "createRule", "id": "actual-repeating", "stage": "pre", "conditions": [
			{"field": "date", "op": "is", "type": "date", "value": {
				"start": "2026-08-01", "frequency": "monthly", "interval": 1, "patterns": [],
				"skipWeekend": false, "weekendSolveMode": "before", "endMode": "never"}}
		], "actions": [
			{"op": "set", "field": "cleared", "value": true, "type": "boolean"}
		]}
	])
}

/// small.zip with a few unfiled card purchases, then two rules only Actual can edit.
fn rules_base(scratch: &Scratch, oracle: &Path) -> PathBuf {
	let session = import(scratch, &small_zip(), "setup");
	see(&session, "2026-11-05");
	perform(
		&session,
		spend(
			"amzn-1",
			"2026-10-21",
			-2_399,
			None,
			Some("AMZN Mktp US*2K4"),
			None,
		),
	);
	perform(
		&session,
		spend(
			"amzn-2",
			"2026-10-25",
			-5_649,
			None,
			Some("AMZN Mktp US*7Q1"),
			Some("birthday #gift"),
		),
	);
	perform(
		&session,
		spend(
			"grocer",
			"2026-10-28",
			-3_200,
			Some(CORNER_GROCER),
			None,
			Some("weekly shop"),
		),
	);
	perform(
		&session,
		spend("costco", "2026-11-03", -15_000, None, Some("Costco"), None),
	);
	let ours = scratch.join("base-veritas.zip");
	export(&session, &ours);
	session.close().unwrap();
	let base = scratch.join("base.zip");
	actual_apply(oracle, &ours, actual_only_rules(), &base, "2026-11-05", 1);
	base
}

/// Actual's rule editor search for each rule, in its order.
fn actual_matches(oracle: &Path, zip: &Path, rules: &[&str], scratch: &Scratch) -> Value {
	let file = scratch.join("rule-ids.json");
	std::fs::write(&file, json!(rules).to_string()).unwrap();
	serde_json::from_slice(&node(
		oracle,
		&[
			"rule-matches",
			&text(zip),
			&text(&file),
			"--today",
			"2026-11-05",
		],
	))
	.unwrap()
}

/// Every live transaction as Actual stores it, with payees by name so a payee
/// each side created reads the same, and split lines under their parent.
fn ledger(dump: &Value) -> Vec<Value> {
	let payees: std::collections::BTreeMap<&str, &Value> = rows(dump, "payees")
		.iter()
		.map(|row| (row["id"].as_str().unwrap(), &row["name"]))
		.collect();
	let mapped: std::collections::BTreeMap<&str, &str> = rows(dump, "payee_mapping")
		.iter()
		.map(|row| {
			(
				row["id"].as_str().unwrap(),
				row["targetId"].as_str().unwrap(),
			)
		})
		.collect();
	let name = |payee: &Value| {
		payee.as_str().map(|p| {
			payees
				.get(mapped.get(p).copied().unwrap_or(p))
				.cloned()
				.cloned()
		})
	};
	let transactions = rows(dump, "transactions");
	let parent_of = |row: &Value| {
		transactions
			.iter()
			.find(|p| p["id"] == row["parent_id"])
			.map(|p| (p["date"].clone(), p["amount"].clone()))
	};
	let mut ledger: Vec<Value> = transactions
		.iter()
		.filter(|row| row["tombstone"] != 1)
		.map(|row| {
			json!({
				"acct": row["acct"], "date": row["date"], "amount": row["amount"],
				"payee": name(&row["description"]), "category": row["category"], "notes": row["notes"],
				"cleared": row["cleared"], "isParent": row["isParent"], "isChild": row["isChild"],
				"error": row["error"], "parent": parent_of(row),
			})
		})
		.collect();
	ledger.sort_by_key(|row| row.to_string());
	ledger
}

fn rule_row(dump: &Value, id: &str) -> Value {
	rows(dump, "rules")
		.iter()
		.find(|row| row["id"] == id)
		.unwrap()
		.clone()
}

#[test]
fn a_rules_session_previews_exactly_what_actual_finds_and_applies_what_actual_applies() {
	// R-RULE-2: create, edit and delete rules in the sentence editor, a split
	// rule, and "Apply to existing" on the rows the preview lists. The preview
	// must find the rows Actual's rule editor finds, and the apply must write
	// what the preview promised and what Actual's rule-apply-actions writes.
	let Some(oracle) = oracle() else { return };
	let scratch = Scratch::new("rules");
	let base = rules_base(&scratch, &oracle);
	let session = import(&scratch, &base, "veritas");
	see(&session, "2026-11-05");
	let groups = session
		.budget_month(
			veritas_ffi::parse_month("2026-11".to_owned()).unwrap(),
			"2026-11-05".to_owned(),
		)
		.unwrap()
		.groups;
	let everyday = groups
		.iter()
		.find(|g| g.envelopes.iter().any(|e| e.id == GROCERIES))
		.unwrap()
		.id
		.clone();
	let shopping = created(
		&perform(
			&session,
			Command::CreateCategory {
				group_id: everyday,
				name: "Shopping".to_owned(),
			},
		),
		EntityKind::Category,
	);
	let costco = session
		.payees()
		.unwrap()
		.into_iter()
		.find(|p| p.name == "Costco")
		.unwrap()
		.id;

	let amazon = create_rule(
		&session,
		draft(
			RuleConditionsOp::And,
			vec![condition(
				RuleField::Payee,
				RuleConditionOp::Contains,
				words("amzn"),
			)],
			vec![
				set(RuleField::PayeeName, words("Amazon")),
				set(RuleField::Category, id(&shopping)),
			],
		),
	);
	let weekly = create_rule(
		&session,
		draft(
			RuleConditionsOp::And,
			vec![condition(
				RuleField::Notes,
				RuleConditionOp::Contains,
				words("weekly shop"),
			)],
			vec![set(RuleField::Category, id(GROCERIES))],
		),
	);
	perform(
		&session,
		Command::EditRule {
			rule_id: amazon.clone(),
			draft: draft(
				RuleConditionsOp::And,
				vec![condition(
					RuleField::Payee,
					RuleConditionOp::Contains,
					words("amzn"),
				)],
				vec![
					set(RuleField::PayeeName, words("Amazon")),
					set(RuleField::Category, id(&shopping)),
					RuleAction {
						op: RuleActionOp::AppendNotes,
						field: None,
						value: words(" (online)"),
						split_index: 0,
					},
				],
			),
		},
	);
	perform(
		&session,
		Command::DeleteRule {
			rule_id: weekly.clone(),
		},
	);
	let mut split_actions = Vec::new();
	split_actions.extend(line(1, RuleValue::Percent { percent: 60.0 }, GROCERIES));
	split_actions.extend(line(2, RuleValue::Nothing, DINING_OUT));
	let costco_rule = create_rule(
		&session,
		draft(
			RuleConditionsOp::And,
			vec![condition(
				RuleField::Payee,
				RuleConditionOp::Is,
				id(&costco),
			)],
			split_actions,
		),
	);
	let searches = [
		draft(
			RuleConditionsOp::And,
			vec![condition(
				RuleField::Payee,
				RuleConditionOp::Matches,
				words("^AMZN.*\\*7"),
			)],
			vec![set(RuleField::Notes, words("regex"))],
		),
		draft(
			RuleConditionsOp::Or,
			vec![
				condition(RuleField::Notes, RuleConditionOp::HasTags, words("#gift")),
				condition(
					RuleField::Amount,
					RuleConditionOp::Lt,
					RuleValue::Amount { amount: -100_000 },
				),
			],
			vec![set(RuleField::Cleared, RuleValue::Bool { value: true })],
		),
		draft(
			RuleConditionsOp::And,
			vec![
				condition(
					RuleField::Amount,
					RuleConditionOp::IsApprox,
					RuleValue::Amount { amount: -12_657 },
				),
				condition(
					RuleField::Date,
					RuleConditionOp::Gte,
					RuleValue::Date {
						date: "2026-09-01".to_owned(),
					},
				),
			],
			vec![set(RuleField::Notes, words("approx"))],
		),
		draft(
			RuleConditionsOp::And,
			vec![condition(
				RuleField::Category,
				RuleConditionOp::Is,
				RuleValue::Id { id: None },
			)],
			vec![set(RuleField::Category, id(GROCERIES))],
		),
	];
	let searches: Vec<String> = searches
		.into_iter()
		.map(|d| create_rule(&session, d))
		.collect();

	let listed = session.rules().unwrap();
	let access = |id: &str| listed.iter().find(|r| r.id == id).map(|r| r.access.clone());
	assert_eq!(access(&amazon), Some(RuleAccess::Editable));
	assert_eq!(access(&weekly), None, "a deleted rule leaves the list");
	for actual_only in ["actual-template", "actual-repeating"] {
		assert!(
			matches!(access(actual_only), Some(RuleAccess::EditInActual { .. })),
			"{actual_only}: {:?}",
			access(actual_only)
		);
		assert!(matches!(
			session.perform(Command::EditRule {
				rule_id: actual_only.to_owned(),
				draft: draft(
					RuleConditionsOp::And,
					vec![condition(
						RuleField::Notes,
						RuleConditionOp::Contains,
						words("x")
					)],
					vec![set(RuleField::Cleared, RuleValue::Bool { value: true })],
				),
			}),
			Err(FfiError::Invalid { .. })
		));
	}
	let template = listed.iter().find(|r| r.id == "actual-template").unwrap();
	assert!(
		!template.runs_in_veritas,
		"Veritas can't run a template action"
	);
	assert!(matches!(
		block_on(session.rule_preview(
			RuleSource::Saved {
				rule_id: "actual-template".to_owned()
			},
			CancelToken::new()
		)),
		Err(FfiError::Invalid { .. })
	));
	assert!(
		matches!(
			session.perform(Command::ApplyRule {
				source: RuleSource::Saved {
					rule_id: "actual-template".to_owned()
				},
				transaction_ids: vec!["amzn-1".to_owned()],
			}),
			Err(FfiError::Invalid { .. })
		),
		"a rule Veritas can't run writes nothing (D60)"
	);
	let saved = scratch.join("rules.zip");
	export(&session, &saved);

	// The preview finds what Actual's rule editor finds, in its order.
	let mut all: Vec<&str> = vec![amazon.as_str(), costco_rule.as_str()];
	all.extend(searches.iter().map(String::as_str));
	let theirs = actual_matches(&oracle, &saved, &all, &scratch);
	for rule in &all {
		let ours: Vec<String> = preview(&session, rule)
			.into_iter()
			.map(|m| m.transaction_id)
			.collect();
		let expected: Vec<String> = serde_json::from_value(theirs[*rule].clone()).unwrap();
		assert_eq!(ours, expected, "rule {rule}");
	}

	// Apply to the rows the preview lists, and check each landed as previewed.
	let found = preview(&session, &amazon);
	assert_eq!(
		found
			.iter()
			.map(|m| m.transaction_id.as_str())
			.collect::<Vec<_>>(),
		["amzn-2", "amzn-1"]
	);
	perform(
		&session,
		Command::ApplyRule {
			source: RuleSource::Saved {
				rule_id: amazon.clone(),
			},
			transaction_ids: found.iter().map(|m| m.transaction_id.clone()).collect(),
		},
	);
	for promised in &found {
		let row = session
			.register_row(promised.transaction_id.clone())
			.unwrap();
		for change in &promised.changes {
			match change.field {
				veritas_ffi::RuleChangedField::Category => {
					assert_eq!(
						change.after,
						RuleValue::Id {
							id: row.category_id.clone()
						}
					)
				}
				veritas_ffi::RuleChangedField::Notes => {
					assert_eq!(
						change.after,
						RuleValue::Text {
							text: row.notes.clone().unwrap_or_default()
						}
					)
				}
				veritas_ffi::RuleChangedField::Payee => {
					assert_eq!(row.payee_name.as_deref(), Some("Amazon"));
					assert!(
						matches!(&change.after, RuleValue::Text { text } if text == "Amazon")
							|| change.after
								== RuleValue::Id {
									id: row.payee_id.clone()
								}
					);
				}
				other => panic!("the amazon rule changes nothing else, got {other:?}"),
			}
		}
	}
	let split = preview(&session, &costco_rule);
	assert_eq!(split.len(), 1);
	perform(
		&session,
		Command::ApplyRule {
			source: RuleSource::Saved {
				rule_id: costco_rule.clone(),
			},
			transaction_ids: vec!["costco".to_owned()],
		},
	);
	let parent = session.register_row("costco".to_owned()).unwrap();
	let lines: Vec<(i64, Option<String>)> = parent
		.lines
		.iter()
		.map(|l| (l.amount, l.category_id.clone()))
		.collect();
	let promised: Vec<(i64, Option<String>)> = split[0]
		.split_lines
		.iter()
		.map(|l| (l.amount, l.category_id.clone()))
		.collect();
	assert_eq!(lines, promised);
	assert_eq!(
		lines,
		[
			(-9_000, Some(GROCERIES.to_owned())),
			(-6_000, Some(DINING_OUT.to_owned()))
		]
	);
	let applied = scratch.join("applied.zip");
	export(&session, &applied);

	// Actual applies the same rules to the same rows of the same file.
	let theirs_zip = scratch.join("theirs.zip");
	actual_apply(
		&oracle,
		&saved,
		json!([
			{"op": "applyActions", "rule": amazon, "transactions": ["amzn-2", "amzn-1"]},
			{"op": "applyActions", "rule": costco_rule, "transactions": ["costco"]}
		]),
		&theirs_zip,
		"2026-11-05",
		7,
	);
	let (ours, theirs) = (
		actual_reads(&oracle, &applied, "2026-11-05"),
		actual_reads(&oracle, &theirs_zip, "2026-11-05"),
	);
	assert_eq!(ledger(&ours), ledger(&theirs));

	// The rules only Actual can edit come through the session byte for byte.
	let original = actual_reads(&oracle, &base, "2026-11-05");
	for actual_only in ["actual-template", "actual-repeating"] {
		assert_eq!(
			rule_row(&ours, actual_only),
			rule_row(&original, actual_only),
			"{actual_only}"
		);
	}

	// Undoing the split apply puts the purchase back as one row.
	session.undo().unwrap();
	assert!(
		session
			.register_row("costco".to_owned())
			.unwrap()
			.lines
			.is_empty()
	);
	session.close().unwrap();
}

#[test]
fn the_preview_finds_what_actuals_rule_editor_finds_for_each_kind_of_condition() {
	// R-RULE-2: one saved rule per condition shape, each previewed in Veritas
	// and searched in Actual's rule editor on the same file. Empty notes,
	// empty categories, transfers and amounts by direction are where a search
	// and a rule run most often part ways.
	let Some(oracle) = oracle() else { return };
	let scratch = Scratch::new("matcher");
	let base = rules_base(&scratch, &oracle);
	let session = import(&scratch, &base, "veritas");
	see(&session, "2026-11-05");
	let savings = "91c4a470-079c-4d7c-ab80-575788e5dc9d";
	let ids = |ids: &[&str]| RuleValue::Ids {
		ids: ids.iter().map(|s| (*s).to_owned()).collect(),
	};
	let shapes = [
		condition(
			RuleField::Notes,
			RuleConditionOp::DoesNotContain,
			words("shop"),
		),
		condition(
			RuleField::Notes,
			RuleConditionOp::IsNot,
			words("weekly shop"),
		),
		condition(RuleField::Notes, RuleConditionOp::Is, words("WEEKLY SHOP")),
		condition(RuleField::Notes, RuleConditionOp::Matches, words("^birth")),
		condition(
			RuleField::Notes,
			RuleConditionOp::HasAnyTag,
			words("#gift #birthday"),
		),
		condition(RuleField::Category, RuleConditionOp::IsNot, id(GROCERIES)),
		condition(
			RuleField::Category,
			RuleConditionOp::NotOneOf,
			ids(&[GROCERIES, RENT]),
		),
		condition(
			RuleField::Payee,
			RuleConditionOp::NotOneOf,
			ids(&[LANDLORD, ACME]),
		),
		condition(
			RuleField::Payee,
			RuleConditionOp::DoesNotContain,
			words("amzn"),
		),
		condition(
			RuleField::Payee,
			RuleConditionOp::Is,
			RuleValue::Id { id: None },
		),
		condition(
			RuleField::ImportedPayee,
			RuleConditionOp::Contains,
			words("power"),
		),
		condition(
			RuleField::Amount,
			RuleConditionOp::IsBetween,
			RuleValue::AmountRange {
				num1: -10_000,
				num2: -2_000,
			},
		),
		condition(
			RuleField::AmountOutflow,
			RuleConditionOp::Gt,
			RuleValue::Amount { amount: 10_000 },
		),
		condition(
			RuleField::AmountInflow,
			RuleConditionOp::Gte,
			RuleValue::Amount { amount: 400_000 },
		),
		condition(
			RuleField::Amount,
			RuleConditionOp::IsApprox,
			RuleValue::Amount { amount: -150_000 },
		),
		condition(
			RuleField::Date,
			RuleConditionOp::Is,
			RuleValue::Month {
				month: "2026-10".to_owned(),
			},
		),
		condition(
			RuleField::Date,
			RuleConditionOp::Lt,
			RuleValue::Date {
				date: "2026-09-02".to_owned(),
			},
		),
		condition(RuleField::Account, RuleConditionOp::Is, id(savings)),
		condition(
			RuleField::Transfer,
			RuleConditionOp::Is,
			RuleValue::Bool { value: true },
		),
		condition(
			RuleField::Cleared,
			RuleConditionOp::Is,
			RuleValue::Bool { value: false },
		),
	];
	let mut rules = Vec::new();
	for shape in shapes {
		let made = session.perform(Command::CreateRule {
			draft: draft(
				RuleConditionsOp::And,
				vec![shape.clone()],
				vec![set(RuleField::Notes, words("found"))],
			),
		});
		let made = made.unwrap_or_else(|error| {
			panic!("{shape:?} is a condition Actual's editor builds: {error:?}")
		});
		rules.push((created(&made, EntityKind::Rule), shape));
	}
	let saved = scratch.join("rules.zip");
	export(&session, &saved);
	let ids: Vec<&str> = rules.iter().map(|(id, _)| id.as_str()).collect();
	let theirs = actual_matches(&oracle, &saved, &ids, &scratch);
	let mut wrong = Vec::new();
	for (rule, shape) in &rules {
		let ours: Vec<String> = preview(&session, rule)
			.into_iter()
			.map(|m| m.transaction_id)
			.collect();
		let expected: Vec<String> = serde_json::from_value(theirs[rule.as_str()].clone()).unwrap();
		if ours != expected {
			wrong.push(format!(
				"{shape:?}\n  Veritas {ours:?}\n  Actual  {expected:?}"
			));
		}
	}
	assert!(wrong.is_empty(), "{}", wrong.join("\n"));
	session.close().unwrap();
}

/// The full integrity check and the open snapshot run in the background.
fn settled(session: &Session) {
	for _ in 0..400 {
		if !session.state().unwrap().checking {
			return;
		}
		std::thread::sleep(std::time::Duration::from_millis(25));
	}
	panic!("the background check never finished");
}

fn posts_of(session: &Session, bill: &str) -> Vec<(String, i64)> {
	let mut rows: Vec<(String, i64)> = session
		.register(
			veritas_ffi::RegisterScope::All,
			veritas_ffi::RegisterFilter::default(),
			veritas_ffi::RegisterSort {
				key: veritas_ffi::RegisterSortKey::Date,
				ascending: true,
			},
			0,
			1_000,
		)
		.unwrap()
		.rows
		.into_iter()
		.filter(|r| r.schedule_id.as_deref() == Some(bill))
		.map(|r| (r.date, r.amount))
		.collect();
	rows.sort();
	rows
}

fn newest_snapshot(session: &Session, kind: veritas_ffi::SnapshotKind) -> String {
	session
		.snapshots()
		.unwrap()
		.into_iter()
		.find(|s| s.reason == kind)
		.unwrap_or_else(|| panic!("no {kind:?} snapshot"))
		.id
}

#[test]
fn restoring_a_snapshot_lets_the_service_run_again_without_posting_anything_twice() {
	// R-BILL-2, R-REL-3: a restore clears lastScheduleRun, as Actual's
	// loadBackup leads to. Restoring from before the day's run reposts what
	// the restore took away; restoring from after it posts nothing again.
	let scratch = Scratch::new("restore");
	let base = household_base(&scratch);
	let session = import(&scratch, &base, "veritas");
	see(&session, "2026-11-02");
	settled(&session);
	let before_run = session
		.snapshots()
		.unwrap()
		.first()
		.expect("the open takes a snapshot")
		.id
		.clone();

	session.maintain("2026-11-02".to_owned()).unwrap();
	assert_eq!(
		posts_of(&session, "rent"),
		[("2026-11-02".to_owned(), -150_000)]
	);
	assert_eq!(last_run(&scratch, "veritas").as_deref(), Some("2026-11-02"));

	session.restore_snapshot(before_run).unwrap();
	assert!(
		posts_of(&session, "rent").is_empty(),
		"the restore took the post away"
	);
	assert_eq!(
		last_run(&scratch, "veritas"),
		None,
		"so the day can run again"
	);
	session.maintain("2026-11-02".to_owned()).unwrap();
	assert_eq!(
		posts_of(&session, "rent"),
		[("2026-11-02".to_owned(), -150_000)]
	);

	// Exporting snapshots the budget first, with the rent posted.
	export(&session, &scratch.join("midday.zip"));
	let after_run = newest_snapshot(&session, veritas_ffi::SnapshotKind::Export);
	session.restore_snapshot(after_run).unwrap();
	assert_eq!(last_run(&scratch, "veritas"), None);
	session.maintain("2026-11-02".to_owned()).unwrap();
	assert_eq!(
		posts_of(&session, "rent"),
		[("2026-11-02".to_owned(), -150_000)],
		"the restored budget already paid rent"
	);
	assert_eq!(last_run(&scratch, "veritas").as_deref(), Some("2026-11-02"));

	see(&session, "2026-11-06");
	session.maintain("2026-11-06".to_owned()).unwrap();
	let ours = scratch.join("ours.zip");
	export(&session, &ours);
	session.close().unwrap();

	let Some(oracle) = oracle() else { return };
	let open = json!([{"op": "openBudgetSchedules"}]);
	let ours_later = scratch.join("ours-later.zip");
	actual_apply(&oracle, &ours, open.clone(), &ours_later, "2026-11-10", 60);
	let mut zip = base;
	for (seed, day) in ["2026-11-02", "2026-11-06", "2026-11-10"]
		.iter()
		.enumerate()
	{
		let out = scratch.join(&format!("actual-{day}.zip"));
		actual_apply(&oracle, &zip, open.clone(), &out, day, seed as u32 + 70);
		zip = out;
	}
	let (ours, theirs) = (
		actual_reads(&oracle, &ours_later, "2026-11-10"),
		actual_reads(&oracle, &zip, "2026-11-10"),
	);
	assert_eq!(
		posted(&ours),
		posted(&theirs),
		"nothing Actual alone wouldn't post"
	);
	assert_eq!(schedules(&ours), schedules(&theirs));
}

#[test]
fn an_older_budget_opened_read_only_shows_its_goals_and_bills_and_stays_as_it_was() {
	// 46e3016: a read-only open can't add the D72 goal columns or the
	// schedule labels table, so it reads them as empty and changes nothing.
	let scratch = Scratch::new("old-read-only");
	let session = import(&scratch, &small_zip(), "old");
	see(&session, "2026-11-02");
	let goal = created(
		&perform(
			&session,
			Command::CreateGoal {
				name: "Bike".to_owned(),
				target: 60_000,
				target_date: None,
				icon: None,
				color: None,
			},
		),
		EntityKind::Goal,
	);
	perform(
		&session,
		Command::CloseGoal {
			goal_id: goal.clone(),
			release_to_category_id: None,
			also_move_later: false,
		},
	);
	household_bills(&session);
	perform(
		&session,
		Command::PauseBill {
			bill_id: "gym".to_owned(),
		},
	);
	settled(&session);
	session.close().unwrap();

	let db = sqlite(&scratch, "old");
	db.execute_batch(
		"ALTER TABLE veritas_goal DROP COLUMN activity_seen_through;
		 ALTER TABLE veritas_goal DROP COLUMN activity_seen_total;
		 ALTER TABLE veritas_goal DROP COLUMN close_month_dismissed_available;
		 DROP TABLE veritas_schedule_meta;
		 INSERT OR REPLACE INTO preferences (id, value) VALUES ('budgetType', 'tracking');",
	)
	.unwrap();
	let fingerprint = |db: &rusqlite::Connection| -> Vec<String> {
		let mut rows: Vec<String> = db
			.prepare(
				"SELECT name || ':' || sql FROM sqlite_master WHERE name LIKE 'veritas_%' ORDER BY name",
			)
			.unwrap()
			.query_map([], |row| row.get(0))
			.unwrap()
			.map(Result::unwrap)
			.collect();
		rows.extend(
			db.prepare(
				"SELECT quote(id) || quote(status) || quote(closed_month) FROM veritas_goal ORDER BY id",
			)
			.unwrap()
			.query_map([], |row| row.get::<_, String>(0))
			.unwrap()
			.map(Result::unwrap),
		);
		rows
	};
	let before = fingerprint(&db);
	drop(db);
	let metadata_before = std::fs::read(scratch.join("old").join("metadata.json")).unwrap();

	let session = reopen(&scratch, "old");
	assert_eq!(
		session.state().unwrap().read_only,
		Some(veritas_ffi::ReadOnlyKind::TrackingBudget)
	);
	see(&session, "2026-11-02");
	let goals = session.goals("2026-11-02".to_owned()).unwrap().goals;
	let bike = goals.iter().find(|g| g.id == goal).unwrap();
	assert_eq!(bike.state, veritas_ffi::GoalState::Closed);
	assert!(!bike.new_activity);
	let bills = session.bills("2026-11-02".to_owned()).unwrap();
	assert_eq!(bills.len(), 5);
	assert_ne!(
		bills.iter().find(|b| b.id == "gym").unwrap().state,
		BillState::Active,
		"without its label the paused bill still reads as not active"
	);
	session.upcoming(30, "2026-11-02".to_owned()).unwrap();
	session.rules().unwrap();
	session.maintain("2026-11-02".to_owned()).unwrap();
	assert!(
		posts_of(&session, "rent").is_empty(),
		"a read-only budget posts nothing"
	);
	assert_eq!(session.state().unwrap().schedule_problem, None);
	settled(&session);
	session.close().unwrap();

	assert_eq!(
		fingerprint(&sqlite(&scratch, "old")),
		before,
		"nothing was added or changed"
	);
	assert_eq!(
		std::fs::read(scratch.join("old").join("metadata.json")).unwrap(),
		metadata_before,
		"lastScheduleRun isn't touched either"
	);
}

fn scheduled_rows(session: &Session, schedule: &str) -> Vec<veritas_ffi::RegisterRow> {
	session
		.register(
			veritas_ffi::RegisterScope::Account {
				account_id: CHECKING.to_owned(),
			},
			veritas_ffi::RegisterFilter::default(),
			veritas_ffi::RegisterSort {
				key: veritas_ffi::RegisterSortKey::Date,
				ascending: false,
			},
			0,
			500,
		)
		.unwrap()
		.rows
		.into_iter()
		.filter(|row| row.schedule_id.as_deref() == Some(schedule))
		.collect()
}

#[test]
fn an_auto_posted_about_or_between_bill_reads_as_an_estimate_until_edited_or_cleared() {
	// Ryan's ruling for v12: an About or Between bill posts as Actual does,
	// and its row carries the estimate marker while it still holds the
	// amount Actual's schedule service posts (getScheduledAmount: the
	// number, or Math.round of the range's middle) and isn't cleared. An
	// exact bill never does. Rows Actual posted count the same.
	let scratch = Scratch::new("estimate");
	let setup = import(&scratch, &small_zip(), "setup");
	see(&setup, "2026-10-31");
	let water = bill(
		"Water",
		CITY_POWER,
		BillAmount::Between {
			low: -6_001,
			high: -9_000,
		},
		monthly("2026-11-03", None),
		Some(UTILITIES),
		true,
	);
	let power = bill(
		"Power",
		CITY_POWER,
		BillAmount::About { amount: -9_000 },
		monthly("2026-11-03", None),
		Some(UTILITIES),
		true,
	);
	let rent = bill(
		"Rent",
		LANDLORD,
		BillAmount::Exact { amount: -150_000 },
		monthly("2026-11-03", None),
		Some(RENT),
		true,
	);
	let tie = bill(
		"Sewer",
		CITY_POWER,
		BillAmount::Between {
			low: -1_000,
			high: -1_001,
		},
		monthly("2026-11-03", None),
		Some(UTILITIES),
		true,
	);
	create(&setup, "water", water);
	create(&setup, "tie", tie);
	create(&setup, "power", power);
	create(&setup, "rent", rent);
	let base = scratch.join("base.zip");
	export(&setup, &base);
	setup.close().unwrap();

	let check = |session: &Session, who: &str| {
		let mut water = scheduled_rows(session, "water");
		let mut power = scheduled_rows(session, "power");
		let rent = scheduled_rows(session, "rent");
		assert_eq!(
			(water.len(), power.len(), rent.len()),
			(1, 1, 1),
			"{who} posts each bill once"
		);
		assert_eq!(water[0].amount, -7_500, "{who}: Math.round(-7500.5)");
		let tie = scheduled_rows(session, "tie");
		assert_eq!(
			(tie.len(), tie[0].amount),
			(1, -1_000),
			"{who}: a negative tie rounds toward +∞, as Math.round does"
		);
		assert!(tie[0].is_unconfirmed_estimate, "{who}");
		assert_eq!(power[0].amount, -9_000, "{who}");
		assert!(water[0].is_unconfirmed_estimate, "{who}: Between");
		assert!(power[0].is_unconfirmed_estimate, "{who}: About");
		assert!(
			!rent[0].is_unconfirmed_estimate,
			"{who}: an exact bill never is"
		);
		(water.remove(0), power.remove(0))
	};

	let session = import(&scratch, &base, "veritas");
	see(&session, "2026-11-03");
	session.maintain("2026-11-03".to_owned()).unwrap();
	let (water_row, power_row) = check(&session, "Veritas");

	perform(
		&session,
		Command::EditTransaction {
			transaction_id: water_row.id.clone(),
			transaction: TransactionDraft {
				id: Some(water_row.id.clone()),
				account_id: water_row.account_id.clone(),
				date: water_row.date.clone(),
				amount: -7_312,
				payee_id: water_row.payee_id.clone(),
				payee_name: None,
				category_id: water_row.category_id.clone(),
				notes: water_row.notes.clone(),
				cleared: false,
				splits: Vec::new(),
			},
		},
	);
	assert!(
		!scheduled_rows(&session, "water")[0].is_unconfirmed_estimate,
		"the real amount replaces the estimate"
	);
	perform(
		&session,
		Command::SetCleared {
			transaction_ids: vec![power_row.id.clone()],
			cleared: true,
		},
	);
	assert!(
		!scheduled_rows(&session, "power")[0].is_unconfirmed_estimate,
		"clearing it confirms the amount"
	);
	session.undo().unwrap();
	assert!(scheduled_rows(&session, "power")[0].is_unconfirmed_estimate);
	session.close().unwrap();

	let Some(oracle) = oracle() else { return };
	let theirs = scratch.join("actual-posted.zip");
	actual_apply(
		&oracle,
		&base,
		json!([{"op": "openBudgetSchedules"}]),
		&theirs,
		"2026-11-03",
		7,
	);
	let opened = import(&scratch, &theirs, "actual");
	see(&opened, "2026-11-03");
	check(&opened, "Actual");
	opened.close().unwrap();
}

// Contract v13: what the bill and rule screens show beside the v8 models.

/// What Actual itself says about bills (the oracle's `bill-facts`): linked
/// transactions in Actual's order, `v_schedules` rows and previewed dates.
fn bill_facts(oracle: &Path, zip: &Path, request: Value, today: &str, scratch: &Scratch) -> Value {
	let file = scratch.join("bill-facts.json");
	std::fs::write(&file, request.to_string()).unwrap();
	serde_json::from_slice(&node(
		oracle,
		&["bill-facts", &text(zip), &text(&file), "--today", today],
	))
	.unwrap()
}

fn actual_schedule<'a>(facts: &'a Value, id: &str) -> &'a Value {
	facts["schedules"]
		.as_array()
		.unwrap()
		.iter()
		.find(|row| row["id"] == id)
		.unwrap()
}

fn name_of(dump: &Value, table: &str, id: &Value) -> Option<String> {
	rows(dump, table)
		.iter()
		.find(|row| &row["id"] == id)
		.and_then(|row| row["name"].as_str())
		.map(str::to_owned)
}

const HOUSEHOLD: [&str; 5] = ["rent", "power", "paycheck", "savings", "gym"];

#[test]
fn a_bills_last_payment_is_the_newest_transaction_actual_links_to_it() {
	// v13 lastPaidAmount and lastPaidDate: after a month of posting, Paid with
	// an edited amount and a transfer, each bill's last payment is the first
	// row Actual's own query lists for it in the bill's account.
	let scratch = Scratch::new("last-paid");
	let base = household_base(&scratch);
	let session = import(&scratch, &base, "veritas");
	see(&session, "2026-10-31");
	create(
		&session,
		"phone",
		bill(
			"Phone",
			ACME,
			BillAmount::Exact { amount: -4_500 },
			monthly("2026-12-05", None),
			None,
			false,
		),
	);
	for day in [
		"2026-11-02",
		"2026-11-06",
		"2026-11-10",
		"2026-11-20",
		"2026-12-01",
	] {
		see(&session, day);
		session.maintain(day.to_owned()).unwrap();
	}
	perform(
		&session,
		Command::PayBill {
			bill_id: "power".to_owned(),
			amount: -8_350,
			date: "2026-11-13".to_owned(),
		},
	);
	let last = |id: &str| {
		let bill = bill_on(&session, id, "2026-12-01");
		(bill.last_paid_date, bill.last_paid_amount)
	};
	assert_eq!(last("rent"), (Some("2026-12-01".into()), Some(-150_000)));
	assert_eq!(last("power"), (Some("2026-11-13".into()), Some(-8_350)));
	assert_eq!(last("paycheck"), (Some("2026-11-20".into()), Some(250_000)));
	assert_eq!(
		last("savings"),
		(Some("2026-11-20".into()), Some(-20_000)),
		"the leg in the bill's account, not the other leg"
	);
	assert_eq!(last("phone"), (None, None), "never paid");
	let ours = scratch.join("ours.zip");
	export(&session, &ours);
	session.close().unwrap();
	let Some(oracle) = oracle() else { return };

	let mut ids = HOUSEHOLD.to_vec();
	ids.push("phone");
	let facts = bill_facts(
		&oracle,
		&ours,
		json!({"linked": ids}),
		"2026-12-01",
		&scratch,
	);
	assert!(
		facts["linked"]["savings"]
			.as_array()
			.unwrap()
			.iter()
			.any(|row| row["account"] != CHECKING && row["amount"] == 20_000),
		"Actual links the transfer's other leg to the bill too"
	);
	let session = reopen(&scratch, "veritas");
	for id in ids {
		let theirs = facts["linked"][id]
			.as_array()
			.unwrap()
			.iter()
			.find(|row| row["account"] == CHECKING)
			.map(|row| {
				(
					row["date"].as_str().unwrap().to_owned(),
					row["amount"].as_i64().unwrap(),
				)
			});
		let bill = bill_on(&session, id, "2026-12-01");
		assert_eq!(
			bill.last_paid_date.zip(bill.last_paid_amount),
			theirs,
			"{id}"
		);
	}
	session.close().unwrap();
}

#[test]
fn the_next_dates_preview_matches_actuals_schedule_editor() {
	// v13 previewBillDates: Actual's schedule editor previews a repeat with
	// `schedule/get-upcoming-dates`, whose weekend moves can land before today
	// or twice on one Monday, and a one-time date only when it hasn't passed.
	let today = "2026-11-15";
	let draft = |name: &str, date: BillDate| {
		bill(
			name,
			ACME,
			BillAmount::Exact { amount: -100 },
			date,
			None,
			false,
		)
	};
	let with_patterns = |mut date: BillDate, patterns: Vec<veritas_ffi::BillRepeatPattern>| {
		if let BillDate::Repeat { repeat } = &mut date {
			repeat.patterns = patterns;
		}
		date
	};
	let repeats = [
		draft(
			"Friday before",
			monthly("2026-08-15", Some(BillWeekend::FridayBefore)),
		),
		draft(
			"Daily, Monday after",
			repeat(
				"2026-11-01",
				1,
				BillFrequency::Daily,
				Some(BillWeekend::MondayAfter),
				BillRepeatEnd::Never,
			),
		),
		draft(
			"Three fortnights",
			repeat(
				"2026-11-06",
				2,
				BillFrequency::Weekly,
				None,
				BillRepeatEnd::AfterOccurrences { count: 3 },
			),
		),
		draft(
			"Last day and second Friday",
			with_patterns(
				repeat(
					"2026-09-01",
					1,
					BillFrequency::Monthly,
					None,
					BillRepeatEnd::OnDate {
						date: "2027-01-31".into(),
					},
				),
				vec![
					veritas_ffi::BillRepeatPattern::Day { day: -1 },
					veritas_ffi::BillRepeatPattern::Weekday {
						weekday: veritas_ffi::BillWeekday::Friday,
						nth: 2,
					},
				],
			),
		),
		draft(
			"Leap day",
			repeat(
				"2024-02-29",
				1,
				BillFrequency::Yearly,
				None,
				BillRepeatEnd::Never,
			),
		),
	];
	let count = 4;
	let ours: Vec<Vec<String>> = repeats
		.iter()
		.map(|draft| {
			veritas_ffi::preview_bill_dates(draft.clone(), today.to_owned(), count).unwrap()
		})
		.collect();
	assert_eq!(
		ours[0],
		["2026-11-13", "2026-12-15", "2027-01-15", "2027-02-15"],
		"15 November is a Sunday, so it shows on the Friday before today"
	);
	assert_eq!(
		ours[1],
		["2026-11-16", "2026-11-16", "2026-11-17", "2026-11-18"],
		"today, a Sunday, moves to Monday the 16th along with the 16th itself"
	);
	assert_eq!(
		ours[2],
		["2026-11-20", "2026-12-04"],
		"the third is the last"
	);
	let once = |date: &str| {
		veritas_ffi::preview_bill_dates(
			draft("Once", BillDate::Once { date: date.into() }),
			today.to_owned(),
			count,
		)
		.unwrap()
	};
	assert_eq!(once("2026-11-15"), ["2026-11-15"]);
	assert_eq!(
		once("2026-11-14"),
		Vec::<String>::new(),
		"a date gone by previews nothing"
	);
	let impossible = with_patterns(
		monthly("2026-11-01", None),
		vec![veritas_ffi::BillRepeatPattern::Day { day: 32 }],
	);
	assert!(matches!(
		veritas_ffi::preview_bill_dates(draft("Impossible", impossible), today.to_owned(), count),
		Err(FfiError::Invalid { .. })
	));
	assert!(
		veritas_ffi::preview_bill_dates(repeats[0].clone(), today.to_owned(), 101).is_err(),
		"at most 100 dates"
	);
	let Some(oracle) = oracle() else { return };

	// Actual previews the RecurConfig each draft saves as.
	let scratch = Scratch::new("preview");
	let session = import(&scratch, &small_zip(), "veritas");
	see(&session, today);
	for (n, draft) in repeats.iter().enumerate() {
		create(&session, &format!("preview-{n}"), draft.clone());
	}
	let saved = scratch.join("saved.zip");
	export(&session, &saved);
	session.close().unwrap();
	let dump = actual_reads(&oracle, &saved, today);
	let mut upcoming: Vec<Value> = (0..repeats.len())
		.map(|n| {
			let conditions = conditions_of(&dump, &format!("preview-{n}"));
			let date = conditions
				.as_array()
				.unwrap()
				.iter()
				.find(|c| c["field"] == "date")
				.unwrap()["value"]
				.clone();
			json!({"config": date, "count": count})
		})
		.collect();
	upcoming.push(json!({"config": {
		"start": "2026-11-01", "interval": 1, "frequency": "monthly",
		"patterns": [{"value": 32, "type": "day"}], "skipWeekend": false,
		"weekendSolveMode": "before", "endMode": "never", "endOccurrences": 1,
		"endDate": "2026-11-01"}, "count": count}));
	let facts = bill_facts(
		&oracle,
		&small_zip(),
		json!({"upcoming": upcoming}),
		today,
		&scratch,
	);
	let theirs = facts["upcoming"].as_array().unwrap();
	for (n, dates) in ours.iter().enumerate() {
		assert_eq!(
			&json!(dates),
			&theirs[n],
			"{}",
			repeats[n].name.as_deref().unwrap()
		);
	}
	assert_eq!(theirs[repeats.len()], "Invalid rule");
}

#[test]
fn upcoming_bills_carry_their_amount_kind_account_and_category() {
	// v13 UpcomingBill.amountKind, accountName and categoryName, against
	// `v_schedules` (`_amountOp`, `_account`) and the rule's category action.
	let scratch = Scratch::new("upcoming-fields");
	let base = household_base(&scratch);
	let session = import(&scratch, &base, "veritas");
	see(&session, "2026-10-31");
	create(
		&session,
		"water",
		bill(
			"Water",
			CITY_POWER,
			BillAmount::Between {
				low: -4_000,
				high: -6_000,
			},
			monthly("2026-11-18", None),
			Some(UTILITIES),
			false,
		),
	);
	let upcoming = session.upcoming(30, "2026-11-01".to_owned()).unwrap();
	let first = |id: &str| {
		upcoming
			.iter()
			.find(|bill| bill.bill_id == id)
			.unwrap()
			.clone()
	};
	let kinds = |id: &str| {
		let bill = first(id);
		(bill.amount_kind, bill.category_name, bill.account_name)
	};
	assert_eq!(
		kinds("rent"),
		(
			Some(veritas_ffi::BillAmountKind::Exact),
			Some("Rent".into()),
			Some("Checking".into())
		)
	);
	assert_eq!(kinds("power").0, Some(veritas_ffi::BillAmountKind::About));
	assert_eq!(kinds("water").0, Some(veritas_ffi::BillAmountKind::Between));
	assert_eq!(kinds("savings").1, None, "a bill without a category");
	// v14 paidTodayCounts: the bill editor's answer, on its next occurrence only.
	let mut seen = std::collections::BTreeSet::new();
	for row in &upcoming {
		let editor = session
			.bill(row.bill_id.clone(), "2026-11-01".to_owned())
			.unwrap()
			.paid_today_counts;
		let first = seen.insert(row.bill_id.clone());
		assert_eq!(
			row.paid_today_counts,
			first && editor,
			"{} on {}",
			row.bill_id,
			row.date
		);
	}
	let near = session.upcoming(30, "2026-11-16".to_owned()).unwrap();
	let water_rows: Vec<_> = near.iter().filter(|row| row.bill_id == "water").collect();
	assert_eq!(
		(water_rows[0].date.as_str(), water_rows[0].paid_today_counts),
		("2026-11-18", true),
		"two days out, Paid today pays the next occurrence"
	);
	assert!(water_rows[1..].iter().all(|row| !row.paid_today_counts));
	let ours = scratch.join("ours.zip");
	export(&session, &ours);
	let Some(oracle) = oracle() else { return };

	let dump = actual_reads(&oracle, &ours, "2026-11-01");
	let facts = bill_facts(&oracle, &ours, json!({}), "2026-11-01", &scratch);
	for row in &upcoming {
		let theirs = actual_schedule(&facts, &row.bill_id);
		let kind = match theirs["_amountOp"].as_str() {
			Some("is") => Some(veritas_ffi::BillAmountKind::Exact),
			Some("isapprox") => Some(veritas_ffi::BillAmountKind::About),
			Some("isbetween") => Some(veritas_ffi::BillAmountKind::Between),
			_ => None,
		};
		assert_eq!(row.amount_kind, kind, "{}", row.bill_id);
		assert_eq!(
			row.account_name,
			name_of(&dump, "accounts", &theirs["_account"]),
			"{}",
			row.bill_id
		);
		let category = theirs["_actions"]
			.as_array()
			.unwrap()
			.iter()
			.find(|action| action["op"] == "set" && action["field"] == "category")
			.map(|action| action["value"].clone());
		assert_eq!(
			row.category_name,
			category.and_then(|id| name_of(&dump, "categories", &id)),
			"{}",
			row.bill_id
		);
	}
	session.close().unwrap();
}

/// A stored rule's items with Actual's public field names, as its API takes them.
fn public_items(items: &Value) -> Vec<Value> {
	items
		.as_array()
		.unwrap()
		.iter()
		.map(|item| {
			let mut item = item.clone();
			let field = match item["field"].as_str() {
				Some("acct") => Some("account"),
				Some("description") => Some("payee"),
				_ => None,
			};
			if let Some(field) = field {
				item["field"] = json!(field);
			}
			item
		})
		.collect()
}

#[test]
fn a_schedule_actual_edited_into_a_shape_bills_cant_show_says_why() {
	// v13 readOnlyReason and customParts: Actual's own rule editor turns the
	// household's bills into shapes the bill screen can't show or doesn't
	// show in full. The read-only ones are still listed, with the reason.
	let scratch = Scratch::new("read-only");
	let base = household_base(&scratch);
	let Some(oracle) = oracle() else { return };
	let dump = actual_reads(&oracle, &base, "2026-11-01");
	let rule_of = |schedule: &str| {
		rows(&dump, "schedules")
			.iter()
			.find(|row| row["id"] == schedule)
			.unwrap()["rule"]
			.clone()
	};
	let actions_of = |schedule: &str| {
		let rule = rule_of(schedule);
		let stored = rows(&dump, "rules")
			.iter()
			.find(|row| row["id"] == rule)
			.unwrap();
		serde_json::from_str::<Value>(stored["actions"].as_str().unwrap()).unwrap()
	};
	let edit = |schedule: &str, change: &dyn Fn(&mut Vec<Value>, &mut Vec<Value>)| {
		let mut conditions = public_items(&conditions_of(&dump, schedule));
		let mut actions = public_items(&actions_of(schedule));
		change(&mut conditions, &mut actions);
		json!({"op": "updateRule", "id": rule_of(schedule), "conditions": conditions, "actions": actions})
	};
	let ops = json!([
		edit("rent", &|c, _| c.retain(|c| c["field"] != "amount")),
		edit("power", &|c, _| {
			for c in c.iter_mut().filter(|c| c["field"] == "amount") {
				c["value"] = json!(-8000.5);
			}
		}),
		edit("paycheck", &|c, _| c.retain(|c| c["field"] != "date")),
		edit("savings", &|c, _| {
			for c in c.iter_mut().filter(|c| c["field"] == "date") {
				*c = json!({"op": "is", "field": "date", "value": "2026-12", "type": "date"});
			}
		}),
		edit("gym", &|c, a| {
			c.push(json!({"op": "contains", "field": "notes", "value": "gym", "type": "string"}));
			a.push(
				json!({"op": "set", "field": "notes", "value": "joining fee", "type": "string"}),
			);
		}),
	]);
	let edited = scratch.join("edited.zip");
	actual_apply(&oracle, &base, ops, &edited, "2026-11-01", 3);
	let session = import(&scratch, &edited, "veritas");
	let bills = session.bills("2026-11-01".to_owned()).unwrap();
	let find = |id: &str| bills.iter().find(|bill| bill.id == id).unwrap();
	use veritas_ffi::{BillCustomPart, BillReadOnlyReason};
	for (id, reason) in [
		("rent", BillReadOnlyReason::NoAmount),
		("power", BillReadOnlyReason::UnsupportedAmount),
		("paycheck", BillReadOnlyReason::NoDate),
		("savings", BillReadOnlyReason::UnsupportedDate),
	] {
		let bill = find(id);
		assert_eq!(bill.read_only_reason, Some(reason), "{id}");
		assert_eq!(bill.draft, None, "{id}");
		assert!(bill.custom_parts.is_empty(), "{id}");
	}
	let gym = find("gym");
	assert!(gym.draft.is_some() && gym.custom_rule);
	assert_eq!(gym.read_only_reason, None);
	assert_eq!(
		gym.custom_parts,
		[
			BillCustomPart::ExtraConditions,
			BillCustomPart::ExtraActions
		]
	);
	assert_eq!(find("rent").amount_kind, None);
	assert_eq!(
		find("power").amount_kind,
		Some(veritas_ffi::BillAmountKind::About),
		"its amount still reads as About, though not as whole cents"
	);

	// Actual still lists every one of them, with no amount op where Veritas has no kind.
	let facts = bill_facts(&oracle, &edited, json!({}), "2026-11-01", &scratch);
	for id in HOUSEHOLD {
		let theirs = actual_schedule(&facts, id);
		assert_eq!(
			theirs["_amountOp"].is_null(),
			find(id).amount_kind.is_none(),
			"{id}"
		);
	}
	session.close().unwrap();
}

#[test]
fn skip_is_offered_exactly_when_actuals_skip_moves_the_bill() {
	// v13 canSkip: Actual offers Skip on every schedule that isn't completed
	// (SchedulesTable.tsx:189-194), but it only moves one with a later date
	// (`getNextDateAfter`). Each bill is skipped in Actual on the same file.
	let scratch = Scratch::new("can-skip");
	let session = import(&scratch, &small_zip(), "setup");
	see(&session, "2026-10-31");
	household_bills(&session);
	create(
		&session,
		"dentist",
		bill(
			"Dentist",
			ACME,
			BillAmount::Exact { amount: -12_000 },
			BillDate::Once {
				date: "2026-11-25".into(),
			},
			None,
			false,
		),
	);
	create(
		&session,
		"course",
		bill(
			"Course",
			ACME,
			BillAmount::Exact { amount: -3_000 },
			repeat(
				"2026-10-10",
				1,
				BillFrequency::Monthly,
				None,
				BillRepeatEnd::AfterOccurrences { count: 2 },
			),
			None,
			false,
		),
	);
	perform(
		&session,
		Command::PauseBill {
			bill_id: "paycheck".into(),
		},
	);
	let ids = [
		"rent", "power", "paycheck", "savings", "gym", "dentist", "course",
	];
	let can_skip: Vec<bool> = ids
		.iter()
		.map(|id| bill_on(&session, id, "2026-10-31").can_skip)
		.collect();
	assert_eq!(can_skip, [true, true, false, true, false, false, false]);
	let base = scratch.join("base.zip");
	export(&session, &base);
	for (id, offered) in ids
		.iter()
		.zip(&can_skip)
		.filter(|(id, _)| **id != "paycheck")
	{
		let skipped = session.perform(Command::SkipBill {
			bill_id: (*id).to_owned(),
		});
		assert_eq!(skipped.is_ok(), *offered, "{id}");
	}
	session.close().unwrap();
	let Some(oracle) = oracle() else { return };

	let before = schedules(&actual_reads(&oracle, &base, "2026-10-31"));
	let skips: Vec<Value> = ids
		.iter()
		.map(|id| json!({"op": "skipSchedule", "id": id}))
		.collect();
	let skipped = scratch.join("skipped.zip");
	actual_apply(&oracle, &base, json!(skips), &skipped, "2026-10-31", 4);
	let after = schedules(&actual_reads(&oracle, &skipped, "2026-10-31"));
	for (id, offered) in ids.iter().zip(can_skip) {
		let moved = before[*id]["nextDate"] != after[*id]["nextDate"];
		if before[*id]["completed"] == true {
			assert!(!offered, "{id}: Actual hides Skip on a completed schedule");
		} else {
			assert_eq!(offered, moved, "{id}");
		}
	}
}

fn kind_name(field: RuleField) -> &'static str {
	match field {
		RuleField::ImportedPayee => "imported_payee",
		RuleField::Payee => "payee",
		RuleField::PayeeName => "payee_name",
		RuleField::Account => "account",
		RuleField::Category => "category",
		RuleField::CategoryGroup => "category_group",
		RuleField::Notes => "notes",
		RuleField::Amount => "amount",
		RuleField::AmountInflow => "amount-inflow",
		RuleField::AmountOutflow => "amount-outflow",
		RuleField::Date => "date",
		RuleField::Cleared => "cleared",
		RuleField::Reconciled => "reconciled",
		RuleField::Transfer => "transfer",
		RuleField::Parent => "parent",
		RuleField::Saved => "saved",
	}
}

fn op_name(op: RuleConditionOp) -> &'static str {
	match op {
		RuleConditionOp::Is => "is",
		RuleConditionOp::IsNot => "isNot",
		RuleConditionOp::Contains => "contains",
		RuleConditionOp::DoesNotContain => "doesNotContain",
		RuleConditionOp::Matches => "matches",
		RuleConditionOp::OneOf => "oneOf",
		RuleConditionOp::NotOneOf => "notOneOf",
		RuleConditionOp::HasTags => "hasTags",
		RuleConditionOp::HasAnyTag => "hasAnyTag",
		RuleConditionOp::IsApprox => "isapprox",
		RuleConditionOp::IsBetween => "isbetween",
		RuleConditionOp::Gt => "gt",
		RuleConditionOp::Gte => "gte",
		RuleConditionOp::Lt => "lt",
		RuleConditionOp::Lte => "lte",
		RuleConditionOp::OnBudget => "onBudget",
		RuleConditionOp::OffBudget => "offBudget",
		RuleConditionOp::And => "and",
	}
}

fn action_op_name(op: RuleActionOp) -> &'static str {
	match op {
		RuleActionOp::Set => "set",
		RuleActionOp::PrependNotes => "prepend-notes",
		RuleActionOp::AppendNotes => "append-notes",
		RuleActionOp::DeleteTransaction => "delete-transaction",
		RuleActionOp::SetSplitAmount => "set-split-amount",
		RuleActionOp::LinkSchedule => "link-schedule",
	}
}

fn strings_of(value: &Value) -> Vec<String> {
	value
		.as_array()
		.unwrap()
		.iter()
		.map(|item| item.as_str().unwrap().to_owned())
		.collect()
}

/// A value of `kind` for `field`, naming entities small.zip has.
fn sample(kind: veritas_ffi::RuleValueKind, field: RuleField, group: &str) -> RuleValue {
	use veritas_ffi::RuleValueKind as K;
	let entity = match field {
		RuleField::Payee => LANDLORD,
		RuleField::Account => CHECKING,
		RuleField::CategoryGroup => group,
		_ => RENT,
	};
	match kind {
		K::Id => id(entity),
		K::Ids => RuleValue::Ids {
			ids: vec![entity.to_owned()],
		},
		K::Text => words("rent"),
		K::Texts => RuleValue::Texts {
			texts: vec!["rent".to_owned()],
		},
		K::Amount => RuleValue::Amount { amount: 5_000 },
		K::AmountRange => RuleValue::AmountRange {
			num1: 1_000,
			num2: 5_000,
		},
		K::Date => RuleValue::Date {
			date: "2026-11-01".to_owned(),
		},
		K::DateMonthOrYear => RuleValue::Month {
			month: "2026-11".to_owned(),
		},
		K::Bool => RuleValue::Bool { value: true },
		K::Percent => RuleValue::Percent { percent: 50.0 },
		K::Nothing => RuleValue::Nothing,
	}
}

#[test]
fn rule_choices_are_what_actuals_rule_editor_offers_and_every_one_saves() {
	// v13 ruleChoices: the condition fields and their ops, the action ops and
	// fields and the split-line fields come from Actual's own tables (run from
	// its shared/rules.ts and read from RuleEditor.tsx), and a draft built from
	// each choice saves and loads in Actual.
	let choices = veritas_ffi::rule_choices().unwrap();
	let scratch = Scratch::new("choices");
	let session = import(&scratch, &small_zip(), "veritas");
	see(&session, "2026-11-05");
	let group = session
		.budget_month(
			veritas_ffi::parse_month("2026-11".to_owned()).unwrap(),
			"2026-11-05".to_owned(),
		)
		.unwrap()
		.groups
		.into_iter()
		.find(|group| !group.is_income)
		.unwrap()
		.id;
	let notes = |text: &str| set(RuleField::Notes, words(text));
	let any = || {
		condition(
			RuleField::ImportedPayee,
			RuleConditionOp::Contains,
			words("choice"),
		)
	};
	let mut saved = Vec::new();
	for choice in &choices.condition_fields {
		for op in &choice.ops {
			let value = sample(op.value, choice.field, &group);
			saved.push(create_rule(
				&session,
				draft(
					RuleConditionsOp::And,
					vec![condition(choice.field, op.op, value)],
					vec![notes("condition")],
				),
			));
		}
	}
	for field in &choices.action_fields {
		saved.push(create_rule(
			&session,
			draft(
				RuleConditionsOp::And,
				vec![any()],
				vec![set(field.field, sample(field.value, field.field, &group))],
			),
		));
	}
	for op in &choices.action_ops {
		let Some(kind) = op.value else { continue };
		saved.push(create_rule(
			&session,
			draft(
				RuleConditionsOp::And,
				vec![any()],
				vec![RuleAction {
					op: op.op,
					field: None,
					value: sample(kind, RuleField::Notes, &group),
					split_index: 0,
				}],
			),
		));
	}
	let mut split = Vec::new();
	for (n, kind) in choices.split_amounts.iter().enumerate() {
		let line = n as u32 + 1;
		split.push(RuleAction {
			op: RuleActionOp::SetSplitAmount,
			field: None,
			value: sample(*kind, RuleField::Amount, &group),
			split_index: line,
		});
		for field in &choices.split_line_fields {
			split.push(RuleAction {
				op: RuleActionOp::Set,
				field: Some(field.field),
				value: sample(field.value, field.field, &group),
				split_index: line,
			});
		}
	}
	saved.push(create_rule(
		&session,
		draft(RuleConditionsOp::And, vec![any()], split),
	));
	let ours = scratch.join("ours.zip");
	export(&session, &ours);
	session.close().unwrap();
	let Some(oracle) = oracle() else { return };

	let theirs: Value = serde_json::from_slice(&node(&oracle, &["rule-choices"])).unwrap();
	let fields: Vec<&str> = choices
		.condition_fields
		.iter()
		.map(|choice| kind_name(choice.field))
		.collect();
	assert_eq!(json!(fields), theirs["conditionFields"]);
	assert_eq!(theirs["inflowOutflowDropIsBetween"], true);
	for choice in &choices.condition_fields {
		let field = kind_name(choice.field);
		let signed = field.starts_with("amount-");
		let mut ops = strings_of(&theirs["validOps"][if signed { "amount" } else { field }]);
		if signed {
			ops.retain(|op| op != "isbetween");
		}
		let ours: Vec<&str> = choice.ops.iter().map(|op| op_name(op.op)).collect();
		assert_eq!(ours, ops, "{field}");
	}
	let action_ops: Vec<&str> = choices
		.action_ops
		.iter()
		.map(|op| action_op_name(op.op))
		.collect();
	assert_eq!(json!(action_ops), theirs["actionOps"]);
	assert_eq!(theirs["hidesNameFields"], true);
	let offered: Vec<String> = strings_of(&theirs["actionFields"])
		.into_iter()
		.filter(|field| !field.contains("_name"))
		.collect();
	let action_fields: Vec<&str> = choices
		.action_fields
		.iter()
		.map(|field| kind_name(field.field))
		.collect();
	assert_eq!(action_fields, offered);
	let parent_only = strings_of(&theirs["parentOnlyFields"]);
	let split_fields: Vec<&str> = choices
		.split_line_fields
		.iter()
		.map(|field| kind_name(field.field))
		.collect();
	let offered_split: Vec<&String> = offered
		.iter()
		.filter(|field| !parent_only.contains(field))
		.collect();
	assert_eq!(split_fields, offered_split);
	assert_eq!(
		theirs["allocationMethods"],
		json!(["fixed-amount", "fixed-percent", "remainder"])
	);
	use veritas_ffi::RuleValueKind as K;
	assert_eq!(choices.split_amounts, [K::Amount, K::Percent, K::Nothing]);

	// Actual loads every rule those choices made: `rule-get` throws otherwise.
	let ids: Vec<&str> = saved.iter().map(String::as_str).collect();
	let matches = actual_matches(&oracle, &ours, &ids, &scratch);
	assert_eq!(matches.as_object().unwrap().len(), saved.len());
}

#[test]
fn a_bills_rule_is_listed_with_the_bills_name() {
	// v13 RuleSentence.billName: the rules page names the bill a rule belongs
	// to, the name Actual stores on the schedule.
	let scratch = Scratch::new("bill-names");
	let base = household_base(&scratch);
	let session = import(&scratch, &base, "veritas");
	see(&session, "2026-11-01");
	let editable = create_rule(
		&session,
		draft(
			RuleConditionsOp::And,
			vec![condition(
				RuleField::Notes,
				RuleConditionOp::Contains,
				words("rent"),
			)],
			vec![set(RuleField::Category, id(RENT))],
		),
	);
	let rules = session.rules().unwrap();
	let named: Vec<(String, Option<String>)> = rules
		.iter()
		.filter_map(|rule| match &rule.access {
			RuleAccess::Bill { schedule_id } => Some((schedule_id.clone(), rule.bill_name.clone())),
			_ => None,
		})
		.collect();
	assert_eq!(named.len(), HOUSEHOLD.len());
	assert!(named.contains(&("gym".to_owned(), Some("Gym joining fee".to_owned()))));
	assert_eq!(
		rules
			.iter()
			.find(|rule| rule.id == editable)
			.unwrap()
			.bill_name,
		None
	);
	session.close().unwrap();
	let Some(oracle) = oracle() else { return };

	let dump = actual_reads(&oracle, &base, "2026-11-01");
	for (schedule, name) in named {
		assert_eq!(
			name,
			name_of(&dump, "schedules", &json!(schedule)),
			"{schedule}"
		);
	}
}

#[test]
fn a_preview_flags_reconciled_rows_and_rows_that_may_need_actual() {
	// v13 RuleMatch.reconciled and mayNeedActual: a row Actual reconciled and
	// a row saved while a rule only Actual can run applied to it are flagged
	// in the preview, the way Actual and the register store them.
	let scratch = Scratch::new("match-flags");
	let Some(oracle) = oracle() else { return };
	let base = rules_base(&scratch, &oracle);
	let reconciled = scratch.join("reconciled.zip");
	actual_apply(
		&oracle,
		&base,
		json!([
			{"op": "updateTransaction", "id": "grocer", "fields": {"cleared": true, "reconciled": true}},
			{"op": "createRule", "id": "actual-lunch", "conditions": [
				{"field": "notes", "op": "contains", "value": "lunch", "type": "string"}
			], "actions": [
				{"op": "set", "field": "notes", "value": "", "type": "string",
				 "options": {"template": "{{notes}} (work)"}}
			]}
		]),
		&reconciled,
		"2026-11-05",
		5,
	);
	let session = import(&scratch, &reconciled, "veritas");
	see(&session, "2026-11-05");
	// "actual-lunch" has a template action, which only Actual can run.
	perform(
		&session,
		spend(
			"lunch",
			"2026-11-01",
			-1_850,
			None,
			Some("Diner"),
			Some("lunch out"),
		),
	);
	let rule = create_rule(
		&session,
		draft(
			RuleConditionsOp::Or,
			vec![
				condition(RuleField::Notes, RuleConditionOp::Contains, words("weekly")),
				condition(RuleField::Notes, RuleConditionOp::Contains, words("lunch")),
				condition(
					RuleField::Amount,
					RuleConditionOp::Is,
					RuleValue::Amount { amount: -2_399 },
				),
			],
			vec![set(RuleField::Category, id(DINING_OUT))],
		),
	);
	let matches = preview(&session, &rule);
	let flags = |id: &str| {
		let found = matches.iter().find(|m| m.transaction_id == id).unwrap();
		(found.reconciled, found.may_need_actual)
	};
	let register = scheduled_rows_all(&session);
	let needs = |id: &str| {
		register
			.iter()
			.find(|row| row.id == id)
			.unwrap()
			.needs_actual
			.is_some()
	};
	assert_eq!(flags("grocer"), (true, false));
	assert_eq!(flags("lunch"), (false, needs("lunch")));
	assert!(
		needs("lunch"),
		"the template rule may apply to it in Actual"
	);
	assert_eq!(flags("amzn-1"), (false, false));
	let ours = scratch.join("ours.zip");
	export(&session, &ours);
	session.close().unwrap();
	let dump = actual_reads(&oracle, &ours, "2026-11-05");
	for found in &matches {
		let row = rows(&dump, "transactions")
			.iter()
			.find(|row| row["id"] == found.transaction_id.as_str())
			.unwrap();
		assert_eq!(
			found.reconciled,
			row["reconciled"] == 1,
			"{}",
			found.transaction_id
		);
	}
}

fn scheduled_rows_all(session: &Session) -> Vec<veritas_ffi::RegisterRow> {
	session
		.register(
			veritas_ffi::RegisterScope::Account {
				account_id: CHECKING.to_owned(),
			},
			veritas_ffi::RegisterFilter::default(),
			veritas_ffi::RegisterSort {
				key: veritas_ffi::RegisterSortKey::Date,
				ascending: false,
			},
			0,
			500,
		)
		.unwrap()
		.rows
}

#[test]
fn amount_inflow_and_outflow_conditions_use_actuals_signs() {
	// v13 sign convention: `amount` compares Actual's signed amount, inflow a
	// positive amount as it is, outflow a negative amount as its absolute
	// value. The preview searches as Actual's rule editor does, where 0 is
	// neither money in nor money out.
	let scratch = Scratch::new("signs");
	let Some(oracle) = oracle() else { return };
	let base = rules_base(&scratch, &oracle);
	let session = import(&scratch, &base, "veritas");
	see(&session, "2026-11-05");
	perform(
		&session,
		spend(
			"refund",
			"2026-11-04",
			2_399,
			None,
			Some("AMZN refund"),
			None,
		),
	);
	perform(
		&session,
		spend("zero", "2026-11-04", 0, None, Some("Free sample"), None),
	);
	let amount = |n: i64| RuleValue::Amount { amount: n };
	let rule = |field: RuleField, op: RuleConditionOp, value: RuleValue| {
		create_rule(
			&session,
			draft(
				RuleConditionsOp::And,
				vec![condition(field, op, value)],
				vec![set(RuleField::Category, id(GROCERIES))],
			),
		)
	};
	// small.zip has more rows; these are the ones this test added or knows.
	let known = ["amzn-1", "amzn-2", "grocer", "costco", "refund", "zero"];
	let cases = [
		(
			rule(RuleField::Amount, RuleConditionOp::Is, amount(-2_399)),
			vec!["amzn-1"],
		),
		(
			rule(RuleField::AmountOutflow, RuleConditionOp::Is, amount(2_399)),
			vec!["amzn-1"],
		),
		(
			rule(RuleField::AmountInflow, RuleConditionOp::Is, amount(2_399)),
			vec!["refund"],
		),
		(
			rule(RuleField::AmountOutflow, RuleConditionOp::Gt, amount(5_000)),
			vec!["costco", "amzn-2"],
		),
		(
			rule(RuleField::AmountInflow, RuleConditionOp::Lte, amount(0)),
			vec![],
		),
		(
			rule(RuleField::AmountOutflow, RuleConditionOp::Lte, amount(0)),
			vec![],
		),
		(
			rule(RuleField::Amount, RuleConditionOp::Is, amount(0)),
			vec!["zero"],
		),
	];
	let ours = scratch.join("ours.zip");
	export(&session, &ours);
	let ids: Vec<&str> = cases.iter().map(|(id, _)| id.as_str()).collect();
	let theirs = actual_matches(&oracle, &ours, &ids, &scratch);
	for (rule, expected) in &cases {
		let found: Vec<String> = preview(&session, rule)
			.into_iter()
			.map(|m| m.transaction_id)
			.collect();
		assert_eq!(json!(found), theirs[rule.as_str()], "rule {rule}");
		let ours: Vec<&str> = found
			.iter()
			.map(String::as_str)
			.filter(|id| known.contains(id))
			.collect();
		assert_eq!(ours, *expected, "rule {rule}");
	}
	session.close().unwrap();
}

#[test]
fn a_range_bill_takes_its_smaller_amount_first_and_reads_actuals_either_way() {
	// v13: Actual's editor keeps a range's ends as typed and orders them only
	// when it matches (condition.ts:326-330). Veritas orders a range by size,
	// not sign: it refuses a draft whose first amount is the larger one, and
	// reads a range Actual saved larger amount first smaller first, so that
	// bill stays editable, posts the same and reads "$40 and $60" in Actual.
	let scratch = Scratch::new("range-order");
	let session = import(&scratch, &small_zip(), "veritas");
	see(&session, "2026-11-01");
	let water = |low: i64, high: i64| {
		bill(
			"Water",
			CITY_POWER,
			BillAmount::Between { low, high },
			monthly("2026-11-18", None),
			Some(UTILITIES),
			true,
		)
	};
	let refused = session.perform(Command::CreateBill {
		bill_id: Some("water".to_owned()),
		bill: water(-6_000, -4_000),
	});
	assert!(
		matches!(&refused, Err(FfiError::Invalid { message }) if message == "Put the smaller amount first"),
		"{refused:?}"
	);
	let ours = scratch.join("ours.zip");
	export(&session, &ours);
	session.close().unwrap();
	let Some(oracle) = oracle() else { return };

	let theirs = scratch.join("theirs.zip");
	actual_apply(
		&oracle,
		&ours,
		json!([{"op": "createSchedule", "id": "water", "name": "Water", "postsTransaction": true,
			"payee": CITY_POWER, "account": CHECKING, "amountOp": "isbetween",
			"amount": {"num1": -6_000, "num2": -4_000},
			"date": {"start": "2026-11-18", "interval": 1, "frequency": "monthly", "patterns": [],
				"skipWeekend": false, "weekendSolveMode": "before", "endMode": "never",
				"endOccurrences": 1, "endDate": "2026-11-18"}}]),
		&theirs,
		"2026-11-01",
		6,
	);
	let session = import(&scratch, &theirs, "edited");
	see(&session, "2026-11-01");
	let read = bill_on(&session, "water", "2026-11-01");
	assert_eq!(
		read.draft.as_ref().unwrap().amount,
		BillAmount::Between {
			low: -4_000,
			high: -6_000
		}
	);
	assert_eq!(read.scheduled_amount, Some(-5_000));
	let mut draft = read.draft.unwrap();
	draft.name = Some("Water and sewer".to_owned());
	perform(
		&session,
		Command::EditBill {
			bill_id: "water".to_owned(),
			bill: draft,
		},
	);
	see(&session, "2026-11-18");
	session.maintain("2026-11-18".to_owned()).unwrap();
	let after = scratch.join("posted.zip");
	export(&session, &after);
	session.close().unwrap();
	let dump = actual_reads(&oracle, &after, "2026-11-18");
	let rows: Vec<Value> = posted(&dump)
		.into_iter()
		.filter(|row| row["schedule"] == "water")
		.collect();
	assert_eq!(rows.len(), 1);
	assert_eq!(rows[0]["amount"], -5_000, "the middle, as Actual posts it");
	let conditions = conditions_of(&dump, "water");
	let amount = conditions
		.as_array()
		.unwrap()
		.iter()
		.find(|c| c["field"] == "amount")
		.unwrap();
	assert_eq!(amount["value"], json!({"num1": -4_000, "num2": -6_000}));
}
