//! `lastScheduleRun` in a budget's metadata.json, read and written the way
//! Actual's `savePrefs` does: the rest of the file is kept as it is, a new
//! key goes last, and clearing the key leaves it out, as `JSON.stringify`
//! drops `undefined` (`lc/server/prefs.ts:64-78`).

use std::io::Write;
use std::path::Path;

use veritas_domain::{CivilDate, CoreError};

use crate::canonical::json::{self, Str, Value};

const LAST_SCHEDULE_RUN: &str = "lastScheduleRun";

fn io_error(path: &Path, error: &std::io::Error) -> CoreError {
	CoreError::Io(format!("{}: {error}", path.display()))
}

fn read(path: &Path) -> Result<Vec<(Str, Value)>, CoreError> {
	let bytes = std::fs::read(path).map_err(|error| io_error(path, &error))?;
	if bytes.len() as u64 > crate::archive::MAX_METADATA_SIZE {
		return Err(CoreError::Invalid(
			"the budget's metadata.json is larger than 1 MB".to_owned(),
		));
	}
	match json::parse(&String::from_utf8_lossy(&bytes)) {
		Ok(Value::Object(entries)) => Ok(entries),
		_ => Err(CoreError::NotAnActualBudget(
			"its metadata.json isn't a JSON object".to_owned(),
		)),
	}
}

/// The day Actual's schedule service last ran here, `YYYY-MM-DD`, as stored.
pub fn last_schedule_run(path: &Path) -> Result<Option<String>, CoreError> {
	Ok(read(path)?
		.into_iter()
		.find(|(key, _)| key.eq_ascii(LAST_SCHEDULE_RUN))
		.and_then(|(_, value)| match value {
			Value::String(day) => day.to_utf8(),
			_ => None,
		}))
}

/// Sets `lastScheduleRun` to `day`, or removes it with `None`. The file is
/// replaced atomically, so a crash leaves the old one or the new one.
pub fn set_last_schedule_run(path: &Path, day: Option<CivilDate>) -> Result<(), CoreError> {
	let mut entries = read(path)?;
	let at = entries
		.iter()
		.position(|(key, _)| key.eq_ascii(LAST_SCHEDULE_RUN));
	match (at, day) {
		(Some(at), Some(day)) => entries[at].1 = Value::String(Str::from(day.to_string().as_str())),
		(None, Some(day)) => entries.push((
			Str::from(LAST_SCHEDULE_RUN),
			Value::String(Str::from(day.to_string().as_str())),
		)),
		(Some(at), None) => {
			entries.remove(at);
		}
		(None, None) => return Ok(()),
	}
	let mut out = Vec::new();
	json::stringify(&mut out, &Value::Object(entries))
		.map_err(|error| CoreError::Internal(error.to_string()))?;
	let partial = path.with_extension("json.partial");
	let mut file = std::fs::File::create(&partial).map_err(|error| io_error(&partial, &error))?;
	file.write_all(&out)
		.and_then(|()| file.sync_all())
		.map_err(|error| io_error(&partial, &error))?;
	std::fs::rename(&partial, path).map_err(|error| io_error(path, &error))
}

#[cfg(test)]
mod tests {
	#![allow(clippy::unwrap_used)]

	use super::*;

	fn day(text: &str) -> CivilDate {
		let parts: Vec<u32> = text.split('-').map(|part| part.parse().unwrap()).collect();
		CivilDate::new(parts[0] as i32, parts[1], parts[2]).unwrap()
	}

	#[test]
	fn the_run_day_is_set_in_place_appended_and_cleared_like_save_prefs() {
		let dir = std::env::temp_dir().join(format!("veritas-metadata-{}", std::process::id()));
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join("metadata.json");
		std::fs::write(
			&path,
			r#"{"id":"b","budgetName":"Home","resetClock":false}"#,
		)
		.unwrap();

		assert_eq!(last_schedule_run(&path).unwrap(), None);
		set_last_schedule_run(&path, Some(day("2026-11-13"))).unwrap();
		assert_eq!(
			std::fs::read_to_string(&path).unwrap(),
			r#"{"id":"b","budgetName":"Home","resetClock":false,"lastScheduleRun":"2026-11-13"}"#
		);
		std::fs::write(
			&path,
			r#"{"id":"b","lastScheduleRun":"2026-11-01","budgetName":"Home"}"#,
		)
		.unwrap();
		set_last_schedule_run(&path, Some(day("2026-11-13"))).unwrap();
		assert_eq!(
			std::fs::read_to_string(&path).unwrap(),
			r#"{"id":"b","lastScheduleRun":"2026-11-13","budgetName":"Home"}"#
		);
		assert_eq!(
			last_schedule_run(&path).unwrap().as_deref(),
			Some("2026-11-13")
		);
		set_last_schedule_run(&path, None).unwrap();
		assert_eq!(
			std::fs::read_to_string(&path).unwrap(),
			r#"{"id":"b","budgetName":"Home"}"#
		);
		std::fs::remove_dir_all(&dir).unwrap();
	}
}
