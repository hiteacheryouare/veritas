//! R-REL-7 for the rule and schedule JSON a user's Actual file hands Veritas:
//! whatever sits in `rules.conditions` and `rules.actions` parses to a typed
//! gap or a value, and every consumer of a parsed value (evaluating it on a
//! transaction, expanding a recurrence, the daily schedule service, the bill
//! screen) answers or refuses without panicking or hanging.
//!
//! Inputs grow from real stored shapes (fixtures/synthetic/all-features.zip
//! and the shapes Actual's schedule editor writes), mutated with the values a
//! damaged, hand-edited or future-version file can hold.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::sync::mpsc;
use std::time::Duration;

use common::Rng;
use serde_json::{Value, json};
use veritas_domain::{
	Account, AccountId, BudgetData, BudgetType, CivilDate, ConditionsOp, Money, Payee, PayeeId,
	Rule, RuleId, RuleStage, Schedule, ScheduleId, ScheduleJsonPaths,
};
use veritas_rules::{
	Action, BudgetPayees, Condition, DateValue, FieldValue, Op, RuleBook, RuleTransaction,
	advance_schedules, bill_dates, bill_status, next_date, next_date_after, next_date_from,
	parse_date_value, read_bill, schedule_dates, upcoming_dates,
};

const ACCOUNT: &str = "0a1c000f-c328-42b8-8966-a4be59a2504b";
const PAYEE: &str = "b0cf23b5-c753-4920-bd28-6ba8860df316";
const SCHEDULE: &str = "a03f874c-fde2-4296-b515-fa2f0fb33c4b";

/// Condition items as Actual stores them, from all-features.zip and the
/// filter and schedule editors.
const CONDITIONS: &[&str] = &[
	r#"{"field":"imported_description","op":"contains","value":"CORNER GROCER","type":"imported_payee"}"#,
	r#"{"field":"description","op":"is","value":"75aba692-9ea4-443c-876d-0b29acecd8db","type":"id"}"#,
	r#"{"field":"amount","op":"lt","value":-100000,"type":"number"}"#,
	r#"{"op":"is","field":"acct","value":"0a1c000f-c328-42b8-8966-a4be59a2504b"}"#,
	r#"{"op":"isapprox","field":"date","value":{"start":"2025-11-03","frequency":"monthly","interval":1,"patterns":[],"skipWeekend":false,"weekendSolveMode":"after","endMode":"never"}}"#,
	r#"{"op":"isapprox","field":"amount","value":-6999}"#,
	r#"{"op":"isbetween","field":"amount","value":{"num1":-5000,"num2":-4000}}"#,
	r#"{"op":"isapprox","field":"date","value":"2026-12-05"}"#,
	r#"{"op":"is","field":"date","value":"2026-11"}"#,
	r#"{"op":"is","field":"date","value":"2026"}"#,
	r#"{"op":"gte","field":"date","value":"2026-01-31"}"#,
	r#"{"field":"amount","op":"gt","value":0,"type":"number","options":{"inflow":true}}"#,
	r#"{"field":"amount","op":"is","value":4210,"type":"number","options":{"outflow":true}}"#,
	r#"{"field":"notes","op":"matches","value":"^rent\\b.*(june|july)","type":"string"}"#,
	r#"{"field":"category","op":"oneOf","value":["fc093a41-2d11-4d98-a211-e5db92205f23","fffcd498-80e7-4984-b378-87cee6b5625c"],"type":"id"}"#,
	r#"{"field":"imported_description","op":"oneOf","value":["Amazon","AMZN Mktp"],"type":"imported_payee"}"#,
	r##"{"field":"notes","op":"hasTags","value":"#food #travel","type":"string"}"##,
	r#"{"field":"cleared","op":"is","value":true,"type":"boolean"}"#,
	r#"{"field":"account","op":"onBudget","value":""}"#,
	r#"{"field":"payee_name","op":"doesNotContain","value":"refund","type":"string"}"#,
];

/// Action items as Actual stores them.
const ACTIONS: &[&str] = &[
	r#"{"field":"description","op":"set","value":"c36be19d-d59f-44bc-9478-8389389f5aa4","type":"id"}"#,
	r#"{"field":"category","op":"set","value":"fffcd498-80e7-4984-b378-87cee6b5625c","type":"id"}"#,
	r#"{"field":"notes","op":"set","value":"","type":"string","options":{"template":"{{payee_name}} paid on {{date}}"}}"#,
	r#"{"op":"append-notes","value":" #reimbursable","type":"string"}"#,
	r#"{"op":"prepend-notes","value":"Lunch ","type":"string"}"#,
	r#"{"field":"amount","op":"set","value":0,"type":"number","options":{"formula":"=amount / 2"}}"#,
	r#"{"op":"link-schedule","value":"a03f874c-fde2-4296-b515-fa2f0fb33c4b"}"#,
	r#"{"op":"set-split-amount","value":1000,"options":{"splitIndex":1,"method":"fixed-amount"}}"#,
	r#"{"op":"set-split-amount","value":10,"options":{"splitIndex":2,"method":"fixed-percent"}}"#,
	r#"{"op":"set-split-amount","value":null,"options":{"splitIndex":3,"method":"remainder"}}"#,
	r#"{"field":"category","op":"set","value":"rent","options":{"splitIndex":3}}"#,
	r#"{"field":"payee_name","op":"set","value":"Corner Grocer","type":"string"}"#,
	r#"{"field":"cleared","op":"set","value":true,"type":"boolean"}"#,
	r#"{"field":"date","op":"set","value":"2026-11-30","type":"date"}"#,
	r#"{"field":"acct","op":"set","value":"2cf26d00-3bc3-4cd9-a05c-5d9552dd0e63","type":"id"}"#,
	r#"{"op":"delete-transaction","value":null}"#,
];

/// Recurrence configs as the schedule editor writes them.
const RECURRENCES: &[&str] = &[
	r#"{"start":"2025-11-03","frequency":"monthly","interval":1,"patterns":[],"skipWeekend":false,"weekendSolveMode":"after","endMode":"never"}"#,
	r#"{"start":"2025-11-10","frequency":"monthly","interval":1,"patterns":[],"skipWeekend":true,"weekendSolveMode":"before","endMode":"never"}"#,
	r#"{"start":"2026-01-31","frequency":"monthly","interval":1,"patterns":[],"skipWeekend":false,"weekendSolveMode":"after","endMode":"after_n_occurrences","endOccurrences":24}"#,
	r#"{"start":"2024-02-29","frequency":"yearly","interval":1,"patterns":[],"skipWeekend":false,"weekendSolveMode":"after","endMode":"on_date","endDate":"2030-12-31"}"#,
	r#"{"start":"2025-11-01","frequency":"monthly","interval":1,"patterns":[{"type":"day","value":1},{"type":"day","value":15}],"skipWeekend":true,"weekendSolveMode":"before","endMode":"never"}"#,
	r#"{"start":"2026-03-06","frequency":"monthly","interval":2,"patterns":[{"type":"FR","value":-1},{"type":"MO","value":1}],"skipWeekend":false,"weekendSolveMode":"after","endMode":"never"}"#,
	r#"{"start":"2026-01-05","frequency":"weekly","interval":2,"patterns":[],"skipWeekend":false,"weekendSolveMode":"after","endMode":"never"}"#,
	r#"{"start":"2026-11-09","frequency":"daily","interval":3,"skipWeekend":true,"weekendSolveMode":"after","endMode":"after_n_occurrences","endOccurrences":10}"#,
];

/// Every key the parsers look up, so mutations land where they matter.
const KEYS: &[&str] = &[
	"op",
	"field",
	"value",
	"type",
	"options",
	"frequency",
	"start",
	"interval",
	"endMode",
	"endOccurrences",
	"endDate",
	"patterns",
	"skipWeekend",
	"weekendSolveMode",
	"splitIndex",
	"method",
	"template",
	"formula",
	"inflow",
	"outflow",
	"num1",
	"num2",
];

const FIELDS: &[&str] = &[
	"imported_description",
	"imported_payee",
	"description",
	"payee",
	"payee_name",
	"acct",
	"account",
	"category",
	"category_group",
	"notes",
	"amount",
	"date",
	"cleared",
	"reconciled",
	"transfer",
	"parent",
	"saved",
	"",
];

const OPS: &[&str] = &[
	"is",
	"isNot",
	"contains",
	"doesNotContain",
	"matches",
	"oneOf",
	"notOneOf",
	"hasTags",
	"hasAnyTag",
	"isapprox",
	"isbetween",
	"gt",
	"gte",
	"lt",
	"lte",
	"onBudget",
	"offBudget",
	"and",
	"set",
	"set-split-amount",
	"link-schedule",
	"prepend-notes",
	"append-notes",
	"delete-transaction",
	"IS",
];

/// What a damaged, hand-edited or newer file can hold where Actual writes a
/// well-formed value.
fn hostile(rng: &mut Rng) -> Value {
	match rng.below(52) {
		0 => Value::Null,
		1 => json!(true),
		2 => json!(false),
		3 => json!(0),
		4 => json!(-1),
		5 => json!(0.5),
		6 => json!(1e308),
		7 => json!(-1e308),
		8 => json!(9_007_199_254_740_993_u64),
		9 => json!(u64::MAX),
		10 => json!(i64::MIN),
		11 => json!(31),
		12 => json!(-32),
		13 => json!(""),
		14 => json!("0"),
		15 => json!("99999999"),
		16 => json!("20261340"),
		17 => json!("2026-02-30"),
		18 => json!("2026-13-40"),
		19 => json!("0000-00-00"),
		20 => json!("262142-12-31"),
		21 => json!("275760-09-13"),
		22 => json!("1-1-1"),
		23 => json!("-2026-01-01"),
		24 => json!("+002026-01-01"),
		25 => json!("2026-W01-1"),
		26 => json!("never"),
		27 => json!("after_n_occurrences"),
		28 => json!("on_date"),
		29 => json!(*rng.pick(&["daily", "weekly", "monthly", "yearly", "hourly"])),
		30 => json!(*rng.pick(&["before", "after", "Before", ""])),
		31 => json!("🙂 Café"),
		32 => json!("\u{0}\u{FEFF}\u{202E}"),
		33 => json!("x".repeat(20_000)),
		34 => json!(*rng.pick(&["(", "[a-", "\\B", "(?<n>a)\\k<n>", "a{99999}", "(a+)+$"])),
		35 => json!([]),
		36 => json!({}),
		37 => json!([null]),
		38 => json!([{"type": "day", "value": 31}, {"type": "day", "value": -31}]),
		39 => json!([{"type": "FR", "value": 5}, {"type": "SU", "value": -5}]),
		40 => json!([{"type": "day", "value": 0}]),
		41 => json!([{"type": "XX", "value": 1}, {"type": 7}]),
		42 => json!({"num1": 1e308, "num2": -1e308}),
		43 => json!({"num1": "a", "num2": null}),
		44 => json!(
			(1..=31)
				.map(|day| json!({"type": "day", "value": day}))
				.collect::<Vec<_>>()
		),
		45 => nested(rng.below(110) as usize + 1),
		46 => json!(*rng.pick(FIELDS)),
		47 => json!(*rng.pick(OPS)),
		48 => json!(9_007_199_254_740_991_i64),
		49 => json!(4_294_967_296_u64),
		50 => json!(-0.0),
		_ => json!(rng.next() as i64 >> rng.below(64)),
	}
}

/// An array nested `depth` deep, inside serde_json's 128-level parse limit
/// once it sits in a rule's array.
fn nested(depth: usize) -> Value {
	(0..depth).fold(json!(1), |inner, _| json!([inner]))
}

/// Visits every node, parents before children, so a mutation can pick one
/// by its position.
fn count(value: &Value) -> usize {
	1 + match value {
		Value::Array(items) => items.iter().map(count).sum(),
		Value::Object(entries) => entries.values().map(count).sum(),
		_ => 0,
	}
}

fn node_at(value: &mut Value, mut at: usize) -> &mut Value {
	fn walk<'a>(value: &'a mut Value, at: &mut usize) -> Option<&'a mut Value> {
		if *at == 0 {
			return Some(value);
		}
		*at -= 1;
		match value {
			Value::Array(items) => items.iter_mut().find_map(|item| walk(item, at)),
			Value::Object(entries) => entries.values_mut().find_map(|item| walk(item, at)),
			_ => None,
		}
	}
	let total = count(value);
	at %= total;
	walk(value, &mut at).unwrap()
}

fn mutate(rng: &mut Rng, value: &mut Value) {
	let steps = if rng.chance(70) { 1 } else { 2 + rng.below(3) };
	for _ in 0..steps {
		let at = rng.below(count(value) as u64) as usize;
		let node = node_at(value, at);
		let choice = rng.below(7);
		let handled = match &mut *node {
			Value::Object(entries) => match choice {
				0 => {
					entries.insert((*rng.pick(KEYS)).to_owned(), hostile(rng));
					true
				}
				1 if !entries.is_empty() => {
					let key = entries
						.keys()
						.nth(rng.below(entries.len() as u64) as usize)
						.unwrap()
						.clone();
					entries.remove(&key);
					true
				}
				2 => {
					entries.insert("op".to_owned(), json!(*rng.pick(OPS)));
					true
				}
				3 => {
					entries.insert("field".to_owned(), json!(*rng.pick(FIELDS)));
					true
				}
				_ => false,
			},
			Value::Array(items) if choice == 4 => {
				let extra = match items.first() {
					Some(first) if rng.chance(50) => first.clone(),
					_ => hostile(rng),
				};
				items.push(extra);
				true
			}
			_ => false,
		};
		if !handled {
			*node = hostile(rng);
		}
	}
}

fn seed(rng: &mut Rng, pool: &[&str]) -> Value {
	serde_json::from_str(rng.pick(pool)).unwrap()
}

/// Runs `work` on its own thread and fails the test, naming `input`, if it
/// doesn't finish in time, so a hang is a red test instead of a stuck run.
fn within<T: Send + 'static>(input: &str, work: impl FnOnce() -> T + Send + 'static) -> T {
	let (done, finished) = mpsc::channel();
	let worker = std::thread::spawn(move || {
		let result = work();
		let _ = done.send(());
		result
	});
	match finished.recv_timeout(Duration::from_secs(20)) {
		Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => match worker.join() {
			Ok(result) => result,
			Err(panic) => {
				eprintln!("panicked on input: {input}");
				std::panic::resume_unwind(panic)
			}
		},
		Err(mpsc::RecvTimeoutError::Timeout) => {
			panic!("still running after 20 s on input: {input}")
		}
	}
}

fn day(text: &str) -> CivilDate {
	CivilDate::from(chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap())
}

fn today() -> CivilDate {
	day("2026-11-20")
}

fn transaction(imported_payee: &str, amount: i64, date: &str) -> RuleTransaction {
	RuleTransaction {
		account: AccountId::new(ACCOUNT),
		date: day(date),
		amount: Money::from_minor(amount),
		payee: Some(PayeeId::new(PAYEE)),
		category: None,
		notes: Some("rent june #food".into()),
		imported_payee: Some(imported_payee.into()),
		cleared: Some(false),
		reconciled: Some(false),
		schedule: None,
		deleted: false,
		splits: Vec::new(),
		is_child: false,
	}
}

/// Everything a parsed recurrence or date feeds: the schedule list's next
/// date, the editor's preview, the forecast's dates over two years.
fn expand(date: &DateValue) {
	let start = today();
	let end = day("2028-11-20");
	let _ = next_date(date, start);
	let _ = next_date_after(date, start);
	let _ = next_date_from(date, start);
	if let Ok(dates) = schedule_dates(date, start, Some(end), Some(500)) {
		assert!(dates.len() <= 500, "{} dates", dates.len());
		assert!(
			dates.iter().all(|day| *day >= start && *day <= end),
			"a date outside {start}..={end}: {dates:?}"
		);
		assert!(
			dates.windows(2).all(|pair| pair[0] < pair[1]),
			"dates out of order: {dates:?}"
		);
	}
	if let Ok(dates) = upcoming_dates(date, start, 12) {
		assert!(dates.len() <= 12, "{} upcoming dates", dates.len());
	}
	if let DateValue::Recur(recurrence) = date {
		let _ = recurrence.first_on_or_after(start.naive());
		let _ = recurrence.occurs_on(start.naive());
	}
}

#[test]
fn a_stored_condition_parses_to_a_condition_or_a_gap_and_evaluates_without_panicking() {
	let mut rng = Rng(0x5EED_C0DE_0000_0001);
	let mut parsed = 0;
	for _ in 0..3_000 {
		let mut item = seed(&mut rng, CONDITIONS);
		mutate(&mut rng, &mut item);
		let input = item.to_string();
		let ok = within(&input, move || {
			let Ok(condition) = Condition::parse(&item) else {
				return false;
			};
			for value in [
				FieldValue::Absent,
				FieldValue::Null,
				FieldValue::Text(String::new()),
				FieldValue::Text("2026-11-09".into()),
				FieldValue::Text("Rent June 🙂 #food".into()),
				FieldValue::Number(-4210.0),
				FieldValue::Number(9_007_199_254_740_991.0),
				FieldValue::Bool(true),
			] {
				for off_budget in [None, Some(false), Some(true)] {
					let _ = condition.eval(value.clone(), off_budget);
				}
			}
			if let veritas_rules::ConditionValue::Date(date) = &condition.value {
				expand(date);
			}
			true
		});
		parsed += usize::from(ok);
	}
	assert!(
		parsed > 450,
		"only {parsed} of 3000 mutated conditions parsed"
	);
}

#[test]
fn a_stored_action_parses_to_an_action_or_a_gap_and_applies_without_panicking() {
	let mut rng = Rng(0x5EED_C0DE_0000_0002);
	let mut parsed = 0;
	for _ in 0..3_000 {
		let mut item = seed(&mut rng, ACTIONS);
		mutate(&mut rng, &mut item);
		let input = item.to_string();
		let ok = within(&input, move || {
			let Ok(action) = Action::parse(&item) else {
				return false;
			};
			let _ = action.split();
			action.apply(&mut transaction("Corner Grocer", -4210, "2026-11-09"));
			true
		});
		parsed += usize::from(ok);
	}
	assert!(parsed > 450, "only {parsed} of 3000 mutated actions parsed");
}

#[test]
fn a_stored_recurrence_parses_or_refuses_and_its_dates_stay_in_their_window() {
	let mut rng = Rng(0x5EED_C0DE_0000_0003);
	let mut parsed = 0;
	for _ in 0..3_000 {
		let mut config = seed(&mut rng, RECURRENCES);
		mutate(&mut rng, &mut config);
		let op = *rng.pick(&[Op::IsApprox, Op::Is, Op::Gte, Op::Lt]);
		let input = format!("{op:?} {config}");
		let ok = within(&input, move || {
			let Ok(date) = parse_date_value(op, Some(&config)) else {
				return false;
			};
			expand(&date);
			true
		});
		parsed += usize::from(ok);
	}
	assert!(
		parsed > 450,
		"only {parsed} of 3000 mutated recurrences parsed"
	);
}

#[test]
fn a_date_condition_value_from_anywhere_is_a_date_or_a_refusal() {
	// The date shapes a damaged file can hold where Actual writes YYYY-MM-DD,
	// including the integer forms other tools write (20261340 is month 13).
	let texts = [
		"0",
		"99999999",
		"20261340",
		"2026-13-40",
		"0000-00-00",
		"9999-12-31",
		"262142-12-31",
		"275760-09-13",
		"-0001-01-01",
		"2026-02-29",
		"2028-02-29",
		"2026-1",
		"2026-01-",
		"--",
		"",
		"１２３４",
		"2026-11-20T10:00:00Z",
	];
	for op in [Op::Is, Op::IsApprox, Op::Gt, Op::Lte] {
		for text in texts {
			let input = format!("{op:?} {text:?}");
			within(&input, move || {
				if let Ok(date) = parse_date_value(op, Some(&json!(text))) {
					expand(&date);
				}
			});
		}
		for value in [
			json!(0),
			json!(20_261_340),
			json!(99_999_999),
			json!(null),
			json!([]),
			json!({}),
			json!({"frequency": null}),
		] {
			let input = format!("{op:?} {value}");
			within(&input, move || {
				let _ = parse_date_value(op, Some(&value));
			});
		}
	}
}

#[test]
fn recurrences_at_the_edges_of_the_calendar_answer_without_hanging() {
	// Each one is a shape a damaged or hand-edited file could hold, at the
	// limits the parser accepts: the earliest and latest years, the largest
	// counts and intervals JS still calls whole numbers.
	let configs = [
		json!({"start": "2026-11-09", "frequency": "daily", "endMode": "after_n_occurrences", "endOccurrences": 9_007_199_254_740_991_i64}),
		json!({"start": "0100-01-01", "frequency": "monthly", "endMode": "after_n_occurrences", "endOccurrences": 100_000_000}),
		json!({"start": "262142-12-31", "frequency": "daily"}),
		json!({"start": "262142-12-01", "frequency": "monthly", "patterns": [{"type": "day", "value": -1}]}),
		json!({"start": "2026-01-01", "frequency": "daily", "interval": 9_007_199_254_740_991_i64}),
		json!({"start": "2026-01-01", "frequency": "monthly", "interval": 9_007_199_254_740_991_i64, "patterns": [{"type": "FR", "value": -1}]}),
		json!({"start": "2026-02-01", "frequency": "monthly", "interval": 12, "patterns": [{"type": "day", "value": 30}]}),
		json!({"start": "2026-02-01", "frequency": "monthly", "interval": 12, "patterns": [{"type": "day", "value": 30}], "endMode": "on_date", "endDate": "262000-01-01"}),
		json!({"start": "2026-04-01", "frequency": "monthly", "interval": 12, "patterns": [{"type": "FR", "value": 31}]}),
		json!({"start": "2026-04-01", "frequency": "monthly", "interval": 12, "patterns": [{"type": "FR", "value": 31}], "endMode": "after_n_occurrences", "endOccurrences": 5}),
		json!({"start": "2000-02-29", "frequency": "yearly", "interval": 100}),
		json!({"start": "2000-02-29", "frequency": "yearly", "interval": 100, "endMode": "on_date", "endDate": "262142-12-31"}),
		json!({"start": "2026-11-21", "frequency": "weekly", "skipWeekend": true, "weekendSolveMode": "sideways"}),
		json!({"start": "99", "frequency": "yearly", "endMode": "on_date", "endDate": "0"}),
	];
	for config in configs {
		let input = config.to_string();
		within(&input, move || {
			for op in [Op::IsApprox, Op::Is] {
				if let Ok(date) = parse_date_value(op, Some(&config)) {
					expand(&date);
				}
			}
		});
	}
}

fn rule(id: &str, conditions: String, actions: String, op: ConditionsOp) -> Rule {
	Rule {
		id: RuleId::new(id),
		stage: RuleStage::Default,
		conditions_op: op,
		conditions_json: conditions,
		actions_json: actions,
		tombstone: false,
	}
}

/// A budget with one checking account, the bill's payee and the bill itself,
/// whose rule is `conditions` (the schedule slots) and `actions`.
fn budget_with(rules: Vec<Rule>) -> BudgetData {
	let mut budget = BudgetData::empty(BudgetType::Envelope);
	budget.accounts.insert(
		AccountId::new(ACCOUNT),
		Account {
			id: AccountId::new(ACCOUNT),
			name: "Checking".into(),
			off_budget: false,
			closed: false,
			group: None,
			sort_order: 0.0,
			tombstone: false,
			last_reconciled: None,
		},
	);
	budget.payees.insert(
		PayeeId::new(PAYEE),
		Payee {
			id: PayeeId::new(PAYEE),
			name: "Landlord".into(),
			transfer_account: None,
			favorite: false,
			learn_categories: true,
			tombstone: false,
		},
	);
	budget
		.payee_mapping
		.insert(PayeeId::new(PAYEE), PayeeId::new(PAYEE));
	let schedule_rule = rules[0].id.clone();
	for rule in rules {
		budget.rules.insert(rule.id.clone(), rule);
	}
	budget.schedules.insert(
		ScheduleId::new(SCHEDULE),
		Schedule {
			id: ScheduleId::new(SCHEDULE),
			name: Some("Rent".into()),
			rule: schedule_rule,
			posts_transaction: true,
			completed: false,
			next_date: Some(day("2026-11-03")),
			tombstone: false,
		},
	);
	budget.schedule_json_paths.insert(
		ScheduleId::new(SCHEDULE),
		ScheduleJsonPaths {
			payee: Some("$[0]".into()),
			account: Some("$[1]".into()),
			amount: Some("$[3]".into()),
			date: Some("$[2]".into()),
		},
	);
	budget
}

/// A rule's stored text: usually a mutated array of real items, sometimes
/// the damaged text a truncated write or another tool leaves.
fn stored_items(rng: &mut Rng, base: Vec<Value>, pool: &[&str]) -> String {
	let mut items = Value::Array(base);
	if let Value::Array(list) = &mut items {
		for _ in 0..rng.below(3) {
			list.push(seed(rng, pool));
		}
	}
	mutate(rng, &mut items);
	let text = items.to_string();
	match rng.below(12) {
		0 => {
			let keep = rng.below(text.chars().count() as u64) as usize;
			text.chars().take(keep).collect()
		}
		1 => format!("{}{}", "[".repeat(200), "]".repeat(200)),
		2 => "{\"op\":\"is\"}".to_owned(),
		3 => String::new(),
		_ => text,
	}
}

#[test]
fn a_rule_book_built_from_hostile_rows_runs_and_plans_bills_without_panicking() {
	let mut rng = Rng(0x5EED_C0DE_0000_0004);
	let mut ran = 0;
	for case in 0..1_000 {
		let schedule_conditions = vec![
			json!({"op": "is", "field": "description", "value": PAYEE}),
			json!({"op": "is", "field": "acct", "value": ACCOUNT}),
			json!({"op": "isapprox", "field": "date", "value": seed(&mut rng, RECURRENCES)}),
			json!({"op": "is", "field": "amount", "value": -180_000}),
		];
		let mut rules = vec![rule(
			"schedule-rule",
			stored_items(&mut rng, schedule_conditions, CONDITIONS),
			stored_items(
				&mut rng,
				vec![json!({"op": "link-schedule", "value": SCHEDULE})],
				ACTIONS,
			),
			ConditionsOp::And,
		)];
		for n in 0..rng.below(3) {
			let conditions = vec![seed(&mut rng, CONDITIONS)];
			let actions = vec![seed(&mut rng, ACTIONS)];
			rules.push(rule(
				&format!("rule-{n}"),
				stored_items(&mut rng, conditions, CONDITIONS),
				stored_items(&mut rng, actions, ACTIONS),
				if rng.chance(30) {
					ConditionsOp::Or
				} else {
					ConditionsOp::And
				},
			));
		}
		let input = format!(
			"case {case}: {:?}",
			rules
				.iter()
				.map(|rule| (&rule.conditions_json, &rule.actions_json))
				.collect::<Vec<_>>()
		);
		let budget = budget_with(rules);
		let book_ran = within(&input, move || {
			let book = RuleBook::new(&budget);
			for txn in [
				transaction("Corner Grocer", -180_000, "2026-11-03"),
				transaction("split me", -4210, "2026-11-09"),
				transaction("", 9_007_199_254_740_991, "1900-01-01"),
			] {
				let mut payees = BudgetPayees::new(&budget, || PayeeId::new("made"));
				let outcome = book.run(&txn, &budget, &mut payees);
				assert_eq!(
					outcome.transaction.is_none(),
					!outcome.undetermined.is_empty(),
					"a run writes its result exactly when no rule is undetermined"
				);
			}
			let mut payees = BudgetPayees::new(&budget, || PayeeId::new("made"));
			let _ = advance_schedules(&book, &budget, &mut payees, today());
			let schedule = &budget.schedules[&ScheduleId::new(SCHEDULE)];
			let _ = read_bill(schedule, budget.rules.get(&schedule.rule), &budget);
			let id = ScheduleId::new(SCHEDULE);
			let _ = bill_status(&budget, &id, 7, today());
			if let Ok(dates) = bill_dates(&budget, &id, today(), day("2027-11-20")) {
				assert!(
					dates.len() <= 366 * 2,
					"{} bill dates in a year",
					dates.len()
				);
			}
			true
		});
		ran += usize::from(book_ran);
	}
	assert_eq!(ran, 1_000);
}

#[test]
fn a_daily_bill_counted_from_long_ago_lists_five_years_of_dates_without_stalling() {
	// Known bug, left failing: a bill that ends "after N occurrences" walks
	// its count from the start date again for every date it lists, so the
	// work grows with (days since start) x (dates in the window). The bill
	// detail screen asks for five years of dates (`bill_detail` in
	// veritas-ffi), and for this bill that takes minutes.
	let conditions = json!([
		{"op": "is", "field": "description", "value": PAYEE},
		{"op": "is", "field": "acct", "value": ACCOUNT},
		{"op": "isapprox", "field": "date", "value": {
			"start": "1900-01-01",
			"frequency": "daily",
			"interval": 1,
			"patterns": [],
			"skipWeekend": false,
			"weekendSolveMode": "after",
			"endMode": "after_n_occurrences",
			"endOccurrences": 1_000_000
		}},
		{"op": "is", "field": "amount", "value": -500},
	]);
	let budget = budget_with(vec![rule(
		"schedule-rule",
		conditions.to_string(),
		json!([{"op": "link-schedule", "value": SCHEDULE}]).to_string(),
		ConditionsOp::And,
	)]);
	let dates = within(&conditions.to_string(), move || {
		let five_years = day("2031-11-21");
		bill_dates(&budget, &ScheduleId::new(SCHEDULE), today(), five_years).unwrap()
	});
	assert_eq!(dates.len(), 1828, "one date a day, both ends included");
	assert_eq!(dates[0], today());
}
