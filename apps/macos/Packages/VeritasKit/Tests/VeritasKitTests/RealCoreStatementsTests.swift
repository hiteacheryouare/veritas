import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// The Statements screen's model against the real core on
/// fixtures/synthetic/all-features (R-REP-7, ST-3, ST-4, ST-6): every line it
/// shows is the core's figure, and only the lines the spec hides are gone.
@MainActor
struct RealCoreStatementsTests {
	static let today = "2026-11-16"

	@Test func theBalanceSheetShowsTheCoresAccountsAndTotals() async throws {
		try await RealBudget.with { budget in
			let model = StatementsModel(workspace: budget.workspace, today: Self.today)
			await model.previous()
			#expect(model.asOf == "2026-10-31")
			let sheet = try #require(await budget.workspace.report { try await $0.balanceSheet(asOf: "2026-10-31") })
			let document = try #require(model.document)
			let amounts = Self.amounts(document)

			#expect(amounts["assets.total"] == sheet.totalAssets)
			#expect(amounts["liabilities.total"] == sheet.totalLiabilities)
			#expect(amounts["netWorth"] == sheet.netWorth)
			#expect(sheet.totalAssets != 0)
			let accounts = (sheet.assets + sheet.liabilities).flatMap(\.accounts)
			#expect(!accounts.isEmpty)
			for account in accounts {
				#expect(amounts["account.\(account.accountId)"] == account.amount, "\(account.name)")
			}
			#expect(document.lines.filter { $0.id.hasPrefix("account.") }.count == accounts.count, "every account the core lists, and no other")
			for section in sheet.assets + sheet.liabilities where section.groupName != nil && section.accounts.count > 1 {
				#expect(document.lines.contains { $0.style == .subtotal && $0.amount == section.total }, "\(section.groupName ?? "")")
			}
			#expect(document.lines.last?.title == "Net worth")
			#expect(document.basis == "As of October 31, 2026")
		}
	}

	@Test func ownersEquityShowsTheCoresReconcilingLinesAndHidesTheZeroOnes() async throws {
		try await RealBudget.with { budget in
			let model = StatementsModel(workspace: budget.workspace, today: Self.today)
			await model.show(.ownersEquity)
			await model.choose(.year)
			await model.previous()
			#expect(model.period == ReportPeriod(first: Month(year: 2025, month: 1), last: Month(year: 2025, month: 12)))
			let equity = try #require(await budget.workspace.report { try await $0.ownersEquity(first: "2025-01-01", last: "2025-12-31") })
			let document = try #require(model.document)
			let amounts = Self.amounts(document)
			#expect(equity.revaluations.total != 0 && equity.startingBalances.total == 0, "2025 has a line to show and one to hide")

			#expect(amounts["opening"] == equity.opening)
			#expect(amounts["netIncome"] == equity.netIncome.total)
			#expect(amounts["capitalizedTransfers"] == equity.capitalizedTransfers.total)
			#expect(amounts["closing"] == equity.closing)
			let optional: [(String, ReportLine)] = [
				("startingBalances", equity.startingBalances),
				("reconciliationAdjustments", equity.reconciliationAdjustments),
				("revaluations", equity.revaluations),
				("unpairedTransfers", equity.unpairedTransfers),
				("inTransit", equity.inTransit)
			]
			for (id, line) in optional {
				#expect(amounts[id] == (line.total == 0 ? nil : line.total), "\(id)")
			}
			#expect(equity.unexplained == 0, "ST-4 closes on the fixture")
			#expect(amounts["unexplained"] == nil, "unexplained is hidden at zero")
			#expect(document.lines.map(\.title).first == "Opening equity")
			#expect(document.lines.map(\.title).last == "Closing equity")
		}
	}

	@Test func ownersEquityClosesOnTheBalanceSheetForTheSameDay() async throws {
		// ST-3 check 3, as the screen shows both.
		try await RealBudget.with { budget in
			let model = StatementsModel(workspace: budget.workspace, today: Self.today)
			await model.show(.ownersEquity)
			await model.previous()
			let equity = try #require(model.document)
			let closing = try #require(Self.amounts(equity)["closing"])
			await model.show(.balanceSheet)
			await model.setAsOf("2026-10-31")
			let sheet = try #require(model.document)
			#expect(Self.amounts(sheet)["netWorth"] == closing)
		}
	}

	@Test func theIncomeStatementIsNetProfitByCategory() async throws {
		try await RealBudget.with { budget in
			let model = StatementsModel(workspace: budget.workspace, today: Self.today)
			await model.show(.incomeStatement)
			await model.choose(.quarter)
			#expect(model.period == ReportPeriod(first: Month(year: 2026, month: 10), last: Month(year: 2026, month: 12)))
			let report = try #require(await budget.workspace.report { try await $0.netProfit(first: Month(year: 2026, month: 10), last: Month(year: 2026, month: 12)) })
			let amounts = Self.amounts(try #require(model.document))
			#expect(amounts["revenue.total"] == report.total.income.total)
			#expect(amounts["expenses.total"] == report.total.expenses.total)
			#expect(amounts["netIncome"] == report.total.netProfit.total)
			for category in report.total.expenses.categories {
				#expect(amounts["expenses.category.\(category.categoryId ?? "uncategorized")"] == category.total, "\(category.name)")
			}
		}
	}

	@Test func theCashFlowStatementIsTheCoresColumn() async throws {
		try await RealBudget.with { budget in
			let model = StatementsModel(workspace: budget.workspace, today: Self.today)
			await model.show(.cashFlow)
			await model.setCustom(first: Month(year: 2026, month: 1), last: Month(year: 2026, month: 10))
			let flow = try #require(await budget.workspace.report { try await $0.cashFlowStatement(first: "2026-01-01", last: "2026-10-31") })
			let document = try #require(model.document)
			let amounts = Self.amounts(document)
			#expect(amounts["opening"] == flow.opening)
			#expect(amounts["netProfit"] == flow.netProfit.total)
			#expect(amounts["debtPaydown"] == flow.debtPaydown.total)
			#expect(amounts["savings"] == flow.savings.total)
			#expect(amounts["netCashFlow"] == flow.netCashFlow.total)
			#expect(amounts["closing"] == flow.closing)
			#expect(amounts["unreviewed"] == (flow.unreviewedTransfers.total == 0 ? nil : flow.unreviewedTransfers.total))
			#expect(amounts["operations"] == (flow.unreviewedTransfers.total == 0 ? nil : flow.changeFromOperations))
			#expect(amounts["reconciliationAdjustments"] == (flow.reconciliationAdjustments.total == 0 ? nil : flow.reconciliationAdjustments.total))
			#expect(document.basis == "January to October 2026")
		}
	}

	static func amounts(_ document: StatementDocument) -> [String: Int64] {
		Dictionary(document.lines.compactMap { line in line.amount.map { (line.id, $0) } }, uniquingKeysWith: { first, _ in first })
	}
}
