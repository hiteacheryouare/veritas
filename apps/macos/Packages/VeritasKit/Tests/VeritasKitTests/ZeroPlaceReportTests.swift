import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// Dashboard cards in a 0-place budget through the real core (CLS-12.10).
@MainActor
struct ZeroPlaceReportTests {
	@Test func everyCardFigureReadsAtItsTrueAmountInAYenBudget() async throws {
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
		guard case .budgetAnalysis(_, _, let months, _, _, _, _) = analysis.content else {
			Issue.record("a budget analysis card, got \(analysis.content)")
			return
		}
		let point = try #require(months.last { $0.month == sheet.month }, "the card covers this month")
		let format = workspace.format
		#expect(format.amount(abs(point.budgeted), on: .register) == "5,000", "D74: the ¥5,000 budget cell arrives on the register's scale and reads at its true amount")
		#expect(abs(format.chartValue(point.budgeted, on: .register)) == 5_000)
		#expect(format.amount(abs(point.spent), on: .register) == "1,480", "and the ¥1,480 purchase reads as itself, not 100 times off")

		let spending = try #require(model.sheet?.cards.first { $0.kind == .spending && !before.contains($0.id) })
		#expect(spending.config.name == nil)
		#expect(spending.title == "Monthly spending", "an unnamed spending card takes Actual's title in sentence case")
		guard case .spending(.singleMonth, _, _, _, _, let days, let headlineDay, _) = spending.content else {
			Issue.record("a single-month spending card, got \(spending.content)")
			return
		}
		let today = try #require(days.first { $0.day == headlineDay }?.compare)
		#expect(format.amount(abs(today), on: .register) == "1,480", "the month to date is the purchase, on the register's scale")
		await workspace.closeBudget()
	}
}
