import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// docs/finance/assistant.md AI-12.6's lines, formatted from the core's
/// `spendingByCategory` breakdown. The ranking and sums are tested in Rust
/// (veritas-ffi assistant.rs, AI-12.8's examples).
struct AssistantByCategoryTests {
	typealias Text = AssistantSpendingTextTests
	static let november = AssistantPeriod.month(2026, 11)
	static let format = MoneyFormat()
	static let minus = MoneyFormat.minus
	static let second = "This is the Expenses line of the Net Profit report. Debt paydown and savings are not included."

	static func line(_ name: String, _ total: Int64, id: String? = nil, group: String = "Everyday", labels: ReportLabels = Text.labels()) -> ReportCategory {
		Text.line(id ?? name.lowercased(), name, group: group.lowercased(), group, total: total, count: 1, labels: labels)
	}

	static func uncategorized(_ total: Int64) -> ReportCategory {
		ReportCategory(categoryId: nil, name: "", groupId: nil, groupName: nil, labels: Text.labels(needsReview: true), total: total, count: 1)
	}

	static func text(_ total: Int64, _ shown: [ReportCategory], rest: EverythingElse? = nil, uncategorized: ReportCategory? = nil, period: AssistantPeriod = november, format: MoneyFormat = format) -> [String] {
		let breakdown = CategoryBreakdown(expenseTotal: total, shown: shown, everythingElse: rest, uncategorized: uncategorized)
		return SpendingByCategoryTool.text(breakdown, period: period, format: format).components(separatedBy: "\n")
	}

	static let example1Shown = [
		line("Rent", 150_000), line("Groceries", 41_200, labels: Text.labels(hidden: true)), line("Utilities", 12_000), line("Gas", 10_000), line("Dining out", 5_000)
	]

	@Test func ex1MoreThan6Lines() {
		#expect(Self.text(225_200, Self.example1Shown, rest: EverythingElse(count: 2, total: 7_000)) == [
			"Spending, November 2026: $2,252.00",
			Self.second,
			"Spending by category, November 2026, largest first:",
			"Rent: $1,500.00", "Groceries (hidden): $412.00", "Utilities: $120.00", "Gas: $100.00", "Dining out: $50.00",
			"Everything else (2 categories): $70.00"
		])
	}

	@Test func ex3ANegativeEverythingElsePrintsAsTheCoreSummedIt() {
		#expect(Self.text(442_000, [Self.line("A", 100_000)], rest: EverythingElse(count: 2, total: 42_000)).last == "Everything else (2 categories): $420.00")
	}

	@Test func ex4UncategorizedAndDeleted() {
		let lines = Self.text(53_400, [Self.line("Groceries", 41_200), Self.line("Gas", 10_000), Self.line("Dining out", 5_000), Self.line("Old Gym", 1_200, group: "", labels: Text.labels(deleted: true))], uncategorized: Self.uncategorized(-4_000))
		#expect(lines.first == "Spending, November 2026: $534.00")
		#expect(Array(lines.dropFirst(3)) == ["Groceries: $412.00", "Gas: $100.00", "Dining out: $50.00", "(deleted) Old Gym: $12.00", "Uncategorized (needs review): \(Self.minus)$40.00"])
	}

	@Test func ex5TwoNames() {
		let lines = Self.text(5_000, [Self.line("Misc", 3_000, id: "misc-a", group: "Everyday"), Self.line("Misc", 2_000, id: "misc-b", group: "Fun")])
		#expect(Array(lines.suffix(2)) == ["Misc in Everyday: $30.00", "Misc in Fun: $20.00"])
	}

	@Test func ex6And8NoSpendingLines() {
		#expect(Self.text(0, []) == ["Spending, November 2026: $0.00", Self.second, "No spending lines in November 2026."])
		#expect(Self.text(0, [], period: AssistantPeriod.month(2026, 10)) == ["Spending, October 2026: $0.00", Self.second, "No spending lines in October 2026."])
	}

	@Test func ex7GoalLabels() {
		var closed = Text.labels(hidden: true)
		closed.closedGoal = true
		var unhidden = Text.labels()
		unhidden.closedGoal = true
		#expect(Self.text(20_000, [Self.line("Vacation", 20_000)]).last == "Vacation: $200.00")
		#expect(Self.text(20_000, [Self.line("Vacation", 20_000, labels: closed)]).last == "Vacation (hidden, closed goal): $200.00")
		#expect(Self.text(20_000, [Self.line("Vacation", 20_000, labels: unhidden)]).last == "Vacation (closed goal): $200.00")
	}

	@Test func ex9APartialRange() {
		let lines = Self.text(41_200, [Self.line("Groceries", 41_200)], period: AssistantPeriod(first: "2026-11-01", last: "2026-11-22", label: "November 1 to 22, 2026"))
		#expect(lines[0] == "Spending, November 1 to 22, 2026: $412.00")
		#expect(lines[2] == "Spending by category, November 1 to 22, 2026, largest first:")
	}

	@Test func ex10AZeroPlaceCurrency() {
		let yen = MoneyFormat(currencySymbol: "¥", decimalPlaces: 0)
		#expect(Self.text(70_000, [Self.line("Groceries", 50_000), Self.line("Transit", 20_000)], format: yen) == [
			"Spending, November 2026: ¥700", Self.second, "Spending by category, November 2026, largest first:", "Groceries: ¥500", "Transit: ¥200"
		])
	}

	@Test func ex12UncategorizedComesLast() {
		let lines = Self.text(228_200, Self.example1Shown, rest: EverythingElse(count: 2, total: 7_000), uncategorized: Self.uncategorized(3_000))
		#expect(Array(lines.suffix(2)) == ["Everything else (2 categories): $70.00", "Uncategorized (needs review): $30.00"])
	}
}

/// The tool on the real core: its first line is `spending`'s, on all-features,
/// and the core's lines add up to the total.
@MainActor
struct AssistantByCategoryOnRealBudgetTests {
	@Test func lastMonthByCategoryStartsWithLastMonthsSpending() async throws {
		try await RealBudget.with { budget in
			let context = AssistantContext(queries: try #require(budget.workspace.readOnlyQueries), format: budget.workspace.format, today: "2026-11-22")
			let byCategory = try await SpendingByCategoryTool(context: context).call(arguments: .init(period: "last month")).components(separatedBy: "\n")
			let spending = try await SpendingTool(context: context).call(arguments: .init(period: "last month")).components(separatedBy: "\n")
			#expect(byCategory.first == spending.first)
			#expect(byCategory[2] == "Spending by category, October 2026, largest first:")
			let breakdown = try await context.queries.spendingByCategory(first: "2026-10-01", last: "2026-10-31")
			let parts = breakdown.shown.reduce(Int64(0)) { $0 + $1.total } + (breakdown.everythingElse?.total ?? 0) + (breakdown.uncategorized?.total ?? 0)
			#expect(parts == breakdown.expenseTotal)
		}
	}
}
