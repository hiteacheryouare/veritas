import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

extension VeritasCore.MoneyTone {
	var design: VeritasDesign.MoneyTone {
		switch self {
		case .available: .available
		case .low: .low
		case .overspent: .overspent
		case .income: .income
		case .neutral: .neutral
		}
	}
}

/// An amount with its state's colour, icon and word: color is never the only
/// signal (design/tokens.md, money states).
struct AvailableLabel: View {
	let amount: Int64
	let tone: VeritasDesign.MoneyTone
	let format: MoneyFormat
	var word: String?

	var body: some View {
		HStack(spacing: 4) {
			if let symbol = tone.symbol {
				Image(systemName: symbol)
					.imageScale(.small)
					.font(.system(size: 10, weight: .semibold))
					.opacity(0.8)
					.accessibilityHidden(true)
			}
			Text(format.amount(amount))
				.font(.figures(.body, weight: .semibold))
			Text(word ?? tone.word ?? "")
				.font(.text(.caption))
				.opacity(0.85)
				.frame(width: 26, alignment: .leading)
		}
		.foregroundStyle(Color(tone.text))
		.accessibilityElement(children: .combine)
	}
}

/// What's left of the envelope, draining as it's spent. Overspent is a red
/// hatched bar at full width with no fill: the hatch is the non-color signal.
struct DrainBar: View {
	let fill: Double
	let tone: VeritasDesign.MoneyTone
	@Environment(\.accessibilityReduceMotion) private var reduceMotion

	var body: some View {
		GeometryReader { proxy in
			ZStack(alignment: .leading) {
				if tone == .overspent {
					Canvas { context, size in
						var stripes = Path()
						var x = -size.height
						while x < size.width {
							stripes.move(to: CGPoint(x: x, y: size.height))
							stripes.addLine(to: CGPoint(x: x + size.height, y: 0))
							x += 6
						}
						context.stroke(stripes, with: .color(Color(ColorToken.moneyOverspentFill)), lineWidth: 3)
					}
					.clipShape(.rect(cornerRadius: Radius.bar))
					.overlay(RoundedRectangle(cornerRadius: Radius.bar).strokeBorder(Color(ColorToken.moneyOverspentFill)))
				} else {
					Capsule().fill(Color.track)
					Capsule()
						.fill(Color(tone == .low ? ColorToken.moneyLowFill : ColorToken.moneyAvailableFill))
						.frame(width: proxy.size.width * fill)
				}
			}
		}
		.frame(height: Size.bar)
		.clipShape(.rect(cornerRadius: Radius.bar))
		.animation(Motion.fast.animation(reduceMotion: reduceMotion), value: fill)
		.accessibilityHidden(true)
	}
}
