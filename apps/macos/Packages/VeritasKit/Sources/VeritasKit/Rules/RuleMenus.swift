import Foundation
import VeritasCore

/// What the sentence editor's menus offer, from the core's `ruleChoices()`,
/// which ports Actual's rule editor tables. The core still decides whether a
/// draft is valid.
public enum RuleMenus {
	static let choices: RuleChoices? = try? ruleChoices()

	public static var conditionFields: [RuleField] {
		choices?.conditionFields.map(\.field) ?? []
	}

	public static func ops(for field: RuleField) -> [RuleConditionOp] {
		opChoices(field).map(\.op)
	}

	private static func opChoices(_ field: RuleField) -> [RuleOpChoice] {
		choices?.conditionFields.first { $0.field == field }?.ops ?? []
	}

	/// An empty value of the kind the core gives this field and op.
	public static func blank(_ field: RuleField, _ op: RuleConditionOp, today: String) -> RuleValue {
		FilterChoices.blank(opChoices(field).first { $0.op == op }?.value, today: today)
	}

	/// One choice in an action's menu: `set` names its field.
	public enum ActionKind: Hashable, Sendable {
		case set(RuleField)
		case prependNotes
		case appendNotes
		case deleteTransaction

		public init?(_ action: RuleAction) {
			if action.op == .set {
				guard let field = action.field else { return nil }
				self = .set(field)
			} else {
				self.init(op: action.op)
			}
		}

		init?(op: RuleActionOp) {
			switch op {
			case .prependNotes: self = .prependNotes
			case .appendNotes: self = .appendNotes
			case .deleteTransaction: self = .deleteTransaction
			case .set, .setSplitAmount, .linkSchedule: return nil
			}
		}

		var op: RuleActionOp {
			switch self {
			case .set: .set
			case .prependNotes: .prependNotes
			case .appendNotes: .appendNotes
			case .deleteTransaction: .deleteTransaction
			}
		}
	}

	/// The transaction's own actions: `set` on each of its fields, then the other ops, in the core's order.
	public static var actions: [ActionKind] {
		guard let choices else { return [] }
		return choices.actionOps.flatMap { choice -> [ActionKind] in
			choice.op == .set ? choices.actionFields.map { .set($0.field) } : ActionKind(op: choice.op).map { [$0] } ?? []
		}
	}

	public static var splitLineActions: [ActionKind] {
		choices?.splitLineFields.map { .set($0.field) } ?? []
	}

	public static func blank(_ kind: ActionKind, line: UInt32, today: String) -> RuleAction {
		switch kind {
		case .set(let field):
			let fields = line == 0 ? choices?.actionFields : choices?.splitLineFields
			let value = FilterChoices.blank(fields?.first { $0.field == field }?.value, today: today)
			return RuleAction(op: .set, field: field, value: value, splitIndex: line)
		default:
			let value = FilterChoices.blank(choices?.actionOps.first { $0.op == kind.op }?.value, today: today)
			return RuleAction(op: kind.op, field: nil, value: value, splitIndex: line)
		}
	}

	/// How a split line takes its share.
	public enum Allocation: Hashable, Sendable {
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

		init?(_ kind: RuleValueKind) {
			switch kind {
			case .amount: self = .fixedAmount
			case .percent: self = .percent
			case .nothing: self = .remainder
			default: return nil
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

	/// The allocation menu, in the core's order.
	public static var allocations: [Allocation] {
		choices?.splitAmounts.compactMap(Allocation.init) ?? []
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
		guard draft.conditions.indices.contains(index), draft.conditions[index].field != field else { return }
		let ops = RuleMenus.ops(for: field)
		let current = draft.conditions[index].op
		let op = ops.contains(current) ? current : ops.first ?? .is
		draft.conditions[index] = RuleCondition(field: field, op: op, value: RuleMenus.blank(field, op, today: today))
	}

	/// A new op keeps the value when it takes the same kind of value.
	mutating func setOp(_ op: RuleConditionOp, ofCondition index: Int, today: String) {
		guard draft.conditions.indices.contains(index), draft.conditions[index].op != op else { return }
		let condition = draft.conditions[index]
		let blank = RuleMenus.blank(condition.field, op, today: today)
		draft.conditions[index].op = op
		if !blank.isSameKind(as: condition.value) {
			draft.conditions[index].value = blank
		}
	}

	/// The first choice the line doesn't use yet, as Actual adds actions.
	mutating func addAction(line: UInt32, today: String) {
		let choices = line == 0 ? RuleMenus.actions : RuleMenus.splitLineActions
		let used = Set(actionIndices(line: line).compactMap { RuleMenus.ActionKind(draft.actions[$0]) })
		let kind = choices.first { !used.contains($0) } ?? .set(.category)
		let action = RuleMenus.blank(kind, line: line, today: today)
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

	mutating func setKind(_ kind: RuleMenus.ActionKind, ofAction index: Int, today: String) {
		guard draft.actions.indices.contains(index), RuleMenus.ActionKind(draft.actions[index]) != kind else { return }
		draft.actions[index] = RuleMenus.blank(kind, line: draft.actions[index].splitIndex, today: today)
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

	mutating func setAllocation(_ allocation: RuleMenus.Allocation, line: UInt32) {
		guard let index = actionIndices(line: line).first, draft.actions[index].op == .setSplitAmount,
			RuleMenus.Allocation(draft.actions[index].value) != allocation
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
