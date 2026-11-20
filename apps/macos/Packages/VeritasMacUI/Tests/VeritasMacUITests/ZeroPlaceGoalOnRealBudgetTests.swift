import Foundation
import Testing
import VeritasCore
import VeritasKit
@testable import VeritasMacUI

/// CLS-12.14 through the real core: in a 0-place budget a goal's available
/// mixes the budget's and the register's scales, so it is only "Available".
@MainActor
struct ZeroPlaceGoalOnRealBudgetTests {
	@Test func aSpentYenGoalReadsAvailableAndNeverOverOrLeft() async throws {
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
		#expect(await goals.create(name: "Bike", target: 10_000, date: nil, icon: nil, color: nil))
		let bike = try #require(goals.open.first { $0.name == "Bike" })
		let screen = BudgetScreenModel(workspace: workspace)
		await screen.load()
		#expect(await screen.moveBudget(amount: 10_000, from: nil, to: bike.categoryId), "¥10,000 budgeted reaches the goal")
		#expect(await workspace.perform(.addTransaction(transaction: TransactionDraft(
			id: nil, accountId: wallet, date: WorkspaceModel.today(), amount: -50_000, payeeId: nil, payeeName: "Bike shop",
			categoryId: bike.categoryId, notes: nil, cleared: true, splits: []
		))), "a ¥500 purchase")
		await goals.load()
		let spent = try #require(goals.goals.first { $0.id == bike.id })
		#expect(spent.state == .reachedSpent)
		#expect(spent.available == 10_000 - 50_000, "the envelope sheet's mixed figure")
		let words = GoalWords(spent, format: workspace.format)
		let detail = try #require(words.detail)
		#expect(detail.contains("Available"), "\(detail)")
		#expect(!detail.contains("over") && !detail.contains("left"), "\(detail)")
		#expect(detail.hasPrefix("¥500 spent"), "spending is on the register's scale: \(detail)")
		#expect(words.tone != .overspent, "the mixed figure isn't read as overspending")
		await workspace.closeBudget()
	}
}
