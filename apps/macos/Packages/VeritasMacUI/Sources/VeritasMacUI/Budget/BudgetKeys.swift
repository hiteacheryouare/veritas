import SwiftUI

/// What an arrow key does on the Budget screen. AppKit marks every arrow key
/// with the numeric-pad and function flags, so only the chord keys decide.
enum BudgetKeys {
	enum Action: Equatable {
		case moveSelection(Int)
		case moveToGroup(Int)
	}

	static let chordKeys: EventModifiers = [.command, .option, .shift, .control]

	static func action(for key: KeyEquivalent, modifiers: EventModifiers) -> Action? {
		let step: Int
		switch key {
		case .upArrow: step = -1
		case .downArrow: step = 1
		default: return nil
		}
		switch modifiers.intersection(chordKeys) {
		case []: return .moveSelection(step)
		case .option: return .moveToGroup(step)
		default: return nil
		}
	}
}

/// The Budget screen's month stepping, published while the screen has focus
/// so Go › Previous Month and Next Month (⌘← ⌘→) are on only then. A text
/// field with focus keeps those keys for its caret.
struct BudgetMonthActions {
	let canGoBack: Bool
	let canGoForward: Bool
	let previous: () -> Void
	let next: () -> Void
}

extension FocusedValues {
	@Entry var budgetMonths: BudgetMonthActions?
}
