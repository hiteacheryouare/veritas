//! R-BILL-1, R-BILL-4 and PLAN §9 #13: schedule next dates and upcoming dates
//! match Actual's for hundreds of generated recurrences. Actual creates the
//! schedules (`createSchedule` through the oracle), the dump reports what it
//! computed, and Veritas recomputes both from the rule row Actual stored.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use chrono::{Days, NaiveDate};
use common::{Rng, apply_and_dump, oracle_dir, repo_root, scratch_dir};
use serde_json::{Value, json};
use veritas_domain::CivilDate;
use veritas_rules::{Op, next_date_from, parse_date_value, schedule_dates, validate_schedule_date};

const UPCOMING: usize = 12;
const CASES_PER_TODAY: usize = 400;
const TODAYS: [&str; 4] = ["2026-11-09", "2025-12-31", "2027-02-27", "2028-02-29"];

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
	let scratch = scratch_dir("schedules");

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

		let dump = apply_and_dump(
			&oracle_dir,
			&scratch,
			&format!("schedules-{t}"),
			&base,
			ops.clone(),
			today_text,
		);
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
