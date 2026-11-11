import Foundation
import Testing
import VeritasCore
import VeritasKit

/// R-REL-3, R-REL-4, R-REL-2: backups, snapshots, the integrity report and the budget list.
@MainActor
struct SettingsAndReportTests {
	let local = InMemoryBudgetSettings()
	let budget = URL(filePath: "/tmp/budget", directoryHint: .isDirectory)

	@Test func theSavedBackupFolderIsHandedToTheCoreOnOpen() async throws {
		let session = MaintainedSession()
		try local.rememberBackupFolder(URL(filePath: "/Volumes/OneDrive/Backups"), for: budget)
		let model = await opened(session)
		#expect(session.backupFolder?.path(percentEncoded: false) == "/Volumes/OneDrive/Backups")
		#expect(model.settings?.backup.folder != nil)
	}

	@Test func aDueScheduleBacksUpOnOpenAndAnUnduOneDoesnt() async throws {
		let session = MaintainedSession()
		try local.rememberBackupFolder(URL(filePath: "/backups"), for: budget)
		local.setBackupSchedule(.daily, for: budget)
		_ = await opened(session)
		#expect(session.backups == 1)

		let recent = MaintainedSession()
		recent.lastBackup = .now.addingTimeInterval(-60 * 60)
		_ = await opened(recent)
		#expect(recent.backups == 0)
	}

	@Test func schedulesFollowTheirInterval() {
		let now = Date(timeIntervalSince1970: 1_000_000_000)
		#expect(!BackupSchedule.off.isDue(lastBackup: nil, now: now))
		#expect(BackupSchedule.daily.isDue(lastBackup: nil, now: now))
		#expect(!BackupSchedule.daily.isDue(lastBackup: now.addingTimeInterval(-23 * 3600), now: now))
		#expect(BackupSchedule.daily.isDue(lastBackup: now.addingTimeInterval(-24 * 3600), now: now))
		#expect(!BackupSchedule.weekly.isDue(lastBackup: now.addingTimeInterval(-6 * 86400), now: now))
	}

	@Test func aFailedBackupShowsTheCoresProblemAndNeverAlerts() async throws {
		let session = MaintainedSession()
		session.backupProblem = "Can't write to /Volumes/OneDrive/Backups. Check that the folder exists."
		try local.rememberBackupFolder(URL(filePath: "/Volumes/OneDrive/Backups"), for: budget)
		let model = await opened(session)
		await model.settings?.backUpNow()
		#expect(model.settings?.backup.lastProblem == session.backupProblem)
		#expect(model.alert == nil)
	}

	@Test func backUpNowNeedsAFolder() async {
		let session = MaintainedSession()
		let model = await opened(session)
		await model.settings?.backUpNow()
		#expect(session.backups == 0)
	}

	@Test func restoringASnapshotReloadsTheList() async throws {
		let session = MaintainedSession()
		let model = await opened(session)
		let settings = try #require(model.settings)
		await settings.load()
		#expect(settings.snapshots.map(\.id) == ["s2", "s1"])
		await settings.restoreSnapshot(id: "s1")
		#expect(session.restored == ["s1"])
		#expect(settings.snapshots.first?.reason == .restore)
	}

	@Test func retentionChangesReachTheCore() async throws {
		let session = MaintainedSession()
		let model = await opened(session)
		let settings = try #require(model.settings)
		await settings.load()
		await settings.setRetention(Retention(hourly: 24, daily: 14, monthly: 6, operationDays: 7))
		#expect(session.savedRetention?.hourly == 24)
		#expect(settings.retention?.daily == 14)
	}

	@Test func theReportShowsTheBudgetsFindingsOnceTheCheckIsDone() async throws {
		let session = MaintainedSession()
		session.findings = [AuditFinding(check: .unpairedTransfer, detail: nil, table: "transactions", id: "t1", related: nil)]
		let model = await opened(session)
		model.showIntegrityReport()
		#expect(model.sheet == .integrityReport)
		var turns = 0
		while model.integrityReport == .budget(findings: nil), turns < 100 {
			await Task.yield()
			turns += 1
		}
		#expect(model.integrityReport == .budget(findings: session.findings))
	}

	@Test func anExportWithFindingsOpensTheReport() async {
		let session = MaintainedSession()
		session.exportFindings = [AuditFinding(check: .splitSumMismatch, detail: "Parts add up to 40.00", table: "transactions", id: "p1", related: nil)]
		let model = await opened(session)
		await model.exportActualZip(to: URL(filePath: "/tmp/out.zip"))
		#expect(model.integrityReport == .export(findings: session.exportFindings))
		#expect(model.sheet == .integrityReport)
	}

	@Test func aDamagedImportIsReportedWithoutABudget() async {
		let library = MaintainedLibrary(session: MaintainedSession())
		library.importError = .AuditFailed(violations: 3)
		let model = WorkspaceModel(library: library, localSettings: local, format: MoneyFormat())
		await model.importActualZip(URL(filePath: "/tmp/damaged.zip"))
		#expect(model.integrityReport == .refusedImport(problems: 3))
		#expect(model.phase == .empty)
	}

	@Test func theOpenPickerListsBudgetsAndOpensOne() async throws {
		let session = MaintainedSession()
		let library = MaintainedLibrary(session: session)
		let model = WorkspaceModel(library: library, localSettings: local, format: MoneyFormat())
		model.showOpenPicker()
		var turns = 0
		while model.budgetList == nil, turns < 100 {
			await Task.yield()
			turns += 1
		}
		let first = try #require(model.budgetList?.first)
		#expect(first.name == "Household")
		await model.openListedBudget(first)
		#expect(!model.isOpenPickerShown)
		#expect(model.phase == .open)
		#expect(library.opened == [first.folder])
	}

	private func opened(_ session: MaintainedSession) async -> WorkspaceModel {
		let model = WorkspaceModel(library: MaintainedLibrary(session: session), localSettings: local, format: MoneyFormat())
		await model.reopen(directory: budget)
		return model
	}
}

final class MaintainedSession: BudgetSession, @unchecked Sendable {
	let canExport = true
	var backupFolder: URL?
	var lastBackup: Date?
	var backupProblem: String?
	var backups = 0
	var findings: [AuditFinding] = []
	var exportFindings: [AuditFinding] = []
	var restored: [String] = []
	var savedRetention: Retention?
	var snapshotList = [
		SnapshotInfo(id: "s2", takenAt: "2026-11-11T10:00:00Z", reason: .hourly),
		SnapshotInfo(id: "s1", takenAt: "2026-11-11T09:00:00Z", reason: .open)
	]

	func overview() async throws -> BudgetOverview {
		BudgetOverview(name: "Household", directory: URL(filePath: "/tmp/budget", directoryHint: .isDirectory), readOnly: nil, accountGroups: [])
	}

	func undo() async throws {}
	func redo() async throws {}
	func maintain() async throws {}
	func observeChanges(_ changed: @escaping @Sendable () -> Void) async throws {}
	func close() async {}

	func exportActualZip(to destination: URL) async throws -> [AuditFinding] {
		exportFindings
	}

	func backupStatus() async throws -> BackupStatus {
		BackupStatus(folder: backupFolder, lastAt: lastBackup, lastFile: nil, lastProblem: backupProblem)
	}

	func setBackupFolder(_ folder: URL?) async throws {
		backupFolder = folder
	}

	func backUpNow() async throws {
		backups += 1
		if backupProblem == nil { lastBackup = .now }
	}

	func integrityFindings() async throws -> [AuditFinding]? {
		findings
	}

	func snapshots() async throws -> [SnapshotInfo] {
		snapshotList
	}

	func restoreSnapshot(id: String) async throws {
		restored.append(id)
		snapshotList.insert(SnapshotInfo(id: "s3", takenAt: "2026-11-11T11:00:00Z", reason: .restore), at: 0)
	}

	func retention() async throws -> Retention {
		savedRetention ?? Retention(hourly: 48, daily: 30, monthly: 12, operationDays: 30)
	}

	func setRetention(_ retention: Retention) async throws {
		savedRetention = retention
	}
}

final class MaintainedLibrary: BudgetLibrary, @unchecked Sendable {
	let session: MaintainedSession
	var importError: FfiError?
	var opened: [URL] = []

	init(session: MaintainedSession) {
		self.session = session
	}

	func listBudgets() async throws -> [BudgetListingItem] {
		[BudgetListingItem(id: "b1", name: "Household", folder: URL(filePath: "/tmp/budgets/b1", directoryHint: .isDirectory), lastOpened: .now)]
	}

	func newBudgetDirectory() -> URL {
		URL(filePath: "/tmp/budgets/new", directoryHint: .isDirectory)
	}

	func open(directory: URL) async throws -> any BudgetSession {
		opened.append(directory)
		return session
	}

	func importActualZip(_ zip: URL, into directory: URL) async throws -> any BudgetSession {
		if let importError { throw importError }
		return session
	}

	func createBudget(_ budget: NewBudget, into directory: URL) async throws -> any BudgetSession {
		session
	}

	func currencies() async throws -> [CurrencyOption] { FakeAmounts.currencies }
	func evaluateAmount(_ text: String) async throws -> Int64 { try FakeAmounts.evaluate(text) }
}
