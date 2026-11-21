#if DEBUG
import Foundation
import VeritasCore
import VeritasKit

/// Dashboards for the stub budget, with every widget type and made-up figures,
/// editable in memory so UI tests can add, move, configure and remove.
private func meta(
	name: String? = nil, timeFrame: WidgetTimeFrame? = nil, conditions: [RuleCondition] = [], mode: String? = nil,
	summary: SummaryKind? = nil, content: String? = nil, textAlign: String? = nil, formula: String? = nil, reportId: String? = nil
) -> WidgetConfig {
	WidgetConfig(
		name: name, timeFrame: timeFrame, conditions: conditions, conditionsOr: false, mode: mode, summary: summary?.rawValue,
		content: content, textAlign: textAlign, formula: formula, reportId: reportId
	)
}

final class StubDashboards: @unchecked Sendable {
	static let november = Month(year: 2026, month: 11)

	private let lock = NSLock()
	private let reports: StubReports
	private var sheets: [DashboardSheet] = []
	private var next = 0

	init(reports: StubReports) {
		self.reports = reports
		let ytd = WidgetTimeFrame(mode: .yearToDate, start: Month(year: 2026, month: 1), end: Self.november)
		let year = WidgetTimeFrame(mode: .slidingWindow, start: Month(year: 2025, month: 12), end: Self.november)
		let six = WidgetTimeFrame(mode: .slidingWindow, start: Month(year: 2026, month: 6), end: Self.november)
		let main: [(String, WidgetKind, (Int64, Int64, Int64, Int64), WidgetConfig)] = [
			("salary", .summary, (0, 0, 4, 2), meta(name: "Salary YTD", timeFrame: ytd, conditions: [RuleCondition(field: .category, op: .is, value: .id(id: "paycheck"))], summary: .sum)),
			("burn", .summary, (4, 0, 4, 2), meta(name: "Average month", timeFrame: year, summary: .avgPerMonth)),
			("rate", .formula, (8, 0, 4, 2), meta(name: "Savings rate", formula: "=1-(SPENT/INCOME)")),
			("worth", .netWorth, (0, 2, 6, 3), meta(name: "Net worth", timeFrame: six, mode: "trend")),
			("flow", .cashFlow, (6, 2, 3, 3), meta(name: "Cash flow, this month")),
			("spending", .spending, (9, 2, 3, 3), meta(mode: "single-month")),
			("calendar", .calendar, (0, 5, 4, 4), meta(name: "Calendar")),
			("age", .ageOfMoney, (4, 5, 4, 2), meta(name: "Age of money", timeFrame: six)),
			("analysis", .budgetAnalysis, (8, 5, 4, 2), meta(name: "Budget analysis", timeFrame: six)),
			("crossover", .crossover, (4, 7, 4, 2), meta(name: "Crossover point")),
			("forecast", .balanceForecast, (8, 7, 4, 2), meta()),
			("sankey", .sankey, (0, 9, 8, 3), meta( timeFrame: WidgetTimeFrame(mode: .lastMonth, start: Month(year: 2026, month: 10), end: Month(year: 2026, month: 10)), mode: "spent")),
			("notes", .markdown, (8, 9, 4, 2), meta(content: "## Budget notes\n\nPay the card **in full** every month.", textAlign: "left")),
			("future", .markdown, (8, 11, 4, 1), meta()),
			("montecarlo", .monteCarlo, (0, 12, 6, 2), meta()),
			("report", .customReport, (6, 12, 6, 3), meta(reportId: "spending"))
		]
		var cards = main.map { id, kind, frame, config in
			self.card(id: id, kind: kind, frame: frame, config: config)
		}
		if let index = cards.firstIndex(where: { $0.id == "future" }) {
			cards[index] = DashboardCard(id: "future", kind: nil, rawKind: "future-widget-card", x: 8, y: 11, width: 4, height: 1, config: .empty, content: .unknown(kind: "future-widget-card"))
		}
		sheets = [
			DashboardSheet(id: "main", name: "Main", cards: cards),
			DashboardSheet(id: "deep", name: "Deep dive", cards: [
				card(id: "dining", kind: .summary, frame: (0, 0, 4, 2), config: meta(name: "Dining out YTD", timeFrame: ytd, conditions: [RuleCondition(field: .category, op: .is, value: .id(id: "dining"))], summary: .sum))
			])
		]
	}

	func all() -> [DashboardSheet] {
		lock.withLock { sheets }
	}

	func card(id: String, kind: WidgetKind, frame: (Int64, Int64, Int64, Int64), config: WidgetConfig) -> DashboardCard {
		DashboardCard(
			id: id, kind: kind, rawKind: kind.rawValue, x: frame.0, y: frame.1, width: frame.2, height: frame.3,
			config: config, content: content(kind, config)
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
		let start = frame.startMonth ?? november
		let end = frame.endMonth ?? november
		switch frame.frameMode {
		case .fixed, .full: return ReportPeriod(first: start, last: end)
		case .yearToDate: return ReportPeriod(first: Month(year: november.year, month: 1), last: november)
		case .lastYear: return ReportPeriod(first: Month(year: november.year - 1, month: 1), last: Month(year: november.year - 1, month: 12))
		case .lastMonth: return ReportPeriod(first: november.adding(months: -1), last: november.adding(months: -1))
		case .priorYearToDate: return ReportPeriod(first: Month(year: november.year - 1, month: 1), last: november.adding(months: -12))
		case .currentQuarter: return ReportPeriod(first: Month(year: 2026, month: 10), last: Month(year: 2026, month: 12))
		case .previousQuarter: return ReportPeriod(first: Month(year: 2026, month: 7), last: Month(year: 2026, month: 9))
		case .slidingWindow:
			let width = start.months(through: end)
			return ReportPeriod(first: november.adding(months: -width), last: november)
		}
	}

	func widgetRange(_ kind: WidgetKind, _ config: WidgetConfig) -> WidgetRange {
		let period = range(kind, config) ?? ReportPeriod(first: Self.november, last: Self.november)
		return WidgetRange(start: String(period.first.firstDay.prefix(7)), end: String(period.last.firstDay.prefix(7)), mode: config.timeFrame?.mode ?? "sliding-window")
	}

	func content(_ kind: WidgetKind, _ config: WidgetConfig) -> WidgetView {
		let november = Self.november
		let months = ReportPeriod(first: november.adding(months: -5), last: november).months
		switch kind {
		case .netWorth:
			let worth = reports.netWorth(first: months[0].adding(months: -1), last: november)
			let points = worth.indices.map { index in
				NetWorthCardPoint(
					start: worth[index].month.firstDay, netWorth: worth[index].netWorth, assets: worth[index].assets, debt: worth[index].liabilities,
					change: index == 0 ? 0 : worth[index].netWorth - worth[index - 1].netWorth
				)
			}
			let series = [
				NetWorthCardSeries(accountId: "visa", name: "Visa", points: points.map { -$0.debt }, color: 1),
				NetWorthCardSeries(accountId: "checking", name: "Checking", points: points.map(\.assets), color: 0)
			]
			return .netWorth(
				range: widgetRange(kind, config), interval: .monthly, points: points, netWorth: points.last?.netWorth ?? 0,
				change: (points.last?.netWorth ?? 0) - (points.first?.netWorth ?? 0), lowest: points.map(\.netWorth).min(), highest: points.map(\.netWorth).max(),
				hasNegative: false, accounts: ["checking", "visa"], series: series
			)
		case .cashFlow:
			return .cashFlow(range: widgetRange(kind, config), income: 387_440, expense: -310_566, change: 76_874)
		case .spending:
			let days = (1...28).map { day -> SpendingCardDay in
				let spent = Int64(day) * -11_090
				return SpendingCardDay(day: UInt32(day), compare: day <= 17 ? spent : nil, compareTo: Int64(day) * -16_494, average: Int64(day) * -14_200, budget: Double(day) * -16_100)
			}
			return .spending(mode: .singleMonth, compare: november, compareTo: november.adding(months: -1), averageFirst: nil, averageLast: nil, days: days, headlineDay: 17, difference: -91_852)
		case .sankey:
			let nodes = [
				SankeyCardNode(key: "employer", label: .name(name: "Employer"), percentage: "100%", color: .palette(index: 0)),
				SankeyCardNode(key: "paycheck", label: .name(name: "Paycheck"), percentage: "100%", color: .palette(index: 0)),
				SankeyCardNode(key: "accounts", label: .allAccounts, percentage: "100%", color: .blue),
				SankeyCardNode(key: "housing", label: .name(name: "Housing and utilities"), percentage: "55.1%", color: .palette(index: 1)),
				SankeyCardNode(key: "everyday", label: .name(name: "Everyday"), percentage: "21.9%", color: .palette(index: 2)),
				SankeyCardNode(key: "everyday-other", label: .other, percentage: "8.8%", color: .gray),
				SankeyCardNode(key: "rent", label: .name(name: "Rent"), percentage: "51.2%", color: .palette(index: 1)),
				SankeyCardNode(key: "groceries", label: .name(name: "Groceries"), percentage: "13.1%", color: .palette(index: 2))
			]
			let links = [
				SankeyCardLink(source: 0, target: 1, value: 322_400, folded: [], color: nil),
				SankeyCardLink(source: 1, target: 2, value: 322_400, folded: [], color: nil),
				SankeyCardLink(source: 2, target: 3, value: 177_600, folded: [], color: nil),
				SankeyCardLink(source: 2, target: 4, value: 70_590, folded: [], color: nil),
				SankeyCardLink(source: 3, target: 6, value: 165_000, folded: [], color: nil),
				SankeyCardLink(source: 4, target: 7, value: 42_210, folded: [], color: nil),
				SankeyCardLink(source: 4, target: 5, value: 28_380, folded: [SankeyFolded(name: "Dining out", value: 21_475), SankeyFolded(name: "Household", value: 6_905)], color: nil)
			]
			let shown = Month(year: 2026, month: 10)
			return .sankey(range: widgetRange(kind, config), first: shown, last: shown, mode: config.mode == "budgeted" ? .budgeted : .categorized, showPercentages: false, nodes: nodes, links: links)
		case .customReport:
			let id = config.reportId ?? "spending"
			return (try? reports.customReport(id)).map { WidgetView.customReport(report: $0) } ?? .missingReport
		case .markdown:
			return .markdown(text: config.content ?? "", align: config.textAlign)
		case .summary:
			let period = widgetRange(kind, config)
			let dining = config.conditions.first?.value == .id(id: "dining")
			let base: Double = config.conditions.isEmpty ? 2_961_80 : (dining ? 1_404_20 : 48_210_55)
			let value: Double = switch config.summaryKind {
			case .sum: dining ? -base : base
			case .avgPerMonth: config.conditions.isEmpty ? -base : base / 11
			case .avgPerYear: config.conditions.isEmpty ? -base * 12 : base
			case .avgPerTransact: -4_215
			case .percentage: 18.4
			}
			return .summary(start: period.start, end: period.end, value: value, dividend: 0, divisor: 0)
		case .calendar:
			let days = (1...november.dayCount).map { day -> CalendarDayCell in
				let income: Int64 = day == 1 || day == 15 ? 193_720 : 0
				let expense: Int64 = day > 13 ? 0 : Int64((day * 37) % 9) * 1_250
				return CalendarDayCell(date: String(format: "2026-11-%02d", day), income: income, expense: expense, incomeSize: income > 0 ? 50 : 0, expenseSize: Double(expense) / 100)
			}
			return .calendar(months: [CalendarCardMonth(start: november.firstDay, end: november.lastDay, days: days, totalIncome: 387_440, totalExpense: 211_250)])
		case .formula:
			return .formula(formula: config.formula ?? "", value: .number(value: 0.184), showTitle: true, fontSize: .dynamic, staticFontSize: 32)
		case .crossover:
			let span = ReportPeriod(first: november, last: november.adding(months: 35)).months
			let points = span.indices.map { index in
				CrossoverCardPoint(month: span[index], investmentIncome: 120_000 + Double(index) * 8_200, expenses: 380_000 + Double(index) * 900, nestEgg: 0, adjustedExpenses: nil, projection: index > 0)
			}
			return .crossover(
				start: november, end: span[span.count - 1], points: points, crossover: span.last.map { $0.adding(months: 2) },
				lastKnownBalance: 0, lastKnownMonthlyIncome: 0, lastKnownMonthlyExpenses: 0, historicalReturn: nil, yearsToRetire: 3.2,
				targetMonthlyIncome: nil, targetNestEgg: nil
			)
		case .budgetAnalysis:
			let budgeted: [Int64] = [452_000, 455_000, 460_000, 458_000, 462_000, 465_000]
			let spent: [Int64] = [438_820, 490_255, 610_490, 451_137, 461_844, 310_566]
			let points = months.indices.map { BudgetAnalysisPoint(month: months[$0], budgeted: budgeted[$0], spent: -spent[$0], balance: budgeted[$0] - spent[$0], overspendingAdjustment: 0) }
			return .budgetAnalysis(start: months[0].firstDay, end: november.lastDay, months: points, totalBudgeted: 2_752_000, totalSpent: -2_763_112, totalOverspendingAdjustment: 0, finalOverspendingAdjustment: 0)
		case .balanceForecast:
			let points = (1...30).map { day -> ForecastCardPoint in
				let paid: Int64 = day >= 15 ? 300_000 : 0
				let rent: Int64 = day >= 2 ? -165_000 : 0
				return ForecastCardPoint(date: String(format: "2026-11-%02d", day), balance: 812_440 + rent + paid - Int64(day) * 2_100)
			}
			let low = points.min { $0.balance < $1.balance }
			return .balanceForecast(
				range: WidgetRange(start: november.firstDay, end: november.lastDay, mode: "sliding-window"), source: .schedules, granularity: .daily,
				zeroBased: false, hasForecast: true, points: points, low: low == points.last ? nil : low, scheduled: 3, filtered: !config.conditions.isEmpty, today: nil
			)
		case .ageOfMoney:
			let points = [33, 35, 36, 38, 38, 41].enumerated().map { AgePoint(period: months[$0.offset].firstDay, days: Int64($0.element)) }
			return .ageOfMoney(start: months[0].firstDay, end: november.lastDay, granularity: .monthly, points: points, currentDays: 41, trend: .up, insufficientData: false)
		case .monteCarlo:
			let chart = (0...20).map { year -> MonteCarloCardPoint in
				let middle = 9_500_000 + Double(year) * 2_400_000
				return MonteCarloCardPoint(
					year: UInt32(year), age: 45 + Double(year), p10: middle - Double(year) * 1_100_000, p25: middle - Double(year) * 550_000,
					p50: middle, p75: middle + Double(year) * 800_000, p90: middle + Double(year) * 1_600_000
				)
			}
			return .monteCarlo(successPercent: 85.3, startAge: 45, endAge: 90, simulations: 5_000, chart: chart)
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
