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

	@Test func aZeroPlaceGoalNeverSaysOverOrLeft() {
		// CLS-12.14, with made-up yen: a 300 budget and 500 of spending store
		// -20000 as Available, which is an effect of the two scales.
		let yen = MoneyFormat(currencySymbol: "¥", decimalPlaces: 0)
		let goal = StubBudget.goal("fund", "Fund", target: 30_000, available: -20_000, date: nil, state: .reachedSpent, spent: 50_000)
		let words = GoalWords(goal, format: yen, thisMonth: november)
		#expect(words.note == "Reached → Spent")
		#expect(words.detail == "Spending to date ¥500")
		#expect(GoalWords.progressLabel(goal, format: yen).hasPrefix("available "))
		let dollars = GoalWords(goal, format: MoneyFormat(), thisMonth: november)
		#expect(dollars.detail?.hasSuffix("over") == true, "two places keep the plain wording")
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

	@Test func aReachedGoalSpentPastZeroReadsOverAndOneWithMoneyLeftReadsLeft() {
		let over = GoalWords(StubBudget.goal("tires", "Winter tires", target: 85_000, available: -2_000, date: nil, state: .reachedSpent, spent: 87_000), format: MoneyFormat())
		#expect(over.note == "Reached → Spent, overspent")
		#expect(over.tone == .overspent)
		#expect(over.detail == "$870.00 spent, \u{2212}$20.00 over")
		let left = GoalWords(StubBudget.goal("tires", "Winter tires", target: 85_000, available: 2_000, date: nil, state: .reachedSpent, spent: 83_000), format: MoneyFormat())
		#expect(left.note == "Reached → Spent")
		#expect(left.tone == .available)
		#expect(left.detail == "$830.00 spent, $20.00 left")
	}
}
