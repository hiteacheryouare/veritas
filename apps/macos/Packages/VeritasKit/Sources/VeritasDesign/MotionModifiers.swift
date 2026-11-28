import AppKit
import SwiftUI

// MARK: Transitions

/// How a view comes and goes. Each side names a `Motion`, so its timing and its
/// Reduce Motion form come from the tokens. With Reduce Motion on, every offset
/// is dropped and the view fades instead.
public struct MotionTransition: Equatable, Sendable {
	public struct Side: Equatable, Sendable {
		public let motion: Motion
		public let easing: Easing?
		public let offset: CGSize
		public let fades: Bool

		public init(_ motion: Motion, easing: Easing? = nil, offset: CGSize = .zero, fades: Bool = true) {
			self.motion = motion
			self.easing = easing
			self.offset = offset
			self.fades = fades
		}

		var reduced: Side {
			Side(motion, easing: easing, offset: .zero, fades: true)
		}

		func transition(reduceMotion: Bool) -> AnyTransition {
			var transition: AnyTransition = fades ? .opacity : .identity
			if offset != .zero {
				transition = transition.combined(with: .offset(offset))
			}
			return transition.animation(motion.animation(easing, reduceMotion: reduceMotion))
		}
	}

	public let name: String
	public let insertion: Side
	public let removal: Side

	public init(_ name: String, insertion: Side, removal: Side) {
		self.name = name
		self.insertion = insertion
		self.removal = removal
	}

	public func resolved(reduceMotion: Bool) -> MotionTransition {
		reduceMotion ? MotionTransition(name, insertion: insertion.reduced, removal: removal.reduced) : self
	}

	public func anyTransition(reduceMotion: Bool) -> AnyTransition {
		let sides = resolved(reduceMotion: reduceMotion)
		return .asymmetric(
			insertion: sides.insertion.transition(reduceMotion: reduceMotion),
			removal: sides.removal.transition(reduceMotion: reduceMotion)
		)
	}
}

public extension MotionTransition {
	/// Peer screens: the old page fades out, then the new one fades in rising 4 pt.
	static let page = MotionTransition(
		"page",
		insertion: Side(.pageIn, offset: CGSize(width: 0, height: 4)),
		removal: Side(.pageOut)
	)

	/// Drilling into a row's page: it comes in from the trailing side as the list leaves.
	static let push = MotionTransition(
		"push",
		insertion: Side(.push, offset: CGSize(width: 24, height: 0)),
		removal: Side(.push, offset: CGSize(width: -24, height: 0))
	)

	/// Going back from a pushed page.
	static let pop = MotionTransition(
		"pop",
		insertion: Side(.push, offset: CGSize(width: -24, height: 0)),
		removal: Side(.push, offset: CGSize(width: 24, height: 0))
	)

	/// Banners and bars at the top of a page. Their content leaves first.
	static let reveal = MotionTransition(
		"reveal",
		insertion: Side(.sheet, offset: CGSize(width: 0, height: -6)),
		removal: Side(.leave)
	)

	/// A new bubble or row arriving from below.
	static let rise = MotionTransition(
		"rise",
		insertion: Side(.sheet, offset: CGSize(width: 0, height: 8)),
		removal: Side(.leave)
	)

	/// Content replacing a spinner, or a sheet's step replacing the last.
	static let fade = MotionTransition("fade", insertion: Side(.fast), removal: Side(.leave))

	/// Wrapped: the next card or spread.
	static let turnForward = MotionTransition(
		"turnForward",
		insertion: Side(.turn, offset: CGSize(width: 12, height: 0)),
		removal: Side(.turn, offset: CGSize(width: -12, height: 0))
	)

	/// Wrapped: the previous card or spread.
	static let turnBack = MotionTransition(
		"turnBack",
		insertion: Side(.turn, offset: CGSize(width: -12, height: 0)),
		removal: Side(.turn, offset: CGSize(width: 12, height: 0))
	)

	static let all: [MotionTransition] = [.page, .push, .pop, .reveal, .rise, .fade, .turnForward, .turnBack]
}

// MARK: Running a change

public extension Motion {
	/// Reduce Motion as the system reports it, for code outside a view.
	@MainActor static var systemReducesMotion: Bool {
		NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
	}

	/// `withAnimation` with a named motion. The only way screens start an explicit animation.
	@MainActor static func perform<Result>(_ motion: Motion, _ easing: Easing? = nil, _ body: () throws -> Result) rethrows -> Result {
		try withAnimation(motion.animation(easing, reduceMotion: systemReducesMotion), body)
	}
}

/// Remembers when a command last ran, so a held key changes without animation
/// after its first press (design/motion.md, rapid repeat).
@MainActor
public final class RapidRepeat {
	private var last: Date?

	public init() {}

	/// True when this run follows the last one by less than `Motion.Duration.rapidRepeat`.
	public func isRapid(at now: Date = .now) -> Bool {
		defer { last = now }
		guard let last else { return false }
		return now.timeIntervalSince(last) < Motion.Duration.rapidRepeat
	}
}

// MARK: View modifiers

public extension View {
	/// Animates changes to `value` with a named motion, honoring Reduce Motion.
	func motion<Value: Equatable>(_ motion: Motion, _ easing: Easing? = nil, value: Value) -> some View {
		modifier(MotionModifier(motion: motion, easing: easing, value: value))
	}

	/// Inserts and removes this view with a named transition, honoring Reduce Motion.
	func motionTransition(_ transition: MotionTransition) -> some View {
		modifier(MotionTransitionModifier(motionTransition: transition))
	}

	/// A money figure that cross-fades when a commit changes it. It never rolls
	/// through digits, so no in-between amount is ever drawn.
	func valueChange<Value: Equatable>(_ value: Value) -> some View {
		contentTransition(.opacity).motion(.fast, value: value)
	}

	/// Shows or hides this view by fading only its opacity, so nothing else
	/// that changes alongside it animates.
	func shown(_ isShown: Bool, _ motion: Motion = .fast) -> some View {
		modifier(ShownModifier(isShown: isShown, motion: motion))
	}

	/// No animation inside this view when `value` changes, whatever else
	/// changes with it, such as drain bars when the month changes.
	func still<Value: Equatable>(whenChanging value: Value) -> some View {
		transaction(value: value) { transaction in
			transaction.disablesAnimations = true
			transaction.animation = nil
		}
	}

	/// No animation inside this view at all: data reloads, live resize, the register's host.
	func withoutMotion() -> some View {
		transaction { transaction in
			transaction.disablesAnimations = true
			transaction.animation = nil
		}
	}

	/// No animation when `value` changes again within the rapid-repeat window.
	func stillOnRapidRepeat<Value: Equatable>(of value: Value) -> some View {
		modifier(RapidRepeatModifier(value: value))
	}

	/// `matchedGeometryEffect` for a card growing into its page. With Reduce
	/// Motion on, or without a namespace, there is no geometry match and the
	/// two cross-fade instead.
	func morph<ID: Hashable>(_ id: ID, in namespace: Namespace.ID?, isSource: Bool = true) -> some View {
		modifier(MorphModifier(id: id, namespace: namespace, isSource: isSource))
	}
}

private struct MotionModifier<Value: Equatable>: ViewModifier {
	let motion: Motion
	let easing: Easing?
	let value: Value
	@Environment(\.accessibilityReduceMotion) private var reduceMotion

	func body(content: Content) -> some View {
		content.animation(motion.animation(easing, reduceMotion: reduceMotion), value: value)
	}
}

private struct ShownModifier: ViewModifier {
	let isShown: Bool
	let motion: Motion
	@Environment(\.accessibilityReduceMotion) private var reduceMotion

	func body(content: Content) -> some View {
		content.animation(motion.animation(reduceMotion: reduceMotion)) { view in
			view.opacity(isShown ? 1 : 0)
		}
	}
}

private struct MotionTransitionModifier: ViewModifier {
	let motionTransition: MotionTransition
	@Environment(\.accessibilityReduceMotion) private var reduceMotion

	func body(content: Content) -> some View {
		content.transition(motionTransition.anyTransition(reduceMotion: reduceMotion))
	}
}

private struct RapidRepeatModifier<Value: Equatable>: ViewModifier {
	let value: Value
	@State private var gate = RapidRepeat()

	func body(content: Content) -> some View {
		let gate = gate
		content.transaction(value: value) { transaction in
			let isRapid = MainActor.assumeIsolated { gate.isRapid() }
			if isRapid {
				transaction.disablesAnimations = true
				transaction.animation = nil
			}
		}
	}
}

private struct MorphModifier<ID: Hashable>: ViewModifier {
	let id: ID
	let namespace: Namespace.ID?
	let isSource: Bool
	@Environment(\.accessibilityReduceMotion) private var reduceMotion

	func body(content: Content) -> some View {
		if let namespace, !reduceMotion {
			content.matchedGeometryEffect(id: id, in: namespace, isSource: isSource)
		} else {
			content
		}
	}
}
