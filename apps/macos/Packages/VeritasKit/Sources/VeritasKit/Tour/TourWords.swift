/// The words on the tour's coach marks.
public enum TourWords {
	public static func text(_ step: TourStep) -> String {
		switch step {
		case .toBudget:
			"To Budget is money you have that isn't in an envelope yet. Assign it to envelopes until this reaches zero."
		case .budgetCell:
			"Click a Budgeted amount and type what this envelope gets this month. Return saves it, and Tab moves to the next envelope."
		case .openAccount:
			"Every account has a register with all of its transactions. Click an account to open it."
		case .addTransaction:
			"Click Add transaction in the toolbar, or press ⌘N on any screen."
		case .quickEntry:
			"Start typing a payee you've used before and pick it. The envelope and amount fill in from your last transaction with them."
		}
	}

	/// "2 of 5".
	public static func position(_ number: Int, of count: Int) -> String {
		"\(number) of \(count)"
	}

	/// What VoiceOver reads when a step appears.
	public static func spoken(_ step: TourStep, number: Int, of count: Int) -> String {
		"Tour, step \(number) of \(count). \(text(step))"
	}

	public static func next(isLastStep: Bool) -> String {
		isLastStep ? "Done" : "Next"
	}

	public static let back = "Back"
	public static let skip = "Skip"
}
