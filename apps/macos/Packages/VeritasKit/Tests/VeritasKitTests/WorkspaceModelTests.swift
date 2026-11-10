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
}

private final class FakeSession: BudgetSession, @unchecked Sendable {
	let readOnly: ReadOnlyKind?
	let exportError: FfiError?
	var exports = 0
	var closed = false

	init(readOnly: ReadOnlyKind?, exportError: FfiError?) {
		self.readOnly = readOnly
		self.exportError = exportError
	}

	func overview() async throws -> BudgetOverview {
		BudgetOverview(
			name: "Household",
			directory: URL(filePath: "/tmp/budget"),
			readOnly: readOnly,
			accountGroups: [AccountGroup(name: "Cash", accounts: [AccountSummary(id: "chk", name: "Checking", balance: 214_018)])]
		)
	}

	func exportActualZip(to destination: URL) async throws {
		exports += 1
		if let exportError { throw exportError }
	}

	func close() async {
		closed = true
	}
}

private struct FakeLibrary: BudgetLibrary {
	let session: FakeSession
	let importError: FfiError?

	init(readOnly: ReadOnlyKind? = nil, importError: FfiError? = nil, exportError: FfiError? = nil) {
		session = FakeSession(readOnly: readOnly, exportError: exportError)
		self.importError = importError
	}

	func open(directory: URL) async throws -> any BudgetSession {
		session
	}

	func importActualZip(_ zip: URL) async throws -> any BudgetSession {
		if let importError { throw importError }
		return session
	}
}
