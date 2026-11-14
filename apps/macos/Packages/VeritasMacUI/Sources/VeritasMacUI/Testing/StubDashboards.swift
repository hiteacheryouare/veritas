#if DEBUG
import Foundation
import VeritasCore
import VeritasKit

/// Dashboards for the stub budget, with every widget type and made-up figures,
/// editable in memory so UI tests can add, move, configure and remove.
final class StubDashboards: @unchecked Sendable {
	static let november = Month(year: 2026, month: 11)

	private let lock = NSLock()
	private let reports: StubReports
	private var sheets: [DashboardSheet] = []
	private var next = 0

	init(reports: StubReports) {
		self.reports = reports
		let ytd = TimeFrame(mode: .yearToDate, start: Month(year: 2026, month: 1), end: Self.november)
		let year = TimeFrame(mode: .slidingWindow, start: Month(year: 2025, month: 12), end: Self.november)
		let six = TimeFrame(mode: .slidingWindow, start: Month(year: 2026, month: 6), end: Self.november)
		let main: [(String, WidgetKind, (Int64, Int64, Int64, Int64), WidgetConfig)] = [
			("salary", .summary, (0, 0, 4, 2), WidgetConfig(name: "Salary YTD", timeFrame: ytd, conditions: [ReportCondition(field: "category", op: "is", value: "Paycheck")], summary: .sum)),
			("burn", .summary, (4, 0, 4, 2), WidgetConfig(name: "Average monthly spending", timeFrame: year, summary: .avgPerMonth)),
			("rate", .formula, (8, 0, 4, 2), WidgetConfig(name: "Savings rate", formula: "=1-(SPENT/INCOME)")),
			("worth", .netWorth, (0, 2, 6, 3), WidgetConfig(name: "Net worth", timeFrame: six, mode: "trend")),
			("flow", .cashFlow, (6, 2, 3, 3), WidgetConfig(name: "Cash flow, this month")),
			("spending", .spending, (9, 2, 3, 3), WidgetConfig(name: "Monthly spending", mode: "single-month")),
			("calendar", .calendar, (0, 5, 4, 4), WidgetConfig(name: "Calendar")),
			("age", .ageOfMoney, (4, 5, 4, 2), WidgetConfig(name: "Age of money", timeFrame: six)),
			("analysis", .budgetAnalysis, (8, 5, 4, 2), WidgetConfig(name: "Budget analysis", timeFrame: six)),
			("crossover", .crossover, (4, 7, 4, 2), WidgetConfig(name: "Crossover point")),
			("forecast", .balanceForecast, (8, 7, 4, 2), WidgetConfig(name: "Balance forecast")),
			("sankey", .sankey, (0, 9, 8, 3), WidgetConfig(name: "Where it went", timeFrame: TimeFrame(mode: .lastMonth, start: Month(year: 2026, month: 10), end: Month(year: 2026, month: 10)), mode: "spent")),
			("notes", .markdown, (8, 9, 4, 2), WidgetConfig(content: "## Budget notes\n\nPay the card **in full** every month.", textAlign: "left")),
			("future", .markdown, (8, 11, 4, 1), WidgetConfig()),
			("montecarlo", .monteCarlo, (0, 12, 6, 2), WidgetConfig(name: "Retirement odds")),
			("report", .customReport, (6, 12, 6, 3), WidgetConfig(reportID: "spending"))
		]
		var cards = main.map { id, kind, frame, config in
			self.card(id: id, kind: kind, frame: frame, config: config)
		}
		if let index = cards.firstIndex(where: { $0.id == "future" }) {
			cards[index] = DashboardCard(id: "future", kind: nil, rawKind: "future-widget-card", x: 8, y: 11, width: 4, height: 1, config: WidgetConfig(), content: .unknown)
		}
		sheets = [
			DashboardSheet(id: "main", name: "Main", cards: cards),
			DashboardSheet(id: "deep", name: "Deep dive", cards: [
				card(id: "dining", kind: .summary, frame: (0, 0, 4, 2), config: WidgetConfig(name: "Dining out YTD", timeFrame: ytd, conditions: [ReportCondition(field: "category", op: "is", value: "Dining out")], summary: .sum))
			])
		]
	}

	func all() -> [DashboardSheet] {
		lock.withLock { sheets }
	}

	func card(id: String, kind: WidgetKind, frame: (Int64, Int64, Int64, Int64), config: WidgetConfig) -> DashboardCard {
		DashboardCard(
			id: id, kind: kind, rawKind: kind.rawValue, x: frame.0, y: frame.1, width: frame.2, height: frame.3,
			config: config, content: content(kind, config), range: range(kind, config)
		)
	}

	func range(_ kind: WidgetKind, _ config: WidgetConfig) -> ReportPeriod? {
		guard kind.hasTimeFrame, ![.crossover, .balanceForecast, .monteCarlo].contains(kind) else { return nil }
		let november = Self.november
		guard let frame = config.timeFrame else {
			return kind == .cashFlow || kind == .calendar || kind == .spending
				? ReportPeriod(first: november, last: november)
				: ReportPeriod(first: november.adding(months: -5), last: november)
		}
		switch frame.mode {
		case .fixed, .full: return ReportPeriod(first: frame.start, last: frame.end)
		case .yearToDate: return ReportPeriod(first: Month(year: november.year, month: 1), last: november)
		case .lastYear: return ReportPeriod(first: Month(year: november.year - 1, month: 1), last: Month(year: november.year - 1, month: 12))
		case .lastMonth: return ReportPeriod(first: november.adding(months: -1), last: november.adding(months: -1))
		case .priorYearToDate: return ReportPeriod(first: Month(year: november.year - 1, month: 1), last: november.adding(months: -12))
		case .currentQuarter: return ReportPeriod(first: Month(year: 2026, month: 10), last: Month(year: 2026, month: 12))
		case .previousQuarter: return ReportPeriod(first: Month(year: 2026, month: 7), last: Month(year: 2026, month: 9))
		case .slidingWindow:
			let width = frame.start.months(through: frame.end)
			return ReportPeriod(first: november.adding(months: -width), last: november)
		}
	}

	func content(_ kind: WidgetKind, _ config: WidgetConfig) -> CardContent {
		let november = Self.november
		let months = ReportPeriod(first: november.adding(months: -5), last: november).months
		switch kind {
		case .netWorth: return .netWorth(reports.netWorth(first: months[0], last: november))
		case .cashFlow: return .cashFlow(income: 387_440, expenses: 310_566, netProfit: 76_874)
		case .spending: return .spending(month: november, spent: 310_566, compareMonth: november.adding(months: -1), compareSpent: 461_844)
		case .sankey: return .sankey(reports.sankey(first: "2026-10-01", last: "2026-10-31"))
		case .customReport:
			let id = config.reportID ?? "spending"
			return (try? reports.customReport(id)).map(CardContent.customReport) ?? .unknown
		case .markdown: return .markdown(config.content ?? "")
		case .summary:
			let base: Int64 = config.conditions.isEmpty ? 2_961_80 : (config.conditions.first?.value == "Dining out" ? 1_404_20 : 48_210_55)
			switch config.summary ?? .sum {
			case .sum: return .summary(.money(config.conditions.first?.value == "Dining out" ? -base : base))
			case .avgPerMonth: return .summary(.money(config.conditions.isEmpty ? -base : base / 11))
			case .avgPerTransact: return .summary(.money(-4_215))
			case .percentage: return .summary(.percent(18.4))
			}
		case .calendar:
			let days = (1...november.dayCount).map { day -> CalendarDay in
				let income: Int64 = day == 1 || day == 15 ? 193_720 : 0
				let expenses: Int64 = day > 13 ? 0 : Int64((day * 37) % 9) * 1_250
				return CalendarDay(day: day, income: income, expenses: expenses)
			}
			return .calendar(month: november, days: days)
		case .formula: return .formula(.percent(18.4))
		case .crossover:
			let span = ReportPeriod(first: november, last: november.adding(months: 35)).months
			return .crossover(
				months: span,
				expenses: span.indices.map { 380_000 + Int64($0) * 900 },
				income: span.indices.map { 120_000 + Int64($0) * 8_200 },
				crossover: span.last.map { $0.adding(months: 2) }
			)
		case .budgetAnalysis:
			return .budgetAnalysis(months: months, budgeted: [452_000, 455_000, 460_000, 458_000, 462_000, 465_000], spent: [438_820, 490_255, 610_490, 451_137, 461_844, 310_566])
		case .balanceForecast:
			let span = ReportPeriod(first: november, last: november.adding(months: 11)).months
			return .balanceForecast(span.enumerated().map { index, month in
				let middle = 812_440 + Int64(index) * 41_000
				return ForecastPoint(month: month, low: middle - Int64(index) * 18_000, middle: middle, high: middle + Int64(index) * 18_000)
			})
		case .ageOfMoney: return .ageOfMoney(days: 41, change: 3, series: [33, 35, 36, 38, 38, 41])
		case .monteCarlo:
			let span = ReportPeriod(first: Month(year: 2027, month: 1), last: Month(year: 2046, month: 1)).months.enumerated().filter { $0.offset % 12 == 0 }.map(\.element)
			return .monteCarlo(points: span.enumerated().map { index, month in
				let middle = 9_500_000 + Int64(index) * 2_400_000
				return ForecastPoint(month: month, low: middle - Int64(index) * 1_100_000, middle: middle, high: middle + Int64(index) * 1_600_000)
			}, successRate: 0.82, endAge: 90)
		}
	}

	func add(page: String, kind: WidgetKind, frame: (Int64, Int64, Int64, Int64), config: WidgetConfig) throws -> String {
		try lock.withLock {
			guard let index = sheets.firstIndex(where: { $0.id == page }) else { throw FfiError.NotFound(entity: "dashboard", id: page) }
			next += 1
			let id = "added-\(next)"
			sheets[index].cards.append(card(id: id, kind: kind, frame: frame, config: config))
			return id
		}
	}

	func remove(_ id: String) throws {
		try update(id) { _ in nil }
	}

	func configure(_ id: String, _ config: WidgetConfig) throws {
		try update(id) { card in
			guard let kind = card.kind else { return card }
			return self.card(id: card.id, kind: kind, frame: (card.x, card.y, card.width, card.height), config: config)
		}
	}

	func place(_ id: String, x: Int64, y: Int64, width: Int64, height: Int64) throws {
		try update(id) { card in
			var card = card
			(card.x, card.y, card.width, card.height) = (x, y, width, height)
			return card
		}
	}

	func createDashboard(_ name: String) -> String {
		lock.withLock {
			next += 1
			let id = "page-\(next)"
			sheets.append(DashboardSheet(id: id, name: name, cards: []))
			return id
		}
	}

	func renameDashboard(_ id: String, _ name: String) {
		lock.withLock {
			if let index = sheets.firstIndex(where: { $0.id == id }) { sheets[index].name = name }
		}
	}

	func deleteDashboard(_ id: String) {
		lock.withLock { sheets.removeAll { $0.id == id } }
	}

	private func update(_ id: String, _ change: (DashboardCard) -> DashboardCard?) throws {
		try lock.withLock {
			for page in sheets.indices {
				if let index = sheets[page].cards.firstIndex(where: { $0.id == id }) {
					if let changed = change(sheets[page].cards[index]) {
						sheets[page].cards[index] = changed
					} else {
						sheets[page].cards.remove(at: index)
					}
					return
				}
			}
			throw FfiError.NotFound(entity: "widget", id: id)
		}
	}
}
#endif
