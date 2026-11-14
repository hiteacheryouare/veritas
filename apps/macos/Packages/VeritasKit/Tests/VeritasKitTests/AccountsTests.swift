import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

final class AccountsFakeSession: BudgetSession, @unchecked Sendable {
	let canExport = true
	var commands: [Command] = []
	var pages: [UInt64] = []
	var rows: [RegisterRow] = []
	var memory: PayeeMemory?
	var payeeList: [PayeeDetail] = []
	var list = AccountList(accounts: [
		AccountDetail(id: "checking", name: "Checking", offBudget: false, closed: false, groupId: nil, cleared: 100, uncleared: 0, total: 100, note: nil, lastReconciled: nil),
		AccountDetail(id: "savings", name: "Savings", offBudget: false, closed: false, groupId: nil, cleared: 0, uncleared: 0, total: 0, note: nil, lastReconciled: nil),
		AccountDetail(id: "visa", name: "Visa", offBudget: false, closed: false, groupId: nil, cleared: 0, uncleared: 0, total: 0, note: nil, lastReconciled: nil),
		AccountDetail(id: "brokerage", name: "Brokerage", offBudget: true, closed: false, groupId: nil, cleared: 0, uncleared: 0, total: 0, note: nil, lastReconciled: nil)
	], groups: [])

	func overview() async throws -> BudgetOverview {
		BudgetOverview(name: "Household", directory: URL(filePath: "/tmp/budget"), readOnly: nil, accountGroups: [])
	}
	func undo() async throws {}
	func redo() async throws {}
	func maintain() async throws {}
	func exportActualZip(to destination: URL) async throws -> [AuditFinding] { [] }
	func observeChanges(_ changed: @escaping @Sendable () -> Void) async throws {}
	func close() async {}
	func perform(_ command: Command) async throws { commands.append(command) }
	func accounts() async throws -> AccountList { list }
	func register(_ scope: RegisterScope, filter: RegisterFilter, sort: RegisterSort, offset: UInt64, limit: UInt64) async throws -> RegisterPage {
		pages.append(offset)
		return RegisterPage(total: UInt64(rows.count), rows: Array(rows.dropFirst(Int(offset)).prefix(Int(limit))))
	}
	func payees() async throws -> [PayeeDetail] { payeeList }
	func payeeMemory(_ payeeID: String) async throws -> PayeeMemory? { memory }
	func reconciliation(_ accountID: String, statementBalance: Int64) async throws -> Reconciliation {
		Reconciliation(cleared: 100, difference: statementBalance - 100)
	}

	static func row(_ id: String, amount: Int64, payee: String? = nil, transfer: String? = nil, lines: [RegisterLine] = [], balance: Int64? = nil) -> RegisterRow {
		RegisterRow(
			id: id, accountId: "checking", accountName: "Checking", date: "2026-11-09", amount: amount, payeeId: nil, payeeName: payee,
			categoryId: nil, categoryName: lines.isEmpty ? "Groceries" : nil, notes: nil, tags: [], cleared: false, reconciled: false,
			transferAccountId: transfer, counterpartId: nil, lines: lines, splitDifference: nil, startingBalance: false,
			scheduleId: nil, needsActual: nil, runningBalance: balance
		)
	}
}

final class AccountsFakeLibrary: BudgetLibrary, @unchecked Sendable {
	let session: AccountsFakeSession
	init(session: AccountsFakeSession) { self.session = session }
	func newBudgetDirectory() -> URL { URL(filePath: "/tmp/budgets/new") }
	func open(directory: URL) async throws -> any BudgetSession { session }
	func importActualZip(_ zip: URL, into directory: URL) async throws -> any BudgetSession { session }
	func createBudget(_ budget: NewBudget, into directory: URL) async throws -> any BudgetSession { session }
	func currencies() async throws -> [CurrencyOption] { FakeAmounts.currencies }
	func evaluateAmount(_ text: String, style: NumberStyle) async throws -> Int64 { try FakeAmounts.evaluate(text) }
}

@MainActor
struct AccountsTests {
	let session = AccountsFakeSession()

	func workspace() async -> WorkspaceModel {
		let workspace = WorkspaceModel(library: AccountsFakeLibrary(session: session), format: MoneyFormat())
		await workspace.reopen(directory: URL(filePath: "/tmp/budget"))
		return workspace
	}

	@Test func transfersReadAsToAndFromNeverAsAPayee() {
		// R-XFER-2
		let out = PayeeText(AccountsFakeSession.row("a", amount: -20_000, payee: "Savings", transfer: "savings"))
		#expect(out.direction == "→")
		#expect(out.text == "To Savings")
		let back = PayeeText(AccountsFakeSession.row("b", amount: 15_000, payee: "Savings", transfer: "savings"))
		#expect(back.text == "From Savings")
		#expect(PayeeText(AccountsFakeSession.row("c", amount: -100, payee: "Fernwood Market")).direction == nil)
		let split = AccountsFakeSession.row("d", amount: -41_200, lines: [
			RegisterLine(id: "1", amount: -33_180, payeeId: nil, payeeName: nil, categoryId: "p", categoryName: "Principal", notes: nil, transferAccountId: nil, counterpartId: nil),
			RegisterLine(id: "2", amount: -8_020, payeeId: nil, payeeName: nil, categoryId: "i", categoryName: "Interest", notes: nil, transferAccountId: nil, counterpartId: nil)
		])
		#expect(split.envelopeText == "Split, 2 lines")
	}

	@Test func theRegisterFetchesOnlyThePagesAskedFor() async {
		session.rows = (0..<600).map { AccountsFakeSession.row("t\($0)", amount: -100, balance: Int64($0)) }
		let register = RegisterModel(workspace: await workspace(), scope: .account(accountId: "checking"))
		await register.load()
		#expect(register.total == 600)
		#expect(session.pages == [0])
		#expect(register.row(at: 10)?.id == "t10")
		#expect(register.row(at: 300) == nil, "a later page loads on demand")
		await waitUntil { register.row(at: 300) != nil }
		#expect(session.pages == [0, 250])
		#expect(register.showsBalance)
		await register.setFilter { $0.search = "fern" }
		#expect(!register.showsBalance, "a filtered register has no running balance")
	}

	@Test func duplicatingPicksTheCopysIdSoItCanBeSelected() async {
		let register = RegisterModel(workspace: await workspace(), scope: .account(accountId: "checking"))
		await register.duplicate("t1")
		guard case .duplicateTransaction(let original, let copy) = session.commands.first else { Issue.record("no duplicate"); return }
		#expect(original == "t1")
		#expect(register.selection == [copy ?? ""])
	}

	@Test func aKnownPayeeFillsOnlyWhatsStillEmpty() async {
		// R-ACC-4, D38
		session.payeeList = [PayeeDetail(id: "fern", name: "Fernwood Market", transferAccountId: nil, favorite: true, learnCategories: true, transactions: 40)]
		session.memory = PayeeMemory(categoryId: "groceries", amount: -8_214, date: "2026-11-09")
		let editor = TransactionEditorModel(workspace: await workspace(), accountID: "checking", reader: FakeAmounts.reader)
		await editor.load()
		editor.typePayee("fern")
		#expect(editor.suggestions.map(\.id) == ["fern"])
		await editor.choose(editor.suggestions[0])
		#expect(editor.categoryID == "groceries")
		#expect(editor.amount.text == "82.14")
		#expect(!editor.isDeposit)

		let typed = TransactionEditorModel(workspace: await workspace(), accountID: "checking", reader: FakeAmounts.reader)
		await typed.load()
		typed.categoryID = "household"
		typed.typeAmount("36.40")
		await typed.choose(session.payeeList[0])
		#expect(typed.categoryID == "household", "what was chosen stays")
		#expect(typed.amount.text == "36.40")
	}

	@Test func savingANewPayeeSendsItsNameAndAPickedID() async {
		let editor = TransactionEditorModel(workspace: await workspace(), accountID: "checking", reader: FakeAmounts.reader)
		await editor.load()
		editor.typePayee("Lakeside Pharmacy")
		editor.typeAmount("36.40")
		editor.categoryID = "household"
		#expect(await editor.save())
		guard case .addTransaction(let draft) = session.commands.last else { Issue.record("no add"); return }
		#expect(draft.payeeId == nil)
		#expect(draft.payeeName == "Lakeside Pharmacy")
		#expect(draft.amount == -3_640)
		#expect(draft.categoryId == "household")
		#expect(draft.id != nil, "the app picks the new row's id")
	}

	@Test func aSplitSendsItsLinesWithTheSameSign() async {
		let editor = TransactionEditorModel(workspace: await workspace(), accountID: "checking", reader: FakeAmounts.reader)
		await editor.load()
		editor.typeAmount("100")
		editor.addSplitLine()
		editor.splits[0].amount.type("60")
		editor.splits[1].amount.type("40")
		#expect(await editor.save())
		guard case .addTransaction(let draft) = session.commands.last else { Issue.record("no add"); return }
		#expect(draft.splits.map(\.amount) == [-6_000, -4_000])
		#expect(draft.categoryId == nil, "a split's envelopes are on its lines")
	}

	@Test func accountsMoveWithinTheirListAndCloseIntoADestination() async {
		let accounts = AccountsModel(workspace: await workspace())
		await accounts.load()
		#expect(accounts.onBudget.map(\.id) == ["checking", "savings", "visa"])
		#expect(!accounts.canMove("checking", by: -1))
		await accounts.move("checking", by: 1)
		#expect(session.commands.last == .moveAccount(accountId: "checking", beforeAccountId: "visa"))
		await accounts.move("visa", by: -1)
		#expect(session.commands.last == .moveAccount(accountId: "visa", beforeAccountId: "savings"))
		#expect(accounts.closingNeedsCategory("checking", into: "brokerage"))
		#expect(!accounts.closingNeedsCategory("checking", into: "savings"))
	}

	@Test func mergingLeavesTransferPayeesAndTheKeptOneOut() async {
		session.payeeList = [
			PayeeDetail(id: "a", name: "Fernwood", transferAccountId: nil, favorite: false, learnCategories: true, transactions: 3),
			PayeeDetail(id: "b", name: "Fernwood Market", transferAccountId: nil, favorite: false, learnCategories: true, transactions: 9),
			PayeeDetail(id: "s", name: "Savings", transferAccountId: "savings", favorite: false, learnCategories: false, transactions: 4)
		]
		let payees = PayeesModel(workspace: await workspace())
		await payees.load()
		payees.selection = ["a", "b", "s"]
		#expect(payees.mergeable.map(\.id) == ["a", "b"])
		await payees.merge(into: "b")
		#expect(session.commands == [.mergePayees(targetPayeeId: "b", payeeIds: ["a"])])
	}

	@Test func reconcilingFinishesOnlyWhenTheDifferenceIsZero() async {
		let reconcile = ReconcileModel(workspace: await workspace(), accountID: "checking")
		await reconcile.check(150)
		#expect(reconcile.result?.difference == 50)
		#expect(!reconcile.isBalanced)
		await reconcile.check(100)
		#expect(reconcile.isBalanced)
		#expect(await reconcile.finish())
		#expect(session.commands == [.finishReconcile(accountId: "checking", statementBalance: 100)])
	}

	private func waitUntil(_ condition: @MainActor () -> Bool) async {
		for _ in 0..<100 where !condition() {
			try? await Task.sleep(for: .milliseconds(10))
		}
	}
}

@MainActor
struct SplitStartTests {
	@Test func splittingStartsWithTheWholeAmountOnTheFirstLine() async {
		let session = AccountsFakeSession()
		let workspace = WorkspaceModel(library: AccountsFakeLibrary(session: session), format: MoneyFormat())
		await workspace.reopen(directory: URL(filePath: "/tmp/budget"))
		let editor = TransactionEditorModel(workspace: workspace, accountID: "checking", reader: FakeAmounts.reader)
		editor.typeAmount("412.00")
		editor.categoryID = "principal"
		editor.addSplitLine()
		#expect(editor.splits.count == 2)
		#expect(editor.splits[0].amount.text == "412.00")
		#expect(editor.splits[0].categoryID == "principal")
	}
}
