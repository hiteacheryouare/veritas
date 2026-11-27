import Foundation
import Testing
import VeritasCore
import VeritasKit

/// docs/finance/migration.md MIG-1: the offer after an import, the boxes,
/// and the one Apply.
@MainActor
struct UpgradeModelTests {
	@Test func anImportOffersTheSheetOnceAndRecordsItOutsideUndo() async {
		let library = UpgradeLibrary()
		let model = WorkspaceModel(library: library, format: MoneyFormat())
		await model.importActualZip(URL(filePath: "/tmp/actual.zip"))
		#expect(model.upgrade?.step == .offer)
		#expect(library.session.offerMarks == 1)

		model.upgrade = nil
		await model.reopen(directory: URL(filePath: "/tmp/budget"))
		#expect(model.upgrade == nil, "it never opens by itself again")
		await model.importActualZip(URL(filePath: "/tmp/actual.zip"))
		#expect(model.upgrade == nil, "not even after another import of the same file")

		model.showUpgrade()
		#expect(model.upgrade?.step == .overview, "the File menu opens it at any time")
	}

	@Test func onlyPreTickedItemsStartTickedAndABlockedPairTicksOne() async {
		let (model, _) = await openUpgrade()
		#expect(model.ticked == ["goal:vacation", "bill:netflix"])
		let second = model.visible.first { $0.key == "goal:vacation2" }!
		#expect(!model.isTickable(second))
		#expect(model.blockedReason(second) == "Rename one of them first.")
		model.toggle(model.visible.first { $0.key == "goal:vacation" }!)
		#expect(model.isTickable(second))
		model.toggle(model.visible.first { $0.key == "kind:carloan" }!)
		#expect(model.ticked.contains("kind:carloan"))
		model.selectSuggested()
		#expect(!model.ticked.contains("kind:carloan"), "Select suggested never ticks a shown item")
	}

	@Test func applySendsEveryTickedItemWithItsTargetAndNoteChoice() async {
		let (model, session) = await openUpgrade()
		model.writesNoteInActual.insert("goal:vacation")
		#expect(await model.apply())
		let sent = session.applied.first
		#expect(sent?.items.map(\.key).sorted() == ["bill:netflix", "goal:vacation"])
		let goal = sent?.items.first { $0.key == "goal:vacation" }
		#expect(goal?.edits.target == 300_000)
		#expect(goal?.edits.writeNoteInActual == true)
		#expect(sent?.consentShowGoalsGroup == false)
	}

	@Test func aHiddenGoalsGroupNeedsTheUsersYes() async {
		let session = UpgradeSession()
		session.goalsGroupHidden = true
		let (model, _) = await openUpgrade(session)
		#expect(model.needsGoalsGroupConsent)
		#expect(!model.canApply)
		model.consentShowGoalsGroup = true
		#expect(model.canApply)
	}

	@Test func aKindRevertAsksFirstAndThenWrites() async {
		let (model, session) = await openUpgrade()
		await model.revert("kind:carloan")
		guard case .kind(let key, let change, _, _) = model.question else {
			Issue.record("expected the reversed confirm")
			return
		}
		#expect(key == "kind:carloan")
		#expect(UpgradeWords.kindChange(change, format: MoneyFormat(), reverting: true) == "3 rows will change: 3 transfers of 1,156.50 move from debt paydown back to spending.")
		#expect(session.reverts.isEmpty, "nothing is written until confirmed")
		await model.revert(key, confirmed: true)
		#expect(session.reverts == ["kind:carloan"])
		#expect(model.question?.id == nil)
	}

	@Test func theOverviewCountsEachConversion() async {
		let (model, _) = await openUpgrade()
		#expect(model.counts.map(\.count) == [2, 1, 1])
		#expect(model.counts.map(\.conversion) == [.goal, .kind, .bill])
	}

	private func openUpgrade(_ session: UpgradeSession = UpgradeSession()) async -> (UpgradeModel, UpgradeSession) {
		let library = UpgradeLibrary(session: session)
		let workspace = WorkspaceModel(library: library, format: MoneyFormat())
		await workspace.reopen(directory: URL(filePath: "/tmp/budget"))
		let model = UpgradeModel(workspace: workspace, offering: false, today: "2026-11-22")
		await model.load()
		return (model, session)
	}
}

private func suggestion(
	_ key: String, _ conversion: MigrationConversion, name: String, confidence: MigrationConfidence,
	target: Int64? = nil, blockedBy: String? = nil, asksToShowGoalsGroup: Bool = false
) -> MigrationSuggestion {
	MigrationSuggestion(
		key: key,
		conversion: conversion,
		subjectId: String(key.split(separator: ":").last ?? ""),
		subjectName: name,
		confidence: confidence,
		signals: [],
		proposal: MigrationProposal(target: target, targetDate: nil, kind: nil, displayName: nil, icon: nil, nativeType: nil, nativeMeta: nil),
		conflicts: [],
		blocked: nil,
		blockedBy: blockedBy,
		confirmLines: [],
		effects: [],
		kindChange: nil,
		targetInActual: false,
		asksToShowGoalsGroup: asksToShowGoalsGroup,
		dismissed: false
	)
}

private final class UpgradeSession: BudgetSession, @unchecked Sendable {
	let canExport = true
	var offered = false
	var offerMarks = 0
	var goalsGroupHidden = false
	var applied: [MigrationSelection] = []
	var reverts: [String] = []

	func overview() async throws -> BudgetOverview {
		BudgetOverview(name: "Household", directory: URL(filePath: "/tmp/budget"), readOnly: nil, accountGroups: [])
	}

	func undo() async throws {}
	func redo() async throws {}
	func maintain() async throws {}
	func exportActualZip(to destination: URL) async throws -> [AuditFinding] { [] }
	func observeChanges(_ changed: @escaping @Sendable () -> Void) async throws {}
	func close() async {}

	func migrationSuggestions(today: String) async throws -> [MigrationSuggestion] {
		[
			suggestion("goal:vacation", .goal, name: "Vacation", confidence: .preTicked, target: 300_000, asksToShowGoalsGroup: goalsGroupHidden),
			suggestion("goal:vacation2", .goal, name: "Vacation", confidence: .preTicked, target: 100_000, blockedBy: "goal:vacation"),
			suggestion("kind:carloan", .kind, name: "Car Loan", confidence: .shown),
			suggestion("bill:netflix", .bill, name: "Netflix", confidence: .preTicked)
		]
	}

	func applyMigration(_ selection: MigrationSelection, today: String) async throws {
		applied.append(selection)
	}

	func revertMigration(key: String, groupID: String?, confirmed: Bool, today: String) async throws -> MigrationRevert {
		if key.hasPrefix("kind:"), !confirmed {
			return .confirmKind(change: MigrationKindChange(rows: 3, transfers: 3, amount: 115_650, purchases: 0, kind: .debtPrincipal), effects: [], replaces: nil)
		}
		reverts.append(key)
		return .reverted(change: Change(entities: [], everything: false), notes: [])
	}

	func appliedMigrations() async throws -> [AppliedMigration] { [] }
	func migrationOffered() async throws -> Bool { offered }

	func markMigrationOffered() async throws {
		offered = true
		offerMarks += 1
	}
}

private final class UpgradeLibrary: BudgetLibrary, @unchecked Sendable {
	let session: UpgradeSession

	init(session: UpgradeSession = UpgradeSession()) {
		self.session = session
	}

	func newBudgetDirectory() -> URL { URL(filePath: "/tmp/budgets/\(UUID().uuidString)", directoryHint: .isDirectory) }
	func open(directory: URL) async throws -> any BudgetSession { session }
	func importActualZip(_ zip: URL, into directory: URL) async throws -> any BudgetSession { session }
	func createBudget(_ budget: NewBudget, into directory: URL) async throws -> any BudgetSession { session }
	func currencies() async throws -> [CurrencyOption] { FakeAmounts.currencies }
	func evaluateAmount(_ text: String, style: NumberStyle) async throws -> Int64 { try FakeAmounts.evaluate(text) }
}
