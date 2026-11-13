import Foundation
import VeritasCore

/// Actual's widget types, by the `type` it stores (`dashboard/app.ts`).
public enum WidgetKind: String, CaseIterable, Hashable, Sendable {
	case netWorth = "net-worth-card"
	case cashFlow = "cash-flow-card"
	case spending = "spending-card"
	case budgetAnalysis = "budget-analysis-card"
	case customReport = "custom-report"
	case crossover = "crossover-card"
	case markdown = "markdown-card"
	case summary = "summary-card"
	case calendar = "calendar-card"
	case formula = "formula-card"
	case sankey = "sankey-card"
	case balanceForecast = "balance-forecast-card"
	case ageOfMoney = "age-of-money-card"
	case monteCarlo = "monte-carlo-card"

	public var title: String {
		switch self {
		case .netWorth: "Net worth"
		case .cashFlow: "Cash flow"
		case .spending: "Spending"
		case .budgetAnalysis: "Budget analysis"
		case .customReport: "Custom report"
		case .crossover: "Crossover point"
		case .markdown: "Text"
		case .summary: "Summary"
		case .calendar: "Calendar"
		case .formula: "Formula"
		case .sankey: "Sankey"
		case .balanceForecast: "Balance forecast"
		case .ageOfMoney: "Age of money"
		case .monteCarlo: "Monte Carlo"
		}
	}

	/// What Actual gives a new widget (`Overview.tsx` `onAddWidget`).
	public var defaultSize: (width: Int64, height: Int64) {
		(4, self == .sankey ? 3 : 2)
	}

	/// The smallest frame Actual's grid allows (`Overview.tsx`), which the core enforces too.
	public var minimumSize: (width: Int64, height: Int64) {
		let width: Int64 = switch self {
		case .formula: 1
		case .customReport, .markdown, .cashFlow, .summary: 2
		default: 3
		}
		let height: Int64 = switch self {
		case .customReport, .markdown, .formula: 1
		case .sankey: 3
		default: 2
		}
		return (width, height)
	}

	/// The settings a widget of this kind has, beyond its name.
	public var hasTimeFrame: Bool {
		switch self {
		case .markdown, .customReport, .formula: false
		default: true
		}
	}

	public var hasFilters: Bool {
		switch self {
		case .markdown, .customReport, .formula: false
		default: true
		}
	}

	/// Kinds with a mode picker, and the modes Actual stores for each.
	public var modes: [(id: String, title: String)] {
		switch self {
		case .netWorth: [("trend", "Trend"), ("stacked", "By account")]
		case .spending: [("single-month", "Compare two months"), ("average", "Against the average"), ("budget", "Against the budget")]
		case .sankey: [("spent", "Spent"), ("budgeted", "Budgeted")]
		default: []
		}
	}
}

/// One filter, as Actual stores it: `{ field, op, value }`.
public struct ReportCondition: Hashable, Sendable, Identifiable {
	public var id = UUID()
	public var field: String
	public var op: String
	public var value: String

	public init(field: String, op: String, value: String) {
		self.field = field
		self.op = op
		self.value = value
	}

	public static func == (lhs: ReportCondition, rhs: ReportCondition) -> Bool {
		(lhs.field, lhs.op, lhs.value) == (rhs.field, rhs.op, rhs.value)
	}

	public func hash(into hasher: inout Hasher) {
		hasher.combine(field)
		hasher.combine(op)
		hasher.combine(value)
	}

	/// The fields and operators Actual's filter editor offers for reports.
	public static let fields: [(id: String, title: String)] = [
		("account", "Account"), ("payee", "Payee"), ("category", "Category"), ("notes", "Notes"),
		("amount", "Amount"), ("date", "Date"), ("cleared", "Cleared"), ("reconciled", "Reconciled")
	]

	public static func operators(for field: String) -> [(id: String, title: String)] {
		switch field {
		case "amount": [("is", "is"), ("gt", "is more than"), ("gte", "is at least"), ("lt", "is less than"), ("lte", "is at most")]
		case "date": [("is", "is"), ("gt", "is after"), ("gte", "is on or after"), ("lt", "is before"), ("lte", "is on or before")]
		case "cleared", "reconciled": [("is", "is")]
		case "notes": [("is", "is"), ("contains", "contains"), ("doesNotContain", "doesn't contain")]
		default: [("is", "is"), ("isNot", "is not"), ("contains", "contains"), ("doesNotContain", "doesn't contain")]
		}
	}
}

/// Actual's `timeFrame` modes.
public enum TimeFrameMode: String, CaseIterable, Hashable, Sendable {
	case slidingWindow = "sliding-window"
	case fixed = "static"
	case full
	case yearToDate
	case lastYear
	case lastMonth
	case priorYearToDate
	case currentQuarter
	case previousQuarter

	public var title: String {
		switch self {
		case .slidingWindow: "Last months"
		case .fixed: "Fixed months"
		case .full: "All time"
		case .yearToDate: "Year to date"
		case .lastYear: "Last year"
		case .lastMonth: "Last month"
		case .priorYearToDate: "Last year to date"
		case .currentQuarter: "This quarter"
		case .previousQuarter: "Last quarter"
		}
	}
}

public struct TimeFrame: Hashable, Sendable {
	public var mode: TimeFrameMode
	public var start: Month
	public var end: Month

	public init(mode: TimeFrameMode, start: Month, end: Month) {
		self.mode = mode
		self.start = start
		self.end = end
	}
}

public enum SummaryKind: String, CaseIterable, Hashable, Sendable {
	case sum
	case avgPerMonth
	case avgPerTransact
	case percentage

	public var title: String {
		switch self {
		case .sum: "Total"
		case .avgPerMonth: "Average per month"
		case .avgPerTransact: "Average per transaction"
		case .percentage: "Percentage"
		}
	}
}

/// A widget's settings: what Actual keeps in its `meta`.
public struct WidgetConfig: Hashable, Sendable {
	public var name: String?
	public var timeFrame: TimeFrame?
	public var conditions: [ReportCondition]
	public var conditionsOr: Bool
	public var mode: String?
	public var summary: SummaryKind?
	public var content: String?
	public var textAlign: String?
	public var formula: String?
	public var reportID: String?

	public init(
		name: String? = nil, timeFrame: TimeFrame? = nil, conditions: [ReportCondition] = [], conditionsOr: Bool = false,
		mode: String? = nil, summary: SummaryKind? = nil, content: String? = nil, textAlign: String? = nil,
		formula: String? = nil, reportID: String? = nil
	) {
		self.name = name
		self.timeFrame = timeFrame
		self.conditions = conditions
		self.conditionsOr = conditionsOr
		self.mode = mode
		self.summary = summary
		self.content = content
		self.textAlign = textAlign
		self.formula = formula
		self.reportID = reportID
	}
}

/// A figure a card shows in large type.
public enum CardFigure: Hashable, Sendable {
	case money(Int64)
	case percent(Double)
	case number(Double)
	case days(Int)

	public var isNegative: Bool {
		switch self {
		case .money(let value): value < 0
		case .percent(let value), .number(let value): value < 0
		case .days(let value): value < 0
		}
	}
}

public struct CalendarDay: Hashable, Sendable {
	public let day: Int
	public let income: Int64
	public let expenses: Int64

	public init(day: Int, income: Int64, expenses: Int64) {
		self.day = day
		self.income = income
		self.expenses = expenses
	}
}

public struct ForecastPoint: Hashable, Sendable {
	public let month: Month
	public let low: Int64
	public let middle: Int64
	public let high: Int64

	public init(month: Month, low: Int64, middle: Int64, high: Int64) {
		self.month = month
		self.low = low
		self.middle = middle
		self.high = high
	}
}

/// What a card draws. `pending` is a kind the core doesn't compute yet.
public enum CardContent: Hashable, Sendable {
	case netWorth([NetWorthMonth])
	case cashFlow(income: Int64, expenses: Int64, netProfit: Int64)
	case spending(month: Month, spent: Int64, compareMonth: Month, compareSpent: Int64)
	case sankey(SankeyReport)
	case customReport(CustomReportView)
	case markdown(String)
	case summary(CardFigure)
	case calendar(month: Month, days: [CalendarDay])
	case formula(CardFigure)
	case crossover(months: [Month], expenses: [Int64], income: [Int64], crossover: Month?)
	case budgetAnalysis(months: [Month], budgeted: [Int64], spent: [Int64])
	case balanceForecast([ForecastPoint])
	case ageOfMoney(days: Int, change: Int, series: [Int])
	case monteCarlo(points: [ForecastPoint], successRate: Double?)
	case pending
	case unknown
}

public struct DashboardCard: Identifiable, Hashable, Sendable {
	public let id: String
	/// `nil` for a type Veritas doesn't know, which `rawKind` names.
	public let kind: WidgetKind?
	public let rawKind: String
	public var x: Int64
	public var y: Int64
	public var width: Int64
	public var height: Int64
	public var config: WidgetConfig
	public var content: CardContent
	/// The months the card covers, when it has a range.
	public var range: ReportPeriod?

	public init(
		id: String, kind: WidgetKind?, rawKind: String, x: Int64, y: Int64, width: Int64, height: Int64,
		config: WidgetConfig, content: CardContent, range: ReportPeriod? = nil
	) {
		self.id = id
		self.kind = kind
		self.rawKind = rawKind
		self.x = x
		self.y = y
		self.width = width
		self.height = height
		self.config = config
		self.content = content
		self.range = range
	}

	public var minimumSize: (width: Int64, height: Int64) { kind?.minimumSize ?? (1, 1) }

	public var title: String? {
		if let name = config.name, !name.isEmpty { return name }
		if case .customReport(let report) = content, !report.settings.name.isEmpty { return report.settings.name }
		if kind == .markdown { return nil }
		return kind?.title ?? rawKind
	}
}

public struct DashboardSheet: Identifiable, Hashable, Sendable {
	public let id: String
	public var name: String
	public var cards: [DashboardCard]

	public init(id: String, name: String, cards: [DashboardCard]) {
		self.id = id
		self.name = name
		self.cards = cards
	}
}

/// A custom report's settings with its filters, for the viewer and editor.
public struct ReportDraft: Hashable, Sendable {
	public var settings: CustomReportSettings
	public var conditions: [ReportCondition]
	public var conditionsOr: Bool

	public init(settings: CustomReportSettings, conditions: [ReportCondition] = [], conditionsOr: Bool = false) {
		self.settings = settings
		self.conditions = conditions
		self.conditionsOr = conditionsOr
	}
}

extension DashboardSheet {
	/// The core's dashboards, as far as its contract draws them today.
	public init(_ page: DashboardPage) {
		self.init(id: page.id, name: page.name, cards: page.widgets.map(DashboardCard.init))
	}
}

extension DashboardCard {
	init(_ widget: DashboardWidget) {
		let kind = WidgetKind(rawValue: widget.kind)
		var range: ReportPeriod?
		let content: CardContent
		switch widget.view {
		case .netWorth(let points):
			content = .netWorth(points)
			if let first = points.first, let last = points.last { range = ReportPeriod(first: first.month, last: last.month) }
		case .cashFlow(let first, let last, let income, let expenses, let netProfit):
			content = .cashFlow(income: income, expenses: expenses, netProfit: netProfit)
			range = ReportPeriod(first: first, last: last)
		case .spending(let month, let spent, let compareMonth, let compareSpent):
			content = .spending(month: month, spent: spent, compareMonth: compareMonth, compareSpent: compareSpent)
		case .sankey(let report):
			content = .sankey(report)
			if let first = Month(date: report.first), let last = Month(date: report.last) { range = ReportPeriod(first: first, last: last) }
		case .customReport(let report): content = .customReport(report)
		case .markdown(let text, _): content = .markdown(text)
		case .openInActual: content = kind == nil ? .unknown : .pending
		}
		self.init(
			id: widget.id, kind: kind, rawKind: widget.kind, x: widget.x, y: widget.y, width: widget.width, height: widget.height,
			config: WidgetConfig(name: widget.name), content: content, range: range
		)
	}
}
