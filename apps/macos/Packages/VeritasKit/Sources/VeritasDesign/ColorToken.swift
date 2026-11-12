import SwiftUI

#if canImport(AppKit)
import AppKit
#elseif canImport(UIKit)
import UIKit
#endif

public struct RGBA: Hashable, Sendable {
	public let red: Double
	public let green: Double
	public let blue: Double
	public let alpha: Double

	public init(_ hex: UInt32, alpha: Double = 1) {
		red = Double((hex >> 16) & 0xff) / 255
		green = Double((hex >> 8) & 0xff) / 255
		blue = Double(hex & 0xff) / 255
		self.alpha = alpha
	}
}

public struct ColorToken: Hashable, Sendable {
	public let light: RGBA
	public let dark: RGBA

	public init(light: RGBA, dark: RGBA) {
		self.light = light
		self.dark = dark
	}
}

public extension Color {
	init(_ token: ColorToken) {
		#if canImport(AppKit)
		self.init(nsColor: NSColor(name: nil) { appearance in
			let isDark = appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
			return NSColor(isDark ? token.dark : token.light)
		})
		#elseif canImport(UIKit)
		self.init(uiColor: UIColor { traits in
			UIColor(traits.userInterfaceStyle == .dark ? token.dark : token.light)
		})
		#endif
	}
}

#if canImport(AppKit)
extension NSColor {
	convenience init(_ rgba: RGBA) {
		self.init(srgbRed: rgba.red, green: rgba.green, blue: rgba.blue, alpha: rgba.alpha)
	}
}
#elseif canImport(UIKit)
extension UIColor {
	convenience init(_ rgba: RGBA) {
		self.init(red: rgba.red, green: rgba.green, blue: rgba.blue, alpha: rgba.alpha)
	}
}
#endif
