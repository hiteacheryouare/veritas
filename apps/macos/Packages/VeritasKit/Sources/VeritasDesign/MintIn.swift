import SwiftUI

/// Ryan's "coin mint-in" for the splash. One clock (`Motion.mintIn`) runs from
/// 0 to 1 at an even pace; each part eases in over its own stretch of it:
/// the rings draw, the $ fades up, the wordmark settles beside the coin, then
/// the actions rise in. After that nothing moves.
public enum MintIn {
	public static let rings: ClosedRange<Double> = 0...0.45
	public static let dollar: ClosedRange<Double> = 0.3...0.6
	public static let wordmark: ClosedRange<Double> = 0.42...0.8
	public static let actions: ClosedRange<Double> = 0.66...1

	/// How far one part is through its stretch, eased with `Easing.standard`.
	public static func stage(_ progress: Double, _ stretch: ClosedRange<Double>) -> Double {
		let local = min(max((progress - stretch.lowerBound) / (stretch.upperBound - stretch.lowerBound), 0), 1)
		let points = Easing.standard.controlPoints
		let curve = UnitCurve.bezier(startControlPoint: UnitPoint(x: points.x1, y: points.y1), endControlPoint: UnitPoint(x: points.x2, y: points.y2))
		return curve.value(at: local)
	}
}

/// The horizontal lockup at a point in the mint-in. At `progress` 1 it draws
/// exactly what `VeritasLockup` draws.
public struct MintInLockup: View, Animatable {
	public var progress: Double
	public var capHeight: CGFloat

	public init(progress: Double, capHeight: CGFloat) {
		self.progress = progress
		self.capHeight = capHeight
	}

	public nonisolated var animatableData: Double {
		get { progress }
		set { progress = newValue }
	}

	public var body: some View {
		let unit = capHeight / BrandArtwork.capHeight
		let box = BrandArtwork.wordmarkViewBox
		let coin = BrandArtwork.lockupCoinDiameter * unit
		let word = MintIn.stage(progress, MintIn.wordmark)
		HStack(alignment: .center, spacing: BrandArtwork.lockupGap * unit) {
			MintingCoin(rings: MintIn.stage(progress, MintIn.rings), dollar: MintIn.stage(progress, MintIn.dollar))
				.frame(width: coin, height: coin)
			Wordmark()
				.frame(width: box.width * unit, height: box.height * unit)
				.offset(x: (1 - word) * -capHeight * 0.4, y: (box.maxY + box.minY + BrandArtwork.capHeight) / 2 * unit)
				.opacity(word)
		}
		.accessibilityElement()
		.accessibilityLabel("Veritas")
	}
}

/// The full coin with its rings drawn as far as `rings` and its $ faded up by `dollar`.
private struct MintingCoin: View {
	let rings: Double
	let dollar: Double

	var body: some View {
		GeometryReader { proxy in
			let scale = min(proxy.size.width, proxy.size.height) / BrandArtwork.coinViewBox.width
			ZStack {
				ring(BrandArtwork.outerRing, scale: scale)
				ring(BrandArtwork.innerRing, scale: scale)
				DollarShape()
					.offset(y: (1 - dollar) * 3 * scale)
					.opacity(dollar)
			}
			.frame(width: proxy.size.width, height: proxy.size.height)
		}
	}

	private func ring(_ ring: (radius: CGFloat, width: CGFloat), scale: CGFloat) -> some View {
		Circle()
			.trim(from: 0, to: rings)
			.stroke(style: StrokeStyle(lineWidth: ring.width * scale, lineCap: rings < 1 ? .round : .butt))
			.rotationEffect(.degrees(-90))
			.frame(width: ring.radius * 2 * scale, height: ring.radius * 2 * scale)
	}
}

private struct DollarShape: Shape {
	func path(in rect: CGRect) -> Path {
		Path(svg: BrandArtwork.dollar).fitted(BrandArtwork.coinViewBox, in: rect)
	}
}
