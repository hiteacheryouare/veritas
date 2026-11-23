import Foundation
import VeritasCore

/// How a bill's schedule reads in the list: "1st of month", "Every 2 weeks".
public enum BillWords {
	public static func schedule(_ date: BillDate) -> String {
		switch date {
		case .once(let date): return "Once, \(day(date))"
		case .repeat(let rule): return repeating(rule)
		}
	}

	/// A bill's whole schedule in one phrase, for a rule's date condition:
	/// "31st of month, from Jan 31, 2026, 24 times, a weekend date moves to the Friday before".
	public static func sentence(_ date: BillDate) -> String {
		switch date {
		case .once(let date):
			return "once, \(long(date))"
		case .repeat(let rule):
			let schedule = repeating(rule)
			var parts = ["\(schedule.prefix(1).lowercased())\(schedule.dropFirst())", "from \(long(rule.start))"]
			switch rule.end {
			case .never: break
			case .afterOccurrences(let count): parts.append(count == 1 ? "once" : "\(count) times")
			case .onDate(let date): parts.append("until \(long(date))")
			}
			switch rule.weekend {
			case .fridayBefore: parts.append("a weekend date moves to the Friday before")
			case .mondayAfter: parts.append("a weekend date moves to the Monday after")
			case nil: break
			}
			return parts.joined(separator: ", ")
		}
	}

	/// "Jan 31, 2026".
	static func long(_ date: String) -> String {
		CivilDate.date(date)?.formatted(.dateTime.month(.abbreviated).day().year()) ?? date
	}

	/// "Nov 12" for `2026-11-12`, and "Feb 29, 2028" for a date in a later
	/// year or more than twelve months away, so a far date never reads as a near one.
	public static func day(_ date: String) -> String {
		day(date, today: WorkspaceModel.today())
	}

	public static func day(_ date: String, today: String) -> String {
		guard let value = CivilDate.date(date), let now = CivilDate.date(today) else { return ReportPeriod.interval(date) }
		let calendar = Calendar.current.civil
		let yearAway = calendar.date(byAdding: .month, value: 12, to: now) ?? now
		if calendar.component(.year, from: value) > calendar.component(.year, from: now) || value > yearAway {
			return value.formatted(.dateTime.month(.abbreviated).day().year())
		}
		return ReportPeriod.interval(date)
	}

	static func repeating(_ rule: BillRepeat) -> String {
		let every = rule.interval
		switch rule.frequency {
		case .daily: return every == 1 ? "Daily" : "Every \(every) days"
		case .weekly: return every == 1 ? "Weekly" : "Every \(every) weeks"
		case .yearly: return every == 1 ? "Yearly" : "Every \(every) years"
		case .monthly:
			let days = rule.patterns.isEmpty ? [ordinal(Int(rule.start.suffix(2)) ?? 1)] : rule.patterns.map { pattern($0).lowercasedLast }
			let list = ListFormatter.localizedString(byJoining: days)
			return every == 1 ? "\(list.prefix(1).uppercased())\(list.dropFirst()) of month" : "Every \(every) months, \(list)"
		}
	}

	public static func pattern(_ pattern: BillRepeatPattern) -> String {
		switch pattern {
		case .day(let day): day == -1 ? "Last day" : ordinal(Int(day))
		case .weekday(let weekday, let nth): "\(nth == -1 ? "Last" : ordinal(Int(nth))) \(Self.weekday(weekday))"
		}
	}

	public static func weekday(_ weekday: BillWeekday) -> String {
		switch weekday {
		case .sunday: "Sunday"
		case .monday: "Monday"
		case .tuesday: "Tuesday"
		case .wednesday: "Wednesday"
		case .thursday: "Thursday"
		case .friday: "Friday"
		case .saturday: "Saturday"
		}
	}

	public static func ordinal(_ number: Int) -> String {
		let formatter = NumberFormatter()
		formatter.numberStyle = .ordinal
		return formatter.string(from: NSNumber(value: number)) ?? "\(number)"
	}

	/// "November 2026" for any date in it.
	public static func month(_ date: String) -> String {
		Month(date: date)?.date().map { $0.formatted(.dateTime.month(.wide).year()) } ?? date
	}

	/// "about 90.00", "2,400.00 to 2,600.00": the magnitudes on the register scale.
	public static func amount(_ amount: BillAmount, format: MoneyFormat) -> (qualifier: String?, figures: String) {
		switch amount.magnitudes {
		case .exact(let amount): (nil, format.amount(amount, on: .register))
		case .about(let amount): ("about", format.amount(amount, on: .register))
		case .between(let low, let high): (nil, "\(format.amount(low, on: .register)) to \(format.amount(high, on: .register))")
		}
	}

	/// "month" or "months", for "Every 2 months".
	public static func unit(_ frequency: BillFrequency, count: UInt32) -> String {
		let one = count == 1
		switch frequency {
		case .daily: return one ? "day" : "days"
		case .weekly: return one ? "week" : "weeks"
		case .monthly: return one ? "month" : "months"
		case .yearly: return one ? "year" : "years"
		}
	}

	public static func state(_ state: BillState) -> String {
		switch state {
		case .active: "Active"
		case .paused: "Paused"
		case .ended: "Ended"
		}
	}

	/// The tray's button for one occurrence: "Paid", or "Received" for income.
	public static func paid(isIncome: Bool) -> String {
		isIncome ? "Received" : "Paid"
	}

	/// The tray's inline confirm: "Mark paid", or "Mark received" for income.
	public static func markPaid(isIncome: Bool) -> String {
		isIncome ? "Mark received" : "Mark paid"
	}

	/// The date picker's label beside it: "Paid on" or "Received on".
	public static func paidOn(isIncome: Bool) -> String {
		isIncome ? "Received on" : "Paid on"
	}

	/// "Post for Nov 15": posts the occurrence on its own date, not today's.
	public static func postFor(_ date: String) -> String {
		"Post for \(day(date))"
	}

	/// Asked before Post today when Actual won't count a payment today as
	/// paying that occurrence (`UpcomingBill.paidTodayCounts`, contract v15).
	public static func postTodayConfirm(_ date: String, isIncome: Bool) -> String {
		isIncome
			? "This won't count as the \(day(date)) income. Post today anyway?"
			: "This won't pay the \(day(date)) bill. Post today anyway?"
	}

	/// Beside the editor's title when the bill matches or sets more than the
	/// editor shows (`BillSummary.customRule`). Saving keeps those parts.
	public static let customParts = "Has settings only Actual shows"

	/// The editor's line under Auto-post for an estimated amount. Nil for an exact one.
	public static func estimate(_ kind: BillAmountKind) -> String? {
		switch kind {
		case .exact: nil
		case .about: "Auto-post adds a transaction for this amount."
		case .between: "Auto-post adds a transaction for the middle of the range."
		}
	}

	public static func status(_ status: BillStatus, isIncome: Bool) -> String {
		switch status {
		case .paid: paid(isIncome: isIncome)
		case .due: "Due"
		case .upcoming: "Upcoming"
		case .missed: "Missed"
		case .scheduled: "Scheduled"
		case .completed: "Completed"
		}
	}
}

private extension String {
	/// "last day" inside a list such as "3rd and last day".
	var lowercasedLast: String {
		hasPrefix("Last") ? "l" + dropFirst() : self
	}
}
