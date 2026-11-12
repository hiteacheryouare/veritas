import SwiftUI
import VeritasDesign
import VeritasKit

/// Budgets in the app's budgets folder, most recently opened first.
struct OpenBudgetSheet: View {
	let budgets: [BudgetListingItem]?
	let open: (BudgetListingItem) -> Void
	@Environment(\.dismiss) private var dismiss
	@State private var selection: BudgetListingItem.ID?

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s4) {
			Text("Open budget")
				.font(.text(.title, weight: .semibold))
			if let budgets {
				if budgets.isEmpty {
					Text("No budgets yet.")
						.foregroundStyle(.textSecondary)
				} else {
					List(budgets, selection: $selection) { budget in
						HStack {
							Text(budget.name ?? "Untitled budget")
							Spacer()
							if let opened = budget.lastOpened {
								Text(When.text(opened))
									.font(.figures(.body))
									.foregroundStyle(.textSecondary)
							}
						}
						.accessibilityElement(children: .combine)
						.accessibilityIdentifier("openBudget.\(budget.id)")
					}
					.scrollContentBackground(.hidden)
					.background(.surfaceGrouped, in: .rect(cornerRadius: Radius.section))
					.frame(height: 220)
					.contextMenu(forSelectionType: BudgetListingItem.ID.self) { _ in } primaryAction: { ids in
						chosen(ids.first).map(open)
					}
				}
			} else {
				ProgressView().controlSize(.small)
			}
			HStack {
				Spacer()
				Button("Cancel", role: .cancel) { dismiss() }
					.keyboardShortcut(.cancelAction)
				Button("Open") { chosen(selection).map(open) }
					.buttonStyle(.borderedProminent)
					.keyboardShortcut(.defaultAction)
					.disabled(chosen(selection) == nil)
					.accessibilityIdentifier("openBudget.open")
			}
		}
		.padding(Spacing.s5)
		.frame(width: 480)
		.onAppear { selection = budgets?.first?.id }
		.onChange(of: budgets) { selection = budgets?.first?.id }
		.containerIdentifier("sheet.openBudget")
	}

	private func chosen(_ id: BudgetListingItem.ID?) -> BudgetListingItem? {
		budgets?.first { $0.id == id }
	}
}
