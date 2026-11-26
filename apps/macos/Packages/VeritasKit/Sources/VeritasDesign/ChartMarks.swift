import SwiftUI

/// Ryan's colors fill chart marks exactly, but mint and yellow sit near 1.7:1
/// on light surfaces, and the red and the secondary near 3:1 on dark ones. A mark
/// that is the information itself needs 3:1, so each brand fill has an edge: its
/// same-hue text shade. Marks that can take a stroke keep the fill and draw the
/// edge; marks that can't (lines, Swift Charts series) use `chartMark`, which is
/// the fill wherever it already reaches 3:1 and the edge where it doesn't.
public extension ColorToken {
	/// The surfaces charts are drawn on.
	static let chartSurfaces: [ColorToken] = [.surfaceWindow, .surfaceGrouped]

	/// Tokens compare by value, so chart-savings shares money-available-fill's entry
	/// and brand-secondary-fill shares money-income-fill's.
	static let chartEdges: [(fill: ColorToken, edge: ColorToken)] = [
		(.moneyAvailableFill, .moneyAvailable),
		(.moneyLowFill, .moneyLow),
		(.moneyOverspentFill, .moneyOverspent),
		(.moneyIncomeFill, .moneyIncome)
	]

	var chartEdge: ColorToken { Self.chartEdges.first { $0.fill == self }?.edge ?? self }

	var chartMark: ColorToken {
		func pick(_ fill: RGBA, _ edge: RGBA, dark: Bool) -> RGBA {
			let reaches = Self.chartSurfaces.allSatisfy { Contrast.ratio(fill, dark ? $0.dark : $0.light) >= 3 }
			return reaches ? fill : edge
		}
		return ColorToken(light: pick(light, chartEdge.light, dark: false), dark: pick(dark, chartEdge.dark, dark: true))
	}
}

public extension Color {
	static func chartMark(_ token: ColorToken) -> Color { Color(token.chartMark) }
	static func chartEdge(_ token: ColorToken) -> Color { Color(token.chartEdge) }
}

/// WCAG relative-luminance contrast, and alpha compositing onto an opaque color.
public enum Contrast {
	public static func over(_ top: RGBA, _ bottom: RGBA) -> RGBA {
		func mix(_ a: Double, _ b: Double) -> Double { a * top.alpha + b * (1 - top.alpha) }
		let hex = [mix(top.red, bottom.red), mix(top.green, bottom.green), mix(top.blue, bottom.blue)].map { UInt32(($0 * 255).rounded()) }
		return RGBA(hex[0] << 16 | hex[1] << 8 | hex[2])
	}

	public static func ratio(_ a: RGBA, _ b: RGBA) -> Double {
		func channel(_ c: Double) -> Double { c <= 0.04045 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4) }
		func luminance(_ c: RGBA) -> Double { 0.2126 * channel(c.red) + 0.7152 * channel(c.green) + 0.0722 * channel(c.blue) }
		let (high, low) = (max(luminance(a), luminance(b)), min(luminance(a), luminance(b)))
		return (high + 0.05) / (low + 0.05)
	}
}
