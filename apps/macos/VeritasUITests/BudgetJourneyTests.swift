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
		// Actual's budget cell reads "412.00+" as 412, so use text with no amount in it.
		field.typeKey("a", modifierFlags: .command)
		field.typeText("((")
		field.typeKey(.return, modifierFlags: [])
		XCTAssertTrue(app.staticTexts.matching(NSPredicate(format: "value BEGINSWITH %@", "Can't read ((")).firstMatch.waitForExistence(timeout: 5))
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
		let budgetScreen = app.descendants(matching: .any)["screen.budget"]
		let previousMonth = app.menuBars.menuItems["Previous Month"]
		XCTContext.runActivity(named: "Before ⌘←") { activity in
			record(app, in: activity)
		}
		// A closed menu's items report their last validated state, so open Go first.
		let go = app.menuBars.menuBarItems["Go"]
		go.click()
		XCTAssertTrue(previousMonth.isEnabled, "Go › Previous Month is on while the Budget screen is showing")
		app.typeKey(.escape, modifierFlags: [])
		XCTAssertTrue(waitUntil { !previousMonth.isHittable }, "the Go menu closed")
		app.typeKey(.leftArrow, modifierFlags: .command)
		XCTContext.runActivity(named: "After ⌘←") { activity in
			record(app, in: activity)
		}
		let keyWorked = waitUntil { app.windows.firstMatch.title != title }
		if !keyWorked {
			// Evidence: does the menu item itself step the month?
			go.click()
			previousMonth.click()
			let menuWorked = waitUntil { app.windows.firstMatch.title != title }
			XCTContext.runActivity(named: "Menu item \(menuWorked ? "steps" : "doesn't step") the month") { activity in
				record(app, in: activity)
			}
		}
		XCTAssertTrue(keyWorked, "⌘← goes to the previous month")
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
		// A goal card reads as one element: name, progress, then where it stands.
		XCTAssertEqual(laptop.label, "New laptop, $1,120 of $1,500, On track for March")
		XCTAssertEqual(app.descendants(matching: .any)["goal.japan"].label, "Japan trip, $2,310 of $6,000, Behind, needs $410 a month")
		XCTAssertFalse(app.buttons["envelope.landline.budgeted"].exists, "hidden until asked")
		app.menuBars.menuBarItems["View"].click()
		app.menuItems["Show Hidden Envelopes"].click()
		XCTAssertTrue(app.buttons["envelope.landline.budgeted"].waitForExistence(timeout: 5))
		attachScreenshot(of: app, named: "budget-goals-and-hidden")
	}

	/// design/interaction.md: typing any digit or operator on a selected row opens
	/// its field with what was typed.
	@MainActor
	func testTypingPlusFortyOnASelectedEnvelopeAppendsToItsValue() {
		let app = openBudget()
		app.descendants(matching: .any)["envelope.groceries"].click()
		app.typeText("+40")
		let field = app.textFields["amountField"]
		XCTAssertTrue(field.waitForExistence(timeout: 5))
		XCTAssertEqual(field.value as? String, "412.00+40")
		field.typeKey(.return, modifierFlags: [])
		XCTAssertTrue(waitUntil { app.buttons["envelope.groceries.budgeted"].label == "Budgeted 452.00" })
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

	/// Interaction rules 6 to 8: Tab and Shift-Tab commit and move like Return
	/// and Shift-Return, and Esc puts the old value back with nothing written.
	@MainActor
	func testTabAndShiftTabCommitAndMoveAndEscWritesNothing() {
		let app = openBudget()
		let groceries = app.buttons["envelope.groceries.budgeted"]
		groceries.click()
		let field = app.textFields["amountField"]
		XCTAssertTrue(field.waitForExistence(timeout: 5))
		field.typeText("450")
		field.typeKey(.tab, modifierFlags: [])
		XCTAssertTrue(waitUntil { groceries.label == "Budgeted 450.00" }, groceries.label)
		XCTAssertTrue(waitUntil { field.value as? String == "180.00" }, "Tab moves to Dining out's field")
		field.typeKey(.tab, modifierFlags: .shift)
		XCTAssertTrue(waitUntil { field.value as? String == "450.00" }, "Shift-Tab moves back to Groceries")
		XCTAssertEqual(app.buttons["envelope.dining.budgeted"].label, "Budgeted 180.00", "an untouched field writes nothing")
		field.typeText("1")
		field.typeKey(.escape, modifierFlags: [])
		XCTAssertTrue(field.waitForDisappearance())
		XCTAssertEqual(groceries.label, "Budgeted 450.00", "Esc keeps the value from before editing")
	}

	/// With the amount field open, ⌘← and ⌘→ move the caret, not the month.
	@MainActor
	func testCommandArrowsInTheAmountFieldMoveTheCaret() {
		let app = openBudget()
		let title = app.windows.firstMatch.title
		app.buttons["envelope.groceries.budgeted"].click()
		let field = app.textFields["amountField"]
		XCTAssertTrue(field.waitForExistence(timeout: 5))
		field.typeKey(.leftArrow, modifierFlags: .command)
		field.typeText("9")
		XCTAssertEqual(field.value as? String, "9412.00")
		field.typeKey(.rightArrow, modifierFlags: .command)
		field.typeText("1")
		XCTAssertEqual(field.value as? String, "9412.001")
		XCTAssertEqual(app.windows.firstMatch.title, title, "the month stays put")
		field.typeKey(.escape, modifierFlags: [])
	}

	/// design/interaction.md: VoiceOver reads a row's state as a word, not just its color.
	@MainActor
	func testARowReadsItsStateWord() {
		let app = openBudget()
		let dining = app.descendants(matching: .any)["envelope.dining"]
		XCTAssertTrue(dining.waitForExistence(timeout: 5))
		XCTAssertEqual(dining.label, "Dining out, budgeted 180.00, spent 214.75, overspent by 34.75")
		XCTAssertEqual(app.descendants(matching: .any)["envelope.rent"].label, "Rent, budgeted 1,650.00, spent 1,650.00, nothing left")
	}

	/// Actual's hide-fraction preference: no decimals anywhere on the screen.
	@MainActor
	func testABudgetThatHidesDecimalsShowsNone() {
		let app = XCUIApplication.veritas(stub: "open", ["-VeritasStubHideDecimals", "YES"])
		let groceries = app.buttons["envelope.groceries.budgeted"]
		XCTAssertTrue(groceries.waitForExistence(timeout: 10))
		XCTAssertEqual(groceries.label, "Budgeted 412")
		XCTAssertEqual(app.descendants(matching: .any)["envelope.dining"].label, "Dining out, budgeted 180, spent 215, overspent by 35")
		let toBudget = app.descendants(matching: .any)["budget.toBudget"]
		let toBudgetText = [toBudget.label, toBudget.value as? String].compactMap(\.self).joined(separator: " ")
		XCTAssertFalse(toBudgetText.contains("."), toBudgetText)
		attachScreenshot(of: app, named: "budget-hidden-decimals")
	}

	@MainActor
	private func openBudget() -> XCUIApplication {
		let app = XCUIApplication.veritas(stub: "open")
		XCTAssertTrue(app.descendants(matching: .any)["screen.budget"].waitForExistence(timeout: 10))
		XCTAssertTrue(app.buttons["envelope.groceries.budgeted"].waitForExistence(timeout: 10))
		return app
	}

	/// The accessibility tree, which shows where keyboard focus is, and a screenshot.
	@MainActor
	private func record(_ app: XCUIApplication, in activity: XCTActivity) {
		let tree = XCTAttachment(string: app.debugDescription)
		tree.name = "\(activity.name) tree"
		tree.lifetime = .keepAlways
		activity.add(tree)
		let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
		shot.name = "\(activity.name) screenshot"
		shot.lifetime = .keepAlways
		activity.add(shot)
	}

	private func waitUntil(_ condition: @escaping () -> Bool) -> Bool {
		let expectation = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: nil)
		return XCTWaiter().wait(for: [expectation], timeout: 5) == .completed
	}
}
