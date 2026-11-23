import XCTest

/// Goals (R-GOAL-1/3/4, D66) and envelope management (R-BUD-5) against the stub.
final class GoalsAndEnvelopesJourneyTests: XCTestCase {
	override func setUp() {
		continueAfterFailure = false
	}

	@MainActor
	func testCreatingAGoal() {
		let app = openGoals()
		app.buttons["goals.new"].click()
		let sheet = app.descendants(matching: .any)["sheet.newGoal"]
		XCTAssertTrue(sheet.waitForExistence(timeout: 5))
		let name = sheet.textFields["newGoal.name"]
		name.click()
		name.typeText("New bike")
		let target = sheet.textFields["amountField"]
		target.click()
		target.typeKey("a", modifierFlags: .command)
		target.typeText("1200")
		attachScreenshot(of: app, named: "new-goal")
		sheet.buttons["newGoal.create"].click()
		XCTAssertTrue(sheet.waitForDisappearance())
		let card = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH 'goal.goal-'")).firstMatch
		XCTAssertTrue(card.waitForExistence(timeout: 5))
		XCTAssertTrue(card.label.hasPrefix("New bike, $0 of $1,200"), card.label)
	}

	@MainActor
	func testChoosingAGoalIconShowsTheCoresRefusalInline() {
		// R-GOAL-1, v18: a symbol from the grid or one emoji, and nothing else.
		let app = openGoals()
		let laptop = app.descendants(matching: .any)["goal.laptop"]
		XCTAssertTrue(laptop.waitForExistence(timeout: 5))
		XCTAssertTrue(app.descendants(matching: .any)["goal.laptop.icon.laptopcomputer"].exists)
		XCTAssertTrue(app.descendants(matching: .any)["goal.japan.icon.🗾"].exists, "an emoji is drawn as text")
		laptop.rightClick()
		contextItem(app, "Change Icon…")?.click()
		let sheet = app.descendants(matching: .any)["sheet.goalIcon"]
		XCTAssertTrue(sheet.waitForExistence(timeout: 5))
		let emoji = sheet.textFields["goalIcon.emoji"]
		emoji.click()
		emoji.typeText("Car")
		sheet.buttons["goalIcon.save"].click()
		XCTAssertTrue(sheet.staticTexts["Choose a symbol or one emoji."].waitForExistence(timeout: 5))
		attachScreenshot(of: app, named: "goal-icon-refused")
		sheet.buttons["goalIcon.symbol.car"].click()
		XCTAssertFalse(sheet.staticTexts["Choose a symbol or one emoji."].exists, "a new choice clears the refusal")
		sheet.buttons["goalIcon.save"].click()
		XCTAssertTrue(sheet.waitForDisappearance())
		XCTAssertTrue(app.descendants(matching: .any)["goal.laptop.icon.car"].waitForExistence(timeout: 5))
	}

	@MainActor
	func testClosingASpentGoalMovesItToTheArchive() {
		let app = openGoals()
		let close = app.buttons["goal.tires.close"]
		XCTAssertTrue(close.waitForExistence(timeout: 5))
		close.click()
		let sheet = app.descendants(matching: .any)["sheet.closeGoal"]
		XCTAssertTrue(sheet.waitForExistence(timeout: 5))
		XCTAssertTrue(sheet.staticTexts["$20.00 is left in this goal."].waitForExistence(timeout: 5))
		attachScreenshot(of: app, named: "close-goal")
		sheet.buttons["closeGoal.close"].click()
		XCTAssertTrue(sheet.waitForDisappearance())
		let archived = app.descendants(matching: .any)["goal.tires"]
		XCTAssertTrue(archived.waitForExistence(timeout: 5))
		XCTAssertTrue(archived.label.contains("Closed November 2026"), archived.label)
	}

	@MainActor
	func testClosingAGoalOffersToMoveLaterBudget() {
		// D66: later months' budget moves only when asked.
		let app = openGoals()
		let japan = app.descendants(matching: .any)["goal.japan"]
		XCTAssertTrue(japan.waitForExistence(timeout: 5))
		japan.rightClick()
		contextItem(app, "Close Goal…")?.click()
		let later = app.checkBoxes["closeGoal.alsoMoveLater"]
		XCTAssertTrue(later.waitForExistence(timeout: 5))
		XCTAssertEqual(later.value as? Int, 0, "unchecked by default")
		later.click()
		attachScreenshot(of: app, named: "close-goal-later-budget")
		app.buttons["closeGoal.close"].click()
		XCTAssertTrue(app.descendants(matching: .any)["sheet.closeGoal"].waitForDisappearance())
	}

	@MainActor
	func testReopeningAClosedGoalAndChangingATarget() {
		let app = openGoals()
		let vet = app.descendants(matching: .any)["goal.vet"]
		XCTAssertTrue(vet.waitForExistence(timeout: 5))
		vet.rightClick()
		XCTAssertNil(contextItem(app, "Change Target…"), "a closed goal is reopened first")
		contextItem(app, "Reopen Goal")?.click()
		XCTAssertTrue(waitUntil { app.descendants(matching: .any)["goal.vet"].label.contains("Closed") == false })
		let laptop = app.descendants(matching: .any)["goal.laptop"]
		laptop.rightClick()
		contextItem(app, "Change Target…")?.click()
		let sheet = app.descendants(matching: .any)["sheet.retargetGoal"]
		XCTAssertTrue(sheet.waitForExistence(timeout: 5))
		let target = sheet.textFields["amountField"]
		target.typeKey("a", modifierFlags: .command)
		target.typeText("2000")
		target.typeKey(.return, modifierFlags: [])
		XCTAssertTrue(sheet.waitForDisappearance())
		XCTAssertTrue(waitUntil { laptop.label.hasPrefix("New laptop, $1,120 of $2,000") }, laptop.label)
	}

	/// design/interaction.md: arrow keys move between goal cards and Return
	/// opens the goal; every goal action is in the Goal menu, reached here
	/// through Help's menu search so no mouse is used.
	@MainActor
	func testGoalsFromTheKeyboardAlone() {
		let app = openGoals()
		let laptop = app.descendants(matching: .any)["goal.laptop"]
		XCTAssertTrue(laptop.waitForExistence(timeout: 5))
		app.typeKey(.downArrow, modifierFlags: [])
		XCTAssertTrue(waitUntil { laptop.isSelected }, "↓ selects the first goal")
		app.typeKey(.return, modifierFlags: [])
		let retarget = app.descendants(matching: .any)["sheet.retargetGoal"]
		XCTAssertTrue(retarget.waitForExistence(timeout: 5), "Return opens the goal")
		let target = retarget.textFields["amountField"]
		target.typeKey("a", modifierFlags: .command)
		target.typeText("1800")
		target.typeKey(.return, modifierFlags: [])
		XCTAssertTrue(retarget.waitForDisappearance())
		XCTAssertTrue(waitUntil { laptop.label.hasPrefix("New laptop, $1,120 of $1,800") }, laptop.label)

		app.typeKey(.rightArrow, modifierFlags: [])
		app.typeKey(.rightArrow, modifierFlags: [])
		let tires = app.descendants(matching: .any)["goal.tires"]
		XCTAssertTrue(waitUntil { tires.isSelected })
		menuSearch(app, "Close Goal…")
		let close = app.descendants(matching: .any)["sheet.closeGoal"]
		XCTAssertTrue(close.waitForExistence(timeout: 5))
		app.typeKey(.return, modifierFlags: [])
		XCTAssertTrue(close.waitForDisappearance(), "Return closes the goal")
		XCTAssertTrue(waitUntil { tires.label.contains("Closed") }, tires.label)

		let vet = app.descendants(matching: .any)["goal.vet"]
		app.typeKey(.downArrow, modifierFlags: [])
		XCTAssertTrue(waitUntil { vet.isSelected }, "↓ moves from Winter tires, now archived, to the next archived goal")
		menuSearch(app, "Reopen Goal")
		XCTAssertTrue(waitUntil { !vet.label.contains("Closed") }, vet.label)
	}

	/// GOAL-3: the over-limit warning shows as soon as the envelope is picked.
	@MainActor
	func testTheCloseWarningShowsBeforeTheButtonIsPressed() {
		let app = openGoals()
		let close = app.buttons["goal.tires.close"]
		XCTAssertTrue(close.waitForExistence(timeout: 5))
		close.click()
		let sheet = app.descendants(matching: .any)["sheet.closeGoal"]
		XCTAssertTrue(sheet.waitForExistence(timeout: 5))
		XCTAssertFalse(sheet.descendants(matching: .any)["closeGoal.warning"].exists, "To Budget has room")
		sheet.popUpButtons.firstMatch.click()
		app.menuItems.matching(NSPredicate(format: "title BEGINSWITH %@", "Dining out")).firstMatch.click()
		XCTAssertTrue(sheet.descendants(matching: .any)["closeGoal.warning"].waitForExistence(timeout: 5))
		XCTAssertTrue(sheet.staticTexts["Dining out will be $14.75 over."].exists)
		attachScreenshot(of: app, named: "close-goal-warning")
		XCTAssertTrue(sheet.exists, "nothing was pressed")
		sheet.typeKey(.escape, modifierFlags: [])
		XCTAssertTrue(sheet.waitForDisappearance())
	}

	/// Shift-⌘-/ searches the menus; Return runs the highlighted item.
	@MainActor
	private func menuSearch(_ app: XCUIApplication, _ item: String) {
		app.typeKey("/", modifierFlags: [.command, .shift])
		app.typeText(item)
		XCTAssertTrue(app.menuItems[item].waitForExistence(timeout: 5))
		app.typeKey(.downArrow, modifierFlags: [])
		app.typeKey(.return, modifierFlags: [])
	}

	@MainActor
	func testAddingRenamingAndDeletingAnEnvelope() {
		let app = openBudget()
		app.descendants(matching: .any)["group.housing"].rightClick()
		app.menuItems["New Envelope…"].click()
		nameAndSave(app, "Water")
		let water = app.descendants(matching: .any)["envelope.new-1"]
		XCTAssertTrue(water.waitForExistence(timeout: 5))
		water.rightClick()
		app.menuItems["Rename…"].click()
		let field = app.textFields["namePrompt.name"]
		XCTAssertTrue(field.waitForExistence(timeout: 5))
		field.typeKey("a", modifierFlags: .command)
		nameAndSave(app, "Water and sewer")
		XCTAssertTrue(waitUntil { water.label.hasPrefix("Water and sewer") }, water.label)
		water.rightClick()
		app.menuItems["Delete…"].click()
		let delete = app.buttons["Delete"]
		XCTAssertTrue(delete.waitForExistence(timeout: 5), "no history, so a plain delete")
		delete.click()
		XCTAssertTrue(water.waitForDisappearance())
	}

	@MainActor
	func testDeletingAnEnvelopeWithHistoryArchivesItInstead() {
		// R-BUD-5
		let app = openBudget()
		let groceries = app.descendants(matching: .any)["envelope.groceries"]
		groceries.rightClick()
		app.menuItems["Delete…"].click()
		XCTAssertTrue(app.staticTexts["Archive Groceries?"].waitForExistence(timeout: 5))
		attachScreenshot(of: app, named: "archive-instead-of-delete", wholeScreen: true)
		app.buttons["Archive"].click()
		XCTAssertTrue(groceries.waitForDisappearance(), "archived envelopes are hidden")
		app.menuBars.menuBarItems["View"].click()
		app.menuItems["Show Hidden Envelopes"].click()
		XCTAssertTrue(groceries.waitForExistence(timeout: 5), "its history is still there")
	}

	@MainActor
	func testDeletingWithHistoryMovesItBehindTheExplicitConfirmation() {
		let app = openBudget()
		app.descendants(matching: .any)["envelope.dining"].rightClick()
		app.menuItems["Delete…"].click()
		XCTAssertTrue(app.buttons["Delete and Move History…"].waitForExistence(timeout: 5))
		app.buttons["Delete and Move History…"].click()
		let sheet = app.descendants(matching: .any)["sheet.deleteMoving"]
		XCTAssertTrue(sheet.waitForExistence(timeout: 5))
		let delete = sheet.buttons["deleteMoving.delete"]
		XCTAssertFalse(delete.isEnabled)
		sheet.popUpButtons["deleteMoving.destination"].click()
		app.menuItems["Groceries"].click()
		XCTAssertFalse(delete.isEnabled, "still needs the explicit confirmation")
		sheet.checkBoxes["deleteMoving.understood"].click()
		attachScreenshot(of: app, named: "delete-moving-history")
		delete.click()
		XCTAssertTrue(sheet.waitForDisappearance())
		XCTAssertFalse(app.descendants(matching: .any)["envelope.dining"].exists)
		XCTAssertEqual(app.buttons["envelope.groceries.budgeted"].label, "Budgeted 592.00")
	}

	@MainActor
	func testAddingAGroupAndMovingAnEnvelopeUp() {
		let app = openBudget()
		let newGroup = app.buttons["budget.newGroup"]
		let budget = app.screen("budget")
		scroll(budget, until: newGroup, by: -600)
		newGroup.click()
		nameAndSave(app, "Kids")
		XCTAssertTrue(app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH 'group.group-'")).firstMatch.waitForExistence(timeout: 5))
		scroll(budget, until: app.descendants(matching: .any)["envelope.groceries"], by: 600)
		let household = app.descendants(matching: .any)["envelope.household"]
		let groceries = app.descendants(matching: .any)["envelope.groceries"]
		household.rightClick()
		for _ in 0..<3 {
			app.menuItems["Move Up"].click()
			household.rightClick()
		}
		XCTAssertFalse(app.menuItems["Move Up"].isEnabled, "Household is first in its group")
		app.typeKey(.escape, modifierFlags: [])
		XCTAssertTrue(waitUntil { household.frame.minY < groceries.frame.minY }, "Household now sits above Groceries")
		attachScreenshot(of: app, named: "envelope-dragged")
	}

	/// The window may be shorter than the Budget screen; bring `element` into view.
	@MainActor
	private func scroll(_ view: XCUIElement, until element: XCUIElement, by delta: CGFloat) {
		for _ in 0..<6 where !element.isHittable {
			view.scroll(byDeltaX: 0, deltaY: delta)
		}
		XCTAssertTrue(element.isHittable, "scrolled \(element) into view")
	}

	@MainActor
	private func nameAndSave(_ app: XCUIApplication, _ name: String) {
		let field = app.textFields["namePrompt.name"]
		XCTAssertTrue(field.waitForExistence(timeout: 5))
		field.typeText(name)
		app.buttons["namePrompt.save"].click()
		XCTAssertTrue(field.waitForDisappearance())
	}

	@MainActor
	private func openBudget() -> XCUIApplication {
		let app = XCUIApplication.veritas(stub: "open")
		XCTAssertTrue(app.buttons["envelope.groceries.budgeted"].waitForExistence(timeout: 10))
		return app
	}

	/// The Goal menu repeats every context-menu item, so a title alone matches
	/// two; the open context menu's item is the one on screen.
	@MainActor
	private func contextItem(_ app: XCUIApplication, _ title: String) -> XCUIElement? {
		app.menuItems.matching(NSPredicate(format: "title == %@", title)).allElementsBoundByIndex.first { $0.isHittable }
	}

	@MainActor
	private func openGoals() -> XCUIApplication {
		let app = openBudget()
		app.typeKey("4", modifierFlags: .command)
		XCTAssertTrue(app.descendants(matching: .any)["screen.goals"].waitForExistence(timeout: 5))
		return app
	}

	private func waitUntil(_ condition: @escaping () -> Bool) -> Bool {
		let expectation = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: nil)
		return XCTWaiter().wait(for: [expectation], timeout: 5) == .completed
	}
}
