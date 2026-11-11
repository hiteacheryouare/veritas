import Foundation
import VeritasCore

final class CoreBudgetSession: BudgetSession {
	private let session: Session
	private let listener = ChangeRelay()

	init(_ session: Session) {
		self.session = session
	}

	let canExport = false

	func overview() async throws -> BudgetOverview {
		let session = session
		let state = try await Core.run { try session.state() }
		return BudgetOverview(
			name: nil,
			directory: URL(filePath: state.directory, directoryHint: .isDirectory),
			readOnly: state.readOnly,
			canUndo: state.canUndo,
			canRedo: state.canRedo,
			isChecking: state.checking,
			accountGroups: []
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

	func exportActualZip(to destination: URL) async throws {
		throw Core.missing("Session.exportActualZip")
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
