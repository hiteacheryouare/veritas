import SwiftUI
import VeritasDesign

/// Wrapped's page colors (design/wrapped-editorial.md, Color). Paper follows
/// the appearance: light pages in light mode, dark pages in dark mode. Every
/// color is an existing token's own value for that appearance, so an
/// exported image looks the same wherever it opens and adds no new value.
struct WrappedPalette {
	let isDark: Bool
	let window: Color
	let paper: Color
	let ink: Color
	let inkSecondary: Color
	let rule: Color
	/// Dark pages need an edge, since a shadow barely shows on a dark window.
	let edge: Color?
	let shadow: Color
	let bar: Color
	let barSoft: Color
	/// The one highlighted mark per chart and the cover rule.
	let accent: Color
	/// No-spend days on the calendar. The brand's secondary accent takes
	/// this place when its token lands; until then it is the accent.
	let accentSecondary: Color

	static func of(_ scheme: ColorScheme) -> WrappedPalette {
		scheme == .dark ? dark : light
	}

	static let light = WrappedPalette(
		isDark: false,
		window: Color(rgba: ColorToken.surfaceWindow.light),
		paper: Color(rgba: ColorToken.surfacePaper.light),
		ink: Color(rgba: ColorToken.textPaper.light),
		inkSecondary: Color(rgba: ColorToken.textSecondary.light),
		rule: Color(rgba: ColorToken.textPaper.light).opacity(0.16),
		edge: nil,
		shadow: .black.opacity(0.18),
		bar: Color(rgba: ColorToken.chartExpense.light),
		barSoft: Color(rgba: ColorToken.chartExpenseSoft.light),
		accent: Color(rgba: ColorToken.accent.light),
		accentSecondary: Color(rgba: ColorToken.accent.light)
	)

	static let dark = WrappedPalette(
		isDark: true,
		window: Color(rgba: ColorToken.surfaceWindow.dark),
		paper: Color(rgba: ColorToken.surfaceGrouped.dark),
		ink: Color(rgba: ColorToken.textPrimary.dark),
		inkSecondary: Color(rgba: ColorToken.textSecondary.dark),
		rule: Color(rgba: ColorToken.textPrimary.dark).opacity(0.2),
		edge: Color(rgba: ColorToken.edgeGrouped.dark),
		shadow: .black.opacity(0.4),
		bar: Color(rgba: ColorToken.chartExpense.dark),
		barSoft: Color(rgba: ColorToken.chartExpenseSoft.dark),
		accent: Color(rgba: ColorToken.accent.dark),
		accentSecondary: Color(rgba: ColorToken.accent.dark)
	)
}

/// Wrapped's type at canvas scale: New York carries the voice and the
/// display figures, SF Pro carries the data (design/wrapped-editorial.md, Typography).
enum WrappedType {
	static func serif(_ size: CGFloat, _ weight: Font.Weight = .regular) -> Font {
		.system(size: size, weight: weight, design: .serif)
	}

	static let coverYear = serif(168).monospacedDigit()
	static let headline = serif(40, .semibold)
	static let dek = serif(20)
	static let hero = serif(96, .medium).monospacedDigit()
	static let heading = serif(24, .semibold)
	static let contents = serif(17)
	static let label = Font.system(size: 13)
	static let figure = Font.system(size: 13, weight: .medium, design: .rounded).monospacedDigit()
	static let annotation = Font.system(size: 11, weight: .medium)
	static let axis = Font.system(size: 11)
	static let notes = Font.system(size: 13)
	static let folio = Font.system(size: 11, design: .rounded).monospacedDigit()
}

/// The canvas a spread is laid out on before it is scaled to the window.
enum WrappedCanvas {
	static let spread = CGSize(width: 1280, height: 800)
	static let page = CGSize(width: 640, height: 800)
	static let top: CGFloat = 72
	static let bottom: CGFloat = 64
	static let outer: CGFloat = 64
	static let spine: CGFloat = 40
	/// Below this window width the pages of a spread stack and scroll.
	static let narrow: CGFloat = 1000
}

extension Color {
	init(rgba: RGBA) {
		self.init(.sRGB, red: rgba.red, green: rgba.green, blue: rgba.blue, opacity: rgba.alpha)
	}
}
