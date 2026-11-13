import Foundation
import VeritasCore

extension Month: Comparable {
	public static func < (lhs: Month, rhs: Month) -> Bool {
		(lhs.year, lhs.month) < (rhs.year, rhs.month)
	}

	public func adding(months: Int) -> Month {
		let index = Int(year) * 12 + Int(month) - 1 + months
		return Month(year: Int32(index.floorDiv(12)), month: UInt32(index.floorMod(12) + 1))
	}

	public func months(through last: Month) -> Int {
		(Int(last.year) - Int(year)) * 12 + Int(last.month) - Int(month)
	}

	/// `YYYY-MM-01`.
	public var firstDay: String {
		String(format: "%04d-%02d-01", year, month)
	}

	/// The month's last civil date, `YYYY-MM-DD`.
	public var lastDay: String {
		String(format: "%04d-%02d-%02d", year, month, dayCount)
	}

	public var dayCount: Int {
		switch month {
		case 2: (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 ? 29 : 28
		case 4, 6, 9, 11: 30
		default: 31
		}
	}

	/// The month `YYYY-MM-DD` falls in.
	public init?(date: String) {
		let parts = date.split(separator: "-")
		guard parts.count >= 2, let year = Int32(parts[0]), let month = UInt32(parts[1]), (1...12).contains(month) else { return nil }
		self.init(year: year, month: month)
	}

	func date(calendar: Calendar = .current) -> Date? {
		calendar.date(from: DateComponents(year: Int(year), month: Int(month), day: 1))
	}
}

private extension Int {
	func floorDiv(_ divisor: Int) -> Int { (self - floorMod(divisor)) / divisor }
	func floorMod(_ divisor: Int) -> Int { ((self % divisor) + divisor) % divisor }
}

/// An inclusive run of whole months, the unit every report screen steps by.
public struct ReportPeriod: Hashable, Sendable {
	public var first: Month
	public var last: Month

	public init(first: Month, last: Month) {
		self.first = min(first, last)
		self.last = max(first, last)
	}

	public var length: Int { first.months(through: last) + 1 }

	public var months: [Month] {
		(0..<length).map { first.adding(months: $0) }
	}

	public func shifted(by months: Int) -> ReportPeriod {
		ReportPeriod(first: first.adding(months: months), last: last.adding(months: months))
	}

	/// "October 2026", "June to November 2026", "November 2025 to April 2026".
	public var title: String {
		if first == last { return Self.monthYear(first) }
		if first.year == last.year { return "\(Self.monthName(first)) to \(Self.monthYear(last))" }
		return "\(Self.monthYear(first)) to \(Self.monthYear(last))"
	}

	public static func monthName(_ month: Month) -> String {
		month.date()?.formatted(.dateTime.month(.wide)) ?? ""
	}

	public static func monthYear(_ month: Month) -> String {
		month.date()?.formatted(.dateTime.month(.wide).year()) ?? ""
	}

	/// A custom report's interval as a heading: "Jun" for `2026-06`, "Jun 3"
	/// for a day or week, the year as it is.
	public static func interval(_ text: String) -> String {
		let parts = text.split(separator: "-").compactMap { Int($0) }
		switch parts.count {
		case 2: return Month(year: Int32(parts[0]), month: UInt32(parts[1])).date().map { $0.formatted(.dateTime.month(.abbreviated)) } ?? text
		case 3:
			let date = Calendar.current.date(from: DateComponents(year: parts[0], month: parts[1], day: parts[2]))
			return date?.formatted(.dateTime.month(.abbreviated).day()) ?? text
		default: return text
		}
	}

	/// "Jun", for table headings and chart axes.
	public static func shortMonth(_ month: Month) -> String {
		month.date()?.formatted(.dateTime.month(.abbreviated)) ?? ""
	}
}

/// The Net Profit and standard reports' range control (design/mockups/net-profit.html).
public enum MonthSpan: String, CaseIterable, Hashable, Sendable {
	case sixMonths
	case year
	case custom

	public var title: String {
		switch self {
		case .sixMonths: "6 months"
		case .year: "Year"
		case .custom: "Custom"
		}
	}

	/// Six months or the calendar year ending at `anchor`. Custom keeps `current`.
	public func period(endingIn anchor: Month, current: ReportPeriod) -> ReportPeriod {
		switch self {
		case .sixMonths: ReportPeriod(first: anchor.adding(months: -5), last: anchor)
		case .year: ReportPeriod(first: Month(year: anchor.year, month: 1), last: Month(year: anchor.year, month: 12))
		case .custom: current
		}
	}
}

/// The Sankey's range control (design/mockups/sankey.html).
public enum SankeySpan: String, CaseIterable, Hashable, Sendable {
	case month
	case quarter
	case year
	case custom

	public var title: String {
		switch self {
		case .month: "Month"
		case .quarter: "Quarter"
		case .year: "Year"
		case .custom: "Custom"
		}
	}

	public func period(containing month: Month, current: ReportPeriod) -> ReportPeriod {
		let quarter = Month(year: month.year, month: (month.month - 1) / 3 * 3 + 1)
		return switch self {
		case .month: ReportPeriod(first: month, last: month)
		case .quarter: ReportPeriod(first: quarter, last: quarter.adding(months: 2))
		case .year: ReportPeriod(first: Month(year: month.year, month: 1), last: Month(year: month.year, month: 12))
		case .custom: current
		}
	}

	public func title(of period: ReportPeriod) -> String {
		switch self {
		case .quarter: "Q\((period.first.month - 1) / 3 + 1) \(period.first.year)"
		case .year: "\(period.first.year)"
		case .month, .custom: period.title
		}
	}
}
