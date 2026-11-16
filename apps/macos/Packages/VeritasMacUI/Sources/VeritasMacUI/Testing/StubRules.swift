#if DEBUG
import Foundation
import VeritasCore
import VeritasKit

/// The stub budget's rules: editable ones, a split, a bill's rule and one
/// only Actual can edit, plus an "Apply to existing" preview with a reconciled row.
final class StubRules: @unchecked Sendable {
	private let lock = NSLock()
	private var all: [RuleSentence]
	private var created = 0

	init() {
		func set(_ field: RuleField, _ value: RuleValue, line: UInt32 = 0) -> RuleAction {
			RuleAction(op: .set, field: field, value: value, splitIndex: line)
		}
		all = [
			RuleSentence(
				id: "amazon", stage: .pre, conditionsOp: .and,
				conditions: [RuleCondition(field: .importedPayee, op: .contains, value: .text(text: "AMZN"))],
				actions: [set(.payee, .id(id: "amazon")), set(.category, .id(id: "household"))],
				access: .editable, runsInVeritas: true
			),
			RuleSentence(
				id: "coffee", stage: .default, conditionsOp: .or,
				conditions: [
					RuleCondition(field: .importedPayee, op: .oneOf, value: .texts(texts: ["BLUE BOTTLE", "SIGHTGLASS"])),
					RuleCondition(field: .amountOutflow, op: .lt, value: .amount(amount: 1_500))
				],
				actions: [set(.category, .id(id: "dining")), RuleAction(op: .appendNotes, field: nil, value: .text(text: "#coffee"), splitIndex: 0)],
				access: .editable, runsInVeritas: true
			),
			RuleSentence(
				id: "costco", stage: .default, conditionsOp: .and,
				conditions: [RuleCondition(field: .payee, op: .is, value: .id(id: "amazon")), RuleCondition(field: .amount, op: .isBetween, value: .amountRange(num1: -30_000, num2: -10_000))],
				actions: [
					RuleAction(op: .setSplitAmount, field: nil, value: .percent(percent: 60), splitIndex: 1),
					set(.category, .id(id: "groceries"), line: 1),
					RuleAction(op: .setSplitAmount, field: nil, value: .nothing, splitIndex: 2),
					set(.category, .id(id: "household"), line: 2)
				],
				access: .editable, runsInVeritas: true
			),
			RuleSentence(
				id: "rent-rule", stage: .default, conditionsOp: .and,
				conditions: [RuleCondition(field: .payee, op: .is, value: .id(id: "landlord")), RuleCondition(field: .amount, op: .isApprox, value: .amount(amount: -165_000))],
				actions: [RuleAction(op: .linkSchedule, field: nil, value: .id(id: "rent"), splitIndex: 0), set(.category, .id(id: "rent"))],
				access: .bill(scheduleId: "rent"), runsInVeritas: true
			),
			RuleSentence(
				id: "templated", stage: .post, conditionsOp: .and,
				conditions: [RuleCondition(field: .account, op: .is, value: .id(id: "card"))],
				actions: [set(.notes, .raw(json: "\"{{imported_payee}} via card\""))],
				access: .editInActual(reasons: ["a template action"]), runsInVeritas: false
			)
		]
	}

	func rules() -> [RuleSentence] { lock.withLock { all } }

	static let reconciledRow = "tx-reconciled"

	func preview(_ source: RuleSource) throws -> [RuleMatch] {
		let rule: RuleDraft
		switch source {
		case .saved(let id):
			guard let sentence = rules().first(where: { $0.id == id }) else { throw FfiError.NotFound(entity: "rule", id: id) }
			rule = RuleDraft(stage: sentence.stage, conditionsOp: sentence.conditionsOp, conditions: sentence.conditions, actions: sentence.actions)
		case .draft(let draft): rule = draft
		}
		let category = rule.actions.first { $0.field == .category && $0.splitIndex == 0 }?.value ?? .id(id: "household")
		func row(_ id: String, _ date: String, _ amount: Int64, from: String?, notes: String?) -> RuleMatch {
			RuleMatch(
				transactionId: id, parentId: nil, date: date, accountId: "card", amount: amount, payeeId: "amazon", categoryId: from, notes: notes,
				changes: [
					RuleFieldChange(field: .category, before: .id(id: from), after: category),
					id == Self.reconciledRow ? RuleFieldChange(field: .amount, before: .amount(amount: amount), after: .amount(amount: -2_499)) : nil
				].compactMap { $0 },
				splitLines: []
			)
		}
		return [
			row("tx-1", "2026-11-09", -3_412, from: nil, notes: "AMZN Mktp US"),
			row("tx-2", "2026-10-28", -1_899, from: "gifts", notes: nil),
			row(Self.reconciledRow, "2026-09-14", -2_999, from: "hobbies", notes: "AMZN Digital")
		]
	}

	func registerRow(_ id: String) throws -> RegisterRow {
		RegisterRow(
			id: id, accountId: "card", accountName: "Visa", date: "2026-09-14", amount: -2_999, payeeId: "amazon", payeeName: "Amazon",
			categoryId: nil, categoryName: nil, notes: nil, tags: [], cleared: true, reconciled: id == Self.reconciledRow,
			transferAccountId: nil, counterpartId: nil, counterpartParentId: nil, lines: [], splitDifference: nil, startingBalance: false,
			scheduleId: nil, needsActual: nil, runningBalance: nil
		)
	}

	/// Applies a rule command. Returns false for a command that isn't a rule's.
	func perform(_ command: Command) throws -> Bool {
		switch command {
		case .createRule(let draft):
			try Self.validate(draft)
			lock.withLock {
				created += 1
				all.append(RuleSentence(id: "new-rule-\(created)", stage: draft.stage, conditionsOp: draft.conditionsOp, conditions: draft.conditions, actions: draft.actions, access: .editable, runsInVeritas: true))
			}
		case .editRule(let id, let draft):
			try Self.validate(draft)
			try lock.withLock {
				guard let index = all.firstIndex(where: { $0.id == id }) else { throw FfiError.NotFound(entity: "rule", id: id) }
				guard case .editable = all[index].access else { throw FfiError.Invalid(message: "Edit this rule in Actual.") }
				all[index].stage = draft.stage
				all[index].conditionsOp = draft.conditionsOp
				all[index].conditions = draft.conditions
				all[index].actions = draft.actions
			}
		case .deleteRule(let id):
			try lock.withLock {
				if case .bill = all.first(where: { $0.id == id })?.access { throw FfiError.Invalid(message: "A bill uses this rule. End the bill first.") }
				all.removeAll { $0.id == id }
			}
		case .applyRule: break
		default: return false
		}
		return true
	}

	private static func validate(_ draft: RuleDraft) throws {
		for condition in draft.conditions {
			if case .text(let text) = condition.value, text.isEmpty { throw FfiError.Invalid(message: "Type what the \(RuleWords.field(condition.field)) should contain.") }
		}
		if draft.actions.contains(where: { if case .id(nil) = $0.value { return $0.op == .set } else { return false } }) {
			throw FfiError.Invalid(message: "Choose a value for each action.")
		}
	}
}
#endif
