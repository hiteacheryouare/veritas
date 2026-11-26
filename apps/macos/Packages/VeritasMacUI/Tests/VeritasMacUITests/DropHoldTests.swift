import Testing
@testable import VeritasMacUI

/// A dropped dashboard card holds its drop target until the model has moved
/// it, whichever load lands the move.
struct DropHoldTests {
	let origin = GridSlot(column: 0, row: 0)
	let target = GridSlot(column: 4, row: 0)

	private func released() -> DropHold {
		var hold = DropHold(origin: origin)
		hold.isReleased = true
		return hold
	}

	@Test func anOvertakenReloadKeepsTheHoldUntilTheCardMoves() {
		var hold = released()
		// The move's own load is overtaken by the screen's reload, so the move
		// returns while the card is still in its old slot and a load is in flight.
		hold.isMoveFinished = true
		#expect(!hold.isOver(slot: origin, isLoading: true), "the card would bounce back to its old slot")
		// The reload lands with the card in its new slot.
		#expect(hold.isOver(slot: target, isLoading: false))
	}

	@Test func theHoldEndsAsSoonAsTheSlotChanges() {
		let hold = released()
		#expect(hold.isOver(slot: target, isLoading: true), "before the move even returns")
	}

	@Test func aMoveThatChangesNothingSettlesBack() {
		var hold = released()
		#expect(!hold.isOver(slot: origin, isLoading: false), "the move hasn't returned yet")
		hold.isMoveFinished = true
		#expect(hold.isOver(slot: origin, isLoading: false), "rejected or clamped to where it was")
	}

	@Test func nothingEndsTheHoldWhileTheCardIsStillDragged() {
		let hold = DropHold(origin: origin)
		#expect(!hold.isOver(slot: target, isLoading: false), "another window moved it mid-drag")
	}
}
