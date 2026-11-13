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
	return try await Session.importActualZip(
		zip: zip, intoDirectory: scratch.budget, cancel: CancelToken())
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
	let second = try await Session.open(directory: scratch.budget, cancel: CancelToken())

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

	#expect(try await Session.open(directory: scratch.budget, cancel: CancelToken()).state().readOnly == nil)
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
	_ = try await session.exportActualZip(to: zip, cancel: CancelToken())
	#expect(FileManager.default.fileExists(atPath: zip))

	let folder = scratch.url.appending(path: "Backups")
	try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
	try session.setBackupFolder(folder: folder.path)
	let file = try session.backUpNow()
	#expect(try session.state().lastBackup?.file == file)

	let listed = try listBudgets(budgetsDir: scratch.url.path)
	#expect(listed.map(\.name) == [overview.budgetName])
}

@Test func creatingABudgetStartsItWithItsAccounts() async throws {
	let scratch = try Scratch()
	let format = NumberFormat(currency: "USD", style: .commaDot, hideDecimals: false)
	let options = NewBudgetOptions(
		retirement401k: true, hsa: false,
		accounts: [
			NewAccount(name: "Checking", kind: .checking, startingBalance: 250_000),
			NewAccount(name: "Visa", kind: .creditCard, startingBalance: 45_000),
		])

	let session = try await Session.createBudget(
		name: "Household", directory: scratch.budget, format: format, options: options,
		cancel: CancelToken())

	#expect(try session.state().readOnly == nil)
	let overview = try session.overview()
	#expect(overview.budgetName == "Household")
	let credit = try #require(overview.groups.first { $0.group == .credit })
	#expect(credit.accounts.map(\.balance) == [-45_000])
	let investments = try #require(overview.groups.first { $0.group == .investments })
	#expect(investments.accounts.map(\.name) == ["401(k)"])
}

@Test func setupListsActualsCurrenciesAndAmountFieldsDoArithmetic() throws {
	let all = try currencies()
	#expect(all.count == 55)
	#expect(all.first { $0.code == "USD" }?.symbol == "$")
	#expect(try evaluateAmount(text: "12.50+3", style: .commaDot) == 1_550)
	#expect(try evaluateAmount(text: "1.234,5", style: .dotComma) == 123_450)
	#expect(throws: FfiError.self) { try evaluateAmount(text: "abc", style: .commaDot) }
}

@Test func categoriesAndGoalsAreEditedThroughCommands() async throws {
	let scratch = try Scratch()
	let session = try await importSmall(into: scratch)
	_ = try await settled(session)
	let today = "2026-11-15"
	let november = Month(year: 2026, month: 11)

	let group = try session.perform(command: .createCategoryGroup(name: "Travel"))
	let groupId = try #require(group.entities.first { $0.kind == .categoryGroup }?.id)
	let category = try session.perform(command: .createCategory(groupId: groupId, name: "Flights"))
	let categoryId = try #require(category.entities.first { $0.kind == .category }?.id)
	#expect(
		try session.categoryHistory(categoryId: categoryId)
			== CategoryHistory(transactions: 0, budgetedMonths: 0))
	_ = try session.perform(command: .deleteCategory(categoryId: categoryId))

	_ = try session.goals(today: today)
	let created = try session.perform(
		command: .createGoal(
			name: "New laptop", target: 100_000, targetDate: "2027-06-30", icon: nil, color: nil))
	let goalId = try #require(created.entities.first { $0.kind == .goal }?.id)
	let goal = try #require(try session.goals(today: today).first { $0.id == goalId })
	#expect(goal.state == .active)
	#expect(goal.monthsLeft == 8)

	_ = try session.perform(
		command: .setBudget(month: november, categoryId: goal.categoryId, amount: 100_000))
	#expect(try session.goals(today: today).first { $0.id == goalId }?.state == .reached)

	let preview = try session.goalClosePreview(
		goalId: goalId, releaseToCategoryId: nil, today: today)
	#expect(preview.leftover == 100_000)
	_ = try session.perform(
		command: .closeGoal(goalId: goalId, releaseToCategoryId: nil, alsoMoveLater: false))
	#expect(try session.goals(today: today).first { $0.id == goalId }?.state == .closed)

	_ = try session.perform(command: .retargetGoal(goalId: goalId, target: 150_000, targetDate: nil))
	let retargeted = try #require(try session.goals(today: today).first { $0.id == goalId })
	#expect(retargeted.state == .active)
	#expect(retargeted.reachedMonth == november)
	#expect(retargeted.target == 150_000)

	#expect(try evaluateBudgetAmount(text: "5000", style: .commaDot, decimalPlaces: 0) == 5_000)
	#expect(try evaluateBudgetAmount(text: "12.50+3", style: .commaDot, decimalPlaces: 2) == 1_550)
}

/// What VeritasKit's wrapper does: the Swift task's cancellation reaches the
/// core through the token, because UniFFI never cancels the Rust side itself.
private func importCancellably(zip: String, into directory: String) async throws -> Session {
	let token = CancelToken()
	return try await withTaskCancellationHandler {
		try await Session.importActualZip(zip: zip, intoDirectory: directory, cancel: token)
	} onCancel: {
		token.cancel()
	}
}

@Test func cancellingAnImportMidFlightLeavesNothingBehind() async throws {
	let scratch = try Scratch()
	let zip = repoRoot.appending(path: "fixtures/synthetic/perf-100k.zip").path
	let task = Task { try await importCancellably(zip: zip, into: scratch.budget) }
	try await Task.sleep(for: .milliseconds(20))

	task.cancel()

	await #expect {
		_ = try await task.value
	} throws: { error in
		guard case FfiError.Cancelled = error else { return false }
		return true
	}
	let left = try FileManager.default.contentsOfDirectory(atPath: scratch.url.path)
	#expect(left.isEmpty, "left behind: \(left)")
}
