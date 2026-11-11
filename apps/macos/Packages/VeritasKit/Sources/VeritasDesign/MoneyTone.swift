import SwiftUI

/// How an amount reads on screen. The core decides which tone an amount has;
/// this only says what each tone looks like. Color is never the only signal,
/// so every tone but neutral carries an icon, and most carry a word.
public enum MoneyTone: CaseIterable, Sendable {
	case available
	case low
	case overspent
	case income
	case neutral

	public var text: ColorToken {
		switch self {
		case .available: .moneyAvailable
		case .low: .moneyLow
		case .overspent: .moneyOverspent
		case .income: .moneyIncome
		case .neutral: .moneyNeutral
		}
	}

	public var fill: ColorToken {
		switch self {
		case .available: .moneyAvailableFill
		case .low: .moneyLowFill
		case .overspent: .moneyOverspentFill
		case .income: .moneyIncomeFill
		case .neutral: .moneyNeutral
		}
	}

	public var symbol: String? {
		switch self {
		case .available: "checkmark.circle"
		case .low: "arrow.down.circle"
		case .overspent: "exclamationmark.triangle"
		case .income: "tray.and.arrow.down"
		case .neutral: nil
		}
	}

	/// Income has no fixed word: the screen says "To Budget" or "Income".
	public var word: String? {
		switch self {
		case .available: "left"
		case .low: "low"
		case .overspent: "over"
		case .income, .neutral: nil
		}
	}
}
