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
	public let name: String
	public let directory: URL
	public let readOnly: ReadOnlyKind?
	public let accountGroups: [AccountGroup]

	public init(name: String, directory: URL, readOnly: ReadOnlyKind?, accountGroups: [AccountGroup]) {
		self.name = name
		self.directory = directory
		self.readOnly = readOnly
		self.accountGroups = accountGroups
	}

	public var accounts: [AccountSummary] {
		accountGroups.flatMap(\.accounts)
	}
}

/// One open budget. Every call throws `FfiError`.
public protocol BudgetSession: AnyObject, Sendable {
	func overview() async throws -> BudgetOverview
	func exportActualZip(to destination: URL) async throws
	func close() async
}

/// Opens budgets in the app's budgets folder and imports Actual exports into it.
public protocol BudgetLibrary: Sendable {
	/// A folder for one new budget. A retried import reuses it, so it never makes a second copy.
	func newBudgetDirectory() -> URL
	func open(directory: URL) async throws -> any BudgetSession
	func importActualZip(_ zip: URL, into directory: URL) async throws -> any BudgetSession
}
