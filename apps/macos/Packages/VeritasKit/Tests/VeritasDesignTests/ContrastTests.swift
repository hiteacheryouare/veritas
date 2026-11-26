import Foundation
import Testing
import VeritasDesign

/// WCAG AA for every token pair the app draws: 4.5:1 for text and small icons,
/// 3:1 for focus rings, selection marks and other UI. Translucent surfaces are
/// composited over what they sit on, and rows that tint with the accent when
/// selected count as surfaces of their own.
struct ContrastTests {
	enum Use: Double {
		case text = 4.5
		case ui = 3
	}

	struct Pair: CustomStringConvertible {
		let name: String
		let foreground: RGBA
		let background: RGBA
		let use: Use
		var ratio: Double { Contrast.ratio(foreground, background) }
		var description: String { "\(name): \(String(format: "%.2f", ratio)) (needs \(use.rawValue))" }
	}

	static let opaqueSurfaces: [(String, ColorToken)] = [
		("window", .surfaceWindow), ("grouped", .surfaceGrouped), ("raised", .surfaceRaised), ("field", .surfaceField)
	]

	static let textTokens: [(String, ColorToken)] = [
		("text-primary", .textPrimary), ("text-secondary", .textSecondary), ("text-tertiary", .textTertiary),
		("money-neutral", .moneyNeutral), ("money-available", .moneyAvailable), ("money-low", .moneyLow),
		("money-overspent", .moneyOverspent), ("money-income", .moneyIncome), ("brand-secondary", .brandSecondary), ("accent", .accent)
	]

	static func surfaces(dark: Bool) -> [(String, RGBA)] {
		var result: [(String, RGBA)] = []
		for (name, token) in opaqueSurfaces {
			let surface = token.value(dark: dark)
			result.append((name, surface))
			result.append(("\(name) selected", Contrast.over(ColorToken.accentSoft.value(dark: dark), surface)))
			result.append(("\(name) hover", Contrast.over(ColorToken.surfaceHover.value(dark: dark), surface)))
		}
		return result
	}

	static func pairs() -> [Pair] {
		var pairs: [Pair] = []
		for dark in [false, true] {
			let mode = dark ? "dark" : "light"
			for (textName, text) in textTokens {
				for (surfaceName, surface) in surfaces(dark: dark) {
					pairs.append(Pair(name: "\(textName) on \(surfaceName), \(mode)", foreground: text.value(dark: dark), background: surface, use: .text))
				}
			}
			for (surfaceName, token) in opaqueSurfaces {
				let surface = token.value(dark: dark)
				pairs.append(Pair(name: "focus-ring on \(surfaceName), \(mode)", foreground: ColorToken.focusRing.value(dark: dark), background: surface, use: .ui))
				pairs.append(Pair(name: "accent-fill on \(surfaceName), \(mode)", foreground: ColorToken.accentFill.value(dark: dark), background: surface, use: .ui))
			}
			let sidebar = Self.sidebar(dark: dark)
			let sidebarSelected = Contrast.over(ColorToken.surfaceSelected.value(dark: dark), sidebar)
			for (textName, text) in textTokens {
				pairs.append(Pair(name: "\(textName) on the sidebar, \(mode)", foreground: text.value(dark: dark), background: sidebar, use: .text))
				pairs.append(Pair(name: "\(textName) on the selected sidebar row, \(mode)", foreground: text.value(dark: dark), background: sidebarSelected, use: .text))
			}
			// RuleApplySheet tints a row that changes a reconciled balance with money-low at 8%.
			for (surfaceName, token) in [("raised", ColorToken.surfaceRaised), ("window", .surfaceWindow)] {
				let tinted = Contrast.over(Self.tint(ColorToken.moneyLow.value(dark: dark), 0.08), token.value(dark: dark))
				for (textName, text) in textTokens {
					pairs.append(Pair(name: "\(textName) on a reconciled-warning row over \(surfaceName), \(mode)", foreground: text.value(dark: dark), background: tinted, use: .text))
				}
			}
			for fill in ColorToken.chartEdges.map(\.fill) + [.chartSavings, .brandSecondaryFill] {
				for surface in ColorToken.chartSurfaces {
					pairs.append(Pair(name: "chart edge of \(fill) on \(surface), \(mode)", foreground: fill.chartEdge.value(dark: dark), background: surface.value(dark: dark), use: .ui))
					pairs.append(Pair(name: "chart mark of \(fill) on \(surface), \(mode)", foreground: fill.chartMark.value(dark: dark), background: surface.value(dark: dark), use: .ui))
				}
			}
			pairs.append(Pair(name: "text-on-accent on accent-fill, \(mode)", foreground: ColorToken.textOnAccent.value(dark: dark), background: ColorToken.accentFill.value(dark: dark), use: .text))
			pairs.append(Pair(name: "text-on-brand on brand, \(mode)", foreground: ColorToken.textOnBrand.value(dark: dark), background: ColorToken.brand.value(dark: dark), use: .text))
		}
		for paper in [ColorToken.surfacePaper.light, ColorToken.surfacePaper.dark] {
			pairs.append(Pair(name: "money-overspent (light) on paper", foreground: ColorToken.moneyOverspent.light, background: paper, use: .text))
		}
		pairs.append(Pair(name: "money-available-fill on brand (Wrapped's highlight bar)", foreground: ColorToken.moneyAvailableFill.light, background: ColorToken.brand.light, use: .ui))
		return pairs
	}

	@Test func everyTokenPairMeetsItsThreshold() {
		for pair in Self.pairs() {
			#expect(pair.ratio >= pair.use.rawValue, "\(pair)")
		}
	}

	static func tint(_ color: RGBA, _ alpha: Double) -> RGBA {
		let channels = [color.red, color.green, color.blue].map { UInt32(($0 * 255).rounded()) }
		return RGBA(channels[0] << 16 | channels[1] << 8 | channels[2], alpha: alpha)
	}

	static func sidebar(dark: Bool) -> RGBA {
		Contrast.over(ColorToken.surfaceSidebar.value(dark: dark), dark ? ColorToken.surfaceWindow.dark : RGBA(0xffffff))
	}

	@Test func theSelectedSidebarRowKeepsPrimaryTextAt7To1() {
		for dark in [false, true] {
			let sidebar = Self.sidebar(dark: dark)
			let selected = Contrast.over(ColorToken.surfaceSelected.value(dark: dark), sidebar)
			#expect(Contrast.ratio(ColorToken.textPrimary.value(dark: dark), selected) >= 7, "dark \(dark)")
		}
	}

	/// Chart marks keep Ryan's exact fill wherever it reaches 3:1 and fall back to the edge only where it doesn't.
	@Test func chartMarksKeepTheBrandFillWhereItReaches3To1() {
		#expect(ColorToken.moneyIncomeFill.chartMark.light == RGBA(0x007ea7))
		#expect(ColorToken.moneyAvailableFill.chartMark.dark == RGBA(0x56e39f))
		#expect(ColorToken.moneyLowFill.chartMark.dark == RGBA(0xf5b700))
		#expect(ColorToken.moneyAvailableFill.chartMark.light == ColorToken.moneyAvailable.light)
		#expect(ColorToken.chartExpense.chartEdge == .chartExpense)
	}

	@Test func theCheckCatchesAPairBelowItsThreshold() {
		let mint = Pair(name: "good fill as text", foreground: ColorToken.moneyAvailableFill.light, background: ColorToken.surfaceGrouped.light, use: .text)
		#expect(mint.ratio < Use.text.rawValue)
	}
}

extension ColorToken {
	func value(dark: Bool) -> RGBA { dark ? self.dark : light }
}
