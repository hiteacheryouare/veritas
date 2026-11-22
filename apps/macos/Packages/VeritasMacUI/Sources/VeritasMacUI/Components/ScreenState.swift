import SwiftUI
import VeritasDesign

/// What a screen shows in place of its content: loading, or nothing to show yet.
/// Every screen uses this one view, centered in its content area.
struct ScreenState: View {
	enum Kind {
		case loading
		case empty(title: String, message: String? = nil)
	}

	let kind: Kind
	var action: (title: String, run: () -> Void)?
	var isActionDisabled = false
	var identifier = "screenState"
	@State private var isLoadingShown = false

	var body: some View {
		VStack(spacing: Spacing.s3) {
			switch kind {
			case .loading:
				// A load that finishes quickly never flashes a spinner.
				ProgressView()
					.controlSize(.small)
					.opacity(isLoadingShown ? 1 : 0)
					.task {
						try? await Task.sleep(for: .milliseconds(250))
						isLoadingShown = true
					}
					.accessibilityLabel("Loading")
					.accessibilityHidden(!isLoadingShown)
			case .empty(let title, let message):
				Text(title)
					.font(.text(.heading, weight: .semibold))
					.foregroundStyle(.textPrimary)
				if let message {
					Text(message)
						.font(.text(.body))
						.foregroundStyle(.textSecondary)
						.multilineTextAlignment(.center)
						.frame(maxWidth: 360)
				}
				if let action {
					Button(action.title, action: action.run)
						.controlSize(.large)
						.disabled(isActionDisabled)
						.padding(.top, Spacing.s1)
						.accessibilityIdentifier("\(identifier).action")
				}
			}
		}
		.padding(Spacing.s6)
		.frame(maxWidth: .infinity, maxHeight: .infinity)
		.accessibilityElement(children: .contain)
		.accessibilityIdentifier(identifier)
	}
}
