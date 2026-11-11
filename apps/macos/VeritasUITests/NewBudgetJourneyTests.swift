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
		let type = app.popUpButtons["newBudget.account.type"].firstMatch
		XCTAssertEqual(type.value as? String, "Checking")
		type.click()
		XCTAssertTrue(app.menuItems["Savings"].waitForExistence(timeout: 5))
		attachScreenshot(of: app, named: "new-budget-2-type-picker", wholeScreen: true)
		app.menuItems["Savings"].click()
		XCTAssertEqual(type.value as? String, "Savings")
		attachScreenshot(of: app, named: "new-budget-2-accounts")
		app.buttons["newBudget.continue"].click()

		let retirement = app.checkBoxes["newBudget.retirementPlan"]
		XCTAssertTrue(retirement.waitForExistence(timeout: 5))
		retirement.click()
		attachScreenshot(of: app, named: "new-budget-3-401k-and-hsa")
		app.buttons["newBudget.continue"].click()

		XCTAssertTrue(app.screen("budget").waitForExistence(timeout: 10))
		XCTAssertTrue(app.sidebar("account.new-0").exists)
		XCTAssertTrue(app.staticTexts["Cash"].exists)
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

/// The same journey against the real core (R-IO-0): the budget is created from
/// the pinned template in a scratch folder, and the sidebar shows what the
/// core placed where.
final class RealCoreNewBudgetTests: XCTestCase {
	override func setUp() {
		continueAfterFailure = false
	}

	@MainActor
	func testANewBudgetIsCreatedByTheCoreWithItsAccounts() {
		let app = XCUIApplication()
		app.launchArguments = ["-ApplePersistenceIgnoreState", "YES", "-VeritasRealCore", "YES"]
		app.launch()
		XCTAssertTrue(app.buttons["newBudget"].waitForExistence(timeout: 10))
		app.buttons["newBudget"].click()

		let name = app.textFields["newBudget.name"]
		XCTAssertTrue(name.waitForExistence(timeout: 5))
		name.typeText("UI test budget")
		let currency = app.popUpButtons["newBudget.currency"]
		XCTAssertTrue(waitUntil { currency.isEnabled })
		currency.click()
		app.menuItems["US Dollar (USD)"].click()
		XCTAssertEqual(app.popUpButtons["newBudget.numberFormat"].value as? String, "1,000.33")
		attachScreenshot(of: app, named: "real-core-1-basics")
		app.buttons["newBudget.continue"].click()

		let accountName = app.textFields["newBudget.account.name"].firstMatch
		XCTAssertTrue(accountName.waitForExistence(timeout: 5))
		accountName.click()
		accountName.typeText("Checking")
		let balance = app.textFields["newBudget.account.balance"].firstMatch
		balance.click()
		balance.typeText("2,000+140.18")
		app.buttons["newBudget.addAccount"].click()
		let cardName = app.textFields.matching(identifier: "newBudget.account.name").element(boundBy: 1)
		cardName.click()
		cardName.typeText("Visa")
		let cardType = app.popUpButtons.matching(identifier: "newBudget.account.type").element(boundBy: 1)
		cardType.click()
		app.menuItems["Credit card"].click()
		let cardBalance = app.textFields.matching(identifier: "newBudget.account.balance").element(boundBy: 1)
		cardBalance.click()
		cardBalance.typeText("612.33")
		attachScreenshot(of: app, named: "real-core-2-accounts")
		app.buttons["newBudget.continue"].click()

		let retirement = app.checkBoxes["newBudget.retirementPlan"]
		XCTAssertTrue(retirement.waitForExistence(timeout: 5))
		retirement.click()
		app.buttons["newBudget.continue"].click()

		XCTAssertTrue(app.screen("budget").waitForExistence(timeout: 15))
		XCTAssertTrue(app.staticTexts["Cash"].exists)
		XCTAssertTrue(app.staticTexts["Credit"].exists)
		XCTAssertTrue(app.staticTexts["Investments & Other assets"].exists, "the 401(k)")
		let checking = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH 'sidebar.account.' AND label CONTAINS 'Checking'")).firstMatch
		XCTAssertTrue(checking.exists)
		XCTAssertTrue(checking.label.contains("2,140.18"), checking.label)
		let visa = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH 'sidebar.account.' AND label CONTAINS 'Visa'")).firstMatch
		XCTAssertTrue(visa.label.contains("612.33"), visa.label)
		attachScreenshot(of: app, named: "real-core-3-opened")
	}

	private func waitUntil(_ condition: @escaping () -> Bool) -> Bool {
		let expectation = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: nil)
		return XCTWaiter().wait(for: [expectation], timeout: 10) == .completed
	}
}
