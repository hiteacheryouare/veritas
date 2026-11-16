import Foundation

/// The core's `YYYY-MM-DD` dates as the `Date` a date picker edits, at local midnight.
public enum CivilDate {
	public static func date(_ text: String, calendar: Calendar = .current) -> Date? {
		let parts = text.split(separator: "-").compactMap { Int($0) }
		guard parts.count == 3 else { return nil }
		return calendar.date(from: DateComponents(year: parts[0], month: parts[1], day: parts[2]))
	}

	public static func text(_ date: Date, calendar: Calendar = .current) -> String {
		WorkspaceModel.today(date, calendar: calendar)
	}
}
