import SwiftUI
import VeritasDesign

/// First launch, or after Close Budget (§9 #20).
struct EmptyStateView: View {
	let isWorking: Bool
	let importActualZip: () -> Void

	var body: some View {
		ContentUnavailableView {
			Text("No budget open")
				.font(.text(.title, weight: .semibold))
		} description: {
			Text("In Actual, open Settings and choose Export data. Then import the .zip file here.")
				.font(.text(.body))
		} actions: {
			Button("Import from Actual…", action: importActualZip)
				.buttonStyle(.borderedProminent)
				.controlSize(.large)
				.keyboardShortcut(.defaultAction)
				.disabled(isWorking)
				.accessibilityIdentifier("importFromActual")
			if isWorking {
				ProgressView()
					.controlSize(.small)
			}
		}
		.navigationTitle("Veritas")
	}
}
