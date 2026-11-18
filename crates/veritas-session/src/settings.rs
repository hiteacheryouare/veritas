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
	if name
		.chars()
		.any(|c| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
	{
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
