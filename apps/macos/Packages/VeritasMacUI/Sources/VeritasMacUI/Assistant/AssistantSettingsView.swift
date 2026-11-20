import SwiftUI
import VeritasDesign
import VeritasKit

/// Settings › AI: the app-wide opt-in, off by default (R-AI-1).
struct AssistantSettingsView: View {
	static let toggleTitle = "Answer questions about your budget"
	static let scope = "Sees reports, budgets, accounts, goals and tax totals."

	@Bindable var settings: AssistantSettings

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s2) {
			GroupedRows {
				LabeledRow(Self.toggleTitle) {
					Toggle(Self.toggleTitle, isOn: $settings.isOn)
						.toggleStyle(.switch)
						.labelsHidden()
						.disabled(!settings.canTurnOn)
						.accessibilityIdentifier("settings.assistant")
				}
			}
			VStack(alignment: .leading, spacing: Spacing.s1) {
				Text(Self.scope)
					.foregroundStyle(.textSecondary)
				if let explanation = settings.availability?.explanation {
					Text(explanation)
						.foregroundStyle(.textPrimary)
						.accessibilityIdentifier("settings.assistant.unavailable")
				}
			}
			.font(.text(.caption))
			.padding(.horizontal, Spacing.s4)
		}
		.padding(Spacing.s5)
		.frame(maxWidth: 640, alignment: .leading)
		.frame(maxWidth: .infinity, alignment: .leading)
		.onAppear { settings.refresh() }
	}
}
