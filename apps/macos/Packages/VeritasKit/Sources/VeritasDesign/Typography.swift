import SwiftUI

public enum TextSize: CGFloat, CaseIterable, Sendable {
	case caption = 11
	case body = 13
	case heading = 15
	case title = 20
	case display = 40
	case showcase = 64
}

public enum LineHeight {
	public static let tight: CGFloat = 1.15
	public static let body: CGFloat = 1.45
}

public extension Font {
	static func text(_ size: TextSize, weight: Font.Weight = .regular) -> Font {
		.system(size: size.rawValue, weight: weight)
	}

	/// SF Pro Rounded with tabular digits, so columns of amounts line up.
	static func figures(_ size: TextSize, weight: Font.Weight = .medium) -> Font {
		.system(size: size.rawValue, weight: weight, design: .rounded).monospacedDigit()
	}

	static func mono(_ size: TextSize, weight: Font.Weight = .regular) -> Font {
		.system(size: size.rawValue, weight: weight, design: .monospaced)
	}

	static func paper(_ size: TextSize, weight: Font.Weight = .regular) -> Font {
		.system(size: size.rawValue, weight: weight, design: .serif)
	}
}
