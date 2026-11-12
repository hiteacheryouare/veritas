import Foundation
import Testing
import VeritasCore
import VeritasKit

/// R-IO-0, D53, D56.
@MainActor
struct NewBudgetSetupTests {
	let library = SetupLibrary()

	@Test func theCurrencyDefaultsToThePreferredOneAndSetsItsNumberFormat() async {
		let setup = NewBudgetSetup(library: library, preferredCurrencyCode: "EUR")
		await setup.loadCurrencies()
		#expect(setup.currencyCode == "EUR")
		#expect(setup.numberStyle == .dotComma)
		setup.choose(currency: "USD")
		#expect(setup.numberStyle == .commaDot)
		setup.numberStyle = .spaceComma
		#expect(setup.numberStyle == .spaceComma)
	}

	@Test func everyAccountTypeBelongsToOneSidebarGroup() {
		#expect(NewAccountKind.allCases.map(\.group) == [.cash, .cash, .cash, .credit, .credit, .investments, .investments, .investments, .loans, .loans])
		#expect(NewAccountKind.allCases.filter(\.isOwed) == [.creditCard, .lineOfCredit, .loan, .mortgage])
		#expect(NewAccountKind.checking == NewBudgetSetup.AccountDraft().type)
	}

	@Test func anUnknownPreferredCurrencyFallsBackToTheFirst() async {
		let setup = NewBudgetSetup(library: library, preferredCurrencyCode: "XYZ")
		await setup.loadCurrencies()
		#expect(setup.currencyCode == "USD")
	}

	@Test func aNameIsNeededBeforeAccounts() async {
		let setup = NewBudgetSetup(library: library, preferredCurrencyCode: "USD")
		await setup.loadCurrencies()
		#expect(!setup.canContinue)
		setup.name = "   "
		#expect(!setup.canContinue)
		setup.name = "Household"
		_ = await setup.advance()
		#expect(setup.step == .accounts)
	}

	@Test func walkingEveryStepProducesTheBudgetToCreate() async throws {
		let setup = NewBudgetSetup(library: library, preferredCurrencyCode: "USD")
		await setup.loadCurrencies()
		setup.name = " Household "
		_ = await setup.advance()
		setup.accounts[0].name = "Checking"
		setup.accounts[0].balance = "2,140.18"
		setup.addAccount()
		setup.accounts[1].name = "Brokerage"
		setup.accounts[1].balance = "24310.55"
		setup.accounts[1].type = .investment
		setup.addAccount()
		_ = await setup.advance()
		#expect(setup.step == .paycheckSavings)
		setup.hasRetirementPlan = true
		let budget = try #require(await setup.advance())
		#expect(budget == NewBudget(
			name: "Household",
			currencyCode: "USD",
			numberStyle: .commaDot,
			accounts: [
				StartingAccount(name: "Checking", type: .checking, balance: 214_018),
				StartingAccount(name: "Brokerage", type: .investment, balance: 2_431_055)
			],
			hasRetirementPlan: true,
			hasHealthSavingsAccount: false
		))
	}

	@Test func anAccountWithABalanceButNoNameIsCaughtOnItsRow() async {
		let setup = await atAccounts()
		setup.accounts[0].balance = "100"
		_ = await setup.advance()
		#expect(setup.step == .accounts)
		#expect(setup.accounts[0].problem == "Name this account.")
	}

	@Test func aBalanceTheCoreCantReadShowsTheCoresMessageOnItsRow() async {
		let setup = await atAccounts()
		setup.accounts[0].name = "Checking"
		setup.accounts[0].balance = "12..5"
		_ = await setup.advance()
		#expect(setup.step == .accounts)
		#expect(setup.accounts[0].problem == "Can't read 12..5 as an amount.")
		setup.accounts[0].balance = "12.5"
		_ = await setup.advance()
		#expect(setup.step == .paycheckSavings)
		#expect(setup.accounts[0].problem == nil)
	}

	@Test func noAccountsIsAllowed() async throws {
		let setup = await atAccounts()
		_ = await setup.advance()
		let budget = try #require(await setup.advance())
		#expect(budget.accounts.isEmpty)
	}

	@Test func onlyACardOrLineOfCreditBringsUpTheToBudgetNote() async {
		let setup = await atAccounts()
		#expect(!setup.hasOnBudgetDebt)
		setup.accounts[0].type = .loan
		#expect(!setup.hasOnBudgetDebt)
		setup.accounts[0].type = .creditCard
		#expect(setup.hasOnBudgetDebt)
		setup.accounts[0].type = .lineOfCredit
		#expect(setup.hasOnBudgetDebt)
	}

	@Test func backReturnsToTheStepBefore() async {
		let setup = await atAccounts()
		setup.back()
		#expect(setup.step == .basics)
		#expect(setup.name == "Household")
	}

	@Test func finishingOpensTheNewBudgetInTheWindow() async throws {
		let model = WorkspaceModel(library: library, format: MoneyFormat())
		model.startNewBudget()
		let setup = try #require(model.newBudgetSetup)
		await setup.loadCurrencies()
		setup.name = "Household"
		_ = await setup.advance()
		_ = await setup.advance()
		let budget = try #require(await setup.advance())
		await model.finishNewBudget(budget)
		#expect(model.newBudgetSetup == nil)
		#expect(model.phase == .open)
		#expect(library.created == [budget])
	}

	@Test func aFailedCreateSaysNothingWasSavedAndRetriesIntoTheSameFolder() async throws {
		library.createErrors = [.Storage(message: "disk I/O error")]
		let model = WorkspaceModel(library: library, format: MoneyFormat())
		let budget = NewBudget(name: "Household", currencyCode: "USD", numberStyle: .commaDot, accounts: [], hasRetirementPlan: false, hasHealthSavingsAccount: false)
		await model.createBudget(budget)
		#expect(model.phase == .empty)
		#expect(model.alert?.title == "Couldn't create the budget")
		await model.retryFailedAction()
		#expect(model.phase == .open)
		#expect(Set(library.createDirectories).count == 1)
	}

	private func atAccounts() async -> NewBudgetSetup {
		let setup = NewBudgetSetup(library: library, preferredCurrencyCode: "USD")
		await setup.loadCurrencies()
		setup.name = "Household"
		_ = await setup.advance()
		return setup
	}
}

final class SetupLibrary: BudgetLibrary, @unchecked Sendable {
	var created: [NewBudget] = []
	var createDirectories: [URL] = []
	var createErrors: [FfiError] = []

	func newBudgetDirectory() -> URL {
		URL(filePath: "/tmp/budgets/\(UUID().uuidString)", directoryHint: .isDirectory)
	}

	func open(directory: URL) async throws -> any BudgetSession {
		throw FfiError.NotFound(entity: "budget", id: directory.lastPathComponent)
	}

	func importActualZip(_ zip: URL, into directory: URL) async throws -> any BudgetSession {
		throw FfiError.NotAnActualBudget(message: "Not used here.")
	}

	func createBudget(_ budget: NewBudget, into directory: URL) async throws -> any BudgetSession {
		createDirectories.append(directory)
		if !createErrors.isEmpty { throw createErrors.removeFirst() }
		created.append(budget)
		return SetupSession(name: budget.name)
	}

	func currencies() async throws -> [CurrencyOption] {
		FakeAmounts.currencies
	}

	func evaluateAmount(_ text: String, style: NumberStyle) async throws -> Int64 {
		try FakeAmounts.evaluate(text)
	}
}

private final class SetupSession: BudgetSession {
	let name: String
	let canExport = false

	init(name: String) {
		self.name = name
	}

	func overview() async throws -> BudgetOverview {
		BudgetOverview(name: name, directory: URL(filePath: "/tmp/budget"), readOnly: nil, accountGroups: [])
	}

	func undo() async throws {}
	func redo() async throws {}
	func maintain() async throws {}
	func exportActualZip(to destination: URL) async throws -> [AuditFinding] { [] }
	func observeChanges(_ changed: @escaping @Sendable () -> Void) async throws {}
	func close() async {}
}

struct SidebarGroupTitleTests {
	@Test func groupTitlesAreRyansWording() {
		#expect(VeritasCore.AccountGroup.allCases.map(\.title) == ["Cash", "Credit", "Investments & Other assets", "Loans"])
	}
}
