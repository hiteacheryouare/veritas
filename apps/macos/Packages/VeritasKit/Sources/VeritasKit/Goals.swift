import Foundation
import VeritasCore

extension GoalSummary: Identifiable {}

public extension GoalSummary {
	/// How full the ring is: available over target, 0 to 1.
	var progress: Double {
		guard target > 0 else { return 0 }
		return min(max(Double(available) / Double(target), 0), 1)
	}

	/// A goal that was reached reads as reached on its card, even once it's
	/// spent: a full ring and the target as its progress (design/mockups/goals.html).
	var wasReached: Bool {
		state == .reached || state == .reachedSpent
	}

	var ringProgress: Double {
		wasReached ? 1 : progress
	}

	var shownProgress: Int64 {
		state == .reachedSpent ? target : available
	}

	var targetMonth: Month? {
		guard let targetDate, let month = try? parseMonth(text: String(targetDate.prefix(7))) else { return nil }
		return month
	}

	var isOpen: Bool {
		state != .closed
	}

	/// Re-targeting needs an open goal; a closed one is reopened first.
	var canRetarget: Bool {
		state == .active || state == .reached
	}
}
