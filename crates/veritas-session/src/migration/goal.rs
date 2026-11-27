//! (b) MIG-4: an envelope used as a savings target becomes a Veritas goal in
//! the Goals group.

use std::collections::{BTreeSet, HashMap};

use veritas_domain::{
	BudgetData, CategoryGroupId, CategoryId, CategoryKind, CivilDate, CoreError, Money,
	TransactionKind, YearMonth,
};
use veritas_reports::Classification;

use super::{
	Confidence, Conversion, Effect, Inputs, Suggestion, figure, kind, last_complete_month, signal,
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
	Note,
	LongGoal,
}

impl Source {
	fn code(self) -> &'static str {
		match self {
			Source::Settings => "s1_goal_def",
			Source::Note => "s1_note",
			Source::LongGoal => "s1_long_goal",
		}
	}

	fn words(self) -> &'static str {
		match self {
			Source::Settings => "Actual's goal settings",
			Source::Note => "the category's note",
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
			let name_key = category.name.trim().to_lowercase();
			let in_goals = goals.is_some_and(|goals| {
				data.categories.values().any(|other| {
					!other.tombstone
						&& &other.id != id && other.group == goals.id
						&& other.name.trim().to_lowercase() == name_key
				})
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
				Some(goals) if goals.hidden => suggestion.confirm_lines.push(format!(
					"Your group \"{}\" is hidden. Show it again and add these goals to it?",
					goals.name
				)),
				Some(_) => {}
			}
			suggestion.confirm_lines.push(format!(
				"\"{}\" moves to the top of the Goals group. Its budget amounts, notes and transactions stay with it, and every total stays the same.",
				category.name
			));
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

/// A `#goal <amount>` line, or `#template <amount> by <YYYY-MM>` (optionally
/// `#template-<priority>`, and `spend from <YYYY-MM>` after), never with
/// `repeat`. The amount is in major units, as Actual's `amountToInteger`.
pub(super) fn from_line(line: &str, places: u8) -> Option<Target> {
	let words: Vec<&str> = line.split_whitespace().collect();
	let (&directive, rest) = words.split_first()?;
	if directive == "#goal" {
		let [amount] = rest else {
			return None;
		};
		let amount = major_units_text(amount, places)?;
		return (amount > Money::ZERO).then_some(Target {
			source: Source::Note,
			amount,
			date: None,
		});
	}
	let priority = directive.strip_prefix("#template")?;
	if !(priority.is_empty()
		|| priority
			.strip_prefix('-')
			.is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())))
	{
		return None;
	}
	let date = match rest {
		[amount, "by", month] | [amount, "by", month, "spend", "from", _] => {
			(major_units_text(amount, places)?, month_end(month)?)
		}
		_ => return None,
	};
	(date.0 > Money::ZERO).then_some(Target {
		source: Source::Note,
		amount: date.0,
		date: Some(date.1),
	})
}

/// Major units to minor, rounded as `Math.round(amount * 10^places)`.
fn major_units_text(text: &str, places: u8) -> Option<Money> {
	let text = text.trim_start_matches(['$', '€', '£', '¥']);
	let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
	if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) {
		return None;
	}
	if !fraction.bytes().all(|b| b.is_ascii_digit()) {
		return None;
	}
	let scale = 10_i64.checked_pow(u32::from(places))?;
	let mut minor = whole.parse::<i64>().ok()?.checked_mul(scale)?;
	let digits: Vec<i64> = fraction.bytes().map(|b| i64::from(b - b'0')).collect();
	let mut place_value = scale;
	for (index, digit) in digits.iter().enumerate() {
		if index < usize::from(places) {
			place_value /= 10;
			minor = minor.checked_add(digit * place_value)?;
		} else {
			if *digit >= 5 {
				minor = minor.checked_add(1)?;
			}
			break;
		}
	}
	Some(Money::from_minor(minor))
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
	let median = if sorted.len() % 2 == 1 {
		sorted[middle]
	} else {
		Money::from_minor((sorted[middle - 1].minor() + sorted[middle].minor()) / 2)
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
