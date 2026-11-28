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

	/// design/motion.md, Reduce Motion: movement becomes a short fade. No reduced
	/// motion springs, none outlasts `fast`, and small changes happen at once.
	@Test func reduceMotionTurnsMovementIntoAShortFade() {
		for motion in Motion.all {
			if let reduced = motion.reduced {
				#expect(!reduced.isSpring, "\(motion.name)")
				#expect(reduced.duration <= Motion.Duration.fast, "\(motion.name)")
				#expect(reduced.delay == 0, "\(motion.name)")
				#expect(motion.animation(reduceMotion: true) != nil, "\(motion.name)")
			} else {
				#expect(motion.animation(reduceMotion: true) == nil, "\(motion.name)")
				#expect(motion.duration(reduceMotion: true) == 0, "\(motion.name)")
			}
			#expect(motion.animation(reduceMotion: false) != nil, "\(motion.name)")
		}
		for motion in [Motion.instant, .fast, .leave, .settle] {
			#expect(motion.reduced == nil, "\(motion.name) happens at once")
		}
		for motion in [Motion.sheet, .pageIn, .morph, .push, .turn] {
			#expect(motion.reduced != nil, "\(motion.name) still fades")
		}
	}

	@Test func reduceMotionDropsEveryOffsetAndKeepsAFade() {
		for transition in MotionTransition.all {
			let reduced = transition.resolved(reduceMotion: true)
			for side in [reduced.insertion, reduced.removal] {
				#expect(side.offset == .zero, "\(transition.name)")
				#expect(side.fades, "\(transition.name)")
			}
			#expect(transition.resolved(reduceMotion: false) == transition)
		}
	}

	@Test func noSpringBounces() {
		for motion in Motion.all {
			if case .spring(_, let bounce) = motion.curve {
				#expect(bounce == 0, "\(motion.name): an overshoot shows an amount that isn't real")
			}
		}
	}

	/// Sampled every 5 ms, no motion ever passes its target, and each has
	/// arrived by its duration (springs within 1% by one and a half times it).
	@Test func everyMotionArrivesWithoutOvershoot() {
		for motion in Motion.all {
			var last = 0.0
			for step in 0...400 {
				let value = motion.progress(at: Double(step) * 0.005)
				#expect(value <= 1.0001, "\(motion.name) overshoots at \(step * 5) ms")
				#expect(value >= last - 0.0001, "\(motion.name) turns back at \(step * 5) ms")
				last = value
			}
			#expect(motion.progress(at: 0) == 0, "\(motion.name)")
			let settled = motion.curve.isSpring ? motion.progress(at: motion.duration * 1.5) : motion.progress(at: motion.curve.delay + motion.duration)
			#expect(settled >= 0.99, "\(motion.name) hasn't arrived: \(settled)")
		}
		#expect(Motion.pageIn.progress(at: Motion.Duration.pageDelay) == 0, "the new page waits for the old one")
	}

	@Test func exitsAreQuickerThanEntrances() {
		#expect(Motion.pageOut.duration < Motion.pageIn.duration)
		#expect(Motion.leave.duration < Motion.fast.duration)
		#expect(Motion.fast.duration == 0.15)
		#expect(Motion.sheet.duration == 0.22)
	}

	@Test func aRepeatInsideTheWindowIsRapid() {
		let gate = RapidRepeat()
		let start = Date(timeIntervalSinceReferenceDate: 0)
		#expect(!gate.isRapid(at: start), "the first press animates")
		#expect(gate.isRapid(at: start.addingTimeInterval(0.05)))
		#expect(gate.isRapid(at: start.addingTimeInterval(0.15)), "measured from the last press, 0.10 s ago")
		#expect(!gate.isRapid(at: start.addingTimeInterval(0.5)))
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
