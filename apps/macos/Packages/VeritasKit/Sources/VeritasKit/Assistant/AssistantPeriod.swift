import Foundation
import VeritasCore

/// The dates a question asks about come from the core's `resolvePeriod`
/// (docs/finance/assistant.md AI-6), so neither the model nor the app does
/// date arithmetic. `first` and `last` are inclusive `YYYY-MM-DD` dates and
/// `label` states them.
extension AssistantPeriod {
	var firstMonth: Month { Month(date: first)! }
	var lastMonth: Month { Month(date: last)! }
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
