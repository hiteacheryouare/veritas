import Foundation
import VeritasCore

/// docs/finance/assistant.md AI-10: the categories that ran over their
/// monthly budget in most of the last 3 complete months. The core read
/// `budgetHabits` computes every figure; this only formats them.
struct BudgetHabitsTool: AssistantTool {
	let context: AssistantContext
	let name = "budgetHabits"
	let description = "Categories over their monthly budget in most of the last 3 months."

	struct Arguments {}

	let arguments: [AssistantToolArgument] = []

	func run(_ input: AssistantToolInput) async throws -> String {
		try await call(arguments: Arguments())
	}

	func call(arguments: Arguments) async throws -> String {
		HabitsText.text(try await context.queries.budgetHabits(today: context.today), format: context.format)
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

/// AI-10.9's lines for the core's `BudgetHabits`.
enum HabitsText {
	static let noHistory = "Budget habits: no complete month of history yet."
	static let scales = "Budget habits can't be compared in this budget, because budgets and activity are recorded at different scales."
	static let basis = "Activity and budgeted are the Budget screen's figures for each month. Carryover is not counted."
	static let disclosure = "Hidden categories, goal categories and months with no budget aren't included."

	static func text(_ habits: BudgetHabits, format: MoneyFormat) -> String {
		func amount(_ value: Int64) -> String { format.currency(value, on: .budget) }
		let range = habits.months.first.flatMap { first in habits.months.last.map { ReportPeriod(first: first, last: $0).title } } ?? ""
		switch habits.outcome {
		case .noHistory:
			return noHistory
		case .oneMonth:
			let month = habits.months.first.map(ReportPeriod.monthYear) ?? ""
			return "Budget habits: only 1 complete month of history (\(month)), so there is no pattern to report yet."
		case .differentScales(let note):
			return ([scales] + (note.map { [$0] } ?? [])).joined(separator: "\n")
		case .noBudgets:
			return ["No category has a monthly budget in \(range), so there is nothing to compare.", disclosure].joined(separator: "\n")
		case .nothingOver(let largest):
			let names = labels(largest)
			return (["Budget habits, \(range): no category ran over its monthly budget in most months.", "Largest categories by average activity:"]
				+ largest.map { "\(names[$0.categoryId] ?? $0.name): average activity \(amount($0.averageActivity)) a month against \(amount($0.averageBudgeted)) budgeted." }
				+ [basis, disclosure]).joined(separator: "\n")
		case .over(let qualifying, let shown, let suggestion, let suggestionMonths):
			let names = labels(shown)
			var text = [qualifying == 1
				? "Budget habits, \(range): 1 category ran over its monthly budget in most months."
				: "Budget habits, \(range): \(qualifying) categories ran over their monthly budget in most months."]
			text += shown.map { category in
				"\(names[category.categoryId] ?? category.name): over in \(category.overMonths) of \(category.budgetedMonths) months. Average activity \(amount(category.averageActivity)) a month against \(amount(category.averageBudgeted)) budgeted, \(amount(category.overage)) a month over."
			}
			if Int(qualifying) > shown.count { text.append("Showing the \(shown.count) largest of \(qualifying).") }
			if let top = shown.first {
				text.append("Bringing \(names[top.categoryId] ?? top.name) back to its monthly budget would free about \(amount(suggestion)) a month (average of \(suggestionMonths) budgeted months in \(range)).")
			}
			text += [basis, disclosure]
			return text.joined(separator: "\n")
		}
	}

	/// AI-10.6: two shown categories of one name read "<name> in <group>".
	static func labels(_ categories: [HabitsCategory]) -> [String: String] {
		let keys = categories.map { AssistantNames.key($0.name) }
		return Dictionary(categories.enumerated().map { index, category in
			(category.categoryId, keys.filter { $0 == keys[index] }.count > 1 ? "\(category.name) in \(category.groupName)" : category.name)
		}) { first, _ in first }
	}
}
