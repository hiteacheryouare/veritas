import Testing
import VeritasCore
import VeritasKit
@testable import VeritasMacUI

/// CLS-12.12's JPY example as the Budget and Goals screens word it.
@MainActor
struct ZeroPlaceScreenTextTests {
	let yen = MoneyFormat(currencySymbol: "¥", decimalPlaces: 0)

	@Test func voiceOverReadsTheStateWordOnTheBudgetScale() {
		let food = EnvelopeRow(
			id: "food", name: "Food", hidden: false, budgeted: 30_000, spent: -50_000, available: -20_000,
			carryover: false, tone: .overspent, fill: 0, note: nil
		)
		#expect(EnvelopeRowView.spoken(food, isIncome: false, format: yen) == "Food, budgeted 30,000, spent 50,000, overspent by 20,000")
	}

	@Test func goalAmountsAreWholeYenAndSpendingIsOnTheRegisterScale() {
		let bike = StubBudget.goal("bike", "Bike", target: 150_000, available: -20_000, date: nil, state: .reachedSpent, spent: 50_000)
		let words = GoalWords(bike, format: yen)
		#expect(words.note == "Reached → Spent, overspent")
		#expect(words.tone == .overspent)
		#expect(words.detail == "¥500 spent, \u{2212}¥20,000 over")
	}

	@Test func aSuggestionIsAlreadyWholeYen() {
		let laptop = StubBudget.goal("laptop", "Laptop", target: 150_000, available: 0, date: "2027-03-01", state: .active, monthly: 7_089, pace: .behind(neededMonthly: 7_089))
		#expect(GoalWords(laptop, format: yen).note == "Behind, needs ¥7,089 a month")
	}
}
