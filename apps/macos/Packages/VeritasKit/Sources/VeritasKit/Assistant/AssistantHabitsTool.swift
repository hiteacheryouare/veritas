import Foundation
import VeritasCore

/// docs/finance/assistant.md AI-10: the categories that ran over their
/// monthly budget in most of the last 3 complete months, from the Budget
/// screen's own figures, and one figure that could be freed.
struct BudgetHabitsTool: AssistantTool {
	let context: AssistantContext
	let name = "budgetHabits"
	let description = "Categories over their monthly budget in most of the last 3 months."

	struct Arguments {}

	let arguments: [AssistantToolArgument] = []

	func run(_ input: AssistantToolInput) async throws -> String {
		try await call(arguments: Arguments())
	}

	/// AI-10.1: each window month is compared with `firstMonth` itself, and a
	/// sheet the core clamped to another month is never used.
	func call(arguments: Arguments) async throws -> String {
		guard let thisMonth = Month(date: context.today) else { return AssistantPeriod.hint }
		let window = [3, 2, 1].map { thisMonth.adding(months: -$0) }
		let latest = try await context.queries.budgetMonth(window[2], today: context.today)
		var sheets: [BudgetMonth] = []
		for month in window where month >= latest.firstMonth {
			let sheet = month == window[2] ? latest : try await context.queries.budgetMonth(month, today: context.today)
			if sheet.month == month { sheets.append(sheet) }
		}
		let goals = sheets.isEmpty ? [] : Set(try await context.queries.goals(today: context.today).goals.map(\.categoryId))
		return BudgetHabits.text(sheets: sheets, goalCategories: goals, format: context.format)
	}
}

/// AI-11.4: what Mo Nee answers, for a question none of the other tools can.
/// The redirect comes back from a tool rather than sitting in the
/// instructions, where its example questions made the model open real
/// answers with it.
struct WhatMoNeeAnswersTool: AssistantTool {
	let name = "whatMoNeeAnswers"
	let description = "What Mo Nee can answer, for a question no other tool answers."
	let arguments: [AssistantToolArgument] = []

	func run(_ input: AssistantToolInput) async throws -> String {
		AssistantPrompt.redirect
	}
}

enum BudgetHabits {
	static let noHistory = "Budget habits: no complete month of history yet."
	static let scales = "Budget habits can't be compared in this budget, because budgets and activity are recorded at different scales."
	static let basis = "Activity and budgeted are the Budget screen's figures for each month. Carryover is not counted."
	static let disclosure = "Hidden categories, goal categories and months with no budget aren't included."
	static let shown = 3

	/// One considered category over the window's budgeted months (AI-10.3 to AI-10.5).
	struct Category {
		let id: String
		let name: String
		let group: String
		var budgetedMonths = 0
		var overMonths = 0
		var activitySum: Int64 = 0
		var budgetedSum: Int64 = 0

		var averageActivity: Int64 { BudgetHabits.average(activitySum, over: budgetedMonths) }
		var averageBudgeted: Int64 { BudgetHabits.average(budgetedSum, over: budgetedMonths) }
		var overage: Int64 { averageActivity - averageBudgeted }
		var isListed: Bool { budgetedMonths >= 2 && 2 * overMonths > budgetedMonths && overage > 0 }
	}

	/// Rounded once to the minor unit, half away from zero, in integers (AI-10.5).
	static func average(_ sum: Int64, over count: Int) -> Int64 {
		guard count > 0 else { return 0 }
		let months = Int64(count)
		let magnitude = (2 * abs(sum) + months) / (2 * months)
		return sum < 0 ? -magnitude : magnitude
	}

	/// `sheets` are the window months that count, oldest first.
	static func text(sheets: [BudgetMonth], goalCategories: Set<String>, format: MoneyFormat) -> String {
		guard let first = sheets.first, let last = sheets.last else { return noHistory }
		if sheets.count == 1 {
			return "Budget habits: only 1 complete month of history (\(ReportPeriod.monthYear(first.month))), so there is no pattern to report yet."
		}
		if format.decimalPlaces != 2 { return ([scales] + (last.mixedScaleNote.map { [$0] } ?? [])).joined(separator: "\n") }
		func amount(_ value: Int64) -> String { format.currency(value, on: .budget) }
		let range = ReportPeriod(first: first.month, last: last.month).title
		let compared = categories(sheets: sheets, goalCategories: goalCategories).filter { $0.budgetedMonths >= 2 }
		guard !compared.isEmpty else {
			return ["No category has a monthly budget in \(range), so there is nothing to compare.", disclosure].joined(separator: "\n")
		}

		let listed = compared.filter(\.isListed).sorted { a, b in
			(-a.overage, -a.averageActivity, AssistantNames.key(a.name), a.id) < (-b.overage, -b.averageActivity, AssistantNames.key(b.name), b.id)
		}
		guard let top = listed.first else {
			let largest = compared.sorted { a, b in
				(-a.averageActivity, AssistantNames.key(a.name), a.id) < (-b.averageActivity, AssistantNames.key(b.name), b.id)
			}
			.prefix(shown)
			let names = labels(Array(largest))
			return (["Budget habits, \(range): no category ran over its monthly budget in most months.", "Largest categories by average activity:"]
				+ largest.map { "\(names[$0.id] ?? $0.name): average activity \(amount($0.averageActivity)) a month against \(amount($0.averageBudgeted)) budgeted." }
				+ [basis, disclosure]).joined(separator: "\n")
		}

		let k = listed.count
		let head = k == 1
			? "Budget habits, \(range): 1 category ran over its monthly budget in most months."
			: "Budget habits, \(range): \(k) categories ran over their monthly budget in most months."
		let shownCategories = Array(listed.prefix(shown))
		let names = labels(shownCategories)
		var text = [head]
		text += shownCategories.map { category in
			"\(names[category.id] ?? category.name): over in \(category.overMonths) of \(category.budgetedMonths) months. Average activity \(amount(category.averageActivity)) a month against \(amount(category.averageBudgeted)) budgeted, \(amount(category.overage)) a month over."
		}
		if k > shown { text.append("Showing the \(shown) largest of \(k).") }
		text.append("Bringing \(names[top.id] ?? top.name) back to its monthly budget would free about \(amount(top.overage)) a month (average of \(top.budgetedMonths) budgeted months in \(range)).")
		text += [basis, disclosure]
		return text.joined(separator: "\n")
	}

	/// AI-10.2: expense envelopes, not hidden and not in a hidden group as of
	/// today, of kind `expense` (contract v20), with no goal. AI-10.3: a month
	/// counts for a category only when it was budgeted that month.
	static func categories(sheets: [BudgetMonth], goalCategories: Set<String>) -> [Category] {
		guard let today = sheets.last else { return [] }
		var considered: [Category] = []
		for group in today.groups where !group.isIncome && !group.hidden {
			for envelope in group.envelopes where !envelope.hidden && envelope.kind == .expense && !goalCategories.contains(envelope.id) {
				considered.append(Category(id: envelope.id, name: envelope.name, group: group.name))
			}
		}
		for sheet in sheets {
			let rows = Dictionary(sheet.groups.flatMap(\.envelopes).map { ($0.id, $0) }) { first, _ in first }
			for index in considered.indices {
				guard let row = rows[considered[index].id], row.budgeted > 0 else { continue }
				let activity = -row.spent
				considered[index].budgetedMonths += 1
				considered[index].budgetedSum += row.budgeted
				considered[index].activitySum += activity
				if activity > row.budgeted { considered[index].overMonths += 1 }
			}
		}
		return considered
	}

	/// AI-10.6: two shown categories of one name read "<name> in <group>".
	static func labels(_ categories: [Category]) -> [String: String] {
		let keys = categories.map { AssistantNames.key($0.name) }
		return Dictionary(uniqueKeysWithValues: categories.enumerated().map { index, category in
			(category.id, keys.filter { $0 == keys[index] }.count > 1 ? "\(category.name) in \(category.group)" : category.name)
		})
	}
}
