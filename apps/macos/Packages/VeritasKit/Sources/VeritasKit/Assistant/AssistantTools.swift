import Foundation
import VeritasCore

/// The tools the on-device model can call. Each reads through
/// `ReadOnlyBudgetQueries` and returns lines of text whose amounts are
/// already formatted the way the matching screen formats them, so the model
/// never does money arithmetic.
public enum AssistantTools {
	public static func all(_ context: AssistantContext) -> [any AssistantTool] {
		[
			SpendingTool(context: context),
			CategorySpendingTool(context: context),
			IncomeTool(context: context),
			BudgetTool(context: context),
			NetProfitTool(context: context),
			BudgetMonthTool(context: context),
			AccountsTool(context: context),
			BalanceSheetTool(context: context),
			CashFlowTool(context: context),
			GoalsTool(context: context),
			TaxSummaryTool(context: context),
			SavedReportTool(context: context)
		]
	}

	static let yearHint = "Ask again with a calendar year such as 2026."
	/// The years a tool reads. The model's arguments come from another image,
	/// so every tool checks them again before they reach the core.
	static let years = 1900...2200
}

/// A read the model can ask for. The AI plug-in describes it to the model and
/// hands back the arguments the model filled in.
public protocol AssistantTool: Sendable {
	var name: String { get }
	var description: String { get }
	var arguments: [AssistantToolArgument] { get }
	func run(_ input: AssistantToolInput) async throws -> String
}

public struct AssistantToolArgument: Hashable, Sendable {
	public enum Kind: Hashable, Sendable {
		case text
		case integer(ClosedRange<Int>)
	}

	public let name: String
	public let description: String
	public let kind: Kind
}

/// The arguments the model filled in. A missing one reads as empty text or 0,
/// which each tool answers with a hint rather than a read.
public struct AssistantToolInput: Sendable {
	public enum Value: Hashable, Sendable {
		case text(String)
		case integer(Int)
	}

	public let values: [String: Value]

	public init(_ values: [String: Value]) {
		self.values = values
	}

	func text(_ name: String) -> String {
		if case .text(let text) = values[name] { text } else { "" }
	}

	func integer(_ name: String) -> Int {
		if case .integer(let number) = values[name] { number } else { 0 }
	}
}

enum AssistantDates {
	/// `2026-10`, or the month of `2026-10-14`.
	static func month(_ text: String) -> Month? {
		let parts = text.trimmingCharacters(in: .whitespaces).split(separator: "-")
		guard parts.count >= 2, parts[0].count == 4, let year = Int32(parts[0]), AssistantTools.years.contains(Int(year)),
			let month = UInt32(parts[1]), (1...12).contains(month) else { return nil }
		return Month(year: year, month: month)
	}

	/// A real civil date, `YYYY-MM-DD`.
	static func day(_ text: String) -> String? {
		let parts = text.trimmingCharacters(in: .whitespaces).split(separator: "-")
		guard parts.count == 3, let month = month(text), let day = Int(parts[2]), (1...month.dayCount).contains(day) else { return nil }
		return String(format: "%04d-%02d-%02d", month.year, month.month, day)
	}
}

struct NetProfitTool: AssistantTool {
	let context: AssistantContext
	let name = "netProfit"
	let description = "The Net Profit report for a period: Income, Expenses, Net Profit, debt paydown, savings and net cash flow. Use it only when the question names Net Profit, net cash flow, debt paydown or savings."

	struct Arguments {
		var period: String
	}

	let arguments = [AssistantArguments.period]

	func run(_ input: AssistantToolInput) async throws -> String {
		try await call(arguments: Arguments(period: input.text("period")))
	}

	func call(arguments: Arguments) async throws -> String {
		guard let period = AssistantPeriod.resolve(arguments.period, today: context.today) else { return AssistantPeriod.hint }
		let report = try await context.queries.netProfitPeriod(first: period.first, last: period.last)
		return Self.text(report.column, period: period, format: context.format)
	}

	static let lines: [NetProfitLine] = [.income, .expenses, .debtPaydown, .savings, .netCashFlow]

	static func text(_ column: NetProfitColumn, period: AssistantPeriod, format: MoneyFormat) -> String {
		func amount(_ line: NetProfitLine) -> String {
			format.amount(NetProfitModel.value(line, in: column).total, on: .register)
		}
		var shown = lines
		if column.unreviewed.total != 0 { shown.append(.unreviewed) }
		return (["Net Profit, \(period.title): \(amount(.netProfit))", "Net Profit report, \(period.title):"]
			+ shown.map { "\(ReportWords.line($0)): \(amount($0))" }).joined(separator: "\n")
	}
}

struct BudgetMonthTool: AssistantTool {
	let context: AssistantContext
	let name = "budgetMonth"
	let description = "Every envelope on the Budget screen for one month: To Budget, and each envelope's budgeted, activity and available amounts. For one category or group, use budget."

	struct Arguments {
		var period: String
	}

	let arguments = [AssistantArguments.period]

	func run(_ input: AssistantToolInput) async throws -> String {
		try await call(arguments: Arguments(period: input.text("period")))
	}

	func call(arguments: Arguments) async throws -> String {
		guard let period = AssistantPeriod.resolve(arguments.period, today: context.today) else { return AssistantPeriod.hint }
		guard period.firstMonth == period.lastMonth else { return BudgetTool.oneMonth }
		let sheet = try await context.queries.budgetMonth(period.firstMonth, today: context.today)
		return Self.text(sheet, format: context.format)
	}

	/// The Budget screen with hidden envelopes left out, as it opens. AI-7:
	/// its figures say "activity", never "spent".
	static func text(_ sheet: BudgetMonth, format: MoneyFormat) -> String {
		func amount(_ value: Int64) -> String { format.amount(value, on: .budget) }
		var text = sheet.mixedScaleNote.map { [$0] } ?? []
		text += [
			"Budget screen figures for \(ReportPeriod.monthYear(sheet.month))",
			"To Budget: \(format.currency(sheet.toBudget, on: .budget))"
		]
		if sheet.forNextMonth != 0 { text.append("Held for next month: \(amount(sheet.forNextMonth))") }
		for group in sheet.groups where !group.hidden {
			if group.isIncome {
				text.append("Income group \(group.name): received \(amount(group.spent))")
				text += group.envelopes.filter { !$0.hidden }.map { "\($0.name): received \(amount($0.spent))" }
			} else {
				text.append("Group \(group.name): budgeted \(amount(group.budgeted)), activity \(amount(-group.spent)), available \(amount(group.available))")
				text += group.envelopes.filter { !$0.hidden }.map {
					"\($0.name): budgeted \(amount($0.budgeted)), activity \(amount(-$0.spent)), available \(amount($0.available))"
				}
			}
		}
		return text.joined(separator: "\n")
	}
}

struct AccountsTool: AssistantTool {
	let context: AssistantContext
	let name = "accounts"
	let description = "Every account with its current balance, group, and whether it is off budget or closed."

	struct Arguments {}

	let arguments: [AssistantToolArgument] = []

	func run(_ input: AssistantToolInput) async throws -> String {
		try await call(arguments: Arguments())
	}

	func call(arguments: Arguments) async throws -> String {
		Self.text(try await context.queries.accounts(), format: context.format)
	}

	static func text(_ list: AccountList, format: MoneyFormat) -> String {
		let groups = Dictionary(list.groups.map { ($0.id, $0.name) }, uniquingKeysWith: { first, _ in first })
		var text = ["Accounts and current balances"]
		text += list.accounts.map { account in
			var traits = account.groupId.flatMap { groups[$0] }.map { ["in \($0)"] } ?? []
			if account.offBudget { traits.append("off budget") }
			if account.closed { traits.append("closed") }
			let suffix = traits.isEmpty ? "" : " (\(traits.joined(separator: ", ")))"
			return "\(account.name): \(format.amount(account.total, on: .register))\(suffix)"
		}
		return text.joined(separator: "\n")
	}
}

/// Statement lines in the Statements screen's order, with plain register
/// amounts: parentheses are for the printed statement only (ST-10.6), so a
/// liability reads as the positive figure it is.
enum StatementText {
	static func lines(_ lines: [StatementLine], format: MoneyFormat) -> [String] {
		lines.map { line in
			line.amount.map { "\(line.title): \(format.amount($0, on: .register))" } ?? line.title
		}
	}
}

struct BalanceSheetTool: AssistantTool {
	let context: AssistantContext
	let name = "balanceSheet"
	let description = "Assets, liabilities and net worth at the end of a day."

	struct Arguments {
		var date: String
	}

	let arguments: [AssistantToolArgument] = [
		AssistantToolArgument(name: "date", description: "Date, YYYY-MM-DD. Today's date for now.", kind: .text)
	]

	func run(_ input: AssistantToolInput) async throws -> String {
		try await call(arguments: Arguments(date: input.text("date")))
	}

	func call(arguments: Arguments) async throws -> String {
		guard let day = AssistantDates.day(arguments.date) else { return "Ask again with a date written as YYYY-MM-DD." }
		let asOf = min(day, context.today)
		let sheet = try await context.queries.balanceSheet(asOf: asOf)
		return (["Balance sheet as of \(StatementsModel.date(asOf, style: .wide))"] + StatementText.lines(StatementsModel.lines(sheet), format: context.format)).joined(separator: "\n")
	}
}

struct CashFlowTool: AssistantTool {
	let context: AssistantContext
	let name = "cashFlow"
	let description = "The cash flow statement for a period: opening and closing budget account balances, Net Profit, debt paydown and savings."

	struct Arguments {
		var period: String
	}

	let arguments = [AssistantArguments.period]

	func run(_ input: AssistantToolInput) async throws -> String {
		try await call(arguments: Arguments(period: input.text("period")))
	}

	func call(arguments: Arguments) async throws -> String {
		guard let period = AssistantPeriod.resolve(arguments.period, today: context.today) else { return AssistantPeriod.hint }
		let flow = try await context.queries.cashFlowStatement(first: period.first, last: period.last)
		return (["Cash flow statement, \(period.title)"] + StatementText.lines(StatementsModel.lines(flow), format: context.format)).joined(separator: "\n")
	}
}

struct GoalsTool: AssistantTool {
	let context: AssistantContext
	let name = "goals"
	let description = "Every savings goal: progress toward its target, target date, pace, and closed or reached goals."

	struct Arguments {}

	let arguments: [AssistantToolArgument] = []

	func run(_ input: AssistantToolInput) async throws -> String {
		try await call(arguments: Arguments())
	}

	func call(arguments: Arguments) async throws -> String {
		Self.text(try await context.queries.goals(today: context.today), format: context.format)
	}

	/// In a 0-place budget (CLS-12.14) the core's note comes first, a goal's mixed
	/// figure is labeled only "available", and spending to date has its own line.
	static func text(_ list: GoalList, format: MoneyFormat) -> String {
		guard !list.goals.isEmpty else { return "This budget has no goals." }
		var text = list.mixedScaleNote.map { [$0] } ?? []
		text.append("Goals")
		for goal in list.goals {
			text += lines(goal, format: format, zeroPlaces: list.zeroPlaces)
		}
		return text.joined(separator: "\n")
	}

	/// The goal card's figures: whole units on the budget scale, spending on the register's.
	static func lines(_ goal: GoalSummary, format: MoneyFormat, zeroPlaces: Bool) -> [String] {
		func whole(_ value: Int64) -> String { format.wholeCurrency(value, on: .budget) }
		func monthly(_ value: Int64) -> String { format.currency(value, on: .budget) }
		var parts: [String]
		if zeroPlaces {
			parts = goal.state == .reachedSpent
				? ["target \(whole(goal.target))", "available \(format.currency(goal.available, on: .budget))"]
				: ["available \(whole(goal.progressAmount))", "target \(whole(goal.target))"]
		} else {
			parts = ["\(whole(goal.progressAmount)) of \(whole(goal.target)) (\(goal.progressPercent)%)"]
		}
		if let date = goal.targetDate { parts.append("target date \(StatementsModel.date(date, style: .wide))") }
		switch goal.state {
		case .active:
			switch goal.pace {
			case .onTrack(let eta): parts.append("on track to reach it in \(ReportPeriod.monthYear(eta))")
			case .behind(let needed): parts.append("behind, needs \(monthly(needed)) a month")
			case .noHistory(let needed): parts.append("needs \(monthly(needed)) a month")
			case .notAdding(let needed): parts.append("nothing is being added, needs \(monthly(needed)) a month")
			case .spendingRecorded: parts.append("no ETA, spending recorded")
			case nil: if let suggested = goal.suggestedMonthly { parts.append("budget \(monthly(suggested)) a month") }
			}
		case .reached:
			parts.append(goal.reachedMonth.map { "reached in \(ReportPeriod.monthYear($0))" } ?? "reached")
		case .reachedSpent:
			parts.append("reached, then spent")
			if !zeroPlaces { parts.append("available \(format.currency(goal.available, on: .budget))") }
		case .closed:
			parts.append(goal.closedMonth.map { "closed in \(ReportPeriod.monthYear($0))" } ?? "closed")
		}
		var lines = ["\(goal.name): \(parts.joined(separator: ", "))"]
		if goal.state == .reachedSpent || goal.state == .closed {
			lines.append("\(goal.name) spending to date: \(format.currency(goal.spent, on: .register))")
		}
		return lines
	}
}

struct TaxSummaryTool: AssistantTool {
	let context: AssistantContext
	let name = "taxSummary"
	let description = "One calendar year's tax buckets, such as W-2 income, charitable giving and medical, each with its total."

	struct Arguments {
		var year: Int
	}

	let arguments: [AssistantToolArgument] = [
		AssistantToolArgument(name: "year", description: "Calendar year, such as 2026", kind: .integer(AssistantTools.years))
	]

	func run(_ input: AssistantToolInput) async throws -> String {
		try await call(arguments: Arguments(year: input.integer("year")))
	}

	func call(arguments: Arguments) async throws -> String {
		guard AssistantTools.years.contains(arguments.year), let year = Int32(exactly: arguments.year) else { return AssistantTools.yearHint }
		return Self.text(try await context.queries.taxSummary(year: year), format: context.format)
	}

	static func text(_ summary: TaxSummary, format: MoneyFormat) -> String {
		var text = ["Tax summary for \(summary.year)"]
		text += summary.buckets.map { bucket in
			let line = "\(TaxBuckets.name(bucket.bucket)): \(format.amount(bucket.total, on: .register)) from \(bucket.count) \(bucket.count == 1 ? "transaction" : "transactions")"
			return TaxBuckets.note(for: bucket).map { "\(line). \($0)" } ?? line
		}
		if summary.buckets.isEmpty { text.append("No transactions are in a tax bucket this year.") }
		if summary.conflicts > 0 { text.append("Transactions whose tags name more than one bucket: \(summary.conflicts)") }
		return text.joined(separator: "\n")
	}
}

struct SavedReportTool: AssistantTool {
	let context: AssistantContext
	let name = "savedReport"
	let description = "A custom report saved in the budget, by name. With a name that matches none, lists the saved reports."

	struct Arguments {
		var name: String
	}

	let arguments: [AssistantToolArgument] = [
		AssistantToolArgument(name: "name", description: "The saved report's name", kind: .text)
	]

	func run(_ input: AssistantToolInput) async throws -> String {
		try await call(arguments: Arguments(name: input.text("name")))
	}

	func call(arguments: Arguments) async throws -> String {
		let items = try await context.queries.customReports()
		let wanted = arguments.name.trimmingCharacters(in: .whitespaces)
		guard let item = items.first(where: { $0.name.compare(wanted, options: [.caseInsensitive, .diacriticInsensitive]) == .orderedSame }) else {
			guard !items.isEmpty else { return "This budget has no saved reports." }
			return "No saved report is called \(wanted). Saved reports: \(items.map(\.name).joined(separator: ", "))."
		}
		return Self.text(name: item.name, try await context.queries.customReport(item.id, today: context.today), format: context.format)
	}

	/// The report's table as the Custom reports screen shows it.
	static func text(name: String, _ view: CustomReportView, format: MoneyFormat) -> String {
		guard let data = view.data else { return "\(name) can't be shown. \(view.problem ?? "")" }
		let settings = view.settings
		let range = !settings.dateStatic ? (settings.dateRange ?? "\(settings.startDate) to \(settings.endDate)") : "\(settings.startDate) to \(settings.endDate)"
		var text = ["Saved report \(name), \(settings.balanceType.title) by \(settings.groupBy.title.lowercased()), \(range)"]
		text += data.groups.map { "\($0.name): \(format.amount($0.amount, on: .register))" }
		text.append("Total: \(format.amount(data.total, on: .register))")
		return text.joined(separator: "\n")
	}
}
