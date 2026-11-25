import SwiftUI
import VeritasDesign
import VeritasKit

/// The tour's coach mark over a screen. It waits briefly for its target to
/// appear as the screen loads, then shows unanchored at the bottom if the
/// target never does, or if the user has gone to another screen.
struct TourOverlay: View {
	let tour: GuidedTour
	let anchors: [TourStep: TourTarget]
	let isOnStepScreen: Bool
	@State private var cardHeight: CGFloat = 0
	@State private var isTargetLate = false
	@Environment(\.accessibilityReduceMotion) private var reduceMotion

	var body: some View {
		GeometryReader { proxy in
			if let step = tour.current, let number = tour.number {
				let anchor = isOnStepScreen ? anchors[step] : nil
				let target = anchor.map { proxy[$0.bounds] }
				if target != nil || isTargetLate || !isOnStepScreen {
					let placement = CoachMarkPlacement.place(
						CGSize(width: CoachMark.width, height: cardHeight),
						pointingAt: target,
						in: proxy.size
					)
					if let target, anchor?.isRinged == true, placement.side != .free {
						TargetRing(target: target)
					}
					CoachMark(
						step: step, number: number, count: tour.steps.count,
						isLastStep: tour.isLastStep, canGoBack: tour.canGoBack,
						side: placement.side, arrowX: placement.arrowX,
						back: tour.back, next: tour.next, skip: tour.skip
					)
					.onGeometryChange(for: CGFloat.self) { $0.size.height } action: { cardHeight = $0 }
					.position(x: placement.frame.midX, y: placement.frame.midY)
					.opacity(cardHeight > 0 ? 1 : 0)
					.transition(.opacity)
				}
			}
		}
		.animation(Motion.fast.animation(reduceMotion: reduceMotion), value: tour.current)
		.task(id: tour.current) {
			isTargetLate = false
			try? await Task.sleep(for: .milliseconds(600))
			isTargetLate = true
		}
	}
}

private struct TargetRing: View {
	let target: CGRect

	var body: some View {
		RoundedRectangle(cornerRadius: Radius.control)
			.stroke(.focusRing, lineWidth: 2)
			.frame(width: target.width + 8, height: target.height + 8)
			.position(x: target.midX, y: target.midY)
			.allowsHitTesting(false)
			.accessibilityHidden(true)
	}
}

/// Where the card goes: below its target if it fits, else above, else
/// unanchored at the bottom. Always inside the container's margins.
struct CoachMarkPlacement: Equatable {
	enum Side: Equatable {
		case below
		case above
		case free
	}

	static let margin = Spacing.s4
	static let gap: CGFloat = 6

	let frame: CGRect
	let side: Side
	/// In the card's own coordinates.
	let arrowX: CGFloat

	static func place(_ size: CGSize, pointingAt target: CGRect?, in container: CGSize) -> CoachMarkPlacement {
		let bottomY = max(margin, container.height - margin - size.height)
		guard let target, target.intersects(CGRect(origin: .zero, size: container)) else {
			let frame = CGRect(x: max(margin, (container.width - size.width) / 2), y: bottomY, width: size.width, height: size.height)
			return CoachMarkPlacement(frame: frame, side: .free, arrowX: size.width / 2)
		}
		let x = min(max(target.midX - size.width / 2, margin), max(margin, container.width - margin - size.width))
		let side: Side
		let y: CGFloat
		if target.maxY + gap + size.height <= container.height - margin {
			(side, y) = (.below, target.maxY + gap)
		} else if target.minY - gap - size.height >= margin {
			(side, y) = (.above, target.minY - gap - size.height)
		} else {
			(side, y) = (.free, bottomY)
		}
		let inset = CoachMarkShape.arrowWidth / 2 + Radius.section
		let arrowX = min(max(target.midX - x, inset), size.width - inset)
		return CoachMarkPlacement(frame: CGRect(x: x, y: y, width: size.width, height: size.height), side: side, arrowX: arrowX)
	}
}

/// One step: its sentences, "2 of 5", and Skip, Back and Next. While the
/// card has focus, Return is Next, Esc is Skip and ⌘[ is Back. VoiceOver
/// reads the card once, by its label, when focus moves to it.
struct CoachMark: View {
	static let width: CGFloat = 320

	let step: TourStep
	let number: Int
	let count: Int
	let isLastStep: Bool
	let canGoBack: Bool
	var side = CoachMarkPlacement.Side.free
	var arrowX = CoachMark.width / 2
	let back: () -> Void
	let next: () -> Void
	let skip: () -> Void
	@FocusState private var isFocused: Bool
	@AccessibilityFocusState private var isVoiceOverFocused: Bool

	private var spoken: String { TourWords.spoken(step, number: number, of: count) }

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s3) {
			Text(TourWords.text(step))
				.font(.text(.body))
				.foregroundStyle(.textPrimary)
				.fixedSize(horizontal: false, vertical: true)
				.accessibilityHidden(true)
			HStack(spacing: Spacing.s2) {
				Text(TourWords.position(number, of: count))
					.font(.text(.caption))
					.foregroundStyle(.textSecondary)
					.monospacedDigit()
					.accessibilityIdentifier("tour.position")
				Spacer(minLength: Spacing.s2)
				Button(TourWords.skip, action: skip)
					.buttonStyle(.borderless)
					.help("Skip the tour (Esc)")
					.accessibilityIdentifier("tour.skip")
				Button(TourWords.back, action: back)
					.disabled(!canGoBack)
					.help("Back (⌘[)")
					.accessibilityIdentifier("tour.back")
				Button(TourWords.next(isLastStep: isLastStep), action: next)
					.buttonStyle(.borderedProminent)
					.help("\(TourWords.next(isLastStep: isLastStep)) (Return)")
					.accessibilityIdentifier("tour.next")
			}
		}
		.padding(Spacing.s4)
		.padding(side == .above ? .bottom : .top, CoachMarkShape.arrowHeight)
		.frame(width: Self.width, alignment: .leading)
		.fixedSize(horizontal: false, vertical: true)
		.background {
			let shape = CoachMarkShape(side: side, arrowX: arrowX)
			shape.fill(.surfaceRaised)
				.overlay { shape.stroke(.edgeControl, lineWidth: 1) }
				.shadow(color: .black.opacity(0.18), radius: 12, y: 4)
		}
		.focusable()
		.focusEffectDisabled()
		.focused($isFocused)
		.onKeyPress(.return) {
			next()
			return .handled
		}
		.onKeyPress(.escape) {
			skip()
			return .handled
		}
		.onKeyPress(keys: ["["]) { press in
			guard press.modifiers.contains(.command), canGoBack else { return .ignored }
			back()
			return .handled
		}
		.accessibilityElement(children: .contain)
		.accessibilityLabel(spoken)
		.accessibilityFocused($isVoiceOverFocused)
		.accessibilityIdentifier("tour.card")
		.onAppear(perform: takeFocus)
		.onChange(of: step) { takeFocus() }
	}

	private func takeFocus() {
		// The screen under the card claims focus as it appears; take it after.
		Task { @MainActor in
			isFocused = true
			isVoiceOverFocused = true
		}
	}
}

/// A rounded card with an arrow on the edge facing its target. The arrow's
/// strip is always part of the frame, so the card's size never depends on its side.
struct CoachMarkShape: Shape {
	static let arrowHeight: CGFloat = 8
	static let arrowWidth: CGFloat = 18

	let side: CoachMarkPlacement.Side
	let arrowX: CGFloat

	func path(in rect: CGRect) -> Path {
		var body = rect
		body.size.height -= Self.arrowHeight
		if side != .above {
			body.origin.y += Self.arrowHeight
		}
		let card = Path(roundedRect: body, cornerRadius: Radius.section)
		guard side != .free else { return card }
		let baseY = side == .below ? body.minY + 1 : body.maxY - 1
		let tipY = side == .below ? rect.minY : rect.maxY
		var arrow = Path()
		arrow.move(to: CGPoint(x: arrowX - Self.arrowWidth / 2, y: baseY))
		arrow.addLine(to: CGPoint(x: arrowX, y: tipY))
		arrow.addLine(to: CGPoint(x: arrowX + Self.arrowWidth / 2, y: baseY))
		arrow.closeSubpath()
		return card.union(arrow)
	}
}
