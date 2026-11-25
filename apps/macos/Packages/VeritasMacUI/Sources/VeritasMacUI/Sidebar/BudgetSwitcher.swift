import SwiftUI
import VeritasDesign
import VeritasKit

/// The open budget's name at the top of the sidebar, as a menu of the budgets
/// opened recently and the ways to open another.
struct BudgetSwitcher: View {
	let model: WorkspaceModel

	var body: some View {
		Menu {
			RecentBudgetItems(model: model)
			Divider()
			Button("Open…") { model.showOpenPicker() }
			Button("New Budget…") { model.startNewBudget() }
			Button("Import from Actual…") { model.isImportPanelShown = true }
		} label: {
			HStack(spacing: Spacing.s1) {
				Text(model.overview?.name ?? RecentBudgetItems.untitled)
					.font(.text(.body, weight: .semibold))
					.foregroundStyle(.textPrimary)
					.lineLimit(1)
				Image(systemName: "chevron.up.chevron.down")
					.imageScale(.small)
					.foregroundStyle(.textSecondary)
			}
			.frame(maxWidth: .infinity, alignment: .leading)
			.contentShape(.rect)
		}
		.menuStyle(.button)
		.buttonStyle(.plain)
		.menuIndicator(.hidden)
		.disabled(model.phase == .working)
		.help("Switch budget")
		.accessibilityIdentifier("budgetSwitcher")
	}
}

/// The recent budgets other than the open one, most recent first. The sidebar
/// menu and File › Open Recent share them.
struct RecentBudgetItems: View {
	static let untitled = "Untitled budget"

	let model: WorkspaceModel

	var body: some View {
		ForEach(model.recents.entries(besides: model.overview?.directory)) { budget in
			Button {
				Task { await model.switchTo(budget) }
			} label: {
				Text(budget.name ?? Self.untitled)
				Text(budget.place)
			}
			.accessibilityIdentifier("recentBudget.\(budget.name ?? Self.untitled)")
		}
	}
}
