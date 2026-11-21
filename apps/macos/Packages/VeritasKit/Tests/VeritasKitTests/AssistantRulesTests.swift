import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// Fred's rules for what the model may say (PR #81 review): tax totals never
/// become advice, reserved words stay with their figures, and scale notes travel with them.
struct AssistantRulesTests {
	@Test func theInstructionsCarryEachRuleWordForWord() {
		let instructions = AssistantPrompt.instructions(today: "2026-11-18")
		for rule in [
			"Tax buckets only total what the user tagged. Never say an amount is deductible, taxable or owed, and don't give tax advice.",
			"Use each tool's own words for its figures. Call a figure Income, Expenses, Spending or Net Profit only when the tool names it that. Never call a balance, a saved report's total or a budget figure by those words.",
			"If a result includes a note about scales, repeat it with the figures.",
			"A question about now, such as a current balance or net worth, uses today's date.",
			"Today is 2026-11-18."
		] {
			#expect(instructions.contains(rule), "\(rule)")
		}
	}

	@Test func aNegativeTaxBucketCarriesTheTax4Note() async throws {
		let summary = TaxSummary(
			year: 2026,
			buckets: [
				TaxBucketTotal(bucket: .charitable, inflow: false, total: 13_000, count: 2),
				TaxBucketTotal(bucket: .medical, inflow: false, total: -2_300, count: 1, negative: true)
			],
			detail: [], conflicts: 0, summaryCsv: "", detailCsv: ""
		)
		let context = AssistantContext(queries: CannedQueries(tax: summary), format: MoneyFormat(), today: "2026-11-18")
		let text = try await TaxSummaryTool(context: context).call(arguments: .init(year: 2026))
		#expect(text.contains("\nMedical: \u{2212}23.00 from 1 transaction. Refunds or reversals exceed what was counted."))
		#expect(text.contains("\nCharitable giving: 130.00 from 2 transactions\n") || text.hasSuffix("\nCharitable giving: 130.00 from 2 transactions"))
		#expect(text.components(separatedBy: TaxBuckets.negativeNote).count == 2, "only the negative bucket has the note")
	}
}

/// Conforms to `ReadOnlyBudgetQueries` alone and answers from fixed values.
struct CannedQueries: ReadOnlyBudgetQueries {
	struct Missing: Error {}

	var tax: TaxSummary?
	var goalList: GoalList?

	func netProfit(first: Month, last: Month) async throws -> NetProfitReport { throw Missing() }
	func balanceSheet(asOf: String) async throws -> BalanceSheet { throw Missing() }
	func cashFlowStatement(first: String, last: String) async throws -> CashFlowColumn { throw Missing() }
	func budgetMonth(_ month: Month, today: String) async throws -> BudgetMonth { throw Missing() }
	func accounts() async throws -> AccountList { throw Missing() }
	func goals(today: String) async throws -> GoalList { guard let goalList else { throw Missing() }; return goalList }
	func taxSummary(year: Int32) async throws -> TaxSummary { guard let tax else { throw Missing() }; return tax }
	func customReports() async throws -> [CustomReportItem] { throw Missing() }
	func customReport(_ id: String, today: String) async throws -> CustomReportView { throw Missing() }
}

/// Goals and the budget month in a 0-place budget through the real core (CLS-12.10, CLS-12.14).
@MainActor
struct AssistantZeroPlaceTests {
	@Test func aYenBudgetPutsTheScaleNoteFirstAndKeepsSpendingApartFromAvailable() async throws {
		let budgets = URL.temporaryDirectory.appending(path: "veritas-real-\(UUID().uuidString)", directoryHint: .isDirectory)
		defer { try? FileManager.default.removeItem(at: budgets) }
		let workspace = WorkspaceModel(library: CoreBudgetLibrary(budgetsDirectory: budgets), format: MoneyFormat())
		await workspace.createBudget(NewBudget(
			name: "Yen", currencyCode: "JPY", numberStyle: .commaDot,
			accounts: [StartingAccount(name: "Wallet", type: .cash, balance: 10_000_000)],
			hasRetirementPlan: false, hasHealthSavingsAccount: false
		))
		try #require(workspace.phase == .open)
		let wallet = try #require(workspace.overview?.accounts.first { $0.name == "Wallet" }).id

		let goals = GoalsScreenModel(workspace: workspace)
		await goals.load()
		#expect(await goals.create(name: "Rice cooker", target: 1_000, date: nil, icon: nil, color: nil))
		#expect(await goals.create(name: "Trip", target: 50_000, date: nil, icon: nil, color: nil))
		let cooker = try #require(goals.open.first { $0.name == "Rice cooker" })
		let screen = BudgetScreenModel(workspace: workspace)
		await screen.load()
		screen.beginEditing(cooker.categoryId)
		screen.editing?.type("1000")
		await screen.commitEditing()
		#expect(await workspace.perform(.addTransaction(transaction: TransactionDraft(
			id: nil, accountId: wallet, date: WorkspaceModel.today(), amount: -120_000, payeeId: nil, payeeName: "Appliance shop",
			categoryId: cooker.categoryId, notes: nil, cleared: true, splits: []
		))), "a ¥1,200 purchase is 120000 on the register's scale")
		await goals.load()
		let spent = try #require(goals.goals.first { $0.id == cooker.id })
		#expect(spent.state == .reachedSpent)

		let format = workspace.format
		#expect(format.decimalPlaces == 0)
		let context = AssistantContext(queries: try #require(workspace.readOnlyQueries), format: format, today: WorkspaceModel.today())
		let list = try await context.queries.goals(today: context.today)
		#expect(list.zeroPlaces)
		let coreNote = try #require(list.mixedScaleNote, "the core sets CLS-12.10's note in a 0-place budget")
		let text = try await GoalsTool(context: context).call(arguments: .init())
		let lines = text.components(separatedBy: "\n")
		#expect(lines.first == coreNote)
		let goalLine = try #require(lines.first { $0.hasPrefix("Rice cooker: ") })
		#expect(goalLine.contains("target \(format.wholeCurrency(spent.target, on: .budget))"))
		#expect(goalLine.contains("available \(format.currency(spent.available, on: .budget))"))
		#expect(goalLine.hasSuffix("reached, then spent"), "the screen's Reached → Spent")
		#expect(!goalLine.contains(format.currency(spent.spent, on: .register)), "spending to date is never beside available")
		for word in ["spending", "left", "over", "remaining", " of "] {
			#expect(!goalLine.contains(word), "\(goalLine)")
		}
		#expect(lines.contains("Rice cooker spending to date: \(format.currency(spent.spent, on: .register))"))
		#expect(lines.contains("Rice cooker spending to date: ¥1,200"))
		let trip = try #require(lines.first { $0.hasPrefix("Trip: ") })
		#expect(trip.hasPrefix("Trip: available ¥0, target ¥50,000"))

		let month = try await BudgetMonthTool(context: context).call(arguments: .init(month: String(context.today.prefix(7))))
		#expect(month.components(separatedBy: "\n").first == coreNote, "the note comes before the figures it explains")
		await workspace.closeBudget()
	}

	@Test func aYenGoalWithSpendingRecordedSaysSoWithNoFigure() async throws {
		// D77: in a budget without 2 places, an active goal with spending has no ETA and no needed amount.
		let budgets = URL.temporaryDirectory.appending(path: "veritas-real-\(UUID().uuidString)", directoryHint: .isDirectory)
		defer { try? FileManager.default.removeItem(at: budgets) }
		let workspace = WorkspaceModel(library: CoreBudgetLibrary(budgetsDirectory: budgets), format: MoneyFormat())
		await workspace.createBudget(NewBudget(
			name: "Yen", currencyCode: "JPY", numberStyle: .commaDot,
			accounts: [StartingAccount(name: "Wallet", type: .cash, balance: 10_000_000)],
			hasRetirementPlan: false, hasHealthSavingsAccount: false
		))
		try #require(workspace.phase == .open)
		let wallet = try #require(workspace.overview?.accounts.first { $0.name == "Wallet" }).id

		let goals = GoalsScreenModel(workspace: workspace)
		let due = try #require(Calendar.current.date(byAdding: .month, value: 8, to: .now))
		#expect(await goals.create(name: "Trip", target: 300_000, date: due, icon: nil, color: nil))
		let trip = try #require(goals.open.first { $0.name == "Trip" })
		#expect(await workspace.perform(.addTransaction(transaction: TransactionDraft(
			id: nil, accountId: wallet, date: WorkspaceModel.today(), amount: -500_000, payeeId: nil, payeeName: "Rail pass",
			categoryId: trip.categoryId, notes: nil, cleared: true, splits: []
		))))

		let context = AssistantContext(queries: try #require(workspace.readOnlyQueries), format: workspace.format, today: WorkspaceModel.today())
		let list = try await context.queries.goals(today: context.today)
		let recorded = try #require(list.goals.first { $0.id == trip.id })
		#expect(recorded.state == .active)
		#expect(recorded.pace == .spendingRecorded, "the core's D77 pace")
		#expect(recorded.suggestedMonthly == nil)

		let text = try await GoalsTool(context: context).call(arguments: .init())
		let line = try #require(text.components(separatedBy: "\n").first { $0.hasPrefix("Trip: ") })
		#expect(line.hasSuffix("no ETA, spending recorded"), "\(line)")
		#expect(!line.contains("needs") && !line.contains("a month"), "no needed figure beside it")
		await workspace.closeBudget()
	}
}
