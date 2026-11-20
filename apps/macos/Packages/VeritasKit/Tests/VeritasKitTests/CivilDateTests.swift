import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// The core's dates are Gregorian whatever calendar the user reads dates in.
/// Each user calendar is passed in, so nothing global changes.
@MainActor
struct CivilDateTests {
	static let tokyo = TimeZone(identifier: "Asia/Tokyo")!

	static func calendar(_ identifier: Calendar.Identifier) -> Calendar {
		var calendar = Calendar(identifier: identifier)
		calendar.timeZone = tokyo
		return calendar
	}

	/// 2026-11-19 at 08:00 in Tokyo, which is still 2026-11-18 in UTC.
	static let morning: Date = {
		var calendar = Calendar(identifier: .gregorian)
		calendar.timeZone = tokyo
		return calendar.date(from: DateComponents(year: 2026, month: 11, day: 19, hour: 8))!
	}()

	@Test(arguments: [Calendar.Identifier.buddhist, .japanese, .gregorian])
	func wireDatesStayGregorianInTheUsersCalendar(_ identifier: Calendar.Identifier) throws {
		let user = Self.calendar(identifier)
		#expect(WorkspaceModel.today(Self.morning, calendar: user) == "2026-11-19", "the local date, in its own time zone")
		#expect(CivilDate.text(Self.morning, calendar: user) == "2026-11-19")
		#expect(CivilDate.month(Self.morning, calendar: user) == Month(year: 2026, month: 11))
		#expect(BudgetScreenModel.currentMonth(Self.morning, calendar: user) == Month(year: 2026, month: 11))

		let parsed = try #require(CivilDate.date("2026-11-19", calendar: user))
		#expect(WorkspaceModel.today(parsed, calendar: user) == "2026-11-19", "a parsed date writes back unchanged")
		let first = try #require(Month(year: 2026, month: 11).date(calendar: user))
		#expect(CivilDate.text(first, calendar: user) == "2026-11-01")
	}

	@Test func theUsersCalendarReadsTheSameDayDifferently() {
		// What the fix guards against: these calendars number the year otherwise.
		#expect(Self.calendar(.buddhist).component(.year, from: Self.morning) == 2569)
		#expect(Self.calendar(.japanese).component(.year, from: Self.morning) == 8)
	}
}
