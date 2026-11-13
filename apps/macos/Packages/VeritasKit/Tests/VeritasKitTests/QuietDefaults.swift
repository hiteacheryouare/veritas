import Foundation
import VeritasCore
import VeritasKit

/// No-op settings calls for test sessions that don't exercise them.
extension BudgetSession {
	func backupStatus() async throws -> BackupStatus { .none }
	func setBackupFolder(_ folder: URL?) async throws {}
	func backUpNow() async throws {}
	func integrityFindings() async throws -> [AuditFinding]? { [] }
	func snapshots() async throws -> [SnapshotInfo] { [] }
	func restoreSnapshot(id: String) async throws {}
	func retention() async throws -> Retention { Retention(hourly: 48, daily: 30, monthly: 12, operationDays: 30) }
	func setRetention(_ retention: Retention) async throws {}
	func budgetMonth(_ month: Month, today: String) async throws -> BudgetMonth { throw FfiError.Cancelled }
	func perform(_ command: Command) async throws {}
	func goals(_ month: Month, today: String) async throws -> [GoalSummary] { [] }
}

extension BudgetLibrary {
	func listBudgets() async throws -> [BudgetListingItem] { [] }
}
