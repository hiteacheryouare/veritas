import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// R-AI-1 "read-only: it never mutates data", on the real core: every tool,
/// asked for ordinary, odd and impossible things, leaves every table, the undo
/// history and the window exactly as they were.
@MainActor
struct AssistantLeavesTheBudgetAloneTests {
	private func fingerprint(_ budget: RealBudget) throws -> [String: [[String]]] {
		let tables = try budget.sqlite("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name").map { $0[0] }
		var rows: [String: [[String]]] = [:]
		for table in tables {
			rows[table] = try budget.sqlite("SELECT * FROM \"\(table)\"").sorted { $0.lexicographicallyPrecedes($1) }
		}
		return rows
	}

	@Test func everyToolAskedAnythingLeavesTheBudgetAsItWas() async throws {
		try await RealBudget.with { budget in
			let workspace = budget.workspace
			let context = AssistantContext(queries: try #require(workspace.readOnlyQueries), format: workspace.format, today: WorkspaceModel.today())
			let before = try fingerprint(budget)
			let couldUndo = workspace.canUndo
			#expect(before.count > 20, "the whole file, Actual's tables and Veritas's")

			let periods = ["2026-10", "last month", "so far this month", "this year", "2031-06", "1999-01", "2026-13", "", "next month", "2026-10-31 to 2026-05-01", "May to October 2026"]
			for period in periods {
				_ = try? await BudgetMonthTool(context: context).call(arguments: .init(period: period))
				_ = try? await NetProfitTool(context: context).call(arguments: .init(period: period))
				_ = try? await CashFlowTool(context: context).call(arguments: .init(period: period))
				_ = try? await SpendingTool(context: context).call(arguments: .init(period: period))
				_ = try? await IncomeTool(context: context).call(arguments: .init(period: period))
				_ = try? await SpendingByCategoryTool(context: context).call(arguments: .init(period: period))
				for name in ["Groceries", "everyday", "groc", "", "'; DROP TABLE categories; --"] {
					_ = try? await CategorySpendingTool(context: context).call(arguments: .init(name: name, period: period))
					_ = try? await BudgetTool(context: context).call(arguments: .init(name: name, period: period))
				}
			}
			for date in [context.today, "2031-12-31", "1999-01-01", "2026-02-30", "soon"] {
				_ = try? await BalanceSheetTool(context: context).call(arguments: .init(date: date))
			}
			for year in [2026, 2031, 1900, 99_999, -1] {
				_ = try? await TaxSummaryTool(context: context).call(arguments: .init(year: year))
			}
			for today in [context.today, "2026-11-22", "2026-11-01", "2026-01-15", "2031-12-31", "1999-01-01", "2026-02-30", "soon"] {
				_ = try? await BudgetHabitsTool(context: AssistantContext(queries: context.queries, format: context.format, today: today)).call(arguments: .init())
			}
			_ = try? await AccountsTool(context: context).call(arguments: .init())
			_ = try? await GoalsTool(context: context).call(arguments: .init())
			let reports = try await context.queries.customReports()
			for name in reports.map(\.name) + ["No such report", "", "'; DROP TABLE transactions; --"] {
				_ = try? await SavedReportTool(context: context).call(arguments: .init(name: name))
			}

			#expect(workspace.canUndo == couldUndo, "nothing joined the undo history")
			#expect(workspace.alert == nil)
			let after = try fingerprint(budget)
			for table in Set(before.keys).union(after.keys).sorted() {
				#expect(before[table] == after[table], "\(table) changed")
			}
		}
	}
}
