import Foundation
import SQLite3
import Testing
import VeritasCore
@testable import VeritasKit

/// A fixture budget opened the way the app opens it: imported through
/// `WorkspaceModel` and `CoreBudgetLibrary` into a scratch budgets folder.
@MainActor
final class RealBudget {
	let budgets: URL
	let workspace: WorkspaceModel

	private init(budgets: URL, workspace: WorkspaceModel) {
		self.budgets = budgets
		self.workspace = workspace
	}

	static func fixture(_ name: String) -> URL {
		URL(filePath: #filePath)
			.deletingLastPathComponent()
			.appending(path: "../../../../../../fixtures/synthetic/\(name).zip")
			.standardizedFileURL
	}

	/// Imports the fixture, runs `body`, then closes the budget and deletes the
	/// scratch folder whether or not `body` throws.
	static func with(_ fixture: String = "all-features", _ body: (RealBudget) async throws -> Void) async throws {
		let budgets = URL.temporaryDirectory.appending(path: "veritas-real-\(UUID().uuidString)", directoryHint: .isDirectory)
		let workspace = WorkspaceModel(library: CoreBudgetLibrary(budgetsDirectory: budgets), format: MoneyFormat())
		let budget = RealBudget(budgets: budgets, workspace: workspace)
		defer { try? FileManager.default.removeItem(at: budgets) }
		await workspace.importActualZip(Self.fixture(fixture))
		do {
			try #require(workspace.phase == .open, "the import opened: \(String(describing: workspace.alert))")
			try await body(budget)
		} catch {
			await workspace.closeBudget()
			throw error
		}
		await workspace.closeBudget()
	}

	var directory: URL {
		get throws { try #require(workspace.overview?.directory) }
	}

	func screen(_ month: Month) async -> BudgetScreenModel {
		let screen = BudgetScreenModel(workspace: workspace, month: month
		)
		await screen.load()
		return screen
	}

	func goalsScreen() async -> GoalsScreenModel {
		let goals = GoalsScreenModel(workspace: workspace)
		await goals.load()
		return goals
	}

	/// What a second window would read from disk right now: a fresh, read-only
	/// session on the same folder, with none of the open session's caches.
	func fromDisk<T>(_ read: (Session) throws -> T) async throws -> T {
		let session = try await Session.open(directory: try directory.path(percentEncoded: false), cancel: CancelToken())
		defer { try? session.close() }
		#expect(try session.state().readOnly == .openElsewhere)
		return try read(session)
	}

	func monthFromDisk(_ month: Month) async throws -> BudgetMonth {
		try await fromDisk { try $0.budgetMonth(month: month, today: WorkspaceModel.today()) }
	}

	func goalsFromDisk() async throws -> [GoalSummary] {
		try await fromDisk { try $0.goals(today: WorkspaceModel.today()) }
	}

	/// Rows from the budget's own Actual database, read with SQLite directly,
	/// the way Actual will read the file.
	func sqlite(_ sql: String) throws -> [[String]] {
		try ActualDatabase.rows(try directory.appending(path: "db.sqlite"), sql)
	}

	/// Every live and deleted transaction, as Actual stores it, for "no
	/// transaction was touched" checks.
	func transactionRows() throws -> [[String]] {
		try sqlite("SELECT id, acct, category, amount, date, tombstone, notes FROM transactions ORDER BY id")
	}

	/// Waits for the change listener's refresh, the signal every open screen reloads on.
	func changeAnnounced(after count: Int) async -> Bool {
		for _ in 0..<500 {
			if workspace.changeCount > count { return true }
			try? await Task.sleep(for: .milliseconds(10))
		}
		return false
	}
}

enum ActualDatabase {
	static func rows(_ file: URL, _ sql: String) throws -> [[String]] {
		var db: OpaquePointer?
		guard sqlite3_open_v2(file.path(percentEncoded: false), &db, SQLITE_OPEN_READONLY, nil) == SQLITE_OK else {
			defer { sqlite3_close(db) }
			throw DatabaseError(message: String(cString: sqlite3_errmsg(db)))
		}
		defer { sqlite3_close(db) }
		var statement: OpaquePointer?
		guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
			throw DatabaseError(message: String(cString: sqlite3_errmsg(db)))
		}
		defer { sqlite3_finalize(statement) }
		var rows: [[String]] = []
		while sqlite3_step(statement) == SQLITE_ROW {
			rows.append((0..<sqlite3_column_count(statement)).map { column in
				sqlite3_column_text(statement, column).map { String(cString: $0) } ?? "NULL"
			})
		}
		return rows
	}

	/// Waits out a lock the way Actual's SQLite does, since a budget's
	/// background check can still be finishing just after it closes.
	static func execute(_ file: URL, _ sql: String) throws {
		var db: OpaquePointer?
		defer { sqlite3_close(db) }
		guard sqlite3_open(file.path(percentEncoded: false), &db) == SQLITE_OK,
			sqlite3_busy_timeout(db, 5_000) == SQLITE_OK,
			sqlite3_exec(db, sql, nil, nil, nil) == SQLITE_OK
		else {
			throw DatabaseError(message: String(cString: sqlite3_errmsg(db)))
		}
	}

	struct DatabaseError: Error {
		let message: String
	}
}

/// fixtures/synthetic/all-features: an envelope budget from Aug 2025, with
/// transactions through Nov 2026.
enum AllFeatures {
	static let september = Month(year: 2026, month: 9)
	static let october = Month(year: 2026, month: 10)
	static let november = Month(year: 2026, month: 11)
	static let december = Month(year: 2026, month: 12)

	static let checking = "0a1c000f-c328-42b8-8966-a4be59a2504b"
	static let rent = "63f26a17-4832-430e-b385-2a693a684509"
	static let funMoney = "b07b454d-836a-4659-877c-e69391584e6b"
	static let household = "e2dcc389-9fe5-4b1f-93d6-0099cd592c17"
	static let diningOut = "fffcd498-80e7-4984-b378-87cee6b5625c"
	static let groceries = "fc093a41-2d11-4d98-a211-e5db92205f23"
	static let gifts = "30470428-ba0a-4bfc-8c4a-ba901a52c302"
	static let emergencyFund = "b8070040-5368-407b-b9e7-f6dabe59b7a8"
	static let vacation = "0c2998b7-d4f3-42d6-aca6-ef72ed399ca8"
	static let oldHobby = "311c8af5-f171-410c-81a8-59b7372dc0a4"
	static let everydayGroup = "b850ef40-801c-488b-9843-186722e75add"
	static let interest = "dd4b07d1-4a06-4aef-8eea-6ce1176bc565"
}

extension BudgetMonth {
	func row(_ id: String) throws -> EnvelopeRow {
		try #require(groups.lazy.flatMap(\.envelopes).first { $0.id == id }, "envelope \(id) in \(month)")
	}
}
