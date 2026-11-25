import Foundation
import VeritasCore

/// The dates a question asks about, worked out from the user's own words
/// against today (docs/finance/assistant.md AI-6), so the model never does
/// date arithmetic. `first` and `last` are inclusive `YYYY-MM-DD` dates.
struct AssistantPeriod: Hashable, Sendable {
	let first: String
	let last: String

	static let hint = "Ask again with a period such as this month, so far this month, last month, this year, October 2026, 2026, or 2026-10-01 to 2026-10-15."

	init(first: String, last: String) {
		self.first = min(first, last)
		self.last = max(first, last)
	}

	init(month: Month) {
		self.init(first: month.firstDay, last: month.lastDay)
	}

	init(year: Int32) {
		self.init(first: Month(year: year, month: 1).firstDay, last: Month(year: year, month: 12).lastDay)
	}

	var firstMonth: Month { Month(date: first)! }
	var lastMonth: Month { Month(date: last)! }

	/// The one whole calendar month this is, if it is one.
	var wholeMonth: Month? {
		let month = firstMonth
		return first == month.firstDay && last == month.lastDay ? month : nil
	}

	/// "November 2026", "2026", "May to October 2026", "November 1 to 22, 2026",
	/// "January 1 to November 22, 2026", "December 15, 2025 to January 3, 2026".
	var title: String {
		let (start, end) = (firstMonth, lastMonth)
		if let month = wholeMonth { return ReportPeriod.monthYear(month) }
		if first == Month(year: start.year, month: 1).firstDay, last == Month(year: start.year, month: 12).lastDay { return "\(start.year)" }
		if first == start.firstDay, last == end.lastDay { return ReportPeriod(first: start, last: end).title }
		let (startDay, endDay) = (Self.day(first), Self.day(last))
		if first == last { return "\(ReportPeriod.monthName(start)) \(startDay), \(start.year)" }
		if start == end { return "\(ReportPeriod.monthName(start)) \(startDay) to \(endDay), \(start.year)" }
		if start.year == end.year { return "\(ReportPeriod.monthName(start)) \(startDay) to \(ReportPeriod.monthName(end)) \(endDay), \(end.year)" }
		return "\(ReportPeriod.monthName(start)) \(startDay), \(start.year) to \(ReportPeriod.monthName(end)) \(endDay), \(end.year)"
	}

	private static func day(_ date: String) -> Int {
		Int(date.split(separator: "-").last ?? "") ?? 1
	}

	/// Nil when the words name no period this can read, which the tool answers with `hint`.
	static func resolve(_ words: String, today: String) -> AssistantPeriod? {
		guard let thisMonth = Month(date: today), AssistantTools.years.contains(Int(thisMonth.year)) else { return nil }
		let text = normalized(words)
		let toDate = ["so far", "to date", "ytd", "mtd", "until now", "up to now", "until today", "through today"].contains { text.contains($0) }

		let written = text.matches(of: /\d+-\d+-\d+/).map { AssistantDates.day(String($0.output)) }
		if !written.isEmpty {
			let dates = written.compactMap(\.self)
			guard dates.count == written.count, dates.count == 1 || dates[0] <= dates[1] else { return nil }
			return AssistantPeriod(first: dates[0], last: dates.count > 1 ? dates[1] : dates[0])
		}
		if let match = text.firstMatch(of: /\d+-\d+/) {
			guard match.output.count <= 7, let month = AssistantDates.month(String(match.output)) else { return nil }
			return AssistantPeriod(month: month).endingToday(toDate, today: today)
		}

		let thisYear = thisMonth.year
		let lastYear = ["last year", "previous year", "past year"].contains { text.contains($0) }
		let currentYear = ["this year", "current year", "year to date", "ytd"].contains { text.contains($0) }
		let namedYear = text.firstMatch(of: /\b(\d{4})\b/).flatMap { Int32($0.1) }.flatMap { AssistantTools.years.contains(Int($0)) ? $0 : nil }
		let year = namedYear ?? (lastYear ? thisYear - 1 : currentYear ? thisYear : nil)

		let months = monthNumbers(in: text)
		if let last = months.last {
			let end = year.map { Month(year: $0, month: last) } ?? Month(year: thisYear, month: last).latest(notAfter: thisMonth)
			guard let first = months.first, first != last else { return AssistantPeriod(month: end).endingToday(toDate, today: today) }
			let start = Month(year: end.year, month: first).latest(notAfter: end)
			return AssistantPeriod(first: start.firstDay, last: end.lastDay).endingToday(toDate, today: today)
		}
		let thisMonthNamed = ["this month", "same month", "current month"].contains { text.contains($0) }
		if let year, thisMonthNamed {
			return AssistantPeriod(month: Month(year: year, month: thisMonth.month)).endingToday(toDate, today: today)
		}
		if ["last month", "previous month", "past month"].contains(where: { text.contains($0) }) {
			return AssistantPeriod(month: thisMonth.adding(months: -1))
		}
		if let year { return AssistantPeriod(year: year).endingToday(toDate, today: today) }
		if text == "today" { return AssistantPeriod(first: today, last: today) }
		if text.isEmpty || toDate || thisMonthNamed || ["month to date", "mtd"].contains(where: { text.contains($0) }) {
			return AssistantPeriod(month: thisMonth).endingToday(toDate, today: today)
		}
		return nil
	}

	/// "So far" and "to date" stop a period that holds today at today (AI-6.3).
	private func endingToday(_ toDate: Bool, today: String) -> AssistantPeriod {
		guard toDate, first <= today, today <= last else { return self }
		return AssistantPeriod(first: first, last: today)
	}

	private static func normalized(_ words: String) -> String {
		words.folding(options: [.caseInsensitive, .diacriticInsensitive], locale: nil)
			.replacingOccurrences(of: "\u{2019}", with: "'")
			.components(separatedBy: .whitespacesAndNewlines).filter { !$0.isEmpty }.joined(separator: " ")
	}

	/// The months named, in the order written, by their English names and
	/// abbreviations or the user's own locale's.
	private static func monthNumbers(in text: String) -> [UInt32] {
		var english = Calendar(identifier: .gregorian)
		english.locale = Locale(identifier: "en_US_POSIX")
		let lists = [english.monthSymbols, english.shortMonthSymbols, Calendar.current.monthSymbols, Calendar.current.shortMonthSymbols]
			.map { $0.map { normalized($0).trimmingCharacters(in: .punctuationCharacters) } }
		return text.split(whereSeparator: { !$0.isLetter }).compactMap { word in
			lists.lazy.compactMap { $0.firstIndex(of: String(word)) }.first.map { UInt32($0 + 1) }
		}
	}
}

private extension Month {
	/// The latest month with this month's name that isn't after `today`'s month (AI-6.3).
	func latest(notAfter today: Month) -> Month {
		self > today ? adding(months: -12) : self
	}
}
