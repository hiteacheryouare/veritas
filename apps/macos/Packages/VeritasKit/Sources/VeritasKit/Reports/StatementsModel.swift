import Foundation
import Observation
import VeritasCore

/// The four statements of R-REP-7 (ST-3).
public enum StatementKind: String, CaseIterable, Hashable, Sendable {
	case balanceSheet
	case incomeStatement
	case ownersEquity
	case cashFlow

	public var title: String {
		switch self {
		case .balanceSheet: "Balance sheet"
		case .incomeStatement: "Income statement"
		case .ownersEquity: "Owner's equity"
		case .cashFlow: "Cash flow"
		}
	}

	/// The heading on the statement itself.
	public var documentTitle: String {
		switch self {
		case .balanceSheet: "Balance sheet"
		case .incomeStatement: "Income statement"
		case .ownersEquity: "Statement of owner's equity"
		case .cashFlow: "Cash flow statement"
		}
	}

	/// The balance sheet is a point in time; the others cover a period.
	public var isDated: Bool { self == .balanceSheet }
}

/// One row of a statement, as design/mockups/statement.html rules it.
public struct StatementLine: Identifiable, Hashable, Sendable {
	public enum Style: Hashable, Sendable {
		case section
		case group
		/// A balance carried in, flush left with no rule: opening equity.
		case balance
		case line
		case subtotal
		case total
		case grand
	}

	public let id: String
	public let title: String
	/// Register scale (CLS-12.9). Headings have none.
	public let amount: Int64?
	public let style: Style

	public init(id: String, title: String, amount: Int64? = nil, style: Style) {
		self.id = id
		self.title = title
		self.amount = amount
		self.style = style
	}
}

/// A statement ready to lay out on screen or paper: ST-3's header and its lines.
public struct StatementDocument: Hashable, Sendable {
	public static let unaudited = "Unaudited. Obviously."

	public let kind: StatementKind
	/// The budget's name, standing in for the entity name.
	public let entity: String
	/// "As of November 9, 2026, in USD", "June to November 2026, in USD".
	public let basis: String
	public let lines: [StatementLine]
	/// ST-4's `unexplained`, register scale, when it isn't zero. It is never a
	/// line in the column, only the notice below the statement.
	public let unexplained: Int64?

	public init(kind: StatementKind, entity: String, basis: String, lines: [StatementLine], unexplained: Int64? = nil) {
		self.kind = kind
		self.entity = entity
		self.basis = basis
		self.lines = lines
		self.unexplained = unexplained
	}

	public var title: String { kind.documentTitle }

	/// ST-4's notice: a positive `unexplained` means the lines fall short of
	/// closing equity, a negative one that they go over it.
	public func notice(format: MoneyFormat) -> String? {
		guard let unexplained else { return nil }
		let amount = format.currencyMagnitude(unexplained, on: .register)
		let direction = unexplained > 0 ? "short of" : "over"
		return "The lines above are \(amount) \(direction) closing equity. Check the audit."
	}

	/// "Household Balance sheet 2026-11-09.pdf".
	public func fileName(period: String) -> String {
		"\(entity) \(title) \(period).pdf".replacingOccurrences(of: "/", with: "-")
	}
}

/// The Statements screen (R-REP-7, design/mockups/statement.html): the
/// balance sheet as of a date, and the income statement, owner's equity and
/// cash flow statement for a period of whole months. Every figure is the
/// core's; this model only picks which lines show.
@MainActor
@Observable
public final class StatementsModel {
	public private(set) var kind: StatementKind = .balanceSheet
	/// `YYYY-MM-DD`.
	public private(set) var asOf: String
	public private(set) var span: SankeySpan = .month
	public private(set) var period: ReportPeriod
	public private(set) var balanceSheet: BalanceSheet?
	public private(set) var netProfit: NetProfitReport?
	public private(set) var ownersEquity: OwnersEquity?
	public private(set) var cashFlow: CashFlowColumn?

	@ObservationIgnored let workspace: WorkspaceModel
	@ObservationIgnored public let today: String
	@ObservationIgnored public let thisMonth: Month

	public init(workspace: WorkspaceModel, today: String? = nil) {
		self.workspace = workspace
		let today = today ?? WorkspaceModel.today()
		self.today = today
		let month = Month(date: today) ?? Month(year: 2026, month: 1)
		thisMonth = month
		asOf = today
		period = ReportPeriod(first: month, last: month)
	}

	public var format: MoneyFormat { workspace.format }

	public func show(_ kind: StatementKind) async {
		self.kind = kind
		await load()
	}

	public func load() async {
		switch kind {
		case .balanceSheet:
			let asOf = asOf
			guard let sheet = await workspace.report({ try await $0.balanceSheet(asOf: asOf) }), asOf == self.asOf else { return }
			balanceSheet = sheet
		case .incomeStatement:
			let period = period
			guard let report = await workspace.report({ try await $0.netProfit(first: period.first, last: period.last) }), period == self.period else { return }
			netProfit = report
		case .ownersEquity:
			let period = period
			guard let equity = await workspace.report({ try await $0.ownersEquity(first: period.first.firstDay, last: period.last.lastDay) }), period == self.period else { return }
			ownersEquity = equity
		case .cashFlow:
			let period = period
			guard let flow = await workspace.report({ try await $0.cashFlowStatement(first: period.first.firstDay, last: period.last.lastDay) }), period == self.period else { return }
			cashFlow = flow
		}
	}

	// MARK: Period

	public func setAsOf(_ date: String) async {
		asOf = min(date, today)
		await load()
	}

	public func choose(_ span: SankeySpan) async {
		self.span = span
		period = span.period(containing: min(period.last, thisMonth), current: period)
		await load()
	}

	public func setCustom(first: Month, last: Month) async {
		span = .custom
		period = ReportPeriod(first: first, last: last)
		await load()
	}

	public var canGoForward: Bool {
		kind.isDated ? asOf < today : period.last < thisMonth
	}

	/// The balance sheet steps to the end of the month before; periods step by their length.
	public func previous() async {
		if kind.isDated {
			guard let month = Month(date: asOf) else { return }
			asOf = month.adding(months: -1).lastDay
		} else {
			period = period.shifted(by: -step)
		}
		await load()
	}

	public func next() async {
		guard canGoForward else { return }
		if kind.isDated {
			guard let month = Month(date: asOf) else { return }
			let end = asOf == month.lastDay ? month.adding(months: 1).lastDay : month.lastDay
			asOf = min(end, today)
		} else {
			period = period.shifted(by: step)
		}
		await load()
	}

	private var step: Int { span == .year ? 12 : period.length }

	/// The toolbar's period button: "As of Nov 9, 2026", "Q4 2026", "June to November 2026".
	public var periodLabel: String {
		kind.isDated ? "As of \(Self.date(asOf, style: .abbreviated))" : span.title(of: period)
	}

	/// What a saved PDF is named after: the date or the months it covers.
	public var periodFileName: String {
		kind.isDated ? asOf : (period.first == period.last ? String(period.first.firstDay.prefix(7)) : "\(period.first.firstDay.prefix(7)) to \(period.last.firstDay.prefix(7))")
	}

	// MARK: Document

	/// The statement on show, or `nil` until it has loaded.
	public var document: StatementDocument? {
		let lines: [StatementLine]? = switch kind {
		case .balanceSheet: balanceSheet.map(Self.lines)
		case .incomeStatement: netProfit.map { Self.lines($0.total) }
		case .ownersEquity: ownersEquity.map(Self.lines)
		case .cashFlow: cashFlow.map(Self.lines)
		}
		guard let lines else { return nil }
		let unexplained = kind == .ownersEquity ? ownersEquity.flatMap { $0.unexplained == 0 ? nil : $0.unexplained } : nil
		return StatementDocument(kind: kind, entity: workspace.overview?.name ?? "Budget", basis: basis, lines: lines, unexplained: unexplained)
	}

	private var basis: String {
		let when = kind.isDated ? "As of \(Self.date(asOf, style: .wide))" : period.title
		guard let code = workspace.overview?.currencyCode else { return when }
		return "\(when), in \(code)"
	}

	static func date(_ text: String, style: Date.FormatStyle.Symbol.Month) -> String {
		let parts = text.split(separator: "-").compactMap { Int($0) }
		guard parts.count == 3, let date = Calendar.current.date(from: DateComponents(year: parts[0], month: parts[1], day: parts[2])) else { return text }
		return date.formatted(.dateTime.month(style).day().year())
	}

	/// ST-3 and ST-2: assets and liabilities by account group, each as the
	/// core totals them. A group's subtotal shows when it has more than one account.
	static func lines(_ sheet: BalanceSheet) -> [StatementLine] {
		func side(_ id: String, _ title: String, _ sections: [SheetSection], total: Int64, totalTitle: String) -> [StatementLine] {
			var lines = [StatementLine(id: id, title: title, style: .section)]
			for section in sections {
				let group = section.groupId ?? "none"
				if let name = section.groupName {
					lines.append(StatementLine(id: "\(id).group.\(group)", title: name, style: .group))
				}
				for account in section.accounts {
					lines.append(StatementLine(id: "account.\(account.accountId)", title: accountTitle(account), amount: account.amount, style: .line))
				}
				if let name = section.groupName, section.accounts.count > 1 {
					lines.append(StatementLine(id: "\(id).subtotal.\(group)", title: "Total \(lowercasedFirstWord(name))", amount: section.total, style: .subtotal))
				}
			}
			lines.append(StatementLine(id: "\(id).total", title: totalTitle, amount: total, style: .total))
			return lines
		}
		return side("assets", "Assets", sheet.assets, total: sheet.totalAssets, totalTitle: "Total assets")
			+ side("liabilities", "Liabilities", sheet.liabilities, total: sheet.totalLiabilities, totalTitle: "Total liabilities")
			+ [StatementLine(id: "netWorth", title: "Net worth", amount: sheet.netWorth, style: .grand)]
	}

	static func accountTitle(_ account: SheetAccount) -> String {
		if account.deleted { return "\(account.name) (deleted)" }
		if account.closed { return "\(account.name) (closed)" }
		return account.name
	}

	/// "Total cash" for the group Cash, but "Total HSA" stays as written.
	static func lowercasedFirstWord(_ name: String) -> String {
		let first = name.prefix { !$0.isWhitespace }
		guard first.count > 1, first != first.uppercased() else { return name }
		return name.prefix(1).lowercased() + name.dropFirst()
	}

	/// ST-3's income statement: revenue and expenses by category group then
	/// category, and Net income, which is Net Profit (ST-1).
	static func lines(_ column: NetProfitColumn) -> [StatementLine] {
		func section(_ id: String, _ title: String, _ section: ReportSection, totalTitle: String) -> [StatementLine] {
			var lines = [StatementLine(id: id, title: title, style: .section)]
			var group: String??
			for category in section.categories {
				if category.groupId != group, let name = category.groupName {
					lines.append(StatementLine(id: "\(id).group.\(category.groupId ?? "none")", title: name, style: .group))
				}
				group = category.groupId
				lines.append(StatementLine(
					id: "\(id).category.\(category.categoryId ?? "uncategorized")",
					title: ReportWords.category(category.name, labels: category.labels),
					amount: category.total,
					style: .line
				))
			}
			lines.append(StatementLine(id: "\(id).total", title: totalTitle, amount: section.total, style: .total))
			return lines
		}
		return section("revenue", "Revenue", column.income, totalTitle: "Total revenue")
			+ section("expenses", "Expenses", column.expenses, totalTitle: "Total expenses")
			+ [StatementLine(id: "netIncome", title: "Net income", amount: column.netProfit.total, style: .grand)]
	}

	/// ST-4: opening equity, the seven reconciling lines and closing equity.
	/// Net income and the capitalized transfers line always show; the other
	/// five only when they aren't zero. Closing equity is the core's, so when
	/// `unexplained` isn't zero the column doesn't add up and the notice says so.
	static func lines(_ equity: OwnersEquity) -> [StatementLine] {
		let optional: [(String, String, ReportLine)] = [
			("startingBalances", "Starting balances", equity.startingBalances),
			("reconciliationAdjustments", "Reconciliation adjustments", equity.reconciliationAdjustments),
			("revaluations", "Off-budget revaluations", equity.revaluations),
			("unpairedTransfers", "Unpaired transfer legs", equity.unpairedTransfers),
			("inTransit", "Transfers in transit", equity.inTransit)
		]
		var lines = [
			StatementLine(id: "opening", title: "Opening equity", amount: equity.opening, style: .balance),
			StatementLine(id: "netIncome", title: "Net income", amount: equity.netIncome.total, style: .line),
			StatementLine(id: "capitalizedTransfers", title: "Off-budget assets acquired (expensed above)", amount: equity.capitalizedTransfers.total, style: .line)
		]
		lines += optional.filter { $0.2.total != 0 }.map { StatementLine(id: $0.0, title: $0.1, amount: $0.2.total, style: .line) }
		lines.append(StatementLine(id: "closing", title: "Closing equity", amount: equity.closing, style: .grand))
		return lines
	}

	/// ST-6 in its sections, with ST-10.7's rule for which lines show: the
	/// review and other lines only when they aren't zero, and Change in budget
	/// cash from operations only with Unreviewed transfers. Signs are the core's.
	static func lines(_ flow: CashFlowColumn) -> [StatementLine] {
		var lines = [
			StatementLine(id: "opening.section", title: "Opening", style: .section),
			StatementLine(id: "opening", title: "Budget accounts balance", amount: flow.opening, style: .line),
			StatementLine(id: "operating", title: "Operating", style: .section),
			StatementLine(id: "netProfit", title: "Net Profit", amount: flow.netProfit.total, style: .line),
			StatementLine(id: "financing", title: "Financing", style: .section),
			StatementLine(id: "debtPaydown", title: "Debt paydown", amount: flow.debtPaydown.total, style: .line),
			StatementLine(id: "saving", title: "Saving and investing", style: .section),
			StatementLine(id: "savings", title: "Savings contributions", amount: flow.savings.total, style: .line),
			StatementLine(id: "netCashFlow", title: "Net cash flow", amount: flow.netCashFlow.total, style: .total)
		]
		if flow.unreviewedTransfers.total != 0 {
			lines += [
				StatementLine(id: "review", title: "Review", style: .section),
				StatementLine(id: "unreviewed", title: "Unreviewed transfers", amount: flow.unreviewedTransfers.total, style: .line),
				StatementLine(id: "operations", title: "Change in budget cash from operations", amount: flow.changeFromOperations, style: .total)
			]
		}
		let other = [
			("inTransit", "Transfers in transit between budget accounts", flow.inTransit),
			("startingBalances", "Starting balances in budget accounts", flow.startingBalances),
			("reconciliationAdjustments", "Reconciliation adjustments in budget accounts", flow.reconciliationAdjustments)
		].filter { $0.2.total != 0 }
		if !other.isEmpty {
			lines.append(StatementLine(id: "other", title: "Other", style: .section))
			lines += other.map { StatementLine(id: $0.0, title: $0.1, amount: $0.2.total, style: .line) }
		}
		lines += [
			StatementLine(id: "closing.section", title: "Closing", style: .section),
			StatementLine(id: "closing", title: "Budget accounts balance", amount: flow.closing, style: .grand)
		]
		return lines
	}
}
