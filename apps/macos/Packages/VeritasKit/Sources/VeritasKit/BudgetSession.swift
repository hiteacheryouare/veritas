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

	public init(
		name: String?,
		directory: URL,
		readOnly: ReadOnlyKind?,
		canUndo: Bool = false,
		canRedo: Bool = false,
		isChecking: Bool = false,
		accountGroups: [SidebarGroup],
		format: MoneyFormat? = nil,
		numberStyle: NumberStyle = .commaDot
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
	/// The open goals for `month`, in the order the core shows them.
	func goals(_ month: Month, today: String) async throws -> [GoalSummary]
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
