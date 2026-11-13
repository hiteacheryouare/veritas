import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// R-BUD-1, R-BUD-3, R-BUD-4 on the window side: what the Budget screen asks
/// the core for and what it sends back.
@MainActor
struct BudgetScreenTests {
	let session = BudgetFakeSession()

	@Test func theCoresAmountFieldTextOpensAndAppends() async throws {
		// R-BUD-3: open on 412.00, type +40, Return sets 452.00.
		let field = AmountFieldModel(amount: 41_200, style: .commaDot, reader: CoreAmountReader())
		#expect(field.text == "412.00")
		field.type("+")
		#expect(field.text == "412.00+")
		field.type("412.00+40")
		#expect(await field.commit() == 45_200)
		#expect(field.problem == nil)
	}

	@Test func anUnreadableAmountKeepsTheOldValueAndSaysWhy() async {
		// D37 and design/interaction.md, arithmetic fields 9.
		let field = AmountFieldModel(amount: 41_200, style: .commaDot, reader: CoreAmountReader())
		field.type("+")
		#expect(await field.commit() == nil)
		#expect(field.problem == "Can't read 412.00+ as an amount. Fix it or press Esc.")
		#expect(field.original == 41_200)
	}

	@Test func anExpressionPreviewsWhatReturnWouldSet() async throws {
		let field = AmountFieldModel(amount: 41_200, style: .commaDot, reader: CoreAmountReader())
		field.type("+")
		field.type("412.00+40")
		var turns = 0
		while field.preview == nil, turns < 200 {
			try await Task.sleep(for: .milliseconds(5))
			turns += 1
		}
		#expect(field.preview == 45_200)
		#expect(!AmountFieldModel.isExpression("-50"))
		#expect(AmountFieldModel.isExpression("12*3"))
	}

	@Test func committingSendsSetBudgetAndMovesToTheNextEnvelope() async {
		let screen = await openScreen()
		screen.beginEditing("groceries")
		screen.editing?.type("+")
		screen.editing?.type("412.00+40")
		await screen.commitEditing(thenMove: 1)
		#expect(session.commands == [.setBudget(month: BudgetFakeSession.month, categoryId: "groceries", amount: 45_200)])
		#expect(screen.editingID == "dining")
	}

	@Test func anUnchangedAmountSendsNothing() async {
		let screen = await openScreen()
		screen.beginEditing("groceries")
		await screen.commitEditing()
		#expect(session.commands.isEmpty)
		#expect(screen.editing == nil)
	}

	@Test func incomeEnvelopesAreNotBudgetedHere() async {
		let screen = await openScreen()
		screen.beginEditing("salary")
		#expect(screen.editing == nil)
	}

	@Test func arrowsWalkEnvelopesAndOptionArrowsJumpGroups() async {
		let screen = await openScreen()
		screen.moveSelection(by: 1)
		#expect(screen.selection == "groceries")
		screen.moveSelection(by: 1)
		#expect(screen.selection == "dining")
		screen.moveSelectionToGroup(by: 1)
		#expect(screen.selection == "rent")
		screen.moveSelectionToGroup(by: -1)
		#expect(screen.selection == "groceries")
	}

	@Test func monthsStayInsideTheBudget() async {
		let screen = await openScreen()
		#expect(screen.canGoBack)
		#expect(!screen.canGoForward)
		#expect(BudgetScreenModel.shift(Month(year: 2026, month: 1), by: -1) == Month(year: 2025, month: 12))
		#expect(BudgetScreenModel.shift(Month(year: 2026, month: 12), by: 1) == Month(year: 2027, month: 1))
	}

	@Test func coveringOverspendingAndMovingBudgetSendTheirCommands() async {
		// R-BUD-4
		let screen = await openScreen()
		#expect(await screen.coverOverspending("dining", from: "groceries"))
		#expect(await screen.moveBudget(amount: 500, from: nil, to: "rent"))
		#expect(session.commands == [
			.coverOverspending(month: BudgetFakeSession.month, categoryId: "dining", fromCategoryId: "groceries", amount: nil),
			.moveBudget(month: BudgetFakeSession.month, amount: 500, fromCategoryId: nil, toCategoryId: "rent")
		])
		#expect(screen.sources.map(\.id) == ["groceries"])
	}

	@Test func hiddenEnvelopesShowOnlyWhenAsked() async {
		session.hideRent = true
		let screen = await openScreen()
		#expect(!screen.envelopeIDs.contains("rent"))
		#expect(screen.hasHiddenEnvelopes)
		screen.workspace.showsHiddenEnvelopes = true
		#expect(screen.envelopeIDs.contains("rent"))
	}

	@Test func goalsLoadWithTheMonth() async {
		session.goals = [
			Self.goal("laptop", state: .active, available: 112_000, target: 150_000),
			Self.goal("tires", state: .closed, available: 0, target: 85_000)
		]
		let screen = await openScreen()
		#expect(screen.goals.map(\.id) == ["laptop"], "closed goals stay off the overview")
		#expect(abs(screen.goals[0].progress - 0.7467) < 0.001)
		#expect(screen.goals[0].targetMonth == Month(year: 2027, month: 3))
	}

	@Test func goalCardsRoundToWholeUnits() {
		let format = MoneyFormat()
		#expect(format.wholeCurrency(112_000) == "$1,120")
		#expect(format.wholeCurrency(41_050) == "$411")
		#expect(format.wholeCurrency(-41_049) == "\u{2212}$410")
	}

	@Test func deletingAnEnvelopeWithHistoryArchivesItInstead() async {
		// R-BUD-5
		let screen = await openScreen()
		#expect(await screen.removal(for: "rent") == .delete)
		session.history = CategoryHistory(transactions: 12, budgetedMonths: 3)
		#expect(await screen.removal(for: "rent") == .archive(transactions: 12, budgetedMonths: 3))
	}

	@Test func categoryAndGroupEditsSendTheirCommands() async {
		let screen = await openScreen()
		#expect(await screen.createEnvelope(in: "housing", named: "Water"))
		#expect(await screen.renameEnvelope("rent", to: "Mortgage"))
		await screen.moveEnvelope("rent", to: "everyday", before: "dining")
		await screen.moveEnvelope("rent", to: "everyday", before: "rent")
		await screen.archive("dining")
		#expect(await screen.delete("dining", movingHistoryTo: "groceries"))
		await screen.moveGroup("housing", before: nil)
		await screen.setGroupHidden("housing", true)
		#expect(session.commands == [
			.createCategory(groupId: "housing", name: "Water"),
			.renameCategory(categoryId: "rent", name: "Mortgage"),
			.moveCategory(categoryId: "rent", groupId: "everyday", beforeCategoryId: "dining"),
			.archiveCategory(categoryId: "dining"),
			.deleteCategoryMovingHistory(categoryId: "dining", toCategoryId: "groceries"),
			.moveCategoryGroup(groupId: "housing", beforeGroupId: nil),
			.setCategoryGroupHidden(groupId: "housing", hidden: true)
		])
	}

	@Test func goalsSplitIntoOpenAndArchiveAndOnlyOpenOnesRetarget() async {
		session.goals = [
			Self.goal("laptop", state: .active, available: 112_000, target: 150_000),
			Self.goal("tires", state: .reachedSpent, available: 2_000, target: 85_000),
			Self.goal("vet", state: .closed, available: 0, target: 120_000)
		]
		let workspace = WorkspaceModel(library: BudgetFakeLibrary(session: session), format: MoneyFormat())
		await workspace.reopen(directory: URL(filePath: "/tmp/budget"))
		let goals = GoalsScreenModel(workspace: workspace, reader: CoreAmountReader(budgetDecimalPlaces: 2))
		await goals.load()
		#expect(goals.open.map(\.id) == ["laptop", "tires"])
		#expect(goals.archive.map(\.id) == ["vet"])
		#expect(!(await goals.retarget(goals.archive[0], target: 1, date: nil)), "a closed goal is reopened first")
		#expect(!session.goals[1].canRetarget, "a spent goal is reopened or closed, not re-targeted")
		#expect(await goals.close(goals.open[1], releaseTo: "groceries", alsoMoveLater: false))
		#expect(session.commands == [.closeGoal(goalId: "tires", releaseToCategoryId: "groceries", alsoMoveLater: false)])
	}

	@Test func budgetCellsUseTheCurrencysPlaces() async throws {
		// D63: a 0-place currency shows and reads whole units.
		let field = AmountFieldModel(amount: 5_000, style: .commaDot, reader: CoreAmountReader(budgetDecimalPlaces: 0))
		#expect(field.text == "5,000")
		#expect(await field.commit() == 5_000)
	}

	static func goal(_ id: String, state: GoalState, available: Int64, target: Int64) -> GoalSummary {
		GoalSummary(
			id: id, categoryId: id, name: id.capitalized, target: target, targetDate: "2027-03-01", icon: nil, color: nil,
			state: state, available: available, spent: 0, suggestedMonthly: state == .active ? 7_600 : nil, monthsLeft: 5,
			reachedMonth: nil, closedMonth: state == .closed ? Month(year: 2026, month: 8) : nil, laterBudget: [], newActivity: false
		)
	}

	@Test func todayIsTheLocalCivilDate() {
		var calendar = Calendar(identifier: .gregorian)
		calendar.timeZone = TimeZone(identifier: "Pacific/Auckland")!
		// 2026-11-15 11:00 UTC is already the 16th in Auckland.
		#expect(WorkspaceModel.today(Date(timeIntervalSince1970: 1_794_740_400), calendar: calendar) == "2026-11-16")
	}

	private func openScreen() async -> BudgetScreenModel {
		let workspace = WorkspaceModel(library: BudgetFakeLibrary(session: session), format: MoneyFormat())
		await workspace.reopen(directory: URL(filePath: "/tmp/budget"))
		let screen = BudgetScreenModel(workspace: workspace, reader: CoreAmountReader(), month: BudgetFakeSession.month)
		await screen.load()
		return screen
	}
}

final class BudgetFakeSession: BudgetSession, @unchecked Sendable {
	static let month = Month(year: 2026, month: 11)
	let canExport = true
	var commands: [Command] = []
	var hideRent = false
	var goals: [GoalSummary] = []
	var history = CategoryHistory(transactions: 0, budgetedMonths: 0)

	func goals(today: String) async throws -> [GoalSummary] {
		goals
	}

	func categoryHistory(_ categoryID: String) async throws -> CategoryHistory {
		history
	}

	func overview() async throws -> BudgetOverview {
		BudgetOverview(name: "Household", directory: URL(filePath: "/tmp/budget"), readOnly: nil, accountGroups: [])
	}

	func undo() async throws {}
	func redo() async throws {}
	func maintain() async throws {}
	func exportActualZip(to destination: URL) async throws -> [AuditFinding] { [] }
	func observeChanges(_ changed: @escaping @Sendable () -> Void) async throws {}
	func close() async {}

	func perform(_ command: Command) async throws {
		commands.append(command)
	}

	func budgetMonth(_ month: Month, today: String) async throws -> BudgetMonth {
		let row = { (id: String, budgeted: Int64, spent: Int64, available: Int64, tone: VeritasCore.MoneyTone) in
			EnvelopeRow(id: id, name: id.capitalized, hidden: false, budgeted: budgeted, spent: spent, available: available, carryover: false, tone: tone, fill: 0.5, note: nil)
		}
		return BudgetMonth(
			month: Self.month, firstMonth: Month(year: 2026, month: 1), lastMonth: Self.month,
			toBudget: 31_240, income: 400_000, fromLastMonth: 0, overspentLastMonth: 0, budgeted: 0, forNextMonth: 0, spent: 0, available: 0,
			groups: [
				EnvelopeGroup(id: "everyday", name: "Everyday", isIncome: false, hidden: false, budgeted: 0, spent: 0, available: 0, envelopes: [
					row("groceries", 41_200, -31_862, 9_338, .available),
					row("dining", 18_000, -21_475, -3_475, .overspent)
				]),
				EnvelopeGroup(id: "housing", name: "Housing", isIncome: false, hidden: false, budgeted: 0, spent: 0, available: 0, envelopes: [
					EnvelopeRow(id: "rent", name: "Rent", hidden: hideRent, budgeted: 165_000, spent: -165_000, available: 0, carryover: false, tone: .neutral, fill: 0, note: nil)
				]),
				EnvelopeGroup(id: "income", name: "Income", isIncome: true, hidden: false, budgeted: 0, spent: 0, available: 0, envelopes: [
					row("salary", 0, 400_000, 0, .income)
				])
			]
		)
	}
}

final class BudgetFakeLibrary: BudgetLibrary, @unchecked Sendable {
	let session: BudgetFakeSession

	init(session: BudgetFakeSession) {
		self.session = session
	}

	func newBudgetDirectory() -> URL { URL(filePath: "/tmp/budgets/new") }
	func open(directory: URL) async throws -> any BudgetSession { session }
	func importActualZip(_ zip: URL, into directory: URL) async throws -> any BudgetSession { session }
	func createBudget(_ budget: NewBudget, into directory: URL) async throws -> any BudgetSession { session }
	func currencies() async throws -> [CurrencyOption] { FakeAmounts.currencies }
	func evaluateAmount(_ text: String, style: NumberStyle) async throws -> Int64 { try FakeAmounts.evaluate(text) }
}
