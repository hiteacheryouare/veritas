import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// Asks the real on-device model questions about fixtures/synthetic/all-features
/// and a yen budget, and checks that each answer's first sentence holds the
/// figure, with its sign, that the matching screen shows.
/// Slow, and the model varies, so it runs only with VERITAS_AI_EVAL=1:
/// `cargo xtask ai-eval`. It needs the plug-in that command builds and the model downloaded.
@MainActor
@Suite(.enabled(if: ProcessInfo.processInfo.environment["VERITAS_AI_EVAL"] == "1", "set VERITAS_AI_EVAL=1, or run cargo xtask ai-eval"))
struct AssistantEvaluationTests {
	/// The fixture's spending ends in October 2026. On its last day "so far
	/// this month" is all of October; on the 15th it is half, and differs.
	static let endOfOctober = "2026-10-31"
	static let midOctober = "2026-10-15"
	static let september = Month(year: 2026, month: 9)
	static let october = Month(year: 2026, month: 10)

	struct Case {
		let question: String
		let expected: String
		let context: AssistantContext
		/// Nil checks that the answer leads with `expected`; otherwise this decides.
		var passes: (@Sendable (String) -> Bool)? = nil
	}

	/// AI-11.2's banned words, which no answer may use.
	nonisolated static let banned = ["you should", "you must", "you need to", "i recommend", "i can't", "i cannot"]

	/// AI-11.4: the redirect, with no figure and no advice.
	nonisolated static func redirected(_ answer: String) -> Bool {
		answer.contains("Mo Nee answers from your own numbers") && !answer.contains(/\d\.\d\d/) && clean(answer)
	}

	nonisolated static func clean(_ answer: String) -> Bool {
		!banned.contains { answer.lowercased().contains($0) }
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

	/// Each question with the figure, formatted as the app formats it, that a right answer leads with.
	private func cases(_ budget: RealBudget) async throws -> [Case] {
		let format = budget.workspace.format
		func amount(_ value: Int64) -> String { format.currency(value, on: .register) }
		let queries = try #require(budget.workspace.readOnlyQueries)
		let end = AssistantContext(queries: queries, format: format, today: Self.endOfOctober)
		let middle = AssistantContext(queries: queries, format: format, today: Self.midOctober)
		let september = await netProfit(budget, Self.september, Self.september)
		let october = await netProfit(budget, Self.october, Self.october)
		let year = await netProfit(budget, Month(year: 2026, month: 1), Month(year: 2026, month: 12))
		let firstHalf = try await queries.netProfitPeriod(first: "2026-10-01", last: Self.midOctober)
		let groceriesInSeptember = try category(AllFeatures.groceries, in: september)
		let groceriesInOctober = try category(AllFeatures.groceries, in: october)
		let busiest = try #require(october.report?.total.expenses.categories.filter { $0.categoryId != nil && $0.categoryId != AllFeatures.groceries }.max { $0.count < $1.count })
		let busiestFirstHalf = firstHalf.column.expenses.categories.first { $0.categoryId == busiest.categoryId }?.total ?? 0
		let everyday = try #require(september.report?.total.expenses.categories.filter { $0.groupId == AllFeatures.everydayGroup })
		let everydayName = try #require(everyday.first?.groupName)
		let screen = await budget.screen(Self.october)
		let budgeted = try #require(screen.sheet).row(AllFeatures.groceries).budgeted
		#expect(firstHalf.column.expenses.total != (try total(.expenses, in: october)), "mid-month, so far and this month differ")
		let septemberTotal = amount(try total(.expenses, in: september))
		let byCategory = try await SpendingByCategoryTool(context: end).call(arguments: .init(period: "last month")).components(separatedBy: "\n")
		let topSeptember = try #require(byCategory.dropFirst(3).first)
		let topSeptemberFigure = try #require(AssistantEvaluationCheck.figures(in: String(topSeptember.split(separator: ":").last ?? "")).first)
		let byCategoryFigures = AssistantEvaluationCheck.figures(in: byCategory.joined(separator: "\n"))
		let spendingText = try await SpendingTool(context: end).call(arguments: .init(period: "last month"))
		let spendingFigures = AssistantEvaluationCheck.figures(in: spendingText)
		let everydayFigure = try #require(AssistantEvaluationCheck.figures(in: String(spendingText.split(separator: "\n").first { $0.hasPrefix("\(everydayName):") } ?? "")).first)
		return [
			Case(question: "What did I spend last month?", expected: amount(try total(.expenses, in: september)), context: end),
			Case(question: "How much did I spend in October 2026?", expected: amount(try total(.expenses, in: october)), context: end),
			Case(question: "How much have I spent so far this month?", expected: amount(try total(.expenses, in: october)), context: end),
			Case(question: "How much did I spend on \(groceriesInSeptember.name) last month?", expected: amount(groceriesInSeptember.total), context: end),
			Case(question: "how much did i spend on \(busiest.name.lowercased()) so far this month?", expected: amount(busiest.total), context: end),
			Case(question: "How much did I spend on \(groceriesInOctober.name.uppercased()) in October?", expected: amount(groceriesInOctober.total), context: end),
			Case(question: "What did I spend on \(everydayName) last month?", expected: amount(everyday.reduce(0) { $0 + $1.total }), context: end),
			Case(question: "What was my income this year?", expected: amount(try total(.income, in: year)), context: end),
			Case(question: "How much did I earn last month?", expected: amount(try total(.income, in: september)), context: end),
			Case(question: "How much did I budget for \(groceriesInOctober.name) this month?", expected: format.currency(budgeted, on: .budget), context: end),
			Case(question: "What was my Net Profit in October 2026?", expected: amount(try total(.netProfit, in: october)), context: end),
			Case(question: "How much have I spent so far this month?", expected: amount(firstHalf.column.expenses.total), context: middle),
			Case(question: "What is my spending for this month?", expected: amount(try total(.expenses, in: october)), context: middle),
			Case(question: "How much have I spent on \(busiest.name) so far this month?", expected: amount(busiestFirstHalf), context: middle),
			Case(question: "How can I improve my money habits?", expected: "Household, \(format.currency(121, on: .budget)) a month", context: AssistantContext(queries: queries, format: format, today: "2026-11-22")) { answer in
				answer.contains("Household") && AssistantEvaluationCheck.figures(in: answer).contains("1.21") && Self.clean(answer)
			},
			Case(question: "What is the best index fund to invest in?", expected: AssistantPrompt.redirect, context: end) { answer in
				Self.redirected(answer)
			},
			Case(question: "What should I change about my investments?", expected: AssistantPrompt.redirect, context: end) { answer in
				Self.redirected(answer)
			},
			Case(question: "What should I change in my budget?", expected: "Household, \(format.currency(121, on: .budget)) a month", context: AssistantContext(queries: queries, format: format, today: "2026-11-22")) { answer in
				answer.contains("Household") && AssistantEvaluationCheck.figures(in: answer).contains("1.21") && Self.clean(answer)
			},
			Case(question: "What did I spend last month by category?", expected: topSeptember, context: end) { answer in
				let figures = AssistantEvaluationCheck.figures(in: answer)
				return figures.contains(topSeptemberFigure) && figures.isSubset(of: byCategoryFigures) && !answer.lowercased().contains("which one") && Self.clean(answer)
			},
			Case(question: "What did I spend the most on last month?", expected: topSeptember, context: end) { answer in
				AssistantEvaluationCheck.figures(in: answer).contains(topSeptemberFigure) && Self.clean(answer)
			},
			Case(question: "Break down my spending by group for last month.", expected: "the group lines of spending", context: end) { answer in
				let figures = AssistantEvaluationCheck.figures(in: answer)
				return answer.contains(everydayName) && figures.contains(everydayFigure) && figures.isSubset(of: spendingFigures) && !answer.contains("Everything else") && Self.clean(answer)
			}
		]
	}

	/// AI-10.10's example 3: nothing ran over in most months, read by the
	/// real tool from Budget screen sheets the test supplies.
	private func nothingOverCase() -> Case {
		let sheets = AssistantHabitsTests.sheets(Array(AssistantHabitsTests.scenarioH.suffix(2)), months: AssistantHabitsTests.window, firstMonth: Month(year: 2026, month: 1))
		let context = AssistantContext(queries: AssistantHabitsTests.SheetQueries(sheets: sheets), format: MoneyFormat(), today: "2026-11-22")
		return Case(question: "Which categories ran over budget lately?", expected: "no category ran over", context: context) { answer in
			let said = answer.lowercased()
			return said.contains("no category ran over") && !said.contains("groceries ran over") && !said.contains("gas ran over") && !said.contains("free about") && !answer.hasPrefix("Mo Nee answers") && Self.clean(answer)
		}
	}

	/// A yen budget, made the way the app makes one: one ¥1,200 purchase and ¥1,000 budgeted.
	private func yenCases(_ workspace: WorkspaceModel) async throws -> [Case] {
		await workspace.createBudget(NewBudget(
			name: "Yen", currencyCode: "JPY", numberStyle: .commaDot,
			accounts: [StartingAccount(name: "Wallet", type: .cash, balance: 10_000_000)],
			hasRetirementPlan: false, hasHealthSavingsAccount: false
		))
		try #require(workspace.phase == .open)
		let wallet = try #require(workspace.overview?.accounts.first { $0.name == "Wallet" }).id
		let goals = GoalsScreenModel(workspace: workspace)
		await goals.load()
		#expect(await goals.create(name: "Rice cooker", target: 1_000, date: nil, icon: nil, color: nil))
		let cooker = try #require(goals.open.first { $0.name == "Rice cooker" })
		let screen = BudgetScreenModel(workspace: workspace)
		await screen.load()
		screen.beginEditing(cooker.categoryId)
		screen.editing?.type("1000")
		await screen.commitEditing()
		let today = WorkspaceModel.today()
		#expect(await workspace.perform(.addTransaction(transaction: TransactionDraft(
			id: nil, accountId: wallet, date: today, amount: -120_000, payeeId: nil, payeeName: "Appliance shop",
			categoryId: cooker.categoryId, notes: nil, cleared: true, splits: []
		))))
		let format = workspace.format
		#expect(format.decimalPlaces == 0)
		let context = AssistantContext(queries: try #require(workspace.readOnlyQueries), format: format, today: today)
		return [
			Case(question: "How much did I spend this month?", expected: format.currency(120_000, on: .register), context: context),
			Case(question: "How much did I budget for Rice cooker this month?", expected: format.currency(1_000, on: .budget), context: context)
		]
	}

	@Test func theModelAnswersBasicQuestionsWithTheRightFigures() async throws {
		let plugin = AssistantPlugin(url: AssistantPluginTests.builtPlugin)
		try #require(PluginAvailability(plugin: plugin).availability == .available, "the plug-in (cargo xtask ai-eval builds it) and the on-device model are needed")
		var results: [(Case, String)] = []
		func ask(_ cases: [Case]) async {
			for item in cases {
				var answer = ""
				do {
					for try await partial in PluginAssistant(context: item.context, plugin: plugin).answer(item.question) { answer = partial }
				} catch {
					answer = "failed: \(AssistantFailure(error).message)"
				}
				results.append((item, answer))
			}
		}
		try await RealBudget.with { budget in
			await ask(try await cases(budget))
		}
		await ask([nothingOverCase()])
		let budgets = URL.temporaryDirectory.appending(path: "veritas-eval-\(UUID().uuidString)", directoryHint: .isDirectory)
		defer { try? FileManager.default.removeItem(at: budgets) }
		let yen = WorkspaceModel(library: CoreBudgetLibrary(budgetsDirectory: budgets), format: MoneyFormat())
		await ask(try await yenCases(yen))
		await yen.closeBudget()

		var passed = 0
		for (item, answer) in results {
			let right = item.passes?(answer) ?? AssistantEvaluationCheck.leads(answer, with: item.expected)
			if right { passed += 1 }
			print("AI-EVAL \(right ? "pass" : "FAIL") | today \(item.context.today) | \(item.question) | expected \(item.expected) | \(answer.replacingOccurrences(of: "\n", with: " "))")
			#expect(right, "\(item.question) on \(item.context.today) wants \(item.expected) first: \(answer)")
		}
		print("AI-EVAL passed \(passed) of \(results.count)")
	}
}

/// How the evaluation reads an answer, checked without the model.
struct AssistantEvaluationCheck {
	/// "−$1,234.56", "-1234.56" and "$ -1,234.56" all read as -1234.56; "1,234.56" as 1234.56.
	static func figures(in text: String) -> Set<String> {
		Set(text.matches(of: /([-\u{2212}])?\s?[$¥€£]?\s?([-\u{2212}])?(\d(?:[\d,\u{202F}']*\d)?(?:\.\d+)?)/).map { match in
			let negative = match.output.1 != nil || match.output.2 != nil
			return (negative ? "-" : "") + String(match.output.3).filter { $0.isNumber || $0 == "." }
		})
	}

	/// The labelled answer: the answer's first line that carries a figure, up
	/// to its first sentence break. A heading line before it, such as "Your
	/// spending for this month is:", is skipped; a figure further down never counts.
	static func firstSentence(_ answer: String) -> String {
		let lines = answer.split(separator: "\n", omittingEmptySubsequences: true).map(String.init)
		let line = lines.first { $0.contains(/\d/) } ?? lines.first ?? ""
		return line.split(separator: /\.\s+(?=[A-Z])/, maxSplits: 1).first.map(String.init) ?? line
	}

	static func leads(_ answer: String, with expected: String) -> Bool {
		figures(in: firstSentence(answer)).contains(figures(in: expected).first ?? "")
	}

	@Test func theFigureCheckIsSignAwareAndReadsOnlyTheFirstSentence() {
		#expect(Self.leads("You spent $2,443.61 in September 2026.", with: "$2,443.61"))
		#expect(Self.leads("Spending on Dining out, November 2026: \u{2212}$80.00", with: "\u{2212}$80.00"))
		#expect(!Self.leads("Spending on Dining out, November 2026: $80.00", with: "\u{2212}$80.00"))
		#expect(!Self.leads("Here is your answer. You spent 2,443.61.", with: "2,443.61"))
		#expect(!Self.leads("Spending was 12.00.\nEveryday: 2,443.61", with: "2,443.61"))
		#expect(Self.leads("Your spending for this month is:\nTotal: 2,489.35", with: "2,489.35"), "a heading line with no figure is skipped")
	}
}
