import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// docs/finance/assistant.md: the app works out the dates from the user's
/// words, finds the category by name, and the first line is the answer.
struct AssistantPeriodTests {
	static let today = "2026-11-22"

	@Test(arguments: [
		("", "2026-11-01", "2026-11-30", "November 2026"),
		("this month", "2026-11-01", "2026-11-30", "November 2026"),
		("so far this month", "2026-11-01", "2026-11-22", "November 1 to 22, 2026"),
		("this month so far", "2026-11-01", "2026-11-22", "November 1 to 22, 2026"),
		("Month to date", "2026-11-01", "2026-11-22", "November 1 to 22, 2026"),
		("so far", "2026-11-01", "2026-11-22", "November 1 to 22, 2026"),
		("last month", "2026-10-01", "2026-10-31", "October 2026"),
		("this year", "2026-01-01", "2026-12-31", "2026"),
		("so far this year", "2026-01-01", "2026-11-22", "January 1 to November 22, 2026"),
		("year to date", "2026-01-01", "2026-11-22", "January 1 to November 22, 2026"),
		("last year", "2025-01-01", "2025-12-31", "2025"),
		("October", "2026-10-01", "2026-10-31", "October 2026"),
		("december", "2025-12-01", "2025-12-31", "December 2025"),
		("Oct 2026", "2026-10-01", "2026-10-31", "October 2026"),
		("in October of last year", "2025-10-01", "2025-10-31", "October 2025"),
		("2026-10", "2026-10-01", "2026-10-31", "October 2026"),
		("2025", "2025-01-01", "2025-12-31", "2025"),
		("2026-10-01 to 2026-10-15", "2026-10-01", "2026-10-15", "October 1 to 15, 2026"),
		("2025-12-15 to 2026-01-03", "2025-12-15", "2026-01-03", "December 15, 2025 to January 3, 2026"),
		("May to October 2026", "2026-05-01", "2026-10-31", "May to October 2026"),
		("November to February", "2025-11-01", "2026-02-28", "November 2025 to February 2026"),
		("today", "2026-11-22", "2026-11-22", "November 22, 2026")
	])
	func wordsResolveAgainstToday(_ words: String, first: String, last: String, title: String) throws {
		let period = try #require(AssistantPeriod.resolve(words, today: Self.today), "\(words)")
		#expect(period.first == first, "\(words)")
		#expect(period.last == last, "\(words)")
		#expect(period.title == title, "\(words)")
	}

	@Test func aMonthNamedAloneIsTheLatestNotAfterTodaysMonth() throws {
		#expect(AssistantPeriod.resolve("October", today: "2026-09-05")?.first == "2025-10-01")
		#expect(AssistantPeriod.resolve("October", today: "2026-10-05")?.last == "2026-10-31", "the whole month, days after today included")
	}

	@Test(arguments: ["2026-13", "2026-02-30", "1899", "next fortnight", "last 3 months", "x-y"])
	func wordsItCantReadGetTheHint(_ words: String) {
		#expect(AssistantPeriod.resolve(words, today: Self.today) == nil, "\(words)")
	}
}

struct AssistantNamesTests {
	static let candidates: [AssistantNames.Candidate] = [
		.init(id: "everyday", name: "Everyday", isGroup: true),
		.init(id: "groceries", name: "Groceries", isGroup: false, group: "Everyday"),
		.init(id: "gifts", name: "Gifts", isGroup: false, group: "Fun"),
		.init(id: "cafe", name: "Café", isGroup: false, group: "Fun"),
		.init(id: "misc-a", name: "Misc", isGroup: false, group: "Everyday"),
		.init(id: "misc-b", name: "Misc", isGroup: false, group: "Fun")
	]

	@Test(arguments: [
		("groceries", "groceries"), ("GROCERIES", "groceries"), ("grocery", "groceries"), ("the Groceries category", "groceries"),
		("  groceries ", "groceries"), ("cafe", "cafe"), ("CAFÉ", "cafe"), ("everyday", "everyday"), ("Everyday group", "everyday")
	])
	func aNameMatchesIgnoringCaseAccentsAndPlurals(_ wanted: String, id: String) {
		guard case .found(let candidate) = AssistantNames.match(wanted, in: Self.candidates) else {
			Issue.record("\(wanted) matched nothing")
			return
		}
		#expect(candidate.id == id)
	}

	@Test func aLooserNameOffersTheClosestAndNeverGuesses() {
		let match = AssistantNames.match("groc", in: Self.candidates)
		guard case .notFound(let closest) = match else {
			Issue.record("\(match)")
			return
		}
		#expect(closest.first == "Groceries")
		#expect(closest.count == 3)
		#expect(AssistantNames.text(match, wanted: "groc")?.hasPrefix("No category or group is called \"groc\". The closest names are Groceries, ") == true)
	}

	@Test func twoCategoriesOfOneNameAreAskedAbout() {
		let match = AssistantNames.match("misc", in: Self.candidates)
		#expect(match == .several(["Misc in Everyday", "Misc in Fun"]))
		#expect(AssistantNames.text(match, wanted: "misc") == "More than one category is called \"misc\": Misc in Everyday, Misc in Fun. Ask which one.")
	}
}

/// AI-8's worked examples, in cents, through each tool's text.
struct AssistantSpendingTextTests {
	static let november = AssistantPeriod(month: Month(year: 2026, month: 11))
	static let format = MoneyFormat()

	static func labels(hidden: Bool = false) -> ReportLabels {
		ReportLabels(hidden: hidden, archived: false, deleted: false, closedGoal: false, needsReview: false)
	}

	static func line(_ id: String, _ name: String, group: String, _ groupName: String, total: Int64, count: UInt64, hidden: Bool = false) -> ReportCategory {
		ReportCategory(categoryId: id, name: name, groupId: group, groupName: groupName, labels: labels(hidden: hidden), total: total, count: count)
	}

	static func report(groceriesHidden: Bool = false) -> NetProfitPeriod {
		let expenses = [
			line("groceries", "Groceries", group: "everyday", "Everyday", total: 41_200, count: 2, hidden: groceriesHidden),
			line("gas", "Gas", group: "everyday", "Everyday", total: 10_000, count: 1),
			line("dining", "Dining out", group: "fun", "Fun", total: 5_000, count: 2)
		]
		let income = [line("salary", "Salary", group: "income", "Income", total: 500_000, count: 1)]
		let debt = [line("principal", "Car Loan Principal", group: "debt", "Debt", total: 30_000, count: 1)]
		let savings = [line("emergency", "Emergency fund", group: "debt", "Debt", total: 20_000, count: 1)]
		let column = NetProfitColumn(
			first: "2026-11-01", last: "2026-11-30", month: Month(year: 2026, month: 11),
			income: ReportSection(total: 500_000, count: 1, categories: income),
			expenses: ReportSection(total: 56_200, count: 5, categories: expenses),
			netProfit: ReportLine(total: 443_800, count: 6),
			debtPaydown: ReportSection(total: 30_000, count: 1, categories: debt),
			savings: ReportSection(total: 20_000, count: 1, categories: savings),
			netCashFlow: ReportLine(total: 393_800, count: 8),
			unreviewed: ReportLine(total: 0, count: 0)
		)
		return NetProfitPeriod(
			column: column,
			incomeGroups: [ReportGroup(groupId: "income", name: "Income", total: 500_000, count: 1)],
			expenseGroups: [ReportGroup(groupId: "everyday", name: "Everyday", total: 51_200, count: 3), ReportGroup(groupId: "fun", name: "Fun", total: 5_000, count: 2)]
		)
	}

	static func envelope(_ id: String, _ name: String, budgeted: Int64 = 0, spent: Int64 = 0, available: Int64 = 0) -> EnvelopeRow {
		EnvelopeRow(id: id, name: name, hidden: false, budgeted: budgeted, spent: spent, available: available, carryover: false, tone: .neutral, fill: 0, note: nil)
	}

	static let sheet = BudgetMonth(
		month: Month(year: 2026, month: 11), firstMonth: Month(year: 2025, month: 8), lastMonth: Month(year: 2026, month: 12),
		toBudget: 12_000, toBudgetTone: .income, income: 500_000, fromLastMonth: 0, overspentLastMonth: 0, budgeted: 120_000,
		forNextMonth: 0, spent: -56_200, available: 63_800,
		groups: [
			EnvelopeGroup(id: "everyday", name: "Everyday", isIncome: false, hidden: false, budgeted: 60_000, spent: -51_200, available: 8_800, envelopes: [
				envelope("groceries", "Groceries", budgeted: 45_000, spent: -41_200, available: 3_800),
				envelope("gas", "Gas", budgeted: 15_000, spent: -10_000, available: 5_000)
			]),
			EnvelopeGroup(id: "fun", name: "Fun", isIncome: false, hidden: false, budgeted: 10_000, spent: -5_000, available: 5_000, envelopes: [
				envelope("dining", "Dining out", budgeted: 10_000, spent: -5_000, available: 5_000),
				envelope("gifts", "Gifts")
			]),
			EnvelopeGroup(id: "income", name: "Income", isIncome: true, hidden: false, budgeted: 0, spent: 500_000, available: 0, envelopes: [
				envelope("salary", "Salary", spent: 500_000)
			])
		],
		mixedScaleNote: nil
	)

	static func lines(_ text: String) -> [String] { text.components(separatedBy: "\n") }

	@Test func spendingIsTheExpensesLineAndNeverQuotesDebtPaydownOrSavings() {
		let text = SpendingTool.text(Self.report(), period: Self.november, format: Self.format)
		#expect(Array(Self.lines(text).prefix(2)) == [
			"Spending, November 2026: 562.00",
			"This is the Expenses line of the Net Profit report. Debt paydown and savings are not included."
		])
		#expect(text.contains("\nEveryday: 512.00") && text.contains("\nFun: 50.00"))
		#expect(!text.contains("300.00") && !text.contains("200.00") && !text.contains("Debt paydown:"), "\(text)")
	}

	@Test func incomeIsTheIncomeLine() {
		let text = IncomeTool.text(Self.report(), period: Self.november, format: Self.format)
		#expect(Array(Self.lines(text).prefix(2)) == ["Income, November 2026: 5,000.00", "This is the Income line of the Net Profit report."])
	}

	@Test(arguments: [
		("GROCERIES", "Spending on Groceries, November 2026: 412.00", "From 2 transactions, on the Expenses line of the Net Profit report."),
		("gifts", "Spending on Gifts, November 2026: 0.00", "From 0 transactions, on the Expenses line of the Net Profit report."),
		("salary", "Income from Salary, November 2026: 5,000.00", "From 1 transaction, on the Income line of the Net Profit report."),
		("car loan principal", "Debt paydown for Car Loan Principal, November 2026: 300.00", "From 1 transaction, on the Debt paydown line of the Net Profit report. It is not spending."),
		("Everyday", "Spending on Everyday, November 2026: 512.00", "Everyday is a category group. From 3 transactions, on the Expenses line of the Net Profit report.")
	])
	func aCategoryOrGroupAnswersWithItsOwnLine(_ name: String, first: String, second: String) {
		let text = CategorySpendingTool.text(name, report: Self.report(), sheet: Self.sheet, period: Self.november, format: Self.format)
		#expect(Array(Self.lines(text).prefix(2)) == [first, second], "\(text)")
	}

	@Test func aGroupListsItsCategoriesAndAHiddenOneIsLabelled() {
		let text = CategorySpendingTool.text("everyday", report: Self.report(groceriesHidden: true), sheet: Self.sheet, period: Self.november, format: Self.format)
		#expect(Array(Self.lines(text).dropFirst(2)) == ["Its categories, November 2026:", "Groceries (hidden): 412.00", "Gas: 100.00"])
		let groceries = CategorySpendingTool.text("groceries", report: Self.report(groceriesHidden: true), sheet: Self.sheet, period: Self.november, format: Self.format)
		#expect(groceries.hasPrefix("Spending on Groceries (hidden), November 2026: 412.00\n"))
	}

	@Test func anUnknownNameGetsNoFigure() {
		let text = CategorySpendingTool.text("groc", report: Self.report(), sheet: Self.sheet, period: Self.november, format: Self.format)
		#expect(text.hasPrefix("No category or group is called \"groc\". The closest names are Groceries"), "\(text)")
		#expect(!text.contains(/\d/), "\(text)")
	}

	@Test func budgetFiguresSayActivityNeverSpent() {
		let text = BudgetTool.text("groceries", sheet: Self.sheet, format: Self.format)
		#expect(text == "Budgeted for Groceries, November 2026: 450.00\nBudget screen figures for November 2026: activity 412.00, available 38.00.")
		#expect(BudgetTool.text("", sheet: Self.sheet, format: Self.format).hasPrefix("Budgeted, November 2026: 1,200.00\n"))
		#expect(BudgetTool.text("salary", sheet: Self.sheet, format: Self.format).hasPrefix("Salary is income, so nothing is budgeted for it."))
		for name in ["groceries", "everyday", "", "salary"] {
			let text = BudgetTool.text(name, sheet: Self.sheet, format: Self.format).lowercased()
			#expect(!text.contains("spent") && !text.contains("spending"), "\(text)")
		}
	}
}

/// The focused tools against the real core and fixtures/synthetic/all-features:
/// each figure is the one the Net Profit or Budget screen shows.
@MainActor
struct AssistantSpendingToolsOnRealBudgetTests {
	static let today = "2026-11-22"
	static let october = Month(year: 2026, month: 10)

	private func context(_ budget: RealBudget, today: String = Self.today) throws -> AssistantContext {
		AssistantContext(queries: try #require(budget.workspace.readOnlyQueries), format: budget.workspace.format, today: today)
	}

	private func netProfit(_ budget: RealBudget, _ first: Month, _ last: Month) async -> NetProfitModel {
		let model = NetProfitModel(workspace: budget.workspace, thisMonth: Month(year: 2026, month: 11))
		await model.setCustom(first: first, last: last)
		return model
	}

	@Test func lastMonthsSpendingIsTheNetProfitScreensExpenses() async throws {
		try await RealBudget.with { budget in
			let model = await netProfit(budget, Self.october, Self.october)
			let format = model.format
			let expenses = try #require(model.rows.first { $0.line == .expenses }?.cells.last?.amount)
			#expect(expenses != 0)
			let text = try await SpendingTool(context: context(budget)).call(arguments: .init(period: "last month"))
			#expect(text.hasPrefix("Spending, October 2026: \(format.amount(expenses, on: .register))\nThis is the Expenses line"), "\(text)")
			for line in [NetProfitLine.debtPaydown, .savings] {
				#expect(!text.contains(ReportWords.line(line) + ":"), "\(line) is never quoted")
			}
		}
	}

	@Test func aCategoryInAnyCaseHasItsNetProfitLine() async throws {
		try await RealBudget.with { budget in
			let model = await netProfit(budget, Self.october, Self.october)
			let format = model.format
			let category = try #require(model.report?.total.expenses.categories.filter { $0.categoryId != nil && !$0.labels.hidden }.max { $0.count < $1.count })
			let text = try await CategorySpendingTool(context: context(budget)).call(arguments: .init(name: category.name.uppercased(), period: "last month"))
			let title = ReportWords.category(category.name, labels: category.labels)
			#expect(text.hasPrefix("Spending on \(title), October 2026: \(format.amount(category.total, on: .register))\n"), "\(text)")
		}
	}

	@Test func aGroupIsTheSumOfItsCategoriesOnTheNetProfitScreen() async throws {
		try await RealBudget.with { budget in
			let model = await netProfit(budget, Self.october, Self.october)
			let format = model.format
			let categories = try #require(model.report?.total.expenses.categories)
			let everyday = categories.filter { $0.groupId == AllFeatures.everydayGroup }
			let name = try #require(everyday.first?.groupName)
			let total = everyday.reduce(Int64(0)) { $0 + $1.total }
			let text = try await CategorySpendingTool(context: context(budget)).call(arguments: .init(name: name.lowercased(), period: "October 2026"))
			#expect(text.hasPrefix("Spending on \(name), October 2026: \(format.amount(total, on: .register))\n\(name) is a category group."), "\(text)")
		}
	}

	@Test func thisYearsIncomeIsTheNetProfitScreensIncome() async throws {
		try await RealBudget.with { budget in
			let model = await netProfit(budget, Month(year: 2026, month: 1), Month(year: 2026, month: 12))
			let income = try #require(model.rows.first { $0.line == .income }?.cells.last?.amount)
			let text = try await IncomeTool(context: context(budget)).call(arguments: .init(period: "this year"))
			#expect(text.hasPrefix("Income, 2026: \(model.format.amount(income, on: .register))\n"), "\(text)")
		}
	}

	@Test func soFarThisMonthStopsAtToday() async throws {
		try await RealBudget.with { budget in
			let context = try context(budget, today: "2026-11-15")
			let partial = try await context.queries.netProfitPeriod(first: "2026-11-01", last: "2026-11-15")
			let text = try await SpendingTool(context: context).call(arguments: .init(period: "so far this month"))
			#expect(text.hasPrefix("Spending, November 1 to 15, 2026: \(context.format.amount(partial.column.expenses.total, on: .register))\n"), "\(text)")
		}
	}

	@Test func aBudgetFigureIsTheBudgetScreens() async throws {
		try await RealBudget.with { budget in
			let screen = await budget.screen(Self.october)
			let groceries = try #require(screen.sheet).row(AllFeatures.groceries)
			let format = screen.format
			let text = try await BudgetTool(context: context(budget)).call(arguments: .init(name: groceries.name.lowercased(), period: "last month"))
			#expect(text == """
				Budgeted for \(groceries.name), October 2026: \(format.amount(groceries.budgeted, on: .budget))
				Budget screen figures for October 2026: activity \(format.amount(-groceries.spent, on: .budget)), available \(format.amount(groceries.available, on: .budget)).
				""")
		}
	}
}
