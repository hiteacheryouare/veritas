import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// The Rules page through `RulesModel` on fixtures/synthetic/all-features,
/// against the real core (R-RULE-2, R-RULE-3).
@MainActor
struct RealCoreRulesTests {
	typealias F = AllFeatures

	/// Twelve Visa refunds imported as "Hardware Store", all in Household.
	static let hardwareStore = "Hardware Store"
	static let hardwareRefund = "e1645933-d7c0-466a-87c3-466f2f59e4a9"
	static let cornerGrocerRule = "bad57923-5af6-4d51-b6f0-5ebf51614e21"

	private func rules(_ budget: RealBudget) async -> RulesModel {
		let model = RulesModel(workspace: budget.workspace)
		await model.load()
		return model
	}

	private func draft(contains text: String, then actions: [RuleAction]) -> RuleDraft {
		RuleDraft(
			stage: .default, conditionsOp: .and,
			conditions: [RuleCondition(field: .importedPayee, op: .contains, value: .text(text: text))],
			actions: actions
		)
	}

	private func set(_ field: RuleField, _ value: RuleValue, line: UInt32 = 0) -> RuleAction {
		RuleAction(op: .set, field: field, value: value, splitIndex: line)
	}

	private func row(_ budget: RealBudget, _ id: String) async throws -> RegisterRow {
		try await budget.fromDisk { try $0.registerRow(transactionId: id) }
	}

	@Test func rulesArriveInTheOrderActualRunsThemWithEachKindOfAccess() async throws {
		try await RealBudget.with { budget in
			let model = await rules(budget)
			let order: [RuleStage: Int] = [.pre: 0, .default: 1, .post: 2]
			let ranks = model.rules.map { order[$0.stage] ?? -1 }
			#expect(!ranks.isEmpty)
			#expect(ranks == ranks.sorted(), "pre, then default, then post")
			#expect(model.rules.first?.id == Self.cornerGrocerRule)
			#expect(model.rules.contains { $0.access == .editable })
			#expect(model.rules.contains { if case .bill = $0.access { true } else { false } })
			let actualOnly = model.rules.compactMap { if case .editInActual(let reasons) = $0.access { reasons } else { nil } }
			#expect(actualOnly.count == 2, "the template and the formula rules")
			#expect(actualOnly.allSatisfy { !$0.isEmpty })
			#expect(model.names.payees.isEmpty == false, "names load with the rules")
		}
	}

	@Test func aNewRuleIsCreatedAndThenEdited() async throws {
		try await RealBudget.with { budget in
			let model = await rules(budget)
			let before = Set(model.rules.map(\.id))
			model.startNew()
			model.editing?.draft = draft(contains: Self.hardwareStore, then: [set(.category, .id(id: F.gifts))])
			#expect(await model.save(), "\(String(describing: model.editing?.problem)) \(String(describing: budget.workspace.alert))")
			#expect(model.editing == nil)
			let created = try #require(model.rules.first { !before.contains($0.id) })
			#expect(created.access == .editable)
			#expect(created.conditions == [RuleCondition(field: .importedPayee, op: .contains, value: .text(text: Self.hardwareStore))])

			model.edit(created)
			model.editing?.draft.stage = .post
			model.editing?.addAction(line: 0, today: WorkspaceModel.today())
			let added = try #require(model.editing?.draft.actions.last)
			#expect(added.field == .payee, "the first action Actual offers that the rule doesn't use yet")
			model.editing?.setKind(.appendNotes, ofAction: 1, today: WorkspaceModel.today())
			model.editing?.draft.actions[1].value = .text(text: " #hardware")
			#expect(await model.save(), "\(String(describing: model.editing?.problem))")
			let edited = try #require(model.rules.first { $0.id == created.id })
			#expect(edited.stage == .post)
			#expect(edited.actions.map(\.op) == [.set, .appendNotes])
			#expect(model.rules.last?.stage == .post)
		}
	}

	@Test func aDraftPreviewsAndAppliesOnlyToTheTickedRows() async throws {
		try await RealBudget.with { budget in
			let model = await rules(budget)
			model.startNew()
			model.editing?.draft = draft(contains: Self.hardwareStore, then: [set(.category, .id(id: F.gifts))])
			await model.preview(.draft(draft: try #require(model.editing?.draft)))
			let applying = try #require(model.applying, "\(String(describing: model.editing?.problem))")
			#expect(applying.rows.count == 12)
			#expect(applying.selected.count == 12, "every unreconciled row starts ticked")
			let first = try #require(applying.rows.first)
			#expect(first.match.changes == [RuleFieldChange(field: .category, before: .id(id: F.household), after: .id(id: F.gifts))])

			let chosen = applying.rows.prefix(2).map(\.id)
			for row in applying.rows.dropFirst(2) { model.toggle(row.id) }
			#expect(model.applying?.selected == Set(chosen))
			#expect(await model.apply(), "\(String(describing: model.applying?.problem))")
			#expect(model.applying == nil)
			#expect(model.editing != nil, "applying leaves the draft open and unsaved")

			for id in chosen {
				#expect(try await row(budget, id).categoryId == F.gifts)
			}
			let untouched = try #require(applying.rows.last)
			#expect(try await row(budget, untouched.id).categoryId == F.household)
		}
	}

	@Test func aSplitRuleIsSavedAndAppliedAsSplitLines() async throws {
		try await RealBudget.with { budget in
			let model = await rules(budget)
			let before = Set(model.rules.map(\.id))
			model.startNew()
			var editing = try #require(model.editing)
			editing.draft = draft(contains: "Trattoria", then: [])
			editing.addSplitLine()
			editing.setAllocation(.percent, line: 1)
			editing.draft.actions[0].value = .percent(percent: 50)
			editing.addAction(line: 1, today: WorkspaceModel.today())
			editing.draft.actions[1].value = .id(id: F.diningOut)
			editing.addSplitLine()
			editing.addAction(line: 2, today: WorkspaceModel.today())
			editing.draft.actions[3].value = .id(id: F.funMoney)
			#expect(editing.splitLines == [1, 2])
			#expect(editing.isSplit)
			model.editing = editing
			#expect(await model.save(), "\(String(describing: model.editing?.problem))")
			let split = try #require(model.rules.first { !before.contains($0.id) })

			await model.preview(.saved(ruleId: split.id))
			let applying = try #require(model.applying)
			let target = try #require(applying.rows.first)
			#expect(target.match.splitLines.count == 2)
			#expect(target.match.splitLines.map(\.categoryId) == [F.diningOut, F.funMoney])
			for row in applying.rows.dropFirst() { model.toggle(row.id) }
			#expect(await model.apply(), "\(String(describing: model.applying?.problem))")

			let parent = try await row(budget, target.id)
			#expect(parent.lines.count == 2)
			#expect(parent.lines.map(\.categoryId) == [F.diningOut, F.funMoney])
			#expect(parent.lines.map(\.amount).reduce(0, +) == parent.amount)
		}
	}

	@Test func anEmptyContainsLandsBesideTheSentence() async throws {
		try await RealBudget.with { budget in
			let model = await rules(budget)
			let count = model.rules.count
			model.startNew()
			model.editing?.draft.actions[0].value = .id(id: F.gifts)
			#expect(await model.save() == false)
			#expect(model.editing?.problem?.isEmpty == false)
			#expect(budget.workspace.alert == nil, "an Invalid draft is shown inline, not as an alert")
			#expect(model.rules.count == count)
		}
	}

	@Test func aBillsRuleCantBeDeleted() async throws {
		try await RealBudget.with { budget in
			let model = await rules(budget)
			let billRule = try #require(model.rules.first { if case .bill = $0.access { true } else { false } })
			#expect(await model.delete(billRule.id) == false)
			#expect(model.rules.contains { $0.id == billRule.id })
		}
	}

	private func reconcileHardwareRefund(_ budget: RealBudget) async throws {
		let directory = try budget.directory
		await budget.workspace.closeBudget()
		try ActualDatabase.execute(
			directory.appending(path: "db.sqlite"),
			"UPDATE transactions SET reconciled = 1, cleared = 1 WHERE id = '\(Self.hardwareRefund)'"
		)
		await budget.workspace.reopen(directory: directory)
		try #require(budget.workspace.phase == .open)
	}

	@Test func aReconciledRowIsFlaggedAndLeftUnticked() async throws {
		try await RealBudget.with { budget in
			try await reconcileHardwareRefund(budget)
			let model = await rules(budget)
			model.startNew()
			model.editing?.draft = draft(contains: Self.hardwareStore, then: [set(.amount, .amount(amount: 1_000))])
			await model.preview(.draft(draft: try #require(model.editing?.draft)))
			let applying = try #require(model.applying, "\(String(describing: model.editing?.problem))")
			let locked = try #require(applying.rows.first { $0.id == Self.hardwareRefund })
			#expect(locked.reconciled)
			#expect(locked.movesReconciledBalance, "applying would move a reconciled balance")
			#expect(applying.reconciledRows.map(\.id) == [Self.hardwareRefund])
			#expect(!applying.selected.contains(Self.hardwareRefund))
			#expect(applying.selected.count == applying.rows.count - 1)

			model.toggle(Self.hardwareRefund)
			#expect(model.applying?.selected.contains(Self.hardwareRefund) == true, "a reconciled row stays pickable")
		}
	}

	@Test func movingAReconciledRowToAnotherAccountIsFlagged() async throws {
		try await RealBudget.with { budget in
			try await reconcileHardwareRefund(budget)
			let model = await rules(budget)
			model.startNew()
			model.editing?.draft = draft(contains: Self.hardwareStore, then: [set(.account, .id(id: F.checking))])
			await model.preview(.draft(draft: try #require(model.editing?.draft)))
			let applying = try #require(model.applying, "\(String(describing: model.editing?.problem))")
			let locked = try #require(applying.rows.first { $0.id == Self.hardwareRefund })
			#expect(locked.match.changes.map(\.field) == [.account])
			#expect(locked.movesReconciledBalance, "moving it out of its account changes that account's reconciled balance")
			#expect(applying.rows.filter { $0.id != Self.hardwareRefund }.allSatisfy { !$0.movesReconciledBalance })
		}
	}
}
