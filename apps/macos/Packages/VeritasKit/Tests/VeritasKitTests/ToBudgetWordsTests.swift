import Testing
import VeritasCore
@testable import VeritasKit

/// The To Budget popover's words from the core's figures
/// (docs/finance/to-budget.md TB-1, TB-2, TB-3, TB-4, TB-8).
struct ToBudgetWordsTests {
	let format = MoneyFormat()

	func plan(_ fills: [ToBudgetFill], envelopes: UInt32, total: Int64, hidden: UInt32 = 0, skipped: [ToBudgetSkip] = [], block: ToBudgetBlock? = nil) -> ToBudgetPlanView {
		ToBudgetPlanView(block: block, fills: fills, envelopes: envelopes, total: total, hiddenLeftOut: hidden, skipped: skipped)
	}

	func fill(_ name: String, need: Int64, amount: Int64, reason: ToBudgetFillReason = .overspent) -> ToBudgetFill {
		ToBudgetFill(categoryId: name.lowercased(), name: name, reason: reason, goalId: nil, need: need, amount: amount)
	}

	@Test func tb_1_1_theLabelIsActualsOwn() {
		#expect(ToBudgetWords.label(.positive) == "To Budget:")
		#expect(ToBudgetWords.label(.zero) == "To Budget:")
		#expect(ToBudgetWords.label(.overbudgeted) == "Overbudgeted:")
	}

	@Test func tb_8_example_1_theSubtitleCountsEnvelopesOnceAndNamesTheHiddenOne() {
		let example1 = plan(
			[fill("Groceries", need: 12_000, amount: 12_000), fill("Gas", need: 3_000, amount: 3_000), fill("Insurance", need: 15_000, amount: 15_000, reason: .goalShortfall)],
			envelopes: 3, total: 30_000, hidden: 1
		)
		#expect(ToBudgetWords.subtitle(example1, counting: .envelopes, format: format) == "3 envelopes, $300.00")
		#expect(ToBudgetWords.notes(example1) == ["1 hidden envelope isn't included"])
		#expect(ToBudgetWords.result(assigned: 30_000, plan: example1, counting: .envelopes, format: format) == "Assigned $300.00 to 3 envelopes")
	}

	@Test func tb_1_3_aRowThatCantActSaysWhy() {
		let words = { (block: ToBudgetBlock) in ToBudgetWords.subtitle(self.plan([], envelopes: 0, total: 0, block: block), counting: .envelopes, format: self.format) }
		#expect(words(.zero) == "To Budget is $0.00")
		#expect(words(.overbudgeted(amount: 5_000)) == "Overbudgeted by $50.00. Cover it from a category first.")
		#expect(words(.nothingUnderfunded) == "Nothing is underfunded")
		#expect(words(.noGoalNeedsFunding) == "No active goal needs funding")
		#expect(words(.notCurrentMonth) == "Goals are funded for the current month only.", "TB-1.4, TB-8 example 8")
		#expect(words(.mixedScale(places: 0)) == "Spending is recorded at 2 places and budgets at 0, so To Budget can't be trusted here.", "TB-2.6, TB-8 example 7")
		#expect(words(.mixedScale(places: 3)) == "Spending is recorded at 2 places and budgets at 3, so To Budget can't be trusted here.")
	}

	@Test func tb_8_example_4_aGoalCutShortByToBudgetSaysOfWhat() {
		let laptop = fill("New laptop", need: 13_750, amount: 10_000, reason: .goalMonthly)
		#expect(ToBudgetWords.fill(laptop, format: format) == "$100.00 of $137.50")
		#expect(ToBudgetWords.fill(fill("Gas", need: 3_000, amount: 3_000), format: format) == "$30.00")
		let goals = plan([laptop], envelopes: 1, total: 10_000, skipped: [ToBudgetSkip(goalId: "someday", name: "Someday", reason: .noTargetDate)])
		#expect(ToBudgetWords.subtitle(goals, counting: .goals, format: format) == "1 goal, $100.00", "TB-4.5")
		#expect(ToBudgetWords.notes(goals) == ["1 goal skipped: no target date"], "TB-4.3")
	}

	@Test func tb_5_holdSaysHowMuchAndResetSaysWhatComesBack() {
		let hold = ToBudgetHold(offered: true, block: nil, defaultAmount: 35_000, held: 20_000, autoHeld: 0, canReset: true)
		#expect(ToBudgetWords.hold(hold, format: format) == "Up to $350.00 into next month's To Budget")
		#expect(ToBudgetWords.reset(hold, format: format) == "$200.00 held goes back to To Budget")
		let yen = ToBudgetHold(offered: true, block: .mixedScale(places: 0), defaultAmount: 0, held: 0, autoHeld: 0, canReset: false)
		#expect(ToBudgetWords.hold(yen, format: format).hasPrefix("Spending is recorded at 2 places"), "TB-5.4")
	}
}
