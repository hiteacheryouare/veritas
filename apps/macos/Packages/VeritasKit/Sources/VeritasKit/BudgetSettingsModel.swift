import Foundation
import Observation
import VeritasCore

/// Settings for the open budget: backups (R-REL-4) and snapshots (R-REL-3).
@MainActor
@Observable
public final class BudgetSettingsModel {
	public private(set) var backup = BackupStatus.none
	public private(set) var schedule = BackupSchedule.off
	public private(set) var snapshots: [SnapshotInfo] = []
	public private(set) var retention: Retention?
	public private(set) var isBackingUp = false
	public private(set) var isRestoring = false

	@ObservationIgnored private let session: any BudgetSession
	@ObservationIgnored private let local: any LocalBudgetSettings
	@ObservationIgnored private let budget: URL
	@ObservationIgnored private let report: (any Error) -> Void

	init(session: any BudgetSession, local: any LocalBudgetSettings, budget: URL, report: @escaping (any Error) -> Void) {
		self.session = session
		self.local = local
		self.budget = budget
		self.report = report
		schedule = local.backupSchedule(for: budget)
	}

	/// The core forgets the folder between opens, so the app hands it over each time.
	func connectBackupFolder() async {
		guard let folder = local.backupFolder(for: budget) else { return }
		do {
			try await session.setBackupFolder(folder)
		} catch {
			report(error)
		}
		await refreshBackup()
	}

	public func load() async {
		await refreshBackup()
		do {
			snapshots = try await session.snapshots()
			retention = try await session.retention()
		} catch {
			report(error)
		}
	}

	public func chooseBackupFolder(_ folder: URL?) async {
		do {
			try local.rememberBackupFolder(folder, for: budget)
			try await session.setBackupFolder(folder.flatMap { _ in local.backupFolder(for: budget) })
		} catch {
			report(error)
		}
		await refreshBackup()
	}

	public func setSchedule(_ schedule: BackupSchedule) {
		self.schedule = schedule
		local.setBackupSchedule(schedule, for: budget)
	}

	public func backUpNow() async {
		guard backup.folder != nil, !isBackingUp else { return }
		isBackingUp = true
		defer { isBackingUp = false }
		// A failed backup never blocks editing; the core records why in the status.
		try? await session.backUpNow()
		await refreshBackup()
	}

	func backUpIfDue(now: Date = .now) async {
		guard backup.folder != nil, schedule.isDue(lastBackup: backup.lastAt, now: now) else { return }
		await backUpNow()
	}

	public func restoreSnapshot(id: String) async {
		guard !isRestoring else { return }
		isRestoring = true
		defer { isRestoring = false }
		do {
			try await session.restoreSnapshot(id: id)
			snapshots = try await session.snapshots()
		} catch {
			report(error)
		}
	}

	public func setRetention(_ retention: Retention) async {
		do {
			try await session.setRetention(retention)
			self.retention = retention
		} catch {
			report(error)
		}
	}

	func refreshBackup() async {
		do {
			backup = try await session.backupStatus()
		} catch {
			report(error)
		}
	}
}

/// Keeps local settings in memory, for tests and previews.
public final class InMemoryBudgetSettings: LocalBudgetSettings, @unchecked Sendable {
	private let lock = NSLock()
	private var folders: [URL: URL] = [:]
	private var schedules: [URL: BackupSchedule] = [:]

	public init() {}

	public func backupFolder(for budget: URL) -> URL? {
		lock.withLock { folders[budget] }
	}

	public func rememberBackupFolder(_ folder: URL?, for budget: URL) throws {
		lock.withLock { folders[budget] = folder }
	}

	public func backupSchedule(for budget: URL) -> BackupSchedule {
		lock.withLock { schedules[budget] ?? .off }
	}

	public func setBackupSchedule(_ schedule: BackupSchedule, for budget: URL) {
		lock.withLock { schedules[budget] = schedule }
	}
}
