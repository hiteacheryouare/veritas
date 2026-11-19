import Foundation
import VeritasCore

/// A field Actual's filter editor offers, with the ops it takes and the kind
/// of value each op asks for.
public struct FilterChoice: Hashable, Sendable {
	public let field: RuleField
	public let ops: [RuleOpChoice]

	public func valueKind(for op: RuleConditionOp) -> RuleValueKind? {
		ops.first { $0.op == op }?.value
	}
}

/// Report and widget filters (contract v14): the typed conditions of the
/// Rules section, offered as Actual's filter editor offers them.
public enum FilterChoices {
	/// `filterFields` in Actual's FiltersMenu.tsx, with its subfields (category
	/// group, inflow and outflow) as fields of their own, as the contract types them.
	static let order: [RuleField] = [
		.date, .account, .payee, .notes, .category, .categoryGroup, .amount, .amountInflow, .amountOutflow, .cleared, .reconciled, .transfer
	]

	/// The core's rule editor choices, less `isBetween`, which Actual's filter
	/// editor drops. Reconciled and transfer aren't rule editor fields; as
	/// flags they take only `is`.
	public static func make(from rules: RuleChoices) -> [FilterChoice] {
		order.map { field in
			let ops = rules.conditionFields.first { $0.field == field }?.ops.filter { $0.op != .isBetween }
			return FilterChoice(field: field, ops: ops ?? [RuleOpChoice(op: .is, value: .bool)])
		}
	}

	public static let all: [FilterChoice] = (try? ruleChoices()).map(make) ?? []

	public static func choice(for field: RuleField) -> FilterChoice? {
		all.first { $0.field == field }
	}

	/// What the editor adds: notes containing some text, as Actual's menu starts.
	public static let starter = RuleCondition(field: .notes, op: .contains, value: .text(text: ""))

	public static func blank(_ kind: RuleValueKind?, today: String) -> RuleValue {
		switch kind {
		case .id: .id(id: nil)
		case .ids: .ids(ids: [])
		case .text: .text(text: "")
		case .texts: .texts(texts: [])
		case .amount: .amount(amount: 0)
		case .amountRange: .amountRange(num1: 0, num2: 0)
		case .date, .dateMonthOrYear: .date(date: today)
		case .bool: .bool(value: true)
		case .percent: .percent(percent: 0)
		case .nothing, nil: .nothing
		}
	}

	/// Whether `value` is one the kind accepts, so a new op can keep it.
	public static func fits(_ value: RuleValue, _ kind: RuleValueKind?) -> Bool {
		switch (kind, value) {
		case (.id, .id), (.ids, .ids), (.text, .text), (.texts, .texts), (.amount, .amount), (.amountRange, .amountRange),
			(.date, .date), (.dateMonthOrYear, .date), (.dateMonthOrYear, .month), (.dateMonthOrYear, .year),
			(.bool, .bool), (.percent, .percent), (.nothing, .nothing):
			true
		default:
			false
		}
	}

	/// A new field keeps the op when the field takes it, and starts a blank value.
	public static func setting(_ field: RuleField, of condition: RuleCondition, today: String) -> RuleCondition {
		guard let choice = choice(for: field) else { return condition }
		let op = choice.valueKind(for: condition.op) == nil ? (choice.ops.first?.op ?? .is) : condition.op
		return RuleCondition(field: field, op: op, value: blank(choice.valueKind(for: op), today: today))
	}

	/// A new op keeps the value when it takes the same kind.
	public static func setting(_ op: RuleConditionOp, of condition: RuleCondition, today: String) -> RuleCondition {
		let kind = choice(for: condition.field)?.valueKind(for: op)
		let value = fits(condition.value, kind) ? condition.value : blank(kind, today: today)
		return RuleCondition(field: condition.field, op: op, value: value)
	}

	/// The editor shows field, op and value controls only for fields it offers
	/// and values it can show. Anything else, such as a saved filter or a
	/// repeating date, stays as stored and can only be removed.
	public static func isEditable(_ condition: RuleCondition) -> Bool {
		guard let choice = choice(for: condition.field), choice.valueKind(for: condition.op) != nil else { return false }
		if case .raw = condition.value { return false }
		return true
	}
}

public enum FilterWords {
	public static func field(_ field: RuleField) -> String {
		switch field {
		case .importedPayee: "Imported payee"
		case .payee: "Payee"
		case .payeeName: "Payee name"
		case .account: "Account"
		case .category: "Category"
		case .categoryGroup: "Category group"
		case .notes: "Notes"
		case .amount: "Amount"
		case .amountInflow: "Amount (inflow)"
		case .amountOutflow: "Amount (outflow)"
		case .date: "Date"
		case .cleared: "Cleared"
		case .reconciled: "Reconciled"
		case .transfer: "Transfer"
		case .parent: "Split parent"
		case .saved: "Saved filter"
		}
	}

	public static func op(_ op: RuleConditionOp, on field: RuleField) -> String {
		let isDate = field == .date
		return switch op {
		case .is: "is"
		case .isNot: "is not"
		case .contains: "contains"
		case .doesNotContain: "doesn't contain"
		case .matches: "matches"
		case .oneOf: "is one of"
		case .notOneOf: "is not one of"
		case .hasTags: "has tags"
		case .hasAnyTag: "has any tag"
		case .isApprox: "is about"
		case .isBetween: "is between"
		case .gt: isDate ? "is after" : "is more than"
		case .gte: isDate ? "is on or after" : "is at least"
		case .lt: isDate ? "is before" : "is less than"
		case .lte: isDate ? "is on or before" : "is at most"
		case .onBudget: "is on budget"
		case .offBudget: "is off budget"
		case .and: "and"
		}
	}

	/// A condition the editor can't change, read as one line. The core
	/// names no saved filter, so a saved one reads only as that.
	public static func stored(_ condition: RuleCondition) -> String {
		if condition.field == .saved { return "Saved filter" }
		return "\(field(condition.field)) \(op(condition.op, on: condition.field)) \u{2026}"
	}
}
