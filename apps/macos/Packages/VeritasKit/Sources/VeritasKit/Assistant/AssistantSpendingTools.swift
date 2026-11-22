import Foundation
import VeritasCore

/// The tools for "how much did I spend, earn or budget" (docs/finance/assistant.md).
/// Each one's first line is the answer, labelled with the exact dates it
/// covers. The core computes every figure; these only find and format them.
enum AssistantArguments {
	static let period = AssistantToolArgument(
		name: "period",
		description: "The time the question asks about, in the user's own words, such as \"last month\", \"so far this month\", \"this year\" or \"October 2026\". Empty when the question names no time.",
		kind: .text
	)

	static let name = AssistantToolArgument(
		name: "name",
		description: "The category or category group the question names, as the user wrote it.",
		kind: .text
	)

	static let nameHint = "Ask again with the name of a category or category group."
}

/// ST-10.3 labels for the assistant's lines (AI-3.5).
enum AssistantLabels {
	static let unnamedGroup = "Unnamed group"

	static func group(_ group: ReportGroup) -> String {
		group.name.isEmpty ? unnamedGroup : ReportWords.category(group.name, labels: group.labels)
	}

	static func category(_ category: ReportCategory) -> String {
		ReportWords.category(category.name, labels: category.labels)
	}

	static func plain(hidden: Bool) -> ReportLabels {
		ReportLabels(hidden: hidden, archived: false, deleted: false, closedGoal: false, needsReview: false)
	}
}

struct SpendingTool: AssistantTool {
	let context: AssistantContext
	let name = "spending"
	let description = "Total spending for a period: what was spent, all categories together. Use it for \"how much did I spend\" without a category."

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
		return Self.text(report, period: period, format: context.format)
	}

	/// AI-2.2 and AI-2.5: the group lines, then every expense line in no
	/// group, add up to the first line to the cent.
	static func text(_ report: NetProfitPeriod, period: AssistantPeriod, format: MoneyFormat) -> String {
		func amount(_ value: Int64) -> String { format.currency(value, on: .register) }
		let expenses = report.column.expenses
		var text = [
			"Spending, \(period.title): \(amount(expenses.total))",
			"This is the Expenses line of the Net Profit report. Debt paydown and savings are not included."
		]
		let ungrouped = expenses.categories.filter { $0.groupId == nil }
		guard !report.expenseGroups.isEmpty || !ungrouped.isEmpty else { return text.joined(separator: "\n") }
		text.append("Spending by category group, \(period.title):")
		text += report.expenseGroups.map { "\(AssistantLabels.group($0)): \(amount($0.total))" }
		text += ungrouped.map { "\(AssistantLabels.category($0)): \(amount($0.total))" }
		return text.joined(separator: "\n")
	}
}

struct IncomeTool: AssistantTool {
	let context: AssistantContext
	let name = "income"
	let description = "Total income for a period: what was earned, with each income category. Use it for \"how much did I earn\" or \"what came in\"."

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
		return Self.text(report, period: period, format: context.format)
	}

	static func text(_ report: NetProfitPeriod, period: AssistantPeriod, format: MoneyFormat) -> String {
		func amount(_ value: Int64) -> String { format.currency(value, on: .register) }
		let income = report.column.income
		var text = [
			"Income, \(period.title): \(amount(income.total))",
			"This is the Income line of the Net Profit report."
		]
		guard !income.categories.isEmpty else { return text.joined(separator: "\n") }
		text.append("Income by category, \(period.title):")
		text += income.categories.map { "\(AssistantLabels.category($0)): \(amount($0.total))" }
		return text.joined(separator: "\n")
	}
}

struct CategorySpendingTool: AssistantTool {
	let context: AssistantContext
	let name = "categorySpending"
	let description = "What was spent on one category or category group for a period, or earned from an income category. Use it whenever the question names a category or group."

	struct Arguments {
		var name: String
		var period: String
	}

	let arguments = [AssistantArguments.name, AssistantArguments.period]

	func run(_ input: AssistantToolInput) async throws -> String {
		try await call(arguments: Arguments(name: input.text("name"), period: input.text("period")))
	}

	func call(arguments: Arguments) async throws -> String {
		guard let period = AssistantPeriod.resolve(arguments.period, today: context.today) else { return AssistantPeriod.hint }
		guard !arguments.name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return AssistantArguments.nameHint }
		let report = try await context.queries.netProfitPeriod(first: period.first, last: period.last)
		let sheet = try await context.queries.budgetMonth(Month(date: context.today) ?? period.lastMonth, today: context.today)
		return Self.text(arguments.name, report: report, sheet: sheet, period: period, format: context.format)
	}

	/// The Net Profit lines, in the order a category's figures are given (AI-3.3).
	static let lines: [NetProfitLine] = [.expenses, .income, .debtPaydown, .savings]

	static func groups(_ line: NetProfitLine, in report: NetProfitPeriod) -> [ReportGroup] {
		switch line {
		case .income: report.incomeGroups
		case .debtPaydown: report.debtPaydownGroups
		case .savings: report.savingsGroups
		default: report.expenseGroups
		}
	}

	static func candidates(_ report: NetProfitPeriod, sheet: BudgetMonth) -> [AssistantNames.Candidate] {
		var candidates: [AssistantNames.Candidate] = []
		for group in sheet.groups {
			candidates.append(.init(id: group.id, name: group.name, isGroup: true))
			candidates += group.envelopes.map { .init(id: $0.id, name: $0.name, isGroup: false, group: group.name) }
		}
		for category in lines.compactMap({ NetProfitModel.section($0, in: report.column) }).flatMap(\.categories) {
			guard let id = category.categoryId, !candidates.contains(where: { $0.id == id }) else { continue }
			candidates.append(.init(id: id, name: category.name, isGroup: false, group: category.groupName))
		}
		for group in lines.flatMap({ groups($0, in: report) }) where !candidates.contains(where: { $0.id == group.groupId }) {
			candidates.append(.init(id: group.groupId, name: group.name, isGroup: true))
		}
		return candidates
	}

	static func text(_ wanted: String, report: NetProfitPeriod, sheet: BudgetMonth, period: AssistantPeriod, format: MoneyFormat) -> String {
		let match = AssistantNames.match(wanted, in: candidates(report, sheet: sheet))
		guard case .found(let candidate) = match else { return AssistantNames.text(match, wanted: wanted) ?? AssistantArguments.nameHint }
		let found: [Figure] = lines.compactMap { line in
			if candidate.isGroup {
				return groups(line, in: report).first { $0.groupId == candidate.id }
					.map { Figure(line: line, label: AssistantLabels.group($0), total: $0.total, count: $0.count) }
			}
			return NetProfitModel.section(line, in: report.column)?.categories.first { $0.categoryId == candidate.id }
				.map { Figure(line: line, label: AssistantLabels.category($0), total: $0.total, count: $0.count) }
		}
		let figures = found.isEmpty ? [nothing(candidate, sheet: sheet)] : found
		return figures.enumerated().flatMap { index, figure in figure.lines(first: index == 0, period: period, format: format) }
			.joined(separator: "\n")
	}

	/// AI-3.4: no rows of any kind. The only place the category's own kind is read.
	private static func nothing(_ candidate: AssistantNames.Candidate, sheet: BudgetMonth) -> Figure {
		let group = sheet.groups.first { $0.id == candidate.id || $0.envelopes.contains { $0.id == candidate.id } }
		let envelope = group?.envelopes.first { $0.id == candidate.id }
		let hidden = group?.hidden == true || envelope?.hidden == true
		let label = candidate.name.isEmpty && candidate.isGroup ? AssistantLabels.unnamedGroup : ReportWords.category(candidate.name, labels: AssistantLabels.plain(hidden: hidden))
		return Figure(line: group?.isIncome == true ? .income : .expenses, label: label, total: 0, count: 0)
	}

	struct Figure {
		let line: NetProfitLine
		let label: String
		let total: Int64
		let count: UInt64

		/// AI-3.3 and AI-5.3.
		func lines(first: Bool, period: AssistantPeriod, format: MoneyFormat) -> [String] {
			let amount = format.currency(total, on: .register)
			let rows = count == 1 ? "From 1 transaction" : "From \(count) transactions"
			switch line {
			case .income:
				let answer = "Income from \(label), \(period.title): \(amount)"
				return first ? [answer, "\(rows), on the Income line of the Net Profit report."] : [answer]
			case .debtPaydown:
				return ["Debt paydown for \(label), \(period.title): \(amount). It is not spending.\(total < 0 ? " \(NetProfitTool.negativeDebtPaydown)" : "")"]
			case .savings:
				return ["Savings for \(label), \(period.title): \(amount). It is not spending.\(total < 0 ? " \(NetProfitTool.negativeSavings)" : "")"]
			default:
				let answer = "Spending on \(label), \(period.title): \(amount)"
				return first ? [answer, "\(rows), on the Expenses line of the Net Profit report."] : [answer]
			}
		}
	}
}

struct BudgetTool: AssistantTool {
	let context: AssistantContext
	let name = "budget"
	let description = "What was budgeted for one month, for a category or category group, or in total when no name is given, with the Budget screen's activity and available."

	struct Arguments {
		var name: String
		var period: String
	}

	let arguments = [
		AssistantToolArgument(name: "name", description: "The category or category group the question names, as the user wrote it. Empty for the whole budget.", kind: .text),
		AssistantArguments.period
	]

	static let oneMonth = "Ask about one month at a time, such as this month or October 2026."

	func run(_ input: AssistantToolInput) async throws -> String {
		try await call(arguments: Arguments(name: input.text("name"), period: input.text("period")))
	}

	func call(arguments: Arguments) async throws -> String {
		guard let period = AssistantPeriod.resolve(arguments.period, today: context.today) else { return AssistantPeriod.hint }
		guard period.firstMonth == period.lastMonth else { return Self.oneMonth }
		let sheet = try await context.queries.budgetMonth(period.firstMonth, today: context.today)
		return Self.text(arguments.name, sheet: sheet, format: context.format)
	}

	/// AI-7: activity is the Budget screen's Spent column as it prints it
	/// (`-spent`), and the word is never "spent". A 0-place budget adds the
	/// core's CLS-12.10 line after the answer.
	static func text(_ wanted: String, sheet: BudgetMonth, format: MoneyFormat) -> String {
		func amount(_ value: Int64) -> String { format.currency(value, on: .budget) }
		let month = ReportPeriod.monthYear(sheet.month)
		var text: [String]
		if wanted.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
			text = [
				"Budgeted, \(month): \(amount(sheet.budgeted))",
				"Budget screen figures for \(month): To Budget \(amount(sheet.toBudget))."
			]
		} else {
			let candidates = sheet.groups.flatMap { group in
				[AssistantNames.Candidate(id: group.id, name: group.name, isGroup: true)]
					+ group.envelopes.map { AssistantNames.Candidate(id: $0.id, name: $0.name, isGroup: false, group: group.name) }
			}
			let match = AssistantNames.match(wanted, in: candidates)
			guard case .found(let candidate) = match else { return AssistantNames.text(match, wanted: wanted) ?? AssistantArguments.nameHint }
			let group = sheet.groups.first { $0.id == candidate.id || $0.envelopes.contains { $0.id == candidate.id } }
			let envelope = group?.envelopes.first { $0.id == candidate.id }
			let figures = envelope.map { ($0.budgeted, $0.spent, $0.available, $0.hidden) }
				?? group.map { ($0.budgeted, $0.spent, $0.available, $0.hidden) } ?? (0, 0, 0, false)
			let name = ReportWords.category(candidate.name, labels: AssistantLabels.plain(hidden: figures.3))
			if group?.isIncome == true {
				text = ["\(name) is income, so nothing is budgeted for it. Budget screen figures for \(month): received \(amount(figures.1))."]
			} else {
				text = [
					"Budgeted for \(name), \(month): \(amount(figures.0))",
					"Budget screen figures for \(month): activity \(amount(-figures.1)), available \(amount(figures.2))."
				]
			}
		}
		if let note = sheet.mixedScaleNote { text.append(note) }
		return text.joined(separator: "\n")
	}
}
