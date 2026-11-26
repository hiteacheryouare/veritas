import Foundation
import AppKit
import SwiftUI
import Testing
import VeritasDesign

@MainActor
struct TokenBehaviorTests {
	@Test func aColorTokenResolvesToItsValueForEachAppearance() throws {
		let color = NSColor(Color(ColorToken.moneyOverspent))
		#expect(try components(of: color, in: .aqua) == [0xc4, 0x26, 0x2e, 255])
		#expect(try components(of: color, in: .darkAqua) == [0xff, 0x6a, 0x64, 255])
	}

	@Test func translucentTokensKeepTheirAlpha() throws {
		let color = NSColor(Color(ColorToken.accentSoft))
		#expect(try components(of: color, in: .aqua) == [40, 42, 52, 23])
	}

	@Test func theAccentIsGraphiteNotAnyMoneyColor() {
		let money: [ColorToken] = [.moneyAvailable, .moneyLow, .moneyOverspent, .moneyIncome]
		#expect(!money.contains(.accent))
		#expect(ColorToken.accent == ColorToken(light: RGBA(0x3b3d45), dark: RGBA(0xd9dae0)))
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

	/// design/motion.md: a staged entrance ends within `stage`, and with
	/// Reduce Motion nothing is staged.
	@Test func aStagedEntranceEndsWithinItsBudget() {
		let last = Motion.staged(Motion.stageSteps + 10)
		#expect(last.curve.delay + last.duration <= Motion.Duration.stage)
		#expect(Motion.staged(0).curve.delay == 0)
		#expect(Motion.staged(Motion.stageSteps).curve.delay == last.curve.delay, "steps stop at the last one")
		#expect(Motion.staged(5).animation(reduceMotion: true) == nil)
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

	/// WCAG AA: every text token reads at 4.5:1 or better on every surface it sits on,
	/// in both appearances. The selected sidebar row's text is primary, at 7:1 or better.
	@Test func everyTextTokenMeetsAAOnEverySurface() {
		let texts: [ColorToken] = [.textPrimary, .textSecondary, .textTertiary, .moneyNeutral, .moneyAvailable, .moneyLow, .moneyOverspent, .moneyIncome]
		let opaque: [ColorToken] = [.surfaceWindow, .surfaceGrouped, .surfaceRaised, .surfaceField]
		for dark in [false, true] {
			let sidebar = Self.over(dark ? ColorToken.surfaceSidebar.dark : ColorToken.surfaceSidebar.light, dark ? ColorToken.surfaceWindow.dark : RGBA(0xffffff))
			let selected = Self.over(dark ? ColorToken.surfaceSelected.dark : ColorToken.surfaceSelected.light, sidebar)
			for text in texts {
				for surface in opaque.map({ dark ? $0.dark : $0.light }) {
					let ratio = Self.contrast(dark ? text.dark : text.light, surface)
					#expect(ratio >= 4.5, "\(text) on \(surface), dark \(dark): \(ratio)")
				}
			}
			for text in [ColorToken.textPrimary, .textSecondary, .textTertiary] {
				let ratio = Self.contrast(dark ? text.dark : text.light, sidebar)
				#expect(ratio >= 4.5, "\(text) on the sidebar, dark \(dark): \(ratio)")
			}
			#expect(Self.contrast(dark ? ColorToken.textPrimary.dark : ColorToken.textPrimary.light, selected) >= 7, "the selected row's text")
		}
	}

	private static func over(_ top: RGBA, _ bottom: RGBA) -> RGBA {
		func mix(_ a: Double, _ b: Double) -> Double { a * top.alpha + b * (1 - top.alpha) }
		let hex = [mix(top.red, bottom.red), mix(top.green, bottom.green), mix(top.blue, bottom.blue)].map { UInt32(($0 * 255).rounded()) }
		return RGBA(hex[0] << 16 | hex[1] << 8 | hex[2])
	}

	private static func contrast(_ a: RGBA, _ b: RGBA) -> Double {
		func channel(_ c: Double) -> Double { c <= 0.03928 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4) }
		func luminance(_ c: RGBA) -> Double { 0.2126 * channel(c.red) + 0.7152 * channel(c.green) + 0.0722 * channel(c.blue) }
		let (high, low) = (max(luminance(a), luminance(b)), min(luminance(a), luminance(b)))
		return (high + 0.05) / (low + 0.05)
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
