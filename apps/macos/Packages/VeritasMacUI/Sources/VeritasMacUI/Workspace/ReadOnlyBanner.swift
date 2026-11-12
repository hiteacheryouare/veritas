import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

struct ReadOnlyBanner: View {
	let notice: ReadOnlyNotice
	let perform: (ReadOnlyNotice.Action) -> Void

	var body: some View {
		HStack(spacing: Spacing.s2) {
			Image(systemName: "lock")
				.foregroundStyle(.textSecondary)
				.accessibilityHidden(true)
			Text(notice.message)
				.foregroundStyle(.textPrimary)
				.frame(maxWidth: .infinity, alignment: .leading)
			if let action = notice.action {
				Button(action.title) { perform(action) }
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
		.accessibilityIdentifier("readOnlyBanner")
	}
}
