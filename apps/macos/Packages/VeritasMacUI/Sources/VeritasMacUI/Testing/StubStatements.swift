#if DEBUG
import Foundation
import VeritasCore
import VeritasKit

/// The stub budget's statements, tied to StubReports so the four agree:
/// net worth matches the Net worth report, budget cash matches the Cash flow
/// report, and owner's equity runs from one month-end net worth to the next.
enum StubStatements {
	static let savings: Int64 = 841_200
	static let oldSavings: Int64 = 12_000
	static let visa: Int64 = 61_233
	/// A bike bought from checking into an off-budget asset in September 2026 (ST-4 E2).
	static let bike = (month: Month(year: 2026, month: 9), amount: Int64(25_000))
	static let reconciliationPerMonth: Int64 = -1_200

	static func balanceSheet(asOf: String) throws -> BalanceSheet {
		guard let month = Month(date: asOf) else { throw FfiError.Invalid(message: "\(asOf) isn't a date.") }
		let worth = netWorth(endOf: month)
		let budgetCash = StubReports().cashFlow(first: month, last: month).last?.closing ?? 812_440
		let checking = budgetCash + visa
		let brokerage = worth.assets - checking - savings - oldSavings
		let carLoan = worth.liabilities - visa
		return BalanceSheet(
			asOf: asOf,
			assets: [
				SheetSection(groupId: "cash", groupName: "Cash", accounts: [
					account("checking", "Checking", checking),
					account("savings", "Savings", savings),
					account("old-savings", "Old savings", oldSavings, closed: true)
				], total: checking + savings + oldSavings),
				SheetSection(groupId: "investments", groupName: "Investments", accounts: [account("brokerage", "Brokerage", brokerage)], total: brokerage)
			],
			totalAssets: worth.assets,
			liabilities: [
				SheetSection(groupId: "credit", groupName: "Credit", accounts: [account("card", "Visa", visa)], total: visa),
				SheetSection(groupId: "loans", groupName: "Loans", accounts: [account("car-loan", "Car loan", carLoan)], total: carLoan)
			],
			totalLiabilities: worth.liabilities,
			netWorth: worth.netWorth
		)
	}

	static func ownersEquity(first: String, last: String) throws -> OwnersEquity {
		guard let from = Month(date: first), let to = Month(date: last) else { throw FfiError.Invalid(message: "\(first) to \(last) isn't a range.") }
		let months = ReportPeriod(first: from, last: to).months
		let opening = netWorth(endOf: from.adding(months: -1)).netWorth
		let closing = netWorth(endOf: to).netWorth
		let netIncome = StubReports.column(months).netProfit
		let capitalized = months.contains(bike.month) ? ReportLine(total: bike.amount, count: 1) : ReportLine(total: 0, count: 0)
		let reconciliation = ReportLine(total: reconciliationPerMonth * Int64(months.count), count: UInt64(months.count))
		let revaluation = closing - opening - netIncome.total - capitalized.total - reconciliation.total
		let none = ReportLine(total: 0, count: 0)
		return OwnersEquity(
			first: first, last: last, opening: opening,
			netIncome: netIncome, capitalizedTransfers: capitalized, startingBalances: none,
			reconciliationAdjustments: reconciliation, revaluations: ReportLine(total: revaluation, count: UInt64(months.count)),
			unpairedTransfers: none, inTransit: none, closing: closing, unexplained: 0
		)
	}

	/// Net Profit's column for the months, grouped the way the core groups it.
	static func incomeStatement(first: String, last: String) throws -> IncomeStatement {
		guard let from = Month(date: first), let to = Month(date: last) else { throw FfiError.Invalid(message: "\(first) to \(last) isn't a range.") }
		let column = StubReports.column(ReportPeriod(first: from, last: to).months)
		func groups(_ categories: [ReportCategory]) -> [StatementGroup] {
			var groups: [StatementGroup] = []
			for category in categories {
				if let index = groups.lastIndex(where: { $0.groupId == category.groupId }) {
					groups[index].categories.append(category)
					groups[index].total += category.total
				} else {
					groups.append(StatementGroup(groupId: category.groupId, groupName: category.groupName, categories: [category], total: category.total))
				}
			}
			return groups
		}
		return IncomeStatement(
			first: first, last: last,
			revenue: groups(column.income.categories), totalRevenue: ReportLine(total: column.income.total, count: column.income.count),
			expenses: groups(column.expenses.categories), totalExpenses: ReportLine(total: column.expenses.total, count: column.expenses.count),
			netIncome: column.netProfit
		)
	}

	private static func netWorth(endOf month: Month) -> NetWorthMonth {
		StubReports().netWorth(first: month, last: month)[0]
	}

	private static func account(_ id: String, _ name: String, _ amount: Int64, closed: Bool = false) -> SheetAccount {
		SheetAccount(accountId: id, name: name, amount: amount, closed: closed, deleted: false)
	}
}
#endif
