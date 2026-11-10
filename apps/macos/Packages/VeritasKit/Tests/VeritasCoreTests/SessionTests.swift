import Foundation
import Testing
import VeritasCore

private let repoRoot = URL(fileURLWithPath: #filePath)
	.deletingLastPathComponent()
	.deletingLastPathComponent()
	.deletingLastPathComponent()
	.deletingLastPathComponent()
	.deletingLastPathComponent()
	.deletingLastPathComponent()
	.deletingLastPathComponent()

private final class Scratch {
	let url: URL

	init() throws {
		url = FileManager.default.temporaryDirectory.appending(path: "veritas-\(UUID().uuidString)")
		try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
	}

	var budget: String { url.appending(path: "budget").path }

	deinit {
		try? FileManager.default.removeItem(at: url)
	}
}

private func importSmall(into scratch: Scratch) throws -> Session {
	let zip = repoRoot.appending(path: "fixtures/synthetic/small.zip").path
	return try Session.importActualZip(zip: zip, intoDirectory: scratch.budget)
}

/// The full integrity check runs in the background after opening.
private func settled(_ session: Session) throws -> SessionState {
	for _ in 0..<400 {
		let state = try session.state()
		if !state.checking { return state }
		Thread.sleep(forTimeInterval: 0.025)
	}
	Issue.record("the background check never finished")
	return try session.state()
}

private final class Recorder: ChangeListener, @unchecked Sendable {
	private let lock = NSLock()
	private var heard: [Change] = []

	func changed(change: Change) {
		lock.withLock { heard.append(change) }
	}

	var changes: [Change] { lock.withLock { heard } }
}

@Test func importingABudgetOpensItWritableAndSnapshotsIt() throws {
	let scratch = try Scratch()
	let session = try importSmall(into: scratch)

	let state = try settled(session)

	#expect(state.readOnly == nil)
	#expect(!state.canUndo)
	#expect(state.snapshotProblem == nil)
	#expect(state.auditFindings == 0)
	#expect(try session.auditReport() == [])
	#expect(try session.snapshots().map(\.reason) == [.open])
}

@Test func aSecondWindowShowsTheOpenElsewhereBanner() throws {
	let scratch = try Scratch()
	let first = try importSmall(into: scratch)
	let second = try Session.open(directory: scratch.budget)

	#expect(try second.state().readOnly == .openElsewhere)
	#expect {
		try second.perform(command: .renameAccount(accountId: "any", name: "Everyday"))
	} throws: { error in
		guard case FfiError.ReadOnly(let reason) = error else { return false }
		return reason == .openElsewhere
	}
	_ = first
}

@Test func closingABudgetFreesItForTheNextWindow() throws {
	let scratch = try Scratch()
	let first = try importSmall(into: scratch)
	_ = try settled(first)

	try first.close()

	#expect(try Session.open(directory: scratch.budget).state().readOnly == nil)
	#expect(throws: FfiError.self) { try first.state() }
}

@Test func anUnknownAccountComesBackAsNotFound() throws {
	let scratch = try Scratch()
	let session = try importSmall(into: scratch)
	#expect {
		try session.perform(command: .renameAccount(accountId: "no-such-account", name: "Everyday"))
	} throws: { error in
		guard case FfiError.NotFound(let entity, _) = error else { return false }
		return entity == "account"
	}
}

@Test func restoringASnapshotTellsListenersToRefetchEverything() throws {
	let scratch = try Scratch()
	let session = try importSmall(into: scratch)
	_ = try settled(session)
	let recorder = Recorder()
	_ = try session.subscribe(listener: recorder)
	let opened = try #require(try session.snapshots().first)

	let change = try session.restoreSnapshot(id: opened.id)

	#expect(change.everything)
	#expect(recorder.changes == [change])
	#expect(try session.snapshots().map(\.reason).contains(.restore))
}
