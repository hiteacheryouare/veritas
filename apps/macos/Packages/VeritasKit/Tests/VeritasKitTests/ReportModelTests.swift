import CoreGraphics
import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

struct ReportPeriodTests {
	@Test func monthsStepAcrossYearsAndKnowTheirLastDay() {
		let november = Month(year: 2026, month: 11)
		#expect(november.adding(months: 3) == Month(year: 2027, month: 2))
		#expect(november.adding(months: -11) == Month(year: 2025, month: 12))
		#expect(Month(year: 2028, month: 2).lastDay == "2028-02-29")
		#expect(Month(year: 2100, month: 2).lastDay == "2100-02-28")
		#expect(november.firstDay == "2026-11-01")
		#expect(Month(date: "2026-06-30") == Month(year: 2026, month: 6))
		#expect(ReportPeriod.interval("2026-06") == "Jun")
		#expect(ReportPeriod.interval("2026-06-03") == "Jun 3")
		#expect(ReportPeriod.interval("2026") == "2026")
	}

	@Test func rangesReadAsTheMockupsWriteThem() {
		let six = MonthSpan.sixMonths.period(endingIn: Month(year: 2026, month: 11), current: ReportPeriod(first: Month(year: 2026, month: 11), last: Month(year: 2026, month: 11)))
		#expect(six.title == "June to November 2026")
		#expect(six.length == 6)
		let across = ReportPeriod(first: Month(year: 2025, month: 11), last: Month(year: 2026, month: 4))
		#expect(across.title == "November 2025 to April 2026")
		let quarter = SankeySpan.quarter.period(containing: Month(year: 2026, month: 11), current: six)
		#expect(quarter == ReportPeriod(first: Month(year: 2026, month: 10), last: Month(year: 2026, month: 12)))
		#expect(SankeySpan.quarter.title(of: quarter) == "Q4 2026")
		#expect(SankeySpan.month.title(of: ReportPeriod(first: Month(year: 2026, month: 10), last: Month(year: 2026, month: 10))) == "October 2026")
	}
}

struct ReportWordsTests {
	let none = ReportLabels(hidden: false, archived: false, deleted: false, closedGoal: false, needsReview: false)

	@Test func categoriesCarryTheirLabelsAsFredRuled() {
		var labels = none
		#expect(ReportWords.category("Rent", labels: labels) == "Rent")
		labels.hidden = true
		labels.archived = true
		#expect(ReportWords.category("Old Gym", labels: labels) == "Old Gym (hidden, archived)")
		labels.deleted = true
		labels.archived = false
		#expect(ReportWords.category("Old Gym", labels: labels) == "(deleted) Old Gym (hidden)")
		#expect(ReportWords.category("", labels: ReportLabels(hidden: false, archived: false, deleted: false, closedGoal: false, needsReview: true)) == "Uncategorized (needs review)")
		var goal = none
		goal.closedGoal = true
		#expect(ReportWords.category("Vet bill", labels: goal) == "Vet bill (closed goal)")
	}

	@Test func drilldownButtonsCountTheirRows() {
		#expect(ReportWords.showTransactions(0) == nil)
		#expect(ReportWords.showTransactions(1) == "Show 1 transaction")
		#expect(ReportWords.showTransactions(1_204) == "Show 1,204 transactions")
		let shortfall = SankeyNode(key: SankeyNodeKey(kind: .shortfall, id: nil), name: "", value: 100, labels: none, count: 0, column: 0)
		#expect(ReportWords.drillAction(for: shortfall) == nil)
		#expect(ReportWords.node(shortfall) == "Shortfall")
		let unallocated = SankeyNode(key: SankeyNodeKey(kind: .unallocated, id: nil), name: "", value: 100, labels: none, count: 9, column: 2)
		#expect(ReportWords.drillAction(for: unallocated) == "Show income and outflow totals")
		let uncategorized = SankeyNode(key: SankeyNodeKey(kind: .categoryGroup, id: nil), name: "", value: 100, labels: none, count: 2, column: 2)
		#expect(ReportWords.node(uncategorized) == "Uncategorized (needs review)")
	}
}

enum ReportSamples {
	static let none = ReportLabels(hidden: false, archived: false, deleted: false, closedGoal: false, needsReview: false)

	static func section(_ total: Int64, _ categories: [ReportCategory] = []) -> ReportSection {
		ReportSection(total: total, count: total == 0 ? 0 : 3, categories: categories)
	}

	static func category(_ id: String?, _ name: String, _ total: Int64) -> ReportCategory {
		ReportCategory(categoryId: id, name: name, groupId: "g", groupName: "Group", labels: none, total: total, count: 2)
	}

	static func column(_ month: Month?, first: String, last: String, income: Int64, expenses: Int64, unreviewed: Int64 = 0, categories: [ReportCategory] = []) -> NetProfitColumn {
		NetProfitColumn(
			first: first, last: last, month: month,
			income: section(income), expenses: section(expenses, categories),
			netProfit: ReportLine(total: income - expenses, count: 6),
			debtPaydown: section(0), savings: section(0),
			netCashFlow: ReportLine(total: income - expenses, count: 6),
			unreviewed: ReportLine(total: unreviewed, count: unreviewed == 0 ? 0 : 1)
		)
	}

	static func line(_ total: Int64) -> ReportLine { ReportLine(total: total, count: total == 0 ? 0 : 1) }

	static func flow(_ month: Month?, opening: Int64, closing: Int64, unreviewed: Int64 = 0) -> CashFlowColumn {
		CashFlowColumn(
			first: month?.firstDay ?? "2026-10-01", last: month?.lastDay ?? "2026-11-30", month: month,
			opening: opening, netProfit: line(closing - opening), debtPaydown: line(0), savings: line(0),
			netCashFlow: line(closing - opening - unreviewed), unreviewedTransfers: line(unreviewed),
			changeFromOperations: closing - opening, inTransit: line(0), startingBalances: line(0),
			reconciliationAdjustments: line(0), closing: closing
		)
	}

	static var settings: CustomReportSettings {
		CustomReportSettings(
			id: "r", name: "Spending", startDate: "2026-01", endDate: "2026-06", dateStatic: false,
			dateRange: "Last 6 months", totalMode: true, groupBy: .category, balanceType: .payment,
			sortBy: .descending, interval: .monthly, graphType: "BarGraph", showEmpty: false,
			showOffBudget: false, showHidden: false, showUncategorized: false, includeCurrent: false,
			trimIntervals: false, conditions: [], conditionsOr: false
		)
	}

	static var twoMonths: NetProfitReport {
		let october = Month(year: 2026, month: 10), november = Month(year: 2026, month: 11)
		return NetProfitReport(
			months: [
				column(october, first: "2026-10-01", last: "2026-10-31", income: 5_000_00, expenses: 5_600_00, categories: [category("rent", "Rent", 6_000_00), category(nil, "", -400_00)]),
				column(november, first: "2026-11-01", last: "2026-11-30", income: 5_000_00, expenses: -120_00, unreviewed: 100_00, categories: [category("rent", "Rent", -120_00)])
			],
			total: column(nil, first: "2026-10-01", last: "2026-11-30", income: 10_000_00, expenses: 5_480_00, unreviewed: 100_00, categories: [category("rent", "Rent", 5_880_00), category(nil, "", -400_00)])
		)
	}
}

final class ReportFakeSession: BudgetSession, @unchecked Sendable {
	let canExport = true
	var commands: [Command] = []
	var asked: [ReportPeriod] = []
	var report = ReportSamples.twoMonths
	var dashboard: [DashboardPage] = []

	func overview() async throws -> BudgetOverview {
		BudgetOverview(name: "Household", directory: URL(filePath: "/tmp/budget"), readOnly: nil, accountGroups: [])
	}
	func undo() async throws {}
	func redo() async throws {}
	func maintain() async throws {}
	func exportActualZip(to destination: URL) async throws -> [AuditFinding] { [] }
	func observeChanges(_ changed: @escaping @Sendable () -> Void) async throws {}
	func close() async {}
	func perform(_ command: Command) async throws {
		commands.append(command)
		if case .placeWidget(let id, let x, let y, let width, let height) = command {
			for page in dashboard.indices {
				for index in dashboard[page].widgets.indices where dashboard[page].widgets[index].id == id {
					(dashboard[page].widgets[index].x, dashboard[page].widgets[index].y) = (x, y)
					(dashboard[page].widgets[index].width, dashboard[page].widgets[index].height) = (width, height)
				}
			}
		}
	}
	func netProfit(first: Month, last: Month) async throws -> NetProfitReport {
		asked.append(ReportPeriod(first: first, last: last))
		return report
	}
	/// Holds the next read with what it read, until `releaseDashboards`, as a slow load would.
	var holdsNextDashboards = false
	private(set) var isHoldingDashboards = false
	private var heldDashboards: CheckedContinuation<Void, Never>?
	func dashboards(today: String) async throws -> [DashboardPage] {
		let read = dashboard
		if holdsNextDashboards {
			holdsNextDashboards = false
			await withCheckedContinuation { heldDashboards = $0; isHoldingDashboards = true }
		}
		return read
	}
	func releaseDashboards() {
		heldDashboards?.resume()
		heldDashboards = nil
	}
	var flows: [CashFlowColumn] = []
	func cashFlow(first: Month, last: Month) async throws -> [CashFlowColumn] { flows }
	func cashFlowStatement(first: String, last: String) async throws -> CashFlowColumn {
		ReportSamples.flow(nil, opening: flows.first?.opening ?? 0, closing: flows.last?.closing ?? 0, unreviewed: flows.map(\.unreviewedTransfers.total).reduce(0, +))
	}
	var saved: CustomReportView?
	func customReports() async throws -> [CustomReportItem] { [CustomReportItem(id: "r", name: "Spending", problem: nil)] }
	func customReport(_ id: String, today: String) async throws -> CustomReportView {
		guard let saved else { throw FfiError.NotFound(entity: "custom report", id: id) }
		return saved
	}
	func previewCustomReport(_ settings: CustomReportSettings, today: String) async throws -> CustomReportView {
		CustomReportView(settings: settings, data: nil, problem: "preview")
	}
	var edits: [String] = []
	var canEditDashboards: Bool { true }
	func addWidget(page: String, kind: WidgetKind, x: Int64, y: Int64, width: Int64, height: Int64, config: WidgetConfig) async throws -> String {
		edits.append("add \(kind.rawValue) on \(page) at \(x),\(y) \(width)x\(height) \(config.content ?? "")")
		return "new"
	}
	func removeWidget(_ id: String) async throws { edits.append("remove \(id)") }
	func setWidgetConfig(_ id: String, _ config: WidgetConfig) async throws { edits.append("config \(id) \(config.name ?? "")") }
	func createDashboard(named name: String) async throws -> String {
		edits.append("create \(name)")
		dashboard.append(DashboardPage(id: "p\(dashboard.count)", name: name, widgets: []))
		return "p\(dashboard.count - 1)"
	}
	func createReport(_ draft: ReportDraft) async throws -> String {
		edits.append("report \(draft.settings.name) \(draft.conditions.map { "\(FilterWords.field($0.field)) \(FilterWords.op($0.op, on: $0.field))" })")
		return "r2"
	}
}

final class ReportFakeLibrary: BudgetLibrary, @unchecked Sendable {
	let session: ReportFakeSession

	init(session: ReportFakeSession) {
		self.session = session
	}

	func newBudgetDirectory() -> URL { URL(filePath: "/tmp/budgets/new") }
	func open(directory: URL) async throws -> any BudgetSession { session }
	func importActualZip(_ zip: URL, into directory: URL) async throws -> any BudgetSession { session }
	func createBudget(_ budget: NewBudget, into directory: URL) async throws -> any BudgetSession { session }
	func currencies() async throws -> [CurrencyOption] { FakeAmounts.currencies }
	func evaluateAmount(_ text: String, style: NumberStyle) async throws -> Int64 { try FakeAmounts.evaluate(text) }
}

@MainActor
struct NetProfitModelTests {
	let session = ReportFakeSession()

	func open() async -> NetProfitModel {
		let workspace = WorkspaceModel(library: ReportFakeLibrary(session: session), format: MoneyFormat())
		await workspace.reopen(directory: URL(filePath: "/tmp/budget"))
		let model = NetProfitModel(workspace: workspace, thisMonth: Month(year: 2026, month: 11))
		await model.load()
		return model
	}

	@Test func theTableReadsTopToBottomAsST1() async {
		let model = await open()
		#expect(session.asked == [ReportPeriod(first: Month(year: 2026, month: 6), last: Month(year: 2026, month: 11))])
		#expect(model.rows.map(\.title) == ["Income", "Expenses", "Net Profit", "Debt paydown", "Savings", "Net cash flow", "Unreviewed transfers"])
		#expect(model.columns.map(model.heading) == ["Oct", "Nov", "6 months"])
		let expenses = model.rows[1]
		#expect(expenses.cells.map(\.amount) == [5_600_00, -120_00, 5_480_00])
		#expect(expenses.cells.allSatisfy { $0.tone == .neutral }, "refunds winning isn't a loss")
		#expect(model.rows[0].cells.allSatisfy { $0.tone == .income })
		#expect(model.rows[2].cells.map(\.tone) == [.overspent, .neutral, .neutral])
		#expect(expenses.cells[0].drill == .netProfit(first: "2026-10-01", last: "2026-10-31", line: .expenses, category: nil))
		#expect(expenses.cells[0].title == "Expenses, October 2026")
	}

	@Test func unreviewedTransfersOnlyShowWhenAnyColumnHasThem() async {
		session.report = NetProfitReport(
			months: [ReportSamples.column(Month(year: 2026, month: 11), first: "2026-11-01", last: "2026-11-30", income: 1, expenses: 1)],
			total: ReportSamples.column(nil, first: "2026-11-01", last: "2026-11-30", income: 1, expenses: 1)
		)
		let model = await open()
		#expect(!model.rows.map(\.title).contains("Unreviewed transfers"))
	}

	@Test func expandingALineListsItsCategoriesWithLabels() async {
		let model = await open()
		model.toggle(.expenses)
		let titles = model.rows.map(\.title)
		#expect(Array(titles.prefix(4)) == ["Income", "Expenses", "Rent", "Uncategorized (needs review)"])
		let uncategorized = model.rows[3]
		#expect(uncategorized.cells.map(\.amount) == [-400_00, 0, -400_00])
		#expect(uncategorized.cells[2].drill == .netProfit(first: "2026-10-01", last: "2026-11-30", line: .expenses, category: .uncategorized))
	}

	@Test func stepsByTheRangeAndStopsAtThisMonth() async {
		let model = await open()
		#expect(!model.canGoForward)
		await model.previous()
		#expect(model.period == ReportPeriod(first: Month(year: 2025, month: 12), last: Month(year: 2026, month: 5)))
		await model.choose(.year)
		#expect(model.period == ReportPeriod(first: Month(year: 2026, month: 1), last: Month(year: 2026, month: 12)))
	}
}

struct SankeyLayoutTests {
	let none = ReportSamples.none

	func node(_ kind: SankeyNodeKind, _ value: Int64, column: UInt8) -> SankeyNode {
		SankeyNode(key: SankeyNodeKey(kind: kind, id: nil), name: "", value: value, labels: none, count: 1, column: column)
	}

	@Test func ribbonsStackInsideTheirNodesAndColumnsShareOneScale() {
		let report = SankeyReport(
			first: "2026-10-01", last: "2026-10-31",
			nodes: [
				node(.incomeCategory, 300, column: 0), node(.incomeCategory, 100, column: 0),
				node(.income, 400, column: 1),
				node(.categoryGroup, 250, column: 2), node(.category, 250, column: 3), node(.unallocated, 150, column: 2)
			],
			flows: [
				SankeyFlow(from: 0, to: 2, value: 300), SankeyFlow(from: 1, to: 2, value: 100),
				SankeyFlow(from: 2, to: 3, value: 250), SankeyFlow(from: 3, to: 4, value: 250), SankeyFlow(from: 2, to: 5, value: 150)
			]
		)
		let layout = SankeyLayout(report: report, size: CGSize(width: 960, height: 600))
		let rect = { (index: Int) in layout.nodes[index].rect }
		#expect(rect(2).minX == rect(2).minX && rect(0).minX < rect(2).minX && rect(2).minX < rect(3).minX && rect(3).minX < rect(4).minX)
		#expect(abs(rect(2).height - (rect(0).height + rect(1).height)) < 0.5)
		#expect(abs(rect(2).height - (rect(3).height + rect(5).height)) < 0.5)
		let intoIncome = layout.ribbons.filter { $0.to == 2 }
		#expect(intoIncome[0].target.lowerBound == rect(2).minY)
		#expect(abs(intoIncome[1].target.upperBound - rect(2).maxY) < 0.5)
		#expect(layout.nodes[0].labelSide == .leading)
		#expect(layout.nodes[2].labelSide == .above)
		#expect(layout.nodes[5].labelSide == .trailing)
		#expect(layout.nodes.allSatisfy { $0.rect.maxY <= 600 })
	}
}

@MainActor
struct DashboardModelTests {
	@Test func v14CardsTakeTheirTitleRangeAndFigureFromTheCoresView() {
		var named = WidgetConfig.named("Retirement odds")
		named.timeFrame = WidgetTimeFrame(mode: "static", start: "2026-06", end: "2026-11")
		var percent = WidgetConfig.named(nil)
		percent.summary = "percentage"
		var filtered = WidgetConfig.named(nil)
		filtered.conditions = [RuleCondition(field: .payee, op: .is, value: .id(id: "employer"))]
		let range = WidgetRange(start: "2026-06", end: "2026-11", mode: "static")
		let cards = [
			DashboardWidget(id: "mc", kind: "monte-carlo-card", x: 0, y: 0, width: 4, height: 2, minWidth: 3, minHeight: 2, config: named, view: .monteCarlo(successPercent: 85.3, startAge: 45, endAge: 90, simulations: 5_000, chart: [])),
			DashboardWidget(id: "f", kind: "cash-flow-card", x: 0, y: 0, width: 4, height: 2, minWidth: 2, minHeight: 2, config: .named(nil), view: .cashFlow(range: range, income: 100, expense: -40, change: 60)),
			DashboardWidget(id: "s", kind: "summary-card", x: 0, y: 0, width: 4, height: 2, minWidth: 2, minHeight: 2, config: percent, view: .summary(start: "2026-01-01", end: "2026-11-30", value: 18.4, dividend: 184, divisor: 1000)),
			DashboardWidget(id: "m", kind: "custom-report", x: 0, y: 0, width: 4, height: 2, minWidth: 2, minHeight: 1, config: .named(nil), view: .missingReport),
			DashboardWidget(id: "u", kind: "future-card", x: 0, y: 0, width: 4, height: 2, minWidth: 1, minHeight: 1, config: .named(nil), view: .unknown(kind: "future-card")),
			DashboardWidget(id: "nw", kind: "net-worth-card", x: 0, y: 0, width: 4, height: 2, minWidth: 3, minHeight: 2, config: filtered, view: Self.netWorth(range)),
			DashboardWidget(id: "fx", kind: "formula-card", x: 0, y: 0, width: 4, height: 2, minWidth: 1, minHeight: 1, config: .named("Hidden"), view: .formula(formula: "=1", value: .number(value: 1), showTitle: false, fontSize: .dynamic, staticFontSize: 32)),
			DashboardWidget(id: "sp", kind: "spending-card", x: 0, y: 0, width: 4, height: 2, minWidth: 3, minHeight: 2, config: .named(nil), view: .spending(mode: .budget, compare: Month(year: 2026, month: 11), compareTo: Month(year: 2026, month: 10), averageFirst: nil, averageLast: nil, days: [], headlineDay: 17, difference: 0)),
			DashboardWidget(id: "nan", kind: "summary-card", x: 0, y: 0, width: 4, height: 2, minWidth: 2, minHeight: 2, config: .named(nil), view: .summary(start: "2026-01-01", end: "2026-11-30", value: .nan, dividend: 0, divisor: 0))
		].map(DashboardCard.init)
		#expect(cards[0].title == "Retirement odds")
		#expect(cards[0].range == nil, "a Monte Carlo card has no range")
		#expect(cards[1].title == "Cash flow")
		#expect(cards[1].range == ReportPeriod(first: Month(year: 2026, month: 6), last: Month(year: 2026, month: 11)), "the range is the core's, not the stored time frame")
		#expect(cards[2].summaryFigure == .percent(18.4))
		#expect(cards[2].minimumSize == (2, 2))
		#expect(cards[3].content == .missingReport)
		#expect(cards[4].content == .unknown(kind: "future-card"))
		#expect(cards[4].title == "future-card")
		#expect(cards[5].title == "Balance of matching rows", "ST-10.10: a filtered net worth card isn't the budget's net worth")
		#expect(cards[6].title == nil, "a formula card set to hide its title shows none, its name included")
		#expect(cards[7].title == "Monthly spending")
		#expect(cards[7].subtitle == "Compare November 2026 to budgeted")
		#expect(cards[8].summaryFigure == .undefined, "a NaN summary is drawn blank")
	}

	static func netWorth(_ range: WidgetRange) -> WidgetView {
		.netWorth(range: range, interval: .monthly, points: [], netWorth: 0, change: 0, lowest: nil, highest: nil, hasNegative: false, accounts: [], series: [])
	}

	@Test func aSlowLoadThatStartedBeforeAMoveNeverPutsTheCardBack() async throws {
		// The Reports journey's Move › Down: a load the screen started just before the
		// move read the old layout, finished last, and drew the card where it had been.
		let session = ReportFakeSession()
		session.dashboard = [DashboardPage(id: "main", name: "Main", widgets: [
			DashboardWidget(id: "w", kind: "net-worth-card", x: 0, y: 2, width: 6, height: 3, minWidth: 3, minHeight: 2, config: .named("Net worth"), view: .markdown(text: "", align: nil))
		])]
		let workspace = WorkspaceModel(library: ReportFakeLibrary(session: session), format: MoneyFormat())
		await workspace.reopen(directory: URL(filePath: "/tmp/budget"))
		let model = DashboardModel(workspace: workspace)
		await model.load()
		let widget = try #require(model.sheet?.cards.first)
		session.holdsNextDashboards = true
		let slow = Task { await model.load() }
		while !session.isHoldingDashboards { await Task.yield() }
		await model.move(widget, rows: 1)
		#expect(model.sheet?.cards.first?.y == 3)
		session.releaseDashboards()
		await slow.value
		#expect(model.sheet?.cards.first?.y == 3, "the older load's layout never lands after the move's")
	}

	@Test func placingAWidgetClampsToTheGridAndSendsOneCommand() async {
		let session = ReportFakeSession()
		session.dashboard = [DashboardPage(id: "main", name: "Main", widgets: [
			DashboardWidget(id: "w", kind: "net-worth-card", x: 0, y: 0, width: 6, height: 2, minWidth: 3, minHeight: 2, config: .named("Net worth"), view: .markdown(text: "", align: nil))
		])]
		let workspace = WorkspaceModel(library: ReportFakeLibrary(session: session), format: MoneyFormat())
		await workspace.reopen(directory: URL(filePath: "/tmp/budget"))
		let model = DashboardModel(workspace: workspace)
		await model.load()
		guard let widget = model.sheet?.cards.first else { Issue.record("no widget"); return }
		await model.move(widget, columns: 9)
		await model.resize(widget, columns: -5)
		await model.move(widget, columns: -1)
		#expect(session.commands == [
			.placeWidget(widgetId: "w", x: 6, y: 0, width: 6, height: 2),
			.placeWidget(widgetId: "w", x: 0, y: 0, width: 3, height: 2)
		])
	}
}

@MainActor
struct StandardReportTests {
	let session = ReportFakeSession()

	func open(_ kind: StandardReport) async -> StandardReportModel {
		let workspace = WorkspaceModel(library: ReportFakeLibrary(session: session), format: MoneyFormat())
		await workspace.reopen(directory: URL(filePath: "/tmp/budget"))
		let model = StandardReportModel(workspace: workspace, kind: kind, thisMonth: Month(year: 2026, month: 11))
		await model.load()
		return model
	}

	@Test func cashFlowHidesReconcilingLinesThatAreZero() async {
		session.flows = [
			ReportSamples.flow(Month(year: 2026, month: 10), opening: 100, closing: 300),
			ReportSamples.flow(Month(year: 2026, month: 11), opening: 300, closing: 250)
		]
		let quiet = await open(.cashFlow)
		#expect(quiet.cashFlowRows.map(\.title) == ["Budget accounts balance", "Net Profit", "Debt paydown", "Savings contributions", "Net cash flow", "Budget accounts balance"])
		#expect(quiet.cashFlowRows.last?.values == [300, 250, 250])
		#expect(quiet.cashFlowHeadings == ["Oct", "Nov", "6 months"])

		let tones = Dictionary(uniqueKeysWithValues: quiet.cashFlowRows.map { ($0.id.split(separator: ".").last.map(String.init) ?? $0.id, $0.tones) })
		#expect(tones["netCashFlow"] == [.neutral, .overspent, .neutral], "ST-10.4: a negative Net cash flow is a loss")
		#expect(tones["closing"]?.allSatisfy { $0 == .neutral } == true, "a balance is never a loss")

		session.flows[1] = ReportSamples.flow(Month(year: 2026, month: 11), opening: 300, closing: 250, unreviewed: 40)
		let busy = await open(.cashFlow)
		#expect(busy.cashFlowRows.map(\.title).contains("Unreviewed transfers"))
		#expect(busy.cashFlowRows.map(\.title).contains("Change in budget cash from operations"))
	}

	@Test func aNegativeNetWorthKeepsTheNeutralTone() async {
		let model = await open(.netWorth)
		#expect(model.netWorthRows.allSatisfy { $0.tones.allSatisfy { $0 == .neutral } })
		#expect(model.netWorthRows.map(\.title) == ["Assets", "Liabilities", "Net worth"])
	}

	@Test func spendingByCategoryKeepsRefundsAndAddsUpToExpenses() async {
		let model = await open(.spendingByCategory)
		#expect(model.spending.map(\.title) == ["Rent", "Uncategorized (needs review)"])
		#expect(model.spending.map(\.amount) == [5_880_00, -400_00])
		#expect(model.spendingTotal == model.spending.map(\.amount).reduce(0, +))
	}

	@Test func editingACustomReportPreviewsThenSavesItsSettings() async {
		session.saved = CustomReportView(settings: ReportSamples.settings, data: nil, problem: nil)
		let workspace = WorkspaceModel(library: ReportFakeLibrary(session: session), format: MoneyFormat())
		await workspace.reopen(directory: URL(filePath: "/tmp/budget"))
		let model = CustomReportsModel(workspace: workspace, thisMonth: Month(year: 2026, month: 11))
		await model.load()
		await model.show(.custom(id: "r"))
		await model.edit()
		model.draft?.settings.groupBy = .payee
		await model.refreshPreview()
		#expect(model.shown?.problem == "preview")
		#expect(await model.save())
		var expected = ReportSamples.settings
		expected.groupBy = .payee
		#expect(session.commands == [.saveCustomReport(settings: expected)])
		#expect(!model.isEditing)
	}
}

@MainActor
struct CustomReportRangeTests {
	@Test func aFixedRangeKeepsTheShapeTheReportStores() async {
		let session = ReportFakeSession()
		session.saved = CustomReportView(settings: ReportSamples.settings, data: nil, problem: nil)
		let workspace = WorkspaceModel(library: ReportFakeLibrary(session: session), format: MoneyFormat())
		await workspace.reopen(directory: URL(filePath: "/tmp/budget"))
		let model = CustomReportsModel(workspace: workspace, thisMonth: Month(year: 2026, month: 11))
		await model.show(.custom(id: "r"))
		await model.edit()
		model.setFixedRange(first: Month(year: 2026, month: 3), last: Month(year: 2026, month: 1))
		#expect(model.draft?.settings.dateStatic == true)
		#expect(model.draft?.settings.startDate == "2026-01")
		#expect(model.draft?.settings.endDate == "2026-03")
		model.draft?.settings.startDate = "2026-01-01"
		model.setFixedRange(first: Month(year: 2026, month: 2), last: Month(year: 2026, month: 2))
		#expect(model.draft?.settings.endDate == "2026-02-28")
		model.setLiveRange("Last 3 months")
		#expect(model.draft?.settings.dateStatic == false)
		#expect(model.draft?.settings.dateRange == "Last 3 months")
	}
}

@MainActor
struct DashboardEditingTests {
	let session = ReportFakeSession()

	func open() async -> DashboardModel {
		session.dashboard = [DashboardPage(id: "main", name: "Main", widgets: [
			DashboardWidget(id: "w", kind: "net-worth-card", x: 2, y: 0, width: 6, height: 2, minWidth: 3, minHeight: 2, config: .named("Net worth"), view: DashboardModelTests.netWorth(WidgetRange(start: "2026-06", end: "2026-11", mode: "sliding-window"))),
			DashboardWidget(id: "s", kind: "sankey-card", x: 0, y: 2, width: 8, height: 3, minWidth: 3, minHeight: 3, config: .named(nil), view: DashboardModelTests.netWorth(WidgetRange(start: "2026-06", end: "2026-11", mode: "sliding-window")))
		])]
		let workspace = WorkspaceModel(library: ReportFakeLibrary(session: session), format: MoneyFormat())
		await workspace.reopen(directory: URL(filePath: "/tmp/budget"))
		let model = DashboardModel(workspace: workspace)
		await model.load()
		return model
	}

	@Test func aNewWidgetGoesUnderEverythingAtActualsDefaultSize() async {
		let model = await open()
		#expect(model.canEdit)
		await model.add(.summary)
		await model.add(.sankey)
		await model.add(.markdown)
		#expect(session.edits == [
			"add summary-card on main at 0,5 4x2 ",
			"add sankey-card on main at 0,5 4x3 ",
			"add markdown-card on main at 0,5 4x2 ## Notes"
		])
	}

	@Test func dragginALeadingEdgeKeepsTheTrailingEdgeWhereItWas() async {
		let model = await open()
		guard let card = model.sheet?.cards.first else { Issue.record("no card"); return }
		await model.resize(card, leading: -2)
		await model.resize(card, top: 0, bottom: 1)
		await model.resize(card, leading: 4)
		#expect(session.commands == [
			.placeWidget(widgetId: "w", x: 0, y: 0, width: 8, height: 2),
			.placeWidget(widgetId: "w", x: 2, y: 0, width: 6, height: 3),
			.placeWidget(widgetId: "w", x: 5, y: 0, width: 3, height: 2)
		])
	}

	@Test func resizingPastTheRightEdgeClampsTheWidthAndKeepsTheCardInPlace() async {
		let model = await open()
		guard let card = model.sheet?.cards.first else { Issue.record("no card"); return }
		await model.resize(card, columns: 9)
		await model.resize(card, trailing: 9, bottom: 1)
		await model.resize(card, leading: -9)
		#expect(session.commands == [
			.placeWidget(widgetId: "w", x: 2, y: 0, width: 10, height: 2),
			.placeWidget(widgetId: "w", x: 2, y: 0, width: 10, height: 3),
			.placeWidget(widgetId: "w", x: 0, y: 0, width: 8, height: 2)
		])
	}

	@Test func aNewDashboardIsShownOnceMade() async {
		let model = await open()
		await model.createDashboard(named: "  Taxes ")
		#expect(session.edits == ["create Taxes"])
		#expect(model.sheet?.name == "Taxes")
		await model.createDashboard(named: "  ")
		#expect(session.edits.count == 1, "a blank name makes nothing")
	}

	@Test func aNewReportIsCreatedOnlyOnSave() async {
		let workspace = WorkspaceModel(library: ReportFakeLibrary(session: session), format: MoneyFormat())
		await workspace.reopen(directory: URL(filePath: "/tmp/budget"))
		let model = CustomReportsModel(workspace: workspace, thisMonth: Month(year: 2026, month: 11))
		await model.startNew()
		#expect(model.isCreating)
		#expect(session.edits.isEmpty)
		model.draft?.settings.name = "Dining"
		model.draft?.conditions = [RuleCondition(field: .category, op: .is, value: .id(id: "dining"))]
		#expect(await model.save())
		#expect(session.edits == ["report Dining [\"Category is\"]"])
		#expect(model.choice == .custom(id: "r2"))
	}
}

extension WidgetConfig {
	static func named(_ name: String?) -> WidgetConfig {
		var config = WidgetConfig.empty
		config.name = name
		return config
	}
}

/// ST-4: an equity statement that doesn't close shows the core's closing
/// equity and says how far off the lines are, below the column.
@MainActor
struct OwnersEquityLinesTests {
	static func equity(unexplained: Int64) -> OwnersEquity {
		let none = ReportLine(total: 0, count: 0)
		return OwnersEquity(
			first: "2026-01-01", last: "2026-01-31", opening: 100_000,
			netIncome: ReportLine(total: 5_000, count: 1), capitalizedTransfers: none, startingBalances: none,
			reconciliationAdjustments: none, revaluations: none, unpairedTransfers: none, inTransit: none,
			closing: 106_000, unexplained: unexplained
		)
	}

	@Test func theGapIsNeverALineAndClosingIsTheCores() {
		let lines = StatementsModel.lines(Self.equity(unexplained: 1_000))
		#expect(lines.map(\.id) == ["opening", "netIncome", "capitalizedTransfers", "closing"])
		#expect(lines.last?.amount == 106_000)
	}

	@Test func theNoticeNamesTheDirectionByTheCoresSign() {
		let format = MoneyFormat()
		func document(_ unexplained: Int64?) -> StatementDocument {
			StatementDocument(kind: .ownersEquity, entity: "Home", basis: "January 2026", lines: [], unexplained: unexplained)
		}
		#expect(document(1_000).notice(format: format) == "The lines above are $10.00 short of closing equity. Check the audit.")
		#expect(document(-1_000).notice(format: format) == "The lines above are $10.00 over closing equity. Check the audit.")
		#expect(document(nil).notice(format: format) == nil)
		#expect(document(-50_050).notice(format: .yen) == "The lines above are ¥501 over closing equity. Check the audit.")
	}
}
