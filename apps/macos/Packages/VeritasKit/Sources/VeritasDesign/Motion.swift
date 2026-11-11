import SwiftUI

public struct Motion: Hashable, Sendable {
	public let duration: Double

	public static let fast = Motion(duration: 0.15)
	public static let sheet = Motion(duration: 0.22)

	public func duration(reduceMotion: Bool) -> Double {
		reduceMotion ? 0 : duration
	}

	/// With Reduce Motion on there is no animation at all: values change in place.
	public func animation(_ easing: Easing = .standard, reduceMotion: Bool) -> Animation? {
		guard !reduceMotion else { return nil }
		let curve = easing.controlPoints
		return .timingCurve(curve.x1, curve.y1, curve.x2, curve.y2, duration: duration)
	}
}

public enum Easing: Sendable {
	case standard
	case exit

	public var controlPoints: (x1: Double, y1: Double, x2: Double, y2: Double) {
		switch self {
		case .standard: (0.2, 0.7, 0.2, 1)
		case .exit: (0.4, 0, 1, 1)
		}
	}
}

public extension View {
	func motion<Value: Equatable>(_ motion: Motion, _ easing: Easing = .standard, value: Value) -> some View {
		modifier(MotionModifier(motion: motion, easing: easing, value: value))
	}
}

private struct MotionModifier<Value: Equatable>: ViewModifier {
	let motion: Motion
	let easing: Easing
	let value: Value
	@Environment(\.accessibilityReduceMotion) private var reduceMotion

	func body(content: Content) -> some View {
		content.animation(motion.animation(easing, reduceMotion: reduceMotion), value: value)
	}
}
