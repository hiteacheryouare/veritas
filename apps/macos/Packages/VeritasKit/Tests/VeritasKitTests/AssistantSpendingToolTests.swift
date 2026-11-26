import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// docs/finance/assistant.md AI-6 through the core's `resolvePeriod`. Every
/// row of AI-6.2 and the cross-year ranges are in veritas-ffi's periods module.
struct AssistantPeriodTests {
	@Test func theToolsPassTheWordsAndTodayToTheCore() async throws {
		let context = AssistantContext(queries: RecordingQueries(), format: MoneyFormat(), today: "2026-11-22")
		let text = try await SpendingTool(context: context).call(arguments: .init(period: "November 2025 to January 2026"))
		#expect(text.hasPrefix("Spending, November 2025 to January 2026:"), "\(text)")
		let period = try #require(try resolvePeriod(text: "November 2025 to January", today: "2026-11-22").period)
		#expect((period.first, period.last) == ("2025-11-01", "2026-01-31"))
		#expect((period.firstMonth, period.lastMonth) == (Month(year: 2025, month: 11), Month(year: 2026, month: 1)))
	}

	@Test func wordsItCantReadGetTheCoresHint() throws {
		let refused = try resolvePeriod(text: "next fortnight", today: "2026-11-22")
		#expect(refused.period == nil)
		#expect(refused.hint == AssistantPeriod.hint)
		#expect(AssistantPeriod.hint.hasPrefix("Ask again with a period"))
	}
}

extension AssistantPeriod {
	static func month(_ year: Int32, _ month: UInt32) -> AssistantPeriod {
		let month = Month(year: year, month: month)
		return AssistantPeriod(first: month.firstDay, last: month.lastDay, label: ReportPeriod.monthYear(month))
	}

	static var hint: String {
		(try? resolvePeriod(text: "next fortnight", today: "2026-11-22"))?.hint ?? "no hint"
	}
}

/// AI-3.6 and AI-3.7 through the core's matcher. Its own tests, with every
/// tier and the accent folding, are in veritas-ffi's names module.
struct AssistantNamesTests {
	static let candidates: [AssistantNames.Candidate] = [
		.init(id: "everyday", name: "Everyday", isGroup: true),
		.init(id: "groceries", name: "Groceries", isGroup: false, group: "Everyday"),
		.init(id: "gas", name: "Gas", isGroup: false, group: "Everyday"),
		.init(id: "gifts", name: "Gifts", isGroup: false, group: "Fun"),
		.init(id: "cafe", name: "Café", isGroup: false, group: "Fun"),
		.init(id: "kids", name: "The Kids", isGroup: false, group: "Fun"),
		.init(id: "misc-a", name: "Misc", isGroup: false, group: "Everyday"),
		.init(id: "misc-b", name: "Misc", isGroup: false, group: "Fun"),
		.init(id: "food-group", name: "Food", isGroup: true),
		.init(id: "food", name: "Food", isGroup: false, group: "Everyday"),
		.init(id: "gone", name: "", isGroup: true)
	]

	@Test(arguments: [
		("groceries", "groceries"), ("GROCERIES", "groceries"), ("grocery", "groceries"), ("the Groceries category", "groceries"),
		("  groceries ", "groceries"), ("cafe", "cafe"), ("CAFÉ", "cafe"), ("everyday", "everyday"), ("Everyday group", "everyday"),
		("the kids", "kids"), ("kids", "kids"), ("food group", "food-group"), ("food category", "food"), ("misc in fun", "misc-b")
	])
	func aNameMatchesByItsKey(_ wanted: String, id: String) {
		guard case .found(let candidate) = AssistantNames.match(wanted, in: Self.candidates) else {
			Issue.record("\(wanted) matched nothing")
			return
		}
		#expect(candidate.id == id)
	}

	@Test func aCategoryAndAGroupOfOneNameAreBothNamedWithNoFigure() {
		let match = AssistantNames.match("food", in: Self.candidates)
		#expect(AssistantNames.text(match, wanted: "food") == "More than one category or group is called \"food\": the category Food in Everyday, the category group Food. Ask which one.")
		#expect(AssistantNames.text(AssistantNames.match("misc", in: Self.candidates), wanted: "misc") == "More than one category or group is called \"misc\": the category Misc in Everyday, the category Misc in Fun. Ask which one.")
	}

	@Test func closestNamesFollowTheTiersAndTheEditLimit() {
		#expect(Self.closest("groc") == ["Groceries"], "Gas and Gifts are 3 edits away")
		#expect(Self.closest("gift card") == ["Gifts"], "tier 3: the asked key holds the name")
		#expect(Self.closest("ga") == ["Gas"])
		#expect(Self.closest("gs") == ["Gas"], "tier 4, one edit")
		#expect(Self.closest("zzzzzz").isEmpty)
		#expect(AssistantNames.text(.notFound(closest: ["Groceries"]), wanted: "groc") == "No category or group is called \"groc\". The closest name is Groceries.")
		#expect(AssistantNames.text(.notFound(closest: ["Groceries", "Gifts"]), wanted: "g") == "No category or group is called \"g\". The closest names are Groceries, Gifts.")
		#expect(AssistantNames.text(.notFound(closest: []), wanted: "zzz") == "No category or group is called \"zzz\".")
	}

	static func closest(_ wanted: String) -> [String] {
		guard case .notFound(let closest) = AssistantNames.match(wanted, in: candidates) else { return ["(matched)"] }
		return closest
	}

	@Test func aNamelessCandidateIsNeverMatched() {
		guard case .notFound = AssistantNames.match("", in: Self.candidates) else {
			Issue.record("an empty name matched")
			return
		}
	}
}

/// AI-8's worked examples, in cents, through each tool's text. Scenario A:
/// Groceries and Gas in Everyday, Dining out in Fun, a principal transfer,
/// a savings transfer and Salary.
struct AssistantSpendingTextTests {
	static let november = AssistantPeriod.month(2026, 11)
	static let soFar = AssistantPeriod(first: "2026-11-01", last: "2026-11-22", label: "November 1 to 22, 2026")
	static let format = MoneyFormat()
	static let minus = MoneyFormat.minus

	static func labels(hidden: Bool = false, archived: Bool = false, deleted: Bool = false, needsReview: Bool = false) -> ReportLabels {
		ReportLabels(hidden: hidden, archived: archived, deleted: deleted, closedGoal: false, needsReview: needsReview)
	}

	static func line(_ id: String?, _ name: String, group: String?, _ groupName: String?, total: Int64, count: UInt64, labels: ReportLabels = labels()) -> ReportCategory {
		ReportCategory(categoryId: id, name: name, groupId: group, groupName: groupName, labels: labels, total: total, count: count)
	}

	static func group(_ id: String, _ name: String, total: Int64, count: UInt64, hidden: Bool = false) -> ReportGroup {
		ReportGroup(groupId: id, name: name, labels: labels(hidden: hidden), total: total, count: count)
	}

	static func column(
		income: [ReportCategory] = [line("salary", "Salary", group: "income", "Income", total: 500_000, count: 1)],
		expenses: [ReportCategory],
		debt: [ReportCategory] = [line("principal", "Car Loan Principal", group: "debt", "Savings and Debt", total: 30_000, count: 1)],
		savings: [ReportCategory] = [line("emergency", "Emergency fund", group: "debt", "Savings and Debt", total: 20_000, count: 1)]
	) -> NetProfitColumn {
		func section(_ lines: [ReportCategory]) -> ReportSection {
			ReportSection(total: lines.reduce(0) { $0 + $1.total }, count: lines.reduce(0) { $0 + $1.count }, categories: lines)
		}
		let (income, expenses, debt, savings) = (section(income), section(expenses), section(debt), section(savings))
		let netProfit = income.total - expenses.total
		return NetProfitColumn(
			first: "2026-11-01", last: "2026-11-30", month: Month(year: 2026, month: 11),
			income: income, expenses: expenses, netProfit: ReportLine(total: netProfit, count: 0),
			debtPaydown: debt, savings: savings,
			netCashFlow: ReportLine(total: netProfit - debt.total - savings.total, count: 0),
			unreviewed: ReportLine(total: 0, count: 0)
		)
	}

	static func scenarioA(groceries: ReportLabels = labels(), extra: [ReportCategory] = []) -> NetProfitPeriod {
		let expenses = [
			line("groceries", "Groceries", group: "everyday", "Everyday", total: 41_200, count: 2, labels: groceries),
			line("gas", "Gas", group: "everyday", "Everyday", total: 10_000, count: 1),
			line("dining", "Dining out", group: "fun", "Fun", total: 5_000, count: 2)
		] + extra
		return NetProfitPeriod(
			column: column(expenses: expenses),
			incomeGroups: [group("income", "Income", total: 500_000, count: 1)],
			expenseGroups: [group("everyday", "Everyday", total: 51_200, count: 3), group("fun", "Fun", total: 5_000, count: 2)],
			debtPaydownGroups: [group("debt", "Savings and Debt", total: 30_000, count: 1)],
			savingsGroups: [group("debt", "Savings and Debt", total: 20_000, count: 1)]
		)
	}

	static func envelope(_ id: String, _ name: String, budgeted: Int64 = 0, spent: Int64 = 0, available: Int64 = 0, hidden: Bool = false) -> EnvelopeRow {
		EnvelopeRow(id: id, name: name, hidden: hidden, budgeted: budgeted, spent: spent, available: available, carryover: false, tone: .neutral, fill: 0, note: nil)
	}

	static func sheet(note: String? = nil) -> BudgetMonth {
		BudgetMonth(
			month: Month(year: 2026, month: 11), firstMonth: Month(year: 2025, month: 8), lastMonth: Month(year: 2026, month: 12),
			toBudget: 12_000, toBudgetTone: .income, income: 500_000, fromLastMonth: 0, overspentLastMonth: 0, budgeted: 120_000,
			forNextMonth: 0, spent: -56_200, available: 63_800,
			groups: [
				EnvelopeGroup(id: "everyday", name: "Everyday", isIncome: false, hidden: false, budgeted: 60_000, spent: -51_200, available: 8_800, envelopes: [
					envelope("groceries", "Groceries", budgeted: 45_000, spent: -41_200, available: 3_800),
					envelope("gas", "Gas", budgeted: 15_000, spent: -10_000, available: 5_000)
				]),
				EnvelopeGroup(id: "fun", name: "Fun", isIncome: false, hidden: false, budgeted: 20_000, spent: -2_000, available: 18_000, envelopes: [
					envelope("dining", "Dining out", budgeted: 10_000, spent: -5_000, available: 5_000),
					envelope("returns", "Returns", budgeted: 10_000, spent: 3_000, available: 13_000),
					envelope("gifts", "Gifts")
				]),
				EnvelopeGroup(id: "debt", name: "Savings and Debt", isIncome: false, hidden: false, budgeted: 50_000, spent: -50_000, available: 0, envelopes: [
					envelope("principal", "Car Loan Principal", budgeted: 30_000, spent: -30_000),
					envelope("emergency", "Emergency fund", budgeted: 20_000, spent: -20_000),
					envelope("fees", "Fees")
				]),
				EnvelopeGroup(id: "income", name: "Income", isIncome: true, hidden: false, budgeted: 0, spent: 500_000, available: 0, envelopes: [
					envelope("salary", "Salary", spent: 500_000),
					envelope("side", "Side work")
				])
			],
			mixedScaleNote: note
		)
	}

	static func lines(_ text: String) -> [String] { text.components(separatedBy: "\n") }

	static func category(_ name: String, _ report: NetProfitPeriod = scenarioA(), period: AssistantPeriod = november) -> [String] {
		lines(CategorySpendingTool.text(name, report: report, sheet: sheet(), period: period, format: format))
	}

	@Test func ex1SpendingIsTheExpensesLineByGroupAndNeverQuotesDebtPaydownOrSavings() {
		let text = SpendingTool.text(Self.scenarioA(), period: Self.soFar, format: Self.format)
		#expect(Self.lines(text) == [
			"Spending, November 1 to 22, 2026: $562.00",
			"This is the Expenses line of the Net Profit report. Debt paydown and savings are not included.",
			"Spending by category group, November 1 to 22, 2026:",
			"Everyday: $512.00",
			"Fun: $50.00"
		])
	}

	@Test func ex12TheLinesReconcileWithUncategorizedAndDeletedRows() {
		let extra = [
			Self.line("old-gym", "Old Gym", group: nil, nil, total: 1_200, count: 1, labels: Self.labels(deleted: true)),
			Self.line(nil, "", group: nil, nil, total: -4_000, count: 1, labels: Self.labels(needsReview: true))
		]
		let text = SpendingTool.text(Self.scenarioA(extra: extra), period: Self.november, format: Self.format)
		#expect(Array(Self.lines(text).prefix(1)) == ["Spending, November 2026: $534.00"])
		#expect(Array(Self.lines(text).dropFirst(3)) == ["Everyday: $512.00", "Fun: $50.00", "(deleted) Old Gym: $12.00", "Uncategorized (needs review): \(Self.minus)$40.00"])
	}

	@Test func aNamelessGroupReadsUnnamedGroup() {
		var report = Self.scenarioA()
		report.expenseGroups[1] = Self.group("fun", "", total: 5_000, count: 2)
		#expect(Self.lines(SpendingTool.text(report, period: Self.november, format: Self.format)).last == "Unnamed group: $50.00")
	}

	@Test func ex6IncomeIsTheIncomeLine() {
		let report = Self.scenarioA()
		let text = IncomeTool.text(report, period: AssistantPeriod(first: "2026-01-01", last: "2026-11-22", label: "January 1 to November 22, 2026"), format: Self.format)
		#expect(Array(Self.lines(text).prefix(2)) == ["Income, January 1 to November 22, 2026: $5,000.00", "This is the Income line of the Net Profit report."])
	}

	@Test(arguments: [
		("GROCERIES", ["Spending on Groceries, November 2026: $412.00", "From 2 transactions, on the Expenses line of the Net Profit report."]),
		("gifts", ["Spending on Gifts, November 2026: $0.00", "From 0 transactions, on the Expenses line of the Net Profit report."]),
		("side work", ["Income from Side work, November 2026: $0.00", "From 0 transactions, on the Income line of the Net Profit report."]),
		("salary", ["Income from Salary, November 2026: $5,000.00", "From 1 transaction, on the Income line of the Net Profit report."]),
		("Everyday", ["Spending on Everyday, November 2026: $512.00", "From 3 transactions, on the Expenses line of the Net Profit report."]),
		("groc", ["No category or group is called \"groc\". The closest name is Groceries."])
	])
	func ex3Ex5Ex7ACategoryOrGroupAnswersWithItsOwnLines(_ name: String, expected: [String]) {
		#expect(Self.category(name) == expected)
	}

	@Test func ex4LabelsFollowST10_3() {
		#expect(Self.category("groceries", Self.scenarioA(groceries: Self.labels(hidden: true))).first == "Spending on Groceries (hidden), November 2026: $412.00")
		let gym = Self.line("old-gym", "Old Gym", group: "fun", "Fun", total: 1_200, count: 1, labels: Self.labels(hidden: true, archived: true))
		#expect(Self.category("old gym", Self.scenarioA(extra: [gym])) == [
			"Spending on Old Gym (hidden, archived), November 2026: $12.00",
			"From 1 transaction, on the Expenses line of the Net Profit report."
		])
	}

	@Test func ex11AMixedGroupGivesEachKindItsOwnLineAndNeverSumsThem() {
		let report = NetProfitPeriod(
			column: Self.column(
				expenses: [Self.line("fees", "Fees", group: "debt", "Savings and Debt", total: 1_000, count: 1)],
				savings: []
			),
			incomeGroups: [],
			expenseGroups: [Self.group("debt", "Savings and Debt", total: 1_000, count: 1)],
			debtPaydownGroups: [Self.group("debt", "Savings and Debt", total: 30_000, count: 1)],
			savingsGroups: []
		)
		#expect(Self.category("savings and debt", report) == [
			"Spending on Savings and Debt, November 2026: $10.00",
			"From 1 transaction, on the Expenses line of the Net Profit report.",
			"Debt paydown for Savings and Debt, November 2026: $300.00. It is not spending."
		])
		let misfiled = NetProfitPeriod(
			column: Self.column(expenses: [Self.line("principal", "Car Loan Principal", group: "debt", "Savings and Debt", total: 2_000, count: 1)], debt: [], savings: []),
			incomeGroups: [], expenseGroups: [Self.group("debt", "Savings and Debt", total: 2_000, count: 1)], debtPaydownGroups: [], savingsGroups: []
		)
		#expect(Self.category("car loan principal", misfiled) == [
			"Spending on Car Loan Principal, November 2026: $20.00",
			"From 1 transaction, on the Expenses line of the Net Profit report."
		])
	}

	@Test func ex13AndAI9TheNetProfitSummaryLabelsEveryLine() {
		let report = Self.scenarioA()
		#expect(Self.lines(NetProfitTool.text(report.column, period: Self.soFar, format: Self.format)) == [
			"Net Profit, November 1 to 22, 2026: $4,438.00",
			"Income, November 1 to 22, 2026: $5,000.00",
			"Expenses, November 1 to 22, 2026: $562.00",
			"Debt paydown, November 1 to 22, 2026: $300.00. It is not spending.",
			"Savings, November 1 to 22, 2026: $200.00. It is not spending.",
			"Net cash flow, November 1 to 22, 2026: $3,938.00"
		])
		let withdrawn = Self.column(expenses: [], savings: [Self.line("emergency", "Emergency fund", group: "debt", "Savings and Debt", total: -5_000, count: 1)])
		#expect(Self.lines(NetProfitTool.text(withdrawn, period: Self.november, format: Self.format)).contains(
			"Savings, November 2026: \(Self.minus)$50.00. It is not spending. Negative means more was withdrawn than saved."
		))
	}

	@Test func ex8BudgetFiguresAreTheBudgetScreensAndNeverSaySpent() {
		#expect(BudgetTool.text("groceries", sheet: Self.sheet(), format: Self.format) == """
			Budgeted for Groceries, November 2026: $450.00
			Budget screen figures for November 2026: activity $412.00, available $38.00.
			""")
		#expect(BudgetTool.text("returns", sheet: Self.sheet(), format: Self.format).hasSuffix("activity \(Self.minus)$30.00, available $130.00."), "refunds win: the screen's negative activity")
		#expect(BudgetTool.text("", sheet: Self.sheet(), format: Self.format).hasPrefix("Budgeted, November 2026: $1,200.00\n"))
		#expect(BudgetTool.text("salary", sheet: Self.sheet(), format: Self.format).hasPrefix("Salary is income, so nothing is budgeted for it."))
		for name in ["groceries", "everyday", "", "salary", "returns"] {
			let text = BudgetTool.text(name, sheet: Self.sheet(), format: Self.format).lowercased()
			#expect(!text.contains("spent") && !text.contains("spending"), "\(text)")
		}
	}

	@Test func ex14AZeroPlaceBudgetShowsYenAndTheMixedScaleLine() {
		let yen = MoneyFormat(currencySymbol: "¥", decimalPlaces: 0)
		let report = NetProfitPeriod(
			column: Self.column(income: [], expenses: [Self.line("rice", "Rice cooker", group: "everyday", "Everyday", total: 50_000, count: 1)], debt: [], savings: []),
			incomeGroups: [], expenseGroups: [Self.group("everyday", "Everyday", total: 50_000, count: 1)], debtPaydownGroups: [], savingsGroups: []
		)
		#expect(Self.lines(SpendingTool.text(report, period: Self.november, format: yen)).first == "Spending, November 2026: ¥500")
		let note = "Spending is recorded at 2 places and budgets at 0, so these numbers mix both, as in Actual."
		var sheet = Self.sheet(note: note)
		sheet.groups[0].envelopes[0] = Self.envelope("rice", "Rice cooker", budgeted: 600, spent: -50_000, available: 100)
		let text = BudgetTool.text("rice cooker", sheet: sheet, format: yen)
		#expect(Self.lines(text).first == "Budgeted for Rice cooker, November 2026: ¥600")
		#expect(Self.lines(text).last == note)
		#expect(!text.lowercased().contains("over"))
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
			#expect(text.hasPrefix("Spending, October 2026: \(format.currency(expenses, on: .register))\nThis is the Expenses line"), "\(text)")
			for line in [NetProfitLine.debtPaydown, .savings] {
				#expect(!text.components(separatedBy: "\n").contains { $0.hasPrefix(ReportWords.line(line)) }, "\(line) is never quoted")
			}
		}
	}

	@Test func theSpendingLinesReconcileToTheTotal() async throws {
		try await RealBudget.with { budget in
			let report = try await context(budget).queries.netProfitPeriod(first: "2025-08-01", last: "2026-11-30")
			let grouped = report.expenseGroups.reduce(Int64(0)) { $0 + $1.total }
			let ungrouped = report.column.expenses.categories.filter { $0.groupId == nil }.reduce(Int64(0)) { $0 + $1.total }
			#expect(grouped + ungrouped == report.column.expenses.total)
		}
	}

	@Test func aCategoryInAnyCaseHasItsNetProfitLine() async throws {
		try await RealBudget.with { budget in
			let model = await netProfit(budget, Self.october, Self.october)
			let format = model.format
			let category = try #require(model.report?.total.expenses.categories.filter { $0.categoryId != nil && !$0.labels.hidden }.max { $0.count < $1.count })
			let text = try await CategorySpendingTool(context: context(budget)).call(arguments: .init(name: category.name.uppercased(), period: "last month"))
			let title = ReportWords.category(category.name, labels: category.labels)
			#expect(text.hasPrefix("Spending on \(title), October 2026: \(format.currency(category.total, on: .register))\n"), "\(text)")
		}
	}

	@Test func aGroupIsItsExpenseRowsOnTheNetProfitScreen() async throws {
		try await RealBudget.with { budget in
			let model = await netProfit(budget, Self.october, Self.october)
			let format = model.format
			let categories = try #require(model.report?.total.expenses.categories)
			let everyday = categories.filter { $0.groupId == AllFeatures.everydayGroup }
			let name = try #require(everyday.first?.groupName)
			let total = everyday.reduce(Int64(0)) { $0 + $1.total }
			let count = everyday.reduce(UInt64(0)) { $0 + $1.count }
			let text = try await CategorySpendingTool(context: context(budget)).call(arguments: .init(name: name.lowercased(), period: "October 2026"))
			#expect(text.hasPrefix("Spending on \(name), October 2026: \(format.currency(total, on: .register))\nFrom \(count) transactions, on the Expenses line"), "\(text)")
		}
	}

	@Test func thisYearsIncomeIsTheNetProfitScreensIncome() async throws {
		try await RealBudget.with { budget in
			let model = await netProfit(budget, Month(year: 2026, month: 1), Month(year: 2026, month: 12))
			let income = try #require(model.rows.first { $0.line == .income }?.cells.last?.amount)
			let text = try await IncomeTool(context: context(budget)).call(arguments: .init(period: "this year"))
			#expect(text.hasPrefix("Income, 2026: \(model.format.currency(income, on: .register))\n"), "\(text)")
		}
	}

	@Test func soFarThisMonthStopsAtToday() async throws {
		try await RealBudget.with { budget in
			let context = try context(budget, today: "2026-10-15")
			let partial = try await context.queries.netProfitPeriod(first: "2026-10-01", last: "2026-10-15")
			let whole = try await context.queries.netProfitPeriod(first: "2026-10-01", last: "2026-10-31")
			#expect(partial.column.expenses.total != whole.column.expenses.total, "the fixture has spending after the 15th")
			let text = try await SpendingTool(context: context).call(arguments: .init(period: "so far this month"))
			#expect(text.hasPrefix("Spending, October 1 to 15, 2026: \(context.format.currency(partial.column.expenses.total, on: .register))\n"), "\(text)")
		}
	}

	@Test func aBudgetFigureIsTheBudgetScreens() async throws {
		try await RealBudget.with { budget in
			let screen = await budget.screen(Self.october)
			let groceries = try #require(screen.sheet).row(AllFeatures.groceries)
			let format = screen.format
			let text = try await BudgetTool(context: context(budget)).call(arguments: .init(name: groceries.name.lowercased(), period: "last month"))
			#expect(text == """
				Budgeted for \(groceries.name), October 2026: \(format.currency(groceries.budgeted, on: .budget))
				Budget screen figures for October 2026: activity \(format.currency(-groceries.spent, on: .budget)), available \(format.currency(groceries.available, on: .budget)).
				""")
		}
	}
}
