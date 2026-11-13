import XCTest

/// The Budget screen against the stub's mockup month: arithmetic in amount
/// fields (R-BUD-3), Move budget and Cover overspending (R-BUD-4), notes
/// (R-BUD-6) and the Gate 1 shortcuts.
final class BudgetJourneyTests: XCTestCase {
	override func setUp() {
		continueAfterFailure = false
	}

	@MainActor
	func testAppendingToABudgetedAmountAndMovingToTheNextEnvelope() {
		let app = openBudget()
		let groceries = app.buttons["envelope.groceries.budgeted"]
		XCTAssertTrue(groceries.waitForExistence(timeout: 5))
		groceries.click()
		let field = app.textFields["amountField"]
		XCTAssertTrue(field.waitForExistence(timeout: 5))
		XCTAssertEqual(field.value as? String, "412.00")
		field.typeText("+40")
		XCTAssertEqual(field.value as? String, "412.00+40")
		XCTAssertTrue(app.descendants(matching: .any)["budget.editPreview"].waitForExistence(timeout: 5))
		attachScreenshot(of: app, named: "budget-editing")
		field.typeKey(.return, modifierFlags: [])
		XCTAssertTrue(waitUntil { groceries.label == "Budgeted 452.00" }, groceries.label)
		XCTAssertTrue(field.waitForExistence(timeout: 5), "Return moves to the next envelope's field")
		XCTAssertEqual(field.value as? String, "180.00")
		field.typeKey(.escape, modifierFlags: [])
		XCTAssertTrue(field.waitForDisappearance())
	}

	@MainActor
	func testAnAmountThatCantBeReadKeepsTheOldValue() {
		let app = openBudget()
		app.buttons["envelope.groceries.budgeted"].click()
		let field = app.textFields["amountField"]
		XCTAssertTrue(field.waitForExistence(timeout: 5))
		field.typeText("+")
		field.typeKey(.return, modifierFlags: [])
		XCTAssertTrue(app.staticTexts.matching(NSPredicate(format: "value BEGINSWITH %@", "Can't read 412.00+")).firstMatch.waitForExistence(timeout: 5))
		attachScreenshot(of: app, named: "budget-unreadable")
		field.typeKey(.escape, modifierFlags: [])
		XCTAssertEqual(app.buttons["envelope.groceries.budgeted"].label, "Budgeted 412.00")
	}

	@MainActor
	func testMovingBudgetBetweenEnvelopes() {
		let app = openBudget()
		app.buttons["budget.moveBudget"].click()
		let sheet = app.descendants(matching: .any)["sheet.moveBudget"]
		XCTAssertTrue(sheet.waitForExistence(timeout: 5))
		sheet.popUpButtons["moveBudget.from"].click()
		app.menuItems.matching(NSPredicate(format: "title BEGINSWITH %@", "Groceries")).firstMatch.click()
		sheet.popUpButtons["moveBudget.to"].click()
		app.menuItems.matching(NSPredicate(format: "title BEGINSWITH %@", "Gas")).firstMatch.click()
		let amount = sheet.textFields["amountField"]
		amount.click()
		amount.typeKey("a", modifierFlags: .command)
		amount.typeText("50")
		attachScreenshot(of: app, named: "move-budget")
		sheet.buttons["moveBudget.move"].click()
		XCTAssertTrue(sheet.waitForDisappearance())
		XCTAssertEqual(app.buttons["envelope.groceries.budgeted"].label, "Budgeted 362.00")
		XCTAssertEqual(app.buttons["envelope.gas.budgeted"].label, "Budgeted 190.00")
	}

	@MainActor
	func testCoveringAnOverspentEnvelope() {
		let app = openBudget()
		let cover = app.buttons["envelope.dining.cover"]
		XCTAssertTrue(cover.waitForExistence(timeout: 5))
		cover.click()
		let popover = app.descendants(matching: .any)["popover.cover"]
		XCTAssertTrue(popover.waitForExistence(timeout: 5))
		popover.popUpButtons.firstMatch.click()
		app.menuItems.matching(NSPredicate(format: "title BEGINSWITH %@", "Groceries")).firstMatch.click()
		attachScreenshot(of: app, named: "cover-overspending")
		popover.buttons["cover.cover"].click()
		XCTAssertTrue(waitUntil { app.buttons["envelope.dining.budgeted"].label == "Budgeted 214.75" })
		XCTAssertFalse(app.buttons["envelope.dining.cover"].exists, "no longer overspent")
	}

	@MainActor
	func testKeyboardWalksEnvelopesAndOpensTheField() {
		let app = openBudget()
		app.descendants(matching: .any)["envelope.groceries"].click()
		app.typeKey(.downArrow, modifierFlags: [])
		app.typeKey(.downArrow, modifierFlags: .option)
		app.typeKey(.return, modifierFlags: [])
		let field = app.textFields["amountField"]
		XCTAssertTrue(field.waitForExistence(timeout: 5))
		XCTAssertEqual(field.value as? String, "1,650.00", "⌥↓ jumps to Rent, the next group's first envelope")
		field.typeKey(.escape, modifierFlags: [])
		app.typeKey("7", modifierFlags: [])
		XCTAssertTrue(field.waitForExistence(timeout: 5))
		XCTAssertEqual(field.value as? String, "7", "typing a digit opens the field over the value")
		field.typeKey(.escape, modifierFlags: [])
		let title = app.windows.firstMatch.title
		app.typeKey(.leftArrow, modifierFlags: .command)
		XCTAssertTrue(waitUntil { app.windows.firstMatch.title != title }, "⌘← goes to the previous month")
	}

	@MainActor
	func testAddingANote() {
		let app = openBudget()
		let hobbies = app.descendants(matching: .any)["envelope.hobbies"]
		XCTAssertTrue(hobbies.waitForExistence(timeout: 5))
		hobbies.rightClick()
		app.menuItems["Add Note…"].click()
		let text = app.textViews["note.text"]
		XCTAssertTrue(text.waitForExistence(timeout: 5))
		text.click()
		text.typeText("Guitar strings in January")
		app.buttons["note.save"].click()
		XCTAssertTrue(hobbies.descendants(matching: .any).matching(NSPredicate(format: "label == %@", "Note: Guitar strings in January")).firstMatch.waitForExistence(timeout: 5))
		attachScreenshot(of: app, named: "budget-note")
	}

	@MainActor
	func testDenseModeKeepsEveryEnvelope() {
		let app = openBudget()
		app.typeKey("d", modifierFlags: .command)
		XCTAssertTrue(app.buttons["envelope.paycheck.budgeted"].exists == false)
		XCTAssertTrue(app.buttons["envelope.gifts.budgeted"].waitForExistence(timeout: 5))
		attachScreenshot(of: app, named: "budget-dense")
	}

	@MainActor
	func testGoalCardsAndHiddenEnvelopes() {
		let app = openBudget()
		let laptop = app.descendants(matching: .any)["goal.laptop"]
		XCTAssertTrue(laptop.waitForExistence(timeout: 5))
		XCTAssertTrue(laptop.staticTexts["On track for March"].exists)
		XCTAssertTrue(app.descendants(matching: .any)["goal.japan"].staticTexts["Behind, needs $410 a month"].exists)
		XCTAssertFalse(app.buttons["envelope.landline.budgeted"].exists, "hidden until asked")
		app.menuBars.menuBarItems["View"].click()
		app.menuItems["Show Hidden Envelopes"].click()
		XCTAssertTrue(app.buttons["envelope.landline.budgeted"].waitForExistence(timeout: 5))
		attachScreenshot(of: app, named: "budget-goals-and-hidden")
	}

	@MainActor
	func testCommandLeftInTheFindFieldMovesTheCaret() {
		let app = openBudget()
		let title = app.windows.firstMatch.title
		app.typeKey("f", modifierFlags: .command)
		let find = app.textFields["findBar.field"]
		XCTAssertTrue(find.waitForExistence(timeout: 5))
		find.typeText("abc")
		find.typeKey(.leftArrow, modifierFlags: .command)
		find.typeText("X")
		XCTAssertEqual(find.value as? String, "Xabc")
		XCTAssertEqual(app.windows.firstMatch.title, title, "the month stays put")
	}

	@MainActor
	private func openBudget() -> XCUIApplication {
		let app = XCUIApplication.veritas(stub: "open")
		XCTAssertTrue(app.descendants(matching: .any)["screen.budget"].waitForExistence(timeout: 10))
		XCTAssertTrue(app.buttons["envelope.groceries.budgeted"].waitForExistence(timeout: 10))
		return app
	}

	private func waitUntil(_ condition: @escaping () -> Bool) -> Bool {
		let expectation = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: nil)
		return XCTWaiter().wait(for: [expectation], timeout: 5) == .completed
	}
}
