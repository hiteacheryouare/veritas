import SwiftUI

/// The Veritas coin, drawn from design/brand's master geometry. `small`
/// drops the inner ring and thickens the $ for 32 pt and below.
public struct CoinMark: Shape {
	public var small: Bool

	public init(small: Bool = false) {
		self.small = small
	}

	public func path(in rect: CGRect) -> Path {
		BrandPath.coin(small: small).fitted(BrandArtwork.coinViewBox, in: rect)
	}

	public func sizeThatFits(_ proposal: ProposedViewSize) -> CGSize {
		let side = min(proposal.width ?? 32, proposal.height ?? 32)
		return CGSize(width: side, height: side)
	}
}

/// The traced "Veritas" wordmark. It keeps its proportions inside any frame.
public struct Wordmark: Shape {
	public init() {}

	public func path(in rect: CGRect) -> Path {
		BrandPath.wordmark.fitted(BrandArtwork.wordmarkViewBox, in: rect)
	}

	public static let aspectRatio = BrandArtwork.wordmarkViewBox.width / BrandArtwork.wordmarkViewBox.height
}

/// Coin and wordmark side by side, spaced as design/brand/svg/lockup-horizontal.svg.
/// `capHeight` sets the size; the lockup takes its color from the foreground style.
public struct VeritasLockup: View {
	public var capHeight: CGFloat

	public init(capHeight: CGFloat) {
		self.capHeight = capHeight
	}

	public var body: some View {
		let unit = capHeight / BrandArtwork.capHeight
		let box = BrandArtwork.wordmarkViewBox
		let coin = BrandArtwork.lockupCoinDiameter * unit
		HStack(alignment: .center, spacing: BrandArtwork.lockupGap * unit) {
			CoinMark(small: coin < 33)
				.frame(width: coin, height: coin)
			Wordmark()
				.frame(width: box.width * unit, height: box.height * unit)
				// The wordmark's box runs from cap height to its deepest overshoot; centre on the caps.
				.offset(y: (box.maxY + box.minY + BrandArtwork.capHeight) / 2 * unit)
		}
		.accessibilityElement()
		.accessibilityLabel("Veritas")
	}
}

enum BrandPath {
	static let fullCoin = Path(svg: BrandArtwork.coin)
	static let smallCoin = Path(svg: BrandArtwork.smallCoin)
	static let wordmark = Path(svg: BrandArtwork.wordmark)

	static func coin(small: Bool) -> Path { small ? smallCoin : fullCoin }
}

extension Path {
	/// Reads the absolute M, L, C and Z commands that tools/brand/build-brand.swift writes.
	init(svg: String) {
		self.init()
		let scanner = Scanner(string: svg)
		scanner.charactersToBeSkipped = CharacterSet(charactersIn: " ,")
		func point() -> CGPoint { CGPoint(x: scanner.scanDouble() ?? 0, y: scanner.scanDouble() ?? 0) }
		while let command = scanner.scanCharacter() {
			switch command {
			case "M": move(to: point())
			case "L": addLine(to: point())
			case "C":
				let control1 = point(), control2 = point(), end = point()
				addCurve(to: end, control1: control1, control2: control2)
			case "Z": closeSubpath()
			default: return
			}
		}
	}

	func fitted(_ box: CGRect, in rect: CGRect) -> Path {
		let scale = min(rect.width / box.width, rect.height / box.height)
		let x = rect.midX - box.width * scale / 2 - box.minX * scale
		let y = rect.midY - box.height * scale / 2 - box.minY * scale
		return applying(CGAffineTransform(a: scale, b: 0, c: 0, d: scale, tx: x, ty: y))
	}
}
