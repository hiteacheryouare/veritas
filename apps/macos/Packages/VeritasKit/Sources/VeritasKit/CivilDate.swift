import Foundation
import VeritasCore

public extension Calendar {
	/// The calendar the core's civil dates (`YYYY-MM-DD`, `YYYY-MM`) are
	/// written in: Gregorian, in this calendar's time zone, whatever calendar
	/// the user reads dates in. Dates are shown in the user's own calendar.
	var civil: Calendar {
		var civil = Calendar(identifier: .gregorian)
		civil.timeZone = timeZone
		civil.locale = Locale(identifier: "en_US_POSIX")
		return civil
	}
}

/// The core's `YYYY-MM-DD` dates as the `Date` a date picker edits, at local midnight, and back.
public enum CivilDate {
	public static func date(_ text: String, calendar: Calendar = .current) -> Date? {
		let parts = text.split(separator: "-").compactMap { Int($0) }
		guard parts.count == 3 else { return nil }
		return calendar.civil.date(from: DateComponents(year: parts[0], month: parts[1], day: parts[2]))
	}

	public static func text(_ date: Date, calendar: Calendar = .current) -> String {
		WorkspaceModel.today(date, calendar: calendar)
	}

	public static func month(_ date: Date, calendar: Calendar = .current) -> Month {
		let parts = calendar.civil.dateComponents([.year, .month], from: date)
		return Month(year: Int32(parts.year ?? 2026), month: UInt32(parts.month ?? 1))
	}
}
