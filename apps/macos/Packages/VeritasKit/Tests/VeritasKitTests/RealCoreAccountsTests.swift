import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// The Accounts screen's models against the real core on
/// fixtures/synthetic/all-features (R-ACC-1..5, R-XFER-1/3/4, R-DEBT-1/2).
@MainActor
struct RealCoreAccountsTests {
	static let checking = AllFeatures.checking
	static let savings = "66260e81-beca-4d3d-8ab4-00c8dc2c2ded"
	static let brokerage = "3128ef6e-0073-4914-b33a-9808cc093fa4"
	static let carLoan = "87979271-7007-41a8-a77e-7bce6f3e2da1"
	static let carLoanInterest = "78174a83-35a0-40b1-baa9-e64170eaada6"

	static func accounts(_ workspace: WorkspaceModel) async throws -> [String: AccountDetail] {
		let list = try #require(await workspace.report { try await $0.accounts() })
		return Dictionary(uniqueKeysWithValues: list.accounts.map { ($0.id, $0) })
	}

	static func allRows(_ workspace: WorkspaceModel, _ scope: RegisterScope) async throws -> [RegisterRow] {
		let filter = RegisterFilter(search: nil, status: nil, from: nil, to: nil, categoryId: nil, payeeId: nil, uncategorized: false)
		return try #require(await workspace.report {
			try await $0.register(scope, filter: filter, sort: RegisterSort(key: .date, ascending: false), offset: 0, limit: 100_000)
		}).rows
	}

	@Test func theRegisterPagesThroughEveryRowAndItsTagsCountWhatsThere() async throws {
		// R-ACC-2, R-PERF-3: pages of 250, in the core's order.
		try await RealBudget.with { budget in
			let everything = try await Self.allRows(budget.workspace, .all)
			#expect(everything.count > RegisterModel.pageSize, "the fixture spans two pages")
			let register = RegisterModel(workspace: budget.workspace, scope: .all)
			await register.load()
			#expect(register.total == everything.count)
			#expect(register.loadedRows.count == RegisterModel.pageSize)
			#expect(register.row(at: RegisterModel.pageSize) == nil, "the next page isn't loaded until it's asked for")
			for _ in 0..<400 where register.loadedRows.count < everything.count {
				try await Task.sleep(for: .milliseconds(5))
			}
			#expect(register.loadedRows.map(\.id) == everything.map(\.id), "both pages, in order, with nothing twice")
			#expect(register.index(of: everything.last!.id) == everything.count - 1)

			let tags = try #require(await budget.workspace.report { try await $0.tags() })
			#expect(!tags.isEmpty)
			for tag in tags {
				let lines = everything.flatMap(\.lines).filter { ($0.notes ?? "").split(separator: " ").contains("#\(tag.name)") }.count
				let tagged = everything.filter { $0.tags.contains(tag.name) }.count + lines
				#expect(UInt64(tagged) == tag.count, "#\(tag.name): \(tag.count) counted, \(tagged) rows carry it")
			}
		}
	}

	@Test func theBalanceChartEndsOnTheRunningBalanceOfItsLastDay() async throws {
		// The register's chart over a date filter (R-ACC-2).
		try await RealBudget.with { budget in
			let register = RegisterModel(workspace: budget.workspace, scope: .account(accountId: Self.checking))
			await register.setFilter { $0.from = "2026-08-01"; $0.to = "2026-10-31" }
			let history = try #require(register.history)
			let points = history.points
			#expect(points.first?.date == "2026-08-01" && points.last?.date == "2026-10-31")
			let rows = try await Self.allRows(budget.workspace, .account(accountId: Self.checking))
			let closing = try #require(rows.first { $0.date <= "2026-10-31" }?.runningBalance)
			#expect(points.last?.balance == closing, "the chart's last day is the register's running balance that day")
			let opening = try #require(rows.first { $0.date <= "2026-08-01" }?.runningBalance)
			#expect(points.first?.balance == opening, "the chart opens on the end of the range's first day")
			let change = try #require(history.change)
			#expect(change == Double(closing - opening) / Double(abs(opening)), "the change is (last − first) / |first|: \(change)")
		}
	}

	@Test func quickEntryRemembersThePayeeAndReconcileBalancesTheStatement() async throws {
		// R-ACC-4 (D38) and R-ACC-3.
		try await RealBudget.with { budget in
			let workspace = budget.workspace
			let editor = TransactionEditorModel(workspace: workspace, accountID: Self.checking)
			await editor.load()
			editor.typePayee("Corner")
			let grocer = try #require(editor.suggestions.first { $0.name == "Corner Grocer" })
			await editor.choose(grocer)
			let remembered = try #require(await workspace.report { try await $0.payeeMemory(grocer.id) })
			let memory = try #require(remembered)
			#expect(editor.remembered)
			#expect(editor.categoryID == memory.categoryId)
			#expect(editor.isDeposit == (memory.amount > 0))
			#expect(await editor.save())
			let rows = try await Self.allRows(workspace, .account(accountId: Self.checking))
			let saved = try #require(rows.first { $0.payeeName == "Corner Grocer" && $0.date == WorkspaceModel.today() })
			#expect(saved.amount == memory.amount, "the remembered amount, sign included")
			#expect(saved.categoryId == memory.categoryId)

			let account = try #require(try await Self.accounts(workspace)[Self.checking])
			let reconcile = ReconcileModel(workspace: workspace, accountID: Self.checking)
			let statement = account.cleared - 1_234
			await reconcile.check(statement)
			#expect(reconcile.result?.cleared == account.cleared)
			#expect(reconcile.result?.difference == -1_234)
			#expect(!reconcile.isBalanced)
			await reconcile.addAdjustment()
			#expect(reconcile.isBalanced, "the adjustment closes the gap")
			#expect(await reconcile.finish())
			let after = try await Self.allRows(workspace, .account(accountId: Self.checking))
			let unlocked = after.filter { $0.cleared && !$0.reconciled }
			#expect(unlocked.isEmpty, "finishing locks every cleared row")
			#expect(after.contains { $0.amount == -1_234 && $0.cleared && $0.reconciled })
		}
	}

	@Test func moveMoneyShowsBothBalancesBeforeAndAfterAndEditsThePairFromEitherSide() async throws {
		// R-XFER-1, R-XFER-3.
		try await RealBudget.with { budget in
			let workspace = budget.workspace
			let before = try await Self.accounts(workspace)
			let move = MoveMoneyModel(workspace: workspace, fromID: Self.checking)
			await move.load()
			move.toID = Self.savings
			move.typeAmount("100")
			await move.refreshPreview()
			let preview = try #require(move.preview)
			#expect(preview.purpose == .none, "between two budget accounts nothing is asked")
			#expect(move.showsAfter)
			#expect(preview.from.now == before[Self.checking]?.total && preview.to.now == before[Self.savings]?.total)
			#expect(preview.from.after == preview.from.now - 10_000 && preview.to.after == preview.to.now + 10_000)
			#expect(await move.save())
			var after = try await Self.accounts(workspace)
			#expect(after[Self.checking]?.total == preview.from.after)
			#expect(after[Self.savings]?.total == preview.to.after)

			let landed = try #require(try await Self.allRows(workspace, .account(accountId: Self.savings)).first { $0.transferAccountId == Self.checking && $0.amount == 10_000 })
			let count = try await Self.allRows(workspace, .all).count
			let edit = MoveMoneyModel(workspace: workspace, fromID: Self.savings, editing: landed)
			await edit.load()
			#expect(edit.fromID == Self.checking && edit.toID == Self.savings, "opened from the side it arrived on, it still moves money out of Checking")
			edit.typeAmount("125")
			await edit.refreshPreview()
			#expect(await edit.save())
			after = try await Self.accounts(workspace)
			#expect(after[Self.checking]?.total == (before[Self.checking]?.total ?? 0) - 12_500)
			#expect(after[Self.savings]?.total == (before[Self.savings]?.total ?? 0) + 12_500)
			let rows = try await Self.allRows(workspace, .all)
			#expect(rows.count == count, "the edit keeps the pair, it doesn't add one")
			let edited = try #require(rows.first { $0.id == landed.id })
			#expect(edited.amount == 12_500)
			#expect(rows.first { $0.id == edited.counterpartId }?.amount == -12_500)
		}
	}

	@Test func moneyLeavingTheBudgetAsksWhatItIsAndMakesASavingsEnvelopeInAHiddenGroupOnlyWhenTold() async throws {
		// R-XFER-4, CLS-4.1: no envelope has the savings kind, so one is made,
		// and its group is hidden, so it's shown again only with the person's yes.
		try await RealBudget.with { budget in
			let workspace = budget.workspace
			let month = try #require(Month(date: WorkspaceModel.today()))
			#expect(await workspace.perform(.createCategoryGroup(name: "Savings")))
			let group = try #require(await workspace.budgetMonth(month)?.groups.first { $0.name == "Savings" })
			#expect(await workspace.perform(.setCategoryGroupHidden(groupId: group.id, hidden: true)))
			let plan = try #require(await workspace.report { try await $0.kindCategoryPlan(.savings) })
			#expect(plan.groupName == "Savings" && plan.groupIsHidden, "the hidden Savings group is reused")

			let move = MoveMoneyModel(workspace: workspace, fromID: Self.checking)
			await move.load()
			move.toID = Self.brokerage
			move.typeAmount("250")
			await move.refreshPreview()
			#expect(move.asksPurpose, "money leaving the budget asks what it is")
			#expect(!move.canSave, "and can't be saved until it's answered")
			move.purpose.answer = .saving
			#expect(move.purpose.needsKind, "no envelope is for savings yet")
			await move.purpose.chooseNewEnvelope()
			#expect(move.purpose.newEnvelopeName == "Savings")
			let prompt = try #require(move.purpose.unhidePrompt)
			#expect(prompt.contains(plan.groupName))
			#expect(move.canSave)
			#expect(await move.save(), "saved after the person confirms")

			let target = try #require(await workspace.report { try await $0.kindTarget(.savings) })
			guard case .one(let envelope) = target else {
				Issue.record("the new envelope is the savings envelope, got \(target)")
				return
			}
			let sheet = try #require(await workspace.budgetMonth(month))
			let shown = try #require(sheet.groups.first { $0.id == group.id })
			#expect(!shown.hidden, "the group shows again")
			#expect(shown.envelopes.contains { $0.id == envelope && $0.name == "Savings" })
			let leg = try #require(try await Self.allRows(workspace, .account(accountId: Self.checking)).first { $0.transferAccountId == Self.brokerage && $0.amount == -25_000 })
			#expect(leg.categoryId == envelope, "the on-budget side is filed under it (CLS-1)")
		}
	}

	@Test func aLoanPaymentSplitsIntoPrincipalInterestEscrowAndFeesAndDeletingTheLoanSideNamesEveryLine() async throws {
		// R-DEBT-1, R-DEBT-2, DEBT-6 A.5.
		try await RealBudget.with { budget in
			let workspace = budget.workspace
			let accounts = AccountsModel(workspace: workspace)
			await accounts.load()
			let editor = accounts.loanTermsEditor(accounts.loans[Self.carLoan])
			editor.rateText = "6.5%"
			editor.paymentDay = 5
			editor.lender = "Auto Lender"
			#expect(await editor.save(to: Self.carLoan))
			let stored = try #require(await workspace.report { try await $0.loanTerms(Self.carLoan) })
			let terms = try #require(stored)
			#expect(terms.rateBps == 650 && terms.paymentDay == 5 && terms.lender == "Auto Lender")

			let debt = try #require(await workspace.report { try await $0.kindCategoryPlan(.debtPrincipal) })
			_ = debt
			let principal = try #require(await workspace.report { try await $0.createKindCategory(kind: .debtPrincipal, name: "Car principal", unhideGroup: false) })
			let before = try await Self.accounts(workspace)
			let move = MoveMoneyModel(workspace: workspace, fromID: Self.carLoan)
			await move.load()
			#expect(move.toID == Self.carLoan, "Move money opened on a loan pays it")
			#expect(move.fromID == Self.checking)
			move.typeAmount("380")
			move.interest.type("62.50")
			move.showsEscrowAndFees = true
			move.escrow.type("40")
			move.fees.type("5")
			move.principalCategoryID = principal
			move.interestCategoryID = Self.carLoanInterest
			move.escrowCategoryID = AllFeatures.household
			move.feesCategoryID = AllFeatures.household
			await move.refreshPreview()
			#expect(move.isLoanPayment)
			let preview = try #require(move.loan)
			#expect(preview.principal == 38_000 - 6_250 - 4_000 - 500, "principal is what's left of the payment")
			#expect(await move.save())

			let after = try await Self.accounts(workspace)
			#expect(after[Self.checking]?.total == (before[Self.checking]?.total ?? 0) - 38_000)
			#expect(after[Self.carLoan]?.total == (before[Self.carLoan]?.total ?? 0) + preview.principal, "only principal reduces the loan")
			let payment = try #require(try await Self.allRows(workspace, .account(accountId: Self.checking)).first { $0.amount == -38_000 && !$0.lines.isEmpty })
			#expect(payment.lines.map(\.amount).reduce(0, +) == -38_000)
			#expect(Set(payment.lines.compactMap(\.categoryId)) == [principal, Self.carLoanInterest, AllFeatures.household])
			#expect(payment.lines.count == 4, "principal, interest, escrow and fees")

			let register = RegisterModel(workspace: workspace, scope: .account(accountId: Self.carLoan))
			await register.load()
			let loanSide = try #require(register.loadedRows.first { $0.counterpartParentId == payment.id })
			let deleted = await register.paymentsDeleted(with: [loanSide])
			#expect(deleted.map(\.id) == [payment.id])
			let words = register.describe(try #require(deleted.first))
			for line in payment.lines {
				let amount = workspace.format.amount(abs(line.amount), on: .register)
				#expect(words.contains(amount), "the confirmation names \(amount): \(words)")
			}
		}
	}

	@Test func aLoanPaymentLeavesThePrincipalEnvelopesBudgetAlone() async throws {
		// DEBT-6 A.6: funding the principal waits for the core's
		// LoanPaymentDraft.fundPrincipal (#72), so the sheet doesn't offer it
		// and a payment changes no budget, even when the amount changes after
		// the last preview.
		try await RealBudget.with { budget in
			let workspace = budget.workspace
			let principal = try #require(await workspace.report { try await $0.createKindCategory(kind: .debtPrincipal, name: "Car principal", unhideGroup: false) })
			let month = try #require(Month(date: WorkspaceModel.today()))
			let before = try #require(await workspace.budgetMonth(month)).row(principal).budgeted
			let move = MoveMoneyModel(workspace: workspace, fromID: Self.carLoan)
			await move.load()
			move.typeAmount("380")
			move.interest.type("62.50")
			move.principalCategoryID = principal
			move.interestCategoryID = Self.carLoanInterest
			await move.refreshPreview()
			#expect(move.loan?.principal == 31_750)
			move.typeAmount("400")
			#expect(await move.save())
			let payment = try #require(try await Self.allRows(workspace, .account(accountId: Self.checking)).first { $0.amount == -40_000 && !$0.lines.isEmpty })
			let paid = try #require(payment.lines.first { $0.categoryId == principal }).amount
			#expect(paid == -33_750)
			let after = try #require(await workspace.budgetMonth(month)).row(principal).budgeted
			#expect(after == before, "Veritas budgets nothing for the principal until the core does it in the same command")
		}
	}

	@Test func aPaymentShortOfItsPartsIsShortByInterestEscrowAndFeesLessTheTotal() async throws {
		// DEBT-2.2: $50 against $30 interest and $40 escrow is $20 short, and can't be saved.
		try await RealBudget.with { budget in
			let move = MoveMoneyModel(workspace: budget.workspace, fromID: Self.carLoan)
			await move.load()
			move.typeAmount("50")
			move.interest.type("30")
			move.escrow.type("40")
			await move.refreshPreview()
			#expect(move.loan?.problem == .paymentTooSmall(shortBy: 2_000))
			#expect(!move.canSave)
		}
	}

	@Test func aPaymentWithNoInterestSavesWithoutAnInterestCategory() async throws {
		// A 0 interest writes no interest line, so it needs no category.
		try await RealBudget.with { budget in
			let workspace = budget.workspace
			let principal = try #require(await workspace.report { try await $0.createKindCategory(kind: .debtPrincipal, name: "Car principal", unhideGroup: false) })
			let move = MoveMoneyModel(workspace: workspace, fromID: Self.carLoan)
			await move.load()
			move.typeAmount("250")
			move.interest.type("0")
			move.principalCategoryID = principal
			move.interestCategoryID = nil
			await move.refreshPreview()
			#expect(move.canSave)
			#expect(await move.save())
			let payment = try #require(try await Self.allRows(workspace, .account(accountId: Self.checking)).first { $0.amount == -25_000 })
			#expect(payment.lines.isEmpty ? payment.categoryId == principal : payment.lines.allSatisfy { $0.categoryId == principal })
		}
	}

	@Test func blankInterestAndEscrowSaveAsAPrincipalOnlyPayment() async throws {
		// DEBT-3 can leave interest blank. A blank part is 0, not a refused field.
		try await RealBudget.with { budget in
			let workspace = budget.workspace
			let principal = try #require(await workspace.report { try await $0.createKindCategory(kind: .debtPrincipal, name: "Car principal", unhideGroup: false) })
			let move = MoveMoneyModel(workspace: workspace, fromID: Self.carLoan)
			await move.load()
			move.typeAmount("275")
			move.interest.type("")
			move.escrow.type("")
			move.principalCategoryID = principal
			await move.refreshPreview()
			#expect(move.loan?.principal == 27_500)
			#expect(await move.save(), "\(String(describing: move.problem))")
			let payment = try #require(try await Self.allRows(workspace, .account(accountId: Self.checking)).first { $0.amount == -27_500 })
			let categories = payment.lines.isEmpty ? [payment.categoryId] : payment.lines.map(\.categoryId)
			#expect(categories == [principal], "principal only: no interest or escrow line")
		}
	}

	@Test func payeesRenameMergeAndFavoriteAndTheRegisterFollows() async throws {
		// R-ACC-5.
		try await RealBudget.with { budget in
			let model = PayeesModel(workspace: budget.workspace)
			await model.load()
			let cafe = try #require(model.payees.first { $0.name == "Bean There Cafe" })
			let trattoria = try #require(model.payees.first { $0.name == "Trattoria" })
			let transfer = try #require(model.payees.first { $0.transferAccountId == Self.savings })
			let rows = try await Self.allRows(budget.workspace, .all)
			let both = rows.filter { $0.payeeId == cafe.id || $0.payeeId == trattoria.id }.count

			#expect(await model.rename(cafe.id, to: "Bean There"))
			model.selection = [cafe.id, trattoria.id, transfer.id]
			#expect(Set(model.mergeable.map(\.id)) == [cafe.id, trattoria.id], "an account's transfer payee isn't merged")
			await model.merge(into: cafe.id)
			await model.setFavorite(cafe.id, true)
			let merged = try #require(model.payees.first { $0.id == cafe.id })
			#expect(merged.name == "Bean There" && merged.favorite)
			#expect(merged.transactions == UInt64(both), "Trattoria's rows now count under it")
			#expect(!model.payees.contains { $0.id == trattoria.id })
			#expect(model.payees.contains { $0.id == transfer.id })
			let after = try await Self.allRows(budget.workspace, .all)
			#expect(after.filter { $0.payeeName == "Bean There" }.count == both, "the register reads every merged row under the new name")
			#expect(after.count == rows.count, "merging keeps every row")
		}
	}

	@Test func afterAPaymentWithNoInterestTheNextPaymentPrefillsNoInterest() async throws {
		// DEBT-3: the next payment starts from the last one's figures. The last
		// one had no interest, $40 escrow and $5 fees, so it pre-fills 0 interest
		// and $40 escrow, never escrow's figure as interest.
		try await RealBudget.with { budget in
			let workspace = budget.workspace
			let principal = try #require(await workspace.report { try await $0.createKindCategory(kind: .debtPrincipal, name: "Car principal", unhideGroup: false) })
			let move = MoveMoneyModel(workspace: workspace, fromID: Self.carLoan)
			await move.load()
			move.typeAmount("300")
			move.interest.type("0")
			move.showsEscrowAndFees = true
			move.escrow.type("40")
			move.fees.type("5")
			move.principalCategoryID = principal
			move.interestCategoryID = nil
			move.escrowCategoryID = AllFeatures.household
			move.feesCategoryID = AllFeatures.funMoney
			await move.refreshPreview()
			#expect(await move.save())
			#expect(try budget.sqlite("SELECT count(*) FROM transactions WHERE category = ''") == [["0"]], "the empty interest category v10 is sent is never written")
			let defaults = try #require(await workspace.report { try await $0.loanPaymentDefaults(Self.carLoan, today: WorkspaceModel.today()) })
			// v13 (L-5c): parts come from the recorded lines, never by position, and no interest line pre-fills none.
			#expect(defaults.total == 29_500, "the last payment less its fees: \(String(describing: defaults.total))")
			#expect(defaults.interest == nil, "interest: \(String(describing: defaults.interest))")
			#expect(defaults.escrow == 4_000, "escrow: \(String(describing: defaults.escrow))")
			#expect(defaults.interestCategoryId != AllFeatures.household, "escrow's envelope isn't the interest envelope")
		}
	}

	@Test func aReadOnlyBudgetOffersNoWrites() async throws {
		try await RealBudget.with { budget in
			let directory = try budget.directory
			await budget.workspace.closeBudget()
			try ActualDatabase.execute(directory.appending(path: "db.sqlite"), "INSERT OR REPLACE INTO preferences (id, value) VALUES ('budgetType', 'tracking')")
			await budget.workspace.reopen(directory: directory)
			let register = RegisterModel(workspace: budget.workspace, scope: .account(accountId: Self.checking))
			await register.load()
			#expect(register.isReadOnly)
			let move = MoveMoneyModel(workspace: budget.workspace, fromID: Self.checking)
			await move.load()
			move.toID = Self.savings
			move.typeAmount("10")
			#expect(!move.canSave)
			#expect(!(await move.save()))
			#expect(AccountsModel(workspace: budget.workspace).isReadOnly)
		}
	}
}
