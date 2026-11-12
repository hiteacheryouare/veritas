import VeritasKit

extension Screen {
	var symbol: String {
		switch self {
		case .budget: "envelope"
		case .accounts: "building.columns"
		case .bills: "calendar"
		case .goals: "target"
		case .reports, .dashboard, .netProfit, .sankey, .customReports: "chart.bar"
		case .insights, .forecast, .whatIfs, .wrapped, .statements, .tax: "chart.line.uptrend.xyaxis"
		case .settings: "slider.horizontal.3"
		}
	}
}
