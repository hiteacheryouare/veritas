import Foundation
import VeritasCore

extension GoalSummary: Identifiable {}

public extension GoalSummary {
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
