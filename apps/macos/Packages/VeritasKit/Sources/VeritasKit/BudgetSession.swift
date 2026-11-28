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
	/// The core placed these accounts as liabilities (ST-9.1): Credit and Loans.
	public let isLiability: Bool

	public var id: String { name }

	public init(name: String, accounts: [SidebarAccount], isLiability: Bool = false) {
		self.name = name
		self.accounts = accounts
		self.isLiability = isLiability
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
	/// The budget's number format and its currency's symbol and places, read
	/// with the overview so they are set before any screen shows the budget.
	/// `nil` until the core reads the budget's own format.
	public let format: MoneyFormat?
	/// The budget currency's ISO 4217 code, `nil` when it has none.
	public let currencyCode: String?
	/// How amount fields read what's typed: the budget's separators.
	public let numberStyle: NumberStyle
	/// Why the last daily bill run failed, until one works.
	public let scheduleProblem: String?
	/// Why the last snapshot failed.
	public let snapshotProblem: String?
	/// The problem is the import snapshot's, which stays for the session.
	public let snapshotProblemIsImport: Bool

	public init(
		name: String?,
		directory: URL,
		readOnly: ReadOnlyKind?,
		canUndo: Bool = false,
		canRedo: Bool = false,
		isChecking: Bool = false,
		accountGroups: [SidebarGroup],
		format: MoneyFormat? = nil,
		currencyCode: String? = nil,
		numberStyle: NumberStyle = .commaDot,
		scheduleProblem: String? = nil,
		snapshotProblem: String? = nil,
		snapshotProblemIsImport: Bool = false
	) {
		self.name = name
		self.directory = directory
		self.readOnly = readOnly
		self.canUndo = canUndo
		self.canRedo = canRedo
		self.isChecking = isChecking
		self.accountGroups = accountGroups
		self.format = format
		self.currencyCode = currencyCode
		self.numberStyle = numberStyle
		self.scheduleProblem = scheduleProblem
		self.snapshotProblem = snapshotProblem
		self.snapshotProblemIsImport = snapshotProblemIsImport
	}

	public var accounts: [SidebarAccount] {
		accountGroups.flatMap(\.accounts)
	}
}

public extension BudgetOverview {
	/// Whether the core places this account as a liability (ST-9.1), as the sidebar groups it.
	func isLiability(_ accountID: String) -> Bool {
		accountGroups.first { $0.accounts.contains { $0.id == accountID } }?.isLiability ?? false
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
	func goals(today: String) async throws -> GoalList
	/// What closing a goal would move, this month and later (D66).
	func goalClosePreview(_ goalID: String, releaseTo: String?, today: String) async throws -> GoalClosePreview
	/// Whether a category has history, which turns delete into Archive (R-BUD-5).
	func categoryHistory(_ categoryID: String) async throws -> CategoryHistory

	/// Net Profit for each month in the range and for the whole range (R-REP-2).
	func netProfit(first: Month, last: Month) async throws -> NetProfitReport
	/// Net Profit for inclusive `YYYY-MM-DD` dates, with group totals (R-AI-1).
	func netProfitPeriod(first: String, last: String) async throws -> NetProfitPeriod
	/// The assistant's budget habits (R-AI-1, AI-10), computed in the core.
	func budgetHabits(today: String) async throws -> BudgetHabits
	/// Net Profit's Expenses by category for inclusive dates (R-AI-1, AI-12), ranked and summed in the core.
	func spendingByCategory(first: String, last: String) async throws -> CategoryBreakdown
	/// Assets, liabilities and net worth at each month's end (R-REP-5).
	func netWorth(first: Month, last: Month) async throws -> [NetWorthMonth]
	/// One cash flow statement per month (R-REP-5).
	func cashFlow(first: Month, last: Month) async throws -> [CashFlowColumn]
	/// One cash flow statement for an inclusive date range (R-REP-7).
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

	/// Every account with its balances, and the account groups (R-ACC-1).
	func accounts() async throws -> AccountList
	/// One page of a register, in the session's order (R-ACC-2, R-PERF-3).
	func register(_ scope: RegisterScope, filter: RegisterFilter, sort: RegisterSort, offset: UInt64, limit: UInt64) async throws -> RegisterPage
	func registerRow(_ id: String) async throws -> RegisterRow
	/// Every payee by name (R-ACC-5).
	func payees() async throws -> [PayeeDetail]
	/// The payee's last category and amount, for quick entry (R-ACC-4).
	func payeeMemory(_ payeeID: String) async throws -> PayeeMemory?
	/// What's cleared and how far it is from the statement (R-ACC-3).
	func reconciliation(_ accountID: String, statementBalance: Int64) async throws -> Reconciliation

	/// Whether the core has the transfer, category-kind and loan calls below.
	/// Until it does, Move money is a plain transfer with no previews.
	var canPlanTransfers: Bool { get }
	/// Both balances before and after a move, and whether Veritas asks what it's for (R-XFER-1, R-XFER-4).
	func transferPreview(from: String, to: String, amount: Int64) async throws -> TransferPreview
	func kindTarget(_ kind: CategoryKind) async throws -> KindTarget
	/// How many transactions a kind change reclassifies (CLS-3.3).
	func kindChangePreview(_ categoryID: String, kind: CategoryKind) async throws -> UInt64
	func setCategoryKind(_ categoryID: String, kind: CategoryKind) async throws
	/// Where a new envelope of the kind would go, and whether its group is hidden (CLS-4.1).
	func kindCategoryPlan(_ kind: CategoryKind) async throws -> KindCategoryPlan
	/// Returns the new envelope's id. `unhideGroup` is the user's yes to showing a hidden group again.
	@discardableResult
	func createKindCategory(kind: CategoryKind, name: String, unhideGroup: Bool) async throws -> String
	/// `nil` when the account isn't a loan.
	func loanTerms(_ accountID: String) async throws -> LoanTerms?
	func setLoanTerms(_ accountID: String, _ terms: LoanTerms) async throws
	/// Returns the new account's id.
	@discardableResult
	func createAccount(_ account: NewAccount, loan: LoanTerms?) async throws -> String
	func loanPaymentDefaults(_ loanAccountID: String, today: String) async throws -> LoanPaymentDefaults
	func loanPaymentPreview(_ loanAccountID: String, date: String, total: Int64, interest: Int64, escrow: Int64, fees: Int64) async throws -> LoanPaymentPreview
	func recordLoanPayment(_ payment: LoanPaymentDraft) async throws
	/// Basis points: "9.80%" is 980.
	func evaluateRate(_ text: String, style: NumberStyle) async throws -> Int64
	/// End-of-day balances for the register's chart, and the change over the range.
	func balanceHistory(_ accountID: String, from: String, to: String) async throws -> BalanceHistory
	/// Every tag, most used first.
	func tags() async throws -> [TagSummary]

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
	/// Active bills by next date, then paused and ended ones (R-BILL-1).
	func bills(today: String) async throws -> [BillSummary]
	/// One bill for the editor, with its next dates.
	func bill(_ id: String, today: String) async throws -> BillDetail
	/// Unpaid occurrences of active bills through `today + days`, oldest first (R-BILL-3).
	func upcoming(days: UInt32, today: String) async throws -> [UpcomingBill]
	/// Live rules in the order Actual runs them (R-RULE-2).
	func rules() async throws -> [RuleSentence]
	/// The rows "Apply to existing" would change, newest first.
	func rulePreview(_ source: RuleSource) async throws -> [RuleMatch]
	/// The name on every statement's header and on the tax summary (ST-3).
	func statementEntity() async throws -> StatementEntity
	/// Revenue and expenses by category group over inclusive `YYYY-MM-DD` dates (R-REP-7, ST-3).
	func incomeStatement(first: String, last: String) async throws -> IncomeStatement
	/// One year's Wrapped, or this year's to `today` (R-WRAP-1, FC-8).
	func wrapped(year: Int32, today: String) async throws -> WrappedReport
	/// The years whose Wrapped has data, newest first (FC-8, v18).
	func wrappedYears(today: String) async throws -> [Int32]
	/// The balance sheet at the end of `asOf`, `YYYY-MM-DD` (R-REP-7).
	func balanceSheet(asOf: String) async throws -> BalanceSheet
	/// Owner's equity over inclusive `YYYY-MM-DD` dates (R-REP-7).
	func ownersEquity(first: String, last: String) async throws -> OwnersEquity
	/// One tax year's buckets, detail, conflicts and CSVs (R-REP-8).
	func taxSummary(year: Int32) async throws -> TaxSummary
	/// The baseline and up to 3 what-ifs as of `today` (R-FC-1, R-FC-2, R-FC-4).
	func forecast(today: String, scenarioIDs: [String]) async throws -> Forecast
	/// Next month's suggestions (R-FC-3).
	func budgetSuggestions(today: String) async throws -> SuggestionList
	/// Every what-if in the order it was made, with its events (R-FC-2).
	func scenarios(today: String) async throws -> [ScenarioSummary]
	/// Returns the new what-if's id. With `firstEvent`, the what-if and its
	/// first line are one undo step, and a refused line makes nothing (v17).
	@discardableResult
	func createScenario(named name: String, firstEvent: ScenarioChange?, today: String) async throws -> String
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
