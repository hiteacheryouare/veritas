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

/// A screen's month stepping, published for its window, so Go › Previous
/// Month and Next Month (⌘← ⌘→) work wherever focus is. In a text field
/// those keys still move the caret (VeritasCommands).
struct BudgetMonthActions {
	let canGoBack: Bool
	let canGoForward: Bool
	let previous: () -> Void
	let next: () -> Void
}

extension FocusedValues {
	@Entry var budgetMonths: BudgetMonthActions?
}
