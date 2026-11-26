import Foundation
import VeritasCore

/// The dates a question asks about come from the core's `resolvePeriod`
/// (docs/finance/assistant.md AI-6), so neither the model nor the app does
/// date arithmetic. `first` and `last` are inclusive `YYYY-MM-DD` dates and
/// `label` states them.
extension AssistantPeriod {
	static let notStartedLine = "This period has not started."

	var firstMonth: Month { Month(date: first)! }
	var lastMonth: Month { Month(date: last)! }

	/// AI-6.16: a tool that reads recorded rows says so, directly after its
	/// first line, when the period starts after today.
	func marked(_ text: String) -> String {
		guard notStarted else { return text }
		var lines = text.components(separatedBy: "\n")
		lines.insert(Self.notStartedLine, at: min(1, lines.count))
		return lines.joined(separator: "\n")
	}
}

extension PeriodResolution {
	var period: AssistantPeriod? {
		guard case .range(let period) = self else { return nil }
		return period
	}

	/// What the tool answers with when the words name no period, in place of a figure.
	var hint: String {
		guard case .refused(let hint) = self else { return "" }
		return hint
	}
}
