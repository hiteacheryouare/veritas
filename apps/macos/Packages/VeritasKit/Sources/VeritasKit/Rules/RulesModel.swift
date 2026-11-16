import Foundation
import Observation
import VeritasCore

/// A rule open in the sentence editor.
public struct RuleEditing: Hashable, Sendable {
	/// Nil for a new rule.
	public let id: String?
	public var draft: RuleDraft
	/// The core's `Invalid` message, shown beside the sentence.
	public var problem: String?

	public init(id: String?, draft: RuleDraft, problem: String? = nil) {
		self.id = id
		self.draft = draft
		self.problem = problem
	}

	public var isNew: Bool { id == nil }
	public var isSplit: Bool { draft.actions.contains { $0.splitIndex > 0 } }
}

/// One row "Apply to existing" would change.
public struct RulePreviewRow: Hashable, Identifiable, Sendable {
	public let match: RuleMatch
	/// The register's reconciled flag. Applying moves a locked balance when
	/// this row's amount changes (Fred on #63).
	public let reconciled: Bool

	public var id: String { match.transactionId }

	public var changesAmount: Bool { match.changes.contains { $0.field == .amount } }
}

/// "Apply to existing": the preview, the rows ticked, and the result.
public struct RuleApplying: Hashable, Sendable {
	public let source: RuleSource
	public let rows: [RulePreviewRow]
	public var selected: Set<String>
	public var problem: String?

	public var reconciledRows: [RulePreviewRow] { rows.filter(\.reconciled) }
}

/// The Rules page: every rule as a sentence, the sentence editor and
/// "Apply to existing" (R-RULE-2, R-RULE-3).
@MainActor
@Observable
public final class RulesModel {
	public private(set) var rules: [RuleSentence] = []
	public private(set) var names = BudgetNames.empty
	public var editing: RuleEditing?
	public var applying: RuleApplying?
	/// Set while a preview is being found.
	public private(set) var isPreviewing = false

	@ObservationIgnored let workspace: WorkspaceModel

	public init(workspace: WorkspaceModel) {
		self.workspace = workspace
	}

	public var format: MoneyFormat { workspace.format }
	public var numberStyle: NumberStyle { workspace.numberStyle }
	public var reader: any AmountReader { CoreAmountReader() }
	public var isReadOnly: Bool { workspace.readOnly != nil }

	public func load() async {
		async let names = workspace.budgetNames()
		let loaded = await workspace.report { try await $0.rules() }
		self.names = await names
		if let loaded { rules = loaded }
	}

	/// Actual's new rule: when imported payee contains something, set the category.
	public func startNew() {
		let draft = RuleDraft(
			stage: .default, conditionsOp: .and,
			conditions: [RuleCondition(field: .importedPayee, op: .contains, value: .text(text: ""))],
			actions: [RuleAction(op: .set, field: .category, value: .id(id: nil), splitIndex: 0)]
		)
		editing = RuleEditing(id: nil, draft: draft)
	}

	public func edit(_ rule: RuleSentence) {
		guard case .editable = rule.access else { return }
		editing = RuleEditing(id: rule.id, draft: RuleDraft(stage: rule.stage, conditionsOp: rule.conditionsOp, conditions: rule.conditions, actions: rule.actions))
	}

	public func cancelEditing() {
		editing = nil
	}

	@discardableResult
	public func save() async -> Bool {
		guard let editing else { return false }
		let command: Command = if let id = editing.id {
			.editRule(ruleId: id, draft: editing.draft)
		} else {
			.createRule(draft: editing.draft)
		}
		self.editing?.problem = nil
		let saved = await workspace.perform(command) { self.editing?.problem = $0 }
		if saved {
			self.editing = nil
			await load()
		}
		return saved
	}

	@discardableResult
	public func delete(_ ruleId: String) async -> Bool {
		let deleted = await workspace.perform(.deleteRule(ruleId: ruleId))
		if deleted {
			if editing?.id == ruleId { editing = nil }
			await load()
		}
		return deleted
	}

	/// Finds the rows the rule would change. The editor's draft previews
	/// unsaved; a saved rule previews as stored. Every row starts ticked, as in
	/// Actual, except reconciled ones, which stay pickable (Ryan's ruling).
	public func preview(_ source: RuleSource) async {
		isPreviewing = true
		defer { isPreviewing = false }
		guard let result = await workspace.attempt({ session in
			let matches = try await session.rulePreview(source)
			var reconciled = Set<String>()
			for match in matches {
				if let row = try? await session.registerRow(match.parentId ?? match.transactionId), row.reconciled {
					reconciled.insert(match.transactionId)
				}
			}
			return matches.map { RulePreviewRow(match: $0, reconciled: reconciled.contains($0.transactionId)) }
		}) else { return }
		switch result {
		case .success(let rows):
			applying = RuleApplying(source: source, rows: rows, selected: Set(rows.filter { !$0.reconciled }.map(\.id)))
		case .failure(let error):
			if case .draft = source {
				editing?.problem = ErrorPresentation.coreMessage(error)
			} else {
				workspace.present(error)
			}
		}
	}

	public func toggle(_ rowId: String) {
		guard var applying else { return }
		if applying.selected.contains(rowId) { applying.selected.remove(rowId) } else { applying.selected.insert(rowId) }
		self.applying = applying
	}

	@discardableResult
	public func apply() async -> Bool {
		guard let applying, !applying.selected.isEmpty else { return false }
		let ids = applying.rows.map(\.id).filter(applying.selected.contains)
		self.applying?.problem = nil
		let applied = await workspace.perform(.applyRule(source: applying.source, transactionIds: ids)) { self.applying?.problem = $0 }
		if applied { self.applying = nil }
		return applied
	}

	/// A bill's rule is edited through its bill.
	public func openBill(_ scheduleId: String) {
		workspace.pendingBillID = scheduleId
		workspace.go(to: .screen(.bills))
	}
}
