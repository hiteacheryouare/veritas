import XCTest

/// The first-launch tour (D79): it walks the Budget grid, then Accounts and a
/// register, and Skip or Done ends it for good. Help › Show Tour replays it.
final class TourJourneyTests: XCTestCase {
	override func setUp() {
		continueAfterFailure = false
	}

	@MainActor
	func testTheTourWalksEveryStepAfterTheFirstBudgetOpens() {
		let app = XCUIApplication.veritas(stub: "empty", ["-VeritasTour", "YES"])
		let importButton = app.buttons["importFromActual"]
		XCTAssertTrue(importButton.waitForExistence(timeout: 10))
		importButton.click()

		let card = app.groups["tour.card"]
		XCTAssertTrue(card.waitForExistence(timeout: 10), "the tour starts once the budget is open")
		let bodies = ["To Budget is the money", "Click a Budgeted amount", "Every account has a register", "Click Add transaction", "Start typing a payee"]
		let steps = [("1 of 5", "budget"), ("2 of 5", "budget"), ("3 of 5", "accounts"), ("4 of 5", "register"), ("5 of 5", "register")]
		for (index, (position, screen)) in steps.enumerated() {
			XCTAssertTrue(waitForPosition(position, in: app), "step \(index + 1)")
			XCTAssertTrue(app.screen(screen).waitForExistence(timeout: 15), "step \(index + 1) is on \(screen)")
			XCTAssertEqual(app.buttons["tour.back"].isEnabled, index > 0)
			let spoken = app.groups.matching(NSPredicate(format: "label BEGINSWITH %@", "Tour, step \(index + 1) of 5. "))
			XCTAssertEqual(spoken.count, 1, "VoiceOver finds the step once")
			XCTAssertEqual(card.staticTexts.matching(NSPredicate(format: "label BEGINSWITH %@", bodies[index])).count, 0, "the card's body isn't read a second time")
			attachScreenshot(of: app, named: "tour-\(index + 1)")
			if index == 1 {
				let groceries = app.buttons["envelope.groceries.budgeted"]
				groceries.click()
				let field = app.textFields["amountField"]
				XCTAssertTrue(field.waitForExistence(timeout: 5))
				field.typeText("+40")
				field.typeKey(.return, modifierFlags: [])
				XCTAssertTrue(waitUntil { groceries.label == "Budgeted 452.00" }, "Return in a Budget cell saves: \(groceries.label)")
				XCTAssertTrue(waitForPosition("2 of 5", in: app), "and doesn't move the tour on")
				field.typeKey(.escape, modifierFlags: [])
				XCTAssertTrue(field.waitForDisappearance())
				XCTAssertTrue(card.exists, "Esc in the cell cancels the field, not the tour")
			}
			if index == 2 {
				app.buttons["tour.back"].click()
				XCTAssertTrue(waitForPosition("2 of 5", in: app), "Back returns a step")
				XCTAssertTrue(app.screen("budget").waitForExistence(timeout: 5))
				card.typeKey("[", modifierFlags: .command)
				XCTAssertTrue(waitForPosition("1 of 5", in: app), "⌘[ is Back while the card has focus")
				card.typeKey(.return, modifierFlags: [])
				XCTAssertTrue(waitForPosition("2 of 5", in: app), "Return is Next")
				card.typeKey(.return, modifierFlags: [])
				XCTAssertTrue(waitForPosition("3 of 5", in: app))
			}
			let next = app.buttons["tour.next"]
			XCTAssertTrue([next.title, next.label].contains(index == steps.count - 1 ? "Done" : "Next"))
			next.click()
		}
		XCTAssertTrue(card.waitForDisappearance(), "Done ends the tour")

		app.menuBars.menuBarItems["Help"].click()
		app.menuItems["Show Tour"].click()
		XCTAssertTrue(waitForPosition("1 of 5", in: app), "Show Tour replays it from the start")
		XCTAssertTrue(app.screen("budget").waitForExistence(timeout: 5))
		card.typeKey(.escape, modifierFlags: [])
		XCTAssertTrue(card.waitForDisappearance(), "Esc is Skip")
	}

	@MainActor
	func testSkipEndsTheTourForGood() {
		let app = XCUIApplication.veritas(stub: "open", ["-VeritasTour", "YES"])
		let card = app.groups["tour.card"]
		XCTAssertTrue(card.waitForExistence(timeout: 10))
		app.buttons["tour.next"].click()
		XCTAssertTrue(waitForPosition("2 of 5", in: app))
		app.buttons["tour.skip"].click()
		XCTAssertTrue(card.waitForDisappearance())

		app.menuBars.menuBarItems["File"].click()
		app.menuItems["Close Budget"].click()
		let importButton = app.buttons["importFromActual"]
		XCTAssertTrue(importButton.waitForExistence(timeout: 10))
		importButton.click()
		XCTAssertTrue(app.screen("budget").waitForExistence(timeout: 10))
		XCTAssertFalse(card.waitForExistence(timeout: 3), "a skipped tour doesn't come back when another budget opens")
	}

	@MainActor
	private func waitUntil(timeout: TimeInterval = 5, _ condition: @escaping () -> Bool) -> Bool {
		let met = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: nil)
		return XCTWaiter().wait(for: [met], timeout: timeout) == .completed
	}

	@MainActor
	private func waitForPosition(_ position: String, in app: XCUIApplication) -> Bool {
		let card = app.groups["tour.card"]
		let shown = XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == true AND label BEGINSWITH %@", "Tour, step \(position). "), object: card)
		return XCTWaiter().wait(for: [shown], timeout: 15) == .completed
	}
}
