/// Every place the sidebar, the Go menu and the command palette can take you.
public enum Screen: String, CaseIterable, Codable, Sendable {
	case budget
	case accounts
	case bills
	case goals
	case reports
	case dashboard
	case netProfit
	case sankey
	case customReports
	case insights
	case forecast
	case whatIfs
	case wrapped
	case statements
	case tax
	case settings

	public static let sections: [Screen] = [.budget, .accounts, .bills, .goals, .reports, .insights]

	public var title: String {
		switch self {
		case .budget: "Budget"
		case .accounts: "Accounts"
		case .bills: "Bills"
		case .goals: "Goals"
		case .reports: "Reports"
		case .dashboard: "Dashboard"
		case .netProfit: "Net Profit"
		case .sankey: "Sankey"
		case .customReports: "Custom reports"
		case .insights: "Insights"
		case .forecast: "Forecast"
		case .whatIfs: "What-ifs"
		case .wrapped: "Wrapped"
		case .statements: "Statements"
		case .tax: "Tax"
		case .settings: "Settings"
		}
	}

	public var pages: [Screen] {
		switch self {
		case .reports: [.dashboard, .netProfit, .sankey, .customReports]
		case .insights: [.forecast, .whatIfs, .wrapped, .statements, .tax]
		default: []
		}
	}

	public var section: Screen {
		Screen.sections.first { $0.pages.contains(self) } ?? self
	}

	/// ⌘1 to ⌘6.
	public var shortcutNumber: Int? {
		Screen.sections.firstIndex(of: self).map { $0 + 1 }
	}
}

public enum Destination: Hashable, Sendable {
	case screen(Screen)
	case account(id: String)

	public static let start = Destination.screen(.budget)

	public var section: Screen {
		switch self {
		case .screen(let screen): screen.section
		case .account: .accounts
		}
	}
}

/// A plain string, so a window can keep its place in scene storage.
extension Destination: RawRepresentable {
	private static let accountPrefix = "account:"

	public init?(rawValue: String) {
		if rawValue.hasPrefix(Self.accountPrefix) {
			let id = String(rawValue.dropFirst(Self.accountPrefix.count))
			guard !id.isEmpty else { return nil }
			self = .account(id: id)
		} else if let screen = Screen(rawValue: rawValue) {
			self = .screen(screen)
		} else {
			return nil
		}
	}

	public var rawValue: String {
		switch self {
		case .screen(let screen): screen.rawValue
		case .account(let id): Self.accountPrefix + id
		}
	}
}
