import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

final class TransferFakeSession: BudgetSession, @unchecked Sendable {
	let canExport = true
	let canPlanTransfers = true
	var calls: [String] = []
	var commands: [Command] = []
	var purpose = TransferPurpose.none
	var toLoan = false
	var debt = KindTarget.none
	var savings = KindTarget.none
	var reclassified: UInt64 = 0
	var defaults = LoanPaymentDefaults.filled()
	var payments: [LoanPaymentDraft] = []
	var rows: [String: RegisterRow] = [:]
	var list = AccountList(accounts: [
		AccountDetail(id: "checking", name: "Checking", offBudget: false, closed: false, groupId: nil, cleared: 214_018, uncleared: 0, total: 214_018, note: nil, lastReconciled: nil),
		AccountDetail(id: "savings", name: "Savings", offBudget: false, closed: false, groupId: nil, cleared: 0, uncleared: 0, total: 1_240_000, note: nil, lastReconciled: nil),
		AccountDetail(id: "brokerage", name: "Brokerage", offBudget: true, closed: false, groupId: nil, cleared: 0, uncleared: 0, total: 3_180_000, note: nil, lastReconciled: nil),
		AccountDetail(id: "carloan", name: "Car loan", offBudget: true, closed: false, groupId: nil, cleared: 0, uncleared: 0, total: -980_000, note: nil, lastReconciled: nil)
	], groups: [])
	let payeeList = ["checking", "savings", "brokerage", "carloan"].map {
		PayeeDetail(id: "to-\($0)", name: $0, transferAccountId: $0, favorite: false, learnCategories: false, transactions: 0)
	}
	let month = BudgetMonth(
		month: Month(year: 2026, month: 11), firstMonth: Month(year: 2026, month: 1), lastMonth: Month(year: 2026, month: 12), toBudget: 0, income: 0,
		fromLastMonth: 0, overspentLastMonth: 0, budgeted: 0, forNextMonth: 0, spent: 0, available: 0,
		groups: [
			EnvelopeGroup(id: "g", name: "Savings and Debt", isIncome: false, hidden: false, budgeted: 0, spent: 0, available: 0, envelopes: [
				TransferFakeSession.envelope("savings-a", "Savings"), TransferFakeSession.envelope("savings-b", "Investments"),
				TransferFakeSession.envelope("carloan-cat", "Car Loan"), TransferFakeSession.envelope("interest", "Loan interest")
			])
		]
	)

	static func envelope(_ id: String, _ name: String) -> EnvelopeRow {
		EnvelopeRow(id: id, name: name, hidden: false, budgeted: 0, spent: 0, available: 0, carryover: false, tone: .neutral, fill: 0, note: nil)
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
	func budgetMonth(_ month: Month, today: String) async throws -> BudgetMonth { self.month }
	func perform(_ command: Command) async throws { commands.append(command) }
	func accounts() async throws -> AccountList { list }
	func payees() async throws -> [PayeeDetail] { payeeList }
	func registerRow(_ id: String) async throws -> RegisterRow {
		guard let row = rows[id] else { throw FfiError.NotFound(entity: "transaction", id: id) }
		return row
	}

	func transferPreview(from: String, to: String, amount: Int64) async throws -> TransferPreview {
		calls.append("preview \(from)→\(to) \(amount)")
		let total = { (id: String) in self.list.accounts.first { $0.id == id }?.total ?? 0 }
		return TransferPreview(
			from: BalanceAfter(accountId: from, now: total(from), after: total(from) - amount),
			to: BalanceAfter(accountId: to, now: total(to), after: total(to) + amount),
			purpose: purpose, toLoan: toLoan
		)
	}
	func kindTarget(_ kind: CategoryKind) async throws -> KindTarget { kind == .savings ? savings : debt }
	func kindChangePreview(_ categoryID: String, kind: CategoryKind) async throws -> UInt64 { reclassified }
	func setCategoryKind(_ categoryID: String, kind: CategoryKind) async throws { calls.append("kind \(categoryID) \(kind)") }
	var groupHidden = false
	func kindCategoryPlan(_ kind: CategoryKind) async throws -> KindCategoryPlan { KindCategoryPlan(groupName: "Savings and Debt", groupIsHidden: groupHidden) }
	func createKindCategory(kind: CategoryKind, name: String, unhideGroup: Bool) async throws -> String {
		calls.append("create \(name) \(kind)\(unhideGroup ? " unhiding" : "")")
		return "made"
	}
	func loanPaymentDefaults(_ loanAccountID: String, today: String) async throws -> LoanPaymentDefaults { defaults }
	func loanPaymentPreview(_ loanAccountID: String, date: String, total: Int64, interest: Int64, escrow: Int64, fees: Int64) async throws -> LoanPaymentPreview {
		let principal = total - interest - escrow - fees
		return LoanPaymentPreview(principal: principal, outstanding: 980_000, estimate: 8_003, rateBps: 980, problem: principal < 0 ? .interestAbovePayment(shortBy: -principal) : nil)
	}
	func recordLoanPayment(_ payment: LoanPaymentDraft) async throws { payments.append(payment) }
	var loans: [String: LoanTerms] = ["carloan": LoanTerms(rateBps: 980, payment: 41_200, paymentDay: 12, lender: "Lakeshore Credit Union")]
	var created: [(NewAccount, LoanTerms?)] = []
	func loanTerms(_ accountID: String) async throws -> LoanTerms? { loans[accountID] }
	func setLoanTerms(_ accountID: String, _ terms: LoanTerms) async throws { loans[accountID] = terms }
	func createAccount(_ account: NewAccount, loan: LoanTerms?) async throws -> String {
		created.append((account, loan))
		return "new"
	}
	func evaluateRate(_ text: String, style: NumberStyle) async throws -> Int64 {
		guard let value = Decimal(string: text.replacing("%", with: "")) else { throw FfiError.Invalid(message: "rate") }
		return NSDecimalNumber(decimal: value * 100).int64Value
	}

	static func transfer(_ id: String, account: String, other: String, amount: Int64, counterpart: String?, category: String? = nil) -> RegisterRow {
		RegisterRow(
			id: id, accountId: account, accountName: account, date: "2026-11-04", amount: amount, payeeId: "to-\(other)", payeeName: other,
			categoryId: category, categoryName: nil, notes: nil, tags: [], cleared: true, reconciled: false,
			transferAccountId: other, counterpartId: counterpart, counterpartParentId: nil, lines: [], splitDifference: nil, startingBalance: false,
			scheduleId: nil, needsActual: nil, runningBalance: nil
		)
	}
}

private final class TransferFakeLibrary: BudgetLibrary, @unchecked Sendable {
	let session: TransferFakeSession
	init(session: TransferFakeSession) { self.session = session }
	func newBudgetDirectory() -> URL { URL(filePath: "/tmp/budgets/new") }
	func open(directory: URL) async throws -> any BudgetSession { session }
	func importActualZip(_ zip: URL, into directory: URL) async throws -> any BudgetSession { session }
	func createBudget(_ budget: NewBudget, into directory: URL) async throws -> any BudgetSession { session }
	func currencies() async throws -> [CurrencyOption] { FakeAmounts.currencies }
	func evaluateAmount(_ text: String, style: NumberStyle) async throws -> Int64 { try FakeAmounts.evaluate(text) }
}

@MainActor
struct TransferTests {
	let session = TransferFakeSession()

	func workspace() async -> WorkspaceModel {
		let workspace = WorkspaceModel(library: TransferFakeLibrary(session: session), format: MoneyFormat())
		await workspace.reopen(directory: URL(filePath: "/tmp/budget"))
		return workspace
	}

	func move(from: String = "checking", to: String, amount: String) async -> MoveMoneyModel {
		let model = MoveMoneyModel(workspace: await workspace(), fromID: from, reader: FakeAmounts.reader)
		await model.load()
		model.toID = to
		model.typeAmount(amount)
		await model.refreshPreview()
		return model
	}

	@Test func movingMoneyShowsBothBalancesAndWritesOnePairFromTheFromSide() async {
		// R-XFER-1
		let model = await move(to: "savings", amount: "200.00")
		#expect(model.preview?.from.after == 194_018)
		#expect(model.preview?.to.after == 1_260_000)
		#expect(!model.asksPurpose)
		#expect(await model.save())
		guard case .addTransaction(let draft) = session.commands.last else { Issue.record("no add"); return }
		#expect(draft.accountId == "checking")
		#expect(draft.amount == -20_000)
		#expect(draft.payeeId == "to-savings")
		#expect(draft.categoryId == nil)
	}

	@Test func theSameAccountOnBothEndsCantBeSaved() async {
		let model = await move(to: "checking", amount: "10.00")
		#expect(!model.canSave)
		#expect(model.preview == nil)
	}

	@Test func moneyLeavingTheBudgetWaitsForAnAnswerThenFilesUnderIt() async {
		// R-XFER-4, CLS-4.1: several savings envelopes, so one is picked.
		session.purpose = .ask
		session.savings = .several(categoryIds: ["savings-a", "savings-b"])
		let model = await move(to: "brokerage", amount: "500.00")
		#expect(model.asksPurpose)
		#expect(!model.canSave)
		model.purpose.answer = .saving
		#expect(model.purpose.choices.map(\.id) == ["savings-a", "savings-b"])
		#expect(!model.canSave)
		await model.purpose.choose("savings-b")
		#expect(model.canSave)
		#expect(await model.save())
		guard case .addTransaction(let draft) = session.commands.last else { Issue.record("no add"); return }
		#expect(draft.categoryId == "savings-b")
		#expect(!session.calls.contains { $0.hasPrefix("kind") }, "an envelope that has the kind keeps it")
	}

	@Test func oneDebtEnvelopeIsUsedWithoutAsking() async {
		session.purpose = .ask
		session.debt = .one(categoryId: "carloan-cat")
		let model = await move(to: "brokerage", amount: "50.00")
		model.purpose.answer = .debt
		#expect(model.purpose.fixedEnvelope?.name == "Car Loan")
		#expect(model.canSave)
	}

	@Test func withNoDebtEnvelopeTheChosenOneTakesTheKindAndSaysWhatChanges() async {
		// CLS-4.1 "if none exists", CLS-3.3's count.
		session.purpose = .ask
		session.reclassified = 12
		let model = await move(to: "brokerage", amount: "50.00")
		model.purpose.answer = .debt
		#expect(model.purpose.needsKind)
		await model.purpose.choose("carloan-cat")
		#expect(model.purpose.reclassifies == 12)
		#expect(await model.save())
		#expect(session.calls.last == "kind carloan-cat debtPrincipal")
		guard case .addTransaction(let draft) = session.commands.last else { Issue.record("no add"); return }
		#expect(draft.categoryId == "carloan-cat")
	}

	@Test func aNewEnvelopeIsMadeWithTheKind() async {
		session.purpose = .ask
		let model = await move(to: "brokerage", amount: "50.00")
		model.purpose.answer = .saving
		session.groupHidden = true
		await model.purpose.chooseNewEnvelope()
		#expect(model.purpose.newEnvelopeName == "Savings")
		#expect(model.purpose.unhidePrompt == "Your group \"Savings and Debt\" is hidden. Show it again and add \"Savings\" to it?")
		#expect(model.canSave)
		#expect(await model.save())
		#expect(session.calls.last == "create Savings savings unhiding")
		guard case .addTransaction(let draft) = session.commands.last else { Issue.record("no add"); return }
		#expect(draft.categoryId == "made")
	}

	@Test func aLoanPaymentFillsFromTheLastOneAndTheCoreBuildsTheSplit() async {
		// R-DEBT-2, DEBT-3, DEBT-6
		session.purpose = .ask
		session.toLoan = true
		session.defaults = LoanPaymentDefaults.filled(total: 41_200, interest: 8_020, principalCategoryId: "carloan-cat", interestCategoryId: "interest")
		let model = MoveMoneyModel(workspace: await workspace(), fromID: "checking", reader: FakeAmounts.reader)
		await model.load()
		model.toID = "carloan"
		await model.refreshPreview()
		await model.refreshPreview()
		#expect(model.isLoanPayment)
		#expect(!model.asksPurpose, "a loan gets the split, not the question")
		#expect(model.amount.text == "412.00")
		#expect(model.interest.text == "80.20")
		#expect(model.loan?.principal == 33_180)
		#expect(model.loan?.estimate == 8_003)
		#expect(await model.save())
		#expect(session.commands.isEmpty)
		let payment = session.payments.last
		#expect(payment?.total == 41_200)
		#expect(payment?.interest == 8_020)
		#expect(payment?.principalCategoryId == "carloan-cat")
		#expect(payment?.interestCategoryId == "interest")
	}

	@Test func movingMoneyFromALoansRegisterPaysTheLoan() async {
		session.defaults = LoanPaymentDefaults.filled(fromAccountId: "savings")
		let model = MoveMoneyModel(workspace: await workspace(), fromID: "carloan", reader: FakeAmounts.reader)
		await model.load()
		#expect(model.toID == "carloan")
		#expect(model.fromID == "savings")
	}

	@Test func aNewLoanKeepsItsTermsAndTheyCanBeChangedLater() async {
		// R-DEBT-1
		let accounts = AccountsModel(workspace: await workspace())
		await accounts.load()
		#expect(accounts.loans.keys.sorted() == ["carloan"])
		let terms = accounts.loanTermsEditor()
		terms.rateText = "6.25"
		terms.paymentDay = 3
		terms.lender = "  Harbor Bank "
		let typed = await terms.terms()
		#expect(typed == LoanTerms(rateBps: 625, payment: nil, paymentDay: 3, lender: "Harbor Bank"))
		#expect(await accounts.create(name: "Mortgage", kind: .mortgage, startingBalance: 31_000_000, loan: typed))
		#expect(session.created.first?.1?.rateBps == 625)

		let existing = accounts.loanTermsEditor(accounts.loans["carloan"])
		#expect(existing.rateText == "9.80")
		#expect(existing.lender == "Lakeshore Credit Union")
		existing.rateText = "abc"
		#expect(await existing.save(to: "carloan") == false)
		#expect(existing.problem == "Can't read abc as a rate.")
	}

	@Test func escrowAndFeesGoToTheCoreAndThePrincipalCanBeBudgeted() async {
		// DEBT-2's parts and DEBT-6 A.6.
		session.toLoan = true
		session.defaults = .filled(total: 200_000, interest: 100_000, escrow: 40_000, principalCategoryId: "carloan-cat", interestCategoryId: "interest", escrowCategoryId: "savings-a")
		let model = MoveMoneyModel(workspace: await workspace(), fromID: "checking", reader: FakeAmounts.reader)
		await model.load()
		model.toID = "carloan"
		await model.refreshPreview()
		await model.refreshPreview()
		#expect(model.showsEscrowAndFees, "a last payment with escrow opens the escrow row")
		model.fees.type("25.00")
		await model.refreshPreview()
		#expect(model.loan?.principal == 57_500)
		model.budgetsPrincipal = true
		#expect(await model.save())
		let payment = session.payments.last
		#expect(payment?.escrow == 40_000)
		#expect(payment?.fees == 2_500)
		#expect(payment?.escrowCategoryId == "savings-a")
		guard case .moveBudget(_, let amount, let from, let to) = session.commands.last else { Issue.record("no budget move"); return }
		#expect(amount == model.loan?.principal)
		#expect(from == nil)
		#expect(to == "carloan-cat")
	}

	@Test func interestAboveThePaymentCantBeSaved() async {
		// DEBT-2.2
		session.toLoan = true
		let model = await move(to: "carloan", amount: "50.00")
		model.interest.type("80.00")
		await model.refreshPreview()
		#expect(model.loan?.problem == .interestAbovePayment(shortBy: 3_000))
		#expect(!model.canSave)
	}

	@Test func editingTheIncomingSideEditsTheOutgoingLeg() async {
		// R-XFER-3: either side edits the pair; the core syncs the other.
		let incoming = TransferFakeSession.transfer("in", account: "checking", other: "savings", amount: 15_000, counterpart: "out")
		session.rows["out"] = TransferFakeSession.transfer("out", account: "savings", other: "checking", amount: -15_000, counterpart: "in")
		let model = MoveMoneyModel(workspace: await workspace(), fromID: "checking", editing: incoming, reader: FakeAmounts.reader)
		#expect(model.fromID == "savings")
		#expect(model.toID == "checking")
		await model.load()
		model.typeAmount("175.00")
		#expect(await model.save())
		guard case .editTransaction(let id, let draft) = session.commands.last else { Issue.record("no edit"); return }
		#expect(id == "out")
		#expect(draft.accountId == "savings")
		#expect(draft.amount == -17_500)
		#expect(draft.payeeId == "to-checking")
	}

	@Test func aOneSidedTransferIsEditedFromItsOwnSide() async {
		// Edge case 4: no counterpart to edit.
		let orphan = TransferFakeSession.transfer("in", account: "checking", other: "savings", amount: 15_000, counterpart: nil)
		let model = MoveMoneyModel(workspace: await workspace(), fromID: "checking", editing: orphan, reader: FakeAmounts.reader)
		await model.load()
		#expect(await model.save())
		guard case .editTransaction(let id, let draft) = session.commands.last else { Issue.record("no edit"); return }
		#expect(id == "in")
		#expect(draft.accountId == "checking")
		#expect(draft.amount == 15_000)
		#expect(draft.payeeId == "to-savings")
	}

	@Test func deletingTheLoansSideNamesTheWholePayment() async {
		// DEBT-6 A.5: the core deletes the split the counterpart came from.
		var loanSide = TransferFakeSession.transfer("m", account: "carloan", other: "checking", amount: 33_180, counterpart: "l1")
		loanSide.counterpartParentId = "p"
		var payment = TransferFakeSession.transfer("p", account: "checking", other: "carloan", amount: -41_200, counterpart: nil)
		payment.transferAccountId = nil
		payment.lines = [
			RegisterLine(id: "l1", amount: -33_180, payeeId: "to-carloan", payeeName: "Car loan", categoryId: "carloan-cat", categoryName: "Car Loan", notes: nil, transferAccountId: "carloan", counterpartId: "m"),
			RegisterLine(id: "l2", amount: -8_020, payeeId: "lender", payeeName: "Lender", categoryId: "interest", categoryName: "Loan interest", notes: nil, transferAccountId: nil, counterpartId: nil)
		]
		session.rows["p"] = payment
		let register = RegisterModel(workspace: await workspace(), scope: .account(accountId: "carloan"))
		let payments = await register.paymentsDeleted(with: [loanSide])
		#expect(payments.map(\.id) == ["p"])
		#expect(register.describe(payments[0]) == "checking: To Car loan 331.80, Loan interest 80.20")
	}

	@Test func editingASplitKeepsItsTransferLine() async {
		// DEBT-6: re-saving a loan payment must not turn its principal line into an expense.
		let payment = RegisterRow(
			id: "p", accountId: "checking", accountName: "Checking", date: "2026-10-12", amount: -41_200, payeeId: "lender", payeeName: "Lender",
			categoryId: nil, categoryName: nil, notes: nil, tags: [], cleared: false, reconciled: false, transferAccountId: nil, counterpartId: nil, counterpartParentId: nil,
			lines: [
				RegisterLine(id: "l1", amount: -33_180, payeeId: "to-carloan", payeeName: "Car loan", categoryId: "carloan-cat", categoryName: "Car Loan", notes: nil, transferAccountId: "carloan", counterpartId: "m"),
				RegisterLine(id: "l2", amount: -8_020, payeeId: "lender", payeeName: "Lender", categoryId: "interest", categoryName: "Loan interest", notes: nil, transferAccountId: nil, counterpartId: nil)
			],
			splitDifference: nil, startingBalance: false, scheduleId: nil, needsActual: nil, runningBalance: nil
		)
		let editor = TransactionEditorModel(workspace: await workspace(), accountID: "checking", editing: payment, reader: FakeAmounts.reader)
		#expect(editor.splits.first?.transferName == "Car loan")
		#expect(await editor.save())
		guard case .editTransaction(_, let draft) = session.commands.last else { Issue.record("no edit"); return }
		#expect(draft.splits.map(\.payeeId) == ["to-carloan", nil])
	}

	@Test func quickEntryAsksForATransferLeavingTheBudgetAndWritesTheOnBudgetSide() async {
		// R-XFER-4 from quick entry in the off-budget account: money comes in from Checking.
		session.purpose = .ask
		session.savings = .one(categoryId: "savings-a")
		let editor = TransactionEditorModel(workspace: await workspace(), accountID: "brokerage", reader: FakeAmounts.reader)
		await editor.load()
		await editor.choose(session.payeeList[0])
		editor.isDeposit = true
		editor.typeAmount("300.00")
		await editor.refreshTransfer()
		#expect(editor.transferAsks)
		#expect(session.calls.last == "preview checking→brokerage 1", "the core refuses a zero move, so the purpose is asked about one cent")
		editor.transferPurpose.answer = .saving
		#expect(await editor.save())
		guard case .addTransaction(let draft) = session.commands.last else { Issue.record("no add"); return }
		#expect(draft.accountId == "checking")
		#expect(draft.payeeId == "to-brokerage")
		#expect(draft.amount == -30_000)
		#expect(draft.categoryId == "savings-a")
	}
}
