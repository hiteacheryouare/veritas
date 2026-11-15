import SwiftUI
import VeritasDesign

struct SectionTitle: View {
	let text: String

	init(_ text: String) {
		self.text = text
	}

	var body: some View {
		Text(text)
			.font(.text(.heading, weight: .semibold))
			.padding(.leading, Spacing.s1)
			.accessibilityAddTraits(.isHeader)
	}
}
