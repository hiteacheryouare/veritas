//! Veritas settings kept as `veritas_meta` rows: the statements' entity name
//! (docs/finance/statements.md ST-3).

use veritas_domain::{BudgetData, CoreError, Row, Setting, SettingKey};

pub const ENTITY_NAME_MAX_CHARS: usize = 100;

/// The name a statement's header shows, and the one the user set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityName {
	/// The user's name when set, else the budget's name, else empty.
	pub shown: String,
	pub custom: Option<String>,
}

pub fn entity_name(data: &BudgetData, budget_name: Option<&str>) -> EntityName {
	let custom = data
		.settings
		.get(&SettingKey::EntityName)
		.map(|name| name.trim())
		.filter(|name| !name.is_empty())
		.map(str::to_owned);
	EntityName {
		shown: custom
			.clone()
			.or_else(|| budget_name.map(str::to_owned))
			.unwrap_or_default(),
		custom,
	}
}

/// Trimmed; blank clears it, so the budget's name shows again.
pub fn set_entity_name(data: &BudgetData, name: &str) -> Result<Vec<Row>, CoreError> {
	let name = name.trim();
	if name.chars().any(is_line_break) {
		return Err(CoreError::Invalid(
			"Put the entity name on one line.".to_owned(),
		));
	}
	if name.chars().count() > ENTITY_NAME_MAX_CHARS {
		return Err(CoreError::Invalid(format!(
			"Keep the entity name to {ENTITY_NAME_MAX_CHARS} characters."
		)));
	}
	let value = (!name.is_empty()).then(|| name.to_owned());
	if data.settings.get(&SettingKey::EntityName) == value.as_ref() {
		return Ok(Vec::new());
	}
	Ok(vec![Row::Setting(Setting {
		key: SettingKey::EntityName,
		value,
	})])
}

/// Unicode's mandatory breaks (UAX #14 BK, CR, LF, NL): LF, VT, FF, CR, NEL,
/// LINE SEPARATOR and PARAGRAPH SEPARATOR.
fn is_line_break(c: char) -> bool {
	matches!(
		c,
		'\n' | '\u{b}' | '\u{c}' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}'
	)
}

#[cfg(test)]
mod tests {
	use veritas_domain::BudgetType;

	use super::*;

	fn set(name: &str) -> Result<Vec<Row>, CoreError> {
		set_entity_name(&BudgetData::empty(BudgetType::Envelope), name)
	}

	#[test]
	fn st_3_any_line_break_inside_the_name_is_invalid() {
		for name in [
			"Household\nLLC",
			"Household\rLLC",
			"Household\r\nLLC",
			"Household\u{b}LLC",
			"Household\u{c}LLC",
			"Household\u{85}LLC",
			"Household\u{2028}LLC",
			"Household\u{2029}LLC",
		] {
			assert!(matches!(set(name), Err(CoreError::Invalid(_))), "{name:?}");
		}
		assert!(
			set(" Household, LLC\n").is_ok(),
			"a trailing break is trimmed away"
		);
	}

	#[test]
	fn st_3_the_limit_is_100_unicode_scalar_values() {
		assert!(set(&"é".repeat(100)).is_ok(), "100 scalars, 200 bytes");
		assert!(matches!(set(&"é".repeat(101)), Err(CoreError::Invalid(_))));
		let decomposed = "e\u{301}";
		assert!(
			set(&decomposed.repeat(50)).is_ok(),
			"50 characters as seen, 100 scalars"
		);
		assert!(
			matches!(set(&decomposed.repeat(51)), Err(CoreError::Invalid(_))),
			"51 characters as seen, but 102 scalars"
		);
	}
}
