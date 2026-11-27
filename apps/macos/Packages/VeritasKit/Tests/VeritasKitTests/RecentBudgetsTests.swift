import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

@MainActor
struct RecentBudgetsTests {
	nonisolated static func folder(_ name: String) -> URL {
		URL(filePath: "/tmp/budgets/\(name)", directoryHint: .isDirectory)
	}

	@Test func theMostRecentComesFirstAndEachBudgetOnce() {
		let recents = RecentBudgets(defaults: nil, locate: { _ in .here })
		recents.opened(name: "Household", folder: Self.folder("a"))
		recents.opened(name: "Business", folder: Self.folder("b"))
		recents.opened(name: "Household", folder: URL(filePath: "/tmp/budgets/a"))
		#expect(recents.entries.map(\.name) == ["Household", "Business"], "the same folder, with or without its slash, is one budget")
		#expect(recents.entries(besides: Self.folder("a")).map(\.name) == ["Business"], "the menus leave out the open budget")
		#expect(recents.entries.first?.place == "budgets/a")
	}

	@Test func itKeepsTheTenMostRecent() {
		let recents = RecentBudgets(defaults: nil, locate: { _ in .here })
		for index in 1...12 {
			recents.opened(name: "Budget \(index)", folder: Self.folder("\(index)"))
		}
		#expect(recents.entries.count == RecentBudgets.limit)
		#expect(recents.entries.first?.name == "Budget 12")
		#expect(recents.entries.last?.name == "Budget 3")
	}

	@Test func aRenamedBudgetKeepsItsPlace() {
		let recents = RecentBudgets(defaults: nil, locate: { _ in .here })
		recents.opened(name: "Household", folder: Self.folder("a"))
		recents.opened(name: "Business", folder: Self.folder("b"))
		recents.renamed("Home", folder: Self.folder("a"))
		recents.renamed("Never opened", folder: Self.folder("c"))
		#expect(recents.entries.map(\.name) == ["Business", "Home"])
	}

	@Test func aFolderThatsGoneIsDropped() {
		var gone: Set<URL> = []
		let recents = RecentBudgets(defaults: nil, locate: { gone.contains($0) ? .gone : .here })
		recents.opened(name: "Household", folder: Self.folder("a"))
		recents.opened(name: "Business", folder: Self.folder("b"))
		gone.insert(Self.folder("a"))
		recents.forgetMissing()
		#expect(recents.entries.map(\.name) == ["Business"])
	}

	@Test func aBudgetOnADisconnectedDriveStaysButIsUnavailable() {
		let places: [URL: RecentBudgets.Location] = [Self.folder("drive"): .unmounted, Self.folder("deleted"): .gone]
		let recents = RecentBudgets(defaults: nil, locate: { places[$0] ?? .here })
		recents.opened(name: "Deleted", folder: Self.folder("deleted"))
		recents.opened(name: "On the drive", folder: Self.folder("drive"))
		recents.opened(name: "Household", folder: Self.folder("a"))
		recents.forgetMissing()
		#expect(recents.entries.map(\.name) == ["Household", "On the drive"], "a deleted folder goes; an unplugged drive's budget stays")
		#expect(recents.entries.map(recents.isAvailable) == [true, false])
	}

	@Test func aFolderOnAVolumeThatIsntMountedIsUnmountedNotGone() throws {
		let here = FileManager.default.temporaryDirectory
		#expect(RecentBudgets.location(here) == .here)
		#expect(RecentBudgets.location(URL(filePath: "/Volumes/Veritas-\(UUID().uuidString)/budgets/a")) == .unmounted)
		#expect(RecentBudgets.location(here.appending(path: "veritas-\(UUID().uuidString)")) == .gone)
	}

	@Test func theListIsAnAppPreferenceThatOutlivesTheWindow() throws {
		let suite = "veritas-recent-budgets-tests"
		let defaults = try #require(UserDefaults(suiteName: suite))
		defer { defaults.removePersistentDomain(forName: suite) }
		defaults.removePersistentDomain(forName: suite)
		RecentBudgets(defaults: defaults, locate: { _ in .here }).opened(name: "Household", folder: Self.folder("a"))
		let later = RecentBudgets(defaults: defaults, locate: { _ in .here })
		#expect(later.entries.map(\.name) == ["Household"])
		let afterADelete = RecentBudgets(defaults: defaults, locate: { _ in .gone })
		#expect(afterADelete.entries.isEmpty, "a folder gone since last launch is dropped on the next")
		later.opened(name: "Household", folder: Self.folder("a"))
		later.clear()
		#expect(RecentBudgets(defaults: defaults, locate: { _ in .here }).entries.isEmpty)
	}

	@Test func aWindowWithNothingToRestoreReopensTheLastBudgetUnlessItsOpenElsewhere() {
		var unplugged = false
		let recents = RecentBudgets(defaults: nil, locate: { _ in unplugged ? .unmounted : .here })
		recents.opened(name: "Business", folder: Self.folder("b"))
		recents.opened(name: "Household", folder: Self.folder("a"))
		recents.hold(Self.folder("a"))
		#expect(recents.budgetToReopen == nil, "another window has it open")
		recents.release(Self.folder("a"))
		#expect(recents.budgetToReopen?.name == "Household", "the most recent, not the next one down")
		unplugged = true
		#expect(recents.budgetToReopen == nil, "its drive isn't connected")
	}

	@Test func closingTheWindowAndOpeningANewOneReopensTheBudget() async {
		let library = SwitchingLibrary()
		let recents = RecentBudgets(defaults: nil, locate: { _ in .here })
		let first = WorkspaceModel(library: library, recents: recents, format: MoneyFormat())
		await first.reopen(directory: Self.folder("a"))
		let alongside = WorkspaceModel(library: library, recents: recents, format: MoneyFormat())
		await alongside.reopenMostRecent()
		#expect(alongside.phase == .empty, "a second window doesn't open the same budget")
		await first.closeBudget()
		let later = WorkspaceModel(library: library, recents: recents, format: MoneyFormat())
		await later.reopenMostRecent()
		#expect(later.overview?.name == "a")
	}

	@Test func twoWindowsOpeningAtOnceDontBothPickTheLastBudget() async {
		let library = SwitchingLibrary()
		let recents = RecentBudgets(defaults: nil, locate: { _ in .here })
		recents.opened(name: "a", folder: Self.folder("a"))
		library.holdsOpen = true
		let first = WorkspaceModel(library: library, recents: recents, format: MoneyFormat())
		let second = WorkspaceModel(library: library, recents: recents, format: MoneyFormat())
		let opening = Task { await first.reopenMostRecent() }
		while !library.isHoldingOpen { await Task.yield() }
		await second.reopenMostRecent()
		#expect(second.phase == .empty, "the first window already holds it, though it hasn't finished opening")
		library.releaseOpen()
		await opening.value
		#expect(first.overview?.name == "a")
		#expect(library.log == ["open a"])
	}

	@Test func aBudgetThatFailsToReopenIsntLeftHeld() async {
		let library = SwitchingLibrary()
		library.missing.insert(Self.folder("a"))
		let recents = RecentBudgets(defaults: nil, locate: { _ in .here })
		recents.opened(name: "a", folder: Self.folder("a"))
		let window = WorkspaceModel(library: library, recents: recents, format: MoneyFormat())
		await window.reopenMostRecent()
		#expect(window.phase == .empty)
		#expect(recents.budgetToReopen?.name == "a", "the next window may try it again")
	}

	@Test func closingOneOfTwoWindowsOnABudgetKeepsItHeld() async {
		let library = SwitchingLibrary()
		let recents = RecentBudgets(defaults: nil, locate: { _ in .here })
		let first = WorkspaceModel(library: library, recents: recents, format: MoneyFormat())
		let second = WorkspaceModel(library: library, recents: recents, format: MoneyFormat())
		await first.reopen(directory: Self.folder("a"))
		await second.reopen(directory: Self.folder("a"))
		await first.closeBudget()
		#expect(recents.budgetToReopen == nil, "the second window still has it open")
		await second.closeBudget()
		#expect(recents.budgetToReopen?.name == "a")
	}

	@Test func drawingTheMenusNeverLooksAtTheDisk() {
		var looks = 0
		let recents = RecentBudgets(defaults: nil, locate: { _ in looks += 1; return .here })
		recents.opened(name: "Household", folder: Self.folder("a"))
		recents.opened(name: "Business", folder: Self.folder("b"))
		let afterOpening = looks
		for _ in 0..<20 {
			_ = recents.entries.map(recents.isAvailable)
		}
		#expect(looks == afterOpening, "a drive that doesn't answer can't freeze the menu")
	}

	@Test func switchingClosesTheOpenBudgetBeforeOpeningTheNext() async throws {
		let library = SwitchingLibrary()
		let model = WorkspaceModel(library: library, recents: RecentBudgets(defaults: nil, locate: { _ in .here }), format: MoneyFormat())
		await model.reopen(directory: Self.folder("a"))
		await model.reopen(directory: Self.folder("b"))
		#expect(model.recents.entries.map(\.name) == ["b", "a"])
		library.log = []

		let offered = model.recents.entries(besides: model.overview?.directory)
		let household = try #require(offered.first)
		await model.switchTo(household)
		#expect(library.log == ["close b", "open a"], "the lock and snapshots settle before the next budget opens")
		#expect(model.overview?.name == "a")
		#expect(model.recents.entries.map(\.name) == ["a", "b"])

		library.log = []
		await model.switchTo(RecentBudgets.Entry(name: "a", folder: Self.folder("a")))
		#expect(library.log.isEmpty, "choosing the open budget does nothing")
	}

	@Test func aSecondSwitchWaitsForTheFirstToFinishClosing() async throws {
		let library = SwitchingLibrary()
		let model = WorkspaceModel(library: library, recents: RecentBudgets(defaults: nil, locate: { _ in .here }), format: MoneyFormat())
		await model.reopen(directory: Self.folder("c"))
		await model.reopen(directory: Self.folder("a"))
		await model.reopen(directory: Self.folder("b"))
		library.log = []
		library.holdsClose = true
		let first = Task { await model.switchTo(RecentBudgets.Entry(name: "a", folder: Self.folder("a"))) }
		while !library.isHoldingClose { await Task.yield() }
		#expect(model.phase == .working, "the menus are off while the budget closes")
		await model.switchTo(RecentBudgets.Entry(name: "c", folder: Self.folder("c")))
		library.releaseClose()
		await first.value
		#expect(library.log == ["close b", "open a"], "the second switch never started")
		#expect(model.overview?.name == "a")
		#expect(model.phase == .open)
	}

	@Test func aBudgetThatCantOpenLeavesTheWindowEmptyAndTheListWithoutIt() async {
		let library = SwitchingLibrary()
		var gone: Set<URL> = []
		let model = WorkspaceModel(library: library, recents: RecentBudgets(defaults: nil, locate: { gone.contains($0) ? .gone : .here }), format: MoneyFormat())
		await model.reopen(directory: Self.folder("a"))
		await model.reopen(directory: Self.folder("b"))
		gone.insert(Self.folder("a"))
		library.missing.insert(Self.folder("a"))
		await model.switchTo(RecentBudgets.Entry(name: "a", folder: Self.folder("a")))
		#expect(model.phase == .empty)
		#expect(model.alert != nil)
		#expect(model.recents.entries.map(\.name) == ["b"])
	}
}

/// Opens one session per folder, named after it, and logs opens and closes.
private final class SwitchingLibrary: BudgetLibrary, @unchecked Sendable {
	var log: [String] = []
	var missing: Set<URL> = []
	/// Holds the next open until `releaseOpen`, as a slow open would.
	var holdsOpen = false
	private(set) var isHoldingOpen = false
	private var heldOpen: CheckedContinuation<Void, Never>?

	func releaseOpen() {
		heldOpen?.resume()
		heldOpen = nil
	}

	/// Holds the next close until `releaseClose`, as a slow close would.
	var holdsClose = false
	private(set) var isHoldingClose = false
	private var heldClose: CheckedContinuation<Void, Never>?

	func closed(_ name: String) async {
		if holdsClose {
			holdsClose = false
			await withCheckedContinuation { heldClose = $0; isHoldingClose = true }
		}
		log.append("close \(name)")
	}

	func releaseClose() {
		heldClose?.resume()
		heldClose = nil
	}

	func newBudgetDirectory() -> URL { RecentBudgetsTests.folder("new") }

	func open(directory: URL) async throws -> any BudgetSession {
		if missing.contains(directory) { throw FfiError.Io(message: "\(directory.path) doesn't hold a budget") }
		if holdsOpen {
			holdsOpen = false
			await withCheckedContinuation { heldOpen = $0; isHoldingOpen = true }
		}
		let name = directory.lastPathComponent
		log.append("open \(name)")
		return SwitchingSession(name: name, directory: directory) { [weak self] in await self?.closed(name) }
	}

	func importActualZip(_ zip: URL, into directory: URL) async throws -> any BudgetSession { try await open(directory: directory) }
	func createBudget(_ budget: NewBudget, into directory: URL) async throws -> any BudgetSession { try await open(directory: directory) }
	func currencies() async throws -> [CurrencyOption] { FakeAmounts.currencies }
	func evaluateAmount(_ text: String, style: NumberStyle) async throws -> Int64 { try FakeAmounts.evaluate(text) }
}

private final class SwitchingSession: BudgetSession, @unchecked Sendable {
	let name: String
	let directory: URL
	let closed: () async -> Void
	let canExport = true

	init(name: String, directory: URL, closed: @escaping () async -> Void) {
		self.name = name
		self.directory = directory
		self.closed = closed
	}

	func overview() async throws -> BudgetOverview {
		BudgetOverview(name: name, directory: directory, readOnly: nil, accountGroups: [])
	}

	func undo() async throws {}
	func redo() async throws {}
	func maintain() async throws {}
	func exportActualZip(to destination: URL) async throws -> [AuditFinding] { [] }
	func observeChanges(_ changed: @escaping @Sendable () -> Void) async throws {}
	func close() async { await closed() }
}
