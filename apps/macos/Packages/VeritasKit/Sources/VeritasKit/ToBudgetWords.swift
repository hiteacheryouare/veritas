import Foundation
import VeritasCore

/// The To Budget popover's words, written from the core's figures
/// (docs/finance/to-budget.md TB-1.3, TB-2.3, TB-2.6, TB-3.5, TB-4.3, TB-4.5).
/// Nothing here computes a figure; it only formats the read's.
public enum ToBudgetWords {
	public enum Counting: Sendable {
		case envelopes
		case goals
	}

	/// TB-1.1: Actual's label.
	public static func label(_ state: ToBudgetState) -> String {
		state == .overbudgeted ? "Overbudgeted:" : "To Budget:"
	}

	/// Why a row can't act (TB-1.3, TB-1.4, TB-2.6).
	public static func reason(_ block: ToBudgetBlock, format: MoneyFormat) -> String {
		switch block {
		case .zero:
			"To Budget is \(format.currency(0, on: .budget))"
		case .overbudgeted(let amount):
			"Overbudgeted by \(format.currency(amount, on: .budget)). Move budget from an envelope to cover it."
		case .nothingUnderfunded:
			"Nothing is underfunded"
		case .noGoalNeedsFunding:
			"No active goal needs funding"
		case .notCurrentMonth:
			"Goals are funded for the current month only."
		case .mixedScale(let places):
			"Spending is recorded at 2 places and budgets at \(places), so To Budget can't be trusted here."
		}
	}

	/// A plan's subtitle: "3 envelopes, $300.00" (TB-3.5) or "2 goals, $137.50"
	/// (TB-4.5), or the reason the row is disabled.
	public static func subtitle(_ plan: ToBudgetPlanView, counting: Counting, format: MoneyFormat) -> String {
		if let block = plan.block {
			return reason(block, format: format)
		}
		return "\(count(plan, counting)), \(format.currency(plan.total, on: .budget))"
	}

	/// The notes under a plan: hidden envelopes left out (TB-2.3) and goals
	/// skipped (TB-4.3).
	public static func notes(_ plan: ToBudgetPlanView) -> [String] {
		var notes: [String] = []
		if plan.hiddenLeftOut == 1 {
			notes.append("1 hidden envelope isn't included")
		} else if plan.hiddenLeftOut > 1 {
			notes.append("\(plan.hiddenLeftOut) hidden envelopes aren't included")
		}
		let skipped = plan.skipped.filter { $0.reason == .noTargetDate }.count
		if skipped == 1 {
			notes.append("1 goal skipped: no target date")
		} else if skipped > 1 {
			notes.append("\(skipped) goals skipped: no target date")
		}
		return notes
	}

	/// One line of a plan's list: the amount, and "of" what was needed when
	/// To Budget ran out first (TB-8 example 4: "$100.00 of $137.50").
	public static func fill(_ fill: ToBudgetFill, format: MoneyFormat) -> String {
		let amount = format.currency(fill.amount, on: .budget)
		return fill.amount < fill.need ? "\(amount) of \(format.currency(fill.need, on: .budget))" : amount
	}

	/// TB-2.4's result line: "Assigned $412.00 to 3 envelopes", from what the
	/// command wrote.
	public static func result(assigned: Int64, count: Int, counting: Counting, format: MoneyFormat) -> String {
		let noun = switch counting {
		case .envelopes: count == 1 ? "envelope" : "envelopes"
		case .goals: count == 1 ? "goal" : "goals"
		}
		return "Assigned \(format.currency(assigned, on: .budget)) to \(count) \(noun)"
	}

	/// Hold's subtitle: how much it can hold, or why it can't.
	public static func hold(_ hold: ToBudgetHold, format: MoneyFormat) -> String {
		if let block = hold.block {
			return reason(block, format: format)
		}
		return "Up to \(format.currency(hold.defaultAmount, on: .budget)) into next month's To Budget"
	}

	/// Reset next month's buffer's subtitle.
	public static func reset(_ hold: ToBudgetHold, format: MoneyFormat) -> String {
		"\(format.currency(hold.held, on: .budget)) held goes back to To Budget"
	}

	private static func count(_ plan: ToBudgetPlanView, _ counting: Counting) -> String {
		switch counting {
		case .envelopes:
			plan.envelopes == 1 ? "1 envelope" : "\(plan.envelopes) envelopes"
		case .goals:
			plan.fills.count == 1 ? "1 goal" : "\(plan.fills.count) goals"
		}
	}
}
