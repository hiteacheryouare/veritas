import XCTest

/// Reports against the stub (R-REP-2 to R-REP-6): every number opens its
/// transactions, placeholders stay put, and edits save back.
final class ReportsJourneyTests: XCTestCase {
	override func setUp() {
		continueAfterFailure = false
	}

	@MainActor
	func testANetProfitCellOpensItsTransactions() {
		let app = openReport("netProfit")
		let august = app.buttons["netProfit.cell.expenses.2"]
		XCTAssertTrue(august.waitForExistence(timeout: 5))
		XCTAssertTrue(august.label.hasPrefix("Expenses, August 2026"), august.label)
		attachScreenshot(of: app, named: "net-profit")
		august.click()
		let sheet = app.descendants(matching: .any)["sheet.drilldown"]
		XCTAssertTrue(sheet.waitForExistence(timeout: 5))
		XCTAssertEqual(sheet.staticTexts["drilldown.count"].label, "47 transactions")
		attachScreenshot(of: app, named: "drilldown")
		sheet.buttons["Done"].click()
		XCTAssertTrue(sheet.waitForDisappearance())
	}

	@MainActor
	func testNetProfitExpandsIntoCategoriesAndStepsWithTheGoMenu() {
		let app = openReport("netProfit")
		let expand = app.buttons["netProfit.expand.expenses"]
		XCTAssertTrue(expand.waitForExistence(timeout: 5))
		expand.click()
		XCTAssertTrue(app.buttons["netProfit.cell.expenses.rent.6"].waitForExistence(timeout: 5))
		XCTAssertTrue(app.menuBars.menuBarItems["Go"].menus.menuItems["Previous Month"].isEnabled)
		app.screen("netProfit").typeKey(.leftArrow, modifierFlags: .command)
		XCTAssertTrue(app.staticTexts["December 2025 to May 2026"].waitForExistence(timeout: 5))
		attachScreenshot(of: app, named: "net-profit-earlier")
	}

	@MainActor
	func testASankeyNodeOpensItsTransactions() {
		let app = openReport("sankey")
		app.segmentedControls["sankey.range"].buttons["Month"].click()
		app.screen("sankey").typeKey(.leftArrow, modifierFlags: .command)
		let dining = app.buttons["sankey.node.category.dining"]
		XCTAssertTrue(dining.waitForExistence(timeout: 5))
		XCTAssertTrue(dining.label.hasPrefix("Dining out, 401.38"), dining.label)
		attachScreenshot(of: app, named: "sankey")
		dining.click()
		let sheet = app.descendants(matching: .any)["sheet.drilldown"]
		XCTAssertTrue(sheet.waitForExistence(timeout: 5))
		XCTAssertEqual(sheet.staticTexts["drilldown.count"].label, "14 transactions")
	}

	@MainActor
	func testTheDashboardKeepsPlaceholdersAndMovesAWidget() {
		let app = openReport("dashboard")
		let future = app.descendants(matching: .any)["widget.future"]
		XCTAssertTrue(future.waitForExistence(timeout: 5))
		XCTAssertTrue(future.staticTexts["Open in Actual"].exists)
		app.buttons["dashboard.edit"].click()
		let worth = app.descendants(matching: .any)["widget.worth"]
		let before = worth.frame
		app.buttons["widget.worth.move-down"].click()
		XCTAssertTrue(waitUntil { worth.frame.minY > before.minY })
		attachScreenshot(of: app, named: "dashboard-editing")
	}

	@MainActor
	func testEditingACustomReportSavesItBack() {
		let app = openReport("customReports")
		app.buttons["reports.custom.spending"].click()
		app.buttons["customReport.edit"].click()
		let name = app.textFields["customReport.name"]
		XCTAssertTrue(name.waitForExistence(timeout: 5))
		name.click()
		name.typeKey("a", modifierFlags: .command)
		name.typeText("Where it went")
		attachScreenshot(of: app, named: "custom-report-editing")
		app.buttons["customReport.save"].click()
		XCTAssertTrue(name.waitForDisappearance())
		XCTAssertTrue(app.buttons["reports.custom.spending"].label.contains("Where it went"))
	}

	@MainActor
	func testTheStandardReportsAreOneClickAway() {
		let app = openReport("customReports")
		for (button, table) in [("cashFlow", "standard.cashFlow"), ("netWorth", "standard.netWorth"), ("incomeVsExpense", "standard.incomeVsExpense")] {
			app.buttons["reports.standard.\(button)"].click()
			XCTAssertTrue(app.descendants(matching: .any)[table].waitForExistence(timeout: 5), table)
		}
		app.buttons["reports.standard.spendingByCategory"].click()
		XCTAssertTrue(app.descendants(matching: .any)["spending.total"].waitForExistence(timeout: 5))
		attachScreenshot(of: app, named: "spending-by-category")
	}

	@MainActor
	private func openReport(_ page: String) -> XCUIApplication {
		let app = XCUIApplication.veritas(stub: "open")
		XCTAssertTrue(app.buttons["envelope.groceries.budgeted"].waitForExistence(timeout: 10))
		app.typeKey("5", modifierFlags: .command)
		XCTAssertTrue(app.screen("dashboard").waitForExistence(timeout: 5))
		if page != "dashboard" {
			app.sidebar(page).click()
			XCTAssertTrue(app.screen(page).waitForExistence(timeout: 5))
		}
		return app
	}

	private func waitUntil(_ condition: @escaping () -> Bool) -> Bool {
		let expectation = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: nil)
		return XCTWaiter().wait(for: [expectation], timeout: 5) == .completed
	}
}
