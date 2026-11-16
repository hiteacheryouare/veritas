import XCTest

/// The Tax summary against the stub (R-REP-8): years, a bucket's rows, the
/// conflict, and both CSV exports reaching a save panel.
final class TaxJourneyTests: XCTestCase {
	override func setUp() {
		continueAfterFailure = false
	}

	@MainActor
	func testTaxShowsTheBucketsAndTheConflict() {
		let app = openTax()
		XCTAssertTrue(app.buttons["tax.bucket.charitable"].waitForExistence(timeout: 5))
		XCTAssertTrue(app.buttons["tax.bucket.charitable"].label.hasPrefix("Charitable giving, 130.00"), app.buttons["tax.bucket.charitable"].label)
		let business = app.buttons["tax.bucket.business_expense"]
		XCTAssertTrue(business.label.hasPrefix("Business expenses, \u{2212}15.00"), "a negative total stays negative: \(business.label)")
		let conflicts = app.descendants(matching: .any)["tax.conflicts"]
		XCTAssertTrue(conflicts.exists)
		XCTAssertTrue(app.staticTexts["Needs a decision"].exists)
		XCTAssertTrue(conflicts.descendants(matching: .any)["tax.row.tax-glasses"].exists)
		attachScreenshot(of: app, named: "tax")
	}

	@MainActor
	func testChangingTheYearShowsThatYear() {
		let app = openTax()
		choose("2025", in: app)
		XCTAssertTrue(app.descendants(matching: .any)["tax.title"].waitForValueOrLabel("2025"))
		XCTAssertTrue(app.buttons["tax.bucket.interest_income"].label.hasPrefix("Interest income, 23.90"), app.buttons["tax.bucket.interest_income"].label)
		XCTAssertFalse(app.descendants(matching: .any)["tax.conflicts"].exists, "2025 has no conflicts")
		app.buttons["tax.previous"].click()
		XCTAssertTrue(app.descendants(matching: .any)["tax.empty"].waitForExistence(timeout: 5))
		attachScreenshot(of: app, named: "tax-2024")
		app.buttons["tax.next"].click()
		XCTAssertTrue(app.buttons["tax.bucket.charitable"].waitForExistence(timeout: 5))
		XCTAssertTrue(app.buttons["tax.bucket.charitable"].label.hasPrefix("Charitable giving, 100.00"), app.buttons["tax.bucket.charitable"].label)
	}

	@MainActor
	func testABucketOpensIntoItsRows() {
		let app = openTax()
		let medical = app.buttons["tax.bucket.medical"]
		XCTAssertTrue(medical.waitForExistence(timeout: 5))
		let pharmacy = app.descendants(matching: .any)["tax.row.tax-pharmacy"]
		XCTAssertFalse(pharmacy.exists)
		medical.click()
		XCTAssertTrue(pharmacy.waitForExistence(timeout: 5))
		XCTAssertTrue(app.descendants(matching: .any)["tax.row.tax-doctor"].exists)
		attachScreenshot(of: app, named: "tax-medical")
		medical.click()
		XCTAssertTrue(pharmacy.waitForDisappearance())
	}

	@MainActor
	func testTheSummaryExportOpensASavePanel() {
		let app = openTax()
		export("Summary CSV…", in: app)
		assertSavePanel(named: "veritas-tax-summary-2026.csv", in: app)
	}

	@MainActor
	func testTheDetailExportOpensASavePanel() {
		let app = openTax()
		export("Detail CSV…", in: app)
		assertSavePanel(named: "veritas-tax-detail-2026.csv", in: app)
	}

	/// The stub's tax data is 2026's, so each journey starts there whatever today is.
	@MainActor
	private func openTax() -> XCUIApplication {
		let app = XCUIApplication.veritas(stub: "open")
		XCTAssertTrue(app.buttons["envelope.groceries.budgeted"].waitForExistence(timeout: 10))
		app.typeKey("6", modifierFlags: .command)
		XCTAssertTrue(app.sidebar("tax").waitForExistence(timeout: 5))
		app.sidebar("tax").click()
		XCTAssertTrue(app.screen("tax").waitForExistence(timeout: 5))
		choose("2026", in: app)
		XCTAssertTrue(app.descendants(matching: .any)["tax.title"].waitForValueOrLabel("2026"))
		return app
	}

	@MainActor
	private func choose(_ year: String, in app: XCUIApplication) {
		let picker = app.popUpButtons["tax.yearPicker"]
		XCTAssertTrue(picker.waitForExistence(timeout: 5))
		picker.click()
		app.menuItems[year].click()
	}

	@MainActor
	private func export(_ item: String, in app: XCUIApplication) {
		let menu = app.descendants(matching: .any)["tax.export"]
		XCTAssertTrue(menu.waitForExistence(timeout: 5))
		menu.click()
		let choice = app.menuItems[item]
		XCTAssertTrue(choice.waitForExistence(timeout: 5))
		choice.click()
	}

	/// The panel shows the TAX-5 file name; Escape closes it without writing.
	@MainActor
	private func assertSavePanel(named name: String, in app: XCUIApplication) {
		let dialog = app.dialogs.firstMatch
		let panel = dialog.waitForExistence(timeout: 5) ? dialog : app.sheets.firstMatch
		XCTAssertTrue(panel.waitForExistence(timeout: 5), "a save panel opened")
		let field = panel.textFields.firstMatch
		XCTAssertTrue(field.waitForExistence(timeout: 5))
		XCTAssertEqual(field.value as? String, name)
		attachScreenshot(of: app, named: name, wholeScreen: true)
		panel.typeKey(.escape, modifierFlags: [])
		XCTAssertTrue(panel.waitForDisappearance())
	}
}

private extension XCUIElement {
	/// SwiftUI puts a combined element's text in `value` or `label`, depending on the element.
	func waitForValueOrLabel(_ text: String, timeout: TimeInterval = 5) -> Bool {
		let predicate = NSPredicate { object, _ in
			guard let element = object as? XCUIElement, element.exists else { return false }
			return element.label.contains(text) || (element.value as? String)?.contains(text) == true
		}
		return XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: predicate, object: self)], timeout: timeout) == .completed
	}
}
