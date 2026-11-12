import XCTest

/// Launch, the empty state, import, then the sidebar and every global shortcut.
final class ShellJourneyTests: XCTestCase {
	override func setUp() {
		continueAfterFailure = false
	}

	@MainActor
	func testFirstLaunchShowsTheEmptyStateWithImportFromActual() {
		let app = XCUIApplication.veritas(stub: "empty")
		XCTAssertTrue(app.staticTexts["No budget open"].waitForExistence(timeout: 10))
		XCTAssertTrue(app.buttons["importFromActual"].isEnabled)
		XCTAssertFalse(app.sidebar("budget").exists)
		attachScreenshot(of: app, named: "empty-state")
	}

	@MainActor
	func testImportThenNavigateWithTheSidebarAndShortcuts() {
		let app = XCUIApplication.veritas(stub: "empty")
		let importButton = app.buttons["importFromActual"]
		XCTAssertTrue(importButton.waitForExistence(timeout: 10))
		importButton.click()

		XCTAssertTrue(app.screen("budget").waitForExistence(timeout: 10))
		XCTAssertTrue(app.sidebar("account.checking").exists)
		attachScreenshot(of: app, named: "budget")

		app.sidebar("bills").click()
		XCTAssertTrue(app.screen("bills").waitForExistence(timeout: 5))
		app.sidebar("account.checking").click()
		XCTAssertTrue(app.screen("account:checking").waitForExistence(timeout: 5))
		app.sidebar("reports").click()
		XCTAssertTrue(app.sidebar("netProfit").waitForExistence(timeout: 5))
		app.sidebar("netProfit").click()
		XCTAssertTrue(app.screen("netProfit").waitForExistence(timeout: 5))
		attachScreenshot(of: app, named: "net-profit")
		app.sidebar("settings").click()
		XCTAssertTrue(app.screen("settings").waitForExistence(timeout: 5))

		let digits = ["1": "budget", "2": "accounts", "3": "bills", "4": "goals", "5": "reports", "6": "insights"]
		for (digit, screen) in digits.sorted(by: { $0.key < $1.key }) {
			app.typeKey(digit, modifierFlags: .command)
			XCTAssertTrue(app.screen(screen).waitForExistence(timeout: 5), "⌘\(digit) → \(screen)")
		}
	}

	@MainActor
	func testCommandPaletteFindsAScreenAndAnAccount() {
		let app = XCUIApplication.veritas(stub: "open")
		XCTAssertTrue(app.screen("budget").waitForExistence(timeout: 10))
		app.screen("budget").click()

		app.typeKey("k", modifierFlags: .command)
		let field = app.textFields["palette.field"]
		XCTAssertTrue(field.waitForExistence(timeout: 5))
		field.typeText("sank\r")
		XCTAssertTrue(app.screen("sankey").waitForExistence(timeout: 5))
		XCTAssertTrue(field.waitForDisappearance())

		app.buttons["toolbar.goToAnything"].click()
		XCTAssertTrue(field.waitForExistence(timeout: 5))
		field.typeText("card")
		attachScreenshot(of: app, named: "palette")
		app.typeKey(.downArrow, modifierFlags: [])
		app.typeKey(.upArrow, modifierFlags: [])
		app.typeKey(.return, modifierFlags: [])
		XCTAssertTrue(app.screen("account:card").waitForExistence(timeout: 5))

		app.typeKey("k", modifierFlags: .command)
		XCTAssertTrue(field.waitForExistence(timeout: 5))
		app.typeKey(.escape, modifierFlags: [])
		XCTAssertTrue(field.waitForDisappearance())

		app.typeKey("k", modifierFlags: .command)
		XCTAssertTrue(field.waitForExistence(timeout: 5))
		field.typeText("move\r")
		XCTAssertTrue(app.descendants(matching: .any)["sheet.moveMoney"].waitForExistence(timeout: 5))
	}

	@MainActor
	func testAddTransactionFindAndDenseOpenTheirPlaceholders() {
		let app = XCUIApplication.veritas(stub: "open")
		XCTAssertTrue(app.screen("budget").waitForExistence(timeout: 10))
		app.screen("budget").click()

		app.typeKey("n", modifierFlags: .command)
		let sheet = app.descendants(matching: .any)["sheet.addTransaction"]
		XCTAssertTrue(sheet.waitForExistence(timeout: 5))
		XCTAssertTrue(app.staticTexts["Add transaction"].exists)
		app.typeKey(.escape, modifierFlags: [])
		XCTAssertTrue(sheet.waitForDisappearance())

		app.typeKey("f", modifierFlags: .command)
		let find = app.textFields["findBar.field"]
		XCTAssertTrue(find.waitForExistence(timeout: 5))
		find.typeText("gro")
		XCTAssertEqual(find.value as? String, "gro")
		app.typeKey(.escape, modifierFlags: [])
		XCTAssertTrue(find.waitForDisappearance())

		let dense = app.descendants(matching: .any)["toolbar.dense"]
		XCTAssertTrue(dense.exists)
		XCTAssertEqual(dense.value as? Int, 0)
		app.typeKey("d", modifierFlags: .command)
		XCTAssertEqual(dense.value as? Int, 1)

		app.typeKey("3", modifierFlags: .command)
		XCTAssertTrue(app.screen("bills").waitForExistence(timeout: 5))
		XCTAssertFalse(dense.exists)
	}
}
