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
		tone = goal.tone.design
		switch goal.state {
		case .active:
			// DEBT-8.1: suggestions stay in minor units, so they show at the currency's places, never rounded to whole units.
			let suggestion = goal.suggestedMonthly.map { "Budget \(format.currency($0, on: .budget)) a month" }
			switch goal.pace {
			case .onTrack(let eta):
				note = "On track for \(GoalWords.etaName(eta, from: thisMonth))"
				detail = suggestion
			case .behind(let needed):
				note = "Behind, needs \(format.currency(needed, on: .budget)) a month"
				detail = nil
			case .noHistory(let needed):
				note = "Budget \(format.currency(needed, on: .budget)) a month"
				detail = "No ETA yet"
			case .notAdding(let needed):
				note = "Nothing is being added"
				detail = "Budget \(format.currency(needed, on: .budget)) a month"
			case nil:
				if let suggestion, let month = goal.targetMonth {
					note = suggestion
					detail = "Target \(GoalWords.monthYear(month))"
				} else {
					note = "No target date"
					detail = nil
				}
			}
		case .reached:
			note = "Reached"
			detail = nil
		case .reachedSpent:
			let available = format.currency(goal.available, on: .budget)
			// CLS-12.14: in a 0-place budget the figure mixes scales, so it is only
			// ever "Available", whatever its sign.
			let position = if format.decimalPlaces == 0 {
				"Available \(available)"
			} else if goal.tone == .overspent {
				"\(available) over"
			} else {
				"\(available) left"
			}
			note = goal.tone == .overspent ? "Reached → Spent, overspent" : "Reached → Spent"
			detail = "\(format.currency(goal.spent, on: .register)) spent, \(position)"
		case .closed:
			note = goal.closedMonth.map { "Closed \(GoalWords.monthYear($0))" } ?? "Closed"
			detail = "\(format.amount(goal.spent, on: .register)) spent"
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
