import Foundation
import VeritasCore

// TEMPORARY, until contract v13's `ruleChoices()`. The menus below copy
// Actual's rule editor by hand; once the core hands them over, this file
// gets deleted and the editor reads the core's lists instead.
// Adapted from Actual (MIT): RuleEditor.tsx, loot-core/src/shared/rules.ts and desktop-client/src/util/rule.ts.

/// What the sentence editor's menus offer, copied from Actual's rule editor:
/// `FIELD_INFO` and `TYPE_INFO` in loot-core/src/shared/rules.ts, and
/// `conditionFields`, `getActionFields` and `getSplitActionFields` in
/// RuleEditor.tsx. The core still decides whether a draft is valid.
public enum RuleChoices {
	public static let conditionFields: [RuleField] = [
		.importedPayee, .account, .category, .categoryGroup, .date, .payee, .notes, .amount, .amountInflow, .amountOutflow, .cleared
	]

	/// Actual's `getValidOps`, with inflow and outflow losing `isBetween` as its editor does.
	public static func ops(for field: RuleField) -> [RuleConditionOp] {
		let text: [RuleConditionOp] = [.is, .contains, .matches, .oneOf, .isNot, .doesNotContain, .notOneOf]
		let number: [RuleConditionOp] = [.is, .isApprox, .isBetween, .gt, .gte, .lt, .lte]
		switch field {
		case .importedPayee, .payeeName, .payee, .category, .categoryGroup: return text
		case .account: return text + [.onBudget, .offBudget]
		case .notes: return [.is, .contains, .matches, .isNot, .doesNotContain, .hasTags, .hasAnyTag]
		case .date: return [.is, .isApprox, .gt, .gte, .lt, .lte]
		case .amount: return number
		case .amountInflow, .amountOutflow: return number.filter { $0 != .isBetween }
		case .cleared, .reconciled, .transfer, .parent: return [.is]
		}
	}

	/// An empty value of the kind the contract asks for with this field and op.
	public static func blank(_ field: RuleField, _ op: RuleConditionOp, today: String) -> RuleValue {
		switch op {
		case .onBudget, .offBudget, .and: return .nothing
		case .contains, .doesNotContain, .matches, .hasTags, .hasAnyTag: return .text(text: "")
		case .oneOf, .notOneOf: return holdsIds(field) ? .ids(ids: []) : .texts(texts: [])
		case .isBetween: return .amountRange(num1: 0, num2: 0)
		case .is, .isNot, .isApprox, .gt, .gte, .lt, .lte:
			switch field {
			case .payee, .account, .category, .categoryGroup: return .id(id: nil)
			case .amount, .amountInflow, .amountOutflow: return .amount(amount: 0)
			case .date: return .date(date: today)
			case .cleared, .reconciled, .transfer, .parent: return .bool(value: true)
			case .importedPayee, .payeeName, .notes: return .text(text: "")
			}
		}
	}

	public static func holdsIds(_ field: RuleField) -> Bool {
		[.payee, .account, .category, .categoryGroup].contains(field)
	}

	/// One choice in an action's menu: `set` names its field.
	public enum ActionKind: Hashable, Sendable {
		case set(RuleField)
		case prependNotes
		case appendNotes
		case deleteTransaction

		public init?(_ action: RuleAction) {
			switch action.op {
			case .set:
				guard let field = action.field else { return nil }
				self = .set(field)
			case .prependNotes: self = .prependNotes
			case .appendNotes: self = .appendNotes
			case .deleteTransaction: self = .deleteTransaction
			case .setSplitAmount, .linkSchedule: return nil
			}
		}
	}

	public static let actions: [ActionKind] = [
		.set(.category), .set(.payee), .set(.notes), .set(.cleared), .set(.account), .set(.date), .set(.amount),
		.prependNotes, .appendNotes, .deleteTransaction
	]

	public static let splitLineActions: [ActionKind] = [.set(.category), .set(.payee), .set(.notes)]

	public static func blank(_ kind: ActionKind, line: UInt32, today: String) -> RuleAction {
		switch kind {
		case .set(let field):
			let value: RuleValue = switch field {
			case .payee, .account, .category, .categoryGroup: .id(id: nil)
			case .amount, .amountInflow, .amountOutflow: .amount(amount: 0)
			case .date: .date(date: today)
			case .cleared, .reconciled, .transfer, .parent: .bool(value: true)
			case .importedPayee, .payeeName, .notes: .text(text: "")
			}
			return RuleAction(op: .set, field: field, value: value, splitIndex: line)
		case .prependNotes: return RuleAction(op: .prependNotes, field: nil, value: .text(text: ""), splitIndex: line)
		case .appendNotes: return RuleAction(op: .appendNotes, field: nil, value: .text(text: ""), splitIndex: line)
		case .deleteTransaction: return RuleAction(op: .deleteTransaction, field: nil, value: .nothing, splitIndex: line)
		}
	}

	/// How a split line takes its share, as Actual's allocation menu lists them.
	public enum Allocation: CaseIterable, Hashable, Sendable {
		case fixedAmount
		case percent
		case remainder

		public init(_ value: RuleValue) {
			switch value {
			case .amount: self = .fixedAmount
			case .percent: self = .percent
			default: self = .remainder
			}
		}

		public var blank: RuleValue {
			switch self {
			case .fixedAmount: .amount(amount: 0)
			case .percent: .percent(percent: 0)
			case .remainder: .nothing
			}
		}
	}
}

/// Changing a draft from the editor's controls. Each change keeps the draft's
/// shape (numbered split lines, one allocation each) and leaves validity to the core.
public extension RuleEditing {
	/// The split lines' numbers, in order.
	var splitLines: [UInt32] {
		Array(Set(draft.actions.map(\.splitIndex).filter { $0 > 0 })).sorted()
	}

	/// Where line `line`'s actions sit in `draft.actions`, its allocation first.
	func actionIndices(line: UInt32) -> [Int] {
		let indices = draft.actions.indices.filter { draft.actions[$0].splitIndex == line }
		return indices.filter { draft.actions[$0].op == .setSplitAmount } + indices.filter { draft.actions[$0].op != .setSplitAmount }
	}

	mutating func addCondition() {
		draft.conditions.append(RuleCondition(field: .payee, op: .is, value: .id(id: nil)))
	}

	mutating func removeCondition(at index: Int) {
		guard draft.conditions.indices.contains(index) else { return }
		draft.conditions.remove(at: index)
	}

	mutating func setField(_ field: RuleField, ofCondition index: Int, today: String) {
		guard draft.conditions.indices.contains(index) else { return }
		let ops = RuleChoices.ops(for: field)
		let current = draft.conditions[index].op
		let op = ops.contains(current) ? current : ops.first ?? .is
		draft.conditions[index] = RuleCondition(field: field, op: op, value: RuleChoices.blank(field, op, today: today))
	}

	/// A new op keeps the value when it takes the same kind of value.
	mutating func setOp(_ op: RuleConditionOp, ofCondition index: Int, today: String) {
		guard draft.conditions.indices.contains(index) else { return }
		let condition = draft.conditions[index]
		let blank = RuleChoices.blank(condition.field, op, today: today)
		draft.conditions[index].op = op
		if !blank.isSameKind(as: condition.value) {
			draft.conditions[index].value = blank
		}
	}

	/// The first choice the line doesn't use yet, as Actual adds actions.
	mutating func addAction(line: UInt32, today: String) {
		let choices = line == 0 ? RuleChoices.actions : RuleChoices.splitLineActions
		let used = Set(actionIndices(line: line).compactMap { RuleChoices.ActionKind(draft.actions[$0]) })
		let kind = choices.first { !used.contains($0) } ?? .set(.category)
		let action = RuleChoices.blank(kind, line: line, today: today)
		if let last = actionIndices(line: line).last {
			draft.actions.insert(action, at: last + 1)
		} else {
			draft.actions.append(action)
		}
	}

	mutating func removeAction(at index: Int) {
		guard draft.actions.indices.contains(index), draft.actions[index].op != .setSplitAmount else { return }
		draft.actions.remove(at: index)
	}

	mutating func setKind(_ kind: RuleChoices.ActionKind, ofAction index: Int, today: String) {
		guard draft.actions.indices.contains(index), RuleChoices.ActionKind(draft.actions[index]) != kind else { return }
		draft.actions[index] = RuleChoices.blank(kind, line: draft.actions[index].splitIndex, today: today)
	}

	/// A new line takes an equal share of the remainder, as in Actual.
	mutating func addSplitLine() {
		let line = (splitLines.last ?? 0) + 1
		draft.actions.append(RuleAction(op: .setSplitAmount, field: nil, value: .nothing, splitIndex: line))
	}

	/// Removes the line and numbers the ones after it down, so there are no gaps.
	mutating func removeSplitLine(_ line: UInt32) {
		guard line > 0 else { return }
		draft.actions.removeAll { $0.splitIndex == line }
		for index in draft.actions.indices where draft.actions[index].splitIndex > line {
			draft.actions[index].splitIndex -= 1
		}
	}

	mutating func setAllocation(_ allocation: RuleChoices.Allocation, line: UInt32) {
		guard let index = actionIndices(line: line).first, draft.actions[index].op == .setSplitAmount,
			RuleChoices.Allocation(draft.actions[index].value) != allocation
		else { return }
		draft.actions[index].value = allocation.blank
	}
}

extension RuleValue {
	func isSameKind(as other: RuleValue) -> Bool {
		switch (self, other) {
		case (.id, .id), (.ids, .ids), (.text, .text), (.texts, .texts), (.amount, .amount), (.amountRange, .amountRange),
			(.date, .date), (.month, .month), (.year, .year), (.bool, .bool),
			(.percent, .percent), (.nothing, .nothing), (.raw, .raw):
			true
		default:
			false
		}
	}
}
