import Foundation
import VeritasCore
import VeritasDesign
import VeritasKit

/// What a goal card says about where a goal stands (design/mockups/goals.html).
struct GoalWords {
	let note: String
	let tone: VeritasDesign.MoneyTone
	let detail: String?

	init(_ goal: GoalSummary, format: MoneyFormat, thisMonth: Month = GoalWords.currentMonth()) {
		switch goal.state {
		case .active:
			let suggestion = goal.suggestedMonthly.map { "Budget \(format.wholeCurrency($0)) a month" }
			switch goal.pace {
			case .onTrack(let eta):
				note = "On track for \(GoalWords.etaName(eta, from: thisMonth))"
				tone = .available
				detail = suggestion
			case .behind(let needed):
				note = "Behind, needs \(format.wholeCurrency(needed)) a month"
				tone = .low
				detail = nil
			case nil:
				if let suggestion, let month = goal.targetMonth {
					note = suggestion
					detail = "Target \(GoalWords.monthYear(month))"
				} else {
					note = "No target date"
					detail = nil
				}
				tone = .available
			}
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

	/// The mockups name a near month alone ("On track for March") and a
	/// farther one with its year ("On track for August 2027").
	static func etaName(_ month: Month, from now: Month) -> String {
		let ahead = (Int(month.year) - Int(now.year)) * 12 + Int(month.month) - Int(now.month)
		guard (0...6).contains(ahead) else { return monthYear(month) }
		return date(month)?.formatted(.dateTime.month(.wide)) ?? ""
	}

	static func currentMonth(_ date: Date = .now, calendar: Calendar = .current) -> Month {
		let parts = calendar.dateComponents([.year, .month], from: date)
		return Month(year: Int32(parts.year ?? 2000), month: UInt32(parts.month ?? 1))
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
