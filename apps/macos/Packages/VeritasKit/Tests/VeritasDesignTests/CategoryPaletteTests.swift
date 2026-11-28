import Foundation
import Testing
import VeritasDesign

/// The chart categories (design/tokens.md, Categories): anchored on Ryan's
/// primary and secondary, clear of the money states, and apart from each other
/// under normal vision and full protanopia, deuteranopia and tritanopia
/// (Machado, Oliveira and Fernandes 2009). tools/brand/chart-palette.py chose them.
struct CategoryPaletteTests {
	static let floors = (light: 7.0, dark: 5.0)

	@Test func theCategoriesStartFromRyansPrimaryAndSecondary() {
		#expect(ColorToken.categories.count == 9, "one per core chart series, so series 9 never repeats series 1")
		#expect(ColorToken.category1.light == RGBA(0x23169c))
		#expect(ColorToken.category2.light == RGBA(0x007ea7))
	}

	@Test func noCategoryReadsAsAMoneyState() {
		let money: [(hue: Double, clear: Double)] = [(25, 26), (85, 20), (160, 22)]
		for category in ColorToken.categories {
			for value in [category.light, category.dark] {
				let (_, chroma, hue) = OKLab.lch(value)
				guard chroma >= 0.05 else { continue }
				for state in money {
					let apart = min(abs(hue - state.hue), 360 - abs(hue - state.hue))
					#expect(apart > state.clear, "\(category) sits \(apart)° from a money state")
				}
			}
		}
	}

	@Test func everyPairStaysApartUnderEveryKindOfVision() {
		for dark in [false, true] {
			let colors = ColorToken.categories.map { dark ? $0.dark : $0.light }
			let floor = dark ? Self.floors.dark : Self.floors.light
			for (i, a) in colors.enumerated() {
				for b in colors[(i + 1)...] {
					for vision in OKLab.Vision.allCases {
						let distance = OKLab.distance(a, b, vision)
						#expect(distance >= floor, "\(a) and \(b), \(vision), dark \(dark): \(distance)")
					}
				}
			}
		}
	}
}

enum OKLab {
	enum Vision: CaseIterable {
		case normal, protan, deutan, tritan

		var matrix: [[Double]]? {
			switch self {
			case .normal: nil
			case .protan: [[0.152286, 1.052583, -0.204868], [0.114503, 0.786281, 0.099216], [-0.003882, -0.048116, 1.051998]]
			case .deutan: [[0.367322, 0.860646, -0.227968], [0.280085, 0.672501, 0.047413], [-0.011820, 0.042940, 0.968881]]
			case .tritan: [[1.255528, -0.076749, -0.178779], [-0.078411, 0.930809, 0.147602], [0.004733, 0.691367, 0.303900]]
			}
		}
	}

	static func lab(_ color: RGBA, _ vision: Vision = .normal) -> (Double, Double, Double) {
		func linear(_ c: Double) -> Double { c <= 0.04045 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4) }
		var rgb = [linear(color.red), linear(color.green), linear(color.blue)]
		if let m = vision.matrix {
			rgb = m.map { row in min(max(row[0] * rgb[0] + row[1] * rgb[1] + row[2] * rgb[2], 0), 1) }
		}
		let l = cbrt(0.4122214708 * rgb[0] + 0.5363325363 * rgb[1] + 0.0514459929 * rgb[2])
		let m = cbrt(0.2119034982 * rgb[0] + 0.6806995451 * rgb[1] + 0.1073969566 * rgb[2])
		let s = cbrt(0.0883024619 * rgb[0] + 0.2817188376 * rgb[1] + 0.6299787005 * rgb[2])
		return (
			0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
			1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
			0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s
		)
	}

	static func lch(_ color: RGBA) -> (Double, Double, Double) {
		let (l, a, b) = lab(color)
		let hue = atan2(b, a) * 180 / .pi
		return (l, (a * a + b * b).squareRoot(), hue < 0 ? hue + 360 : hue)
	}

	/// OKLab distance, times 100.
	static func distance(_ a: RGBA, _ b: RGBA, _ vision: Vision) -> Double {
		let (l1, a1, b1) = lab(a, vision)
		let (l2, a2, b2) = lab(b, vision)
		return 100 * ((l1 - l2) * (l1 - l2) + (a1 - a2) * (a1 - a2) + (b1 - b2) * (b1 - b2)).squareRoot()
	}
}
