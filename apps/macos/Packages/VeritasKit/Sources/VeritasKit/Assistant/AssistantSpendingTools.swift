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

	static func text(_ report: NetProfitPeriod, period: AssistantPeriod, format: MoneyFormat) -> String {
		func amount(_ value: Int64) -> String { format.amount(value, on: .register) }
		let expenses = report.column.expenses
		var text = [
			"Spending, \(period.title): \(amount(expenses.total))",
			"This is the Expenses line of the Net Profit report. Debt paydown and savings are not included."
		]
		let ungrouped = expenses.categories.filter { $0.groupId == nil }
		guard !report.expenseGroups.isEmpty || !ungrouped.isEmpty else { return text.joined(separator: "\n") }
		text.append("Spending by category group, \(period.title):")
		text += report.expenseGroups.map { "\($0.name): \(amount($0.total))" }
		text += ungrouped.map { "\(ReportWords.category($0.name, labels: $0.labels)): \(amount($0.total))" }
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
		func amount(_ value: Int64) -> String { format.amount(value, on: .register) }
		let income = report.column.income
		var text = [
			"Income, \(period.title): \(amount(income.total))",
			"This is the Income line of the Net Profit report."
		]
		guard !income.categories.isEmpty else { return text.joined(separator: "\n") }
		text.append("Income by category, \(period.title):")
		text += income.categories.map { "\(ReportWords.category($0.name, labels: $0.labels)): \(amount($0.total))" }
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

	/// The Net Profit lines, in the order a category's figures are given.
	static let lines: [NetProfitLine] = [.expenses, .income, .debtPaydown, .savings]

	static func candidates(_ report: NetProfitPeriod, sheet: BudgetMonth) -> [AssistantNames.Candidate] {
		var candidates: [AssistantNames.Candidate] = []
		for group in sheet.groups {
			candidates.append(.init(id: group.id, name: group.name, isGroup: true))
			candidates += group.envelopes.map { .init(id: $0.id, name: $0.name, isGroup: false, group: group.name) }
		}
		let known = Set(candidates.map(\.id))
		let reported = lines.compactMap { NetProfitModel.section($0, in: report.column) }.flatMap(\.categories)
		for category in reported {
			guard let id = category.categoryId, !known.contains(id), !candidates.contains(where: { $0.id == id }) else { continue }
			candidates.append(.init(id: id, name: category.name, isGroup: false, group: category.groupName))
		}
		for group in report.expenseGroups + report.incomeGroups where !candidates.contains(where: { $0.id == group.groupId }) {
			candidates.append(.init(id: group.groupId, name: group.name, isGroup: true))
		}
		return candidates
	}

	static func text(_ wanted: String, report: NetProfitPeriod, sheet: BudgetMonth, period: AssistantPeriod, format: MoneyFormat) -> String {
		let match = AssistantNames.match(wanted, in: candidates(report, sheet: sheet))
		guard case .found(let candidate) = match else { return AssistantNames.text(match, wanted: wanted) ?? AssistantArguments.nameHint }
		return candidate.isGroup
			? group(candidate, report: report, sheet: sheet, period: period, format: format)
			: category(candidate, report: report, sheet: sheet, period: period, format: format)
	}

	private static func category(_ candidate: AssistantNames.Candidate, report: NetProfitPeriod, sheet: BudgetMonth, period: AssistantPeriod, format: MoneyFormat) -> String {
		var found = lines.compactMap { line -> (NetProfitLine, ReportCategory)? in
			guard let category = NetProfitModel.section(line, in: report.column)?.categories.first(where: { $0.categoryId == candidate.id }) else { return nil }
			return (line, category)
		}
		if found.isEmpty {
			let group = sheet.groups.first { $0.envelopes.contains { $0.id == candidate.id } }
			let hidden = group?.hidden == true || group?.envelopes.first { $0.id == candidate.id }?.hidden == true
			let labels = ReportLabels(hidden: hidden, archived: false, deleted: false, closedGoal: false, needsReview: false)
			found = [(group?.isIncome == true ? .income : .expenses, ReportCategory(categoryId: candidate.id, name: candidate.name, groupId: group?.id, groupName: group?.name, labels: labels, total: 0, count: 0))]
		}
		return found.flatMap { line, category in
			figure(line, name: ReportWords.category(category.name, labels: category.labels), total: category.total, count: category.count, period: period, format: format)
		}
		.joined(separator: "\n")
	}

	private static func group(_ candidate: AssistantNames.Candidate, report: NetProfitPeriod, sheet: BudgetMonth, period: AssistantPeriod, format: MoneyFormat) -> String {
		let isIncome = report.incomeGroups.contains { $0.groupId == candidate.id } || sheet.groups.first { $0.id == candidate.id }?.isIncome == true
		let line: NetProfitLine = isIncome ? .income : .expenses
		let total = (isIncome ? report.incomeGroups : report.expenseGroups).first { $0.groupId == candidate.id }
		var text = figure(line, name: candidate.name, total: total?.total ?? 0, count: total?.count ?? 0, period: period, format: format)
		text[1] = "\(candidate.name) is a category group. \(text[1])"
		let categories = NetProfitModel.section(line, in: report.column)?.categories.filter { $0.groupId == candidate.id } ?? []
		if !categories.isEmpty {
			text.append("Its categories, \(period.title):")
			text += categories.map { "\(ReportWords.category($0.name, labels: $0.labels)): \(format.amount($0.total, on: .register))" }
		}
		return text.joined(separator: "\n")
	}

	/// The answer line and where it comes from (AI-3.3, AI-3.4, AI-5.2).
	private static func figure(_ line: NetProfitLine, name: String, total: Int64, count: UInt64, period: AssistantPeriod, format: MoneyFormat) -> [String] {
		let amount = format.amount(total, on: .register)
		let rows = count == 1 ? "From 1 transaction" : "From \(count) transactions"
		return switch line {
		case .income: ["Income from \(name), \(period.title): \(amount)", "\(rows), on the Income line of the Net Profit report."]
		case .debtPaydown: ["Debt paydown for \(name), \(period.title): \(amount)", "\(rows), on the Debt paydown line of the Net Profit report. It is not spending."]
		case .savings: ["Savings for \(name), \(period.title): \(amount)", "\(rows), on the Savings line of the Net Profit report. It is not spending."]
		default: ["Spending on \(name), \(period.title): \(amount)", "\(rows), on the Expenses line of the Net Profit report."]
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

	/// AI-7: Budget screen figures say "activity" and "available", never "spent".
	static func text(_ wanted: String, sheet: BudgetMonth, format: MoneyFormat) -> String {
		func amount(_ value: Int64) -> String { format.amount(value, on: .budget) }
		let month = ReportPeriod.monthYear(sheet.month)
		var text: [String]
		if wanted.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
			text = [
				"Budgeted, \(month): \(amount(sheet.budgeted))",
				"Budget screen figures for \(month): To Budget \(format.currency(sheet.toBudget, on: .budget))."
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
			let name = figures.3 ? "\(candidate.name) (hidden)" : candidate.name
			if group?.isIncome == true {
				text = ["\(name) is income, so nothing is budgeted for it. Budget screen figures for \(month): received \(amount(figures.1))."]
			} else {
				text = [
					"Budgeted for \(name), \(month): \(amount(figures.0))",
					"\(candidate.isGroup ? "\(candidate.name) is a category group. " : "")Budget screen figures for \(month): activity \(amount(-figures.1)), available \(amount(figures.2))."
				]
			}
		}
		if let note = sheet.mixedScaleNote { text.append(note) }
		return text.joined(separator: "\n")
	}
}
