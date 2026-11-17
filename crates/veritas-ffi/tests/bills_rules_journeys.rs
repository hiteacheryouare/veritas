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
			low: -1_001,
			high: -1_000,
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
