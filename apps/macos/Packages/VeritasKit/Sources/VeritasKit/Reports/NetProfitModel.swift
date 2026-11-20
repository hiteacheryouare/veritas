import Foundation
import Observation
import VeritasCore

/// One number in the Net Profit table and the rows behind it.
public struct NetProfitCell: Hashable, Sendable {
	public let amount: Int64
	public let count: UInt64
	public let tone: MoneyTone
	public let drill: ReportDrill
	/// What the drilldown is titled: "Expenses, August 2026".
	public let title: String
}

public struct NetProfitRow: Identifiable, Hashable, Sendable {
	public enum Style: Hashable, Sendable {
		case line
		case total
		case below
		case category
		case note
	}

	public let id: String
	public let title: String
	public let line: NetProfitLine
	public let style: Style
	/// Rows whose categories can be shown underneath.
	public let canExpand: Bool
	/// One cell per month, then the range total.
	public let cells: [NetProfitCell]
}

/// The Net Profit screen (R-REP-2, ST-1, design/mockups/net-profit.html):
/// income, expenses, Net Profit, then debt paydown, savings and net cash flow
/// for each month and the range.
@MainActor
@Observable
public final class NetProfitModel {
	public private(set) var span: MonthSpan = .sixMonths
	public private(set) var period: ReportPeriod
	public private(set) var report: NetProfitReport?
	public var expanded: Set<NetProfitLine> = []

	@ObservationIgnored let workspace: WorkspaceModel
	@ObservationIgnored public let thisMonth: Month

	public init(workspace: WorkspaceModel, thisMonth: Month? = nil) {
		self.workspace = workspace
		let month = thisMonth ?? Month(date: WorkspaceModel.today()) ?? Month(year: 2026, month: 1)
		self.thisMonth = month
		period = MonthSpan.sixMonths.period(endingIn: month, current: ReportPeriod(first: month, last: month))
	}

	public var format: MoneyFormat { workspace.format }
	public var canGoForward: Bool { period.last < thisMonth }
	public var canGoBack: Bool { true }

	public func load() async {
		let period = period
		guard let report = await workspace.report({ try await $0.netProfit(first: period.first, last: period.last) }) else { return }
		guard period == self.period else { return }
		self.report = report
	}

	public func choose(_ span: MonthSpan) async {
		self.span = span
		period = span.period(endingIn: min(period.last, thisMonth), current: period)
		await load()
	}

	public func setCustom(first: Month, last: Month) async {
		span = .custom
		period = ReportPeriod(first: first, last: min(last, thisMonth.adding(months: 12)))
		await load()
	}

	public func previous() async {
		period = period.shifted(by: -step)
		await load()
	}

	public func next() async {
		guard canGoForward else { return }
		period = period.shifted(by: step)
		await load()
	}

	private var step: Int { span == .year ? 12 : period.length }

	public var showsUnreviewed: Bool {
		guard let report else { return false }
		return (report.months + [report.total]).contains { $0.unreviewed.total != 0 }
	}

	public func toggle(_ line: NetProfitLine) {
		if expanded.contains(line) { expanded.remove(line) } else { expanded.insert(line) }
	}

	public var columns: [NetProfitColumn] {
		guard let report else { return [] }
		return report.months + [report.total]
	}

	public func heading(_ column: NetProfitColumn) -> String {
		column.month.map(ReportPeriod.shortMonth) ?? "\(period.length) months"
	}

	public var rows: [NetProfitRow] {
		guard report != nil else { return [] }
		var rows: [NetProfitRow] = []
		for line in [NetProfitLine.income, .expenses] {
			rows.append(row(line, style: .line))
			if expanded.contains(line) { rows += categoryRows(line) }
		}
		rows.append(row(.netProfit, style: .total))
		for line in [NetProfitLine.debtPaydown, .savings] {
			rows.append(row(line, style: .below))
			if expanded.contains(line) { rows += categoryRows(line) }
		}
		rows.append(row(.netCashFlow, style: .total))
		if showsUnreviewed { rows.append(row(.unreviewed, style: .note)) }
		return rows
	}

	private func row(_ line: NetProfitLine, style: NetProfitRow.Style) -> NetProfitRow {
		let cells = columns.map { column in
			let value = Self.value(line, in: column)
			return cell(line, category: nil, amount: value.total, count: value.count, column: column, title: ReportWords.line(line))
		}
		let canExpand = Self.section(line, in: report?.total).map { !$0.categories.isEmpty } ?? false
		return NetProfitRow(id: "\(line)", title: ReportWords.line(line), line: line, style: style, canExpand: canExpand, cells: cells)
	}

	private func categoryRows(_ line: NetProfitLine) -> [NetProfitRow] {
		guard let total = Self.section(line, in: report?.total) else { return [] }
		return total.categories.map { category in
			let title = ReportWords.category(category.name, labels: category.labels)
			let cells = columns.map { column in
				let match = Self.section(line, in: column)?.categories.first { $0.categoryId == category.categoryId }
				return cell(line, category: category, amount: match?.total ?? 0, count: match?.count ?? 0, column: column, title: title)
			}
			return NetProfitRow(id: "\(line).\(category.categoryId ?? "-")", title: title, line: line, style: .category, canExpand: false, cells: cells)
		}
	}

	private func cell(_ line: NetProfitLine, category: ReportCategory?, amount: Int64, count: UInt64, column: NetProfitColumn, title: String) -> NetProfitCell {
		let drillCategory = category.map { $0.categoryId.map { DrillCategory.category(id: $0) } ?? .uncategorized }
		let when = column.month.map(ReportPeriod.monthYear) ?? period.title
		return NetProfitCell(
			amount: amount,
			count: count,
			tone: Self.tone(line, amount: amount, isCategory: category != nil),
			drill: .netProfit(first: column.first, last: column.last, line: line, category: drillCategory),
			title: "\(title), \(when)"
		)
	}

	/// Fred's ruling: only a negative Net Profit or Net cash flow is a loss.
	/// Negative expenses are refunds winning, and stay neutral.
	static func tone(_ line: NetProfitLine, amount: Int64, isCategory: Bool) -> MoneyTone {
		switch line {
		case .income: isCategory ? .neutral : .income
		case .netProfit, .netCashFlow: amount < 0 ? .overspent : .neutral
		default: .neutral
		}
	}

	static func section(_ line: NetProfitLine, in column: NetProfitColumn?) -> ReportSection? {
		guard let column else { return nil }
		switch line {
		case .income: return column.income
		case .expenses: return column.expenses
		case .debtPaydown: return column.debtPaydown
		case .savings: return column.savings
		default: return nil
		}
	}

	static func value(_ line: NetProfitLine, in column: NetProfitColumn) -> ReportLine {
		switch line {
		case .income: ReportLine(total: column.income.total, count: column.income.count)
		case .expenses: ReportLine(total: column.expenses.total, count: column.expenses.count)
		case .netProfit: column.netProfit
		case .debtPaydown: ReportLine(total: column.debtPaydown.total, count: column.debtPaydown.count)
		case .savings: ReportLine(total: column.savings.total, count: column.savings.count)
		case .netCashFlow: column.netCashFlow
		case .unreviewed: column.unreviewed
		}
	}
}

/// The transactions behind one report number (ST-1.6, ST-8 rule 6).
@MainActor
@Observable
public final class DrilldownModel: Identifiable {
	public let id = UUID()
	public let title: String
	/// Shown under the title, such as Unallocated's caption.
	public let caption: String?
	public let drill: ReportDrill
	public private(set) var transactions: [ReportTransaction]?

	@ObservationIgnored let workspace: WorkspaceModel

	public init(workspace: WorkspaceModel, title: String, caption: String? = nil, drill: ReportDrill) {
		self.workspace = workspace
		self.title = title
		self.caption = caption
		self.drill = drill
	}

	public var format: MoneyFormat { workspace.format }

	public func load() async {
		let drill = drill
		transactions = await workspace.report { try await $0.reportTransactions(drill) } ?? []
	}
}
