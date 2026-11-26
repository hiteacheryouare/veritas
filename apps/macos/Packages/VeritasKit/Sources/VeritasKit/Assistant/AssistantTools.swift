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
			SpendingByCategoryTool(context: context),
			CategorySpendingTool(context: context),
			IncomeTool(context: context),
			BudgetTool(context: context),
			BudgetHabitsTool(context: context),
			WhatMoNeeAnswersTool(),
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

struct NetProfitTool: AssistantTool {
	let context: AssistantContext
	let name = "netProfit"
	let description = "The Net Profit report for a period, with debt paydown, savings and net cash flow."

	struct Arguments {
		var period: String
	}

	let arguments = [AssistantArguments.period]

	func run(_ input: AssistantToolInput) async throws -> String {
		try await call(arguments: Arguments(period: input.text("period")))
	}

	func call(arguments: Arguments) async throws -> String {
		let resolved = try resolvePeriod(text: arguments.period, today: context.today)
		guard let period = resolved.period else { return resolved.hint }
		let report = try await context.queries.netProfitPeriod(first: period.first, last: period.last)
		return period.marked(Self.text(report.column, period: period, format: context.format))
	}

	static let lines: [NetProfitLine] = [.netProfit, .income, .expenses, .debtPaydown, .savings, .netCashFlow, .unreviewed]
	static let negativeDebtPaydown = "Negative means more was borrowed than paid down."
	static let negativeSavings = "Negative means more was withdrawn than saved."

	/// AI-9: every line carries the range, and debt paydown and savings say they aren't spending.
	static func text(_ column: NetProfitColumn, period: AssistantPeriod, format: MoneyFormat) -> String {
		lines.compactMap { line -> String? in
			let total = NetProfitModel.value(line, in: column).total
			let figure = "\(ReportWords.line(line)), \(period.label): \(format.currency(total, on: .register))"
			switch line {
			case .debtPaydown: return "\(figure). It is not spending.\(total < 0 ? " \(negativeDebtPaydown)" : "")"
			case .savings: return "\(figure). It is not spending.\(total < 0 ? " \(negativeSavings)" : "")"
			case .unreviewed: return total == 0 ? nil : "\(figure). Not in Net cash flow."
			default: return figure
			}
		}
		.joined(separator: "\n")
	}
}

struct BudgetMonthTool: AssistantTool {
	let context: AssistantContext
	let name = "budgetMonth"
	let description = "Every envelope on the Budget screen for one month."

	struct Arguments {
		var period: String
	}

	let arguments = [AssistantArguments.period]

	func run(_ input: AssistantToolInput) async throws -> String {
		try await call(arguments: Arguments(period: input.text("period")))
	}

	func call(arguments: Arguments) async throws -> String {
		let resolved = try resolvePeriod(text: arguments.period, today: context.today)
		guard let period = resolved.period else { return resolved.hint }
		guard period.firstMonth == period.lastMonth else { return BudgetTool.oneMonth }
		let sheet = try await context.queries.budgetMonth(period.firstMonth, today: context.today)
		return Self.text(sheet, format: context.format)
	}

	/// The Budget screen with hidden envelopes left out, as it opens. AI-7:
	/// its figures say "activity", never "spent".
	static func text(_ sheet: BudgetMonth, format: MoneyFormat) -> String {
		func amount(_ value: Int64) -> String { format.currency(value, on: .budget) }
		var text = sheet.mixedScaleNote.map { [$0] } ?? []
		text += [
			"Budget screen figures for \(ReportPeriod.monthYear(sheet.month))",
			"To Budget: \(amount(sheet.toBudget))"
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
		let resolved = try resolveBalanceSheetDate(text: arguments.date, today: context.today)
		guard let day = resolved.period else { return resolved.hint }
		let sheet = try await context.queries.balanceSheet(asOf: day.last)
		return (["Balance sheet as of \(day.label)"] + StatementText.lines(StatementsModel.lines(sheet), format: context.format)).joined(separator: "\n")
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
		let resolved = try resolvePeriod(text: arguments.period, today: context.today)
		guard let period = resolved.period else { return resolved.hint }
		let flow = try await context.queries.cashFlowStatement(first: period.first, last: period.last)
		return period.marked((["Cash flow statement, \(period.label)"] + StatementText.lines(StatementsModel.lines(flow), format: context.format)).joined(separator: "\n"))
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
		var year: String
	}

	let arguments: [AssistantToolArgument] = [
		AssistantToolArgument(name: "year", description: "The calendar year, in the user's own words, such as \"2025\" or \"last year\".", kind: .text)
	]

	func run(_ input: AssistantToolInput) async throws -> String {
		try await call(arguments: Arguments(year: input.text("year")))
	}

	/// AI-6.15: one whole calendar year that has started, read by the core.
	func call(arguments: Arguments) async throws -> String {
		switch try resolveTaxYear(text: arguments.year, today: context.today) {
		case .year(let year): Self.text(try await context.queries.taxSummary(year: year), format: context.format)
		case .refused(let hint): hint
		}
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
