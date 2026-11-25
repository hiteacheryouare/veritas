import XCTest

/// Switching budgets as Actual does: the budget's name at the top of the
/// sidebar lists recent budgets, as does File › Open Recent, and choosing one
/// closes this budget and opens that one in the same window. Against the real
/// core, so each switch goes through the writer lock.
final class BudgetSwitcherJourneyTests: XCTestCase {
	override func setUp() {
		continueAfterFailure = false
	}

	@MainActor
	func testTheSidebarAndOpenRecentSwitchBetweenTwoBudgets() {
		let app = XCUIApplication()
		app.launchArguments = ["-ApplePersistenceIgnoreState", "YES", "-VeritasRealCore", "YES"]
		app.launch()
		XCTAssertTrue(app.buttons["newBudget"].waitForExistence(timeout: 10))
		app.buttons["newBudget"].click()
		createBudget(named: "Household", account: "Everyday", in: app)

		let switcher = app.descendants(matching: .any)["budgetSwitcher"]
		XCTAssertTrue(waitUntil { self.text(of: switcher).contains("Household") }, text(of: switcher))
		switcher.click()
		XCTAssertFalse(menuItem("Household", in: app).exists, "the open budget isn't offered again")
		app.menuItems["New Budget…"].click()
		createBudget(named: "Side business", account: "Business checking", in: app)
		XCTAssertTrue(waitUntil { self.text(of: switcher).contains("Side business") }, text(of: switcher))

		switcher.click()
		let household = menuItem("Household", in: app)
		XCTAssertTrue(household.waitForExistence(timeout: 5), "the first budget is in the list")
		attachScreenshot(of: app, named: "budget-switcher-menu", wholeScreen: true)
		household.click()
		XCTAssertTrue(waitUntil { self.text(of: switcher).contains("Household") }, text(of: switcher))
		XCTAssertTrue(sidebarAccount(named: "Everyday", in: app).waitForExistence(timeout: 10), "its accounts are back")
		XCTAssertFalse(sidebarAccount(named: "Business checking", in: app).exists)
		XCTAssertFalse(app.descendants(matching: .any)["readOnlyBanner"].exists, "the budget closed before this one opened, so it's writable")

		app.menuBars.menuBarItems["File"].click()
		app.menuItems["Open Recent"].hover()
		let business = menuItem("Side business", in: app)
		XCTAssertTrue(business.waitForExistence(timeout: 5))
		attachScreenshot(of: app, named: "open-recent-menu", wholeScreen: true)
		business.click()
		XCTAssertTrue(waitUntil { self.text(of: switcher).contains("Side business") }, text(of: switcher))
		XCTAssertTrue(sidebarAccount(named: "Business checking", in: app).waitForExistence(timeout: 10))
	}

	@MainActor
	private func createBudget(named name: String, account: String, in app: XCUIApplication) {
		let field = app.textFields["newBudget.name"]
		XCTAssertTrue(field.waitForExistence(timeout: 5))
		field.click()
		field.typeText(name)
		let currency = app.popUpButtons["newBudget.currency"]
		XCTAssertTrue(waitUntil { currency.isEnabled })
		currency.click()
		app.menuItems["US Dollar (USD)"].click()
		app.buttons["newBudget.continue"].click()
		let accountName = app.textFields["newBudget.account.name"].firstMatch
		XCTAssertTrue(accountName.waitForExistence(timeout: 5))
		accountName.click()
		accountName.typeText(account)
		let balance = app.textFields["newBudget.account.balance"].firstMatch
		balance.click()
		balance.typeText("100")
		app.buttons["newBudget.continue"].click()
		XCTAssertTrue(app.checkBoxes["newBudget.retirementPlan"].waitForExistence(timeout: 5))
		app.buttons["newBudget.continue"].click()
		XCTAssertTrue(app.screen("budget").waitForExistence(timeout: 15), "\(name) opens")
	}

	/// A recent budget's menu item, by its identifier or, where a menu drops
	/// identifiers, by the name it starts with.
	@MainActor
	private func menuItem(_ name: String, in app: XCUIApplication) -> XCUIElement {
		app.menuItems.matching(NSPredicate(format: "identifier == %@ OR title BEGINSWITH %@", "recentBudget.\(name)", name)).firstMatch
	}

	@MainActor
	private func sidebarAccount(named name: String, in app: XCUIApplication) -> XCUIElement {
		app.descendants(matching: .any)
			.matching(NSPredicate(format: "identifier BEGINSWITH 'sidebar.account.' AND (label CONTAINS %@ OR value CONTAINS %@)", name, name))
			.firstMatch
	}

	private func waitUntil(_ condition: @escaping () -> Bool) -> Bool {
		let expectation = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: nil)
		return XCTWaiter().wait(for: [expectation], timeout: 10) == .completed
	}

	@MainActor
	private func text(of element: XCUIElement) -> String {
		[element.label, element.value as? String, element.title].compactMap(\.self).joined(separator: " ")
	}
}
