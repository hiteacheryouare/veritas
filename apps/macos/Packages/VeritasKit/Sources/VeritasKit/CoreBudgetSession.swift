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

	/// In the sandbox the core writes only in the app's own folders (contract v2), so
	/// the zip is made in the temporary directory, then moved onto the file the user chose.
	func exportActualZip(to destination: URL) async throws -> [AuditFinding] {
		let files = FileManager.default
		let staging = files.temporaryDirectory.appending(path: "export-\(UUID().uuidString)", directoryHint: .isDirectory)
		defer { try? files.removeItem(at: staging) }
		let staged = staging.appending(path: destination.lastPathComponent)
		do {
			try files.createDirectory(at: staging, withIntermediateDirectories: true)
		} catch {
			throw FfiError.Io(message: "Can't prepare the export in \(staging.path(percentEncoded: false)). Check the disk has space.")
		}
		let session = session
		let findings = try await Core.call { try await session.exportActualZip(to: staged.path(percentEncoded: false)) }
		do {
			try Self.place(staged, at: destination)
		} catch {
			throw FfiError.Io(message: "Can't write \(destination.path(percentEncoded: false)). Check you can save files in that folder.")
		}
		return findings
	}

	/// Copies onto the destination's volume first, so the final replace is atomic there.
	private static func place(_ file: URL, at destination: URL) throws {
		let files = FileManager.default
		let nearby = try files.url(for: .itemReplacementDirectory, in: .userDomainMask, appropriateFor: destination, create: true)
		defer { try? files.removeItem(at: nearby) }
		let copy = nearby.appending(path: destination.lastPathComponent)
		try files.copyItem(at: file, to: copy)
		if files.fileExists(atPath: destination.path(percentEncoded: false)) {
			_ = try files.replaceItemAt(destination, withItemAt: copy)
		} else {
			try files.moveItem(at: copy, to: destination)
		}
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
