import VeritasKit

extension Screen {
	var symbol: String {
		switch self {
		case .budget: "envelope"
		case .accounts: "building.columns"
		case .bills: "calendar"
		case .rules: "list.bullet.indent"
		case .goals: "target"
		case .dashboard: "square.grid.2x2"
		case .reports, .netProfit, .sankey, .customReports: "chart.bar"
		case .insights, .forecast, .whatIfs, .wrapped, .statements, .tax: "chart.line.uptrend.xyaxis"
		case .settings: "slider.horizontal.3"
		}
	}
}
