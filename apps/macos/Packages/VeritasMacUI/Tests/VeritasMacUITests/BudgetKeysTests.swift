import SwiftUI
import Testing
@testable import VeritasMacUI

/// AppKit marks every arrow key with the numeric-pad and function flags; the
/// Budget screen's arrow keys must look past them.
struct BudgetKeysTests {
	static let arrowFlags: EventModifiers = [.numericPad, .function]

	@Test func plainArrowsMoveTheSelectionDespiteTheArrowFlags() {
		#expect(BudgetKeys.action(for: .downArrow, modifiers: Self.arrowFlags) == .moveSelection(1))
		#expect(BudgetKeys.action(for: .upArrow, modifiers: Self.arrowFlags) == .moveSelection(-1))
		#expect(BudgetKeys.action(for: .downArrow, modifiers: []) == .moveSelection(1))
	}

	@Test func optionArrowsJumpGroups() {
		#expect(BudgetKeys.action(for: .downArrow, modifiers: Self.arrowFlags.union(.option)) == .moveToGroup(1))
		#expect(BudgetKeys.action(for: .upArrow, modifiers: Self.arrowFlags.union(.option)) == .moveToGroup(-1))
	}

	@Test func otherChordsAndKeysAreLeftAlone() {
		#expect(BudgetKeys.action(for: .downArrow, modifiers: Self.arrowFlags.union(.command)) == nil)
		#expect(BudgetKeys.action(for: .downArrow, modifiers: Self.arrowFlags.union(.shift)) == nil)
		#expect(BudgetKeys.action(for: .leftArrow, modifiers: Self.arrowFlags.union(.command)) == nil, "⌘← belongs to Go › Previous Month")
		#expect(BudgetKeys.action(for: .return, modifiers: []) == nil)
	}
}
