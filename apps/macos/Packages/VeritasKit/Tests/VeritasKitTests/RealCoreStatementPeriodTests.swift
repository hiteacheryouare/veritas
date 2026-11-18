import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// The Statements screen stepping through time and in a 0-place currency,
/// against the real core (R-REP-7): each period's closing figure is the next
/// period's opening one, whichever way the toolbar steps, and a yen budget's
/// balance sheet shows its true amounts with its currency named.
@MainActor
struct RealCoreStatementPeriodTests {
	static let today = "2026-11-16"

	private func figures(_ model: StatementsModel) throws -> [String: Int64] {
		let document = try #require(model.document)
		return Dictionary(document.lines.compactMap { line in line.amount.map { (line.id, $0) } }, uniquingKeysWith: { first, _ in first })
	}

	@Test(arguments: [StatementKind.ownersEquity, .cashFlow])
	func eachPeriodOpensWhereThePreviousOneClosed(_ kind: StatementKind) async throws {
		try await RealBudget.with { budget in
			let model = StatementsModel(workspace: budget.workspace, today: Self.today)
			await model.show(kind)
			for span in [SankeySpan.month, .quarter, .year] {
				await model.choose(span)
				await model.previous()
				await model.previous()
				let earlier = try figures(model)
				let earlierPeriod = model.period
				await model.next()
				let later = try figures(model)
				#expect(model.period.first == earlierPeriod.last.adding(months: 1), "\(kind) \(span): periods meet")
				#expect(
					later["opening"] == earlier["closing"],
					"\(kind) \(span): \(model.periodLabel) opens on \(String(describing: later["opening"])), the period before closed on \(String(describing: earlier["closing"]))"
				)
			}
			await model.setCustom(first: Month(year: 2025, month: 11), last: Month(year: 2026, month: 2))
			let across = try figures(model)
			await model.setCustom(first: Month(year: 2026, month: 3), last: Month(year: 2026, month: 6))
			#expect(try figures(model)["opening"] == across["closing"], "\(kind): a custom range across the new year closes where the next opens")
		}
	}

	@Test func aYenBudgetsBalanceSheetShowsItsTrueAmountsAndNamesTheCurrency() async throws {
		try await RealBudget.with { budget in
			let workspace = budget.workspace
			await workspace.createBudget(CurrencySwitchTests.yen)
			try #require(workspace.phase == .open)
			// The Budget screen reads the month first, which tells the core
			// the local date a new account's starting balance is dated on.
			_ = await workspace.budgetMonth(try #require(Month(date: WorkspaceModel.today())))
			let account = NewAccount(name: "Savings", kind: .savings, startingBalance: 12_345_600, loan: nil)
			#expect(await workspace.perform(.createAccount(account: account)), "\(String(describing: workspace.alert))")
			let model = StatementsModel(workspace: workspace, today: WorkspaceModel.today())
			await model.show(.balanceSheet)
			let sheet = try #require(await workspace.report { try await $0.balanceSheet(asOf: WorkspaceModel.today()) })
			let shown = try figures(model)
			#expect(shown["netWorth"] == sheet.netWorth)
			#expect(sheet.netWorth == 12_345_600, "a ¥123,456 starting balance, register scale")
			#expect(workspace.format.currency(sheet.netWorth, on: .register) == "¥123,456", "shown as the true amount, never ¥12,345,600")
			let document = try #require(model.document)
			#expect(document.basis.hasSuffix(", in JPY"), "\(document.basis)")
		}
	}
}
