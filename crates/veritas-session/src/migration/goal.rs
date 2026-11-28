//! (b) MIG-4: an envelope used as a savings target becomes a Veritas goal in
//! the Goals group.

use std::collections::{BTreeSet, HashMap};

use veritas_domain::{
	BudgetData, CategoryGroupId, CategoryId, CategoryKind, CivilDate, CoreError, Money,
	TransactionKind, YearMonth,
};
use veritas_reports::Classification;

use super::{
	Confidence, Conversion, Effect, GoalPreview, Inputs, Suggestion, figure, kind,
	last_complete_month, signal,
};

pub(super) const GOALS: &str = "Goals";
pub(super) const RENAME_FIRST: &str = "Rename one of them first.";
const S3_WORDS: [&str; 8] = [
	"savings",
	"sinking",
	"fund",
	"emergency",
	"vacation",
	"holiday",
	"trip",
	"down payment",
];

/// The live, non-income group named "Goals" (trimmed, case ignored), hidden
/// or not, as a goal made in Veritas finds it (MIG-4.5).
pub(super) fn goals_group(data: &BudgetData) -> Option<&veritas_domain::CategoryGroup> {
	data.category_groups
		.values()
		.filter(|group| !group.tombstone && !group.is_income)
		.find(|group| group.name.trim().eq_ignore_ascii_case(GOALS))
}

/// One S1 source's target and date (MIG-4.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Target {
	pub source: Source,
	pub amount: Money,
	pub date: Option<CivilDate>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Source {
	Settings,
	/// A `#goal` line: Actual's own goal representation (DEBT-8).
	Note,
	/// A `#template … by` line, which budgets toward a target but isn't
	/// Actual's goal representation.
	Template,
	LongGoal,
}

impl Source {
	fn code(self) -> &'static str {
		match self {
			Source::Settings => "s1_goal_def",
			Source::Note => "s1_note",
			Source::Template => "s1_template",
			Source::LongGoal => "s1_long_goal",
		}
	}

	fn words(self) -> &'static str {
		match self {
			Source::Settings => "Actual's goal settings",
			Source::Note | Source::Template => "the category's note",
			Source::LongGoal => "Actual's long-term goal",
		}
	}
}

pub(super) fn suggestions(inputs: &Inputs<'_>) -> Result<Vec<Suggestion>, CoreError> {
	let data = inputs.data;
	let excluded = kind::transfer_categories(inputs)?;
	let goals = goals_group(data);
	let last = last_complete_month(inputs.today)?;
	let window: Vec<YearMonth> = (0..6)
		.rev()
		.map(|back| last.offset(-back))
		.collect::<Result<_, _>>()?;
	let spent = expense_by_month(inputs.classification, &window);
	let no_spending = HashMap::new();
	let spending = spent.get(&last).unwrap_or(&no_spending);
	let mut out: Vec<Suggestion> = Vec::new();
	let mut first_with_name: HashMap<String, String> = HashMap::new();
	for (group, categories) in inputs.sheet.group_order() {
		for id in categories {
			if !candidate(data, id) || excluded.contains(id) {
				continue;
			}
			let Some(category) = data.categories.get(id) else {
				continue;
			};
			let targets = s1(data, id);
			let accumulation = if data.format.decimal_places() == 2 {
				s2(inputs, id, &window, &spent)
			} else {
				None
			};
			if targets.is_empty() && accumulation.is_none() {
				continue;
			}
			let mut suggestion = Suggestion::new(Conversion::Goal, id.as_str(), &category.name);
			for target in &targets {
				let by = target
					.date
					.map(|date| format!(", by {date}"))
					.unwrap_or_default();
				suggestion.signals.push(signal(
					target.source.code(),
					format!("A target in {}{by}", target.source.words()),
					vec![figure("target", target.amount.minor())],
				));
			}
			if let Some(steady) = &accumulation {
				suggestion.signals.push(steady.clone());
			}
			if let Some(words) = s3(inputs, &category.name) {
				suggestion.signals.push(signal(
					"s3_name",
					format!("Its name mentions {words}"),
					Vec::new(),
				));
			}
			match targets.first() {
				Some(first) => {
					suggestion.proposed.target = Some(first.amount);
					suggestion.proposed.target_date = first.date;
					suggestion.target_in_actual =
						matches!(first.source, Source::Note | Source::LongGoal);
					let disagree = targets
						.iter()
						.any(|other| other.amount != first.amount || other.date != first.date);
					if disagree {
						suggestion.conflicts = targets
							.iter()
							.map(|target| target.source.words().to_owned())
							.collect();
						suggestion.confidence = Confidence::Shown;
					} else {
						suggestion.confidence = Confidence::PreTicked;
					}
				}
				None => suggestion.confidence = Confidence::Shown,
			}
			// MIG-4.1: the move's own predicate, Unicode upper case with no
			// trimming, so a name called free here can't fail Apply.
			let name_key = category.name.to_uppercase();
			let in_goals = goals.is_some_and(|goals| {
				goals.id != category.group
					&& crate::categories::ensure_free_name(
						data,
						&goals.id,
						&category.name,
						Some(id),
					)
					.is_err()
			});
			if in_goals {
				suggestion.blocked = Some(RENAME_FIRST.to_owned());
			} else if let Some(first) = first_with_name.get(&name_key) {
				suggestion.blocked_by = Some(first.clone());
			} else {
				first_with_name.insert(name_key, suggestion.key.clone());
			}
			match goals {
				None => suggestion
					.confirm_lines
					.push("A new group, \"Goals\", is made after your other groups.".to_owned()),
				Some(goals) if goals.hidden => {
					suggestion.asks_to_show_goals_group = true;
					suggestion.confirm_lines.push(format!(
						"Your group \"{}\" is hidden. Show it again and add these goals to it?",
						goals.name
					));
				}
				Some(_) => {}
			}
			suggestion.confirm_lines.push(format!(
				"\"{}\" moves to the top of the Goals group. Its budget amounts, notes and transactions stay with it, and every total stays the same.",
				category.name
			));
			suggestion.goal_preview = preview(
				inputs,
				id,
				suggestion.proposed.target,
				suggestion.proposed.target_date,
			)?;
			suggestion.effects = group_effects(
				data,
				group,
				goals.map(|goals| &goals.id),
				id,
				spending,
				last,
			)?;
			out.push(suggestion);
		}
	}
	Ok(out)
}

/// MIG-2.4's goal lines for `target` and `date`, as Apply would write the
/// goal: reached now and backfilled (GOAL-9), and DEBT-8.1's suggestion.
pub(super) fn preview(
	inputs: &Inputs<'_>,
	category: &CategoryId,
	target: Option<Money>,
	date: Option<CivilDate>,
) -> Result<Option<GoalPreview>, CoreError> {
	let Some(target) = target.filter(|target| *target > Money::ZERO) else {
		return Ok(None);
	};
	let month = inputs.today.year_month();
	let goal = veritas_domain::Goal {
		id: veritas_domain::GoalId::new("preview"),
		category: category.clone(),
		target,
		target_date: date,
		icon: None,
		color: None,
		status: veritas_domain::GoalStatus::Active,
		reached_month: None,
		closed_month: None,
		released_to: None,
		released: None,
		activity_seen_through: None,
		activity_seen_total: None,
		close_month_dismissed_available: None,
	};
	let available = inputs
		.sheet
		.category(month, category)
		.map_or(Money::ZERO, |cell| cell.balance);
	let reached_month = (available >= target)
		.then(|| {
			veritas_engine::goals::first_reached_month(inputs.sheet, &goal, month).or(Some(month))
		})
		.flatten();
	// D77, the same test the Goals screen uses, so the two never disagree.
	let spending_recorded =
		veritas_forecast::spending_recorded(inputs.data, inputs.classification, category, month)?;
	let suggestion = if spending_recorded || reached_month.is_some() {
		None
	} else {
		veritas_engine::goals::suggestion(inputs.sheet, &goal, month)?
	};
	Ok(Some(GoalPreview {
		reached_month,
		monthly: suggestion.map(|suggestion| suggestion.monthly),
		months_left: suggestion.map(|suggestion| suggestion.months_left),
		spending_recorded,
	}))
}

fn candidate(data: &BudgetData, id: &CategoryId) -> bool {
	let Some(category) = data.categories.get(id) else {
		return false;
	};
	let income_group = data
		.category_groups
		.get(&category.group)
		.is_some_and(|group| group.is_income);
	let archived = data
		.category_meta
		.get(id)
		.is_some_and(|meta| meta.archived_on.is_some());
	!category.tombstone
		&& !category.hidden
		&& !archived
		&& !category.is_income
		&& !income_group
		&& data.category_kind(id) == CategoryKind::Expense
		&& !data.goals.values().any(|goal| &goal.category == id)
}

/// MIG-4.2 S1, every source found, in order: Actual's goal settings, the
/// note's `#goal` or `#template … by` line, then the latest long-term goal.
pub(super) fn s1(data: &BudgetData, id: &CategoryId) -> Vec<Target> {
	let places = data.format.decimal_places();
	let mut found = Vec::new();
	if let Some(target) = from_settings(data, id, places) {
		found.push(target);
	}
	let note = data
		.notes
		.get(id.as_str())
		.and_then(|note| note.as_deref())
		.unwrap_or("");
	if let Some(target) = note.lines().find_map(|line| from_line(line, places)) {
		found.push(target);
	}
	let long_goal = data
		.budget_cells
		.values()
		.filter(|cell| &cell.category == id && cell.long_goal)
		.filter_map(|cell| cell.goal.map(|goal| (cell.month, goal)))
		.max_by_key(|(month, _)| *month);
	if let Some((_, amount)) = long_goal.filter(|(_, amount)| *amount > Money::ZERO) {
		found.push(Target {
			source: Source::LongGoal,
			amount,
			date: None,
		});
	}
	found
}

/// `categories.goal_def` when `template_settings.source` is `ui`: the first
/// `goal`, or `by` or `spend` without `repeat`, template.
fn from_settings(data: &BudgetData, id: &CategoryId, places: u8) -> Option<Target> {
	let templates = data.category_templates.get(id)?;
	let settings: serde_json::Value =
		serde_json::from_str(templates.template_settings.as_deref()?).ok()?;
	if settings.get("source")?.as_str()? != "ui" {
		return None;
	}
	let definitions: serde_json::Value =
		serde_json::from_str(templates.goal_def.as_deref()?).ok()?;
	definitions.as_array()?.iter().find_map(|template| {
		let amount = major_units_value(template.get("amount")?, places)?;
		let kind = template.get("type")?.as_str()?;
		let repeats = template
			.get("repeat")
			.is_some_and(|repeat| !repeat.is_null());
		let date = match kind {
			"goal" => None,
			"by" | "spend" if !repeats => Some(month_end(template.get("month")?.as_str()?)?),
			_ => return None,
		};
		(amount > Money::ZERO).then_some(Target {
			source: Source::Settings,
			amount,
			date,
		})
	})
}

/// MIG-4.2's line test, `goal-template.pegjs` as amended: read from the
/// first `#`, which must start the literal `#goal` or `#template` (case
/// sensitive), then `#goal AMOUNT`, or `#template[-PRIORITY] AMOUNT by
/// YYYY-MM`, optionally `spend from YYYY-MM`, never `repeat`. `by`, `spend`
/// and `from` are in any case, and spaces and tabs between parts are
/// optional.
pub(super) fn from_line(line: &str, places: u8) -> Option<Target> {
	let mut rest = &line[line.find('#')?..];
	if let Some(after) = rest.strip_prefix("#goal") {
		let (amount, after) = amount(after.trim_start_matches([' ', '\t']), places)?;
		if !after.trim_matches([' ', '\t']).is_empty() {
			return None;
		}
		return (amount > Money::ZERO).then_some(Target {
			source: Source::Note,
			amount,
			date: None,
		});
	}
	rest = rest.strip_prefix("#template")?;
	if let Some(priority) = rest.strip_prefix('-') {
		let digits = priority.bytes().take_while(u8::is_ascii_digit).count();
		if digits == 0 {
			return None;
		}
		rest = &priority[digits..];
	}
	let (amount, after) = amount(rest.trim_start_matches([' ', '\t']), places)?;
	let after = word(after, "by")?;
	let (date, after) = month(after)?;
	let after = after.trim_start_matches([' ', '\t']);
	if !after.is_empty() {
		let after = word(word(after, "spend")?, "from")?;
		let (_, after) = month(after)?;
		if !after.trim_matches([' ', '\t']).is_empty() {
			return None;
		}
	}
	(amount > Money::ZERO).then_some(Target {
		source: Source::Template,
		amount,
		date: Some(date),
	})
}

/// `text` after optional spaces and tabs and `wanted` in any case.
fn word<'a>(text: &'a str, wanted: &str) -> Option<&'a str> {
	let text = text.trim_start_matches([' ', '\t']);
	let head = text.get(..wanted.len())?;
	head.eq_ignore_ascii_case(wanted)
		.then(|| &text[wanted.len()..])
}

/// A `YYYY-MM` after optional spaces and tabs, as the last day of its month.
fn month(text: &str) -> Option<(CivilDate, &str)> {
	let text = text.trim_start_matches([' ', '\t']);
	let written = text.get(..7)?;
	Some((month_end(written)?, &text[7..]))
}

/// MIG-4.2's `AMOUNT`: an optional currency symbol (Unicode `Sc`), optional
/// spaces or tabs, an optional `-`, digits, and optionally a decimal point
/// with none, one or two digits. Converted with integer half-up at the
/// currency's places (Actual's `amountToInteger`).
fn amount(text: &str, places: u8) -> Option<(Money, &str)> {
	let mut rest = text;
	if let Some(symbol) = rest.chars().next().filter(|c| is_currency_symbol(*c)) {
		rest = rest[symbol.len_utf8()..].trim_start_matches([' ', '\t']);
	}
	let negative = rest.starts_with('-');
	if negative {
		rest = &rest[1..];
	}
	let whole_len = rest.bytes().take_while(u8::is_ascii_digit).count();
	if whole_len == 0 {
		return None;
	}
	let whole = &rest[..whole_len];
	rest = &rest[whole_len..];
	let mut fraction = "";
	if let Some(after) = rest.strip_prefix('.') {
		let digits = after.bytes().take_while(u8::is_ascii_digit).count();
		if digits > 2 {
			return None;
		}
		fraction = &after[..digits];
		rest = &after[digits..];
	}
	let minor = scaled(whole, fraction, places)?;
	Some((
		Money::from_minor(if negative { -minor } else { minor }),
		rest,
	))
}

/// Unicode's `Sc` (currency symbol) characters, as of Unicode 15.
fn is_currency_symbol(c: char) -> bool {
	matches!(
		c,
		'$' | '\u{A2}'..='\u{A5}'
			| '\u{58F}' | '\u{60B}' | '\u{7FE}'..='\u{7FF}'
			| '\u{9F2}'..='\u{9F3}' | '\u{9FB}' | '\u{AF1}'
			| '\u{BF9}' | '\u{E3F}' | '\u{17DB}' | '\u{20A0}'..='\u{20C0}'
			| '\u{A838}' | '\u{FDFC}' | '\u{FE69}' | '\u{FF04}'
			| '\u{FFE0}'..='\u{FFE1}' | '\u{FFE5}'..='\u{FFE6}'
			| '\u{11FDD}'..='\u{11FE0}' | '\u{1E2FF}' | '\u{1ECB0}'
	)
}

/// Digits written in major units, scaled by `10^places` and rounded half up,
/// in integers.
fn scaled(whole: &str, fraction: &str, places: u8) -> Option<i64> {
	let scale = 10_i64.checked_pow(u32::from(places))?;
	let mut minor = whole.parse::<i64>().ok()?.checked_mul(scale)?;
	let mut place_value = scale;
	for (index, digit) in fraction.bytes().map(|b| i64::from(b - b'0')).enumerate() {
		if index < usize::from(places) {
			place_value /= 10;
			minor = minor.checked_add(digit * place_value)?;
		} else {
			if digit >= 5 {
				minor = minor.checked_add(1)?;
			}
			break;
		}
	}
	Some(minor)
}

/// A `goal_def` template's amount, a JSON number in major units.
fn major_units_text(text: &str, places: u8) -> Option<Money> {
	let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
	if whole.is_empty()
		|| !whole.bytes().all(|b| b.is_ascii_digit())
		|| !fraction.bytes().all(|b| b.is_ascii_digit())
	{
		return None;
	}
	scaled(whole, fraction, places).map(Money::from_minor)
}

fn major_units_value(value: &serde_json::Value, places: u8) -> Option<Money> {
	match value {
		serde_json::Value::Number(number) => major_units_text(&number.to_string(), places),
		serde_json::Value::String(text) => major_units_text(text, places),
		_ => None,
	}
}

/// The last day of a `YYYY-MM` month.
fn month_end(text: &str) -> Option<CivilDate> {
	let month = YearMonth::parse(text).ok()?;
	month
		.offset(1)
		.ok()?
		.first_day()
		.naive()
		.pred_opt()
		.map(CivilDate::from)
}

/// MIG-4.2 S2, in a 2-place budget: over the last 6 complete months not
/// before the budget's first, at least 4 budgeted above 0, the largest at
/// most twice the smallest, at most 1 with spending, and the available at
/// the end of the last at least twice the median budget.
fn s2(
	inputs: &Inputs<'_>,
	id: &CategoryId,
	window: &[YearMonth],
	spent: &HashMap<YearMonth, HashMap<CategoryId, Money>>,
) -> Option<super::Signal> {
	let sheet = inputs.sheet;
	let last = *window.last()?;
	let first_month = sheet.months().first().copied();
	let mut budgeted = Vec::new();
	let mut spending_months = 0;
	for &month in window {
		if first_month.is_some_and(|first| month < first) {
			continue;
		}
		let Some(cell) = sheet.category(month, id) else {
			continue;
		};
		if cell.budgeted > Money::ZERO {
			budgeted.push(cell.budgeted);
		}
		let spending = spent
			.get(&month)
			.and_then(|categories| categories.get(id))
			.is_some_and(|amount| *amount > Money::ZERO);
		if spending {
			spending_months += 1;
		}
	}
	let (Some(&smallest), Some(&largest)) = (budgeted.iter().min(), budgeted.iter().max()) else {
		return None;
	};
	let available = sheet
		.category(last, id)
		.map_or(Money::ZERO, |cell| cell.balance);
	let mut sorted = budgeted.clone();
	sorted.sort_unstable();
	let middle = sorted.len() / 2;
	// FC-1's median: an even count averages the middle two, rounded half
	// away from zero. Every budget here is above 0.
	let median = if sorted.len() % 2 == 1 {
		sorted[middle]
	} else {
		Money::from_minor((sorted[middle - 1].minor() + sorted[middle].minor() + 1) / 2)
	};
	let steady = budgeted.len() >= 4
		&& largest.minor() <= smallest.minor().saturating_mul(2)
		&& spending_months <= 1
		&& available.minor() >= median.minor().saturating_mul(2);
	if !steady {
		return None;
	}
	Some(signal(
		"s2_steady",
		format!(
			"Budgeted in {} of the last 6 months with almost no spending, and it holds its money",
			budgeted.len()
		),
		vec![
			figure("available", available.minor()),
			figure("budgeted_months", budgeted.len() as i64),
			figure("smallest", smallest.minor()),
			figure("largest", largest.minor()),
			figure("spending_months", spending_months),
		],
	))
}

/// MIG-4.2 S3: the words the name's key holds, as whole words in order.
/// Supporting text only (MIG-1.7).
fn s3(inputs: &Inputs<'_>, name: &str) -> Option<String> {
	let key = (inputs.name_key)(name);
	let words: Vec<&str> = key.split_whitespace().collect();
	let found: Vec<&str> = S3_WORDS
		.iter()
		.copied()
		.filter(|word| {
			let wanted = (inputs.name_key)(word);
			let wanted: Vec<&str> = wanted.split_whitespace().collect();
			!wanted.is_empty()
				&& words
					.windows(wanted.len())
					.any(|window| window == wanted.as_slice())
		})
		.collect();
	(!found.is_empty()).then(|| found.join(", "))
}

/// CLS-6 `expense_total` per category for each of `months`: expense rows,
/// primary legs only, as a positive amount. One pass over the rows.
pub(super) fn expense_by_month(
	classification: &Classification,
	months: &[YearMonth],
) -> HashMap<YearMonth, HashMap<CategoryId, Money>> {
	let mut totals: HashMap<YearMonth, HashMap<CategoryId, Money>> = HashMap::new();
	for row in classification.rows() {
		let month = row.date.year_month();
		if !row.is_primary() || row.kind() != TransactionKind::Expense || !months.contains(&month) {
			continue;
		}
		let Some(category) = &row.category else {
			continue;
		};
		let total = totals
			.entry(month)
			.or_default()
			.entry(category.clone())
			.or_insert(Money::ZERO);
		*total = Money::from_minor(total.minor().saturating_sub(row.amount.minor()));
	}
	totals
}

/// MIG-4.5a: the group lines that move with the category, for the last
/// complete month. Grand totals don't change.
fn group_effects(
	data: &BudgetData,
	group: &CategoryGroupId,
	goals: Option<&CategoryGroupId>,
	category: &CategoryId,
	spending: &HashMap<CategoryId, Money>,
	month: YearMonth,
) -> Result<Vec<Effect>, CoreError> {
	if goals == Some(group) {
		return Ok(Vec::new());
	}
	let held = spending.get(category).copied().unwrap_or(Money::ZERO);
	let group_total = |group: &CategoryGroupId| -> Result<Money, CoreError> {
		let members: BTreeSet<&CategoryId> = data
			.categories
			.values()
			.filter(|other| &other.group == group && !other.tombstone)
			.map(|other| &other.id)
			.collect();
		Money::checked_sum(
			members
				.into_iter()
				.map(|member| spending.get(member).copied().unwrap_or(Money::ZERO)),
		)
	};
	let name = |group: &CategoryGroupId| {
		data.category_groups
			.get(group)
			.map_or_else(String::new, |group| group.name.clone())
	};
	let mut effects = Vec::new();
	let before = group_total(group)?;
	effects.push(Effect {
		report: format!("Spending in {}", name(group)),
		month: Some(month),
		before,
		after: before.checked_sub(held)?,
	});
	let goals_before = match goals {
		Some(goals) => group_total(goals)?,
		None => Money::ZERO,
	};
	effects.push(Effect {
		report: format!("Spending in {GOALS}"),
		month: Some(month),
		before: goals_before,
		after: goals_before.checked_add(held)?,
	});
	Ok(effects)
}
