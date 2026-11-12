import SwiftUI
import VeritasDesign

/// ⌘F's find bar, under the toolbar. Each screen wires the text to its own list
/// when it's built. The bar stays in the hierarchy while hidden so the field
/// already exists when ⌘F asks for focus.
struct FindBar: View {
	@Binding var isShown: Bool
	@State private var text = ""
	@FocusState private var isFocused: Bool

	var body: some View {
		HStack(spacing: Spacing.s2) {
			Image(systemName: "magnifyingglass")
				.foregroundStyle(.textTertiary)
				.accessibilityHidden(true)
			TextField("Search", text: $text)
				.textFieldStyle(.plain)
				.focused($isFocused)
				.disabled(!isShown)
				.onKeyPress(.escape) {
					isShown = false
					return .handled
				}
				.accessibilityIdentifier("findBar.field")
			Button("Close") { isShown = false }
				.controlSize(.small)
		}
		.font(.text(.body))
		.padding(.horizontal, Spacing.s5)
		.frame(height: isShown ? Size.control + Spacing.s2 : 0)
		.clipped()
		.opacity(isShown ? 1 : 0)
		.background(.surfaceGrouped)
		.overlay(alignment: .bottom) {
			if isShown {
				Rectangle().fill(Color.separator).frame(height: 1)
			}
		}
		.accessibilityHidden(!isShown)
		.motion(.fast, value: isShown)
		.onChange(of: isShown) { _, isShown in
			isFocused = isShown
			if !isShown { text = "" }
		}
	}
}
