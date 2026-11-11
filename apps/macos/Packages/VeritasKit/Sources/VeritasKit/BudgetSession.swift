import Foundation
import VeritasCore

public struct AccountSummary: Hashable, Identifiable, Sendable {
	public let id: String
	public let name: String
	public let balance: Int64

	public init(id: String, name: String, balance: Int64) {
		self.id = id
		self.name = name
		self.balance = balance
	}
}

public struct AccountGroup: Hashable, Identifiable, Sendable {
	public let name: String
	public let accounts: [AccountSummary]

	public var id: String { name }

	public init(name: String, accounts: [AccountSummary]) {
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
	public let accountGroups: [AccountGroup]

	public init(
		name: String?,
		directory: URL,
		readOnly: ReadOnlyKind?,
		canUndo: Bool = false,
		canRedo: Bool = false,
		isChecking: Bool = false,
		accountGroups: [AccountGroup]
	) {
		self.name = name
		self.directory = directory
		self.readOnly = readOnly
		self.canUndo = canUndo
		self.canRedo = canRedo
		self.isChecking = isChecking
		self.accountGroups = accountGroups
	}

	public var accounts: [AccountSummary] {
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
	func exportActualZip(to destination: URL) async throws
	/// Calls `changed` after every commit, undo, redo or restore, from any thread.
	func observeChanges(_ changed: @escaping @Sendable () -> Void) async throws
	func close() async
}

/// The budgets in the app's budgets folder: new ones, Actual imports and reopened ones.
public protocol BudgetLibrary: Sendable {
	/// A folder for one new budget. A retried import or create reuses it, so it never makes a second copy.
	func newBudgetDirectory() -> URL
	func open(directory: URL) async throws -> any BudgetSession
	func importActualZip(_ zip: URL, into directory: URL) async throws -> any BudgetSession
	func createBudget(_ budget: NewBudget, into directory: URL) async throws -> any BudgetSession
	func currencies() async throws -> [CurrencyOption]
	/// Reads an amount the way every amount field does, arithmetic included.
	func evaluateAmount(_ text: String) async throws -> Int64
}
