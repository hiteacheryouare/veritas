import AppKit

enum AppearanceSetting: String, CaseIterable {
	case system
	case light
	case dark

	static let storageKey = "appearance"

	static var stored: AppearanceSetting {
		UserDefaults.standard.string(forKey: storageKey).flatMap(AppearanceSetting.init(rawValue:)) ?? .system
	}

	var title: String {
		switch self {
		case .system: "System"
		case .light: "Light"
		case .dark: "Dark"
		}
	}

	@MainActor func apply() {
		NSApp.appearance = switch self {
		case .system: nil
		case .light: NSAppearance(named: .aqua)
		case .dark: NSAppearance(named: .darkAqua)
		}
	}
}
