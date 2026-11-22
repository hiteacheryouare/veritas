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
		#expect(words.detail == "Budget $76.00 a month")
		let far = GoalWords(active(pace: .onTrack(etaMonth: Month(year: 2027, month: 8))), format: MoneyFormat(), thisMonth: november)
		#expect(far.note == "On track for August 2027")
	}

	@Test func aGoalBehindSaysWhatItNeedsAsAWarning() {
		let words = GoalWords(active(pace: .behind(neededMonthly: 41_000)), format: MoneyFormat(), thisMonth: november)
		#expect(words.note == "Behind, needs $410.00 a month")
		#expect(words.tone == .low)
		#expect(words.detail == nil)
	}

	@Test func noHistoryIsNeutralAndNothingAddedIsAWarning() {
		let fresh = GoalWords(active(pace: .noHistory(neededMonthly: 7_600)), format: MoneyFormat(), thisMonth: november)
		#expect(fresh.note == "Budget $76.00 a month")
		#expect(fresh.detail == "No ETA yet")
		#expect(fresh.tone == .neutral)
		let stalled = GoalWords(active(pace: .notAdding(neededMonthly: 7_600)), format: MoneyFormat(), thisMonth: november)
		#expect(stalled.note == "Nothing is being added")
		#expect(stalled.tone == .low)
	}

	@Test func withoutAPaceTheSuggestionAndTargetShow() {
		let words = GoalWords(active(pace: nil), format: MoneyFormat(), thisMonth: november)
		#expect(words.note == "Budget $76.00 a month")
		#expect(words.detail == "Target March 2027")
		let undated = GoalWords(active(pace: nil, monthly: nil), format: MoneyFormat(), thisMonth: november)
		#expect(undated.note == "No target date")
	}

	@Test func aSuggestionShowsItsCentsInATwoPlaceBudget() {
		// Fred on #83: DEBT-8.1's G-1, 150000 over 7 months, suggests 21429, never "$214".
		let suggested = GoalWords(active(pace: nil, monthly: 21_429), format: MoneyFormat(), thisMonth: november)
		#expect(suggested.note == "Budget $214.29 a month")
		let behind = GoalWords(active(pace: .behind(neededMonthly: 21_429)), format: MoneyFormat(), thisMonth: november)
		#expect(behind.note == "Behind, needs $214.29 a month")
		let stalled = GoalWords(active(pace: .notAdding(neededMonthly: 21_429)), format: MoneyFormat(), thisMonth: november)
		#expect(stalled.detail == "Budget $214.29 a month")
	}

	@Test func aYenSuggestionIsWholeYenOnTheBudgetScale() {
		// Goal amounts are budget cells (D63), so a yen goal's minor units are whole yen:
		// ¥15,000 over 7 months is ceil(15000 / 7) = 2143.
		let yen = MoneyFormat(currencySymbol: "¥", decimalPlaces: 0)
		let suggested = GoalWords(active(pace: nil, monthly: 2_143), format: yen, thisMonth: november)
		#expect(suggested.note == "Budget ¥2,143 a month")
		let fresh = GoalWords(active(pace: .noHistory(neededMonthly: 2_143)), format: yen, thisMonth: november)
		#expect(fresh.note == "Budget ¥2,143 a month")
	}

	@Test func aYenGoalWithSpendingSaysSoAndOffersNoFigure() {
		// D77: outside a 2-place budget, recorded spending leaves no suggestion and no pace figure.
		let yen = MoneyFormat(currencySymbol: "¥", decimalPlaces: 0)
		let goal = StubBudget.goal("japan", "Japan trip", target: 600_000, available: 231_000, date: "2027-03-01", state: .active, monthly: nil, pace: .spendingRecorded, zeroPlaces: true)
		let words = GoalWords(goal, format: yen, thisMonth: november)
		#expect(words.note == "No ETA: spending recorded")
		#expect(words.detail == nil)
		#expect(words.tone == .neutral)
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
