import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// Dashboard cards in a 0-place budget through the real core (CLS-12.10).
@MainActor
struct ZeroPlaceReportTests {
	@Test func budgetAnalysisChartsTheEnvelopeSheetsOwnFiguresInAYenBudget() async throws {
		let budgets = URL.temporaryDirectory.appending(path: "veritas-real-\(UUID().uuidString)", directoryHint: .isDirectory)
		defer { try? FileManager.default.removeItem(at: budgets) }
		let workspace = WorkspaceModel(library: CoreBudgetLibrary(budgetsDirectory: budgets), format: MoneyFormat())
		let yen = NewBudget(
			name: "Yen", currencyCode: "JPY", numberStyle: .commaDot,
			accounts: [StartingAccount(name: "Wallet", type: .cash, balance: 10_000_000)],
			hasRetirementPlan: false, hasHealthSavingsAccount: false
		)
		await workspace.createBudget(yen)
		try #require(workspace.phase == .open)
		let wallet = try #require(workspace.overview?.accounts.first { $0.name == "Wallet" }).id

		let screen = BudgetScreenModel(workspace: workspace)
		await screen.load()
		let envelope = try #require(screen.spendingEnvelopes.first)
		screen.beginEditing(envelope.id)
		screen.editing?.type("5000")
		await screen.commitEditing()
		#expect(await workspace.perform(.addTransaction(transaction: TransactionDraft(
			id: nil, accountId: wallet, date: WorkspaceModel.today(), amount: -148_000, payeeId: nil, payeeName: "Ramen shop",
			categoryId: envelope.id, notes: nil, cleared: true, splits: []
		))), "a ¥1,480 purchase is 148000 on the register's scale")
		await screen.load()
		let sheet = try #require(screen.sheet)
		let row = try sheet.row(envelope.id)
		#expect(row.budgeted == 5_000)

		let model = DashboardModel(workspace: workspace)
		await model.load()
		if model.sheet == nil { await model.createDashboard(named: "Main") }
		let before = Set(model.sheet?.cards.map(\.id) ?? [])
		await model.add(.budgetAnalysis)
		await model.add(.spending)
		let analysis = try #require(model.sheet?.cards.first { $0.kind == .budgetAnalysis && !before.contains($0.id) })
		guard case .budgetAnalysis(let months, let budgeted, let spent) = analysis.content else {
			Issue.record("a budget analysis card, got \(analysis.content)")
			return
		}
		let index = try #require(months.lastIndex(of: sheet.month), "the card covers this month")
		#expect(budgeted[index] == -sheet.budgeted, "the card's budgeted is the sheet's (\(sheet.budgeted))")
		#expect(abs(spent[index]) == abs(sheet.spent), "and its spent is the sheet's mixed figure (\(sheet.spent))")
		let format = workspace.format
		#expect(format.chartValue(budgeted[index], on: .budget) == 5_000, "charted as ¥5,000, as the Budget screen shows it")
		#expect(format.chartValue(spent[index], on: .budget) == Double(spent[index]), "the sheet's numbers chart at the budget's 0 places (CLS-12.10)")

		let spending = try #require(model.sheet?.cards.first { $0.kind == .spending && !before.contains($0.id) })
		#expect(spending.config.name == nil)
		#expect(spending.title == "Monthly Spending", "an unnamed spending card takes Actual's title")
		await workspace.closeBudget()
	}
}
