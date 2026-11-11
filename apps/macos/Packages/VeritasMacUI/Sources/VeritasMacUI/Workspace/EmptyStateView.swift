import SwiftUI
import VeritasDesign

/// First launch, or after Close Budget: two equal ways in (D53).
struct EmptyStateView: View {
	let isWorking: Bool
	let newBudget: () -> Void
	let importActualZip: () -> Void

	var body: some View {
		ContentUnavailableView {
			Text("No budget open")
				.font(.text(.title, weight: .semibold))
		} actions: {
			HStack(spacing: Spacing.s3) {
				Button("New budget…", action: newBudget)
					.accessibilityIdentifier("newBudget")
				Button("Import from Actual…", action: importActualZip)
					.accessibilityIdentifier("importFromActual")
			}
			.buttonStyle(.bordered)
			.controlSize(.large)
			.disabled(isWorking)
			if isWorking {
				ProgressView()
					.controlSize(.small)
			}
		}
		.navigationTitle("Veritas")
	}
}
