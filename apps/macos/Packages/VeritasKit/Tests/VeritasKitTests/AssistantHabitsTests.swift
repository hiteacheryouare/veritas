import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// docs/finance/assistant.md AI-10.9's lines, formatted from the core's
/// `budgetHabits` results. The figures themselves are tested in Rust
/// (veritas-ffi assistant.rs, AI-10.10's examples 1 to 16).
struct AssistantHabitsTests {
	static let format = MoneyFormat()
	static let august = Month(year: 2026, month: 8)
	static let september = Month(year: 2026, month: 9)
	static let october = Month(year: 2026, month: 10)
	static let window = [august, september, october]
	static let basis = HabitsText.basis
	static let disclosure = HabitsText.disclosure

	static func category(_ name: String, _ over: UInt32, _ budgeted: UInt32, _ activity: Int64, _ budget: Int64, id: String? = nil, group: String = "Everyday") -> HabitsCategory {
		HabitsCategory(categoryId: id ?? name.lowercased(), name: name, groupName: group, budgetedMonths: budgeted, overMonths: over, averageActivity: activity, averageBudgeted: budget, overage: activity - budget)
	}

	static let example1Shown = [category("Dining", 3, 3, 41_250, 30_000), category("Shopping", 2, 3, 23_010, 15_000), category("Rideshare", 3, 3, 9_600, 6_000)]

	static func text(_ outcome: HabitsOutcome, months: [Month] = window, format: MoneyFormat = format) -> [String] {
		HabitsText.text(BudgetHabits(months: months, outcome: outcome), format: format).components(separatedBy: "\n")
	}

	static let example1 = [
		"Budget habits, August to October 2026: 3 categories ran over their monthly budget in most months.",
		"Dining: over in 3 of 3 months. Average activity $412.50 a month against $300.00 budgeted, $112.50 a month over.",
		"Shopping: over in 2 of 3 months. Average activity $230.10 a month against $150.00 budgeted, $80.10 a month over.",
		"Rideshare: over in 3 of 3 months. Average activity $96.00 a month against $60.00 budgeted, $36.00 a month over.",
		"Bringing Dining back to its monthly budget would free about $112.50 a month (average of 3 budgeted months in August to October 2026).",
		basis,
		disclosure
	]

	@Test func ex1RyansPreview() {
		#expect(Self.text(.over(qualifying: 3, shown: Self.example1Shown, suggestion: 11_250, suggestionMonths: 3)) == Self.example1)
	}

	@Test func ex2TheLimitLine() {
		let lines = Self.text(.over(qualifying: 4, shown: Self.example1Shown, suggestion: 11_250, suggestionMonths: 3))
		#expect(lines.first == "Budget habits, August to October 2026: 4 categories ran over their monthly budget in most months.")
		#expect(lines[4] == "Showing the 3 largest of 4.")
		#expect(Array(lines[5...]) == Array(Self.example1[4...]))
	}

	@Test func ex3NothingOver() {
		#expect(Self.text(.nothingOver(largest: [Self.category("Groceries", 1, 3, 44_000, 45_000), Self.category("Gas", 1, 3, 10_000, 10_000)])) == [
			"Budget habits, August to October 2026: no category ran over its monthly budget in most months.",
			"Largest categories by average activity:",
			"Groceries: average activity $440.00 a month against $450.00 budgeted.",
			"Gas: average activity $100.00 a month against $100.00 budgeted.",
			Self.basis,
			Self.disclosure
		])
	}

	@Test func ex5TheSuggestionNamesItsOwnBudgetedMonths() {
		let lines = Self.text(.over(qualifying: 1, shown: [Self.category("Transit", 2, 2, 9_250, 8_000)], suggestion: 1_250, suggestionMonths: 2))
		#expect(lines[1] == "Transit: over in 2 of 2 months. Average activity $92.50 a month against $80.00 budgeted, $12.50 a month over.")
		#expect(lines[2] == "Bringing Transit back to its monthly budget would free about $12.50 a month (average of 2 budgeted months in August to October 2026).")
	}

	@Test func ex8TheShortAnswers() {
		#expect(Self.text(.noHistory, months: []) == ["Budget habits: no complete month of history yet."])
		#expect(Self.text(.oneMonth, months: [Self.october]) == ["Budget habits: only 1 complete month of history (October 2026), so there is no pattern to report yet."])
		#expect(Self.text(.nothingOver(largest: [Self.category("Dining", 1, 2, 36_500, 30_000)]), months: [Self.september, Self.october]).first
			== "Budget habits, September to October 2026: no category ran over its monthly budget in most months.")
	}

	@Test func ex10ACurrencyWithout2Places() {
		let note = "Spending is recorded at 2 places and budgets at 0, so these numbers mix both, as in Actual."
		#expect(Self.text(.differentScales(note: note)) == ["Budget habits can't be compared in this budget, because budgets and activity are recorded at different scales.", note])
		#expect(Self.text(.differentScales(note: nil)) == ["Budget habits can't be compared in this budget, because budgets and activity are recorded at different scales."])
	}

	@Test func ex11TwoNames() {
		let shown = [Self.category("Misc", 3, 3, 3_000, 1_000, id: "misc-a", group: "Everyday"), Self.category("Misc", 3, 3, 2_000, 1_000, id: "misc-b", group: "Fun")]
		let lines = Self.text(.over(qualifying: 2, shown: shown, suggestion: 2_000, suggestionMonths: 3))
		#expect(lines[1].hasPrefix("Misc in Everyday: over in 3 of 3 months."))
		#expect(lines[2].hasPrefix("Misc in Fun: over in 3 of 3 months."))
		#expect(lines[3].hasPrefix("Bringing Misc in Everyday back"))
	}

	@Test func ex13AWindowAcrossAYear() {
		let months = [Month(year: 2025, month: 11), Month(year: 2025, month: 12), Month(year: 2026, month: 1)]
		let lines = Self.text(.over(qualifying: 1, shown: [Self.category("Dining", 2, 3, 31_000, 30_000)], suggestion: 1_000, suggestionMonths: 3), months: months)
		#expect(Array(lines.prefix(3)) == [
			"Budget habits, November 2025 to January 2026: 1 category ran over its monthly budget in most months.",
			"Dining: over in 2 of 3 months. Average activity $310.00 a month against $300.00 budgeted, $10.00 a month over.",
			"Bringing Dining back to its monthly budget would free about $10.00 a month (average of 3 budgeted months in November 2025 to January 2026)."
		])
	}

	@Test func noCategoryHasAMonthlyBudget() {
		#expect(Self.text(.noBudgets) == ["No category has a monthly budget in August to October 2026, so there is nothing to compare.", Self.disclosure])
	}

	@Test func theAnswerNeverSaysSpentOrSpending() {
		for outcome in [HabitsOutcome.over(qualifying: 3, shown: Self.example1Shown, suggestion: 11_250, suggestionMonths: 3), .nothingOver(largest: Self.example1Shown), .noBudgets] {
			let text = Self.text(outcome).joined(separator: "\n").lowercased()
			#expect(!text.contains("spent") && !text.contains("spending"))
		}
	}

	/// Hands the tool a fixed core result, as the evaluation's nothing-over case does.
	struct HabitsQueries: ReadOnlyBudgetQueries {
		struct Unused: Error {}
		let habits: BudgetHabits

		func budgetHabits(today: String) async throws -> BudgetHabits { habits }
		func spendingByCategory(first: String, last: String) async throws -> CategoryBreakdown { throw Unused() }
		func netProfitPeriod(first: String, last: String) async throws -> NetProfitPeriod { throw Unused() }
		func balanceSheet(asOf: String) async throws -> BalanceSheet { throw Unused() }
		func cashFlowStatement(first: String, last: String) async throws -> CashFlowColumn { throw Unused() }
		func budgetMonth(_ month: Month, today: String) async throws -> BudgetMonth { throw Unused() }
		func accounts() async throws -> AccountList { throw Unused() }
		func goals(today: String) async throws -> GoalList { throw Unused() }
		func taxSummary(year: Int32) async throws -> TaxSummary { throw Unused() }
		func customReports() async throws -> [CustomReportItem] { throw Unused() }
		func customReport(_ id: String, today: String) async throws -> CustomReportView { throw Unused() }
	}

	@Test func theToolFormatsTheCoresResult() async throws {
		let habits = BudgetHabits(months: Self.window, outcome: .over(qualifying: 3, shown: Self.example1Shown, suggestion: 11_250, suggestionMonths: 3))
		let context = AssistantContext(queries: HabitsQueries(habits: habits), format: Self.format, today: "2026-11-22")
		#expect(try await BudgetHabitsTool(context: context).call(arguments: .init()).components(separatedBy: "\n") == Self.example1)
	}
}

/// The tool on the real core and fixtures/synthetic/all-features.
@MainActor
struct AssistantHabitsOnRealBudgetTests {
	@Test func theFixturesHabitsComeFromTheCoreRead() async throws {
		try await RealBudget.with { budget in
			let context = AssistantContext(queries: try #require(budget.workspace.readOnlyQueries), format: budget.workspace.format, today: "2026-11-22")
			let habits = try await context.queries.budgetHabits(today: context.today)
			#expect(habits.months == AssistantHabitsTests.window)
			let lines = try await BudgetHabitsTool(context: context).call(arguments: .init()).components(separatedBy: "\n")
			#expect(lines.joined(separator: "\n") == HabitsText.text(habits, format: context.format))
			#expect(lines.first?.hasPrefix("Budget habits, August to October 2026: ") == true, "\(lines)")
			#expect(lines.suffix(2) == [AssistantHabitsTests.basis, AssistantHabitsTests.disclosure])
		}
	}
}
