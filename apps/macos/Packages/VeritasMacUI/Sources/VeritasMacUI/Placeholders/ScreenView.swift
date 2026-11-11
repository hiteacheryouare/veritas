import SwiftUI
import VeritasDesign
import VeritasKit

/// Every screen is owned by another workstream. Until it lands, its place
/// shows the title and nothing else.
struct ScreenView: View {
	let destination: Destination
	let title: String

	var body: some View {
		ContentUnavailableView {
			Text(title)
				.font(.text(.title, weight: .semibold))
				.foregroundStyle(.textTertiary)
		}
		.frame(maxWidth: .infinity, maxHeight: .infinity)
		.background(.surfaceWindow)
		.accessibilityElement(children: .combine)
		.accessibilityIdentifier("screen.\(destination.rawValue)")
	}
}
