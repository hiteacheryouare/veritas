import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// Each assistant tool against the real core, without the language model:
/// the figures it hands the model are the strings the matching screen shows.
@MainActor
struct AssistantToolTests {
	static let may = Month(year: 2026, month: 5)
	static let october = Month(year: 2026, month: 10)
	static let spendingByCategory = "c7eaad37-4a0f-4f44-8c5e-ca30ff232dd2"

	/// `(612.33)`: a figure in a statement's negative style.
	static func hasStatementParentheses(_ text: String) -> Bool {
		text.contains(/\(\D?[0-9][0-9,.\u{202F}']*\)/)
	}

	private func context(_ budget: RealBudget) throws -> AssistantContext {
		AssistantContext(queries: try #require(budget.workspace.readOnlyQueries), format: budget.workspace.format, today: WorkspaceModel.today())
	}

	@Test func netProfitHasTheNetProfitScreensFigures() async throws {
		try await RealBudget.with { budget in
			let model = NetProfitModel(workspace: budget.workspace, thisMonth: Self.october)
			await model.setCustom(first: Self.may, last: Self.october)
			let format = model.format
			let tool = NetProfitTool(context: try context(budget))
			for period in ["May to October 2026", "2026-05-01 to 2026-10-31"] {
				let text = try await tool.call(arguments: .init(period: period))
				#expect(!Self.hasStatementParentheses(text), "\(text)")
				let netProfit = try #require(model.rows.first { $0.line == .netProfit }?.cells.last)
				#expect(text.hasPrefix("Net Profit, May to October 2026: \(format.currency(netProfit.amount, on: .register))\n"), "\(text)")
				for row in model.rows where row.line != .netProfit {
					let total = try #require(row.cells.last)
					#expect(text.contains("\n\(row.title), May to October 2026: \(format.currency(total.amount, on: .register))"), "\(row.title)")
				}
			}
		}
	}

	@Test func budgetMonthHasTheBudgetScreensFigures() async throws {
		try await RealBudget.with { budget in
			let screen = await budget.screen(Self.october)
			let sheet = try #require(screen.sheet)
			let format = screen.format
			let text = try await BudgetMonthTool(context: context(budget)).call(arguments: .init(period: "2026-10"))
			#expect(!Self.hasStatementParentheses(text), "\(text)")
			#expect(text.contains("To Budget: \(format.currency(sheet.toBudget, on: .budget))\n"))
			let groups = screen.groups.filter { !$0.isIncome }
			#expect(!groups.isEmpty)
			for group in groups {
				#expect(text.contains("Group \(group.name): budgeted \(format.currency(group.budgeted, on: .budget)), activity \(format.currency(-group.spent, on: .budget)), available \(format.currency(group.available, on: .budget))"))
				for envelope in group.envelopes {
					#expect(text.contains("\n\(envelope.name): budgeted \(format.currency(envelope.budgeted, on: .budget)), activity \(format.currency(-envelope.spent, on: .budget)), available \(format.currency(envelope.available, on: .budget))"))
				}
			}
			for envelope in screen.groups.filter(\.isIncome).flatMap(\.envelopes) {
				#expect(text.contains("\n\(envelope.name): received \(format.currency(envelope.spent, on: .budget))"))
			}
			let hidden = sheet.groups.flatMap(\.envelopes).filter(\.hidden)
			for envelope in hidden where !screen.groups.flatMap(\.envelopes).contains(where: { $0.name == envelope.name }) {
				#expect(!text.contains("\n\(envelope.name):"), "hidden \(envelope.name) stays out, as on screen")
			}
		}
	}

	@Test func accountsHaveTheSidebarsBalances() async throws {
		try await RealBudget.with { budget in
			let format = budget.workspace.format
			let accounts = try #require(budget.workspace.overview?.accounts)
			#expect(!accounts.isEmpty)
			let text = try await AccountsTool(context: context(budget)).call(arguments: .init())
			#expect(!Self.hasStatementParentheses(text), "\(text)")
			for account in accounts {
				#expect(text.contains("\n\(account.name): \(format.amount(account.balance, on: .register))"), "\(account.name)")
			}
		}
	}

	@Test func balanceSheetHasTheStatementsLines() async throws {
		try await RealBudget.with { budget in
			let model = StatementsModel(workspace: budget.workspace)
			await model.setAsOf("2026-10-31")
			let document = try #require(model.document)
			let format = model.format
			let text = try await BalanceSheetTool(context: context(budget)).call(arguments: .init(date: "2026-10-31"))
			#expect(!Self.hasStatementParentheses(text), "\(text)")
			#expect(text.hasPrefix("Balance sheet as of October 31, 2026\n"))
			let figures = document.lines.compactMap { line in line.amount.map { "\(line.title): \(format.amount($0, on: .register))" } }
			#expect(figures.count > 3)
			for figure in figures {
				#expect(text.contains("\n\(figure)"), "\(figure)")
			}
			let liabilities = try #require(document.lines.first { $0.id == "liabilities.total" }?.amount)
			#expect(liabilities > 0, "the fixture has a liability to show")
			#expect(text.contains("\nTotal liabilities: \(format.amount(liabilities, on: .register))\n"))
			#expect(!format.amount(liabilities, on: .register).hasPrefix(MoneyFormat.minus))
		}
	}

	@Test func cashFlowHasTheStatementsLines() async throws {
		try await RealBudget.with { budget in
			let model = StatementsModel(workspace: budget.workspace)
			await model.show(.cashFlow)
			await model.setCustom(first: Self.october, last: Self.october)
			let document = try #require(model.document)
			let format = model.format
			let text = try await CashFlowTool(context: context(budget)).call(arguments: .init(period: "October 2026"))
			#expect(!Self.hasStatementParentheses(text), "\(text)")
			#expect(text.hasPrefix("Cash flow statement, October 2026\n"))
			for line in document.lines {
				let figure = line.amount.map { "\(line.title): \(format.amount($0, on: .register))" } ?? line.title
				#expect(text.contains("\n\(figure)"), "\(figure)")
			}
		}
	}

	@Test func goalsHaveTheGoalCardsFigures() async throws {
		try await RealBudget.with { budget in
			let goals = await budget.goalsScreen()
			#expect(await goals.create(name: "New laptop", target: 150_000, date: nil, icon: nil, color: nil))
			#expect(await goals.create(name: "Bike", target: 80_000, date: Calendar.current.date(byAdding: .month, value: 5, to: .now), icon: nil, color: nil))
			#expect(goals.goals.count == 2)
			let format = goals.format
			let text = try await GoalsTool(context: context(budget)).call(arguments: .init())
			#expect(text.hasPrefix("Goals\n"), "no scale note in a 2-place budget")
			#expect(!Self.hasStatementParentheses(text), "\(text)")
			for goal in goals.goals {
				#expect(text.contains("\n\(goal.name): \(format.wholeCurrency(goal.progressAmount, on: .budget)) of \(format.wholeCurrency(goal.target, on: .budget)) (\(goal.progressPercent)%)"), "\(goal.name)")
			}
		}
	}

	@Test func taxSummaryHasTheTaxScreensBuckets() async throws {
		try await RealBudget.with { budget in
			let directory = try budget.directory
			await budget.workspace.closeBudget()
			try ActualDatabase.execute(directory.appending(path: "db.sqlite"), """
				INSERT INTO veritas_category_meta (category_id, tax_bucket) VALUES ('\(AllFeatures.interest)', 'interest_income'), ('\(AllFeatures.gifts)', 'charitable')
					ON CONFLICT (category_id) DO UPDATE SET tax_bucket = excluded.tax_bucket;
				""")
			await budget.workspace.reopen(directory: directory)
			try #require(budget.workspace.phase == .open)
			let model = TaxSummaryModel(workspace: budget.workspace, thisYear: 2026)
			await model.load()
			#expect(!model.buckets.isEmpty)
			let text = try await TaxSummaryTool(context: context(budget)).call(arguments: .init(year: 2026))
			#expect(!Self.hasStatementParentheses(text), "\(text)")
			for bucket in model.buckets {
				#expect(text.contains("\n\(TaxBuckets.name(bucket.bucket)): \(model.format.amount(bucket.total, on: .register)) from \(bucket.count) \(bucket.count == 1 ? "transaction" : "transactions")"))
			}
		}
	}

	@Test func aSavedReportHasTheCustomReportsTable() async throws {
		try await RealBudget.with { budget in
			let model = CustomReportsModel(workspace: budget.workspace, thisMonth: Self.october)
			await model.load()
			await model.show(.custom(id: Self.spendingByCategory))
			let data = try #require(model.view?.data)
			let name = try #require(model.items.first { $0.id == Self.spendingByCategory }?.name)
			let format = model.format
			let tool = SavedReportTool(context: try context(budget))
			let text = try await tool.call(arguments: .init(name: name.uppercased()))
			#expect(!Self.hasStatementParentheses(text), "\(text)")
			#expect(text.contains("\nTotal: \(format.amount(data.total, on: .register))"))
			for group in data.groups {
				#expect(text.contains("\n\(group.name): \(format.amount(group.amount, on: .register))"))
			}
			let unknown = try await tool.call(arguments: .init(name: "No such report"))
			#expect(unknown.contains("Saved reports: ") && unknown.contains(name))
		}
	}
}
