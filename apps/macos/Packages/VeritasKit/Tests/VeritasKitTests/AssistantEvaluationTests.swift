import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// Asks the real on-device model questions about fixtures/synthetic/all-features
/// and checks the figure each answer must contain, as the matching screen shows it.
/// Slow, and the model varies, so it runs only with VERITAS_AI_EVAL=1:
/// `cargo xtask ai-eval`. It needs the plug-in that command builds and the model downloaded.
@MainActor
@Suite(.enabled(if: ProcessInfo.processInfo.environment["VERITAS_AI_EVAL"] == "1", "set VERITAS_AI_EVAL=1, or run cargo xtask ai-eval"))
struct AssistantEvaluationTests {
	/// The fixture's spending ends in October 2026, so today is its last day:
	/// "last month" is September, and "so far this month" is all of October.
	static let today = "2026-10-31"
	static let september = Month(year: 2026, month: 9)
	static let october = Month(year: 2026, month: 10)

	struct Case {
		let question: String
		let expected: String
	}

	private func netProfit(_ budget: RealBudget, _ first: Month, _ last: Month) async -> NetProfitModel {
		let model = NetProfitModel(workspace: budget.workspace, thisMonth: Self.october)
		await model.setCustom(first: first, last: last)
		return model
	}

	private func total(_ line: NetProfitLine, in model: NetProfitModel) throws -> Int64 {
		try #require(model.rows.first { $0.line == line && $0.style != .category }?.cells.last?.amount)
	}

	private func category(_ id: String, in model: NetProfitModel) throws -> ReportCategory {
		try #require(model.report?.total.expenses.categories.first { $0.categoryId == id })
	}

	/// Each question with the figure, formatted as the app formats it, that a right answer contains.
	private func cases(_ budget: RealBudget) async throws -> [Case] {
		let format = budget.workspace.format
		func amount(_ value: Int64) -> String { format.amount(value, on: .register) }
		let september = await netProfit(budget, Self.september, Self.september)
		let october = await netProfit(budget, Self.october, Self.october)
		let year = await netProfit(budget, Month(year: 2026, month: 1), Month(year: 2026, month: 12))
		let groceriesInSeptember = try category(AllFeatures.groceries, in: september)
		let groceriesInOctober = try category(AllFeatures.groceries, in: october)
		let busiest = try #require(october.report?.total.expenses.categories.filter { $0.categoryId != nil && $0.categoryId != AllFeatures.groceries }.max { $0.count < $1.count })
		let everyday = try #require(september.report?.total.expenses.categories.filter { $0.groupId == AllFeatures.everydayGroup })
		let everydayName = try #require(everyday.first?.groupName)
		let screen = await budget.screen(Self.october)
		let budgeted = try #require(screen.sheet).row(AllFeatures.groceries).budgeted
		return [
			Case(question: "What did I spend last month?", expected: amount(try total(.expenses, in: september))),
			Case(question: "How much did I spend in October 2026?", expected: amount(try total(.expenses, in: october))),
			Case(question: "How much have I spent so far this month?", expected: amount(try total(.expenses, in: october))),
			Case(question: "How much did I spend on \(groceriesInSeptember.name) last month?", expected: amount(groceriesInSeptember.total)),
			Case(question: "how much did i spend on \(busiest.name.lowercased()) so far this month?", expected: amount(busiest.total)),
			Case(question: "How much did I spend on \(groceriesInOctober.name.uppercased()) in October?", expected: amount(groceriesInOctober.total)),
			Case(question: "What did I spend on \(everydayName) last month?", expected: amount(everyday.reduce(0) { $0 + $1.total })),
			Case(question: "What was my income this year?", expected: amount(try total(.income, in: year))),
			Case(question: "How much did I earn last month?", expected: amount(try total(.income, in: september))),
			Case(question: "How much did I budget for \(groceriesInOctober.name) this month?", expected: format.amount(budgeted, on: .budget)),
			Case(question: "What was my Net Profit in October 2026?", expected: amount(try total(.netProfit, in: october)))
		]
	}

	/// "1,234.56" and "1234.56" and "$1,234.56" all hold 1234.56.
	static func numbers(in text: String) -> Set<String> {
		Set(text.matches(of: /[0-9][0-9,.\u{202F}' ]*[0-9]/).map { match in
			String(match.output).filter { $0.isNumber || $0 == "." }
		})
	}

	static func contains(_ answer: String, _ expected: String) -> Bool {
		let wanted = expected.filter { $0.isNumber || $0 == "." }
		return numbers(in: answer).contains(wanted)
	}

	@Test func theModelAnswersBasicQuestionsWithTheRightFigures() async throws {
		let plugin = AssistantPlugin(url: AssistantPluginTests.builtPlugin)
		try #require(PluginAvailability(plugin: plugin).availability == .available, "the plug-in (cargo xtask ai-eval builds it) and the on-device model are needed")
		try await RealBudget.with { budget in
			let context = AssistantContext(queries: try #require(budget.workspace.readOnlyQueries), format: budget.workspace.format, today: Self.today)
			let cases = try await cases(budget)
			var passed = 0
			for item in cases {
				var answer = ""
				do {
					for try await partial in PluginAssistant(context: context, plugin: plugin).answer(item.question) { answer = partial }
				} catch {
					answer = "failed: \(AssistantFailure(error).message)"
				}
				let right = Self.contains(answer, item.expected)
				if right { passed += 1 }
				print("AI-EVAL \(right ? "pass" : "FAIL") | \(item.question) | expected \(item.expected) | \(answer.replacingOccurrences(of: "\n", with: " "))")
				#expect(right, "\(item.question) wants \(item.expected): \(answer)")
			}
			print("AI-EVAL passed \(passed) of \(cases.count)")
		}
	}
}
