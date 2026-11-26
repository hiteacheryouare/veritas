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
			let window = ColorToken.surfaceWindow.value(dark: dark)
			let sidebar = Contrast.over(ColorToken.surfaceSidebar.value(dark: dark), dark ? window : RGBA(0xffffff))
			for (textName, text) in [("text-primary", ColorToken.textPrimary), ("text-secondary", .textSecondary), ("text-tertiary", .textTertiary), ("accent", .accent)] {
				pairs.append(Pair(name: "\(textName) on the sidebar, \(mode)", foreground: text.value(dark: dark), background: sidebar, use: .text))
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

	@Test func theSelectedSidebarRowKeepsPrimaryTextAt7To1() {
		for dark in [false, true] {
			let window = ColorToken.surfaceWindow.value(dark: dark)
			let sidebar = Contrast.over(ColorToken.surfaceSidebar.value(dark: dark), dark ? window : RGBA(0xffffff))
			let selected = Contrast.over(ColorToken.surfaceSelected.value(dark: dark), sidebar)
			#expect(Contrast.ratio(ColorToken.textPrimary.value(dark: dark), selected) >= 7, "dark \(dark)")
		}
	}

	@Test func theCheckCatchesAPairBelowItsThreshold() {
		let mint = Pair(name: "good fill as text", foreground: ColorToken.moneyAvailableFill.light, background: ColorToken.surfaceGrouped.light, use: .text)
		#expect(mint.ratio < Use.text.rawValue)
	}
}

enum Contrast {
	static func over(_ top: RGBA, _ bottom: RGBA) -> RGBA {
		func mix(_ a: Double, _ b: Double) -> Double { a * top.alpha + b * (1 - top.alpha) }
		let hex = [mix(top.red, bottom.red), mix(top.green, bottom.green), mix(top.blue, bottom.blue)].map { UInt32(($0 * 255).rounded()) }
		return RGBA(hex[0] << 16 | hex[1] << 8 | hex[2])
	}

	static func ratio(_ a: RGBA, _ b: RGBA) -> Double {
		func channel(_ c: Double) -> Double { c <= 0.04045 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4) }
		func luminance(_ c: RGBA) -> Double { 0.2126 * channel(c.red) + 0.7152 * channel(c.green) + 0.0722 * channel(c.blue) }
		let (high, low) = (max(luminance(a), luminance(b)), min(luminance(a), luminance(b)))
		return (high + 0.05) / (low + 0.05)
	}
}

extension ColorToken {
	func value(dark: Bool) -> RGBA { dark ? self.dark : light }
}
