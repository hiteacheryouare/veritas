import Foundation
import VeritasCore

final class CoreBudgetSession: BudgetSession {
	private let session: Session
	private let listener = ChangeRelay()

	init(_ session: Session) {
		self.session = session
	}

	let canExport = true

	func overview() async throws -> BudgetOverview {
		let session = session
		let (state, overview) = try await Core.run { (try session.state(), try session.overview()) }
		return BudgetOverview(
			name: overview.budgetName,
			directory: URL(filePath: state.directory, directoryHint: .isDirectory),
			readOnly: state.readOnly,
			canUndo: state.canUndo,
			canRedo: state.canRedo,
			isChecking: state.checking,
			accountGroups: overview.groups.filter { !$0.accounts.isEmpty }.map { group in
				SidebarGroup(name: group.group.title, accounts: group.accounts.map { account in
					SidebarAccount(id: account.id, name: account.name, balance: account.balance, isInactive: account.closed || account.deleted)
				})
			},
			format: overview.numberFormat.map(MoneyFormat.init)
		)
	}

	func undo() async throws {
		let session = session
		_ = try await Core.run { try session.undo() }
	}

	func redo() async throws {
		let session = session
		_ = try await Core.run { try session.redo() }
	}

	func maintain() async throws {
		let session = session
		_ = try await Core.run { try session.maintain() }
	}

	/// The sandbox lets Veritas write the file the user chose, but nothing beside
	/// it, so the core writes into a replacement folder and the file moves into place.
	func exportActualZip(to destination: URL) async throws -> [AuditFinding] {
		let files = FileManager.default
		let staging: URL
		do {
			staging = try files.url(for: .itemReplacementDirectory, in: .userDomainMask, appropriateFor: destination, create: true)
		} catch {
			throw FfiError.Io(message: "Can't prepare the export next to \(destination.path(percentEncoded: false)). Check the folder still exists.")
		}
		defer { try? files.removeItem(at: staging) }
		let staged = staging.appending(path: destination.lastPathComponent)
		let session = session
		let findings = try await Core.call { try await session.exportActualZip(to: staged.path(percentEncoded: false)) }
		do {
			if files.fileExists(atPath: destination.path(percentEncoded: false)) {
				_ = try files.replaceItemAt(destination, withItemAt: staged)
			} else {
				try files.moveItem(at: staged, to: destination)
			}
		} catch {
			throw FfiError.Io(message: "Can't write \(destination.path(percentEncoded: false)). Check you can save files in that folder.")
		}
		return findings
	}

	func observeChanges(_ changed: @escaping @Sendable () -> Void) async throws {
		listener.set(changed)
		let session = session
		let listener = listener
		_ = try await Core.run { try session.subscribe(listener: listener) }
	}

	func close() async {
		let session = session
		_ = try? await Core.run { try session.close() }
	}

	func backupStatus() async throws -> BackupStatus {
		let session = session
		let state = try await Core.run { try session.state() }
		return BackupStatus(
			folder: state.backupFolder.map { URL(filePath: $0, directoryHint: .isDirectory) },
			lastAt: CoreTime.date(state.lastBackup?.at),
			lastFile: state.lastBackup?.file,
			lastProblem: state.lastBackup?.problem
		)
	}

	func setBackupFolder(_ folder: URL?) async throws {
		let session = session
		let path = folder?.path(percentEncoded: false)
		try await Core.run { try session.setBackupFolder(folder: path) }
	}

	func backUpNow() async throws {
		let session = session
		_ = try await Core.run { try session.backUpNow() }
	}

	func integrityFindings() async throws -> [AuditFinding]? {
		let session = session
		return try await Core.run { try session.auditReport() }
	}

	func snapshots() async throws -> [SnapshotInfo] {
		let session = session
		return try await Core.run { try session.snapshots() }
	}

	func restoreSnapshot(id: String) async throws {
		let session = session
		_ = try await Core.run { try session.restoreSnapshot(id: id) }
	}

	func retention() async throws -> Retention {
		let session = session
		return try await Core.run { try session.retention() }
	}

	func setRetention(_ retention: Retention) async throws {
		let session = session
		try await Core.run { try session.setRetention(retention: retention) }
	}
}

/// The core calls this on whichever thread made the call that changed something.
private final class ChangeRelay: ChangeListener, @unchecked Sendable {
	private let lock = NSLock()
	private var handler: (@Sendable () -> Void)?

	func set(_ handler: @escaping @Sendable () -> Void) {
		lock.withLock { self.handler = handler }
	}

	func changed(change: Change) {
		lock.withLock { handler }?()
	}
}
