import Foundation
import Testing
import VeritasCore
import VeritasKit

@MainActor
struct WorkspaceModelTests {
	@Test func importingABudgetOpensItWithItsAccounts() async {
		let library = FakeLibrary()
		let model = WorkspaceModel(library: library, format: MoneyFormat())
		#expect(model.phase == .empty)
		await model.importActualZip(URL(filePath: "/tmp/My Finances.zip"))
		#expect(model.phase == .open)
		#expect(model.overview?.name == "Household")
		#expect(model.paletteIndex.results(for: "check").first?.detail == "2,140.18")
	}

	@Test func aFailedImportStaysOnTheEmptyStateAndExplains() async {
		let library = FakeLibrary(importError: .UpgradeInActualFirst(required: "26.11.0"))
		let model = WorkspaceModel(library: library, format: MoneyFormat())
		await model.importActualZip(URL(filePath: "/tmp/old.zip"))
		#expect(model.phase == .empty)
		#expect(model.alert?.title == "Upgrade this budget in Actual first")
	}

	@Test func aBudgetOpenedReadOnlyShowsItsReason() async {
		let library = FakeLibrary(readOnly: .trackingBudget)
		let model = WorkspaceModel(library: library, format: MoneyFormat())
		await model.importActualZip(URL(filePath: "/tmp/tracking.zip"))
		#expect(model.readOnly == .trackingBudget)
	}

	@Test func aReadOnlyErrorFromAWriteRaisesTheBanner() async {
		let model = WorkspaceModel(library: FakeLibrary(exportError: .ReadOnly(reason: .openElsewhere)), format: MoneyFormat())
		await model.importActualZip(URL(filePath: "/tmp/a.zip"))
		await model.exportActualZip(to: URL(filePath: "/tmp/out.zip"))
		#expect(model.readOnly == .openElsewhere)
		#expect(model.alert == nil)
	}

	@Test func retryRunsTheFailedExportAgain() async {
		let library = FakeLibrary(exportError: .Storage(message: "disk I/O error"))
		let model = WorkspaceModel(library: library, format: MoneyFormat())
		await model.importActualZip(URL(filePath: "/tmp/a.zip"))
		await model.exportActualZip(to: URL(filePath: "/tmp/out.zip"))
		#expect(model.alert?.offersRetry == true)
		await model.retryFailedAction()
		#expect(library.session.exports == 2)
	}

	@Test func aFailedAuditOpensTheIntegrityReport() async {
		let model = WorkspaceModel(library: FakeLibrary(exportError: .AuditFailed(violations: 2)), format: MoneyFormat())
		await model.importActualZip(URL(filePath: "/tmp/a.zip"))
		await model.exportActualZip(to: URL(filePath: "/tmp/out.zip"))
		#expect(model.sheet == .integrityReport)
	}

	@Test func closingReturnsToTheEmptyState() async {
		let library = FakeLibrary()
		let model = WorkspaceModel(library: library, format: MoneyFormat())
		await model.importActualZip(URL(filePath: "/tmp/a.zip"))
		model.go(to: .screen(.bills))
		await model.closeBudget()
		#expect(model.phase == .empty)
		#expect(model.overview == nil)
		#expect(model.destination == .start)
		#expect(library.session.closed)
	}

	@Test func aRestoredAccountThatNoLongerExistsFallsBackToAccounts() async {
		let model = WorkspaceModel(library: FakeLibrary(), format: MoneyFormat())
		model.destination = .account(id: "gone")
		await model.reopen(directory: URL(filePath: "/tmp/budget"))
		#expect(model.destination == .screen(.accounts))
	}

	@Test func paletteItemsNavigateOrOpenTheirSheet() async {
		let model = WorkspaceModel(library: FakeLibrary(), format: MoneyFormat())
		await model.importActualZip(URL(filePath: "/tmp/a.zip"))
		model.isPaletteShown = true
		model.activate(model.paletteIndex.results(for: "net").first!)
		#expect(model.destination == .screen(.netProfit))
		#expect(!model.isPaletteShown)
		model.activate(model.paletteIndex.results(for: "move").first!)
		#expect(model.sheet == .moveMoney)
	}

	@Test func denseOnlyTogglesOnBudget() async {
		let model = WorkspaceModel(library: FakeLibrary(), format: MoneyFormat())
		await model.importActualZip(URL(filePath: "/tmp/a.zip"))
		model.toggleDense()
		#expect(model.isDense)
		model.go(to: .screen(.bills))
		model.toggleDense()
		#expect(model.isDense)
		#expect(!model.canToggleDense)
	}

	@Test func aFailedAuditDuringImportOpensTheIntegrityReport() async {
		let model = WorkspaceModel(library: FakeLibrary(importError: .AuditFailed(violations: 4)), format: MoneyFormat())
		await model.importActualZip(URL(filePath: "/tmp/a.zip"))
		#expect(model.phase == .empty)
		#expect(model.sheet == .integrityReport)
	}

	@Test func aSessionThatCantDescribeItselfIsClosedSoItsLockIsFreed() async {
		// R-REL-6
		let library = FakeLibrary()
		library.session.overviewErrors = [.Storage(message: "disk I/O error")]
		let model = WorkspaceModel(library: library, format: MoneyFormat())
		await model.reopen(directory: URL(filePath: "/tmp/budget"))
		#expect(model.phase == .empty)
		#expect(library.session.closed)
		#expect(model.alert?.title == "Couldn't open this budget")
	}

	@Test func retryingAFailedImportReusesItsFolder() async {
		let library = FakeLibrary(importError: .Storage(message: "disk I/O error"))
		let model = WorkspaceModel(library: library, format: MoneyFormat())
		await model.importActualZip(URL(filePath: "/tmp/a.zip"))
		#expect(model.alert?.title == "Couldn't import that budget")
		await model.retryFailedAction()
		#expect(model.phase == .open)
		#expect(library.importDirectories.count == 2)
		#expect(Set(library.importDirectories).count == 1)
	}

	@Test func aMissingRowDuringARefreshDoesntRefreshForever() async throws {
		let library = FakeLibrary()
		let model = WorkspaceModel(library: library, format: MoneyFormat())
		await model.importActualZip(URL(filePath: "/tmp/a.zip"))
		library.session.overviewErrors = Array(repeating: .NotFound(entity: "account", id: "a"), count: 50)
		await model.refresh()
		try await Task.sleep(for: .milliseconds(50))
		#expect(library.session.overviews == 2)
	}

	@Test func undoAndRedoFollowTheCoresState() async {
		let library = FakeLibrary()
		library.session.undoable = 1
		let model = WorkspaceModel(library: library, format: MoneyFormat())
		await model.importActualZip(URL(filePath: "/tmp/a.zip"))
		#expect(model.canUndo && !model.canRedo)
		await model.undo()
		#expect(!model.canUndo && model.canRedo)
		await model.redo()
		#expect(model.canUndo && !model.canRedo)
	}

	@Test func aChangeFromTheCoreRefreshesTheWindow() async throws {
		let library = FakeLibrary()
		let model = WorkspaceModel(library: library, format: MoneyFormat())
		await model.importActualZip(URL(filePath: "/tmp/a.zip"))
		library.session.readOnly = .openElsewhere
		let changed = try #require(library.session.changed)
		changed()
		var turns = 0
		while model.readOnly == nil, turns < 100 {
			await Task.yield()
			turns += 1
		}
		#expect(model.readOnly == .openElsewhere)
	}

	@Test func maintenanceCollectsTheOpenCheckWhileItRuns() async throws {
		let library = FakeLibrary()
		library.session.checking = true
		let model = WorkspaceModel(library: library, format: MoneyFormat(), maintenanceInterval: .seconds(60), checkingInterval: .milliseconds(5))
		await model.importActualZip(URL(filePath: "/tmp/a.zip"))
		var turns = 0
		while model.readOnly == nil, turns < 200 {
			try await Task.sleep(for: .milliseconds(5))
			turns += 1
		}
		#expect(model.readOnly == .auditFailed)
		#expect(library.session.maintained == 1)
		await model.closeBudget()
	}

	@Test func exportStaysOffUntilTheCoreCanExport() async {
		let library = FakeLibrary(canExport: false)
		let model = WorkspaceModel(library: library, format: MoneyFormat())
		await model.importActualZip(URL(filePath: "/tmp/a.zip"))
		#expect(!model.canExport)
		await model.exportActualZip(to: URL(filePath: "/tmp/out.zip"))
		#expect(library.session.exports == 0)
	}
}

private final class FakeSession: BudgetSession, @unchecked Sendable {
	let exportError: FfiError?
	let canExport: Bool
	var readOnly: ReadOnlyKind?
	var overviewErrors: [FfiError] = []
	var overviews = 0
	var undoable = 0
	var redoable = 0
	var checking = false
	var exports = 0
	var maintained = 0
	var closed = false
	var changed: (@Sendable () -> Void)?

	init(readOnly: ReadOnlyKind?, exportError: FfiError?, canExport: Bool) {
		self.readOnly = readOnly
		self.exportError = exportError
		self.canExport = canExport
	}

	func overview() async throws -> BudgetOverview {
		overviews += 1
		if !overviewErrors.isEmpty { throw overviewErrors.removeFirst() }
		return BudgetOverview(
			name: "Household",
			directory: URL(filePath: "/tmp/budget"),
			readOnly: readOnly,
			canUndo: undoable > 0,
			canRedo: redoable > 0,
			isChecking: checking,
			accountGroups: [SidebarGroup(name: "Cash", accounts: [SidebarAccount(id: "chk", name: "Checking", balance: 214_018)])]
		)
	}

	func undo() async throws {
		undoable -= 1
		redoable += 1
	}

	func redo() async throws {
		redoable -= 1
		undoable += 1
	}

	func maintain() async throws {
		maintained += 1
		if checking {
			checking = false
			readOnly = .auditFailed
		}
	}

	func exportActualZip(to destination: URL) async throws -> [AuditFinding] {
		exports += 1
		if let exportError { throw exportError }
		return []
	}

	func observeChanges(_ changed: @escaping @Sendable () -> Void) async throws {
		self.changed = changed
	}

	func close() async {
		closed = true
	}
}

private final class FakeLibrary: BudgetLibrary, @unchecked Sendable {
	let session: FakeSession
	var importErrors: [FfiError]
	var importDirectories: [URL] = []
	var created: [NewBudget] = []

	init(readOnly: ReadOnlyKind? = nil, importError: FfiError? = nil, exportError: FfiError? = nil, canExport: Bool = true) {
		session = FakeSession(readOnly: readOnly, exportError: exportError, canExport: canExport)
		importErrors = importError.map { [$0] } ?? []
	}

	func newBudgetDirectory() -> URL {
		URL(filePath: "/tmp/budgets/\(UUID().uuidString)", directoryHint: .isDirectory)
	}

	func open(directory: URL) async throws -> any BudgetSession {
		session
	}

	func importActualZip(_ zip: URL, into directory: URL) async throws -> any BudgetSession {
		importDirectories.append(directory)
		if !importErrors.isEmpty { throw importErrors.removeFirst() }
		return session
	}

	func createBudget(_ budget: NewBudget, into directory: URL) async throws -> any BudgetSession {
		created.append(budget)
		return session
	}

	func currencies() async throws -> [CurrencyOption] {
		FakeAmounts.currencies
	}

	func evaluateAmount(_ text: String) async throws -> Int64 {
		try FakeAmounts.evaluate(text)
	}
}
