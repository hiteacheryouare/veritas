import XCTest

/// Accounts against the stub (R-ACC-1 to R-ACC-5, R-XFER-2).
final class AccountsJourneyTests: XCTestCase {
	override func setUp() {
		continueAfterFailure = false
	}

	@MainActor
	func testTransfersReadAsToAndFromInTheRegister() {
		let app = openChecking()
		let table = app.tables["register.table"]
		XCTAssertTrue(table.staticTexts["→ To Savings"].waitForExistence(timeout: 5))
		XCTAssertTrue(table.staticTexts["← From Savings"].exists)
		XCTAssertFalse(table.staticTexts["Savings"].exists, "a transfer never reads as a bare payee")
		attachScreenshot(of: app, named: "register")
	}

	@MainActor
	func testQuickEntryFillsFromThePayeesLastTransaction() {
		// R-ACC-4, D38
		let app = openChecking()
		app.typeKey("n", modifierFlags: .command)
		let sheet = app.descendants(matching: .any)["sheet.transaction"]
		XCTAssertTrue(sheet.waitForExistence(timeout: 5))
		let payee = sheet.textFields["transaction.payee"]
		payee.typeText("Fern")
		let suggestion = sheet.buttons["transaction.suggestion.fernwood"]
		XCTAssertTrue(suggestion.waitForExistence(timeout: 5))
		suggestion.click()
		XCTAssertTrue(waitUntil { (sheet.popUpButtons["transaction.envelope"].value as? String) == "Groceries" }, "the envelope comes from the last Fernwood Market transaction")
		XCTAssertEqual(sheet.textFields["amountField"].value as? String, "82.14")
		attachScreenshot(of: app, named: "quick-entry")
		sheet.buttons["transaction.save"].click()
		XCTAssertTrue(sheet.waitForDisappearance())
	}

	@MainActor
	func testDuplicatingAndDeletingFromTheRowMenu() {
		let app = openChecking()
		let search = app.textFields["register.search"]
		search.click()
		search.typeText("Dinner with Sam\r")
		let count = app.descendants(matching: .any)["register.count"]
		XCTAssertTrue(waitUntil { self.text(count) == "1 transaction" }, text(count))
		let table = app.tables["register.table"]
		table.staticTexts["Corner Bistro"].firstMatch.rightClick()
		app.menuItems["Duplicate"].click()
		XCTAssertTrue(waitUntil { self.text(count) == "2 transactions" }, text(count))
		table.staticTexts["Corner Bistro"].firstMatch.rightClick()
		app.menuItems["Delete Transaction"].click()
		app.buttons["Delete"].click()
		XCTAssertTrue(waitUntil { self.text(count) == "1 transaction" }, text(count))
	}

	@MainActor
	func testReconcilingShowsTheDifferenceAndOnlyFinishesAtZero() {
		// R-ACC-3
		let app = openChecking()
		app.buttons["register.reconcile"].click()
		let bar = app.descendants(matching: .any)["reconcile.bar"]
		XCTAssertTrue(bar.waitForExistence(timeout: 5))
		let statement = bar.textFields["reconcile.statement"]
		statement.click()
		statement.typeKey("a", modifierFlags: .command)
		statement.typeText("1\r")
		XCTAssertTrue(bar.descendants(matching: .any)["reconcile.difference"].waitForExistence(timeout: 5))
		XCTAssertFalse(bar.buttons["reconcile.finish"].isEnabled, "a statement that doesn't match can't be finished")
		XCTAssertTrue(bar.buttons["reconcile.adjust"].exists)
		attachScreenshot(of: app, named: "reconcile")
		bar.buttons["Cancel"].click()
		XCTAssertTrue(bar.waitForDisappearance())
	}

	@MainActor
	func testClosingAnAccountMovesItsBalanceAndListsItAsClosed() {
		let app = openAccounts()
		let savings = app.descendants(matching: .any)["account.savings"]
		XCTAssertTrue(savings.waitForExistence(timeout: 5))
		savings.rightClick()
		app.menuItems["Close Account…"].click()
		let sheet = app.descendants(matching: .any)["sheet.closeAccount"]
		XCTAssertTrue(sheet.waitForExistence(timeout: 5))
		sheet.popUpButtons["closeAccount.destination"].click()
		app.menuItems["Checking"].click()
		sheet.buttons["closeAccount.close"].click()
		XCTAssertTrue(sheet.waitForDisappearance())
		XCTAssertTrue(waitUntil { savings.label.contains("balance 0.00") }, savings.label)
		attachScreenshot(of: app, named: "accounts-after-close")
	}

	@MainActor
	func testPayeesOpenFromTheAccountsScreen() {
		let app = openAccounts()
		app.buttons["accounts.payees"].click()
		let sheet = app.descendants(matching: .any)["sheet.payees"]
		XCTAssertTrue(sheet.waitForExistence(timeout: 5))
		XCTAssertTrue(sheet.staticTexts["Fernwood Market"].waitForExistence(timeout: 5))
		attachScreenshot(of: app, named: "payees")
		sheet.buttons["Done"].click()
		XCTAssertTrue(sheet.waitForDisappearance())
	}

	@MainActor
	private func openChecking() -> XCUIApplication {
		let app = XCUIApplication.veritas(stub: "open")
		XCTAssertTrue(app.buttons["envelope.groceries.budgeted"].waitForExistence(timeout: 10))
		app.sidebar("account.checking").click()
		XCTAssertTrue(app.screen("register").waitForExistence(timeout: 5))
		return app
	}

	@MainActor
	private func openAccounts() -> XCUIApplication {
		let app = XCUIApplication.veritas(stub: "open")
		XCTAssertTrue(app.buttons["envelope.groceries.budgeted"].waitForExistence(timeout: 10))
		app.typeKey("2", modifierFlags: .command)
		XCTAssertTrue(app.screen("accounts").waitForExistence(timeout: 5))
		return app
	}

	private func text(_ element: XCUIElement) -> String {
		element.label.isEmpty ? (element.value as? String ?? "") : element.label
	}

	private func waitUntil(_ condition: @escaping () -> Bool) -> Bool {
		let expectation = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: nil)
		return XCTWaiter().wait(for: [expectation], timeout: 5) == .completed
	}
}
