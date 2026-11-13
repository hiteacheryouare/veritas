import Foundation
import VeritasCore

public struct SidebarAccount: Hashable, Identifiable, Sendable {
	public let id: String
	public let name: String
	public let balance: Int64
	/// Closed, or deleted but kept for its history (ST-9.4).
	public let isInactive: Bool

	public init(id: String, name: String, balance: Int64, isInactive: Bool = false) {
		self.id = id
		self.name = name
		self.balance = balance
		self.isInactive = isInactive
	}
}

public struct SidebarGroup: Hashable, Identifiable, Sendable {
	public let name: String
	public let accounts: [SidebarAccount]

	public var id: String { name }

	public init(name: String, accounts: [SidebarAccount]) {
		self.name = name
		self.accounts = accounts
	}
}

/// What the window frame needs from an open budget. Screens fetch their own models.
public struct BudgetOverview: Hashable, Sendable {
	public let name: String?
	public let directory: URL
	public let readOnly: ReadOnlyKind?
	public let canUndo: Bool
	public let canRedo: Bool
	/// The full integrity check still runs in the background. The budget is usable meanwhile.
	public let isChecking: Bool
	public let accountGroups: [SidebarGroup]
	/// `nil` until the core reads the budget's own format.
	public let format: MoneyFormat?
	/// How amount fields read what's typed: the budget's separators.
	public let numberStyle: NumberStyle
	public let currencyCode: String?

	public init(
		name: String?,
		directory: URL,
		readOnly: ReadOnlyKind?,
		canUndo: Bool = false,
		canRedo: Bool = false,
		isChecking: Bool = false,
		accountGroups: [SidebarGroup],
		format: MoneyFormat? = nil,
		numberStyle: NumberStyle = .commaDot,
		currencyCode: String? = nil
	) {
		self.name = name
		self.directory = directory
		self.readOnly = readOnly
		self.canUndo = canUndo
		self.canRedo = canRedo
		self.isChecking = isChecking
		self.accountGroups = accountGroups
		self.format = format
		self.numberStyle = numberStyle
		self.currencyCode = currencyCode
	}

	public var accounts: [SidebarAccount] {
		accountGroups.flatMap(\.accounts)
	}
}

/// One open budget. Every call throws `FfiError`.
public protocol BudgetSession: AnyObject, Sendable {
	/// False until the core can export, so Export to Actual stays disabled.
	var canExport: Bool { get }

	func overview() async throws -> BudgetOverview
	func undo() async throws
	func redo() async throws
	/// Collects the background integrity check and takes the hourly snapshot when one is due.
	func maintain() async throws
	/// The core snapshots, audits and writes the zip. Findings that don't stop the export come back.
	func exportActualZip(to destination: URL) async throws -> [AuditFinding]
	/// Calls `changed` after every commit, undo, redo or restore, from any thread.
	func observeChanges(_ changed: @escaping @Sendable () -> Void) async throws
	func close() async

	func backupStatus() async throws -> BackupStatus
	/// Swift holds the folder's sandbox access; the core only writes into it.
	func setBackupFolder(_ folder: URL?) async throws
	/// Writes today's backup zip. A failure also lands in `backupStatus().lastProblem`.
	func backUpNow() async throws
	/// `nil` while the background check still runs.
	func integrityFindings() async throws -> [AuditFinding]?
	/// Newest first.
	func snapshots() async throws -> [SnapshotInfo]
	/// Snapshots the budget as it is first, so a restore can itself be undone (R-REL-3).
	func restoreSnapshot(id: String) async throws
	func retention() async throws -> Retention
	func setRetention(_ retention: Retention) async throws

	/// The Budget screen for `month`. `today` is the local civil date, `YYYY-MM-DD`.
	func budgetMonth(_ month: Month, today: String) async throws -> BudgetMonth
	/// One user action: one transaction and one undo step.
	func perform(_ command: Command) async throws
	/// Every goal, closed ones included, in the core's order.
	func goals(today: String) async throws -> [GoalSummary]
	/// What closing a goal would move, this month and later (D66).
	func goalClosePreview(_ goalID: String, releaseTo: String?, today: String) async throws -> GoalClosePreview
	/// Whether a category has history, which turns delete into Archive (R-BUD-5).
	func categoryHistory(_ categoryID: String) async throws -> CategoryHistory

	/// Net Profit for each month in the range and for the whole range (R-REP-2).
	func netProfit(first: Month, last: Month) async throws -> NetProfitReport
	/// Assets, liabilities and net worth at each month's end (R-REP-5).
	func netWorth(first: Month, last: Month) async throws -> [NetWorthMonth]
	/// One Cash Flow Statement per month (R-REP-5).
	func cashFlow(first: Month, last: Month) async throws -> [CashFlowColumn]
	/// One Cash Flow Statement for an inclusive date range (R-REP-7).
	func cashFlowStatement(first: String, last: String) async throws -> CashFlowColumn
	/// Inclusive `YYYY-MM-DD` dates (R-REP-6).
	func sankey(first: String, last: String) async throws -> SankeyReport
	/// The rows behind one report number.
	func reportTransactions(_ drill: ReportDrill) async throws -> [ReportTransaction]
	func customReports() async throws -> [CustomReportItem]
	func customReport(_ id: String, today: String) async throws -> CustomReportView
	/// What edited settings would show, without saving.
	func previewCustomReport(_ settings: CustomReportSettings, today: String) async throws -> CustomReportView
	func dashboards(today: String) async throws -> [DashboardPage]

	/// Whether the core can add, remove and configure widgets and dashboards,
	/// and save report filters. Until it can, those actions stay hidden.
	var canEditDashboards: Bool { get }
	/// Every dashboard with each card's numbers as of `today`.
	func dashboardSheets(today: String) async throws -> [DashboardSheet]
	/// Returns the new widget's id.
	@discardableResult
	func addWidget(page: String, kind: WidgetKind, x: Int64, y: Int64, width: Int64, height: Int64, config: WidgetConfig) async throws -> String
	func removeWidget(_ id: String) async throws
	func setWidgetConfig(_ id: String, _ config: WidgetConfig) async throws
	/// Returns the new dashboard's id.
	@discardableResult
	func createDashboard(named name: String) async throws -> String
	func renameDashboard(_ id: String, to name: String) async throws
	func deleteDashboard(_ id: String) async throws
	/// A saved report's settings with its filters.
	func reportDraft(_ id: String) async throws -> ReportDraft
	func previewReport(_ draft: ReportDraft, today: String) async throws -> CustomReportView
	func saveReport(_ draft: ReportDraft) async throws
	/// Returns the new report's id.
	@discardableResult
	func createReport(_ draft: ReportDraft) async throws -> String
}

/// The budgets in the app's budgets folder: new ones, Actual imports and reopened ones.
public protocol BudgetLibrary: Sendable {
	/// A folder for one new budget. A retried import or create reuses it, so it never makes a second copy.
	func newBudgetDirectory() -> URL
	/// Most recently opened first.
	func listBudgets() async throws -> [BudgetListingItem]
	func open(directory: URL) async throws -> any BudgetSession
	func importActualZip(_ zip: URL, into directory: URL) async throws -> any BudgetSession
	func createBudget(_ budget: NewBudget, into directory: URL) async throws -> any BudgetSession
	func currencies() async throws -> [CurrencyOption]
	/// Reads an amount the way every amount field does, arithmetic included.
	func evaluateAmount(_ text: String, style: NumberStyle) async throws -> Int64
}
