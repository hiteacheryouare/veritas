import Foundation
import VeritasCore

// Adapted from Actual (MIT): desktop-client/src/util/rule.ts.

/// The words of a rule sentence, as Actual's rules page reads them
/// (desktop-client/src/util/rule.ts).
public enum RuleWords {
	public static func field(_ field: RuleField) -> String {
		switch field {
		case .importedPayee: "imported payee"
		case .payee: "payee"
		case .payeeName: "payee (name)"
		case .account: "account"
		case .category: "category"
		case .categoryGroup: "category group"
		case .notes: "notes"
		case .amount: "amount"
		case .amountInflow: "amount (inflow)"
		case .amountOutflow: "amount (outflow)"
		case .date: "date"
		case .cleared: "cleared"
		case .reconciled: "reconciled"
		case .transfer: "transfer"
		case .parent: "parent"
		case .saved: "saved filter"
		}
	}

	public static func op(_ op: RuleConditionOp, on field: RuleField) -> String {
		let isDate = field == .date
		switch op {
		case .is: return "is"
		case .isNot: return "is not"
		case .contains: return "contains"
		case .doesNotContain: return "does not contain"
		case .matches: return "matches"
		case .oneOf: return "one of"
		case .notOneOf: return "not one of"
		case .hasTags: return "has all tags"
		case .hasAnyTag: return "has any tag"
		case .isApprox: return "is approx"
		case .isBetween: return "is between"
		case .gt: return isDate ? "is after" : "is greater than"
		case .gte: return isDate ? "is after or equals" : "is greater than or equals"
		case .lt: return isDate ? "is before" : "is less than"
		case .lte: return isDate ? "is before or equals" : "is less than or equals"
		case .onBudget: return "is on budget"
		case .offBudget: return "is off budget"
		case .and: return "and"
		}
	}

	public static func action(_ op: RuleActionOp) -> String {
		switch op {
		case .set: "set"
		case .prependNotes: "prepend to notes"
		case .appendNotes: "append to notes"
		case .linkSchedule: "link schedule"
		case .deleteTransaction: "delete transaction"
		case .setSplitAmount: "allocate"
		}
	}

	public static func action(_ kind: RuleMenus.ActionKind) -> String {
		switch kind {
		case .set(let field): "set \(Self.field(field))"
		case .prependNotes: action(RuleActionOp.prependNotes)
		case .appendNotes: action(RuleActionOp.appendNotes)
		case .deleteTransaction: action(RuleActionOp.deleteTransaction)
		}
	}

	/// Actual's allocation menu (desktop-client/src/util/rule.ts).
	public static func allocation(_ allocation: RuleMenus.Allocation) -> String {
		switch allocation {
		case .fixedAmount: "a fixed amount"
		case .percent: "a fixed percent of the remainder"
		case .remainder: "an equal portion of the remainder"
		}
	}

	/// A condition as the rules list reads it: `imported payee contains "AMZN"`.
	public static func condition(_ condition: RuleCondition, names: BudgetNames, format: MoneyFormat) -> String {
		let words = "\(field(condition.field)) \(op(condition.op, on: condition.field))"
		let shown = value(condition.value, on: condition.field, names: names, format: format)
		return shown.isEmpty ? words : "\(words) \(shown)"
	}

	/// An action as the rules list reads it, after Actual's ActionExpression.
	public static func action(_ action: RuleAction, names: BudgetNames, format: MoneyFormat) -> String {
		switch action.op {
		case .set:
			let field = action.field
			return "set \(field.map { Self.field($0) } ?? "") to \(value(action.value, on: field, names: names, format: format))"
		case .prependNotes, .appendNotes:
			return "\(Self.action(action.op)) \(value(action.value, on: .notes, names: names, format: format))"
		case .deleteTransaction, .linkSchedule:
			return Self.action(action.op)
		case .setSplitAmount:
			let allocation = RuleMenus.Allocation(action.value)
			let words = "\(Self.action(action.op)) \(Self.allocation(allocation))"
			return allocation == .remainder ? words : "\(words): \(value(action.value, on: .amount, names: names, format: format))"
		}
	}

	/// The Rules page's group headers, in the order Actual runs the stages.
	public static func stage(_ stage: RuleStage) -> String {
		switch stage {
		case .pre: "Before"
		case .default: "Default"
		case .post: "After"
		}
	}

	public static func changed(_ field: RuleChangedField) -> String {
		switch field {
		case .account: "Account"
		case .date: "Date"
		case .amount: "Amount"
		case .payee: "Payee"
		case .category: "Category"
		case .notes: "Notes"
		case .importedPayee: "Imported payee"
		case .cleared: "Cleared"
		case .reconciled: "Reconciled"
		case .schedule: "Bill"
		case .deleted: "Deleted"
		}
	}

	/// One side of an apply preview's change. An empty payee or category
	/// reads "No payee" or "No category", so a line reads "No category → Household".
	public static func change(_ value: RuleValue, on changed: RuleChangedField, names: BudgetNames, format: MoneyFormat) -> String {
		if isEmpty(value) {
			switch changed {
			case .payee: return "No payee"
			case .category: return "No category"
			case .notes: return "No notes"
			case .importedPayee: return "No imported payee"
			case .schedule: return "No bill"
			default: return "nothing"
			}
		}
		let words = Self.value(value, on: field(of: changed), names: names, format: format)
		return words.isEmpty ? "nothing" : words
	}

	private static func isEmpty(_ value: RuleValue) -> Bool {
		switch value {
		case .id(.none), .nothing: true
		case .text(let text): text.isEmpty
		default: false
		}
	}

	/// The field a changed column names, so its ids read as names.
	public static func field(of changed: RuleChangedField) -> RuleField? {
		switch changed {
		case .account: .account
		case .date: .date
		case .amount: .amount
		case .payee: .payee
		case .category: .category
		case .notes: .notes
		case .importedPayee: .importedPayee
		case .cleared: .cleared
		case .reconciled: .reconciled
		case .schedule, .deleted: nil
		}
	}

	/// A value as the sentence shows it. Ids read as names.
	public static func value(_ value: RuleValue, on field: RuleField?, names: BudgetNames, format: MoneyFormat) -> String {
		switch value {
		case .id(let id):
			guard let id else { return "nothing" }
			return name(id, on: field, names: names) ?? id
		case .ids(let ids): return ListFormatter.localizedString(byJoining: ids.map { name($0, on: field, names: names) ?? $0 })
		case .text(let text): return "\u{201C}\(text)\u{201D}"
		case .texts(let texts): return ListFormatter.localizedString(byJoining: texts.map { "\u{201C}\($0)\u{201D}" })
		case .amount(let amount): return format.amount(amount, on: .register)
		case .amountRange(let first, let second): return "\(format.amount(first, on: .register)) and \(format.amount(second, on: .register))"
		case .date(let date): return BillWords.day(date)
		case .month(let month): return Month(date: month).map(ReportPeriod.monthYear) ?? month
		case .year(let year): return String(year)
		case .bool(let flag): return flag ? "true" : "false"
		case .percent(let percent): return (percent / 100).formatted(.percent.precision(.fractionLength(0...2)))
		case .nothing: return ""
		case .raw(let json): return json
		}
	}

	private static func name(_ id: String, on field: RuleField?, names: BudgetNames) -> String? {
		switch field {
		case .payee: names.payee(id)
		case .account: names.account(id)
		case .category: names.category(id)
		case .categoryGroup: names.categoryGroup(id)
		default: nil
		}
	}
}
