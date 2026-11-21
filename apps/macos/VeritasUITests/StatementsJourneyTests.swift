import XCTest

/// Statements against the stub (R-REP-7): each statement opens as its page,
/// the period changes what it covers, and Export PDF asks where to save.
final class StatementsJourneyTests: XCTestCase {
	override func setUp() {
		continueAfterFailure = false
	}

	@MainActor
	func testEachStatementOpens() {
		let app = openStatements()
		let title = app.staticTexts["statements.title"]
		for (segment, heading, line) in [
			("Balance sheet", "Balance sheet", "statements.line.netWorth"),
			("Income statement", "Income statement", "statements.line.netIncome"),
			("Owner's equity", "Statement of owner's equity", "statements.line.closing"),
			("Cash flow", "Cash flow statement", "statements.line.netCashFlow")
		] {
			kind(app, segment).click()
			XCTAssertTrue(waitUntil { self.text(title) == heading }, "\(segment): \(text(title))")
			XCTAssertTrue(app.descendants(matching: .any)[line].waitForExistence(timeout: 5), line)
			XCTAssertTrue(text(app.staticTexts["statements.entity"]).contains("Unaudited. Obviously."))
			attachScreenshot(of: app, named: segment)
		}
	}

	@MainActor
	func testTheBalanceSheetStepsBackAMonthEnd() {
		let app = openStatements()
		let basis = app.staticTexts["statements.basis"]
		XCTAssertTrue(basis.waitForExistence(timeout: 5))
		let before = text(basis)
		XCTAssertTrue(before.hasPrefix("As of "), before)
		app.buttons["statements.previous"].click()
		XCTAssertTrue(waitUntil { self.text(basis) != before && self.text(basis).hasPrefix("As of ") }, text(basis))
		XCTAssertTrue(app.buttons["statements.next"].isEnabled, "a past date can step forward again")
		attachScreenshot(of: app, named: "balance-sheet-earlier")
	}

	@MainActor
	func testOwnersEquityCoversTheChosenYear() {
		let app = openStatements()
		kind(app, "Owner's equity").click()
		let basis = app.staticTexts["statements.basis"]
		XCTAssertTrue(waitUntil { !self.text(basis).hasPrefix("As of") }, text(basis))
		app.buttons["statements.period"].click()
		let year = app.descendants(matching: .any)["statements.range"].radioButtons["Year"]
		XCTAssertTrue(year.waitForExistence(timeout: 5))
		year.click()
		XCTAssertTrue(waitUntil { self.text(basis).hasPrefix("January to December") }, text(basis))
		let thisYear = text(basis)
		app.buttons["statements.previous"].click()
		XCTAssertTrue(waitUntil { self.text(basis) != thisYear && self.text(basis).hasPrefix("January to December") }, text(basis))
		attachScreenshot(of: app, named: "owners-equity-year")
	}

	@MainActor
	func testExportOpensASavePanel() {
		let app = openStatements()
		let export = app.buttons["statements.export"]
		XCTAssertTrue(export.waitForExistence(timeout: 5))
		XCTAssertTrue(waitUntil { export.isEnabled })
		export.click()
		let panel = app.sheets.firstMatch
		XCTAssertTrue(panel.waitForExistence(timeout: 10), "the save panel opens on the window")
		attachScreenshot(of: app, named: "export-panel")
		panel.buttons["Cancel"].click()
		XCTAssertTrue(panel.waitForDisappearance())
	}

	@MainActor
	private func openStatements() -> XCUIApplication {
		let app = XCUIApplication.veritas(stub: "open")
		XCTAssertTrue(app.buttons["envelope.groceries.budgeted"].waitForExistence(timeout: 10))
		app.sidebar("insights").click()
		let statements = app.sidebar("statements")
		XCTAssertTrue(statements.waitForExistence(timeout: 5))
		statements.click()
		XCTAssertTrue(app.screen("statements").waitForExistence(timeout: 5))
		XCTAssertTrue(app.descendants(matching: .any)["statements.paper"].waitForExistence(timeout: 5))
		return app
	}

	private func kind(_ app: XCUIApplication, _ title: String) -> XCUIElement {
		app.descendants(matching: .any)["statements.kind"].radioButtons[title]
	}

	/// SwiftUI puts a combined element's text in `value` rather than `label`.
	private func text(_ element: XCUIElement) -> String {
		element.label.isEmpty ? (element.value as? String ?? "") : element.label
	}

	private func waitUntil(_ condition: @escaping () -> Bool) -> Bool {
		let expectation = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: nil)
		return XCTWaiter().wait(for: [expectation], timeout: 5) == .completed
	}
}
