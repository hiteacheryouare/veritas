import Foundation
import AppKit
import SwiftUI
import Testing
import VeritasDesign

@MainActor
struct TokenBehaviorTests {
	@Test func aColorTokenResolvesToItsValueForEachAppearance() throws {
		let color = NSColor(Color(ColorToken.moneyOverspent))
		#expect(try components(of: color, in: .aqua) == [0xc3, 0x02, 0x25, 255])
		#expect(try components(of: color, in: .darkAqua) == [0xff, 0x90, 0x8a, 255])
	}

	@Test func translucentTokensKeepTheirAlpha() throws {
		let color = NSColor(Color(ColorToken.accentSoft))
		#expect(try components(of: color, in: .aqua) == [35, 22, 156, 26])
	}

	@Test func theAccentIsRyansPrimaryNotAnyMoneyColor() {
		let money: [ColorToken] = [.moneyAvailable, .moneyLow, .moneyOverspent, .moneyIncome]
		#expect(!money.contains(.accent))
		#expect(ColorToken.accent.light == RGBA(0x23169c))
		#expect(ColorToken.brand == ColorToken(light: RGBA(0x23169c), dark: RGBA(0x23169c)))
	}

	/// Fills, bars, dots and chart marks use Ryan's brand values exactly (brand/colors.txt).
	@Test func moneyFillsAreTheBrandColorsInBothAppearances() {
		let expected: [(ColorToken, UInt32)] = [
			(.moneyAvailableFill, 0x56e39f), (.moneyLowFill, 0xf5b700), (.moneyOverspentFill, 0xdb162f),
			(.moneyIncomeFill, 0x007ea7), (.brandSecondaryFill, 0x007ea7), (.chartSavings, 0x56e39f)
		]
		for (token, hex) in expected {
			#expect(token.light == RGBA(hex) && token.dark == RGBA(hex), "\(token)")
		}
	}

	@Test func reduceMotionTurnsEveryDurationIntoNone() {
		for motion in [Motion.fast, .sheet] {
			#expect(motion.animation(reduceMotion: true) == nil)
			#expect(motion.animation(.exit, reduceMotion: true) == nil)
			#expect(motion.duration(reduceMotion: true) == 0)
			#expect(motion.animation(reduceMotion: false) != nil)
		}
		#expect(Motion.fast.duration(reduceMotion: false) == 0.15)
		#expect(Motion.sheet.duration(reduceMotion: false) == 0.22)
	}

	@Test func everyMoneyStateButZeroHasAnIconThatExists() {
		for tone in MoneyTone.allCases where tone != .neutral {
			let symbol = tone.symbol
			#expect(symbol != nil, "\(tone)")
			#expect(symbol.flatMap { NSImage(systemSymbolName: $0, accessibilityDescription: nil) } != nil, "\(tone)")
		}
		#expect(MoneyTone.neutral.symbol == nil)
		#expect(MoneyTone.neutral.word == nil)
	}

	@Test func moneyStatesUseTheWordsFromTheTokens() {
		#expect(MoneyTone.available.word == "left")
		#expect(MoneyTone.low.word == "low")
		#expect(MoneyTone.overspent.word == "over")
		#expect(MoneyTone.overspent.text == .moneyOverspent)
		#expect(MoneyTone.overspent.fill == .moneyOverspentFill)
	}

	private func components(of color: NSColor, in name: NSAppearance.Name) throws -> [Int] {
		let appearance = try #require(NSAppearance(named: name))
		var result: [Int] = []
		appearance.performAsCurrentDrawingAppearance {
			if let srgb = color.usingColorSpace(.sRGB) {
				result = [srgb.redComponent, srgb.greenComponent, srgb.blueComponent, srgb.alphaComponent]
					.map { Int(($0 * 255).rounded()) }
			}
		}
		return result
	}
}
