import Foundation
import Testing
import VeritasCore
import VeritasKit

/// The first-launch tour (D79).
@MainActor
struct GuidedTourTests {
	static let everything = TourBudget(hasToBudget: true, hasBudgetCell: true, accountID: "checking", isReadOnly: false)

	private func scratchDefaults() -> (UserDefaults, String) {
		let suite = "veritas-tour-tests-\(UUID().uuidString)"
		return (UserDefaults(suiteName: suite)!, suite)
	}

	@Test func walksTheBudgetGridThenAccountsInOrder() {
		let (defaults, suite) = scratchDefaults()
		defer { defaults.removePersistentDomain(forName: suite) }
		let tour = GuidedTour(defaults: defaults)
		#expect(tour.offer(Self.everything))

		var seen: [(TourStep?, Destination?)] = []
		while let step = tour.current {
			seen.append((step, tour.destination))
			tour.next()
		}
		#expect(seen.map(\.0) == [.toBudget, .budgetCell, .openAccount, .addTransaction, .quickEntry])
		#expect(seen.map(\.1) == [.screen(.budget), .screen(.budget), .screen(.accounts), .account(id: "checking"), .account(id: "checking")])
	}

	@Test func backReturnsAStepAndStopsAtTheFirst() {
		let (defaults, suite) = scratchDefaults()
		defer { defaults.removePersistentDomain(forName: suite) }
		let tour = GuidedTour(defaults: defaults)
		tour.offer(Self.everything)
		#expect(!tour.canGoBack)
		tour.back()
		#expect(tour.current == .toBudget)
		tour.next()
		tour.next()
		#expect(tour.current == .openAccount)
		#expect(tour.number == 3)
		tour.back()
		#expect(tour.current == .budgetCell)
		#expect(tour.canGoBack)
	}

	@Test func finishingOnTheLastStepEndsItForGood() {
		let (defaults, suite) = scratchDefaults()
		defer { defaults.removePersistentDomain(forName: suite) }
		let tour = GuidedTour(defaults: defaults)
		tour.offer(Self.everything)
		for _ in 0 ..< 4 { tour.next() }
		#expect(tour.isLastStep)
		#expect(TourWords.next(isLastStep: tour.isLastStep) == "Done")
		tour.next()
		#expect(tour.current == nil)
		#expect(defaults.bool(forKey: GuidedTour.finishedKey))
		#expect(!tour.offer(Self.everything), "the next budget to open doesn't start it again")
	}

	@Test func skippingEndsItForGoodInEveryWindow() {
		let (defaults, suite) = scratchDefaults()
		defer { defaults.removePersistentDomain(forName: suite) }
		let tour = GuidedTour(defaults: defaults)
		tour.offer(Self.everything)
		tour.next()
		tour.skip()
		#expect(tour.current == nil)
		#expect(tour.isFinished)
		let otherWindow = GuidedTour(defaults: defaults)
		#expect(!otherWindow.offer(Self.everything))
		#expect(otherWindow.current == nil)
	}

	@Test func showTourReplaysItFromTheStartAfterItWasSkipped() {
		let (defaults, suite) = scratchDefaults()
		defer { defaults.removePersistentDomain(forName: suite) }
		let tour = GuidedTour(defaults: defaults)
		tour.offer(Self.everything)
		tour.skip()
		#expect(tour.replay(Self.everything))
		#expect(tour.current == .toBudget)
		#expect(tour.number == 1)
		tour.next()
		tour.next()
		#expect(tour.replay(Self.everything), "Show Tour mid-tour starts over")
		#expect(tour.current == .toBudget)
	}

	@Test func aBudgetOpeningWhileTheTourRunsDoesNotRestartIt() {
		let (defaults, suite) = scratchDefaults()
		defer { defaults.removePersistentDomain(forName: suite) }
		let tour = GuidedTour(defaults: defaults)
		tour.offer(Self.everything)
		tour.next()
		#expect(!tour.offer(Self.everything))
		#expect(tour.current == .budgetCell)
	}

	@Test func closingTheBudgetStopsItWithoutFinishing() {
		let (defaults, suite) = scratchDefaults()
		defer { defaults.removePersistentDomain(forName: suite) }
		let tour = GuidedTour(defaults: defaults)
		tour.offer(Self.everything)
		tour.stop()
		#expect(tour.current == nil)
		#expect(!tour.isFinished)
		#expect(tour.offer(Self.everything), "it is offered again when a budget next opens")
	}

	@Test func openingAnAccountWhileAskedMovesOnToThatAccount() {
		let (defaults, suite) = scratchDefaults()
		defer { defaults.removePersistentDomain(forName: suite) }
		let tour = GuidedTour(defaults: defaults)
		tour.offer(Self.everything)
		tour.follow(.account(id: "visa"))
		#expect(tour.current == .toBudget, "only the open-an-account step follows")
		tour.next()
		tour.next()
		tour.follow(.screen(.bills))
		#expect(tour.current == .openAccount)
		tour.follow(.account(id: "visa"))
		#expect(tour.current == .addTransaction)
		#expect(tour.destination == .account(id: "visa"))
	}

	@Test func stepsWithNothingToPointAtAreLeftOut() {
		let noAccounts = TourBudget(hasToBudget: true, hasBudgetCell: true, accountID: nil, isReadOnly: false)
		#expect(GuidedTour.steps(for: noAccounts) == [.toBudget, .budgetCell])
		let noEnvelopes = TourBudget(hasToBudget: true, hasBudgetCell: false, accountID: "checking", isReadOnly: false)
		#expect(GuidedTour.steps(for: noEnvelopes) == [.toBudget, .openAccount, .addTransaction, .quickEntry])
	}

	@Test func aReadOnlyBudgetLeavesOutEveryStepThatEdits() {
		let readOnly = TourBudget(hasToBudget: true, hasBudgetCell: true, accountID: "checking", isReadOnly: true)
		#expect(GuidedTour.steps(for: readOnly) == [.toBudget, .openAccount])
		let tracking = TourBudget(hasToBudget: false, hasBudgetCell: true, accountID: "checking", isReadOnly: true)
		#expect(GuidedTour.steps(for: tracking) == [.openAccount])
		let nothingToRead = TourBudget(hasToBudget: false, hasBudgetCell: true, accountID: nil, isReadOnly: true)
		#expect(GuidedTour.steps(for: nothingToRead).isEmpty)
	}

	@Test func aReadOnlyTourCountsOnlyItsStepsAndEndsAfterOpeningAnAccount() {
		let (defaults, suite) = scratchDefaults()
		defer { defaults.removePersistentDomain(forName: suite) }
		let tour = GuidedTour(defaults: defaults)
		#expect(tour.offer(TourBudget(hasToBudget: true, hasBudgetCell: true, accountID: "checking", isReadOnly: true)))
		#expect(TourWords.position(tour.number ?? 0, of: tour.steps.count) == "1 of 2")
		tour.next()
		#expect(tour.current == .openAccount)
		#expect(tour.isLastStep)
		tour.follow(.account(id: "visa"))
		#expect(tour.current == nil)
		#expect(tour.isFinished)
	}

	@Test func anEmptyBudgetCountsOnlyItsOwnSteps() {
		let (defaults, suite) = scratchDefaults()
		defer { defaults.removePersistentDomain(forName: suite) }
		let tour = GuidedTour(defaults: defaults)
		#expect(tour.offer(TourBudget(hasToBudget: true, hasBudgetCell: false, accountID: nil, isReadOnly: false)))
		#expect(tour.steps == [.toBudget])
		#expect(tour.isLastStep)
		#expect(TourWords.position(tour.number ?? 0, of: tour.steps.count) == "1 of 1")
	}

	@Test func aBudgetWithNothingToPointAtShowsNoTourAndKeepsItForLater() {
		let (defaults, suite) = scratchDefaults()
		defer { defaults.removePersistentDomain(forName: suite) }
		let tour = GuidedTour(defaults: defaults)
		#expect(!tour.offer(TourBudget(hasToBudget: false, hasBudgetCell: false, accountID: nil, isReadOnly: false)))
		#expect(tour.current == nil)
		#expect(!tour.isFinished)
	}

	@Test func readsWhatTheOpenBudgetHas() async {
		let full = WorkspaceModel(library: TourFakeLibrary(session: TourFakeSession()), format: MoneyFormat())
		await full.reopen(directory: URL(filePath: "/tmp/budget"))
		#expect(await full.tourBudget() == TourBudget(hasToBudget: true, hasBudgetCell: true, accountID: "checking", isReadOnly: false))

		let empty = WorkspaceModel(library: TourFakeLibrary(session: TourFakeSession(hasEnvelopes: false, hasAccounts: false)), format: MoneyFormat())
		await empty.reopen(directory: URL(filePath: "/tmp/budget"))
		#expect(await empty.tourBudget() == TourBudget(hasToBudget: true, hasBudgetCell: false, accountID: nil, isReadOnly: false))

		let tracking = WorkspaceModel(library: TourFakeLibrary(session: TourFakeSession(readOnly: .trackingBudget)), format: MoneyFormat())
		await tracking.reopen(directory: URL(filePath: "/tmp/budget"))
		#expect(await tracking.tourBudget() == TourBudget(hasToBudget: false, hasBudgetCell: true, accountID: "checking", isReadOnly: true))

		let newerActual = WorkspaceModel(library: TourFakeLibrary(session: TourFakeSession(readOnly: .newerActualVersion)), format: MoneyFormat())
		await newerActual.reopen(directory: URL(filePath: "/tmp/budget"))
		#expect(GuidedTour.steps(for: await newerActual.tourBudget()) == [.toBudget, .openAccount])
	}

	@Test func everyStepSaysSomethingShortWithItsPosition() {
		for step in TourStep.allCases {
			let text = TourWords.text(step)
			let sentences = text.split(separator: ". ").count
			#expect((1 ... 3).contains(sentences), "\(step): \(text)")
			#expect(TourWords.spoken(step, number: 2, of: 5) == "Tour, step 2 of 5. \(text)")
		}
	}

	/// Fred's wording (docs/finance): To Budget counts budget accounts only and can go negative,
	/// and in a Budgeted cell Return moves on just as Tab does.
	@Test func theBudgetStepsSayWhatToBudgetAndTheCellReallyDo() {
		#expect(TourWords.text(.toBudget) == "To Budget is the money in your budget accounts that isn't in an envelope yet. Give it to envelopes until it reads zero. If it goes negative, you've given out more than you have.")
		#expect(TourWords.text(.budgetCell) == "Click a Budgeted amount and type what this envelope gets this month. Return or Tab saves it and moves to the next envelope.")
	}
}

/// A budget whose envelopes, accounts and kind each test picks.
final class TourFakeSession: BudgetSession, @unchecked Sendable {
	let hasEnvelopes: Bool
	let hasAccounts: Bool
	let readOnly: ReadOnlyKind?
	let canExport = false

	init(hasEnvelopes: Bool = true, hasAccounts: Bool = true, readOnly: ReadOnlyKind? = nil) {
		self.hasEnvelopes = hasEnvelopes
		self.hasAccounts = hasAccounts
		self.readOnly = readOnly
	}

	func overview() async throws -> BudgetOverview {
		let accounts = [
			SidebarAccount(id: "old", name: "Old card", balance: 0, isInactive: true),
			SidebarAccount(id: "checking", name: "Checking", balance: 0)
		]
		return BudgetOverview(
			name: "Household", directory: URL(filePath: "/tmp/budget"), readOnly: readOnly,
			accountGroups: hasAccounts ? [SidebarGroup(name: "Cash", accounts: accounts)] : []
		)
	}

	func budgetMonth(_ month: Month, today: String) async throws -> BudgetMonth {
		let groceries = EnvelopeRow(id: "groceries", name: "Groceries", hidden: false, budgeted: 0, spent: 0, available: 0, carryover: false, tone: .neutral, fill: 0, note: nil)
		let salary = EnvelopeRow(id: "salary", name: "Salary", hidden: false, budgeted: 0, spent: 0, available: 0, carryover: false, tone: .income, fill: 0, note: nil)
		var groups = [EnvelopeGroup(id: "income", name: "Income", isIncome: true, hidden: false, budgeted: 0, spent: 0, available: 0, envelopes: [salary])]
		if hasEnvelopes {
			groups.insert(EnvelopeGroup(id: "everyday", name: "Everyday", isIncome: false, hidden: false, budgeted: 0, spent: 0, available: 0, envelopes: [groceries]), at: 0)
		}
		return BudgetMonth(
			month: month, firstMonth: month, lastMonth: month,
			toBudget: 0, toBudgetTone: .neutral, income: 0, fromLastMonth: 0, overspentLastMonth: 0, budgeted: 0, forNextMonth: 0, spent: 0, available: 0,
			groups: groups, mixedScaleNote: nil
		)
	}

	func undo() async throws {}
	func redo() async throws {}
	func maintain() async throws {}
	func exportActualZip(to destination: URL) async throws -> [AuditFinding] { [] }
	func observeChanges(_ changed: @escaping @Sendable () -> Void) async throws {}
	func close() async {}
}

final class TourFakeLibrary: BudgetLibrary, @unchecked Sendable {
	let session: TourFakeSession

	init(session: TourFakeSession) {
		self.session = session
	}

	func newBudgetDirectory() -> URL { URL(filePath: "/tmp/budgets/new") }
	func open(directory: URL) async throws -> any BudgetSession { session }
	func importActualZip(_ zip: URL, into directory: URL) async throws -> any BudgetSession { session }
	func createBudget(_ budget: NewBudget, into directory: URL) async throws -> any BudgetSession { session }
	func currencies() async throws -> [CurrencyOption] { FakeAmounts.currencies }
	func evaluateAmount(_ text: String, style: NumberStyle) async throws -> Int64 { try FakeAmounts.evaluate(text) }
}
