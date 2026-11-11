import SwiftUI
import VeritasDesign

/// A strip under the toolbar for a state that needs attention but never blocks
/// work: a read-only budget, a failed backup.
struct NoticeBanner: View {
	let symbol: String
	let message: String
	var actionTitle: String?
	var action: () -> Void = {}

	var body: some View {
		HStack(spacing: Spacing.s2) {
			Image(systemName: symbol)
				.foregroundStyle(.textSecondary)
				.accessibilityHidden(true)
			Text(message)
				.foregroundStyle(.textPrimary)
				.frame(maxWidth: .infinity, alignment: .leading)
			if let actionTitle {
				Button(actionTitle, action: action)
					.controlSize(.small)
			}
		}
		.font(.text(.body))
		.padding(.horizontal, Spacing.s5)
		.padding(.vertical, Spacing.s2)
		.frame(minHeight: Size.control)
		.background(.surfaceGrouped)
		.overlay(alignment: .bottom) {
			Rectangle().fill(Color.separator).frame(height: 1)
		}
		.accessibilityElement(children: .contain)
	}
}
