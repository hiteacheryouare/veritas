//! R-BILL-1, R-BILL-4 and PLAN §9 #13: schedule next dates and upcoming dates
//! match Actual's for hundreds of generated recurrences. Actual creates the
//! schedules (`createSchedule` through the oracle), the dump reports what it
//! computed, and Veritas recomputes both from the rule row Actual stored.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::{Days, NaiveDate};
use serde_json::{Value, json};
use veritas_domain::CivilDate;
use veritas_rules::{Op, next_date_from, parse_date_value, schedule_dates, validate_schedule_date};

const UPCOMING: usize = 12;
const CASES_PER_TODAY: usize = 400;
const TODAYS: [&str; 4] = ["2026-11-09", "2025-12-31", "2027-02-27", "2028-02-29"];

fn repo_root() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn oracle_dir() -> Option<PathBuf> {
	let dir = repo_root().join("tools/actual-oracle");
	dir.join("node_modules").exists().then_some(dir)
}

/// xorshift64*: deterministic, so a failure names a case that reproduces.
struct Rng(u64);

impl Rng {
	fn next(&mut self) -> u64 {
		self.0 ^= self.0 >> 12;
		self.0 ^= self.0 << 25;
		self.0 ^= self.0 >> 27;
		self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
	}

	fn below(&mut self, n: u64) -> u64 {
		self.next() % n
	}

	fn chance(&mut self, percent: u64) -> bool {
		self.below(100) < percent
	}

	fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
		&items[self.below(items.len() as u64) as usize]
	}
}

fn random_start(rng: &mut Rng) -> String {
	let year = 2023 + rng.below(6) as i32;
	let month = 1 + rng.below(12) as u32;
	let day = if rng.chance(40) {
		*rng.pick(&[28, 29, 30, 31])
	} else {
		1 + rng.below(28) as u32
	};
	match NaiveDate::from_ymd_opt(year, month, day) {
		Some(date) => date.to_string(),
		None if rng.chance(15) => format!("{year:04}-{month:02}-{day:02}"),
		None => NaiveDate::from_ymd_opt(year, month, 28)
			.map(|date| date.to_string())
			.unwrap_or_default(),
	}
}

fn random_patterns(rng: &mut Rng) -> Vec<Value> {
	(0..1 + rng.below(3))
		.map(|_| {
			if rng.chance(55) {
				let day = if rng.chance(45) {
					*rng.pick(&[29, 30, 31, -1])
				} else {
					1 + rng.below(28) as i64
				};
				json!({"type": "day", "value": day})
			} else {
				let weekday = rng.pick(&["SU", "MO", "TU", "WE", "TH", "FR", "SA"]);
				let nth = rng.pick(&[1, 2, 3, 4, 5, -1]);
				json!({"type": weekday, "value": nth})
			}
		})
		.collect()
}

fn random_config(rng: &mut Rng) -> Value {
	let frequency = *rng.pick(&["daily", "weekly", "monthly", "monthly", "monthly", "yearly"]);
	let start = random_start(rng);
	let mut config = json!({"start": start, "frequency": frequency});
	let fields = config.as_object_mut().unwrap();
	let full_ui_shape = rng.chance(60);
	if full_ui_shape || rng.chance(50) {
		let interval = if rng.chance(55) {
			1
		} else {
			*rng.pick(&[2, 3, 4, 6])
		};
		fields.insert("interval".into(), json!(interval));
	}
	let patterns = if frequency == "monthly" && rng.chance(55) {
		random_patterns(rng)
	} else {
		Vec::new()
	};
	if full_ui_shape || !patterns.is_empty() {
		fields.insert("patterns".into(), json!(patterns));
	}
	let skip_weekend = rng.chance(40);
	if full_ui_shape || skip_weekend {
		fields.insert("skipWeekend".into(), json!(skip_weekend));
		fields.insert(
			"weekendSolveMode".into(),
			json!(rng.pick(&["before", "after"])),
		);
	}
	match rng.below(4) {
		0 => {
			fields.insert("endMode".into(), json!("after_n_occurrences"));
			fields.insert("endOccurrences".into(), json!(1 + rng.below(30)));
		}
		1 => {
			let end = NaiveDate::parse_from_str(&start, "%Y-%m-%d")
				.ok()
				.and_then(|date| date.checked_add_days(Days::new(rng.below(1200))))
				.unwrap_or_default();
			fields.insert("endMode".into(), json!("on_date"));
			fields.insert("endDate".into(), json!(end.to_string()));
		}
		_ if full_ui_shape => {
			fields.insert("endMode".into(), json!("never"));
		}
		_ => {}
	}
	config
}

fn random_date(rng: &mut Rng) -> Value {
	if rng.chance(94) {
		return random_config(rng);
	}
	let start = random_start(rng);
	if NaiveDate::parse_from_str(&start, "%Y-%m-%d").is_ok() {
		json!(start)
	} else {
		random_config(rng)
	}
}

/// Configs Actual's `createSchedule` throws on would stop the whole op file,
/// so only keep ones Veritas expects it to accept. A wrong guess here fails
/// the oracle run and names the op.
fn creatable(date: &Value, today: CivilDate) -> bool {
	let op = if date.is_string() {
		Op::Is
	} else {
		Op::IsApprox
	};
	parse_date_value(op, Some(date)).is_ok_and(|parsed| next_date_from(&parsed, today).is_ok())
}

fn run(command: &mut Command) -> String {
	let output = command
		.output()
		.unwrap_or_else(|error| panic!("running {command:?}: {error}"));
	assert!(
		output.status.success(),
		"{command:?} failed:\n{}",
		String::from_utf8_lossy(&output.stderr)
	);
	String::from_utf8_lossy(&output.stdout).into_owned()
}

fn oracle(oracle: &Path, args: &[&str]) -> String {
	run(Command::new("node")
		.current_dir(oracle)
		.arg("src/main.ts")
		.args(args))
}

fn day(text: &str) -> CivilDate {
	CivilDate::from(NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap_or_default())
}

fn rule_conditions(dump: &Value) -> std::collections::HashMap<String, Vec<Value>> {
	let rules: std::collections::HashMap<&str, &str> = dump["tables"]["rules"]["rows"]
		.as_array()
		.into_iter()
		.flatten()
		.filter_map(|row| Some((row["id"].as_str()?, row["conditions"].as_str()?)))
		.collect();
	dump["tables"]["schedules"]["rows"]
		.as_array()
		.into_iter()
		.flatten()
		.filter_map(|row| {
			let conditions = rules.get(row["rule"].as_str()?)?;
			let parsed: Vec<Value> = serde_json::from_str(conditions).ok()?;
			Some((row["id"].as_str()?.to_owned(), parsed))
		})
		.collect()
}

fn expected_upcoming(raw: &Value, date: &veritas_rules::DateValue, today: CivilDate) -> Value {
	if validate_schedule_date(raw).is_err() {
		return Value::Null;
	}
	match schedule_dates(date, today, None, Some(UPCOMING)) {
		Ok(dates) => json!(dates.iter().map(ToString::to_string).collect::<Vec<_>>()),
		Err(_) => Value::Null,
	}
}

#[test]
fn schedule_dates_match_the_oracle() {
	let Some(oracle_dir) = oracle_dir() else {
		eprintln!(
			"skipping the schedule oracle test: tools/actual-oracle/node_modules is missing, run `cargo xtask oracle setup`"
		);
		return;
	};
	let base = repo_root().join("fixtures/synthetic/small.zip");
	let scratch = std::env::temp_dir().join(format!("veritas-rules-oracle-{}", std::process::id()));
	std::fs::create_dir_all(&scratch).unwrap_or_else(|error| panic!("{error}"));

	let mut rng = Rng(0x5eed_0000_0000_0013);
	let mut compared = 0;
	let mut mismatches = Vec::new();
	for (t, today_text) in TODAYS.iter().enumerate() {
		let today = day(today_text);
		let mut ops = Vec::new();
		let mut skipped = 0;
		while ops.len() < CASES_PER_TODAY {
			let date = random_date(&mut rng);
			if !creatable(&date, today) {
				skipped += 1;
				continue;
			}
			let id = format!("00000000-0000-4000-8{t:03x}-{:012x}", ops.len());
			ops.push(
				json!({"op": "createSchedule", "id": id, "name": format!("s{t}-{}", ops.len()), "date": date}),
			);
		}
		assert!(
			skipped < CASES_PER_TODAY / 10,
			"{skipped} generated configs were not creatable"
		);

		let ops_path = scratch.join(format!("ops-{t}.json"));
		let zip_path = scratch.join(format!("applied-{t}.zip"));
		std::fs::write(&ops_path, json!({"version": 1, "ops": ops}).to_string())
			.unwrap_or_else(|error| panic!("{error}"));
		let (ops_arg, zip_arg, base_arg) = (
			ops_path.to_string_lossy().into_owned(),
			zip_path.to_string_lossy().into_owned(),
			base.to_string_lossy().into_owned(),
		);
		oracle(
			&oracle_dir,
			&[
				"apply", &base_arg, &ops_arg, "--out", &zip_arg, "--today", today_text,
			],
		);
		let dump: Value = serde_json::from_str(&oracle(
			&oracle_dir,
			&["dump", &zip_arg, "--today", today_text],
		))
		.unwrap_or_else(|error| panic!("the dump isn't JSON: {error}"));
		let conditions = rule_conditions(&dump);

		for op in &ops {
			let id = op["id"].as_str().unwrap_or_default();
			let actual = &dump["computed"]["schedules"][id];
			let stored = conditions
				.get(id)
				.and_then(|conditions| {
					conditions.iter().find(|c| {
						c["field"] == "date" && (c["op"] == "is" || c["op"] == "isapprox")
					})
				})
				.unwrap_or_else(|| panic!("schedule {id} has no stored date condition"));
			assert_eq!(
				stored["value"], op["date"],
				"Actual stored a different date for {id}"
			);
			let date = parse_date_value(
				Op::from_stored(stored["op"].as_str().unwrap_or_default()).unwrap_or(Op::Is),
				Some(&stored["value"]),
			)
			.unwrap_or_else(|gap| panic!("{id}: {gap:?}"));

			let next = next_date_from(&date, today)
				.ok()
				.flatten()
				.map_or(Value::Null, |next| json!(next.to_string()));
			let upcoming = expected_upcoming(&stored["value"], &date, today);
			compared += 1;
			if actual["nextDate"] != next || actual["upcoming"] != upcoming {
				mismatches.push(format!(
					"today {today_text}, {}\n  actual:  next {} upcoming {}\n  veritas: next {next} upcoming {upcoming}",
					op["date"], actual["nextDate"], actual["upcoming"]
				));
			}
		}
	}
	let _ = std::fs::remove_dir_all(&scratch);

	eprintln!(
		"compared {compared} schedules against the oracle, {} differ",
		mismatches.len()
	);
	assert!(
		mismatches.is_empty(),
		"{} of {compared} schedules differ from Actual:\n{}",
		mismatches.len(),
		mismatches
			.iter()
			.take(15)
			.cloned()
			.collect::<Vec<_>>()
			.join("\n")
	);
}
