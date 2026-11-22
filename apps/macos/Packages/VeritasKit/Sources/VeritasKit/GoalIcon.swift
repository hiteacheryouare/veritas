import Foundation

/// A goal's icon as the core stores it (R-GOAL-1, v18): text that is all
/// ASCII is an SF Symbol name, anything else is one emoji. The core decides
/// what it accepts; this only tells the two apart for drawing.
public enum GoalIcon: Hashable, Sendable {
	case symbol(String)
	case emoji(String)

	public init?(_ stored: String?) {
		guard let stored, !stored.isEmpty else { return nil }
		self = stored.allSatisfy(\.isASCII) ? .symbol(stored) : .emoji(stored)
	}

	public var stored: String {
		switch self {
		case .symbol(let name), .emoji(let name): name
		}
	}

	/// The symbols the icon picker offers, the new-goal sheet's first.
	public static let symbols = [
		"laptopcomputer", "airplane", "car", "house", "gift", "heart", "graduationcap", "umbrella",
		"bicycle", "pawprint", "stroller", "bed.double", "sofa", "tent", "camera", "gamecontroller",
		"guitars", "tshirt", "wrench.and.screwdriver", "cross.case", "leaf", "banknote", "building.columns", "sparkles"
	]
}
