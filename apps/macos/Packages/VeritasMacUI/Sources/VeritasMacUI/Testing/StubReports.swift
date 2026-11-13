#if DEBUG
import Foundation
import VeritasCore
import VeritasKit

/// Reports for the stub budget, built from the approved Net Profit and Sankey
/// mockups' numbers (June to November 2026, October in detail).
final class StubReports: @unchecked Sendable {
	struct Category {
		let id: String
		let name: String
		let group: String
		let groupName: String
		/// October's amount; other months scale it.
		let october: Int64
		let rows: UInt64
		var labels = ReportLabels(hidden: false, archived: false, deleted: false, closedGoal: false, needsReview: false)
	}

	struct MonthFigures {
		let income: Int64
		let expenses: Int64
		let debt: Int64
		let savings: Int64
	}

	static let none = ReportLabels(hidden: false, archived: false, deleted: false, closedGoal: false, needsReview: false)

	static let expenses: [Category] = [
		Category(id: "rent", name: "Rent", group: "housing", groupName: "Housing and utilities", october: 165_000, rows: 1),
		Category(id: "electric", name: "Electric", group: "housing", groupName: "Housing and utilities", october: 8_836, rows: 1),
		Category(id: "internet", name: "Internet", group: "housing", groupName: "Housing and utilities", october: 7_000, rows: 1),
		Category(id: "phone", name: "Phone", group: "housing", groupName: "Housing and utilities", october: 4_500, rows: 1),
		Category(id: "groceries", name: "Groceries", group: "everyday", groupName: "Everyday", october: 64_012, rows: 9),
		Category(id: "dining", name: "Dining out", group: "everyday", groupName: "Everyday", october: 40_138, rows: 11),
		Category(id: "gas", name: "Gas", group: "everyday", groupName: "Everyday", october: 22_100, rows: 5),
		Category(id: "household", name: "Household", group: "everyday", groupName: "Everyday", october: 14_000, rows: 4),
		Category(id: "gifts", name: "Gifts", group: "personal", groupName: "Personal", october: 85_000, rows: 3),
		Category(id: "hobbies", name: "Hobbies", group: "personal", groupName: "Personal", october: 21_118, rows: 4),
		Category(id: "clothing", name: "Clothing", group: "personal", groupName: "Personal", october: 18_000, rows: 2),
		Category(id: "subscriptions", name: "Subscriptions", group: "personal", groupName: "Personal", october: 3_800, rows: 3),
		Category(id: "interest", name: "Loan interest", group: "debt", groupName: "Debt", october: 8_340, rows: 1)
	]

	static let figures: [Int: MonthFigures] = [
		6: MonthFigures(income: 541_200, expenses: 438_820, debt: 31_810, savings: 40_000),
		7: MonthFigures(income: 541_200, expenses: 490_255, debt: 32_070, savings: 40_000),
		8: MonthFigures(income: 569_040, expenses: 610_490, debt: 32_331, savings: 20_000),
		9: MonthFigures(income: 541_200, expenses: 451_137, debt: 32_595, savings: 40_000),
		10: MonthFigures(income: 541_200, expenses: 461_844, debt: 32_860, savings: 40_000),
		11: MonthFigures(income: 387_440, expenses: 310_566, debt: 0, savings: 20_000)
	]

	private let lock = NSLock()
	private var widgets: [DashboardWidget] = []
	private var settings: [String: CustomReportSettings] = [
		"spending": StubReports.settings(id: "spending", name: "Spending by category", groupBy: .category, totalMode: true, graph: "BarGraph"),
		"worth": StubReports.settings(id: "worth", name: "Net worth over time", groupBy: .account, totalMode: false, graph: "StackedBarGraph")
	]

	init() {
		widgets = Self.initialWidgets
	}

	static func figures(_ month: Month) -> MonthFigures {
		figures[Int(month.month)] ?? MonthFigures(income: 541_200, expenses: 460_000, debt: 31_500, savings: 40_000)
	}

	/// The month's categories, scaled from October so they sum to its expenses.
	static func categories(_ month: Month) -> [(Category, Int64, UInt64)] {
		let total = figures(month).expenses
		let october = expenses.reduce(0) { $0 + $1.october }
		var scaled = expenses.map { ($0, $0.october * total / october, $0.rows) }
		let remainder = total - scaled.reduce(0) { $0 + $1.1 }
		if let groceries = scaled.firstIndex(where: { $0.0.id == "groceries" }) { scaled[groceries].1 += remainder }
		if month.month == 8, let dining = scaled.firstIndex(where: { $0.0.id == "dining" }) { scaled[dining].2 += 1 }
		return scaled
	}

	static func line(_ total: Int64, _ count: UInt64) -> ReportLine { ReportLine(total: total, count: count) }

	static func reportCategory(_ category: Category, _ total: Int64, _ count: UInt64) -> ReportCategory {
		ReportCategory(categoryId: category.id, name: category.name, groupId: category.group, groupName: category.groupName, labels: category.labels, total: total, count: count)
	}

	static func column(_ months: [Month]) -> NetProfitColumn {
		var income: Int64 = 0, expenses: Int64 = 0, debt: Int64 = 0, savings: Int64 = 0
		var byCategory: [String: (Category, Int64, UInt64)] = [:]
		for month in months {
			let figures = figures(month)
			income += figures.income
			expenses += figures.expenses
			debt += figures.debt
			savings += figures.savings
			for (category, total, count) in categories(month) {
				let sum = byCategory[category.id].map { ($0.1 + total, $0.2 + count) } ?? (total, count)
				byCategory[category.id] = (category, sum.0, sum.1)
			}
		}
		let lines = Self.expenses.compactMap { byCategory[$0.id] }.map { reportCategory($0.0, $0.1, $0.2) }
		let rows = lines.reduce(0) { $0 + $1.count }
		let interest = Int64(months.count) * 10_000
		let paycheck = Category(id: "paycheck", name: "Paycheck", group: "income", groupName: "Income", october: 0, rows: 2)
		let interestCategory = Category(id: "interest-income", name: "Interest", group: "income", groupName: "Income", october: 0, rows: 1)
		let incomeCount = UInt64(months.count * 3)
		let first = months.first ?? Month(year: 2026, month: 11), last = months.last ?? first
		let netProfit = income - expenses
		return NetProfitColumn(
			first: first.firstDay, last: last.lastDay, month: months.count == 1 ? first : nil,
			income: ReportSection(total: income, count: incomeCount, categories: [
				reportCategory(paycheck, income - interest, UInt64(months.count * 2)),
				reportCategory(interestCategory, interest, UInt64(months.count))
			]),
			expenses: ReportSection(total: expenses, count: rows, categories: lines),
			netProfit: line(netProfit, rows + incomeCount),
			debtPaydown: ReportSection(total: debt, count: debt == 0 ? 0 : UInt64(months.count), categories: []),
			savings: ReportSection(total: savings, count: UInt64(months.count), categories: []),
			netCashFlow: line(netProfit - debt - savings, rows + incomeCount + UInt64(months.count * 2)),
			unreviewed: line(0, 0)
		)
	}

	func netProfit(first: Month, last: Month) -> NetProfitReport {
		let months = ReportPeriod(first: first, last: last).months
		return NetProfitReport(months: months.map { Self.column([$0]) }, total: Self.column(months))
	}

	func netWorth(first: Month, last: Month) -> [NetWorthMonth] {
		ReportPeriod(first: first, last: last).months.map { month in
			let index = Int64(Month(year: 2026, month: 1).months(through: month))
			let assets = 6_420_000 + index * 61_500
			let liabilities = 1_890_000 - index * 32_200
			return NetWorthMonth(month: month, assets: assets, liabilities: liabilities, netWorth: assets - liabilities)
		}
	}

	func cashFlow(first: Month, last: Month) -> [CashFlowColumn] {
		var opening: Int64 = 812_440
		var columns: [CashFlowColumn] = []
		for month in Month(year: 2026, month: 1).months(through: last) >= 0 ? ReportPeriod(first: Month(year: 2026, month: 1), last: last).months : [] {
			let column = Self.cashFlow(Self.column([month]), opening: opening, month: month)
			opening = column.closing
			if month >= first { columns.append(column) }
		}
		return columns
	}

	func cashFlowStatement(first: String, last: String) -> CashFlowColumn {
		let from = Month(date: first) ?? Month(year: 2026, month: 6), to = Month(date: last) ?? from
		let months = cashFlow(first: from, last: to)
		let total = Self.column(ReportPeriod(first: from, last: to).months)
		return Self.cashFlow(total, opening: months.first?.opening ?? 0, month: nil, first: first, last: last)
	}

	static func cashFlow(_ column: NetProfitColumn, opening: Int64, month: Month?, first: String? = nil, last: String? = nil) -> CashFlowColumn {
		let change = column.netCashFlow.total
		return CashFlowColumn(
			first: first ?? column.first, last: last ?? column.last, month: month, opening: opening,
			netProfit: column.netProfit, debtPaydown: line(-column.debtPaydown.total, column.debtPaydown.count),
			savings: line(-column.savings.total, column.savings.count), netCashFlow: column.netCashFlow,
			unreviewedTransfers: line(0, 0), changeFromOperations: change, inTransit: line(0, 0),
			startingBalances: line(0, 0), reconciliationAdjustments: line(0, 0), closing: opening + change
		)
	}

	func sankey(first: String, last: String) -> SankeyReport {
		let from = Month(date: first) ?? Month(year: 2026, month: 10), to = Month(date: last) ?? from
		let column = Self.column(ReportPeriod(first: from, last: to).months)
		var nodes: [SankeyNode] = []
		var flows: [SankeyFlow] = []
		func add(_ kind: SankeyNodeKind, _ id: String?, _ name: String, _ value: Int64, _ count: UInt64, _ column: UInt8) -> UInt32 {
			nodes.append(SankeyNode(key: SankeyNodeKey(kind: kind, id: id), name: name, value: value, labels: Self.none, count: count, column: column))
			return UInt32(nodes.count - 1)
		}
		let sources = column.income.categories.map { ($0, add(.incomeCategory, $0.categoryId, $0.name, $0.total, $0.count, 0)) }
		let income = add(.income, nil, "", column.income.total, column.income.count, 1)
		for (source, index) in sources { flows.append(SankeyFlow(from: index, to: income, value: source.total)) }
		let groups = Dictionary(grouping: column.expenses.categories, by: { $0.groupId ?? "" })
		for groupID in ["housing", "everyday", "personal", "debt"] {
			guard let members = groups[groupID], let firstMember = members.first else { continue }
			let total = members.reduce(0) { $0 + $1.total }
			let group = add(.categoryGroup, groupID, firstMember.groupName ?? "", total, members.reduce(0) { $0 + $1.count }, 2)
			flows.append(SankeyFlow(from: income, to: group, value: total))
			for member in members {
				let child = add(.category, member.categoryId, member.name, member.total, member.count, 3)
				flows.append(SankeyFlow(from: group, to: child, value: member.total))
			}
		}
		for (kind, section) in [(SankeyNodeKind.debtPaydown, column.debtPaydown), (.savings, column.savings)] where section.total > 0 {
			let node = add(kind, nil, "", section.total, section.count, 2)
			flows.append(SankeyFlow(from: income, to: node, value: section.total))
		}
		let left = column.income.total - column.expenses.total - column.debtPaydown.total - column.savings.total
		if left > 0 {
			let node = add(.unallocated, nil, "", left, column.income.count, 2)
			flows.append(SankeyFlow(from: income, to: node, value: left))
		}
		return SankeyReport(first: column.first, last: column.last, nodes: nodes, flows: flows)
	}

	static let payees: [String: [String]] = [
		"groceries": ["Trader Joe's", "Safeway", "Farmers market"],
		"dining": ["Blue Bottle", "Tacos El Gordo", "Pho 99", "Pizza Night"],
		"gas": ["Shell", "Chevron"],
		"rent": ["Parkside Apartments"],
		"gifts": ["Bookshop", "Florist", "Etsy"],
		"paycheck": ["Acme Corp payroll"]
	]

	/// Rows that add up to the number clicked, spread over its dates.
	func transactions(_ drill: ReportDrill) -> [ReportTransaction] {
		let (first, last, total, count, category): (String, String, Int64, UInt64, String?)
		switch drill {
		case .netProfit(let from, let to, let line, let pick):
			let months = ReportPeriod(first: Month(date: from) ?? Month(year: 2026, month: 10), last: Month(date: to) ?? Month(year: 2026, month: 10)).months
			let column = Self.column(months)
			let section: ReportSection? = switch line {
			case .income: column.income
			case .expenses: column.expenses
			case .debtPaydown: column.debtPaydown
			case .savings: column.savings
			default: nil
			}
			if case .category(let id) = pick, let match = section?.categories.first(where: { $0.categoryId == id }) {
				(first, last, total, count, category) = (from, to, match.total, match.count, match.categoryId)
			} else if let section {
				(first, last, total, count, category) = (from, to, section.total, section.count, nil)
			} else {
				let value = line == .netProfit ? column.netProfit : column.netCashFlow
				(first, last, total, count, category) = (from, to, value.total, value.count, nil)
			}
		case .sankeyNode(let from, let to, let key):
			let node = sankey(first: from, last: to).nodes.first { $0.key == key }
			(first, last, total, count, category) = (from, to, node?.value ?? 0, node?.count ?? 0, key.kind == .category ? key.id : nil)
		}
		guard count > 0 else { return [] }
		let start = Month(date: first) ?? Month(year: 2026, month: 10)
		let days = max(start.months(through: Month(date: last) ?? start) * 30 + start.dayCount, 1)
		let names = category.flatMap { Self.payees[$0] } ?? ["Checking", "Corner store", "Hardware store", "Online order"]
		let label = category.flatMap { id in Self.expenses.first { $0.id == id }?.name }
		let share = total / Int64(count)
		return (0..<Int(count)).map { index in
			let amount = index == Int(count) - 1 ? total - share * Int64(count - 1) : share
			let offset = days * index / Int(count)
			let month = start.adding(months: offset / 30)
			let day = min(offset % 30 + 1, month.dayCount)
			return ReportTransaction(
				id: "t\(index)", parentId: nil, date: String(format: "%04d-%02d-%02d", month.year, month.month, day),
				accountId: "checking", accountName: index % 3 == 2 ? "Card" : "Checking",
				payee: names[index % names.count], category: label, notes: nil, amount: -amount
			)
		}
	}

	func customReports() -> [CustomReportItem] {
		lock.withLock {
			settings.values.sorted { $0.name < $1.name }.map { CustomReportItem(id: $0.id, name: $0.name, problem: nil) }
		}
	}

	func customReport(_ id: String) throws -> CustomReportView {
		guard let saved = lock.withLock({ settings[id] }) else { throw FfiError.NotFound(entity: "custom report", id: id) }
		return Self.view(saved)
	}

	static func view(_ settings: CustomReportSettings) -> CustomReportView {
		if settings.balanceType == .budgeted {
			return CustomReportView(settings: settings, data: nil, problem: "Veritas can't evaluate this report yet: the Budgeted balance type")
		}
		let months = ReportPeriod(first: Month(year: 2026, month: 6), last: Month(year: 2026, month: 11)).months
		let balances = { (amount: Int64) in
			CustomReportBalances(totalAssets: max(amount, 0), totalDebts: min(amount, 0), netAssets: max(amount, 0), netDebts: min(amount, 0), totalTotals: amount)
		}
		let cell = { (month: Month, amount: Int64) in
			CustomReportCell(interval: String(format: "%04d-%02d", month.year, month.month), start: month.firstDay, end: month.lastDay, amount: amount, balances: balances(amount), change: 0)
		}
		var groups: [CustomReportGroup]
		switch settings.groupBy {
		case .account:
			groups = [("Checking", 812_440, 3_100), ("Savings", 1_240_000, 40_000), ("Card", -96_250, 2_200)].map { name, base, step in
				CustomReportGroup(id: name.lowercased(), name: name, uncategorized: nil, amount: base + step * 5, balances: balances(base + step * 5),
					intervals: months.enumerated().map { cell($1, base + step * Int64($0)) })
			}
		case .payee:
			groups = payees.sorted { $0.key < $1.key }.flatMap(\.value).prefix(8).enumerated().map { index, name in
				let amount = -Int64(28_000 - index * 2_900)
				return CustomReportGroup(id: name, name: name, uncategorized: nil, amount: amount, balances: balances(amount),
					intervals: months.map { cell($0, amount / 6) })
			}
		default:
			groups = Self.expenses.prefix(8).map { category in
				CustomReportGroup(id: category.id, name: category.name, uncategorized: nil, amount: -category.october * 6, balances: balances(-category.october * 6),
					intervals: months.map { cell($0, -category.october) })
			}
		}
		if settings.sortBy == .ascending { groups.reverse() }
		let total = groups.reduce(0) { $0 + $1.amount }
		let intervals = months.enumerated().map { index, month in
			CustomReportInterval(
				interval: cell(month, groups.reduce(0) { $0 + $1.intervals[index].amount }),
				stacked: groups.map { StackedAmount(key: $0.id, amount: $0.intervals[index].amount) }
			)
		}
		return CustomReportView(
			settings: settings,
			data: CustomReportData(start: "2026-06-01", end: "2026-11-30", groups: groups, intervals: intervals, total: total, totals: balances(total)),
			problem: nil
		)
	}

	static func settings(id: String, name: String, groupBy: ReportGrouping, totalMode: Bool, graph: String) -> CustomReportSettings {
		CustomReportSettings(
			id: id, name: name, startDate: "2026-06", endDate: "2026-11", dateStatic: false, dateRange: "Last 6 months",
			totalMode: totalMode, groupBy: groupBy, balanceType: groupBy == .account ? .net : .payment,
			sortBy: .descending, interval: .monthly, graphType: graph, showEmpty: false, showOffBudget: false,
			showHidden: false, showUncategorized: false, includeCurrent: true, trimIntervals: false, conditionCount: 0
		)
	}

	func save(_ settings: CustomReportSettings) throws {
		let name = settings.name.trimmingCharacters(in: .whitespaces)
		guard !name.isEmpty else { throw FfiError.Invalid(message: "Enter a name for the report.") }
		lock.withLock {
			var saved = settings
			saved.name = name
			self.settings[settings.id] = saved
		}
	}

	func dashboards() -> [DashboardPage] {
		let widgets = lock.withLock { self.widgets }
		let report = Self.view(lock.withLock { settings["spending"] } ?? Self.settings(id: "spending", name: "Spending by category", groupBy: .category, totalMode: true, graph: "BarGraph"))
		let views: [String: WidgetView] = [
			"worth": .netWorth(points: netWorth(first: Month(year: 2026, month: 6), last: Month(year: 2026, month: 11))),
			"flow": .cashFlow(first: Month(year: 2026, month: 11), last: Month(year: 2026, month: 11), income: 387_440, expenses: 310_566, netProfit: 76_874),
			"spending": .spending(month: Month(year: 2026, month: 11), spent: 310_566, compareMonth: Month(year: 2026, month: 10), compareSpent: 461_844),
			"report": .customReport(report: report),
			"notes": .markdown(text: "## Budget notes\n\nPay the card **in full** every month.", align: "left"),
			"sankey": .sankey(report: sankey(first: "2026-10-01", last: "2026-10-31"))
		]
		return [
			DashboardPage(id: "main", name: "Main", widgets: widgets.map { widget in
				var widget = widget
				if let view = views[widget.id] { widget.view = view }
				return widget
			}),
			DashboardPage(id: "deep", name: "Deep dive", widgets: [])
		]
	}

	func place(_ id: String, x: Int64, y: Int64, width: Int64, height: Int64) throws {
		try lock.withLock {
			guard let index = widgets.firstIndex(where: { $0.id == id }) else { throw FfiError.NotFound(entity: "widget", id: id) }
			widgets[index].x = x
			widgets[index].y = y
			widgets[index].width = width
			widgets[index].height = height
		}
	}

	static let initialWidgets: [DashboardWidget] = [
		DashboardWidget(id: "worth", kind: "net-worth-card", name: "Net worth", x: 0, y: 0, width: 6, height: 2, minWidth: 3, minHeight: 2, view: .openInActual(reason: .notNative)),
		DashboardWidget(id: "flow", kind: "cash-flow-card", name: "Cash flow", x: 6, y: 0, width: 3, height: 2, minWidth: 2, minHeight: 2, view: .openInActual(reason: .notNative)),
		DashboardWidget(id: "spending", kind: "spending-card", name: "This month", x: 9, y: 0, width: 3, height: 2, minWidth: 3, minHeight: 2, view: .openInActual(reason: .notNative)),
		DashboardWidget(id: "report", kind: "custom-report", name: "Spending by category", x: 0, y: 2, width: 6, height: 3, minWidth: 2, minHeight: 1, view: .openInActual(reason: .notNative)),
		DashboardWidget(id: "notes", kind: "markdown-card", name: nil, x: 6, y: 2, width: 3, height: 2, minWidth: 2, minHeight: 1, view: .openInActual(reason: .notNative)),
		DashboardWidget(id: "summary", kind: "summary-card", name: "Total income (YTD)", x: 9, y: 2, width: 3, height: 2, minWidth: 2, minHeight: 2, view: .openInActual(reason: .notNative)),
		DashboardWidget(id: "sankey", kind: "sankey-card", name: "Where it went", x: 0, y: 5, width: 8, height: 3, minWidth: 3, minHeight: 3, view: .openInActual(reason: .notNative)),
		DashboardWidget(id: "future", kind: "future-widget-card", name: nil, x: 8, y: 5, width: 4, height: 2, minWidth: 1, minHeight: 1, view: .openInActual(reason: .unknownType))
	]
}
#endif
