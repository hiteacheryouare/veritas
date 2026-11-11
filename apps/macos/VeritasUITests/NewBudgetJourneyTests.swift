import XCTest

/// R-IO-0, D53, D56: first launch offers New budget and Import from Actual as
/// equal choices, and New budget walks its three steps to an open budget.
final class NewBudgetJourneyTests: XCTestCase {
	override func setUp() {
		continueAfterFailure = false
	}

	@MainActor
	func testFirstLaunchOffersBothWaysInEqually() {
		let app = XCUIApplication.veritas(stub: "empty")
		XCTAssertTrue(app.buttons["newBudget"].waitForExistence(timeout: 10))
		XCTAssertTrue(app.buttons["importFromActual"].exists)
		app.typeKey(.return, modifierFlags: [])
		XCTAssertFalse(app.descendants(matching: .any)["newBudget.basics"].waitForExistence(timeout: 1), "neither choice is the default")
		attachScreenshot(of: app, named: "first-launch")
	}

	@MainActor
	func testANewBudgetWalksEveryStepToAnOpenBudget() {
		let app = XCUIApplication.veritas(stub: "empty")
		XCTAssertTrue(app.buttons["newBudget"].waitForExistence(timeout: 10))
		app.buttons["newBudget"].click()

		let name = app.textFields["newBudget.name"]
		XCTAssertTrue(name.waitForExistence(timeout: 5))
		name.typeText("Household")
		attachScreenshot(of: app, named: "new-budget-1-basics")
		app.buttons["newBudget.continue"].click()

		let accountName = app.textFields["newBudget.account.name"].firstMatch
		XCTAssertTrue(accountName.waitForExistence(timeout: 5))
		accountName.click()
		accountName.typeText("Checking")
		let balance = app.textFields["newBudget.account.balance"].firstMatch
		balance.click()
		balance.typeText("2140.18+")
		app.buttons["newBudget.continue"].click()
		XCTAssertTrue(app.staticTexts.element(matching: NSPredicate(format: "value BEGINSWITH %@", "Can't read")).waitForExistence(timeout: 5))
		attachScreenshot(of: app, named: "new-budget-2-accounts-problem")
		balance.click()
		balance.typeKey("a", modifierFlags: .command)
		balance.typeText("2140.18")
		attachScreenshot(of: app, named: "new-budget-2-accounts")
		app.buttons["newBudget.continue"].click()

		let retirement = app.checkBoxes["newBudget.retirementPlan"]
		XCTAssertTrue(retirement.waitForExistence(timeout: 5))
		retirement.click()
		attachScreenshot(of: app, named: "new-budget-3-401k-and-hsa")
		app.buttons["newBudget.continue"].click()

		XCTAssertTrue(app.screen("budget").waitForExistence(timeout: 10))
		XCTAssertTrue(app.sidebar("account.new-0").exists)
		attachScreenshot(of: app, named: "new-budget-opened")
	}

	@MainActor
	func testCancellingSetupReturnsToFirstLaunch() {
		let app = XCUIApplication.veritas(stub: "empty")
		XCTAssertTrue(app.buttons["newBudget"].waitForExistence(timeout: 10))
		app.buttons["newBudget"].click()
		XCTAssertTrue(app.textFields["newBudget.name"].waitForExistence(timeout: 5))
		app.typeKey(.escape, modifierFlags: [])
		XCTAssertTrue(app.textFields["newBudget.name"].waitForDisappearance())
		XCTAssertTrue(app.buttons["newBudget"].exists)
	}
}
