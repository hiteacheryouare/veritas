//! The Bills screens (R-BILL-1 to R-BILL-5): the list, the editor, and the
//! upcoming tray and timeline. Bills are Actual schedules with the rule they
//! own; rules never show in the bill flow (R-BILL-4).

use chrono::{Days, Weekday};
use veritas_domain::{
	AccountId, BudgetData, CategoryId, CivilDate, CoreError, Money, PayeeId, ScheduleId,
};
use veritas_rules::{
	AmountKind, Bill, BillAmount as CoreAmount, BillDate as CoreDate, Frequency, NotABill, Repeat,
	RepeatEnd, RepeatPattern, ScheduleStatus, WeekendMove, bill_dates, bill_status,
};

use crate::FfiError;
use crate::budget::civil_date;
use crate::session::Session;

/// Negative amounts are bills, positive ones income such as a paycheck (R-BILL-5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BillAmount {
	Exact {
		amount: i64,
	},
	/// "About": Actual matches within 7.5%.
	About {
		amount: i64,
	},
	/// Posts the rounded middle.
	Between {
		low: i64,
		high: i64,
	},
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BillFrequency {
	Daily,
	Weekly,
	Monthly,
	Yearly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BillWeekday {
	Sunday,
	Monday,
	Tuesday,
	Wednesday,
	Thursday,
	Friday,
	Saturday,
}

/// Monthly only. `day` is 1 to 31 or −1 for the last day; `nth` is 1 to 5 or
/// −1 for the last such weekday.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BillRepeatPattern {
	Day { day: i8 },
	Weekday { weekday: BillWeekday, nth: i8 },
}

/// Where a date that lands on a weekend moves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BillWeekend {
	FridayBefore,
	MondayAfter,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum BillRepeatEnd {
	Never,
	AfterOccurrences {
		count: u32,
	},
	/// `YYYY-MM-DD`.
	OnDate {
		date: String,
	},
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BillRepeat {
	/// `YYYY-MM-DD`.
	pub start: String,
	pub interval: u32,
	pub frequency: BillFrequency,
	/// Empty means the start's day of the month.
	pub patterns: Vec<BillRepeatPattern>,
	/// `nil` leaves weekends alone.
	pub weekend: Option<BillWeekend>,
	pub end: BillRepeatEnd,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum BillDate {
	/// `YYYY-MM-DD`.
	Once {
		date: String,
	},
	Repeat {
		repeat: BillRepeat,
	},
}

/// What the bill editor saves.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BillDraft {
	pub name: Option<String>,
	pub payee_id: Option<String>,
	/// A bill without an account never posts.
	pub account_id: Option<String>,
	pub amount: BillAmount,
	pub date: BillDate,
	pub category_id: Option<String>,
	/// R-BILL-2: posts on its date, or waits in Upcoming for Paid.
	pub auto_post: bool,
}

/// How a bill's amount matches a payment (`ScheduleAmountCell`,
/// `dc/components/schedules/SchedulesTable.tsx:66-147`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BillAmountKind {
	Exact,
	About,
	Between,
}

/// Why a bill is read-only ("Edit in Actual").
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BillReadOnlyReason {
	/// The schedule's rule is gone.
	MissingRule,
	/// The rule's stored conditions or actions can't be read.
	UnreadableRule,
	/// No amount condition `extractScheduleConds` finds
	/// (`lc/shared/schedules.ts:255-281`).
	NoAmount,
	/// The amount, or an end of its range, isn't a whole number of minor units.
	UnsupportedAmount,
	/// No date condition.
	NoDate,
	/// A date or repeat shape the bill editor can't show.
	UnsupportedDate,
}

/// What a bill's rule holds besides what the bill editor shows. Saving
/// keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BillCustomPart {
	/// Conditions besides payee, account, date and amount.
	ExtraConditions,
	/// Actions besides the bill's link and one plain category.
	ExtraActions,
}

/// Pause and End both complete the schedule in Actual; only the label differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BillState {
	Active,
	Paused,
	Ended,
}

/// Actual's schedule status, as of the `today` the query was given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BillStatus {
	Paid,
	Due,
	Upcoming,
	Missed,
	Scheduled,
	Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BillSummary {
	pub id: String,
	pub name: Option<String>,
	pub payee_id: Option<String>,
	pub payee_name: Option<String>,
	pub account_id: Option<String>,
	pub account_name: Option<String>,
	pub category_id: Option<String>,
	/// What a post writes, minor units; negative for a bill, positive for income.
	pub scheduled_amount: Option<i64>,
	pub is_income: bool,
	pub auto_post: bool,
	/// `YYYY-MM-DD`.
	pub next_date: Option<String>,
	pub state: BillState,
	pub status: BillStatus,
	/// The editor's model. `nil` when Actual holds a schedule shape the bill
	/// screen can't show; that bill is read-only, "Edit in Actual".
	pub draft: Option<BillDraft>,
	/// The schedule's rule also holds conditions or actions the bill doesn't
	/// show; saving keeps them.
	pub custom_rule: bool,
	/// v13. `nil` when the schedule has no amount condition.
	#[uniffi(default = None)]
	pub amount_kind: Option<BillAmountKind>,
	/// v13. Why `draft` is `nil`; set exactly when it is.
	#[uniffi(default = None)]
	pub read_only_reason: Option<BillReadOnlyReason>,
	/// v13. What makes `custom_rule` true; empty exactly when it is false.
	#[uniffi(default = [])]
	pub custom_parts: Vec<BillCustomPart>,
	/// v13. The amount of the most recent live transaction linked to the
	/// bill, in the bill's account when it has one, split lines left out.
	#[uniffi(default = None)]
	pub last_paid_amount: Option<i64>,
	/// v13. That transaction's date, `YYYY-MM-DD`.
	#[uniffi(default = None)]
	pub last_paid_date: Option<String>,
	/// v13. Whether Skip moves the bill: active, with a later date to move to.
	#[uniffi(default = false)]
	pub can_skip: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BillDetail {
	pub bill: BillSummary,
	/// The next few occurrences from `today`, weekend moves applied.
	pub next_dates: Vec<String>,
	/// Whether Paid dated `today` counts as paying the next occurrence in
	/// Actual. When it doesn't, Paid also moves the bill on itself.
	pub paid_today_counts: bool,
}

/// One occurrence in the upcoming tray or timeline.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct UpcomingBill {
	pub bill_id: String,
	pub name: Option<String>,
	pub payee_name: Option<String>,
	pub account_id: Option<String>,
	/// v13.
	#[uniffi(default = None)]
	pub account_name: Option<String>,
	/// v13. The bill's category, `nil` for a read-only bill or one without one.
	#[uniffi(default = None)]
	pub category_name: Option<String>,
	/// v13.
	#[uniffi(default = None)]
	pub amount_kind: Option<BillAmountKind>,
	/// `YYYY-MM-DD`.
	pub date: String,
	pub scheduled_amount: Option<i64>,
	pub is_income: bool,
	pub auto_post: bool,
	/// Due, Missed or Upcoming.
	pub status: BillStatus,
}

const UPCOMING_DAYS: i64 = 7;
const NEXT_DATES: usize = 5;
const MAX_PREVIEW_DATES: u32 = 100;

fn weekday_of(weekday: BillWeekday) -> Weekday {
	match weekday {
		BillWeekday::Sunday => Weekday::Sun,
		BillWeekday::Monday => Weekday::Mon,
		BillWeekday::Tuesday => Weekday::Tue,
		BillWeekday::Wednesday => Weekday::Wed,
		BillWeekday::Thursday => Weekday::Thu,
		BillWeekday::Friday => Weekday::Fri,
		BillWeekday::Saturday => Weekday::Sat,
	}
}

fn bill_weekday(weekday: Weekday) -> BillWeekday {
	match weekday {
		Weekday::Sun => BillWeekday::Sunday,
		Weekday::Mon => BillWeekday::Monday,
		Weekday::Tue => BillWeekday::Tuesday,
		Weekday::Wed => BillWeekday::Wednesday,
		Weekday::Thu => BillWeekday::Thursday,
		Weekday::Fri => BillWeekday::Friday,
		Weekday::Sat => BillWeekday::Saturday,
	}
}

fn some_id<T>(id: &Option<String>, make: impl Fn(&str) -> T) -> Option<T> {
	id.as_deref().filter(|id| !id.is_empty()).map(make)
}

/// The core bill a draft describes.
pub(crate) fn bill(draft: &BillDraft) -> Result<Bill, CoreError> {
	let amount = match draft.amount {
		BillAmount::Exact { amount } => CoreAmount::Exact(Money::from_minor(amount)),
		BillAmount::About { amount } => CoreAmount::About(Money::from_minor(amount)),
		BillAmount::Between { low, high } => {
			CoreAmount::Between(Money::from_minor(low), Money::from_minor(high))
		}
	};
	let date = match &draft.date {
		BillDate::Once { date } => CoreDate::Once(civil_date(date)?),
		BillDate::Repeat { repeat } => CoreDate::Repeat(Repeat {
			start: civil_date(&repeat.start)?,
			interval: repeat.interval,
			frequency: match repeat.frequency {
				BillFrequency::Daily => Frequency::Daily,
				BillFrequency::Weekly => Frequency::Weekly,
				BillFrequency::Monthly => Frequency::Monthly,
				BillFrequency::Yearly => Frequency::Yearly,
			},
			patterns: repeat
				.patterns
				.iter()
				.map(|pattern| match *pattern {
					BillRepeatPattern::Day { day } => RepeatPattern::Day(day),
					BillRepeatPattern::Weekday { weekday, nth } => {
						RepeatPattern::Weekday(weekday_of(weekday), nth)
					}
				})
				.collect(),
			weekend: repeat.weekend.map(|weekend| match weekend {
				BillWeekend::FridayBefore => WeekendMove::Before,
				BillWeekend::MondayAfter => WeekendMove::After,
			}),
			end: match &repeat.end {
				BillRepeatEnd::Never => RepeatEnd::Never,
				BillRepeatEnd::AfterOccurrences { count } => RepeatEnd::AfterOccurrences(*count),
				BillRepeatEnd::OnDate { date } => RepeatEnd::OnDate(civil_date(date)?),
			},
		}),
	};
	Ok(Bill {
		name: draft.name.clone(),
		payee: some_id(&draft.payee_id, |id| PayeeId::new(id)),
		account: some_id(&draft.account_id, |id| AccountId::new(id)),
		amount,
		date,
		category: some_id(&draft.category_id, |id| CategoryId::new(id)),
		posts_transaction: draft.auto_post,
	})
}

fn draft(bill: &Bill) -> BillDraft {
	BillDraft {
		name: bill.name.clone(),
		payee_id: bill.payee.as_ref().map(ToString::to_string),
		account_id: bill.account.as_ref().map(ToString::to_string),
		amount: match bill.amount {
			CoreAmount::Exact(amount) => BillAmount::Exact {
				amount: amount.minor(),
			},
			CoreAmount::About(amount) => BillAmount::About {
				amount: amount.minor(),
			},
			CoreAmount::Between(low, high) => BillAmount::Between {
				low: low.minor(),
				high: high.minor(),
			},
		},
		date: match &bill.date {
			CoreDate::Once(date) => BillDate::Once {
				date: date.to_string(),
			},
			CoreDate::Repeat(repeat) => BillDate::Repeat {
				repeat: BillRepeat {
					start: repeat.start.to_string(),
					interval: repeat.interval,
					frequency: match repeat.frequency {
						Frequency::Daily => BillFrequency::Daily,
						Frequency::Weekly => BillFrequency::Weekly,
						Frequency::Monthly => BillFrequency::Monthly,
						Frequency::Yearly => BillFrequency::Yearly,
					},
					patterns: repeat
						.patterns
						.iter()
						.map(|pattern| match *pattern {
							RepeatPattern::Day(day) => BillRepeatPattern::Day { day },
							RepeatPattern::Weekday(weekday, nth) => BillRepeatPattern::Weekday {
								weekday: bill_weekday(weekday),
								nth,
							},
						})
						.collect(),
					weekend: repeat.weekend.map(|weekend| match weekend {
						WeekendMove::Before => BillWeekend::FridayBefore,
						WeekendMove::After => BillWeekend::MondayAfter,
					}),
					end: match repeat.end {
						RepeatEnd::Never => BillRepeatEnd::Never,
						RepeatEnd::AfterOccurrences(count) => {
							BillRepeatEnd::AfterOccurrences { count }
						}
						RepeatEnd::OnDate(date) => BillRepeatEnd::OnDate {
							date: date.to_string(),
						},
					},
				},
			},
		},
		category_id: bill.category.as_ref().map(ToString::to_string),
		auto_post: bill.posts_transaction,
	}
}

fn bill_status_of(status: ScheduleStatus) -> BillStatus {
	match status {
		ScheduleStatus::Paid => BillStatus::Paid,
		ScheduleStatus::Due => BillStatus::Due,
		ScheduleStatus::Upcoming => BillStatus::Upcoming,
		ScheduleStatus::Missed => BillStatus::Missed,
		ScheduleStatus::Scheduled => BillStatus::Scheduled,
		ScheduleStatus::Completed => BillStatus::Completed,
	}
}

fn payee_name(data: &BudgetData, payee: &PayeeId) -> Option<String> {
	let row = data.payees.get(payee)?;
	match &row.transfer_account {
		Some(account) => data.accounts.get(account).map(|a| a.name.clone()),
		None => Some(row.name.clone()),
	}
}

fn read_only_reason(reason: NotABill) -> BillReadOnlyReason {
	match reason {
		NotABill::MissingRule => BillReadOnlyReason::MissingRule,
		NotABill::UnreadableRule => BillReadOnlyReason::UnreadableRule,
		NotABill::NoAmount => BillReadOnlyReason::NoAmount,
		NotABill::UnsupportedAmount => BillReadOnlyReason::UnsupportedAmount,
		NotABill::NoDate => BillReadOnlyReason::NoDate,
		NotABill::UnsupportedDate => BillReadOnlyReason::UnsupportedDate,
	}
}

fn summary(data: &BudgetData, id: &ScheduleId, today: CivilDate) -> Result<BillSummary, CoreError> {
	let schedule = data
		.schedules
		.get(id)
		.filter(|schedule| !schedule.tombstone)
		.ok_or_else(|| CoreError::NotFound {
			entity: "bill",
			id: id.to_string(),
		})?;
	let read = veritas_rules::read_bill(schedule, data.rules.get(&schedule.rule), data);
	let read_only = read.as_ref().err().copied().map(read_only_reason);
	let view = read.ok();
	// A schedule whose rule is missing or unreadable has no facts to read,
	// but it is still listed, read-only.
	let readable = !matches!(
		read_only,
		Some(BillReadOnlyReason::MissingRule | BillReadOnlyReason::UnreadableRule)
	);
	let last_paid = if readable {
		veritas_rules::last_paid(data, id)?
	} else {
		None
	};
	let status = if readable {
		bill_status(data, id, UPCOMING_DAYS, today)?
	} else {
		veritas_rules::status(
			schedule.next_date,
			schedule.completed,
			false,
			UPCOMING_DAYS,
			today,
		)
	};
	let (payee, account, amount) =
		veritas_rules::schedule_parts(data, id).unwrap_or((None, None, None));
	let state = match (schedule.completed, data.schedule_meta.get(id)) {
		(false, _) => BillState::Active,
		(true, Some(meta)) if meta.paused => BillState::Paused,
		(true, _) => BillState::Ended,
	};
	Ok(BillSummary {
		id: id.to_string(),
		name: schedule.name.clone(),
		payee_name: payee.as_ref().and_then(|p| payee_name(data, p)),
		payee_id: payee.as_ref().map(ToString::to_string),
		account_name: account
			.as_ref()
			.and_then(|a| data.accounts.get(a))
			.map(|a| a.name.clone()),
		account_id: account.as_ref().map(ToString::to_string),
		category_id: view
			.as_ref()
			.and_then(|view| view.bill.category.as_ref())
			.map(ToString::to_string),
		scheduled_amount: amount,
		is_income: amount.is_some_and(|amount| amount > 0),
		auto_post: schedule.posts_transaction,
		next_date: schedule.next_date.map(|date| date.to_string()),
		state,
		status: bill_status_of(status),
		custom_rule: view.as_ref().is_some_and(|view| view.custom_rule),
		custom_parts: view.as_ref().map_or_else(Vec::new, |view| {
			[
				(view.extra_conditions, BillCustomPart::ExtraConditions),
				(view.extra_actions, BillCustomPart::ExtraActions),
			]
			.into_iter()
			.filter_map(|(on, part)| on.then_some(part))
			.collect()
		}),
		draft: view.map(|view| draft(&view.bill)),
		amount_kind: veritas_rules::amount_kind(schedule, data).map(|kind| match kind {
			AmountKind::Exact => BillAmountKind::Exact,
			AmountKind::About => BillAmountKind::About,
			AmountKind::Between => BillAmountKind::Between,
		}),
		read_only_reason: read_only,
		last_paid_amount: last_paid.map(|(_, amount)| amount.minor()),
		last_paid_date: last_paid.map(|(date, _)| date.to_string()),
		can_skip: readable && veritas_rules::can_skip(data, id)?,
	})
}

fn sorted(mut bills: Vec<BillSummary>) -> Vec<BillSummary> {
	bills.sort_by(|a, b| {
		(a.state != BillState::Active)
			.cmp(&(b.state != BillState::Active))
			.then_with(|| a.next_date.is_none().cmp(&b.next_date.is_none()))
			.then_with(|| a.next_date.cmp(&b.next_date))
			.then_with(|| a.name.cmp(&b.name))
			.then_with(|| a.id.cmp(&b.id))
	});
	bills
}

pub(crate) fn bills(data: &BudgetData, today: CivilDate) -> Result<Vec<BillSummary>, CoreError> {
	let mut out = Vec::new();
	for schedule in data.schedules.values().filter(|s| !s.tombstone) {
		out.push(summary(data, &schedule.id, today)?);
	}
	Ok(sorted(out))
}

pub(crate) fn bill_detail(
	data: &BudgetData,
	id: &str,
	today: CivilDate,
) -> Result<BillDetail, CoreError> {
	let id = ScheduleId::new(id);
	let bill = summary(data, &id, today)?;
	let far = today
		.naive()
		.checked_add_days(Days::new(366 * 5))
		.map_or(today, CivilDate::from);
	let next_dates = if bill.state == BillState::Active {
		let start = data.schedules[&id]
			.next_date
			.map_or(today, |next| next.min(today));
		bill_dates(data, &id, start.max(today), far)
			.unwrap_or_default()
			.into_iter()
			.take(NEXT_DATES)
			.map(|date| date.to_string())
			.collect()
	} else {
		Vec::new()
	};
	Ok(BillDetail {
		paid_today_counts: match bill.read_only_reason {
			Some(BillReadOnlyReason::MissingRule | BillReadOnlyReason::UnreadableRule) => false,
			_ => veritas_rules::counts_as_paid(data, &id, today)?,
		},
		bill,
		next_dates,
	})
}

/// Every occurrence from the bill's next date through `today + days` that
/// isn't paid yet: a due or missed next date first, then the dates after it.
pub(crate) fn upcoming(
	data: &BudgetData,
	days: u32,
	today: CivilDate,
) -> Result<Vec<UpcomingBill>, CoreError> {
	let end = today
		.naive()
		.checked_add_days(Days::new(u64::from(days)))
		.map(CivilDate::from)
		.ok_or(CoreError::Overflow)?;
	let mut out = Vec::new();
	for schedule in data
		.schedules
		.values()
		.filter(|s| !s.tombstone && !s.completed)
	{
		let Some(next) = schedule.next_date else {
			continue;
		};
		let bill = summary(data, &schedule.id, today)?;
		let dates = bill_dates(data, &schedule.id, next, end).unwrap_or_default();
		for date in dates {
			let status = if date == next {
				match bill.status {
					BillStatus::Paid => continue,
					BillStatus::Due | BillStatus::Missed => bill.status,
					_ => BillStatus::Upcoming,
				}
			} else if date < today {
				BillStatus::Missed
			} else if date == today {
				BillStatus::Due
			} else {
				BillStatus::Upcoming
			};
			out.push(UpcomingBill {
				bill_id: bill.id.clone(),
				name: bill.name.clone(),
				payee_name: bill.payee_name.clone(),
				account_id: bill.account_id.clone(),
				account_name: bill.account_name.clone(),
				category_name: bill
					.category_id
					.as_ref()
					.and_then(|id| data.categories.get(&CategoryId::new(id.as_str())))
					.filter(|category| !category.tombstone)
					.map(|category| category.name.clone()),
				amount_kind: bill.amount_kind,
				date: date.to_string(),
				scheduled_amount: bill.scheduled_amount,
				is_income: bill.is_income,
				auto_post: bill.auto_post,
				status,
			});
		}
	}
	out.sort_by(|a, b| {
		a.date
			.cmp(&b.date)
			.then_with(|| a.name.cmp(&b.name))
			.then_with(|| a.bill_id.cmp(&b.bill_id))
	});
	Ok(out)
}

/// The dates the schedule editor previews for a draft's date, up to
/// `count` (Actual's editor shows 3, `dc/hooks/useScheduleEdit.ts:325-328`;
/// its repeat picker 4, `dc/components/select/RecurringSchedulePicker.tsx:412-415`).
pub(crate) fn preview_dates(
	model: &BillDraft,
	today: CivilDate,
	count: u32,
) -> Result<Vec<String>, CoreError> {
	if count > MAX_PREVIEW_DATES {
		return Err(CoreError::Invalid(format!(
			"Preview at most {MAX_PREVIEW_DATES} dates"
		)));
	}
	Ok(bill(model)?
		.upcoming_dates(today, count as usize)?
		.into_iter()
		.map(|date| date.to_string())
		.collect())
}

/// The next dates Actual's schedule editor previews for an unsaved or edited
/// bill, at most `count` (up to 100): a one-time date when it is `today` or
/// later, or the repeat's next occurrences from `today`, each moved off the
/// weekend. `Invalid` where Actual's editor says "Invalid rule". Nothing is
/// saved, and a draft the preview accepts can still fail `createBill`'s checks.
#[uniffi::export]
pub fn preview_bill_dates(
	bill: BillDraft,
	today: String,
	count: u32,
) -> Result<Vec<String>, FfiError> {
	crate::guard(|| preview_dates(&bill, civil_date(&today)?, count))
}

#[uniffi::export]
impl Session {
	/// Every bill, active ones first by next date, then paused and ended
	/// ones. `today` is the local date, `YYYY-MM-DD`.
	pub fn bills(&self, today: String) -> Result<Vec<BillSummary>, FfiError> {
		self.core
			.call(|session| bills(session.data()?, civil_date(&today)?))
	}

	/// One bill for the editor, with its next few dates.
	pub fn bill(&self, bill_id: String, today: String) -> Result<BillDetail, FfiError> {
		self.core
			.call(|session| bill_detail(session.data()?, &bill_id, civil_date(&today)?))
	}

	/// The upcoming tray (Actual shows 7 days) and the timeline: unpaid
	/// occurrences through `today + days`, oldest first.
	pub fn upcoming(&self, days: u32, today: String) -> Result<Vec<UpcomingBill>, FfiError> {
		self.core
			.call(|session| upcoming(session.data()?, days, civil_date(&today)?))
	}
}
