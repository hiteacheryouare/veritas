import SwiftUI
import VeritasDesign

/// First launch, or after Close Budget: two equal ways in (D53).
struct EmptyStateView: View {
	let isWorking: Bool
	let newBudget: () -> Void
	let importActualZip: () -> Void

	var body: some View {
		VStack(spacing: Spacing.s3) {
			Text("No budget open")
				.font(.text(.title, weight: .semibold))
				.foregroundStyle(.textPrimary)
			Text("Start a new budget, or import the .zip from Export data in Actual's settings.")
				.font(.text(.body))
				.foregroundStyle(.textSecondary)
				.multilineTextAlignment(.center)
			HStack(spacing: Spacing.s3) {
				Button("New budget…", action: newBudget)
					.buttonStyle(.borderedProminent)
					.keyboardShortcut(.defaultAction)
					.accessibilityIdentifier("newBudget")
				Button("Import from Actual…", action: importActualZip)
					.buttonStyle(.bordered)
					.accessibilityIdentifier("importFromActual")
			}
			.controlSize(.large)
			.disabled(isWorking)
			.padding(.top, Spacing.s2)
			ProgressView()
				.controlSize(.small)
				.opacity(isWorking ? 1 : 0)
				.accessibilityHidden(!isWorking)
		}
		.padding(Spacing.s6)
		.frame(maxWidth: .infinity, maxHeight: .infinity)
		.navigationTitle("Veritas")
	}
}
