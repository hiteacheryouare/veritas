//! (d) MIG-6: schedules that are bills get a Veritas-only name and icon.
//! No `schedules` or `rules` row is ever written.

use std::collections::BTreeMap;

use veritas_domain::{AccountKind, CategoryId, CoreError, ScheduleId, ScheduleMeta};
use veritas_rules::{BillAmount, read_bill};

use super::{Confidence, Conversion, Inputs, Suggestion, signal};

pub(super) const INCOME_ICON: &str = "arrow.down.circle";
pub(super) const LOAN_ICON: &str = "banknote";

struct Candidate {
	id: ScheduleId,
	meta: ScheduleMeta,
	own_name: Option<String>,
	payee: Option<String>,
	category: Option<CategoryId>,
	actual_only: bool,
	icon: Option<&'static str>,
	loan: Option<String>,
	proposed_name: Option<String>,
}

impl Candidate {
	/// The name Bills shows now, or would show with the proposal (MIG-6.2).
	fn name(&self) -> Option<&str> {
		self.meta
			.display_name
			.as_deref()
			.or(self.own_name.as_deref())
			.or(self.proposed_name.as_deref())
			.or(self.payee.as_deref())
	}
}

pub(super) fn suggestions(inputs: &Inputs<'_>) -> Result<Vec<Suggestion>, CoreError> {
	let data = inputs.data;
	let mut candidates = Vec::new();
	for schedule in data
		.schedules
		.values()
		.filter(|schedule| !schedule.tombstone && !schedule.completed)
	{
		let read = read_bill(schedule, data.rules.get(&schedule.rule), data);
		let view = read.as_ref().ok();
		let payee = view
			.and_then(|view| view.bill.payee.as_ref())
			.and_then(|payee| data.payees.get(payee));
		let income = view.is_some_and(|view| match view.bill.amount {
			BillAmount::Exact(amount) | BillAmount::About(amount) => amount.minor() > 0,
			BillAmount::Between(low, high) => low.minor() > 0 && high.minor() > 0,
		});
		// MIG-6.3: ST-9.1's rule 1 or 2 only, never a balance below zero.
		let loan = payee
			.and_then(|payee| payee.transfer_account.as_ref())
			.filter(|account| {
				data.account_meta.get(*account).is_some_and(|meta| {
					meta.kind == Some(AccountKind::Liability)
						|| meta.loan_rate_bps.is_some()
						|| meta.loan_payment.is_some()
						|| meta.loan_payment_day.is_some()
				})
			})
			.and_then(|account| data.accounts.get(account))
			.map(|account| account.name.clone());
		let meta = data
			.schedule_meta
			.get(&schedule.id)
			.cloned()
			.unwrap_or_default();
		let own_name = schedule
			.name
			.as_deref()
			.map(str::trim)
			.filter(|name| !name.is_empty())
			.map(str::to_owned);
		let payee_name = payee
			.map(|payee| payee.name.clone())
			.filter(|name| !name.trim().is_empty());
		let proposed_name = (meta.display_name.is_none() && own_name.is_none())
			.then(|| payee_name.clone())
			.flatten();
		candidates.push(Candidate {
			id: schedule.id.clone(),
			meta,
			own_name,
			payee: payee_name,
			category: view.and_then(|view| view.bill.category.clone()),
			actual_only: view.is_none_or(|view| view.custom_rule),
			icon: if income {
				Some(INCOME_ICON)
			} else if loan.is_some() {
				Some(LOAN_ICON)
			} else {
				None
			},
			loan,
			proposed_name,
		});
	}
	disambiguate(inputs, &mut candidates);
	let mut out = Vec::new();
	for candidate in candidates {
		let icon = candidate
			.icon
			.filter(|icon| candidate.meta.icon.as_deref() != Some(*icon));
		if candidate.proposed_name.is_none() && icon.is_none() {
			continue;
		}
		let shown = candidate.name().unwrap_or("Bill").to_owned();
		let mut suggestion = Suggestion::new(Conversion::Bill, candidate.id.as_str(), &shown);
		if let (Some(name), Some(payee)) = (&candidate.proposed_name, &candidate.payee) {
			suggestion.signals.push(signal(
				"no_name",
				format!(
					"It has no name in Actual, so Bills shows its payee, {payee}. Proposed: {name}"
				),
				Vec::new(),
			));
		}
		if icon == Some(INCOME_ICON) {
			suggestion
				.signals
				.push(signal("income", "It brings money in", Vec::new()));
		}
		if let (Some(LOAN_ICON), Some(account)) = (icon, &candidate.loan) {
			suggestion.signals.push(signal(
				"loan",
				format!("It pays {account}, a debt"),
				Vec::new(),
			));
		}
		if candidate.actual_only {
			suggestion.signals.push(signal(
				"actual_only",
				"Has settings only Actual can apply",
				Vec::new(),
			));
		}
		let replaces_icon = icon.is_some() && candidate.meta.icon.is_some();
		suggestion.confidence = if replaces_icon {
			Confidence::Shown
		} else {
			Confidence::PreTicked
		};
		suggestion.proposed.display_name = candidate.proposed_name.clone();
		suggestion.proposed.icon = icon.map(str::to_owned);
		suggestion.confirm_lines.push(
			"The name and icon show only in Veritas. Actual never sees them, and its bill is left exactly as it is."
				.to_owned(),
		);
		out.push(suggestion);
	}
	Ok(out)
}

/// MIG-6.2: when two or more bills would show one name (by AI-3.6's key),
/// each proposed name gets its category, `<name> (<category>)`, or, when
/// the category is shared or missing, a number, `<name> <n>`, by ascending
/// schedule id.
fn disambiguate(inputs: &Inputs<'_>, candidates: &mut [Candidate]) {
	let mut by_name: BTreeMap<String, Vec<usize>> = BTreeMap::new();
	for (index, candidate) in candidates.iter().enumerate() {
		if let Some(name) = candidate.name() {
			by_name
				.entry((inputs.name_key)(name))
				.or_default()
				.push(index);
		}
	}
	for members in by_name.values().filter(|members| members.len() > 1) {
		let mut proposed: Vec<usize> = members
			.iter()
			.copied()
			.filter(|index| candidates[*index].proposed_name.is_some())
			.collect();
		proposed.sort_by(|a, b| candidates[*a].id.cmp(&candidates[*b].id));
		let category_name = |index: usize| {
			candidates[index]
				.category
				.as_ref()
				.and_then(|category| inputs.data.categories.get(category))
				.map(|category| category.name.clone())
		};
		let distinct = |index: usize| {
			let mine = category_name(index);
			mine.is_some()
				&& members
					.iter()
					.filter(|other| **other != index)
					.all(|other| category_name(*other) != mine)
		};
		let mut renamed = Vec::new();
		let mut number = 0;
		for &index in &proposed {
			let Some(base) = candidates[index].proposed_name.clone() else {
				continue;
			};
			let name = match (distinct(index), category_name(index)) {
				(true, Some(category)) => format!("{base} ({category})"),
				_ => {
					number += 1;
					format!("{base} {number}")
				}
			};
			renamed.push((index, name));
		}
		for (index, name) in renamed {
			candidates[index].proposed_name = Some(name);
		}
	}
}
