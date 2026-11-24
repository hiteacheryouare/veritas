import Foundation
import VeritasCore

/// Everything the assistant may read from an open budget. Every requirement
/// is a read, so a tool built on this protocol has no way to change the budget.
public protocol ReadOnlyBudgetQueries: Sendable {
	func netProfitPeriod(first: String, last: String) async throws -> NetProfitPeriod
	func budgetHabits(today: String) async throws -> BudgetHabits
	func spendingByCategory(first: String, last: String) async throws -> CategoryBreakdown
	func balanceSheet(asOf: String) async throws -> BalanceSheet
	func cashFlowStatement(first: String, last: String) async throws -> CashFlowColumn
	func budgetMonth(_ month: Month, today: String) async throws -> BudgetMonth
	func accounts() async throws -> AccountList
	func goals(today: String) async throws -> GoalList
	func taxSummary(year: Int32) async throws -> TaxSummary
	func customReports() async throws -> [CustomReportItem]
	func customReport(_ id: String, today: String) async throws -> CustomReportView
}

/// What the tools read with: the queries, the budget's number format, and
/// today's civil date, `YYYY-MM-DD`.
public struct AssistantContext: Sendable {
	public let queries: any ReadOnlyBudgetQueries
	public let format: MoneyFormat
	public let today: String

	public init(queries: any ReadOnlyBudgetQueries, format: MoneyFormat, today: String) {
		self.queries = queries
		self.format = format
		self.today = today
	}
}
