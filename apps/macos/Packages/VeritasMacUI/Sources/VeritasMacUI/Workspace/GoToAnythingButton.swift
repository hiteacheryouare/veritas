import SwiftUI
import VeritasDesign

/// Looks like a search field, as in the mockups. With a Spacer in its label
/// SwiftUI left the item out of the toolbar entirely, so the width is fixed.
struct GoToAnythingButton: View {
	let action: () -> Void

	var body: some View {
		Button(action: action) {
			HStack(spacing: 6) {
				Image(systemName: "magnifyingglass")
				Text("Go to anything")
			}
			.frame(width: 186, alignment: .leading)
			.overlay(alignment: .trailing) {
				Text("⌘K")
					.font(.text(.caption))
			}
			.foregroundStyle(.textTertiary)
		}
		.help("Go to anything (⌘K)")
		.accessibilityLabel("Go to anything")
		.accessibilityIdentifier("toolbar.goToAnything")
	}
}
