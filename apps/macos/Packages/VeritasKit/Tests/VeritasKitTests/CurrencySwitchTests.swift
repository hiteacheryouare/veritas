import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// D63 and CLS-12 through the real core: a budget cell is typed at the open
/// budget's places, even right after switching from a budget with other
/// places. fixtures/synthetic/all-features is a 2-place budget; the 0-place
/// one is a new JPY budget made in the same scratch folder.
@MainActor
struct CurrencySwitchTests {
	static let yen = NewBudget(name: "Yen", currencyCode: "JPY", numberStyle: .commaDot, accounts: [], hasRetirementPlan: false, hasHealthSavingsAccount: false)

	@Test func switchingToAZeroPlaceBudgetTypesWholeYenAndBackTypesCents() async throws {
		try await RealBudget.with { budget in
			let workspace = budget.workspace
			let dollars = try budget.directory
			#expect(workspace.budgetDecimalPlaces == 2)
			// The Budget screen keeps its model in @State, so one made before
			// the switch must not keep the old budget's places.
			let screen = BudgetScreenModel(workspace: workspace)

			let placesWhenShown = Self.placesWhenOpen(workspace, named: Self.yen.name)
			await workspace.createBudget(Self.yen)
			#expect(await placesWhenShown.value == 0, "the places are set before any screen can show the budget")

			await screen.load()
			let envelope = try #require(screen.spendingEnvelopes.first)
			screen.beginEditing(envelope.id)
			screen.editing?.type("5000")
			await screen.commitEditing()
			let budgeted = try #require(screen.sheet).row(envelope.id).budgeted
			#expect(budgeted == 5_000, "5000 typed in a 0-place budget cell is stored as 5000, not 500000")
			#expect(try await budget.monthFromDisk(screen.month).row(envelope.id).budgeted == 5_000)
			#expect(workspace.format.currency(budgeted, on: .budget) == "¥5,000")
			#expect(screen.sheet?.mixedScaleNote != nil, "CLS-12.10's line shows in a 0-place budget")
			#expect(workspace.mixedScaleNote != nil, "and on its goals")

			screen.beginEditing(envelope.id)
			#expect(screen.editing?.text == "5,000", "the field opens on the row's own figure")
			screen.cancelEditing()

			await workspace.reopen(directory: dollars)
			#expect(workspace.budgetDecimalPlaces == 2)
			#expect(workspace.mixedScaleNote == nil)
			let back = BudgetScreenModel(workspace: workspace, month: AllFeatures.october)
			await back.load()
			#expect(back.sheet?.mixedScaleNote == nil)
			back.beginEditing(AllFeatures.groceries)
			back.editing?.type("50")
			await back.commitEditing()
			#expect(try #require(back.sheet).row(AllFeatures.groceries).budgeted == 5_000, "50 typed in a 2-place budget is $50.00")
			#expect(try await budget.monthFromDisk(AllFeatures.october).row(AllFeatures.groceries).budgeted == 5_000)
		}
	}

	@Test func aGoalTargetTypedAfterTheSwitchUsesTheNewPlaces() async throws {
		try await RealBudget.with { budget in
			let workspace = budget.workspace
			let goals = GoalsScreenModel(workspace: workspace)
			await workspace.createBudget(Self.yen)
			let target = AmountFieldModel(amount: 0, style: goals.numberStyle, reader: goals.reader)
			target.type("150000")
			let amount = try #require(await target.commit())
			#expect(amount == 150_000)
			#expect(await goals.create(name: "Bike", target: amount, date: nil, icon: nil, color: nil))
			#expect(goals.goals.first { $0.name == "Bike" }?.target == 150_000)
		}
	}

	/// What the workspace reports for places at the first moment it says the
	/// named budget is open, which is when the window first draws its screens.
	static func placesWhenOpen(_ workspace: WorkspaceModel, named name: String) -> Task<UInt8, Never> {
		Task { @MainActor in
			while !(workspace.phase == .open && workspace.overview?.name == name) {
				await Task.yield()
			}
			return workspace.budgetDecimalPlaces
		}
	}
}
