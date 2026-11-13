import Foundation
import Observation
import VeritasCore

/// What the Custom reports page shows: one of the standard reports, or a
/// report saved in Actual.
public enum ReportChoice: Hashable, Sendable {
	case standard(StandardReport)
	case custom(id: String)
}

/// Actual's graph types, as its report editor names them.
public enum ReportGraph: String, CaseIterable, Sendable {
	case table = "TableGraph"
	case bar = "BarGraph"
	case stackedBar = "StackedBarGraph"
	case line = "LineGraph"
	case area = "AreaGraph"
	case donut = "DonutGraph"

	public var title: String {
		switch self {
		case .table: "Table"
		case .bar: "Bar"
		case .stackedBar: "Stacked bar"
		case .line: "Line"
		case .area: "Area"
		case .donut: "Donut"
		}
	}

	/// Actual offers these graphs for a total report; the others draw intervals.
	public var drawsTotals: Bool {
		switch self {
		case .table, .bar, .donut: true
		case .stackedBar, .line, .area: false
		}
	}

	public init(_ stored: String?) {
		self = stored.flatMap(ReportGraph.init(rawValue:)) ?? .bar
	}
}

/// The live date ranges Actual's report editor offers (`ReportOptions.dateRange`).
public enum ReportDateRanges {
	public static let names = [
		"This week", "Last week", "This month", "Last month", "Last 3 months", "Last 6 months",
		"Last 12 months", "Year to date", "Last year", "Prior year to date", "All time"
	]
}

public extension ReportGrouping {
	var title: String {
		switch self {
		case .category: "Category"
		case .group: "Group"
		case .categoryGroup: "Category group"
		case .payee: "Payee"
		case .account: "Account"
		case .tag: "Tag"
		case .interval: "Interval"
		}
	}
}

public extension ReportBalanceKind {
	var title: String {
		switch self {
		case .payment: "Payment"
		case .deposit: "Deposit"
		case .net: "Net"
		case .netPayment: "Net payment"
		case .netDeposit: "Net deposit"
		case .budgeted: "Budgeted"
		}
	}
}

public extension ReportIntervalUnit {
	var title: String {
		switch self {
		case .daily: "Daily"
		case .weekly: "Weekly"
		case .monthly: "Monthly"
		case .yearly: "Yearly"
		}
	}
}

public extension ReportSortOrder {
	var title: String {
		switch self {
		case .ascending: "Ascending"
		case .descending: "Descending"
		case .name: "Name"
		case .budget: "Budget"
		}
	}
}

/// The custom report viewer and editor (R-REP-4). Edits preview live and
/// save back in Actual's format; filter conditions stay as Actual wrote them.
@MainActor
@Observable
public final class CustomReportsModel {
	public private(set) var items: [CustomReportItem] = []
	public private(set) var choice: ReportChoice = .standard(.netWorth)
	public private(set) var view: CustomReportView?
	/// The report being edited, filters included, with its live preview.
	public var draft: ReportDraft?
	/// True while the draft is a new report that isn't saved yet.
	public private(set) var isCreating = false
	public private(set) var preview: CustomReportView?

	@ObservationIgnored let workspace: WorkspaceModel
	@ObservationIgnored public let standard: StandardReportModel

	public init(workspace: WorkspaceModel, thisMonth: Month? = nil) {
		self.workspace = workspace
		standard = StandardReportModel(workspace: workspace, thisMonth: thisMonth)
	}

	public var format: MoneyFormat { workspace.format }
	public var isReadOnly: Bool { workspace.readOnly != nil }
	public var isEditing: Bool { draft != nil }

	public func load() async {
		if let items = await workspace.report({ try await $0.customReports() }) {
			self.items = items
		}
		await show(choice)
	}

	public func show(_ choice: ReportChoice) async {
		if self.choice != choice { draft = nil }
		self.choice = choice
		switch choice {
		case .standard(let kind):
			view = nil
			await standard.show(kind)
		case .custom(let id):
			let today = WorkspaceModel.today()
			view = await workspace.report { try await $0.customReport(id, today: today) }
		}
	}

	public var canEditFilters: Bool { workspace.canEditDashboards }
	public var canCreate: Bool { !isReadOnly && workspace.canEditDashboards }

	public func edit() async {
		guard case .custom(let id) = choice else { return }
		guard let draft = await workspace.report({ try await $0.reportDraft(id) }) else { return }
		isCreating = false
		self.draft = draft
		preview = view
	}

	/// A new report with Actual's defaults: spending by category over the
	/// last six months. Nothing is written until Save.
	public func startNew() async {
		let settings = CustomReportSettings(
			id: "", name: "New report", startDate: "", endDate: "", dateStatic: false, dateRange: "Last 6 months",
			totalMode: true, groupBy: .category, balanceType: .payment, sortBy: .descending, interval: .monthly,
			graphType: ReportGraph.bar.rawValue, showEmpty: false, showOffBudget: false, showHidden: false,
			showUncategorized: false, includeCurrent: false, trimIntervals: false, conditionCount: 0
		)
		isCreating = true
		draft = ReportDraft(settings: settings)
		preview = nil
		await refreshPreview()
	}

	/// A fixed range, written in the shape the report already stores:
	/// `YYYY-MM`, or whole dates when Actual saved those.
	public func setFixedRange(first: Month, last: Month) {
		guard var draft else { return }
		let period = ReportPeriod(first: first, last: last)
		let usesDays = draft.settings.startDate.count > 7
		draft.settings.dateStatic = true
		draft.settings.startDate = usesDays ? period.first.firstDay : String(period.first.firstDay.prefix(7))
		draft.settings.endDate = usesDays ? period.last.lastDay : String(period.last.firstDay.prefix(7))
		self.draft = draft
	}

	public func setLiveRange(_ name: String) {
		draft?.settings.dateStatic = false
		draft?.settings.dateRange = name
	}

	public func cancelEditing() {
		draft = nil
		preview = nil
		isCreating = false
	}

	public func refreshPreview() async {
		guard let draft else { return }
		let today = WorkspaceModel.today()
		if let preview = await workspace.report({ try await $0.previewReport(draft, today: today) }), self.draft == draft {
			self.preview = preview
		}
	}

	@discardableResult
	public func save() async -> Bool {
		guard let draft else { return false }
		if isCreating {
			guard let id = await workspace.change({ try await $0.createReport(draft) }) else { return false }
			choice = .custom(id: id)
		} else {
			guard await workspace.change({ try await $0.saveReport(draft) }) != nil else { return false }
		}
		self.draft = nil
		preview = nil
		isCreating = false
		await load()
		return true
	}

	/// What the page draws: the preview while editing, else the saved report.
	public var shown: CustomReportView? { draft == nil ? view : preview }
}

/// Actual's dashboards, every widget type drawn natively (R-REP-4, D73).
/// Every change is a partial write through the core: a frame, a config, a
/// widget or a page.
@MainActor
@Observable
public final class DashboardModel {
	public static let columns: Int64 = 12

	public private(set) var sheets: [DashboardSheet] = []
	public var pageID: String?
	public var isEditing = false

	@ObservationIgnored let workspace: WorkspaceModel

	public init(workspace: WorkspaceModel) {
		self.workspace = workspace
	}

	public var format: MoneyFormat { workspace.format }
	public var isReadOnly: Bool { workspace.readOnly != nil }
	public var canEdit: Bool { !isReadOnly && workspace.canEditDashboards }
	public var canPlace: Bool { !isReadOnly }

	public var sheet: DashboardSheet? {
		sheets.first { $0.id == pageID } ?? sheets.first
	}

	public func load() async {
		let today = WorkspaceModel.today()
		guard let sheets = await workspace.report({ try await $0.dashboardSheets(today: today) }) else { return }
		self.sheets = sheets
		if pageID == nil || !sheets.contains(where: { $0.id == pageID }) { pageID = sheets.first?.id }
	}

	/// Clamped as Actual's grid clamps it; the core clamps again.
	public func place(_ card: DashboardCard, x: Int64, y: Int64, width: Int64, height: Int64) async {
		let minimum = card.minimumSize
		let width = min(max(width, minimum.width), Self.columns)
		let height = max(height, minimum.height)
		let x = min(max(x, 0), Self.columns - width)
		let y = max(y, 0)
		guard (x, y, width, height) != (card.x, card.y, card.width, card.height) else { return }
		if await workspace.perform(.placeWidget(widgetId: card.id, x: x, y: y, width: width, height: height)) {
			await load()
		}
	}

	public func move(_ card: DashboardCard, columns: Int64 = 0, rows: Int64 = 0) async {
		await place(card, x: card.x + columns, y: card.y + rows, width: card.width, height: card.height)
	}

	public func resize(_ card: DashboardCard, columns: Int64 = 0, rows: Int64 = 0) async {
		await place(card, x: card.x, y: card.y, width: card.width + columns, height: card.height + rows)
	}

	/// Resizing from an edge or corner: `leading` and `top` edges move the
	/// origin as the size changes, so the opposite edge stays put.
	public func resize(_ card: DashboardCard, leading: Int64 = 0, top: Int64 = 0, trailing: Int64 = 0, bottom: Int64 = 0) async {
		let minimum = card.minimumSize
		let width = min(max(card.width - leading + trailing, minimum.width), Self.columns)
		let height = max(card.height - top + bottom, minimum.height)
		let x = leading == 0 ? card.x : card.x + card.width - width
		let y = top == 0 ? card.y : card.y + card.height - height
		await place(card, x: x, y: y, width: width, height: height)
	}

	/// A new widget at Actual's default size, under everything else on the page.
	public func add(_ kind: WidgetKind, config: WidgetConfig = WidgetConfig()) async {
		guard let sheet else { return }
		let size = kind.defaultSize
		let bottom = sheet.cards.map { $0.y + $0.height }.max() ?? 0
		let page = sheet.id
		var config = config
		if kind == .markdown, config.content == nil { config.content = "## Notes" }
		let settings = config
		await workspace.change { try await $0.addWidget(page: page, kind: kind, x: 0, y: bottom, width: size.width, height: size.height, config: settings) }
		await load()
	}

	public func remove(_ card: DashboardCard) async {
		let id = card.id
		await workspace.change { try await $0.removeWidget(id) }
		await load()
	}

	public func configure(_ card: DashboardCard, _ config: WidgetConfig) async {
		let id = card.id
		await workspace.change { try await $0.setWidgetConfig(id, config) }
		await load()
	}

	public func createDashboard(named name: String) async {
		let name = name.trimmingCharacters(in: .whitespaces)
		guard !name.isEmpty else { return }
		if let id = await workspace.change({ try await $0.createDashboard(named: name) }) {
			await load()
			pageID = id
		}
	}

	public func renameDashboard(to name: String) async {
		guard let id = sheet?.id else { return }
		let name = name.trimmingCharacters(in: .whitespaces)
		guard !name.isEmpty else { return }
		await workspace.change { try await $0.renameDashboard(id, to: name) }
		await load()
	}

	public func deleteDashboard() async {
		guard let id = sheet?.id else { return }
		await workspace.change { try await $0.deleteDashboard(id) }
		pageID = nil
		await load()
	}

	/// Saved custom reports, for adding one as a widget.
	public func customReports() async -> [CustomReportItem] {
		await workspace.report { try await $0.customReports() } ?? []
	}
}
