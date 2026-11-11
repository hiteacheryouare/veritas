import SwiftUI
import VeritasDesign

struct GoToAnythingButton: View {
	let action: () -> Void

	var body: some View {
		Button(action: action) {
			HStack(spacing: 6) {
				Image(systemName: "magnifyingglass")
				Text("Go to anything")
				Spacer(minLength: Spacing.s4)
				Text("⌘K")
					.font(.text(.caption))
			}
			.foregroundStyle(.textTertiary)
			.frame(width: 186)
		}
		.help("Go to anything (⌘K)")
		.accessibilityLabel("Go to anything")
		.accessibilityIdentifier("toolbar.goToAnything")
	}
}
