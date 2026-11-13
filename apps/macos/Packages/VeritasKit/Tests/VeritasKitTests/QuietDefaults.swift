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
	func goals(today: String) async throws -> [GoalSummary] { [] }
	func goalClosePreview(_ goalID: String, releaseTo: String?, today: String) async throws -> GoalClosePreview {
		GoalClosePreview(leftover: 0, laterBudget: [], destinationAvailableAfter: 0)
	}
	func categoryHistory(_ categoryID: String) async throws -> CategoryHistory {
		CategoryHistory(transactions: 0, budgetedMonths: 0, taxBucket: nil)
	}
	func netProfit(first: Month, last: Month) async throws -> NetProfitReport { throw FfiError.Cancelled }
	func netWorth(first: Month, last: Month) async throws -> [NetWorthMonth] { [] }
	func cashFlow(first: Month, last: Month) async throws -> [CashFlowColumn] { [] }
	func cashFlowStatement(first: String, last: String) async throws -> CashFlowColumn { throw FfiError.Cancelled }
	func sankey(first: String, last: String) async throws -> SankeyReport { throw FfiError.Cancelled }
	func reportTransactions(_ drill: ReportDrill) async throws -> [ReportTransaction] { [] }
	func customReports() async throws -> [CustomReportItem] { [] }
	func customReport(_ id: String, today: String) async throws -> CustomReportView { throw FfiError.Cancelled }
	func previewCustomReport(_ settings: CustomReportSettings, today: String) async throws -> CustomReportView { throw FfiError.Cancelled }
	func dashboards(today: String) async throws -> [DashboardPage] { [] }
	var canEditDashboards: Bool { false }
	func dashboardSheets(today: String) async throws -> [DashboardSheet] { try await dashboards(today: today).map(DashboardSheet.init) }
	func addWidget(page: String, kind: WidgetKind, x: Int64, y: Int64, width: Int64, height: Int64, config: WidgetConfig) async throws -> String { "" }
	func removeWidget(_ id: String) async throws {}
	func setWidgetConfig(_ id: String, _ config: WidgetConfig) async throws {}
	func createDashboard(named name: String) async throws -> String { "" }
	func renameDashboard(_ id: String, to name: String) async throws {}
	func deleteDashboard(_ id: String) async throws {}
	func reportDraft(_ id: String) async throws -> ReportDraft { ReportDraft(settings: try await customReport(id, today: "").settings) }
	func previewReport(_ draft: ReportDraft, today: String) async throws -> CustomReportView { try await previewCustomReport(draft.settings, today: today) }
	func saveReport(_ draft: ReportDraft) async throws { try await perform(.saveCustomReport(settings: draft.settings)) }
	func createReport(_ draft: ReportDraft) async throws -> String { "" }
}

extension BudgetLibrary {
	func listBudgets() async throws -> [BudgetListingItem] { [] }
}
