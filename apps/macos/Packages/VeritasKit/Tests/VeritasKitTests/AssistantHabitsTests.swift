import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// docs/finance/assistant.md AI-10.10's worked examples, 1 to 16, through
/// `budgetHabits`. Amounts are cents; activity is the Budget screen's `-spent`.
@MainActor
struct AssistantHabitsTests {
	static let format = MoneyFormat()
	static let august = Month(year: 2026, month: 8)
	static let september = Month(year: 2026, month: 9)
	static let october = Month(year: 2026, month: 10)
	static let basis = "Activity and budgeted are the Budget screen's figures for each month. Carryover is not counted."
	static let disclosure = "Hidden categories, goal categories and months with no budget aren't included."

	/// One envelope across the window: its budgeted and activity per month, oldest first.
	struct Envelope {
		var id: String
		var name: String
		var group = "Everyday"
		var budgeted: [Int64]
		var activity: [Int64]
		var hidden = false
		var kind: CategoryKind = .expense
		var available: Int64 = 0
	}

	static func row(_ envelope: Envelope, _ index: Int) -> EnvelopeRow {
		EnvelopeRow(
			id: envelope.id, name: envelope.name, hidden: envelope.hidden, budgeted: envelope.budgeted[index], spent: -envelope.activity[index],
			available: envelope.available, carryover: false, tone: .neutral, fill: 0, note: nil, kind: envelope.kind
		)
	}

	/// A Budget screen sheet per month, with groups made from the envelopes' group names.
	static func sheets(_ envelopes: [Envelope], months: [Month], firstMonth: Month? = nil, incomeGroups: Set<String> = ["Income"], hiddenGroups: Set<String> = [], note: String? = nil) -> [BudgetMonth] {
		var order: [String] = []
		for envelope in envelopes where !order.contains(envelope.group) { order.append(envelope.group) }
		return months.enumerated().map { index, month in
			BudgetMonth(
				month: month, firstMonth: firstMonth ?? months[0], lastMonth: Month(year: 2026, month: 12), toBudget: 0, toBudgetTone: .neutral,
				income: 0, fromLastMonth: 0, overspentLastMonth: 0, budgeted: 0, forNextMonth: 0, spent: 0, available: 0,
				groups: order.map { group in
					EnvelopeGroup(
						id: group, name: group, isIncome: incomeGroups.contains(group), hidden: hiddenGroups.contains(group),
						budgeted: 0, spent: 0, available: 0,
						envelopes: envelopes.filter { $0.group == group }.map { row($0, index) }
					)
				},
				mixedScaleNote: note
			)
		}
	}

	static let window = [august, september, october]

	static let scenarioH = [
		Envelope(id: "dining", name: "Dining", budgeted: [30_000, 30_000, 30_000], activity: [45_000, 38_250, 40_500]),
		Envelope(id: "shopping", name: "Shopping", budgeted: [15_000, 15_000, 15_000], activity: [26_010, 14_020, 29_000]),
		Envelope(id: "rideshare", name: "Rideshare", budgeted: [6_000, 6_000, 6_000], activity: [12_000, 6_600, 10_200]),
		Envelope(id: "groceries", name: "Groceries", budgeted: [45_000, 45_000, 45_000], activity: [44_000, 47_000, 41_000]),
		Envelope(id: "gas", name: "Gas", budgeted: [10_000, 10_000, 10_000], activity: [12_000, 9_000, 9_000])
	]

	static func text(_ envelopes: [Envelope], months: [Month] = window, goals: Set<String> = [], note: String? = nil, hiddenGroups: Set<String> = [], format: MoneyFormat = format) -> [String] {
		BudgetHabits.text(sheets: sheets(envelopes, months: months, hiddenGroups: hiddenGroups, note: note), goalCategories: goals, format: format)
			.components(separatedBy: "\n")
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
		#expect(Self.text(Self.scenarioH) == Self.example1)
	}

	@Test func ex2RoundingAndTheLimit() {
		let fun = Envelope(id: "fun", name: "Fun", budgeted: [20_000, 20_000, 20_000], activity: [21_000, 20_001, 20_000])
		let lines = Self.text(Self.scenarioH + [fun])
		#expect(lines.first == "Budget habits, August to October 2026: 4 categories ran over their monthly budget in most months.")
		#expect(Array(lines[1...3]) == Array(Self.example1[1...3]))
		#expect(lines[4] == "Showing the 3 largest of 4.")
		#expect(Array(lines[5...]) == Array(Self.example1[4...]))
		#expect(BudgetHabits.average(61_001, over: 3) == 20_334)
	}

	@Test func ex3NothingOver() {
		#expect(Self.text(Array(Self.scenarioH.suffix(2))) == [
			"Budget habits, August to October 2026: no category ran over its monthly budget in most months.",
			"Largest categories by average activity:",
			"Groceries: average activity $440.00 a month against $450.00 budgeted.",
			"Gas: average activity $100.00 a month against $100.00 budgeted.",
			Self.basis,
			Self.disclosure
		])
	}

	@Test func ex4ExcludedRowsChangeNothing() {
		let excluded = [
			Envelope(id: "hobbies", name: "Hobbies", budgeted: [10_000, 10_000, 10_000], activity: [30_000, 30_000, 30_000], hidden: true),
			Envelope(id: "vacation", name: "Vacation", budgeted: [20_000, 20_000, 20_000], activity: [0, 0, 60_000]),
			Envelope(id: "principal", name: "Car Loan Principal", budgeted: [30_000, 30_000, 30_000], activity: [30_000, 30_000, 30_000], kind: .debtPrincipal),
			Envelope(id: "salary", name: "Salary", group: "Income", budgeted: [0, 0, 0], activity: [-500_000, -500_000, -500_000], kind: .income),
			Envelope(id: "coffee", name: "Coffee", budgeted: [0, 0, 0], activity: [5_000, 5_000, 5_000]),
			Envelope(id: "games", name: "Games", group: "Hidden group", budgeted: [1_000, 1_000, 1_000], activity: [9_000, 9_000, 9_000])
		]
		#expect(Self.text(Self.scenarioH + excluded, goals: ["vacation"], hiddenGroups: ["Hidden group"]) == Self.example1)
		#expect(Self.text(Array(Self.scenarioH.suffix(2)) + excluded, goals: ["vacation"], hiddenGroups: ["Hidden group"]) == Self.text(Array(Self.scenarioH.suffix(2))))
	}

	@Test func ex5ACategoryBudgetedPartway() {
		let transit = Envelope(id: "transit", name: "Transit", budgeted: [0, 8_000, 8_000], activity: [3_000, 9_500, 9_000])
		let lines = Self.text([transit])
		#expect(lines[1] == "Transit: over in 2 of 2 months. Average activity $92.50 a month against $80.00 budgeted, $12.50 a month over.")
		#expect(lines[2] == "Bringing Transit back to its monthly budget would free about $12.50 a month (average of 2 budgeted months in August to October 2026).")
	}

	@Test func ex6OverInMostMonthsButUnderOnAverage() {
		let lines = Self.text([Envelope(id: "odd", name: "Odd", budgeted: [10_000, 10_000, 10_000], activity: [10_100, 10_100, 1_000])])
		#expect(lines.first == "Budget habits, August to October 2026: no category ran over its monthly budget in most months.")
	}

	@Test func ex7CarryoverDoesNotHideIt() {
		let carried = Self.scenarioH.map { envelope -> Envelope in
			var envelope = envelope
			if envelope.id == "dining" { envelope.available = 50_000 }
			return envelope
		}
		#expect(Self.text(carried) == Self.example1)
	}

	@Test func ex9NegativeActivityIsNeverOver() {
		let refunds = Envelope(id: "refunds", name: "Returns", budgeted: [10_000, 10_000, 10_000], activity: [12_000, -3_000, 12_000])
		let lines = Self.text([refunds])
		#expect(lines.first == "Budget habits, August to October 2026: no category ran over its monthly budget in most months.", "over in 2 of 3, but the refund month lowers the average below budget")
		#expect(lines[2] == "Returns: average activity $70.00 a month against $100.00 budgeted.")
	}

	@Test func ex10ACurrencyWithout2Places() {
		let note = "Spending is recorded at 2 places and budgets at 0, so these numbers mix both, as in Actual."
		#expect(Self.text(Self.scenarioH, note: note, format: MoneyFormat(currencySymbol: "¥", decimalPlaces: 0)) == [
			"Budget habits can't be compared in this budget, because budgets and activity are recorded at different scales.",
			note
		])
		#expect(Self.text(Self.scenarioH, format: MoneyFormat(currencySymbol: "KD", decimalPlaces: 3)) == [
			"Budget habits can't be compared in this budget, because budgets and activity are recorded at different scales."
		], "a 3-place budget: no note from the core, so the refusal alone")
	}

	@Test func ex11TwoNames() {
		let misc = [
			Envelope(id: "misc-a", name: "Misc", group: "Everyday", budgeted: [1_000, 1_000, 1_000], activity: [3_000, 3_000, 3_000]),
			Envelope(id: "misc-b", name: "Misc", group: "Fun", budgeted: [1_000, 1_000, 1_000], activity: [2_000, 2_000, 2_000])
		]
		let lines = Self.text(misc)
		#expect(lines[1].hasPrefix("Misc in Everyday: over in 3 of 3 months."))
		#expect(lines[2].hasPrefix("Misc in Fun: over in 3 of 3 months."))
		#expect(lines[3].hasPrefix("Bringing Misc in Everyday back"))
	}

	@Test func ex12NoZeroOverage() {
		let lines = Self.text([Envelope(id: "even", name: "Even", budgeted: [10_000, 10_000, 10_000], activity: [10_001, 10_001, 9_999])])
		#expect(lines.first == "Budget habits, August to October 2026: no category ran over its monthly budget in most months.")
		#expect(!lines.joined().contains("$0.00 a month over") && !lines.joined().contains("free about $0.00"))
	}

	@Test func ex16OneCategoryOver() {
		#expect(Self.text([Self.scenarioH[0]]).first == "Budget habits, August to October 2026: 1 category ran over its monthly budget in most months.")
	}

	@Test func noCategoryHasAMonthlyBudget() {
		#expect(Self.text([Envelope(id: "coffee", name: "Coffee", budgeted: [0, 0, 0], activity: [5_000, 5_000, 5_000])]) == [
			"No category has a monthly budget in August to October 2026, so there is nothing to compare.",
			Self.disclosure
		])
	}

	@Test func theAnswerNeverSaysSpentOrSpending() {
		for envelopes in [Self.scenarioH, Array(Self.scenarioH.suffix(2))] {
			let text = Self.text(envelopes).joined(separator: "\n").lowercased()
			#expect(!text.contains("spent") && !text.contains("spending"))
		}
	}

	// MARK: The window, through the tool (examples 8, 13, 14, 15)

	/// Serves one sheet per month, clamped to the months Actual created, as the core does.
	struct SheetQueries: ReadOnlyBudgetQueries {
		struct Unused: Error {}
		let sheets: [BudgetMonth]

		func budgetMonth(_ month: Month, today: String) async throws -> BudgetMonth {
			let first = sheets[0].firstMonth
			let created = sheets.filter { $0.month >= first }
			return created.first { $0.month == month } ?? (month < first ? created[0] : created[created.count - 1])
		}

		func goals(today: String) async throws -> GoalList { GoalList(goals: [], mixedScaleNote: nil, zeroPlaces: false) }
		func netProfitPeriod(first: String, last: String) async throws -> NetProfitPeriod { throw Unused() }
		func balanceSheet(asOf: String) async throws -> BalanceSheet { throw Unused() }
		func cashFlowStatement(first: String, last: String) async throws -> CashFlowColumn { throw Unused() }
		func accounts() async throws -> AccountList { throw Unused() }
		func taxSummary(year: Int32) async throws -> TaxSummary { throw Unused() }
		func customReports() async throws -> [CustomReportItem] { throw Unused() }
		func customReport(_ id: String, today: String) async throws -> CustomReportView { throw Unused() }
	}

	private func ask(_ sheets: [BudgetMonth], today: String) async throws -> [String] {
		let context = AssistantContext(queries: SheetQueries(sheets: sheets), format: Self.format, today: today)
		return try await BudgetHabitsTool(context: context).call(arguments: .init()).components(separatedBy: "\n")
	}

	@Test func ex8FewerThan3Months() async throws {
		let dining = Envelope(id: "dining", name: "Dining", budgeted: [30_000, 30_000, 30_000], activity: [45_000, 45_000, 28_000])
		let fromOctober = Self.sheets([dining], months: Self.window, firstMonth: Self.october)
		#expect(try await ask(fromOctober, today: "2026-11-22") == ["Budget habits: only 1 complete month of history (October 2026), so there is no pattern to report yet."])
		let fromSeptember = Self.sheets([dining], months: Self.window, firstMonth: Self.september)
		let two = try await ask(fromSeptember, today: "2026-11-22")
		#expect(two.first == "Budget habits, September to October 2026: no category ran over its monthly budget in most months.", "over in September only of 2")
		let late = Envelope(id: "dining", name: "Dining", budgeted: [30_000, 30_000, 30_000, 30_000], activity: [45_000, 45_000, 28_000, 0])
		let fromNovember = Self.sheets([late], months: Self.window + [Month(year: 2026, month: 11)], firstMonth: Month(year: 2026, month: 11))
		#expect(try await ask(fromNovember, today: "2026-11-22") == ["Budget habits: no complete month of history yet."])
		let sameWindow = Self.sheets(Self.scenarioH, months: Self.window, firstMonth: Month(year: 2026, month: 1))
		#expect(try await ask(sameWindow, today: "2026-11-01") == Self.example1, "today on the 1st gives the same window")
	}

	@Test func ex13AWindowAcrossAYear() async throws {
		let months = [Month(year: 2025, month: 11), Month(year: 2025, month: 12), Month(year: 2026, month: 1)]
		let dining = Envelope(id: "dining", name: "Dining", budgeted: [30_000, 30_000, 30_000], activity: [31_000, 32_000, 30_000])
		let lines = try await ask(Self.sheets([dining], months: months, firstMonth: Month(year: 2025, month: 1)), today: "2026-02-10")
		#expect(Array(lines.prefix(3)) == [
			"Budget habits, November 2025 to January 2026: 1 category ran over its monthly budget in most months.",
			"Dining: over in 2 of 3 months. Average activity $310.00 a month against $300.00 budgeted, $10.00 a month over.",
			"Bringing Dining back to its monthly budget would free about $10.00 a month (average of 3 budgeted months in November 2025 to January 2026)."
		])
	}

	@Test func ex14TwoMonthsAcrossAYear() async throws {
		let months = [Month(year: 2025, month: 12), Month(year: 2026, month: 1)]
		let dining = Envelope(id: "dining", name: "Dining", budgeted: [30_000, 30_000], activity: [31_000, 32_000])
		let lines = try await ask(Self.sheets([dining], months: months), today: "2026-02-10")
		#expect(lines[1] == "Dining: over in 2 of 2 months. Average activity $315.00 a month against $300.00 budgeted, $15.00 a month over.")
		#expect(lines[2] == "Bringing Dining back to its monthly budget would free about $15.00 a month (average of 2 budgeted months in December 2025 to January 2026).")
	}

	@Test func ex15AClampedReadBeforeTheFirstMonthIsNeverUsed() async throws {
		let dining = Envelope(id: "dining", name: "Dining", budgeted: [30_000, 30_000, 30_000], activity: [99_000, 45_000, 40_500])
		let sheets = Self.sheets([dining], months: Self.window, firstMonth: Self.september)
		let lines = try await ask(sheets, today: "2026-11-22")
		#expect(lines[0] == "Budget habits, September to October 2026: 1 category ran over its monthly budget in most months.")
		#expect(lines[1] == "Dining: over in 2 of 2 months. Average activity $427.50 a month against $300.00 budgeted, $127.50 a month over.")
	}
}

/// The tool on the real core and fixtures/synthetic/all-features: its window
/// and figures come from the Budget screen's own months.
@MainActor
struct AssistantHabitsOnRealBudgetTests {
	@Test func theFixturesHabitsAreTheBudgetScreensFigures() async throws {
		try await RealBudget.with { budget in
			let context = AssistantContext(queries: try #require(budget.workspace.readOnlyQueries), format: budget.workspace.format, today: "2026-11-22")
			let lines = try await BudgetHabitsTool(context: context).call(arguments: .init()).components(separatedBy: "\n")
			#expect(lines.first?.hasPrefix("Budget habits, August to October 2026: ") == true, "\(lines)")
			#expect(lines.suffix(2) == [AssistantHabitsTests.basis, AssistantHabitsTests.disclosure])
			var months: [BudgetMonth] = []
			for month in AssistantHabitsTests.window {
				months.append(try #require(await budget.screen(month).sheet))
			}
			let goals = Set(try await context.queries.goals(today: context.today).goals.map(\.categoryId))
			#expect(lines.joined(separator: "\n") == BudgetHabits.text(sheets: months, goalCategories: goals, format: context.format))
			let text = lines.joined(separator: "\n").lowercased()
			#expect(!text.contains("spent") && !text.contains("spending"))
		}
	}
}
