import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// The Reports screens' models against the real core on
/// fixtures/synthetic/all-features (R-REP-2, R-REP-4, R-REP-5), checked
/// against the core's own calls and against a fresh reopen.
@MainActor
struct RealCoreReportTests {
	static let october = Month(year: 2026, month: 10)
	static let spendingByCategory = "c7eaad37-4a0f-4f44-8c5e-ca30ff232dd2"
	static let mainPage = "48f79e8a-0a29-4f3c-a7a3-9c04110766ad"
	static let deepDive = "76608b8e-9055-432d-a3cf-0f3f1623c5a7"
	static let futureWidget = "b595baa8-f97d-4f1a-ac8f-d7c2a1d7a0b3"

	@Test func everyNetProfitNumberOpensExactlyTheRowsBehindIt() async throws {
		// R-REP-2: "drilldown everywhere". Every cell, every expanded category,
		// every month and the total, over a year of the fixture.
		try await RealBudget.with { budget in
			let model = NetProfitModel(workspace: budget.workspace, thisMonth: Self.october)
			await model.setCustom(first: Month(year: 2025, month: 11), last: Self.october)
			for line in [NetProfitLine.income, .expenses, .debtPaydown, .savings] { model.toggle(line) }
			let rows = model.rows
			#expect(rows.contains { $0.style == .category }, "the lines expand into categories")
			var checked = 0
			var lines = Set<NetProfitLine>()
			for row in rows {
				for cell in row.cells {
					let drill = DrilldownModel(workspace: budget.workspace, title: cell.title, drill: cell.drill)
					await drill.load()
					let found = try #require(drill.transactions, "\(cell.title)")
					let sum = found.reduce(Int64(0)) { $0 + $1.amount }
					#expect(UInt64(found.count) == cell.count, "\(cell.title): \(cell.count) counted, \(found.count) listed")
					let shown = row.line == .expenses || row.line == .unreviewed ? -sum : sum
					#expect(shown == cell.amount, "\(cell.title): \(cell.amount) shown, rows add up to \(shown)")
					checked += 1
					if cell.amount != 0 { lines.insert(row.line) }
				}
			}
			#expect(checked > 100)
			#expect(lines.isSuperset(of: [.income, .expenses, .netProfit, .netCashFlow]), "\(lines)")
		}
	}

	@Test func anEditedAndANewCustomReportSurviveAReopenAsPreviewed() async throws {
		// R-REP-4: edits saved back in Actual's format, filters included.
		try await RealBudget.with { budget in
			let model = CustomReportsModel(workspace: budget.workspace, thisMonth: Self.october)
			await model.load()
			await model.show(.custom(id: Self.spendingByCategory))
			await model.edit()
			var draft = try #require(model.draft)
			draft.settings.name = "Food, without refunds"
			var condition = ReportCondition(field: "notes", op: "doesNotContain", value: "", options: nil)
			condition.text = "refund"
			draft.conditions.append(condition)
			model.draft = draft
			model.setFixedRange(first: Month(year: 2026, month: 5), last: Self.october)
			await model.refreshPreview()
			let previewed = try #require(model.preview)
			let edited = try #require(model.draft).settings
			#expect(await model.save())
			#expect(model.view == previewed, "Save shows what the preview showed")

			await model.startNew()
			var fresh = try #require(model.draft)
			fresh.settings.name = "Big payees"
			fresh.settings.groupBy = .payee
			var big = ReportCondition(field: "amount", op: "lt", value: "", options: nil)
			big.text = "-10000"
			fresh.conditions = [big]
			model.draft = fresh
			await model.refreshPreview()
			withKnownIssue("Known bug: an unsaved report has no id, the core's previewCustomReport answers NotFound for it, and the editor drops that quietly, so a new report shows no preview until it's saved") {
				#expect(model.preview != nil)
			}
			#expect(model.preview != nil || model.previewProblem != nil, "the editor shows a preview or the core's error, never nothing")
			#expect(await model.save())
			let freshSaved = try #require(model.view)
			#expect(freshSaved.data != nil && freshSaved.problem == nil, "saved, the new report computes")
			guard case .custom(let newID) = model.choice else {
				Issue.record("the new report is shown once saved")
				return
			}

			let directory = try budget.directory
			await budget.workspace.closeBudget()
			await budget.workspace.reopen(directory: directory)
			let again = CustomReportsModel(workspace: budget.workspace, thisMonth: Self.october)
			await again.load()
			#expect(again.items.contains { $0.id == newID && $0.name == "Big payees" })
			await again.show(.custom(id: Self.spendingByCategory))
			#expect(again.view == previewed, "the edited report reads back as previewed")
			await again.edit()
			#expect(again.draft?.settings == edited, "every setting and filter came back")
			again.cancelEditing()
			await again.show(.custom(id: newID))
			#expect(again.view == freshSaved, "the new report reads back as saved, filter included")
			#expect(again.view?.settings.conditions == fresh.conditions)
		}
	}

	@Test func dashboardEditsPersistAndLeaveEveryOtherWidgetAsItWas() async throws {
		// R-REP-4, D73: add, configure, move, resize and remove widgets, and
		// create, rename and delete a page, through the screen's model.
		try await RealBudget.with { budget in
			let untouched = "SELECT id, type, x, y, width, height, meta, dashboard_page_id FROM dashboard WHERE tombstone = 0 AND id NOT IN ('\(Self.futureWidget)') ORDER BY id"
			let before = try budget.sqlite(untouched)
			let unknownMeta = try budget.sqlite("SELECT meta FROM dashboard WHERE id = '\(Self.futureWidget)'")
			let model = DashboardModel(workspace: budget.workspace)
			await model.load()
			#expect(model.sheets.map(\.name) == ["Main", "Deep dive"])

			model.pageID = Self.mainPage
			var notes = WidgetConfig.empty
			notes.content = "## Rent day\n\nThe 2nd, every month."
			await model.add(.markdown, config: notes)
			var dining = WidgetConfig.empty
			dining.name = "Dining this year"
			dining.summaryKind = .sum
			dining.timeFrame = WidgetTimeFrame(mode: .fixed, start: Month(year: 2026, month: 1), end: Self.october)
			dining.conditions = [ReportCondition(field: "category", op: "is", value: "\"\(AllFeatures.diningOut)\"", options: nil)]
			await model.add(.summary, config: dining)
			let added = try #require(model.sheet?.cards.first { $0.config.name == "Dining this year" })
			let bottom = try #require(model.sheet?.cards.filter { $0.id != added.id }.map { $0.y + $0.height }.max())
			#expect((added.x, added.y, added.width, added.height) == (0, bottom, 4, 2), "under everything, at Actual's default size")
			guard case .summary(.money(let total)) = added.content else {
				Issue.record("a dining total, got \(added.content)")
				return
			}
			#expect(total < 0, "spending sums negative, as Actual's card does")

			var renamed = added.config
			renamed.name = "Dining, per month"
			renamed.summaryKind = .avgPerMonth
			await model.configure(added, renamed)
			let configured = try #require(model.sheet?.cards.first { $0.id == added.id })
			#expect(configured.config.name == "Dining, per month")
			#expect(configured.config.summaryKind == .avgPerMonth)
			#expect(configured.config.conditions == dining.conditions, "the filter rode along")

			await model.move(configured, columns: 3, rows: 1)
			var card = try #require(model.sheet?.cards.first { $0.id == added.id })
			#expect((card.x, card.y) == (3, bottom + 1))
			await model.resize(card, columns: 2)
			card = try #require(model.sheet?.cards.first { $0.id == added.id })
			#expect((card.x, card.y, card.width, card.height) == (3, bottom + 1, 6, 2))
			for (leading, trailing, bottomEdge) in [(0, 20, 1), (-20, 0, 0), (9, 0, 0), (0, -9, -9)] as [(Int64, Int64, Int64)] {
				let before = card
				await model.resize(card, leading: leading, top: 0, trailing: trailing, bottom: bottomEdge)
				card = try #require(model.sheet?.cards.first { $0.id == added.id })
				if leading == 0, before.x + card.minimumSize.width <= DashboardModel.columns {
					#expect(card.x == before.x, "a trailing resize clamps the width and leaves the card where it is")
				}
				let minimum = card.minimumSize
				#expect(card.x >= 0 && card.x + card.width <= DashboardModel.columns, "inside the grid: \(card.x)+\(card.width)")
				#expect(card.width >= minimum.width && card.height >= minimum.height, "never under its minimum: \(card.width)x\(card.height)")
			}
			let stored = try budget.sqlite("SELECT x, y, width, height FROM dashboard WHERE id = '\(added.id)'")
			#expect(stored == [["\(card.x)", "\(card.y)", "\(card.width)", "\(card.height)"]], "the frame the screen shows is the stored one")

			let markdown = try #require(model.sheet?.cards.first { $0.kind == .markdown && $0.config.content == notes.content })
			await model.remove(markdown)
			#expect(!(model.sheet?.cards.contains { $0.id == markdown.id } ?? true))

			model.pageID = Self.deepDive
			let unknown = try #require(model.sheet?.cards.first { $0.id == Self.futureWidget })
			#expect(unknown.kind == nil && unknown.content == .unknown)
			await model.move(unknown, columns: 4)
			let moved = try #require(model.sheet?.cards.first { $0.id == Self.futureWidget })
			await model.resize(moved, columns: 1, rows: 1)

			await model.createDashboard(named: "  Trips  ")
			#expect(model.sheet?.name == "Trips", "a new page opens, its name trimmed")
			await model.add(.netWorth)
			await model.renameDashboard(to: "Travel")
			#expect(model.sheets.map(\.name) == ["Main", "Deep dive", "Travel"])
			await model.createDashboard(named: "Scratch")
			await model.deleteDashboard()
			#expect(model.sheets.map(\.name) == ["Main", "Deep dive", "Travel"])
			let edited = model.sheets

			let directory = try budget.directory
			await budget.workspace.closeBudget()
			await budget.workspace.reopen(directory: directory)
			let again = DashboardModel(workspace: budget.workspace)
			await again.load()
			#expect(again.sheets == edited, "every edit came back after a reopen")
			let future = try #require(again.sheets.flatMap(\.cards).first { $0.id == Self.futureWidget })
			#expect((future.x, future.y, future.width, future.height) == (4, 6, 5, 3))
			#expect(try budget.sqlite("SELECT meta FROM dashboard WHERE id = '\(Self.futureWidget)'") == unknownMeta, "an unknown widget's settings are never touched")
			#expect(try budget.sqlite(untouched).filter { row in before.contains { $0[0] == row[0] } } == before, "every other widget is byte-identical")
		}
	}

	@Test func standardReportsHideNothingNonZeroAndDrillToTheirRows() async throws {
		// R-REP-5 over a year of the fixture.
		try await RealBudget.with { budget in
			let model = StandardReportModel(workspace: budget.workspace, kind: .spendingByCategory, thisMonth: Self.october)
			await model.setCustom(first: Month(year: 2025, month: 11), last: Self.october)
			let lines = model.spending
			#expect(!lines.isEmpty)
			#expect(lines.reduce(Int64(0)) { $0 + $1.amount } == model.spendingTotal, "the categories add up to Expenses")
			for line in lines {
				let drill = model.drilldown(line)
				await drill.load()
				let rows = try #require(drill.transactions)
				#expect(UInt64(rows.count) == line.count, "\(line.title)")
				#expect(-rows.reduce(Int64(0)) { $0 + $1.amount } == line.amount, "\(line.title)")
			}

			await model.show(.cashFlow)
			let rows = model.cashFlowRows
			let columns = model.cashFlow
			#expect(!columns.isEmpty)
			let optional: [(String, (CashFlowColumn) -> Int64)] = [
				("Unreviewed transfers", { $0.unreviewedTransfers.total }),
				("Transfers in transit between budget accounts", { $0.inTransit.total }),
				("Starting balances in budget accounts", { $0.startingBalances.total }),
				("Reconciliation adjustments in budget accounts", { $0.reconciliationAdjustments.total })
			]
			for (title, value) in optional where columns.contains(where: { value($0) != 0 }) {
				#expect(rows.contains { $0.title == title }, "\(title) isn't zero, so it's shown")
			}
			let opening = try #require(rows.first { $0.title == "Budget accounts balance" && !$0.isTotal })
			let closing = try #require(rows.last { $0.title == "Budget accounts balance" && $0.isTotal })
			for index in 1..<columns.count {
				#expect(opening.values[index] == closing.values[index - 1], "each month opens where the last closed")
			}

			await model.show(.netWorth)
			let worth = model.netWorthRows
			let assets = try #require(worth.first { $0.id == "assets" }).values
			let liabilities = try #require(worth.first { $0.id == "liabilities" }).values
			let net = try #require(worth.first { $0.id == "netWorth" }).values
			#expect(assets.count == 12)
			#expect(net == model.netWorth.map(\.netWorth), "the core's net worth, not a Swift sum of the other rows")
			_ = (assets, liabilities)
		}
	}

	@Test func everySankeyNodeYouCanClickOpensTheRowsItWasDrawnFrom() async throws {
		// R-REP-6: click a node to filter transactions.
		try await RealBudget.with { budget in
			let model = SankeyModel(workspace: budget.workspace, thisMonth: Self.october)
			await model.setCustom(first: Month(year: 2026, month: 5), last: Self.october)
			let report = try #require(model.report)
			var opened = 0
			for node in report.nodes {
				guard let drill = model.drilldown(node) else {
					#expect(node.key.kind == .shortfall || node.count == 0, "only Shortfall and empty nodes don't open")
					continue
				}
				await drill.load()
				let rows = try #require(drill.transactions)
				#expect(UInt64(rows.count) == node.count, "\(node.name): \(node.count) counted, \(rows.count) listed")
				if node.key.kind != .unallocated {
					#expect(abs(rows.reduce(Int64(0)) { $0 + $1.amount }) == node.value, "\(node.name)")
				}
				opened += 1
			}
			#expect(opened > 3)
		}
	}

	@Test func cashFlowCardsCarryNetProfitsClassifiedFiguresForTheirRange() async throws {
		// Contract v7's interim: until the widget follow-up, the cash flow
		// card shows Net Profit's classified income, expenses and Net Profit.
		try await RealBudget.with { budget in
			let model = DashboardModel(workspace: budget.workspace)
			await model.load()
			model.pageID = Self.mainPage
			let card = try #require(model.sheet?.cards.first { $0.kind == .cashFlow })
			var config = card.config
			config.timeFrame = WidgetTimeFrame(mode: .fixed, start: Month(year: 2026, month: 5), end: Self.october)
			await model.configure(card, config)
			let shown = try #require(model.sheet?.cards.first { $0.id == card.id })
			guard case .cashFlow(let income, let expenses, let netProfit, let classified) = shown.content else {
				Issue.record("a cash flow card, got \(shown.content)")
				return
			}
			#expect(classified)
			let range = try #require(shown.range)
			#expect((range.first, range.last) == (Month(year: 2026, month: 5), Self.october))
			let report = try #require(await budget.workspace.report { try await $0.netProfit(first: range.first, last: range.last) })
			#expect(income == report.total.income.total)
			#expect(expenses == report.total.expenses.total)
			#expect(netProfit == report.total.netProfit.total)
			#expect(income != 0 && expenses != 0)
		}
	}
}
