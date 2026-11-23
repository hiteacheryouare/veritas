import SwiftUI
import VeritasDesign

/// A magnifier with its shortcut. It used to look like a wide search field,
/// which pushed other toolbar items into the overflow menu at 1280 pt.
struct GoToAnythingButton: View {
	let action: () -> Void

	var body: some View {
		Button(action: action) {
			HStack(spacing: 4) {
				Image(systemName: "magnifyingglass")
				Text("⌘K")
					.font(.text(.caption))
					.foregroundStyle(.textSecondary)
			}
		}
		.help("Go to anything (⌘K)")
		.accessibilityLabel("Go to anything")
		.accessibilityIdentifier("toolbar.goToAnything")
	}
}
