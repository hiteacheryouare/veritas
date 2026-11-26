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
			Text(format.amount(amount, on: .budget))
				.font(.figures(.body, weight: .semibold))
				.rowFigureChange(amount)
			Text(word ?? tone.word ?? "")
				.font(.text(.caption))
				.opacity(0.85)
				.frame(width: 26, alignment: .leading)
		}
		.foregroundStyle(Color(tone.text))
		.accessibilityElement(children: .combine)
	}
}

extension EnvironmentValues {
	/// The month on screen. A drain bar never animates when it changes, so a
	/// bar doesn't appear to drain from one month into the next.
	@Entry var shownMonth: Month? = nil
	/// The month on screen when the budget has more envelopes than
	/// `Motion.animatedRowLimit`, otherwise nil. Row figures don't cross-fade
	/// when it changes, so a month change on a large budget is one swap.
	@Entry var largeBudgetMonth: Month? = nil
}

extension View {
	/// A row's money figure: it cross-fades after a commit, and on a month
	/// change only when the budget is small enough to animate every row.
	func rowFigureChange<Value: Equatable>(_ value: Value) -> some View {
		modifier(RowFigureChange(value: value))
	}
}

private struct RowFigureChange<Value: Equatable>: ViewModifier {
	let value: Value
	@Environment(\.largeBudgetMonth) private var largeBudgetMonth

	func body(content: Content) -> some View {
		content
			.valueChange(value)
			.still(whenChanging: largeBudgetMonth)
	}
}

/// What's left of the envelope, draining as it's spent. Overspent is a red
/// hatched bar at full width with no fill: the hatch is the non-color signal.
struct DrainBar: View {
	let fill: Double
	let tone: VeritasDesign.MoneyTone
	@Environment(\.shownMonth) private var shownMonth

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
		.motion(.fast, value: fill)
		.still(whenChanging: shownMonth)
		.accessibilityHidden(true)
	}
}
