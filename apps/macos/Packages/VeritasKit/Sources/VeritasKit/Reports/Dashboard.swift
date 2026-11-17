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
		case .spending: "Monthly Spending"
		case .budgetAnalysis: "Budget analysis"
		case .customReport: "Custom report"
		case .crossover: "Crossover point"
		case .markdown: "Text"
		case .summary: "Summary"
		case .calendar: "Calendar"
		case .formula: "Formula"
		case .sankey: "Money flow by category"
		case .balanceForecast: "Balance with scheduled transactions"
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
		case .sankey: [("spent", "Categorized"), ("budgeted", "Budgeted")]
		default: []
		}
	}
}

/// The fields and operators Actual's filter editor offers for reports.
extension ReportCondition {
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

	/// `value` is Actual's JSON as text (contract v7). The editor types plain
	/// text, so a string is quoted; `true`, `false` and numbers stay bare.
	public var text: String {
		get {
			guard let data = value.data(using: .utf8), let decoded = try? JSONDecoder().decode(String.self, from: data) else { return value }
			return decoded
		}
		set {
			let bare = newValue == "true" || newValue == "false" || (field == "amount" && Int64(newValue) != nil)
			value = bare ? newValue : (String(data: (try? JSONEncoder().encode(newValue)) ?? Data(), encoding: .utf8) ?? "\"\"")
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

extension WidgetTimeFrame {
	public init(mode: TimeFrameMode, start: Month, end: Month) {
		self.init(mode: mode.rawValue, start: String(start.firstDay.prefix(7)), end: String(end.firstDay.prefix(7)))
	}

	public var frameMode: TimeFrameMode { TimeFrameMode(rawValue: mode) ?? .slidingWindow }
	public var startMonth: Month? { Month(date: start) }
	public var endMonth: Month? { Month(date: end) }
}

public enum SummaryKind: String, CaseIterable, Hashable, Sendable {
	case sum
	case avgPerMonth
	case avgPerYear
	case avgPerTransact
	case percentage

	public var title: String {
		switch self {
		case .sum: "Total"
		case .avgPerMonth: "Average per month"
		case .avgPerYear: "Average per year"
		case .avgPerTransact: "Average per transaction"
		case .percentage: "Percentage"
		}
	}
}

extension WidgetConfig {
	public static let empty = WidgetConfig(name: nil, timeFrame: nil, conditions: [], conditionsOr: false, mode: nil, summary: nil, content: nil, textAlign: nil, formula: nil, reportId: nil)

	public static func showing(report id: String) -> WidgetConfig {
		var config = empty
		config.reportId = id
		return config
	}

	public var summaryKind: SummaryKind {
		get { summary.flatMap(SummaryKind.init(rawValue:)) ?? .sum }
		set { summary = newValue.rawValue }
	}
}

/// A figure a card shows in large type.
public enum CardFigure: Hashable, Sendable {
	case money(Int64)
	case percent(Double)
	case number(Double)
	case days(Int)
	/// A NaN or infinite result, such as a percentage of nothing. Drawn blank.
	case undefined

	public var isNegative: Bool {
		switch self {
		case .money(let value): value < 0
		case .percent(let value), .number(let value): value < 0
		case .days(let value): value < 0
		case .undefined: false
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
	case netWorth([NetWorthMonth], change: Int64?)
	/// `classified` is Net Profit's figures, which the core sends until the
	/// card switches to Actual's own numbers; it decides the labels (ST-10.10).
	case cashFlow(income: Int64, expenses: Int64, netProfit: Int64, classified: Bool)
	case spending(month: Month, spent: Int64, compareMonth: Month, compareSpent: Int64, difference: Int64, classified: Bool)
	case sankey(SankeyReport)
	case customReport(CustomReportView)
	case markdown(String)
	case summary(CardFigure)
	case calendar(month: Month, days: [CalendarDay], income: Int64, expenses: Int64)
	case formula(CardFigure)
	case crossover(months: [Month], expenses: [Double], income: [Double], crossover: Month?, years: Double?)
	case budgetAnalysis(months: [Month], budgeted: [Int64], spent: [Int64])
	case balanceForecast([ForecastPoint])
	case ageOfMoney(days: Int?, trend: AgeTrend, series: [Int], insufficientData: Bool)
	case monteCarlo(points: [ForecastPoint], successRate: Double?)
	case pending
	case missingReport
	case problem(String)
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
	/// The core's minimum for the type; `nil` falls back to Actual's table.
	public var minimum: (width: Int64, height: Int64)?

	public init(
		id: String, kind: WidgetKind?, rawKind: String, x: Int64, y: Int64, width: Int64, height: Int64,
		config: WidgetConfig, content: CardContent, range: ReportPeriod? = nil, minimum: (width: Int64, height: Int64)? = nil
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
		self.minimum = minimum
	}

	public var minimumSize: (width: Int64, height: Int64) { minimum ?? kind?.minimumSize ?? (1, 1) }

	public static func == (lhs: DashboardCard, rhs: DashboardCard) -> Bool {
		lhs.id == rhs.id && lhs.rawKind == rhs.rawKind && (lhs.x, lhs.y, lhs.width, lhs.height) == (rhs.x, rhs.y, rhs.width, rhs.height)
			&& lhs.config == rhs.config && lhs.content == rhs.content && lhs.range == rhs.range
	}

	public func hash(into hasher: inout Hasher) {
		hasher.combine(id)
		hasher.combine(x)
		hasher.combine(y)
		hasher.combine(width)
		hasher.combine(height)
	}

	public var title: String? {
		if let name = config.name, !name.isEmpty { return name }
		if case .customReport(let report) = content, !report.settings.name.isEmpty { return report.settings.name }
		if kind == .markdown { return nil }
		if kind == .netWorth, !config.conditions.isEmpty { return "Balance of matching rows" }
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

	public init(settings: CustomReportSettings) {
		self.settings = settings
	}

	public var conditions: [ReportCondition] {
		get { settings.conditions }
		set { settings.conditions = newValue }
	}

	public var conditionsOr: Bool {
		get { settings.conditionsOr }
		set { settings.conditionsOr = newValue }
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
		func period(_ first: String, _ last: String) -> ReportPeriod? {
			guard let first = Month(date: first), let last = Month(date: last) else { return nil }
			return ReportPeriod(first: first, last: last)
		}
		var range: ReportPeriod?
		let content: CardContent
		switch widget.view {
		case .netWorth(let points):
			content = .netWorth(points, change: widget.view.change)
			if let first = points.first, let last = points.last { range = ReportPeriod(first: first.month, last: last.month) }
		case .cashFlow(let first, let last, let income, let expenses, let netProfit):
			content = .cashFlow(income: income, expenses: expenses, netProfit: netProfit, classified: true)
			range = ReportPeriod(first: first, last: last)
		case .spending(let month, let spent, let compareMonth, let compareSpent):
			content = .spending(month: month, spent: spent, compareMonth: compareMonth, compareSpent: compareSpent, difference: widget.view.difference ?? 0, classified: true)
		case .sankey(let report):
			content = .sankey(report)
			range = period(report.first, report.last)
		case .customReport(let report): content = .customReport(report)
		case .markdown(let text, _): content = .markdown(text)
		case .summary(let start, let end, let value, _, _):
			content = .summary(Self.figure(value, percentage: widget.config.summaryKind == .percentage))
			range = period(start, end)
		case .calendar(let months):
			if let shown = months.last, let month = Month(date: shown.start) {
				let days = shown.days.map { CalendarDay(day: Int($0.date.suffix(2)) ?? 1, income: $0.income, expenses: -$0.expense) }
				content = .calendar(month: month, days: days, income: shown.totalIncome, expenses: -shown.totalExpense)
			} else {
				content = .pending
			}
			if let first = months.first, let last = months.last { range = period(first.start, last.end) }
		case .ageOfMoney(let start, let end, _, let points, let currentDays, let trend, let insufficientData):
			content = .ageOfMoney(days: currentDays.map(Int.init), trend: trend, series: points.map { Int($0.days) }, insufficientData: insufficientData)
			range = period(start, end)
		case .budgetAnalysis(let start, let end, let months, _, _, _, _):
			content = .budgetAnalysis(months: months.map(\.month), budgeted: months.map(\.budgeted), spent: months.map(\.spent))
			range = period(start, end)
		case .crossover(let start, let end, let points, let crossover, _, _, _, _, let years, _, _):
			content = .crossover(months: points.map(\.month), expenses: points.map(\.expenses), income: points.map(\.investmentIncome), crossover: crossover, years: years)
			range = ReportPeriod(first: start, last: end)
		case .pending:
			content = .pending
			if let frame = widget.config.timeFrame, let first = frame.startMonth, let last = frame.endMonth { range = ReportPeriod(first: first, last: last) }
		case .missingReport: content = .missingReport
		case .problem(let message): content = .problem(message)
		case .unknown: content = .unknown
		}
		self.init(
			id: widget.id, kind: kind, rawKind: widget.kind, x: widget.x, y: widget.y, width: widget.width, height: widget.height,
			config: widget.config, content: content, range: range, minimum: (widget.minWidth, widget.minHeight)
		)
	}

	/// The summary card's value is unrounded and can be NaN or infinite, as in Actual.
	static func figure(_ value: Double, percentage: Bool) -> CardFigure {
		guard value.isFinite else { return .undefined }
		if percentage { return .percent(value) }
		return .money(Int64(value.rounded()))
	}
}
