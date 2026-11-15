import Foundation
import VeritasCore

// TEMPORARY. Each property below stands in for a field the core doesn't
// expose yet (requested on PR #61, and on PR #68 for v11). Each is named
// exactly like the field it waits for, so when `cargo xtask bindings` brings
// that field in, this extension stops compiling and the stand-in gets deleted.
// The `WidgetView` ones match every associated value by label, so a new one
// on `.netWorth` or `.spending` breaks them too.

public extension BudgetMonth {
	/// Requested: `BudgetMonth.toBudgetTone: MoneyTone`.
	var toBudgetTone: MoneyTone {
		toBudget < 0 ? .overspent : toBudget == 0 ? .neutral : .income
	}
}

public extension GoalSummary {
	/// Requested: `GoalSummary.tone: MoneyTone`.
	var tone: MoneyTone {
		switch state {
		case .active:
			switch pace {
			case .onTrack, nil: .available
			case .behind, .notAdding: .low
			case .noHistory: .neutral
			}
		case .reached: .available
		case .reachedSpent: available < 0 ? .overspent : .available
		case .closed: .neutral
		}
	}

	/// Requested: `GoalSummary.progressPercent: UInt8`, 0 to 100, for the ring
	/// and the number beside it. Never 100 before the goal is reached.
	var progressPercent: UInt8 {
		if state == .reached || state == .reachedSpent { return 100 }
		guard target > 0, available > 0 else { return 0 }
		return UInt8(min(available * 100 / target, 100))
	}

	/// Requested: `GoalSummary.progressAmount: Int64`, the "X of target" figure.
	/// A goal that was reached reads as reached even once it's spent
	/// (design/mockups/goals.html).
	var progressAmount: Int64 {
		state == .reachedSpent ? target : available
	}
}

public extension WidgetView {
	/// Requested for v11: the net worth card's change over its range, Actual's
	/// `totalChange` (last month less first). Nil with fewer than two months.
	var change: Int64? {
		guard case .netWorth(points: let points) = self, points.count > 1, let first = points.first, let last = points.last else { return nil }
		return last.netWorth - first.netWorth
	}

	/// Requested for v11: the spending card's difference, the compare month
	/// less this month, as Actual's card shows it.
	var difference: Int64? {
		guard case .spending(month: _, spent: let spent, compareMonth: _, compareSpent: let compareSpent) = self else { return nil }
		return compareSpent - spent
	}
}
