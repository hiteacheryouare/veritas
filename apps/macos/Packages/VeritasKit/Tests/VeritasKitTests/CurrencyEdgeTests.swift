import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// CLS-12 and D63 on real budgets through the core: preferences as Actual
/// stores them, currencies from Actual's own table, and undo across a switch.
@MainActor
struct CurrencyEdgeTests {
	typealias F = AllFeatures

	/// Closes the fixture, sets Actual's preferences in its file, and reopens it.
	private func reopen(_ budget: RealBudget, with preferences: [String: String]) async throws {
		let directory = try budget.directory
		await budget.workspace.closeBudget()
		for (id, value) in preferences {
			try ActualDatabase.execute(
				directory.appending(path: "db.sqlite"),
				"INSERT OR REPLACE INTO preferences (id, value) VALUES ('\(id)', '\(value)')"
			)
		}
		await budget.workspace.reopen(directory: directory)
		try #require(budget.workspace.phase == .open)
	}

	@Test func aBudgetThatHidesDecimalsShowsWholeAmountsButKeepsTheCentsItStores() async throws {
		// Actual's hideFraction preference.
		try await RealBudget.with { budget in
			try await reopen(budget, with: ["hideFraction": "true"])
			let format = budget.workspace.format
			#expect(format.hidesDecimals)
			#expect(budget.workspace.budgetDecimalPlaces == 2, "fields still read cents")
			let screen = BudgetScreenModel(workspace: budget.workspace, month: F.october)
			await screen.load()
			let sheet = try #require(screen.sheet)
			#expect(format.currency(sheet.toBudget, on: .budget) == "45,052", "all-features names no currency, so no symbol")
			#expect(format.amount(try sheet.row(F.groceries).budgeted, on: .budget) == "140")
			#expect(format.amount(try sheet.row(F.household).available, on: .budget) == "\u{2212}1")

			screen.beginEditing(F.groceries)
			screen.editing?.type("+")
			screen.editing?.type("140.00+0.50")
			await screen.commitEditing()
			let groceries = try #require(screen.sheet).row(F.groceries)
			#expect(groceries.budgeted == 14_050, "the hidden cents are still typed and stored")
			#expect(format.amount(groceries.budgeted, on: .budget) == "141", "and shown rounded half away from zero")
			#expect(try await budget.monthFromDisk(F.october).row(F.groceries).budgeted == 14_050)
		}
	}

	@Test func aCurrencyActualDoesntListShowsNoSymbolAndUsesTwoPlaces() async throws {
		// CLS-12.7: a code missing from Actual's table is read as 2 places.
		try await RealBudget.with { budget in
			try await reopen(budget, with: ["defaultCurrencyCode": "XTS"])
			let format = budget.workspace.format
			#expect(format.currencySymbol == "")
			#expect(budget.workspace.budgetDecimalPlaces == 2)
			#expect(budget.workspace.mixedScaleNote == nil)
			let screen = BudgetScreenModel(workspace: budget.workspace, month: F.october)
			await screen.load()
			#expect(format.currency(try #require(screen.sheet).toBudget, on: .budget) == "45,052.29")
			screen.beginEditing(F.groceries)
			screen.editing?.type("50")
			await screen.commitEditing()
			#expect(try await budget.monthFromDisk(F.october).row(F.groceries).budgeted == 5_000)
		}
	}

	@Test func aZeroPlaceCurrencyWithTheSymbolAfterReadsAndTypesWholeRials() async throws {
		// IRR in Actual's table: 0 places, symbol after the amount.
		try await RealBudget.with { budget in
			let workspace = budget.workspace
			let rial = NewBudget(name: "Rial", currencyCode: "IRR", numberStyle: .commaDot, accounts: [], hasRetirementPlan: false, hasHealthSavingsAccount: false)
			await workspace.createBudget(rial)
			try #require(workspace.overview?.name == "Rial")
			let format = workspace.format
			#expect(!format.symbolFirst)
			#expect(format.decimalPlaces == 0)
			#expect(workspace.mixedScaleNote != nil)

			let screen = BudgetScreenModel(workspace: workspace)
			await screen.load()
			let envelope = try #require(screen.spendingEnvelopes.first)
			screen.beginEditing(envelope.id)
			screen.editing?.type("250000")
			await screen.commitEditing()
			let budgeted = try #require(screen.sheet).row(envelope.id).budgeted
			#expect(budgeted == 250_000)
			#expect(format.currency(budgeted, on: .budget) == "250,000\u{FDFC}")
			#expect(format.currency(-budgeted, on: .budget) == "\u{2212}250,000\u{FDFC}")
			#expect(format.currency(148_050, on: .register) == "1,481\u{FDFC}", "a register amount is a hundredth, rounded")

			let goals = GoalsScreenModel(workspace: workspace)
			let target = AmountFieldModel(amount: 0, style: goals.numberStyle, reader: goals.reader)
			target.type("1500000")
			let amount = try #require(await target.commit())
			#expect(await goals.create(name: "Carpet", target: amount, date: nil, icon: nil, color: nil))
			let carpet = try #require(goals.open.first { $0.name == "Carpet" })
			#expect(carpet.target == 1_500_000)
			#expect(format.wholeCurrency(carpet.target, on: .budget) == "1,500,000\u{FDFC}")
		}
	}

	@Test func undoNeverReachesAcrossASwitchToAnotherBudget() async throws {
		// R-REL-5 is per session: switching budgets starts a new history.
		try await RealBudget.with { budget in
			let workspace = budget.workspace
			let dollars = try budget.directory
			let screen = BudgetScreenModel(workspace: workspace, month: F.october)
			await screen.load()
			screen.beginEditing(F.groceries)
			screen.editing?.type("200")
			await screen.commitEditing()
			#expect(workspace.canUndo)

			await workspace.createBudget(CurrencySwitchTests.yen)
			let yen = try budget.directory
			#expect(!workspace.canUndo, "the yen budget has no history yet")
			await workspace.undo()
			let yenScreen = BudgetScreenModel(workspace: workspace)
			await yenScreen.load()
			let envelope = try #require(yenScreen.spendingEnvelopes.first)
			yenScreen.beginEditing(envelope.id)
			yenScreen.editing?.type("5000")
			await yenScreen.commitEditing()
			#expect(workspace.canUndo)

			await workspace.reopen(directory: dollars)
			#expect(!workspace.canUndo, "the dollar budget's undo history ended with its session")
			await workspace.undo()
			#expect(try await budget.monthFromDisk(F.october).row(F.groceries).budgeted == 20_000)

			await workspace.reopen(directory: yen)
			let again = BudgetScreenModel(workspace: workspace)
			await again.load()
			#expect(try #require(again.sheet).row(envelope.id).budgeted == 5_000)
			#expect(workspace.format.currency(5_000, on: .budget) == "¥5,000")
		}
	}
}
