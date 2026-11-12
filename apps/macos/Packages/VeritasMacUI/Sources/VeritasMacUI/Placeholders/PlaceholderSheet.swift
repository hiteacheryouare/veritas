import SwiftUI
import VeritasDesign
import VeritasKit

struct PlaceholderSheet: View {
	let sheet: WorkspaceSheet
	@Environment(\.dismiss) private var dismiss

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s5) {
			Text(sheet.title)
				.font(.text(.title, weight: .semibold))
			HStack {
				Spacer()
				Button(sheet == .integrityReport ? "Close" : "Cancel", role: .cancel) { dismiss() }
					.keyboardShortcut(.cancelAction)
			}
		}
		.padding(Spacing.s5)
		.frame(width: 440)
		.accessibilityIdentifier("sheet.\(sheet.rawValue)")
	}
}
