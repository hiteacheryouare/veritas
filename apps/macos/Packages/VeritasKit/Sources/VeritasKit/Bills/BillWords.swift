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

	/// "Nov 12" for `2026-11-12`.
	public static func day(_ date: String) -> String {
		ReportPeriod.interval(date)
	}

	static func repeating(_ rule: BillRepeat) -> String {
		let every = rule.interval
		switch rule.frequency {
		case .daily: return every == 1 ? "Daily" : "Every \(every) days"
		case .weekly: return every == 1 ? "Weekly" : "Every \(every) weeks"
		case .yearly: return every == 1 ? "Yearly" : "Every \(every) years"
		case .monthly:
			let days = rule.patterns.isEmpty ? [ordinal(Int(rule.start.suffix(2)) ?? 1)] : rule.patterns.map(pattern)
			let list = ListFormatter.localizedString(byJoining: days)
			return every == 1 ? "\(list) of month" : "Every \(every) months, \(list)"
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

	public static func status(_ status: BillStatus) -> String {
		switch status {
		case .paid: "Paid"
		case .due: "Due"
		case .upcoming: "Upcoming"
		case .missed: "Missed"
		case .scheduled: "Scheduled"
		case .completed: "Completed"
		}
	}
}
