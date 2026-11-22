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
			Case(question: "How much have I spent on \(busiest.name) so far this month?", expected: amount(busiestFirstHalf), context: middle)
		]
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
		let budgets = URL.temporaryDirectory.appending(path: "veritas-eval-\(UUID().uuidString)", directoryHint: .isDirectory)
		defer { try? FileManager.default.removeItem(at: budgets) }
		let yen = WorkspaceModel(library: CoreBudgetLibrary(budgetsDirectory: budgets), format: MoneyFormat())
		await ask(try await yenCases(yen))
		await yen.closeBudget()

		var passed = 0
		for (item, answer) in results {
			let right = AssistantEvaluationCheck.leads(answer, with: item.expected)
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

	/// The labelled answer: the answer's first line, up to its first sentence break.
	static func firstSentence(_ answer: String) -> String {
		let line = answer.split(separator: "\n", omittingEmptySubsequences: true).first.map(String.init) ?? ""
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
	}
}
