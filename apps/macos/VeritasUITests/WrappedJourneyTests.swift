import XCTest

/// Wrapped against the stub (R-WRAP-1, R-WRAP-2, design/wrapped-editorial.md):
/// the issue turns spread by spread, the contents turn straight to a spread,
/// Hide amounts swaps amounts for shares, and each export reaches a save panel.
final class WrappedJourneyTests: XCTestCase {
	override func setUp() {
		continueAfterFailure = false
	}

	@MainActor
	func testTheIssueTurnsSpreadBySpread() {
		let app = openWrapped()
		XCTAssertFalse(app.buttons["wrapped.previous"].isEnabled, "the cover")
		app.buttons["wrapped.next"].click()
		XCTAssertTrue(app.descendants(matching: .any)["wrapped.spread.ledger"].waitForExistence(timeout: 5))
		attachScreenshot(of: app, named: "wrapped-ledger")
		app.typeKey(.rightArrow, modifierFlags: [])
		XCTAssertTrue(app.descendants(matching: .any)["wrapped.spread.categories+groups"].waitForExistence(timeout: 5))
		app.typeKey(.leftArrow, modifierFlags: [])
		XCTAssertTrue(app.descendants(matching: .any)["wrapped.spread.ledger"].waitForExistence(timeout: 5))
	}

	@MainActor
	func testTheContentsTurnStraightToASpread() {
		let app = openWrapped()
		let entry = app.descendants(matching: .any)["wrapped.contents.4"]
		XCTAssertTrue(entry.waitForExistence(timeout: 5))
		entry.click()
		XCTAssertTrue(app.descendants(matching: .any)["wrapped.spread.shape"].waitForExistence(timeout: 5))
		let columns = app.descendants(matching: .any)["wrapped.columns"]
		XCTAssertTrue(columns.waitForExistence(timeout: 5))
		XCTAssertTrue(columns.label.contains("January, $5,400.00; February, $4,980.00; March, $8,120.00"), columns.label)
		attachScreenshot(of: app, named: "wrapped-shape")
		app.descendants(matching: .any)["wrapped.hideAmounts"].click()
		XCTAssertTrue(waitUntil { columns.label.contains("January, 8.8% of total out") }, columns.label)
		XCTAssertFalse(columns.label.contains("$"), columns.label)
		attachScreenshot(of: app, named: "wrapped-shape-hidden")
	}

	@MainActor
	func testHideAmountsShowsShares() {
		let app = openWrapped()
		app.buttons["wrapped.next"].click()
		let headline = app.staticTexts.matching(identifier: "wrapped.headline").firstMatch
		XCTAssertTrue(headline.waitForExistence(timeout: 5))
		XCTAssertTrue(text(headline).hasPrefix("$84,210.00 came in."), text(headline))
		app.descendants(matching: .any)["wrapped.hideAmounts"].click()
		XCTAssertTrue(waitUntil { self.text(headline) == "72.9% of what came in went out." }, text(headline))
		attachScreenshot(of: app, named: "wrapped-hidden")
	}

	@MainActor
	func testTheYearPickerListsOnlyYearsWithData() {
		let app = openWrapped()
		let picker = app.descendants(matching: .any)["wrapped.yearPicker"]
		XCTAssertTrue(picker.waitForExistence(timeout: 5))
		picker.click()
		XCTAssertTrue(app.menuItems["2026 so far"].waitForExistence(timeout: 5))
		XCTAssertTrue(app.menuItems["2025"].exists)
		XCTAssertFalse(app.menuItems["2024"].exists, "the core lists 2025 as the first year with data")
		app.menuItems["2025"].click()
		let headline = app.staticTexts.matching(identifier: "wrapped.headline").firstMatch
		XCTAssertTrue(waitUntil { self.text(headline) == "2025" }, text(headline))
	}

	@MainActor
	func testTheSpreadExportOpensASavePanel() {
		let app = openWrapped()
		export("wrapped.export.spread", in: app)
		assertSavePanel(named: "Wrapped 2026 spread 1.png", in: app)
	}

	@MainActor
	func testTheRedactedPDFExportOpensASavePanel() {
		let app = openWrapped()
		export("wrapped.export.pdf.redacted", in: app)
		assertSavePanel(named: "Wrapped 2026 without amounts.pdf", in: app)
	}

	/// The stub's Wrapped is 2026's, so these journeys expect today to fall in 2026.
	@MainActor
	private func openWrapped() -> XCUIApplication {
		let app = XCUIApplication.veritas(stub: "open")
		XCTAssertTrue(app.buttons["envelope.groceries.budgeted"].waitForExistence(timeout: 10))
		app.sidebar("insights").click()
		let wrapped = app.sidebar("wrapped")
		XCTAssertTrue(wrapped.waitForExistence(timeout: 5))
		wrapped.click()
		XCTAssertTrue(app.screen("wrapped").waitForExistence(timeout: 5))
		XCTAssertTrue(app.descendants(matching: .any)["wrapped.spread.cover+contents"].waitForExistence(timeout: 5))
		return app
	}

	@MainActor
	private func export(_ item: String, in app: XCUIApplication) {
		let menu = app.descendants(matching: .any)["wrapped.export"]
		XCTAssertTrue(menu.waitForExistence(timeout: 5))
		XCTAssertTrue(waitUntil { menu.isEnabled })
		menu.click()
		let choice = app.menuItems.matching(identifier: item).firstMatch
		XCTAssertTrue(choice.waitForExistence(timeout: 5))
		choice.click()
	}

	@MainActor
	private func assertSavePanel(named name: String, in app: XCUIApplication) {
		let panel = app.sheets.firstMatch
		XCTAssertTrue(panel.waitForExistence(timeout: 10), "the save panel opens on the window")
		let field = panel.textFields["saveAsNameTextField"]
		XCTAssertTrue(field.waitForExistence(timeout: 5))
		XCTAssertEqual(field.value as? String, name)
		attachScreenshot(of: app, named: name)
		panel.buttons["Cancel"].click()
		XCTAssertTrue(panel.waitForDisappearance())
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
