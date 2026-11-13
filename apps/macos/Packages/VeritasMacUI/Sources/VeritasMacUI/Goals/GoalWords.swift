import Foundation
import VeritasCore
import VeritasDesign
import VeritasKit

/// What a goal card says about where a goal stands (design/mockups/goals.html).
struct GoalWords {
	let note: String
	let tone: VeritasDesign.MoneyTone
	let detail: String?

	init(_ goal: GoalSummary, format: MoneyFormat) {
		switch goal.state {
		case .active:
			if let monthly = goal.suggestedMonthly, let month = goal.targetMonth {
				note = "Budget \(format.wholeCurrency(monthly)) a month"
				detail = "Target \(GoalWords.monthYear(month))"
			} else {
				note = "No target date"
				detail = nil
			}
			tone = .available
		case .reached:
			note = "Reached"
			tone = .available
			detail = nil
		case .reachedSpent:
			let over = goal.available < 0
			note = over ? "Reached → Spent, overspent" : "Reached → Spent"
			tone = over ? .overspent : .available
			let left = over ? "\(format.currency(goal.available)) over" : "\(format.currency(goal.available)) left"
			detail = "\(format.currency(goal.spent)) spent, \(left)"
		case .closed:
			note = goal.closedMonth.map { "Closed \(GoalWords.monthYear($0))" } ?? "Closed"
			tone = .neutral
			detail = "\(format.amount(goal.spent)) spent"
		}
	}

	static func monthYear(_ month: Month) -> String {
		date(month)?.formatted(.dateTime.month(.wide).year()) ?? ""
	}

	static func date(_ month: Month) -> Date? {
		var parts = DateComponents()
		parts.year = Int(month.year)
		parts.month = Int(month.month)
		parts.day = 1
		return Calendar.current.date(from: parts)
	}
}
