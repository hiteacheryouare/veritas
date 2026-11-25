import SwiftUI
import VeritasKit

/// What a tour step points at. A target standing in for something the
/// overlay can't reach, such as a toolbar button, gets no ring.
struct TourTarget {
	let bounds: Anchor<CGRect>
	let isRinged: Bool
}

struct TourAnchorKey: PreferenceKey {
	static var defaultValue: [TourStep: TourTarget] { [:] }

	static func reduce(value: inout [TourStep: TourTarget], nextValue: () -> [TourStep: TourTarget]) {
		value.merge(nextValue()) { $1 }
	}
}

extension View {
	func tourAnchor(_ step: TourStep, isOn: Bool = true, ringed: Bool = true) -> some View {
		anchorPreference(key: TourAnchorKey.self, value: .bounds) { isOn ? [step: TourTarget(bounds: $0, isRinged: ringed)] : [:] }
	}

	/// Draws the tour's coach mark over this view, pointing at an anchor inside it.
	func tourOverlay(_ tour: GuidedTour, isOnStepScreen: Bool = true) -> some View {
		overlayPreferenceValue(TourAnchorKey.self) { anchors in
			TourOverlay(tour: tour, anchors: anchors, isOnStepScreen: isOnStepScreen)
		}
	}
}

extension FocusedValues {
	@Entry var tour: GuidedTour?
}
