import SwiftUI
import VeritasDesign

struct GroupedRows<Content: View>: View {
	@ViewBuilder let content: Content

	var body: some View {
		VStack(spacing: 0) {
			content
		}
		.background(.surfaceGrouped, in: .rect(cornerRadius: Radius.section))
		.overlay {
			RoundedRectangle(cornerRadius: Radius.section).strokeBorder(Color.edgeGrouped)
		}
	}
}

struct LabeledRow<Control: View>: View {
	let title: String
	@ViewBuilder let control: Control

	init(_ title: String, @ViewBuilder control: () -> Control) {
		self.title = title
		self.control = control()
	}

	var body: some View {
		HStack {
			Text(title)
			Spacer(minLength: Spacing.s4)
			control
		}
		.font(.text(.body))
		.padding(.horizontal, Spacing.s4)
		.frame(minHeight: Size.row)
	}
}
