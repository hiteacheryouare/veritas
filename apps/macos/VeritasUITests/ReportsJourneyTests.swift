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
		XCTAssertEqual(text(sheet.staticTexts["drilldown.count"]), "47 transactions")
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
		app.screen("sankey").typeKey(.leftArrow, modifierFlags: .command)
		let dining = app.buttons["sankey.node.category.dining"]
		XCTAssertTrue(dining.waitForExistence(timeout: 5))
		XCTAssertTrue(dining.label.hasPrefix("Dining out, 401.38"), dining.label)
		attachScreenshot(of: app, named: "sankey")
		dining.click()
		let sheet = app.descendants(matching: .any)["sheet.drilldown"]
		XCTAssertTrue(sheet.waitForExistence(timeout: 5))
		XCTAssertEqual(text(sheet.staticTexts["drilldown.count"]), "11 transactions")
	}

	@MainActor
	func testTheDashboardDrawsEveryWidgetAndMovesOne() {
		// D73: nothing opens in Actual. A type Veritas doesn't know is a plain card.
		let app = openReport("dashboard")
		let future = app.descendants(matching: .any)["widget.future"]
		XCTAssertTrue(present(future, in: app))
		XCTAssertEqual(future.label, "future-widget-card")
		XCTAssertTrue(app.descendants(matching: .any)["widget.salary"].descendants(matching: .any)["widget.figure"].exists, "a summary card shows its figure")
		let worth = app.descendants(matching: .any)["widget.worth"]
		let before = worth.frame
		worth.rightClick()
		app.menuItems["Move"].hover()
		app.menuItems["Down"].click()
		XCTAssertTrue(waitUntil { worth.frame.minY > before.minY })
		app.descendants(matching: .any)["dashboard.edit"].click()
		attachScreenshot(of: app, named: "dashboard-editing")
	}

	@MainActor
	func testAddingConfiguringAndRemovingAWidget() {
		let app = openReport("dashboard")
		app.descendants(matching: .any)["dashboard.add"].click()
		app.menuItems["Summary"].click()
		let added = app.descendants(matching: .any)["widget.added-1"]
		XCTAssertTrue(present(added, in: app), "the new summary card is on the page")
		added.rightClick()
		app.menuItems["Configure…"].click()
		let sheet = app.descendants(matching: .any)["sheet.widgetConfig"]
		XCTAssertTrue(sheet.waitForExistence(timeout: 5))
		let name = sheet.textFields["widgetConfig.name"]
		name.click()
		name.typeText("Groceries YTD")
		sheet.buttons["filters.add"].click()
		let value = sheet.textFields["filters.value"]
		value.click()
		value.typeText("Groceries")
		attachScreenshot(of: app, named: "widget-config")
		sheet.buttons["widgetConfig.save"].click()
		XCTAssertTrue(sheet.waitForDisappearance())
		XCTAssertTrue(waitUntil { added.label == "Groceries YTD" }, added.label)
		added.rightClick()
		app.menuItems["Remove Widget…"].click()
		app.buttons["Remove"].click()
		XCTAssertTrue(added.waitForDisappearance())
	}

	@MainActor
	func testDashboardsAreMadeRenamedAndDeleted() {
		let app = openReport("dashboard")
		let pages = app.descendants(matching: .any)["dashboard.pages"]
		pages.click()
		app.menuItems["New Dashboard…"].click()
		let field = app.textFields["namePrompt.name"]
		XCTAssertTrue(field.waitForExistence(timeout: 5))
		field.typeText("Taxes")
		app.buttons["namePrompt.save"].click()
		XCTAssertTrue(waitUntil { app.windows.firstMatch.title == "Taxes" }, app.windows.firstMatch.title)
		XCTAssertTrue(present(app.descendants(matching: .any)["dashboard.empty"], in: app), "a new dashboard starts empty")
		pages.click()
		app.menuItems["Rename Dashboard…"].click()
		XCTAssertTrue(field.waitForExistence(timeout: 5))
		field.typeKey("a", modifierFlags: .command)
		field.typeText("Taxes 2026")
		app.buttons["namePrompt.save"].click()
		XCTAssertTrue(waitUntil { app.windows.firstMatch.title == "Taxes 2026" }, app.windows.firstMatch.title)
		pages.click()
		app.menuItems["Delete Dashboard…"].click()
		app.buttons["Delete Dashboard"].click()
		XCTAssertTrue(waitUntil { app.windows.firstMatch.title == "Main" }, app.windows.firstMatch.title)
	}

	@MainActor
	func testCreatingACustomReportWithAFilter() {
		let app = openReport("customReports")
		app.buttons["customReport.new"].click()
		let name = app.textFields["customReport.name"]
		XCTAssertTrue(name.waitForExistence(timeout: 5))
		name.click()
		name.typeKey("a", modifierFlags: .command)
		name.typeText("Dining")
		app.buttons["filters.add"].click()
		let value = app.textFields["filters.value"]
		value.click()
		value.typeText("Dining out")
		attachScreenshot(of: app, named: "custom-report-new")
		app.buttons["customReport.save"].click()
		XCTAssertTrue(name.waitForDisappearance())
		XCTAssertTrue(waitUntil { self.text(app.buttons["reports.custom.report-1"]).contains("Dining") }, text(app.buttons["reports.custom.report-1"]))
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
		XCTAssertTrue(waitUntil { self.text(app.buttons["reports.custom.spending"]).contains("Where it went") }, text(app.buttons["reports.custom.spending"]))
	}

	@MainActor
	func testTheStandardReportsAreOneClickAway() {
		let app = openReport("customReports")
		for (report, row) in [("cashFlow", "standard.cashFlow.0.opening"), ("netWorth", "standard.netWorth.netWorth"), ("incomeVsExpense", "standard.incomeVsExpense.netProfit")] {
			app.buttons["reports.standard.\(report)"].click()
			XCTAssertTrue(present(app.descendants(matching: .any)["report.\(report)"], in: app), report)
			XCTAssertTrue(app.descendants(matching: .any)[row].firstMatch.exists, row)
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

	/// SwiftUI puts a combined element's text in `value` rather than `label`.
	private func text(_ element: XCUIElement) -> String {
		element.label.isEmpty ? (element.value as? String ?? "") : element.label
	}

	/// Waits for `element`; when it never shows, the hierarchy is attached as evidence.
	@MainActor
	private func present(_ element: XCUIElement, in app: XCUIApplication) -> Bool {
		if element.waitForExistence(timeout: 5) { return true }
		let tree = XCTAttachment(string: app.debugDescription)
		tree.name = "hierarchy when \(element) was missing"
		tree.lifetime = .keepAlways
		add(tree)
		attachScreenshot(of: app, named: "missing")
		return false
	}

	private func waitUntil(_ condition: @escaping () -> Bool) -> Bool {
		let expectation = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: nil)
		return XCTWaiter().wait(for: [expectation], timeout: 5) == .completed
	}
}
