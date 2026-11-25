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
		let recents = RecentBudgets(defaults: nil, exists: { _ in true })
		recents.opened(name: "Household", folder: Self.folder("a"))
		recents.opened(name: "Business", folder: Self.folder("b"))
		recents.opened(name: "Household", folder: URL(filePath: "/tmp/budgets/a"))
		#expect(recents.entries.map(\.name) == ["Household", "Business"], "the same folder, with or without its slash, is one budget")
		#expect(recents.entries(besides: Self.folder("a")).map(\.name) == ["Business"], "the menus leave out the open budget")
		#expect(recents.entries.first?.place == "budgets/a")
	}

	@Test func itKeepsTheTenMostRecent() {
		let recents = RecentBudgets(defaults: nil, exists: { _ in true })
		for index in 1...12 {
			recents.opened(name: "Budget \(index)", folder: Self.folder("\(index)"))
		}
		#expect(recents.entries.count == RecentBudgets.limit)
		#expect(recents.entries.first?.name == "Budget 12")
		#expect(recents.entries.last?.name == "Budget 3")
	}

	@Test func aRenamedBudgetKeepsItsPlace() {
		let recents = RecentBudgets(defaults: nil, exists: { _ in true })
		recents.opened(name: "Household", folder: Self.folder("a"))
		recents.opened(name: "Business", folder: Self.folder("b"))
		recents.renamed("Home", folder: Self.folder("a"))
		recents.renamed("Never opened", folder: Self.folder("c"))
		#expect(recents.entries.map(\.name) == ["Business", "Home"])
	}

	@Test func aFolderThatsGoneIsDropped() {
		var gone: Set<URL> = []
		let recents = RecentBudgets(defaults: nil, exists: { !gone.contains($0) })
		recents.opened(name: "Household", folder: Self.folder("a"))
		recents.opened(name: "Business", folder: Self.folder("b"))
		gone.insert(Self.folder("a"))
		recents.forgetMissing()
		#expect(recents.entries.map(\.name) == ["Business"])
	}

	@Test func theListIsAnAppPreferenceThatOutlivesTheWindow() throws {
		let suite = "veritas-recent-budgets-tests"
		let defaults = try #require(UserDefaults(suiteName: suite))
		defer { defaults.removePersistentDomain(forName: suite) }
		defaults.removePersistentDomain(forName: suite)
		RecentBudgets(defaults: defaults, exists: { _ in true }).opened(name: "Household", folder: Self.folder("a"))
		let later = RecentBudgets(defaults: defaults, exists: { _ in true })
		#expect(later.entries.map(\.name) == ["Household"])
		let afterADelete = RecentBudgets(defaults: defaults, exists: { _ in false })
		#expect(afterADelete.entries.isEmpty, "a folder gone since last launch is dropped on the next")
		later.opened(name: "Household", folder: Self.folder("a"))
		later.clear()
		#expect(RecentBudgets(defaults: defaults, exists: { _ in true }).entries.isEmpty)
	}

	@Test func switchingClosesTheOpenBudgetBeforeOpeningTheNext() async throws {
		let library = SwitchingLibrary()
		let model = WorkspaceModel(library: library, recents: RecentBudgets(defaults: nil, exists: { _ in true }), format: MoneyFormat())
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

	@Test func aBudgetThatCantOpenLeavesTheWindowEmptyAndTheListWithoutIt() async {
		let library = SwitchingLibrary()
		var gone: Set<URL> = []
		let model = WorkspaceModel(library: library, recents: RecentBudgets(defaults: nil, exists: { !gone.contains($0) }), format: MoneyFormat())
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

	func newBudgetDirectory() -> URL { RecentBudgetsTests.folder("new") }

	func open(directory: URL) async throws -> any BudgetSession {
		if missing.contains(directory) { throw FfiError.Io(message: "\(directory.path) doesn't hold a budget") }
		let name = directory.lastPathComponent
		log.append("open \(name)")
		return SwitchingSession(name: name, directory: directory) { [weak self] in self?.log.append("close \(name)") }
	}

	func importActualZip(_ zip: URL, into directory: URL) async throws -> any BudgetSession { try await open(directory: directory) }
	func createBudget(_ budget: NewBudget, into directory: URL) async throws -> any BudgetSession { try await open(directory: directory) }
	func currencies() async throws -> [CurrencyOption] { FakeAmounts.currencies }
	func evaluateAmount(_ text: String, style: NumberStyle) async throws -> Int64 { try FakeAmounts.evaluate(text) }
}

private final class SwitchingSession: BudgetSession, @unchecked Sendable {
	let name: String
	let directory: URL
	let closed: () -> Void
	let canExport = true

	init(name: String, directory: URL, closed: @escaping () -> Void) {
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
	func close() async { closed() }
}
