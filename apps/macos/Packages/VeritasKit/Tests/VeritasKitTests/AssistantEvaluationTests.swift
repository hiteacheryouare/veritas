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
		/// The tool output whose category and group names are the only ones the
		/// answer may use (AI-1.6). Empty for cases with no names to check.
		/// The period the tool's first line states (AI-1.2): any month or year
		/// the answer's headline names must be one of these.
		var label: String? = nil
		/// The budget's own category and group names, checked in prose too.
		var known: [String] = []
		var sources: [String] = []
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
		let acrossYears = await netProfit(budget, Month(year: 2025, month: 11), Month(year: 2026, month: 1))
		let fourthQuarter = await netProfit(budget, Month(year: 2025, month: 10), Month(year: 2025, month: 12))
		let lateNovember = AssistantContext(queries: queries, format: format, today: "2026-11-22")
		let sinceOctober = try await queries.netProfitPeriod(first: "2026-10-01", last: "2026-11-22")
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
		func spentOn(_ name: String, _ period: String, _ context: AssistantContext) async throws -> [String] {
			[try await CategorySpendingTool(context: context).call(arguments: .init(name: name, period: period))]
		}
		let fixtureNames = try await {
			let sheet = try await queries.budgetMonth(Self.october, today: Self.endOfOctober)
			return sheet.groups.flatMap { [$0.name] + $0.envelopes.map(\.name) }
		}()
		let habitsText = try await BudgetHabitsTool(context: AssistantContext(queries: queries, format: format, today: "2026-11-22")).call(arguments: .init())
		let byCategory = try await SpendingByCategoryTool(context: end).call(arguments: .init(period: "last month")).components(separatedBy: "\n")
		let topSeptember = try #require(byCategory.dropFirst(3).first)
		let topSeptemberFigure = try #require(AssistantEvaluationCheck.figures(in: String(topSeptember.split(separator: ":").last ?? "")).first)
		let byCategoryFigures = AssistantEvaluationCheck.figures(in: byCategory.joined(separator: "\n"))
		let spendingText = try await SpendingTool(context: end).call(arguments: .init(period: "last month"))
		let spendingFigures = AssistantEvaluationCheck.figures(in: spendingText)
		let everydayFigure = try #require(AssistantEvaluationCheck.figures(in: String(spendingText.split(separator: "\n").first { $0.hasPrefix("\(everydayName):") } ?? "")).first)
		return [
			Case(question: "What did I spend last month?", expected: amount(try total(.expenses, in: september)), context: end, label: "September 2026"),
			Case(question: "How much did I spend in October 2026?", expected: amount(try total(.expenses, in: october)), context: end, label: "October 2026"),
			Case(question: "How much have I spent so far this month?", expected: amount(try total(.expenses, in: october)), context: end, label: "October 2026"),
			Case(question: "How much did I spend on \(groceriesInSeptember.name) last month?", expected: amount(groceriesInSeptember.total), context: end, label: "September 2026", known: fixtureNames, sources: try await spentOn(groceriesInSeptember.name, "last month", end)),
			Case(question: "how much did i spend on \(busiest.name.lowercased()) so far this month?", expected: amount(busiest.total), context: end, label: "October 2026", known: fixtureNames, sources: try await spentOn(busiest.name, "so far this month", end)),
			Case(question: "How much did I spend on \(groceriesInOctober.name.uppercased()) in October?", expected: amount(groceriesInOctober.total), context: end, label: "October 2026", known: fixtureNames, sources: try await spentOn(groceriesInOctober.name, "October", end)),
			Case(question: "What did I spend on \(everydayName) last month?", expected: amount(everyday.reduce(0) { $0 + $1.total }), context: end, label: "September 2026", known: fixtureNames, sources: try await spentOn(everydayName, "last month", end)),
			Case(question: "How much did I spend from November 2025 to January 2026?", expected: amount(try total(.expenses, in: acrossYears)), context: end, label: "November 2025 to January 2026"),
			Case(question: "How much have I spent since October?", expected: amount(sinceOctober.column.expenses.total), context: lateNovember, label: "October 1 to November 22, 2026"),
			Case(question: "How much did I spend in Q4 2025?", expected: amount(try total(.expenses, in: fourthQuarter)), context: end, label: "October to December 2025"),
			Case(question: "How much did I spend over the holidays last year?", expected: "no figure: the period words are refused (AI-6.11)", context: end) { answer in
				!answer.hasPrefix("failed:") && !answer.contains(/\d\.\d\d/) && Self.clean(answer)
			},
			Case(question: "What was my income this year?", expected: amount(try total(.income, in: year)), context: end, label: "2026"),
			Case(question: "How much did I earn last month?", expected: amount(try total(.income, in: september)), context: end, label: "September 2026"),
			Case(question: "How much did I budget for \(groceriesInOctober.name) this month?", expected: format.currency(budgeted, on: .budget), context: end, label: "October 2026"),
			Case(question: "What was my Net Profit in October 2026?", expected: amount(try total(.netProfit, in: october)), context: end, label: "October 2026"),
			Case(question: "How much have I spent so far this month?", expected: amount(firstHalf.column.expenses.total), context: middle, label: "October 1 to 15, 2026"),
			Case(question: "What is my spending for this month?", expected: amount(try total(.expenses, in: october)), context: middle, label: "October 2026"),
			Case(question: "How much have I spent on \(busiest.name) so far this month?", expected: amount(busiestFirstHalf), context: middle, label: "October 1 to 15, 2026", known: fixtureNames, sources: try await spentOn(busiest.name, "so far this month", middle)),
			Case(question: "How can I improve my money habits?", expected: "Household, \(format.currency(121, on: .budget)) a month", context: AssistantContext(queries: queries, format: format, today: "2026-11-22"), known: fixtureNames, sources: [habitsText]) { answer in
				answer.contains("Household") && AssistantEvaluationCheck.figures(in: answer).contains("1.21") && Self.clean(answer)
			},
			Case(question: "What is the best index fund to invest in?", expected: AssistantPrompt.redirect, context: end) { answer in
				Self.redirected(answer)
			},
			Case(question: "What should I change about my investments?", expected: AssistantPrompt.redirect, context: end) { answer in
				Self.redirected(answer)
			},
			Case(question: "What should I change in my budget?", expected: "Household, \(format.currency(121, on: .budget)) a month", context: AssistantContext(queries: queries, format: format, today: "2026-11-22"), known: fixtureNames, sources: [habitsText]) { answer in
				answer.contains("Household") && AssistantEvaluationCheck.figures(in: answer).contains("1.21") && Self.clean(answer)
			},
			Case(question: "What did I spend last month by category?", expected: topSeptember, context: end, label: "September 2026", known: fixtureNames, sources: [byCategory.joined(separator: "\n")]) { answer in
				let figures = AssistantEvaluationCheck.figures(in: answer)
				return AssistantEvaluationCheck.leads(answer, with: septemberTotal) && figures.contains(topSeptemberFigure) && figures.isSubset(of: byCategoryFigures) && !answer.lowercased().contains("which one") && Self.clean(answer)
			},
			Case(question: "What did I spend the most on last month?", expected: topSeptember, context: end, label: "September 2026", known: fixtureNames, sources: [byCategory.joined(separator: "\n")]) { answer in
				AssistantEvaluationCheck.figures(in: answer).contains(topSeptemberFigure) && !AssistantEvaluationCheck.mislabelsTotal(answer, total: septemberTotal) && Self.clean(answer)
			},
			Case(question: "Break down my spending by group for last month.", expected: "the group lines of spending", context: end, label: "September 2026", known: fixtureNames, sources: [spendingText]) { answer in
				let figures = AssistantEvaluationCheck.figures(in: answer)
				return AssistantEvaluationCheck.leads(answer, with: septemberTotal) && answer.contains(everydayName) && figures.contains(everydayFigure) && figures.isSubset(of: spendingFigures) && !answer.contains("Everything else") && Self.clean(answer)
			}
		]
	}

	/// AI-10.10's example 3: nothing ran over in most months, read by the
	/// real tool from Budget screen sheets the test supplies.
	private func nothingOverCase() -> Case {
		let result = BudgetHabits(months: AssistantHabitsTests.window, outcome: .nothingOver(largest: [
			AssistantHabitsTests.category("Groceries", 1, 3, 44_000, 45_000), AssistantHabitsTests.category("Gas", 1, 3, 10_000, 10_000)
		]))
		let context = AssistantContext(queries: AssistantHabitsTests.HabitsQueries(habits: result), format: MoneyFormat(), today: "2026-11-22")
		let habits = HabitsText.text(result, format: MoneyFormat())
		return Case(question: "Which categories ran over budget lately?", expected: "no category ran over", context: context, known: ["Groceries", "Gas"], sources: [habits]) { answer in
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
			Case(question: "How much did I spend this month?", expected: format.currency(120_000, on: .register), context: context, label: ReportPeriod.monthYear(Month(date: today) ?? Month(year: 2026, month: 11))),
			Case(question: "How much did I budget for Rice cooker this month?", expected: format.currency(1_000, on: .budget), context: context, label: ReportPeriod.monthYear(Month(date: today) ?? Month(year: 2026, month: 11)))
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
			let grounded = item.sources.isEmpty || AssistantEvaluationCheck.grounded(answer, in: item.sources, known: item.known)
			let dated = item.label.map { AssistantEvaluationCheck.datesAgree(answer, with: $0) } ?? true
			let right = (item.passes?(answer) ?? AssistantEvaluationCheck.leads(answer, with: item.expected)) && grounded && dated
			if right { passed += 1 }
			print("AI-EVAL \(right ? "pass" : "FAIL")\(grounded ? "" : " (ungrounded: \(AssistantEvaluationCheck.ungrounded(answer, in: item.sources, known: item.known).sorted()))")\(dated ? "" : " (dates differ from \(item.label ?? ""))") | today \(item.context.today) | \(item.question) | expected \(item.expected) | \(answer.replacingOccurrences(of: "\n", with: " "))")
			#expect(right, "\(item.question) on \(item.context.today) wants \(item.expected) first: \(answer)")
		}
		print("AI-EVAL passed \(passed) of \(results.count)")
	}
}

/// How the evaluation reads an answer, checked without the model.
struct AssistantEvaluationCheck {
	/// Words that label a figure without naming a category or group: the
	/// tools' fixed wording, the model's own headings and dates. A name made
	/// only of these is a heading; one with any other word, such as
	/// "Pet Spending", is checked.
	static let headingWords = Set([
		"total", "spending", "spent", "income", "expenses", "expense", "net", "profit", "cash", "flow", "everything", "else", "average", "budget", "budgeted",
		"activity", "available", "debt", "paydown", "savings", "from", "category", "categories", "group", "groups", "breakdown", "by", "your", "my", "the",
		"of", "for", "in", "on", "last", "this", "month", "year", "line", "is", "was", "you", "and", "a", "an", "to", "so", "far",
		"most", "biggest", "largest", "top", "highest", "spend", "it", "at", "with", "were", "are", "that", "which", "here", "what"
	]).union(monthNames)

	/// Words that may lead into a name in prose ("You spent the most on
	/// Rent"): pronouns, verbs, prepositions, articles and time words. No
	/// finance noun is here, so "Savings Rent" and "Cash Gifts" are not Rent
	/// and Gifts.
	static let leadInWords = Set([
		"you", "i", "we", "spent", "spend", "paid", "went", "go", "goes", "was", "were", "is", "are", "it", "that", "which", "here", "what", "where",
		"the", "a", "an", "most", "biggest", "largest", "top", "highest", "on", "for", "at", "in", "into", "to", "of", "with", "and", "so", "far",
		"your", "my", "this", "last", "month", "year", "week"
	]).union(monthNames)

	static func isHeading(_ name: String) -> Bool {
		let words = name.lowercased().split(whereSeparator: { !$0.isLetter }).map(String.init)
		return words.allSatisfy(headingWords.contains)
	}

	/// The names an answer attaches to a figure ("Rent: 1,800.00",
	/// "Bills (1,934.49)") and any it quotes, without a trailing ST-10.3 label.
	static func labelledNames(in answer: String) -> Set<String> {
		let plain = answer.replacingOccurrences(of: "*", with: "")
		var names: [String] = []
		for match in plain.matches(of: /([A-Za-z][A-Za-z0-9 &'’\/.-]{0,40}?)\s*:\s*[-\x{2212}]?\s?[$¥€£]?\s?\d/) { names.append(String(match.1)) }
		for match in plain.matches(of: /([A-Z][A-Za-z0-9 &'’\/.-]{0,40}?)\s*\(\s*[-\x{2212}]?[$¥€£]?\d/) { names.append(String(match.1)) }
		for match in plain.matches(of: /["“]([^"”]{1,40})["”]/) { names.append(String(match.1)) }
		return Set(names.map { name in
			let words = name.split(separator: " ")
			let last = words.last.map(String.init) ?? ""
			let lead = ["the", "and", "on", "for", "in", "-", "•"]
			let kept = words.drop { lead.contains($0.lowercased()) }.map(String.init)
			return ([String](kept.isEmpty ? [last] : kept)).joined(separator: " ").trimmingCharacters(in: .whitespacesAndNewlines.union(.punctuationCharacters))
		}
		.filter { name in
			let lower = name.lowercased()
			return !lower.isEmpty && !lower.contains(where: \.isNumber) && !isHeading(name)
		})
	}

	static let monthNames = ["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"]

	/// The month names and years a text states.
	static func dates(in text: String) -> Set<String> {
		let words = text.lowercased().split(whereSeparator: { !$0.isLetter && !$0.isNumber }).map(String.init)
		return Set(words.filter { monthNames.contains($0) || ($0.count == 4 && Int($0).map { (1900...2200).contains($0) } == true) })
	}

	/// AI-1.2: the headline may paraphrase the tool's period ("last month")
	/// but never names a month or year the tool's first line doesn't.
	static func datesAgree(_ answer: String, with label: String) -> Bool {
		dates(in: firstSentence(answer)).isSubset(of: dates(in: label))
	}

	/// The total's own wording, which may only ever sit beside the total.
	static let totalWords = ["total", "spending,", "expenses line"]

	/// The answer's lines, split again at sentence breaks (a decimal point is not one).
	static func sentences(_ answer: String) -> [String] {
		answer.split(separator: "\n").flatMap { line in
			String(line).split(separator: /[.!?]\s+(?=[A-Z])/).map(String.init)
		}
	}

	/// AI-1.2: a line or sentence that uses the total's wording ("total",
	/// "Spending, <range>:", "the Expenses line") with any figure but the
	/// total, such as "Rent: 1,800.00 This is the Expenses line".
	static func mislabelsTotal(_ answer: String, total: String) -> Bool {
		let wanted = figures(in: total)
		return sentences(answer).contains { sentence in
			let lower = sentence.lowercased()
			guard totalWords.contains(where: lower.contains) else { return false }
			let found = figures(in: sentence).filter { $0.contains(".") }
			return !found.isEmpty && !found.isSubset(of: wanted)
		}
	}

	/// Whether `name` is in `text` as whole words, so "Car" doesn't match "Car loan".
	static func mentions(_ text: String, _ name: String, ignoringCase: Bool = true) -> Bool {
		let pattern = "(?<![A-Za-z0-9])" + NSRegularExpression.escapedPattern(for: name) + "(?![A-Za-z0-9])"
		return text.range(of: pattern, options: ignoringCase ? [.regularExpression, .caseInsensitive] : [.regularExpression]) != nil
	}

	/// The answer's category and group names that the tool didn't return
	/// (AI-1.6): names it attaches to a figure or quotes, and any of the
	/// budget's own names it uses in prose, written as the budget writes them.
	/// The tool's names are those it attaches to figures, and the budget names
	/// its output mentions; an answer name must be one of them, so "Car" is not
	/// "Car loan".
	static func ungrounded(_ answer: String, in sources: [String], known: [String] = []) -> Set<String> {
		let text = sources.joined(separator: "\n")
		let prose = known.filter { name in
			!name.isEmpty && !isHeading(name) && mentions(answer, name, ignoringCase: false)
		}
		let returned = Set(labelledNames(in: text).union(known.filter { mentions(text, $0) }).map { $0.lowercased() })
		/// "You spent the most on Rent" names Rent: the name is the trailing
		/// words that match the tool's names, and the words before it may only
		/// be lead-in wording. "Pet Rent" is not Rent.
		func names(_ label: String) -> Bool {
			let words = label.split(separator: " ").map(String.init)
			return words.indices.contains { start in
				returned.contains(words[start...].joined(separator: " ").lowercased())
					&& words[..<start].allSatisfy { leadInWords.contains($0.lowercased()) }
			}
		}
		return labelledNames(in: answer).filter { !names($0) }.union(prose.filter { !returned.contains($0.lowercased()) })
	}

	static func grounded(_ answer: String, in sources: [String], known: [String] = []) -> Bool {
		ungrounded(answer, in: sources, known: known).isEmpty
	}

	@Test func theTotalsWordingSitsOnlyBesideTheTotal() {
		#expect(Self.mislabelsTotal("Rent: 1,800.00 This is the Expenses line of the Net Profit report.", total: "2,443.61"))
		#expect(Self.mislabelsTotal("Your total spending in September 2026 was 1,800.00. Rent: 1,800.00.", total: "2,443.61"))
		#expect(!Self.mislabelsTotal("Spending, September 2026: 2,443.61\nThis is the Expenses line of the Net Profit report.\nRent: 1,800.00", total: "2,443.61"))
		#expect(!Self.mislabelsTotal("You spent the most on Rent: 1,800.00.", total: "2,443.61"))
		#expect(!Self.leads("Spending, September 2026: 1,800.00\nRent: 1,800.00", with: "2,443.61"), "a category line quoted as the total fails the headline check")
	}

	@Test func theHeadlineNamesOnlyTheToolsDates() {
		#expect(Self.datesAgree("You spent 2,443.61 in September 2026.", with: "September 2026"))
		#expect(Self.datesAgree("You spent 2,443.61 last month.", with: "September 2026"))
		#expect(!Self.datesAgree("You spent 2,443.61 in October 2026.", with: "September 2026"))
		#expect(Self.datesAgree("You have spent 2,109.92 from October 1 to 15, 2026.", with: "October 1 to 15, 2026"))
	}

	@Test func aNameMustComeFromTheToolsOwnOutput() {
		let tool = "Spending by category, September 2026, largest first:\nRent: 1,800.00\nGroceries: 111.93\nEverything else (3 categories): 141.21"
		#expect(Self.grounded("Here is the breakdown:\n- **Rent**: 1,800.00\n- Groceries: 111.93\n- Everything else (3 categories): 141.21", in: [tool]))
		#expect(Self.grounded("Your total spending: 2,443.61. Rent (1,800.00) and Groceries (111.93).", in: [tool]), "headings and fixed words are not names")
		#expect(!Self.grounded("- Utilities: 69.99\n- Rent: 1,800.00", in: [tool]), "an invented name fails")
		#expect(Self.ungrounded("The names suggested are \"Entertainment\" and \"Rent\".", in: [tool]) == ["Entertainment"], "a quoted invented name fails")
		#expect(!Self.grounded("- Car: 20.00", in: ["Car loan: 20.00"]), "names match on word boundaries, not substrings")
		#expect(!Self.grounded("- Pet Spending: 12.00\n- Rent: 1,800.00", in: [tool]), "an invented name with a heading word in it is still checked")
		#expect(Self.grounded("Total Spending: 2,443.61\nRent: 1,800.00", in: [tool]), "a name made only of heading words is a heading")
		#expect(Self.grounded("You spent the most on Rent: 1,800.00.", in: [tool]), "the name is the part that matches, after the lead-in")
		#expect(!Self.grounded("You spent the most on Pet Insurance: 1,800.00.", in: [tool]), "an invented name after a lead-in still fails")
		#expect(!Self.grounded("- Pet Rent: 1,800.00", in: [tool]), "a word before the name that isn't lead-in wording makes it another name")
		let withGifts = tool + "\nGifts: 25.00"
		#expect(!Self.grounded("- Savings Rent: 1,800.00", in: [withGifts]), "a finance noun is not lead-in wording")
		#expect(!Self.grounded("Cash Gifts: 25.00", in: [withGifts]), "nor is cash")
		#expect(Self.grounded("You spent the most on Rent: 1,800.00.", in: [withGifts]))
		#expect(!Self.grounded("You spent the most on Gas last month.", in: [tool], known: ["Rent", "Gas", "Groceries"]), "a budget name in prose must be in the tool's output")
		#expect(Self.grounded("You spent the most on Rent last month, and your gas bill was small.", in: [tool], known: ["Rent", "Gas"]), "prose names match as the budget writes them, so lowercase prose isn't a name")
	}

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
