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
			if !card.bars.isEmpty {
				WrappedBars(bars: card.bars, colors: colors)
					.frame(height: 190)
			}
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
	/// The biggest month's bar.
	let highlight: Color

	static let window = WrappedColors(card: .accentFill, ink: .textOnAccent, page: .surfaceWindow, highlight: .moneyAvailableFill)

	static let image = WrappedColors(
		card: Color(rgba: ColorToken.accentFill.light),
		ink: Color(rgba: ColorToken.textOnAccent.light),
		page: Color(rgba: ColorToken.surfaceWindow.light),
		highlight: Color(rgba: ColorToken.moneyAvailableFill.light)
	)
}

/// The biggest-month card's chart (design/mockups/wrapped.html): twelve
/// slots from January, each bar as tall as the core's share against the
/// tallest on the card, and a refund month below the line.
private struct WrappedBars: View {
	static let slots = 12
	static let gap: CGFloat = 6
	static let labelHeight: CGFloat = 20
	static let valueHeight: CGFloat = 20

	let bars: [WrappedCard.Bar]
	let colors: WrappedColors

	var body: some View {
		GeometryReader { proxy in
			let width = (proxy.size.width - Self.gap * CGFloat(Self.slots - 1)) / CGFloat(Self.slots)
			let up = CGFloat(bars.map(\.share).max().map { max($0, 0) } ?? 0)
			let down = CGFloat(bars.map(\.share).min().map { max(-$0, 0) } ?? 0)
			let plot = proxy.size.height - Self.labelHeight - Self.valueHeight
			let scale = up + down > 0 ? plot / (up + down) : 0
			let axis = Self.valueHeight + up * scale
			ZStack(alignment: .topLeading) {
				ForEach(Array(bars.enumerated()), id: \.element.id) { index, bar in
					let x = CGFloat(index) * (width + Self.gap)
					let height = max(CGFloat(abs(bar.share)) * scale, bar.share == 0 ? 0 : 2)
					let top = bar.share >= 0 ? axis - height : axis
					RoundedRectangle(cornerRadius: 4)
						.fill(bar.isBiggest ? colors.highlight : colors.ink.opacity(0.4))
						.overlay {
							if bar.isBiggest {
								RoundedRectangle(cornerRadius: 4).strokeBorder(colors.ink, lineWidth: 2)
							}
						}
						.frame(width: width, height: height)
						.offset(x: x, y: top)
					if let value = bar.value {
						Text(value)
							.font(.system(size: 12, weight: .bold, design: .rounded))
							.monospacedDigit()
							.fixedSize()
							.frame(width: width)
							.offset(x: x, y: top - Self.valueHeight + 2)
					}
					Text(bar.letter)
						.font(.text(.caption, weight: bar.isBiggest ? .bold : .regular))
						.opacity(bar.isBiggest ? 1 : 0.75)
						.frame(width: width)
						.offset(x: x, y: proxy.size.height - Self.labelHeight + 4)
				}
			}
		}
		.accessibilityElement(children: .ignore)
		.accessibilityLabel(bars.map(\.spoken).joined(separator: "; "))
		.accessibilityIdentifier("wrapped.bars")
	}
}

private extension Color {
	init(rgba: RGBA) {
		self.init(.sRGB, red: rgba.red, green: rgba.green, blue: rgba.blue, opacity: rgba.alpha)
	}
}
