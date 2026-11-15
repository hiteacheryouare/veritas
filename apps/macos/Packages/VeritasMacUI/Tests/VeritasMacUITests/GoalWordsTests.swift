import Testing
import VeritasCore
import VeritasKit
@testable import VeritasMacUI

@MainActor
struct GoalWordsTests {
	let november = Month(year: 2026, month: 11)

	func active(pace: GoalPace?, monthly: Int64? = 7_600) -> GoalSummary {
		StubBudget.goal("laptop", "New laptop", target: 150_000, available: 112_000, date: "2027-03-01", state: .active, monthly: monthly, pace: pace)
	}

	@Test func anOnTrackGoalNamesItsMonthAndTheSuggestion() {
		let words = GoalWords(active(pace: .onTrack(etaMonth: Month(year: 2027, month: 3))), format: MoneyFormat(), thisMonth: november)
		#expect(words.note == "On track for March")
		#expect(words.tone == .available)
		#expect(words.detail == "Budget $76 a month")
		let far = GoalWords(active(pace: .onTrack(etaMonth: Month(year: 2027, month: 8))), format: MoneyFormat(), thisMonth: november)
		#expect(far.note == "On track for August 2027")
	}

	@Test func aGoalBehindSaysWhatItNeedsAsAWarning() {
		let words = GoalWords(active(pace: .behind(neededMonthly: 41_000)), format: MoneyFormat(), thisMonth: november)
		#expect(words.note == "Behind, needs $410 a month")
		#expect(words.tone == .low)
		#expect(words.detail == nil)
	}

	@Test func noHistoryIsNeutralAndNothingAddedIsAWarning() {
		let fresh = GoalWords(active(pace: .noHistory(neededMonthly: 7_600)), format: MoneyFormat(), thisMonth: november)
		#expect(fresh.note == "Budget $76 a month")
		#expect(fresh.detail == "No ETA yet")
		#expect(fresh.tone == .neutral)
		let stalled = GoalWords(active(pace: .notAdding(neededMonthly: 7_600)), format: MoneyFormat(), thisMonth: november)
		#expect(stalled.note == "Nothing is being added")
		#expect(stalled.tone == .low)
	}

	@Test func withoutAPaceTheSuggestionAndTargetShow() {
		let words = GoalWords(active(pace: nil), format: MoneyFormat(), thisMonth: november)
		#expect(words.note == "Budget $76 a month")
		#expect(words.detail == "Target March 2027")
		let undated = GoalWords(active(pace: nil, monthly: nil), format: MoneyFormat(), thisMonth: november)
		#expect(undated.note == "No target date")
	}
}
