import Foundation
import VeritasCore

// TEMPORARY. Each property below stands in for a field the core doesn't
// expose yet (requested on PR #61). Each is named exactly like the field it
// waits for, so when `cargo xtask bindings` brings that field in, this
// extension stops compiling and the stand-in gets deleted.

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
