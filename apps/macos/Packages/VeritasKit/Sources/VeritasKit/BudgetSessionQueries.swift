import Foundation
import VeritasCore

/// Hands the assistant an open session's reads and nothing else. The session
/// stays private, so code holding `any ReadOnlyBudgetQueries` can't reach `perform`.
struct BudgetSessionQueries: ReadOnlyBudgetQueries {
	private let session: any BudgetSession

	init(_ session: any BudgetSession) {
		self.session = session
	}

	func budgetHabits(today: String) async throws -> BudgetHabits {
		try await session.budgetHabits(today: today)
	}

	func spendingByCategory(first: String, last: String) async throws -> CategoryBreakdown {
		try await session.spendingByCategory(first: first, last: last)
	}

	func netProfitPeriod(first: String, last: String) async throws -> NetProfitPeriod {
		try await session.netProfitPeriod(first: first, last: last)
	}

	func balanceSheet(asOf: String) async throws -> BalanceSheet {
		try await session.balanceSheet(asOf: asOf)
	}

	func cashFlowStatement(first: String, last: String) async throws -> CashFlowColumn {
		try await session.cashFlowStatement(first: first, last: last)
	}

	func budgetMonth(_ month: Month, today: String) async throws -> BudgetMonth {
		try await session.budgetMonth(month, today: today)
	}

	func accounts() async throws -> AccountList {
		try await session.accounts()
	}

	func goals(today: String) async throws -> GoalList {
		try await session.goals(today: today)
	}

	func taxSummary(year: Int32) async throws -> TaxSummary {
		try await session.taxSummary(year: year)
	}

	func customReports() async throws -> [CustomReportItem] {
		try await session.customReports()
	}

	func customReport(_ id: String, today: String) async throws -> CustomReportView {
		try await session.customReport(id, today: today)
	}
}
