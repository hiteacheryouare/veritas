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

private func importSmall(into scratch: Scratch) async throws -> Session {
	let zip = repoRoot.appending(path: "fixtures/synthetic/small.zip").path
	return try await Session.importActualZip(zip: zip, intoDirectory: scratch.budget)
}

/// The full integrity check runs in the background after opening.
private func settled(_ session: Session) async throws -> SessionState {
	for _ in 0..<400 {
		let state = try session.state()
		if !state.checking { return state }
		try await Task.sleep(for: .milliseconds(25))
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

@Test func importingABudgetOpensItWritableAndSnapshotsIt() async throws {
	let scratch = try Scratch()
	let session = try await importSmall(into: scratch)

	let state = try await settled(session)

	#expect(state.readOnly == nil)
	#expect(!state.canUndo)
	#expect(state.snapshotProblem == nil)
	#expect(state.auditFindings == 0)
	#expect(try session.auditReport() == [])
	#expect(try session.snapshots().map(\.reason) == [.open])
}

@Test func aSecondWindowShowsTheOpenElsewhereBanner() async throws {
	let scratch = try Scratch()
	let first = try await importSmall(into: scratch)
	let second = try await Session.open(directory: scratch.budget)

	#expect(try second.state().readOnly == .openElsewhere)
	#expect {
		try second.perform(command: .renameAccount(accountId: "any", name: "Everyday"))
	} throws: { error in
		guard case FfiError.ReadOnly(let reason) = error else { return false }
		return reason == .openElsewhere
	}
	_ = first
}

@Test func closingABudgetFreesItForTheNextWindow() async throws {
	let scratch = try Scratch()
	let first = try await importSmall(into: scratch)
	_ = try await settled(first)

	try first.close()

	#expect(try await Session.open(directory: scratch.budget).state().readOnly == nil)
	#expect(throws: FfiError.self) { try first.state() }
}

@Test func anUnknownAccountComesBackAsNotFound() async throws {
	let scratch = try Scratch()
	let session = try await importSmall(into: scratch)
	#expect {
		try session.perform(command: .renameAccount(accountId: "no-such-account", name: "Everyday"))
	} throws: { error in
		guard case FfiError.NotFound(let entity, _) = error else { return false }
		return entity == "account"
	}
}

@Test func restoringASnapshotTellsListenersToRefetchEverything() async throws {
	let scratch = try Scratch()
	let session = try await importSmall(into: scratch)
	_ = try await settled(session)
	let recorder = Recorder()
	_ = try session.subscribe(listener: recorder)
	let opened = try #require(try session.snapshots().first)

	let change = try session.restoreSnapshot(id: opened.id)

	#expect(change.everything)
	#expect(recorder.changes == [change])
	#expect(try session.snapshots().map(\.reason).contains(.restore))
}

@Test func theShellGetsAnOverviewAnExportABackupAndTheBudgetList() async throws {
	let scratch = try Scratch()
	let session = try await importSmall(into: scratch)

	let overview = try session.overview()
	#expect(overview.budgetName != nil)
	#expect(overview.groups.map(\.group) == [.cash, .credit, .investments, .loans])

	let zip = scratch.url.appending(path: "export.zip").path
	_ = try await session.exportActualZip(to: zip)
	#expect(FileManager.default.fileExists(atPath: zip))

	let folder = scratch.url.appending(path: "Backups")
	try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
	try session.setBackupFolder(folder: folder.path)
	let file = try session.backUpNow()
	#expect(try session.state().lastBackup?.file == file)

	let listed = try listBudgets(budgetsDir: scratch.url.path)
	#expect(listed.map(\.name) == [overview.budgetName])
}

@Test func creatingABudgetSaysWhenTheBuildCantYet() async throws {
	let scratch = try Scratch()
	let format = NumberFormat(currency: "USD", style: .commaDot, hideDecimals: false)
	let options = NewBudgetOptions(retirement401k: true, hsa: false)
	await #expect {
		try await Session.createBudget(
			name: "Household", directory: scratch.budget, format: format, options: options)
	} throws: { error in
		guard case FfiError.Invalid = error else { return false }
		return true
	}
	#expect(!FileManager.default.fileExists(atPath: scratch.budget))
}
