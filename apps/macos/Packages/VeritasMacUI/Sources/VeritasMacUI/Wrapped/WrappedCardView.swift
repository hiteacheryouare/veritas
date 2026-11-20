import SwiftUI
import VeritasDesign
import VeritasKit

/// One story card as design/mockups/wrapped.html draws it. It shows only the
/// card's own text, so the redacted card can hold no amount.
struct WrappedCardView: View {
	static let size = CGSize(width: 400, height: 660)

	let card: WrappedCard
	/// The progress segments along the top: which card this is, of how many.
	var progress: (index: Int, count: Int)?
	var colors: WrappedColors = .window

	var body: some View {
		VStack(alignment: .leading, spacing: 0) {
			if let progress {
				HStack(spacing: 4) {
					ForEach(0..<progress.count, id: \.self) { index in
						Capsule()
							.fill(colors.ink.opacity(index <= progress.index ? 1 : 0.3))
							.frame(height: 3)
					}
				}
				.padding(.bottom, 36)
				.accessibilityElement()
				.accessibilityLabel("Card \(progress.index + 1) of \(progress.count)")
				.accessibilityIdentifier("wrapped.progress")
			} else {
				Spacer().frame(height: 22)
			}
			Text(card.title)
				.font(.system(size: 30, weight: .bold, design: .rounded))
				.lineSpacing(2)
				.fixedSize(horizontal: false, vertical: true)
				.accessibilityAddTraits(.isHeader)
				.accessibilityIdentifier("wrapped.title")
			if let figure = card.figure {
				Text(figure)
					.font(.system(size: TextSize.showcase.rawValue, weight: .bold, design: .rounded))
					.monospacedDigit()
					.lineLimit(1)
					.minimumScaleFactor(0.4)
					.padding(.top, 26)
					.padding(.bottom, 4)
					.accessibilityIdentifier("wrapped.figure")
			}
			if let detail = card.detail {
				Text(detail)
					.font(.text(.heading))
					.opacity(0.9)
					.fixedSize(horizontal: false, vertical: true)
					.padding(.top, card.figure == nil ? Spacing.s3 : 0)
					.accessibilityIdentifier("wrapped.detail")
			}
			Spacer(minLength: Spacing.s4)
			VStack(alignment: .leading, spacing: Spacing.s3) {
				ForEach(card.rows) { row in
					VStack(alignment: .leading, spacing: 2) {
						HStack(alignment: .firstTextBaseline, spacing: Spacing.s3) {
							Text(row.label)
								.font(.text(.heading, weight: .medium))
								.lineLimit(1)
							Spacer(minLength: Spacing.s2)
							Text(row.value)
								.font(.figures(.heading, weight: .semibold))
								.lineLimit(1)
								.layoutPriority(1)
						}
						if let note = row.note {
							Text(note)
								.font(.text(.caption))
								.opacity(0.75)
						}
					}
					.accessibilityElement(children: .combine)
					.accessibilityIdentifier("wrapped.row.\(row.id)")
				}
			}
		}
		.foregroundStyle(colors.ink)
		.padding(.top, 22)
		.padding(.horizontal, 30)
		.padding(.bottom, 28)
		.frame(width: Self.size.width, height: Self.size.height, alignment: .topLeading)
		.background(colors.card, in: .rect(cornerRadius: 28))
		.accessibilityElement(children: .contain)
		.accessibilityIdentifier("wrapped.card.\(card.id)")
	}
}

/// The card's colors: the window's appearance on screen, and the light
/// palette in an exported image so the file looks the same wherever it opens.
struct WrappedColors {
	let card: Color
	let ink: Color
	let page: Color

	static let window = WrappedColors(card: .accentFill, ink: .textOnAccent, page: .surfaceWindow)

	static let image = WrappedColors(
		card: Color(rgba: ColorToken.accentFill.light),
		ink: Color(rgba: ColorToken.textOnAccent.light),
		page: Color(rgba: ColorToken.surfaceWindow.light)
	)
}

private extension Color {
	init(rgba: RGBA) {
		self.init(.sRGB, red: rgba.red, green: rgba.green, blue: rgba.blue, opacity: rgba.alpha)
	}
}
