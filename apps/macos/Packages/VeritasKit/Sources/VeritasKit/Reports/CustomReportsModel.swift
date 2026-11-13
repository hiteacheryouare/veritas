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
	/// The settings being edited, with their live preview.
	public var draft: CustomReportSettings?
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

	public func edit() {
		guard let settings = view?.settings else { return }
		draft = settings
		preview = view
	}

	/// A fixed range, written in the shape the report already stores:
	/// `YYYY-MM`, or whole dates when Actual saved those.
	public func setFixedRange(first: Month, last: Month) {
		guard var draft else { return }
		let period = ReportPeriod(first: first, last: last)
		let usesDays = draft.startDate.count > 7
		draft.dateStatic = true
		draft.startDate = usesDays ? period.first.firstDay : String(period.first.firstDay.prefix(7))
		draft.endDate = usesDays ? period.last.lastDay : String(period.last.firstDay.prefix(7))
		self.draft = draft
	}

	public func setLiveRange(_ name: String) {
		draft?.dateStatic = false
		draft?.dateRange = name
	}

	public func cancelEditing() {
		draft = nil
		preview = nil
	}

	public func refreshPreview() async {
		guard let draft else { return }
		let today = WorkspaceModel.today()
		if let preview = await workspace.report({ try await $0.previewCustomReport(draft, today: today) }), self.draft == draft {
			self.preview = preview
		}
	}

	@discardableResult
	public func save() async -> Bool {
		guard let draft else { return false }
		guard await workspace.perform(.saveCustomReport(settings: draft)) else { return false }
		self.draft = nil
		preview = nil
		await load()
		return true
	}

	/// What the page draws: the preview while editing, else the saved report.
	public var shown: CustomReportView? { draft == nil ? view : preview }
}

/// Actual's dashboards drawn natively (R-REP-4). Moving or resizing a widget
/// writes only its frame; placeholders move too and are never edited.
@MainActor
@Observable
public final class DashboardModel {
	public static let columns: Int64 = 12

	public private(set) var pages: [DashboardPage] = []
	public var pageID: String?
	public var isEditing = false

	@ObservationIgnored let workspace: WorkspaceModel

	public init(workspace: WorkspaceModel) {
		self.workspace = workspace
	}

	public var format: MoneyFormat { workspace.format }
	public var isReadOnly: Bool { workspace.readOnly != nil }

	public var page: DashboardPage? {
		pages.first { $0.id == pageID } ?? pages.first
	}

	public func load() async {
		let today = WorkspaceModel.today()
		guard let pages = await workspace.report({ try await $0.dashboards(today: today) }) else { return }
		self.pages = pages
		if pageID == nil || !pages.contains(where: { $0.id == pageID }) { pageID = pages.first?.id }
	}

	/// Clamped as Actual's grid clamps it; the core clamps again.
	public func place(_ widget: DashboardWidget, x: Int64, y: Int64, width: Int64, height: Int64) async {
		let width = min(max(width, widget.minWidth), Self.columns)
		let height = max(height, widget.minHeight)
		let x = min(max(x, 0), Self.columns - width)
		let y = max(y, 0)
		guard (x, y, width, height) != (widget.x, widget.y, widget.width, widget.height) else { return }
		if await workspace.perform(.placeWidget(widgetId: widget.id, x: x, y: y, width: width, height: height)) {
			await load()
		}
	}

	public func move(_ widget: DashboardWidget, columns: Int64 = 0, rows: Int64 = 0) async {
		await place(widget, x: widget.x + columns, y: widget.y + rows, width: widget.width, height: widget.height)
	}

	public func resize(_ widget: DashboardWidget, columns: Int64 = 0, rows: Int64 = 0) async {
		await place(widget, x: widget.x, y: widget.y, width: widget.width + columns, height: widget.height + rows)
	}
}
