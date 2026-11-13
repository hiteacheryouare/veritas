import Foundation
import Observation
import VeritasCore

/// R-REP-5's four reports.
public enum StandardReport: String, CaseIterable, Hashable, Sendable {
	case netWorth
	case cashFlow
	case spendingByCategory
	case incomeVsExpense

	public var title: String {
		switch self {
		case .netWorth: "Net worth"
		case .cashFlow: "Cash flow"
		case .spendingByCategory: "Spending by category"
		case .incomeVsExpense: "Income vs expense"
		}
	}
}

/// A labeled row of numbers, one per column.
public struct ReportRow: Identifiable, Hashable, Sendable {
	public let id: String
	public let title: String
	public let values: [Int64]
	public let isTotal: Bool
	public let tones: [MoneyTone]

	public init(id: String, title: String, values: [Int64], isTotal: Bool = false, tones: [MoneyTone]? = nil) {
		self.id = id
		self.title = title
		self.values = values
		self.isTotal = isTotal
		self.tones = tones ?? values.map { _ in .neutral }
	}
}

public struct SpendingLine: Identifiable, Hashable, Sendable {
	public let id: String
	public let title: String
	public let amount: Int64
	public let count: UInt64
	public let drill: ReportDrill
}

@MainActor
@Observable
public final class StandardReportModel {
	public private(set) var kind: StandardReport
	public private(set) var span: MonthSpan = .sixMonths
	public private(set) var period: ReportPeriod
	public private(set) var netWorth: [NetWorthMonth] = []
	public private(set) var cashFlow: [CashFlowColumn] = []
	public private(set) var cashFlowTotal: CashFlowColumn?
	public private(set) var netProfit: NetProfitReport?

	@ObservationIgnored let workspace: WorkspaceModel
	@ObservationIgnored public let thisMonth: Month

	public init(workspace: WorkspaceModel, kind: StandardReport = .netWorth, thisMonth: Month? = nil) {
		self.workspace = workspace
		self.kind = kind
		let month = thisMonth ?? Month(date: WorkspaceModel.today()) ?? Month(year: 2026, month: 1)
		self.thisMonth = month
		period = MonthSpan.sixMonths.period(endingIn: month, current: ReportPeriod(first: month, last: month))
	}

	public var format: MoneyFormat { workspace.format }
	public var canGoForward: Bool { period.last < thisMonth }

	public func show(_ kind: StandardReport) async {
		self.kind = kind
		await load()
	}

	public func choose(_ span: MonthSpan) async {
		self.span = span
		period = span.period(endingIn: min(period.last, thisMonth), current: period)
		await load()
	}

	public func setCustom(first: Month, last: Month) async {
		span = .custom
		period = ReportPeriod(first: first, last: last)
		await load()
	}

	public func previous() async {
		period = period.shifted(by: span == .year ? -12 : -period.length)
		await load()
	}

	public func next() async {
		guard canGoForward else { return }
		period = period.shifted(by: span == .year ? 12 : period.length)
		await load()
	}

	public func load() async {
		let period = period
		switch kind {
		case .netWorth:
			if let points = await workspace.report({ try await $0.netWorth(first: period.first, last: period.last) }) {
				netWorth = points
			}
		case .cashFlow:
			let loaded = await workspace.report { session in
				(
					try await session.cashFlow(first: period.first, last: period.last),
					try await session.cashFlowStatement(first: period.first.firstDay, last: period.last.lastDay)
				)
			}
			if let loaded {
				cashFlow = loaded.0
				cashFlowTotal = loaded.1
			}
		case .spendingByCategory, .incomeVsExpense:
			if let report = await workspace.report({ try await $0.netProfit(first: period.first, last: period.last) }) {
				netProfit = report
			}
		}
	}

	public var headings: [String] {
		period.months.map(ReportPeriod.shortMonth)
	}

	public var rangeHeading: String { "\(period.length) months" }

	public var netWorthRows: [ReportRow] {
		[
			ReportRow(id: "assets", title: "Assets", values: netWorth.map(\.assets)),
			ReportRow(id: "liabilities", title: "Liabilities", values: netWorth.map(\.liabilities)),
			ReportRow(id: "netWorth", title: "Net worth", values: netWorth.map(\.netWorth), isTotal: true,
				tones: netWorth.map { $0.netWorth < 0 ? .overspent : .neutral })
		]
	}

	/// Fred's compact Cash flow (ST-6): the always-shown lines, plus the
	/// reconciling ones only when they aren't zero, so the column still adds
	/// up on its face. The last column is the whole range.
	public var cashFlowRows: [ReportRow] {
		guard let total = cashFlowTotal else { return [] }
		let columns = cashFlow + [total]
		func row(_ id: String, _ title: String, isTotal: Bool = false, _ value: (CashFlowColumn) -> Int64) -> ReportRow {
			let values = columns.map(value)
			return ReportRow(id: id, title: title, values: values, isTotal: isTotal,
				tones: isTotal ? values.map { $0 < 0 ? .overspent : .neutral } : nil)
		}
		let optional = { (row: ReportRow) in row.values.contains { $0 != 0 } ? row : nil }
		let unreviewed = optional(row("unreviewed", "Unreviewed transfers") { $0.unreviewedTransfers.total })
		return [
			row("opening", "Budget accounts balance") { $0.opening },
			row("netProfit", "Net Profit") { $0.netProfit.total },
			row("debtPaydown", "Debt paydown") { $0.debtPaydown.total },
			row("savings", "Savings contributions") { $0.savings.total },
			row("netCashFlow", "Net cash flow", isTotal: true) { $0.netCashFlow.total },
			unreviewed,
			unreviewed.map { _ in row("operations", "Change in budget cash from operations") { $0.changeFromOperations } },
			optional(row("inTransit", "Transfers in transit between budget accounts") { $0.inTransit.total }),
			optional(row("starting", "Starting balances in budget accounts") { $0.startingBalances.total }),
			optional(row("reconciliation", "Reconciliation adjustments in budget accounts") { $0.reconciliationAdjustments.total }),
			row("closing", "Budget accounts balance", isTotal: true) { $0.closing }
		].compactMap { $0 }.enumerated().map { index, row in
			ReportRow(id: "\(index).\(row.id)", title: row.title, values: row.values, isTotal: row.isTotal, tones: row.tones)
		}
	}

	public var cashFlowHeadings: [String] {
		cashFlow.map { $0.month.map(ReportPeriod.shortMonth) ?? $0.first } + [rangeHeading]
	}

	/// Every expense category over the range, largest first. Refund-heavy
	/// categories stay in with their minus sign, so the list adds up to
	/// Expenses.
	public var spending: [SpendingLine] {
		guard let total = netProfit?.total else { return [] }
		return total.expenses.categories
			.sorted { $0.total > $1.total }
			.map { category in
				SpendingLine(
					id: category.categoryId ?? "-",
					title: ReportWords.category(category.name, labels: category.labels),
					amount: category.total,
					count: category.count,
					drill: .netProfit(first: total.first, last: total.last, line: .expenses,
						category: category.categoryId.map { .category(id: $0) } ?? .uncategorized)
				)
			}
	}

	public var spendingTotal: Int64 { netProfit?.total.expenses.total ?? 0 }

	public var incomeVsExpenseRows: [ReportRow] {
		guard let report = netProfit else { return [] }
		let columns = report.months + [report.total]
		return [
			ReportRow(id: "income", title: "Income", values: columns.map(\.income.total), tones: columns.map { _ in .income }),
			ReportRow(id: "expenses", title: "Expenses", values: columns.map(\.expenses.total)),
			ReportRow(id: "netProfit", title: "Net Profit", values: columns.map(\.netProfit.total), isTotal: true,
				tones: columns.map { $0.netProfit.total < 0 ? .overspent : .neutral })
		]
	}

	public func drilldown(_ line: SpendingLine) -> DrilldownModel {
		DrilldownModel(workspace: workspace, title: "\(line.title), \(period.title)", drill: line.drill)
	}
}
