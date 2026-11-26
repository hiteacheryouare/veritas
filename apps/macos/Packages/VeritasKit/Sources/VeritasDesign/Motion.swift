import Foundation
import SwiftUI

/// A named motion (design/motion.md): how a change animates normally, and how
/// it animates with Reduce Motion on. Screens name one of these and never
/// write a duration, curve or spring of their own.
public struct Motion: Hashable, Sendable {
	public enum Curve: Hashable, Sendable {
		case timing(Easing, duration: Double, delay: Double = 0)
		/// A spring with a perceptual duration. Every spring in Veritas has a
		/// bounce of 0: an overshooting bar would show an amount that isn't real.
		case spring(duration: Double, bounce: Double)

		public var duration: Double {
			switch self {
			case .timing(_, let duration, _): duration
			case .spring(let duration, _): duration
			}
		}

		public var delay: Double {
			if case .timing(_, _, let delay) = self { delay } else { 0 }
		}

		public var isSpring: Bool {
			if case .spring = self { true } else { false }
		}

		func animation(overriding easing: Easing?, scale: Double) -> Animation {
			switch self {
			case .timing(let own, let duration, let delay):
				let curve = (easing ?? own).controlPoints
				return .timingCurve(curve.x1, curve.y1, curve.x2, curve.y2, duration: duration * scale).delay(delay * scale)
			case .spring(let duration, let bounce):
				return .spring(duration: duration * scale, bounce: bounce)
			}
		}
	}

	public let name: String
	public let curve: Curve
	/// What runs with Reduce Motion on. `nil` means the change happens at once.
	/// Never a spring and never longer than `fast`: movement becomes a short fade.
	public let reduced: Curve?

	public init(_ name: String, _ curve: Curve, reduced: Curve?) {
		self.name = name
		self.curve = curve
		self.reduced = reduced
	}

	public var duration: Double { curve.duration }

	public func duration(reduceMotion: Bool) -> Double {
		reduceMotion ? reduced?.duration ?? 0 : duration
	}

	/// How far along the change is, 0 to 1, `time` seconds after it starts,
	/// at a time scale of 1. Frame strips and tests read the curve through this.
	public func progress(at time: Double, reduceMotion: Bool = false) -> Double {
		guard let curve = reduceMotion ? reduced : curve else { return 1 }
		let elapsed = time - curve.delay
		guard elapsed > 0 else { return 0 }
		switch curve {
		case .timing(let easing, let duration, _):
			guard elapsed < duration else { return 1 }
			let points = easing.controlPoints
			let unit = UnitCurve.bezier(startControlPoint: UnitPoint(x: points.x1, y: points.y1), endControlPoint: UnitPoint(x: points.x2, y: points.y2))
			return unit.value(at: elapsed / duration)
		case .spring(let duration, let bounce):
			return Spring(duration: duration, bounce: bounce).value(target: 1.0, time: elapsed)
		}
	}

	/// `easing` replaces a timing curve's own easing, as `.exit` does for a
	/// change that takes something away. Springs keep their shape.
	public func animation(_ easing: Easing? = nil, reduceMotion: Bool) -> Animation? {
		guard let curve = reduceMotion ? reduced : curve else { return nil }
		return curve.animation(overriding: easing, scale: Self.timeScale)
	}
}

// MARK: Tokens

public extension Motion {
	/// Durations in seconds, mirrored in design/tokens.css.
	enum Duration {
		public static let instant = 0.09
		public static let fast = 0.15
		public static let sheet = 0.22
		public static let page = 0.20
		public static let pageExit = 0.10
		public static let pageDelay = 0.06
		public static let story = 0.48
		/// The whole budget for a staged entrance, every part included.
		public static let stage = 0.70
		/// How long a page waits for its content before showing a spinner.
		public static let readyGate = 0.25
		/// A command repeated sooner than this changes without animation.
		public static let rapidRepeat = 0.15
	}

	private static let fade = Curve.timing(.standard, duration: Duration.fast)

	/// Hover, press, focus.
	static let instant = Motion("instant", .timing(.standard, duration: Duration.instant), reduced: nil)
	/// Selection, drain bars, a figure cross-fading after a commit, spinners.
	static let fast = Motion("fast", .timing(.standard, duration: Duration.fast), reduced: nil)
	/// Something leaving quickly, such as a row's content before its height closes.
	static let leave = Motion("leave", .timing(.exit, duration: Duration.instant), reduced: nil)
	/// Banners, bars, a sheet's step, a question bubble.
	static let sheet = Motion("sheet", .timing(.standard, duration: Duration.sheet), reduced: fade)
	/// A page arriving. It starts once the old page is mostly gone.
	static let pageIn = Motion("pageIn", .timing(.standard, duration: Duration.page, delay: Duration.pageDelay), reduced: fade)
	/// A page leaving.
	static let pageOut = Motion("pageOut", .timing(.exit, duration: Duration.pageExit), reduced: .timing(.exit, duration: Duration.instant))
	/// Layout moves: reorder, drop, collapse, sheet height, a coach mark moving.
	static let settle = Motion("settle", .spring(duration: 0.32, bounce: 0), reduced: nil)
	/// A card growing into its page, and back.
	static let morph = Motion("morph", .spring(duration: 0.40, bounce: 0), reduced: fade)
	/// A row's page pushing in.
	static let push = Motion("push", .spring(duration: 0.32, bounce: 0), reduced: fade)
	/// Wrapped: a spread or card turning.
	static let turn = Motion("turn", .spring(duration: Duration.story, bounce: 0), reduced: fade)

	static let all: [Motion] = [.instant, .fast, .leave, .sheet, .pageIn, .pageOut, .settle, .morph, .push, .turn]

	/// A change touching more rows than this happens without animation
	/// (design/motion.md, performance): hundreds of cross-fades at once cost
	/// frames and tell the user nothing more than one would.
	static let animatedRowLimit = 20
}

// MARK: Debug time scale

public extension Motion {
	/// Every motion runs this many times slower. Debug builds read it from the
	/// `-MotionTimeScale 5` launch argument so timings can be judged by eye;
	/// release builds always use 1.
	static var timeScale: Double {
		#if DEBUG
		let scale = UserDefaults.standard.double(forKey: "MotionTimeScale")
		return scale > 0 ? scale : 1
		#else
		1
		#endif
	}
}

public enum Easing: Hashable, Sendable {
	case standard
	case exit

	public var controlPoints: (x1: Double, y1: Double, x2: Double, y2: Double) {
		switch self {
		case .standard: (0.2, 0.7, 0.2, 1)
		case .exit: (0.4, 0, 1, 1)
		}
	}
}
