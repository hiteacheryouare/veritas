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
		case .spending: "Monthly spending"
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
		case .monteCarlo: "Monte Carlo analysis"
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


/// A summary card's figure, as large as the card allows.
public enum CardFigure: Hashable, Sendable {
	case money(Int64)
	case percent(Double)
	/// A NaN or infinite result, such as a percentage of nothing. Drawn blank.
	case undefined

	public var isNegative: Bool {
		switch self {
		case .money(let value): value < 0
		case .percent(let value): value < 0
		case .undefined: false
		}
	}

	/// The summary card's value is unrounded and can be NaN or infinite, as in
	/// Actual; it's rounded here only to be shown.
	public init(summary value: Double, percentage: Bool) {
		if !value.isFinite {
			self = .undefined
		} else if percentage {
			self = .percent(value)
		} else {
			self = .money(Int64(value.rounded()))
		}
	}
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
	/// The core's view of the card, drawn as it comes (D73).
	public var content: WidgetView
	/// The core's minimum for the type; `nil` falls back to Actual's table.
	public var minimum: (width: Int64, height: Int64)?

	public init(
		id: String, kind: WidgetKind?, rawKind: String, x: Int64, y: Int64, width: Int64, height: Int64,
		config: WidgetConfig, content: WidgetView, minimum: (width: Int64, height: Int64)? = nil
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
		self.minimum = minimum
	}

	public var minimumSize: (width: Int64, height: Int64) { minimum ?? kind?.minimumSize ?? (1, 1) }

	public static func == (lhs: DashboardCard, rhs: DashboardCard) -> Bool {
		lhs.id == rhs.id && lhs.rawKind == rhs.rawKind && (lhs.x, lhs.y, lhs.width, lhs.height) == (rhs.x, rhs.y, rhs.width, rhs.height)
			&& lhs.config == rhs.config && lhs.content == rhs.content
	}

	public func hash(into hasher: inout Hasher) {
		hasher.combine(id)
		hasher.combine(x)
		hasher.combine(y)
		hasher.combine(width)
		hasher.combine(height)
	}

	public var isFiltered: Bool { !config.conditions.isEmpty }

	/// ST-10.10: the user's name, else Actual's title in sentence case. A
	/// formula card set to hide its title shows none, as in Actual.
	public var title: String? {
		if case .formula(_, _, false, _, _) = content { return nil }
		if let name = config.name, !name.isEmpty { return name }
		if case .customReport(let report) = content, !report.settings.name.isEmpty { return report.settings.name }
		if kind == .markdown { return nil }
		if kind == .netWorth, isFiltered { return "Balance of matching rows" }
		return kind?.title ?? rawKind
	}

	/// The months the card covers, when it has a range.
	public var range: ReportPeriod? {
		func period(_ first: String, _ last: String) -> ReportPeriod? {
			guard let first = Month(date: first), let last = Month(date: last) else { return nil }
			return ReportPeriod(first: first, last: last)
		}
		switch content {
		case .netWorth(let range, _, _, _, _, _, _, _, _, _), .cashFlow(let range, _, _, _), .balanceForecast(let range, _, _, _, _, _, _, _, _, _):
			return period(range.start, range.end)
		case .sankey(_, let first, let last, _, _, _, _):
			return ReportPeriod(first: first, last: last)
		case .summary(let start, let end, _, _, _), .ageOfMoney(let start, let end, _, _, _, _, _), .budgetAnalysis(let start, let end, _, _, _, _, _):
			return period(start, end)
		case .crossover(let start, let end, _, _, _, _, _, _, _, _, _):
			return ReportPeriod(first: start, last: end)
		case .calendar(let months):
			guard let first = months.first, let last = months.last else { return nil }
			return period(first.start, last.end)
		default:
			return nil
		}
	}

	/// The line under the title: the range, or what the spending card compares.
	public var subtitle: String? {
		guard case .spending(let mode, let compare, let compareTo, let averageFirst, let averageLast, _, _, _) = content else { return range?.title }
		let shown = ReportPeriod.monthYear(compare)
		switch mode {
		case .singleMonth:
			return "Compare \(shown) to \(ReportPeriod.monthYear(compareTo))"
		case .budget:
			return "Compare \(shown) to budgeted"
		case .average:
			guard let averageFirst, let averageLast else { return "Compare \(shown) to average" }
			return "Compare \(shown) to the average of \(ReportPeriod(first: averageFirst, last: averageLast).title)"
		}
	}

	/// The summary card's figure, `nil` for every other card.
	public var summaryFigure: CardFigure? {
		guard case .summary(_, _, let value, _, _) = content else { return nil }
		return CardFigure(summary: value, percentage: config.summaryKind == .percentage)
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

	/// Each card's row once the grid is compacted upward, as Actual's grid
	/// draws it: a card rises until it meets a card above it in the same columns,
	/// so a stored gap between rows never shows.
	public var rows: [String: Int64] {
		var placed: [(x: Int64, width: Int64, bottom: Int64)] = []
		var rows: [String: Int64] = [:]
		for card in cards.sorted(by: { ($0.y, $0.x) < ($1.y, $1.x) }) {
			let row = placed
				.filter { $0.x < card.x + card.width && card.x < $0.x + $0.width }
				.map(\.bottom)
				.max() ?? 0
			rows[card.id] = row
			placed.append((card.x, card.width, row + card.height))
		}
		return rows
	}
}

/// A custom report's settings with its filters, for the viewer and editor.
public struct ReportDraft: Hashable, Sendable {
	public var settings: CustomReportSettings

	public init(settings: CustomReportSettings) {
		self.settings = settings
	}

	public var conditions: [RuleCondition] {
		get { settings.conditions }
		set { settings.conditions = newValue }
	}

	public var conditionsOr: Bool {
		get { settings.conditionsOr }
		set { settings.conditionsOr = newValue }
	}
}

extension DashboardSheet {
	public init(_ page: DashboardPage) {
		self.init(id: page.id, name: page.name, cards: page.widgets.map(DashboardCard.init))
	}
}

extension DashboardCard {
	public init(_ widget: DashboardWidget) {
		self.init(
			id: widget.id, kind: WidgetKind(rawValue: widget.kind), rawKind: widget.kind, x: widget.x, y: widget.y, width: widget.width, height: widget.height,
			config: widget.config, content: widget.view, minimum: (widget.minWidth, widget.minHeight)
		)
	}
}

extension DashboardCard {
	/// Whether clicking the card opens it full size, as Actual's cards open
	/// their report page. A text card, a deleted report, a card with a
	/// problem and a type Veritas doesn't know have nothing more to show.
	public var opensFullSize: Bool {
		switch content {
		case .markdown, .missingReport, .unknown, .problem: false
		default: true
		}
	}
}
