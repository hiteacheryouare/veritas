import XCTest

/// D63 and CLS-12 against the real core: budgets made through New Budget in a
/// scratch folder, so each journey has a 2-place and a 0-place budget.
final class CurrencyJourneyTests: XCTestCase {
	override func setUp() {
		continueAfterFailure = false
	}

	/// The PR #61 review's failure: open a JPY budget after a USD one, type
	/// 5000, and it must read ¥5,000, not save ¥500,000. Then the other way.
	@MainActor
	func testSwitchingBetweenDollarsAndYenTypesAtEachBudgetsPlaces() {
		let app = launch()
		createBudget(app, named: "Dollars", currency: "US Dollar (USD)", first: true)
		createBudget(app, named: "Yen", currency: "Japanese Yen (JPY)")

		let cell = firstBudgetedCell(app)
		let cellID = cell.identifier
		XCTAssertEqual(cell.label, "Budgeted 0")
		XCTAssertTrue(app.descendants(matching: .any)["mixedScaleNote"].exists, "CLS-12.10's line shows in a 0-place budget")
		budget(app, cellID, "5000")
		XCTAssertTrue(waitUntil { app.buttons[cellID].label == "Budgeted 5,000" }, app.buttons[cellID].label)
		app.buttons[cellID].click()
		let field = app.textFields["amountField"]
		XCTAssertTrue(field.waitForExistence(timeout: 5))
		XCTAssertEqual(field.value as? String, "5,000", "the field opens on what the row shows")
		field.typeKey(.escape, modifierFlags: [])
		attachScreenshot(of: app, named: "yen-budget")

		openBudget(app, named: "Dollars")
		let dollars = firstBudgetedCell(app)
		XCTAssertFalse(app.descendants(matching: .any)["mixedScaleNote"].exists)
		budget(app, dollars.identifier, "50")
		XCTAssertTrue(waitUntil { app.buttons[dollars.identifier].label == "Budgeted 50.00" }, app.buttons[dollars.identifier].label)

		openBudget(app, named: "Yen")
		XCTAssertTrue(app.buttons[cellID].waitForExistence(timeout: 10))
		XCTAssertEqual(app.buttons[cellID].label, "Budgeted 5,000", "the stored 5000 was never rescaled")
	}

	/// Undo from the Edit menu while an untouched field is open shows the
	/// undone value in that field (`refreshEditing`).
	@MainActor
	func testUndoFromTheMenuUpdatesAnOpenUntouchedField() {
		let app = launch()
		createBudget(app, named: "Dollars", currency: "US Dollar (USD)", first: true)
		let cell = firstBudgetedCell(app)
		let cellID = cell.identifier
		cell.click()
		let field = app.textFields["amountField"]
		XCTAssertTrue(field.waitForExistence(timeout: 5))
		field.typeText("50")
		field.typeKey(.return, modifierFlags: [])
		XCTAssertTrue(waitUntil { app.buttons[cellID].label == "Budgeted 50.00" })
		field.typeKey(.return, modifierFlags: .shift)
		XCTAssertTrue(waitUntil { field.value as? String == "50.00" }, "Shift-Return goes back to the first envelope, untouched")
		app.menuBars.menuBarItems["Edit"].click()
		app.menuBars.menuItems["Undo"].click()
		XCTAssertTrue(waitUntil { field.value as? String == "0.00" }, String(describing: field.value))
		field.typeKey(.escape, modifierFlags: [])
		XCTAssertTrue(waitUntil { app.buttons[cellID].label == "Budgeted 0.00" })
	}

	@MainActor
	private func launch() -> XCUIApplication {
		let app = XCUIApplication()
		app.launchArguments = ["-ApplePersistenceIgnoreState", "YES", "-VeritasRealCore", "YES"]
		app.launch()
		return app
	}

	/// New Budget with no accounts and no paycheck plans.
	@MainActor
	private func createBudget(_ app: XCUIApplication, named name: String, currency: String, first: Bool = false) {
		if first {
			XCTAssertTrue(app.buttons["newBudget"].waitForExistence(timeout: 10))
			app.buttons["newBudget"].click()
		} else {
			app.menuBars.menuBarItems["File"].click()
			app.menuBars.menuItems["New Budget…"].click()
		}
		let field = app.textFields["newBudget.name"]
		XCTAssertTrue(field.waitForExistence(timeout: 5))
		field.typeText(name)
		let picker = app.popUpButtons["newBudget.currency"]
		XCTAssertTrue(waitUntil { picker.isEnabled })
		picker.click()
		app.menuItems[currency].click()
		for _ in 0..<3 {
			app.buttons["newBudget.continue"].click()
		}
		XCTAssertTrue(app.screen("budget").waitForExistence(timeout: 15))
		XCTAssertTrue(firstBudgetedCell(app).waitForExistence(timeout: 10))
	}

	@MainActor
	private func openBudget(_ app: XCUIApplication, named name: String) {
		app.typeKey("o", modifierFlags: .command)
		let sheet = app.descendants(matching: .any)["sheet.openBudget"]
		XCTAssertTrue(sheet.waitForExistence(timeout: 5))
		let row = sheet.descendants(matching: .any)
			.matching(NSPredicate(format: "identifier BEGINSWITH 'openBudget.' AND (label BEGINSWITH %@ OR value BEGINSWITH %@)", name, name))
			.firstMatch
		XCTAssertTrue(row.waitForExistence(timeout: 5))
		row.click()
		sheet.buttons["openBudget.open"].click()
		XCTAssertTrue(sheet.waitForDisappearance())
		XCTAssertTrue(app.screen("budget").waitForExistence(timeout: 15))
		XCTAssertTrue(firstBudgetedCell(app).waitForExistence(timeout: 10))
	}

	/// Types `amount` into the cell's field, commits with Return, then closes
	/// the next envelope's field with Esc.
	@MainActor
	private func budget(_ app: XCUIApplication, _ cellID: String, _ amount: String) {
		app.buttons[cellID].click()
		let field = app.textFields["amountField"]
		XCTAssertTrue(field.waitForExistence(timeout: 5))
		field.typeText(amount)
		field.typeKey(.return, modifierFlags: [])
		field.typeKey(.escape, modifierFlags: [])
		XCTAssertTrue(field.waitForDisappearance())
	}

	/// Real envelopes have core ids, so the first spending envelope is found by shape.
	@MainActor
	private func firstBudgetedCell(_ app: XCUIApplication) -> XCUIElement {
		app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH 'envelope.' AND identifier ENDSWITH '.budgeted'")).firstMatch
	}

	private func waitUntil(_ condition: @escaping () -> Bool) -> Bool {
		let expectation = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: nil)
		return XCTWaiter().wait(for: [expectation], timeout: 10) == .completed
	}
}
