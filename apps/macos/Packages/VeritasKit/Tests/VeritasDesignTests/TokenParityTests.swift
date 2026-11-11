import Foundation
import Testing
@testable import VeritasDesign

/// design/tokens.css is the source of truth. These tests fail when the Swift
/// tokens drift from it.
struct TokenParityTests {
	/// Fonts map to system designs, weights to `Font.Weight`, and macOS draws its own shadows.
	/// The desktop colors only paint the mockups' backdrop.
	static let notMirrored: Set<String> = [
		"color-desktop-1", "color-desktop-2",
		"font-text", "font-display", "font-figures", "font-mono", "font-paper",
		"weight-regular", "weight-medium", "weight-semibold", "weight-bold",
		"shadow-window", "shadow-sheet", "shadow-popover", "shadow-control"
	]

	static let metrics: [String: CGFloat] = [
		"space-1": Spacing.s1, "space-2": Spacing.s2, "space-3": Spacing.s3, "space-4": Spacing.s4,
		"space-5": Spacing.s5, "space-6": Spacing.s6, "space-7": Spacing.s7,
		"size-row": Size.row, "size-row-dense": Size.rowDense, "size-control": Size.control,
		"size-control-small": Size.controlSmall, "size-sidebar": Size.sidebar, "size-bar": Size.bar,
		"size-ring": Size.ring, "size-ring-large": Size.ringLarge,
		"radius-window": Radius.window, "radius-panel": Radius.panel, "radius-sheet": Radius.sheet,
		"radius-section": Radius.section, "radius-control": Radius.control, "radius-field": Radius.field,
		"radius-bar": Radius.bar,
		"text-caption": TextSize.caption.rawValue, "text-body": TextSize.body.rawValue,
		"text-heading": TextSize.heading.rawValue, "text-title": TextSize.title.rawValue,
		"text-display": TextSize.display.rawValue, "text-showcase": TextSize.showcase.rawValue
	]

	static let otherMirrored: Set<String> = ["duration-fast", "duration-sheet", "easing-standard", "easing-exit", "line-tight", "line-body"]

	let css: String

	init() throws {
		let root = URL(filePath: #filePath)
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.deletingLastPathComponent()
		css = try String(contentsOf: root.appending(path: "design/tokens.css"), encoding: .utf8)
	}

	@Test func everyTokenIsMirroredOrListedAsNotMirrored() {
		let names = Set(css.matches(of: /--([a-z0-9-]+):/).map { String($0.1) })
		let mirrored = Set(ColorToken.byName.keys.map { "color-" + $0 })
			.union(Self.metrics.keys)
			.union(Self.otherMirrored)
		#expect(mirrored.isDisjoint(with: Self.notMirrored))
		#expect(names.subtracting(mirrored).subtracting(Self.notMirrored).isEmpty, "tokens.css has tokens Swift doesn't mirror")
		#expect(mirrored.union(Self.notMirrored).subtracting(names).isEmpty, "Swift mirrors tokens tokens.css no longer has")
	}

	@Test func everyColorMatchesItsLightAndDarkValue() throws {
		let light = colors(in: try block(from: ":root,", to: "[data-theme=\"dark\"]"))
		let dark = colors(in: try block(from: "[data-theme=\"dark\"]", to: ":root {"))
		let names = Set(ColorToken.byName.keys)
		for name in names {
			let token = try #require(ColorToken.byName[name])
			#expect(token.light == light[name], "light --color-\(name)")
			#expect(token.dark == dark[name], "dark --color-\(name)")
		}
	}

	@Test func spacingSizesAndRadiiMatch() {
		let expected = Self.metrics
		for (name, value) in expected {
			#expect(pixels(name) == value, "--\(name)")
		}
		#expect(number("line-tight") == LineHeight.tight)
		#expect(number("line-body") == LineHeight.body)
	}

	@Test func motionMatches() {
		#expect(value("duration-fast") == "\(Int(Motion.fast.duration * 1000))ms")
		#expect(value("duration-sheet") == "\(Int(Motion.sheet.duration * 1000))ms")
		#expect(value("easing-standard") == bezier(.standard))
		#expect(value("easing-exit") == bezier(.exit))
	}

	private func block(from start: String, to end: String) throws -> Substring {
		let from = try #require(css.range(of: start))
		let to = try #require(css.range(of: end, range: from.upperBound..<css.endIndex))
		return css[from.upperBound..<to.lowerBound]
	}

	private func colors(in block: Substring) -> [String: RGBA] {
		var result: [String: RGBA] = [:]
		for match in block.matches(of: /--color-([a-z0-9-]+):\s*([^;]+);/) {
			result[String(match.1)] = rgba(String(match.2))
		}
		return result
	}

	private func rgba(_ text: String) -> RGBA? {
		if text.hasPrefix("#"), let hex = UInt32(text.dropFirst(), radix: 16) {
			return RGBA(hex)
		}
		guard let match = text.firstMatch(of: /rgba\((\d+),\s*(\d+),\s*(\d+),\s*([\d.]+)\)/),
			let red = UInt32(match.1), let green = UInt32(match.2), let blue = UInt32(match.3),
			let alpha = Double(match.4)
		else { return nil }
		return RGBA(red << 16 | green << 8 | blue, alpha: alpha)
	}

	private func value(_ name: String) -> String? {
		let pattern = try? Regex("--\(name):\\s*([^;]+);")
		guard let pattern, let match = css.firstMatch(of: pattern) else { return nil }
		return match.output[1].substring.map { String($0).trimmingCharacters(in: .whitespaces) }
	}

	private func pixels(_ name: String) -> CGFloat? {
		value(name).flatMap { Double($0.replacing("px", with: "")) }.map { CGFloat($0) }
	}

	private func number(_ name: String) -> CGFloat? {
		value(name).flatMap(Double.init).map { CGFloat($0) }
	}

	private func bezier(_ easing: Easing) -> String {
		let points = easing.controlPoints
		let parts = [points.x1, points.y1, points.x2, points.y2].map { $0.formatted(.number.grouping(.never)) }
		return "cubic-bezier(\(parts.joined(separator: ", ")))"
	}
}

/// The app's AccentColor asset drives system selection and focus rings, so it
/// has to stay graphite like `color-accent`.
struct AccentAssetParityTests {
	@Test func theAppsAccentColorIsTheAccentToken() throws {
		let asset = URL(filePath: #filePath)
			.deletingLastPathComponent()
			.appending(path: "../../../../Veritas/Assets.xcassets/AccentColor.colorset/Contents.json")
			.standardizedFileURL
		let json = try JSONSerialization.jsonObject(with: Data(contentsOf: asset)) as? [String: Any]
		let colors = try #require(json?["colors"] as? [[String: Any]])
		var byAppearance: [String: RGBA] = [:]
		for entry in colors {
			let isDark = (entry["appearances"] as? [[String: String]])?.contains { $0["value"] == "dark" } == true
			let components = try #require((entry["color"] as? [String: Any])?["components"] as? [String: String])
			let channel = { (name: String) -> UInt32 in
				UInt32(components[name]?.dropFirst(2) ?? "", radix: 16) ?? 0
			}
			byAppearance[isDark ? "dark" : "light"] = RGBA(channel("red") << 16 | channel("green") << 8 | channel("blue"))
		}
		#expect(byAppearance["light"] == ColorToken.accent.light)
		#expect(byAppearance["dark"] == ColorToken.accent.dark)
	}
}
