import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// docs/finance/assistant.md AI-12.8's worked examples through `spendingByCategory`.
struct AssistantByCategoryTests {
	typealias Text = AssistantSpendingTextTests
	static let november = AssistantPeriod(month: Month(year: 2026, month: 11))
	static let format = MoneyFormat()
	static let minus = MoneyFormat.minus
	static let second = "This is the Expenses line of the Net Profit report. Debt paydown and savings are not included."

	static func line(_ name: String, _ total: Int64, id: String? = nil, group: String = "Everyday", labels: ReportLabels = Text.labels()) -> ReportCategory {
		Text.line(id ?? name.lowercased(), name, group: group.lowercased(), group, total: total, count: 1, labels: labels)
	}

	static func uncategorized(_ total: Int64) -> ReportCategory {
		ReportCategory(categoryId: nil, name: "", groupId: nil, groupName: nil, labels: Text.labels(needsReview: true), total: total, count: 1)
	}

	static func text(_ lines: [ReportCategory], period: AssistantPeriod = november, format: MoneyFormat = format) -> [String] {
		let report = NetProfitPeriod(column: Text.column(income: [], expenses: lines, debt: [], savings: []), incomeGroups: [], expenseGroups: [], debtPaydownGroups: [], savingsGroups: [])
		return SpendingByCategoryTool.text(report, period: period, format: format).components(separatedBy: "\n")
	}

	static let example1 = [
		line("Rent", 150_000), line("Groceries", 41_200, labels: Text.labels(hidden: true)), line("Utilities", 12_000), line("Gas", 10_000),
		line("Dining out", 5_000), line("Fun Money", 4_500), line("Gifts", 2_500)
	]

	@Test func ex1MoreThan6Lines() {
		#expect(Self.text(Self.example1) == [
			"Spending, November 2026: $2,252.00",
			Self.second,
			"Spending by category, November 2026, largest first:",
			"Rent: $1,500.00", "Groceries (hidden): $412.00", "Utilities: $120.00", "Gas: $100.00", "Dining out: $50.00",
			"Everything else (2 categories): $70.00"
		])
	}

	@Test func theFirstLineIsSpendingsFirstLine() {
		let report = NetProfitPeriod(column: Text.column(income: [], expenses: Self.example1, debt: [], savings: []), incomeGroups: [], expenseGroups: [], debtPaydownGroups: [], savingsGroups: [])
		let spending = SpendingTool.text(report, period: Self.november, format: Self.format).components(separatedBy: "\n")
		#expect(Array(Self.text(Self.example1).prefix(2)) == Array(spending.prefix(2)))
	}

	@Test func ex2Exactly6Categories() {
		let lines = Self.text(Array(Self.example1.dropLast()))
		#expect(lines.first == "Spending, November 2026: $2,227.00")
		#expect(lines.last == "Fun Money: $45.00")
		#expect(!lines.contains { $0.hasPrefix("Everything else") })
	}

	@Test func ex3ANegativeLineFallsIntoEverythingElse() {
		let lines = Self.text(["A", "B", "C", "D", "E", "F"].enumerated().map { Self.line($1, Int64(100_000 - $0 * 10_000)) } + [Self.line("Dining out", -8_000)])
		#expect(lines.first == "Spending, November 2026: $4,420.00")
		#expect(Array(lines.suffix(6)) == ["A: $1,000.00", "B: $900.00", "C: $800.00", "D: $700.00", "E: $600.00", "Everything else (2 categories): $420.00"])
	}

	@Test func ex4UncategorizedAndDeleted() {
		let lines = Self.text([
			Self.line("Groceries", 41_200), Self.line("Gas", 10_000), Self.line("Dining out", 5_000),
			Self.line("Old Gym", 1_200, group: "", labels: Text.labels(deleted: true)), Self.uncategorized(-4_000)
		])
		#expect(lines.first == "Spending, November 2026: $534.00")
		#expect(Array(lines.dropFirst(3)) == ["Groceries: $412.00", "Gas: $100.00", "Dining out: $50.00", "(deleted) Old Gym: $12.00", "Uncategorized (needs review): \(Self.minus)$40.00"])
	}

	@Test func ex5TiesAndTwoNames() {
		let tied = [Self.line("Rent", 150_000), Self.line("Groceries", 41_200), Self.line("Utilities", 12_000), Self.line("Gas", 10_000), Self.line("Dining out", 5_000), Self.line("Coffee", 5_000), Self.line("Gifts", 2_500)]
		let lines = Self.text(tied)
		#expect(lines.contains("Coffee: $50.00"))
		#expect(!lines.contains { $0.hasPrefix("Dining out") }, "Dining out falls into Everything else")
		let misc = Self.text([Self.line("Misc", 3_000, id: "misc-a", group: "Everyday"), Self.line("Misc", 2_000, id: "misc-b", group: "Fun")])
		#expect(Array(misc.suffix(2)) == ["Misc in Everyday: $30.00", "Misc in Fun: $20.00"])
	}

	@Test func ex6AZeroLineIsLeftOut() {
		let lines = Self.text([Self.line("Groceries", 41_200), Self.line("Dining out", 0)])
		#expect(!lines.contains { $0.hasPrefix("Dining out") })
		#expect(Self.text([Self.line("Dining out", 0)]) == ["Spending, November 2026: $0.00", Self.second, "No spending lines in November 2026."])
	}

	@Test func ex7GoalAndMisfiledLines() {
		let goal = Text.labels()
		var closed = Text.labels(hidden: true)
		closed.closedGoal = true
		var unhidden = Text.labels()
		unhidden.closedGoal = true
		#expect(Self.text([Self.line("Vacation", 20_000, labels: goal)]).last == "Vacation: $200.00")
		#expect(Self.text([Self.line("Vacation", 20_000, labels: closed)]).last == "Vacation (hidden, closed goal): $200.00")
		#expect(Self.text([Self.line("Vacation", 20_000, labels: unhidden)]).last == "Vacation (closed goal): $200.00")
		#expect(Self.text([Self.line("Car Loan Principal", 2_000)]).last == "Car Loan Principal: $20.00")
	}

	@Test func ex8NoRows() {
		#expect(Self.text([], period: AssistantPeriod(month: Month(year: 2026, month: 10))) == ["Spending, October 2026: $0.00", Self.second, "No spending lines in October 2026."])
	}

	@Test func ex9APartialRange() {
		let lines = Self.text([Self.line("Groceries", 41_200)], period: AssistantPeriod(first: "2026-11-01", last: "2026-11-22"))
		#expect(lines[0].hasPrefix("Spending, November 1 to 22, 2026: "))
		#expect(lines[2] == "Spending by category, November 1 to 22, 2026, largest first:")
	}

	@Test func ex10AZeroPlaceCurrency() {
		let yen = MoneyFormat(currencySymbol: "¥", decimalPlaces: 0)
		#expect(Self.text([Self.line("Groceries", 50_000), Self.line("Transit", 20_000)], format: yen) == [
			"Spending, November 2026: ¥700", Self.second, "Spending by category, November 2026, largest first:", "Groceries: ¥500", "Transit: ¥200"
		])
	}

	@Test func ex12UncategorizedSitsOutsideTheLimit() {
		let out = Self.text(Self.example1 + [Self.uncategorized(3_000)])
		#expect(out.first == "Spending, November 2026: $2,282.00")
		#expect(Array(out.suffix(2)) == ["Everything else (2 categories): $70.00", "Uncategorized (needs review): $30.00"])
		let back = Self.text(Self.example1 + [Self.uncategorized(-4_000)])
		#expect(back.first == "Spending, November 2026: $2,212.00")
		#expect(Array(back.suffix(2)) == ["Everything else (2 categories): $70.00", "Uncategorized (needs review): \(Self.minus)$40.00"])
	}

	@Test func theLinesAddUpToTheTotal() {
		for lines in [Self.example1, Self.example1 + [Self.uncategorized(-4_000)], [Self.line("A", 100_000), Self.line("B", -8_000)]] {
			let total = lines.reduce(Int64(0)) { $0 + $1.total }
			#expect(Self.text(lines).first == "Spending, November 2026: \(Self.format.currency(total, on: .register))")
		}
	}
}

/// The tool on the real core: its first line is `spending`'s, on all-features.
@MainActor
struct AssistantByCategoryOnRealBudgetTests {
	@Test func lastMonthByCategoryStartsWithLastMonthsSpending() async throws {
		try await RealBudget.with { budget in
			let context = AssistantContext(queries: try #require(budget.workspace.readOnlyQueries), format: budget.workspace.format, today: "2026-11-22")
			let byCategory = try await SpendingByCategoryTool(context: context).call(arguments: .init(period: "last month")).components(separatedBy: "\n")
			let spending = try await SpendingTool(context: context).call(arguments: .init(period: "last month")).components(separatedBy: "\n")
			#expect(byCategory.first == spending.first)
			#expect(byCategory[2] == "Spending by category, October 2026, largest first:")
			#expect(byCategory.count > 3)
		}
	}
}
