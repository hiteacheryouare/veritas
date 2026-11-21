import XCTest

/// Wrapped against the stub (R-WRAP-1, R-WRAP-2): the story steps card by
/// card, Hide amounts swaps amounts for shares, and both image exports reach
/// a save panel.
final class WrappedJourneyTests: XCTestCase {
	override func setUp() {
		continueAfterFailure = false
	}

	@MainActor
	func testTheStoryStepsCardByCard() {
		let app = openWrapped()
		let title = app.staticTexts["wrapped.title"]
		XCTAssertTrue(title.waitForExistence(timeout: 5))
		XCTAssertEqual(text(title), "Your 2026 so far")
		XCTAssertFalse(app.buttons["wrapped.previous"].isEnabled, "the first card")
		app.buttons["wrapped.next"].click()
		XCTAssertTrue(waitUntil { self.text(title) == "Rent topped your spending." }, text(title))
		attachScreenshot(of: app, named: "wrapped-categories")
		app.typeKey(.leftArrow, modifierFlags: [])
		XCTAssertTrue(waitUntil { self.text(title) == "Your 2026 so far" }, text(title))
	}

	@MainActor
	func testHideAmountsShowsShares() {
		let app = openWrapped()
		app.buttons["wrapped.next"].click()
		let figure = app.staticTexts["wrapped.figure"]
		XCTAssertTrue(figure.waitForExistence(timeout: 5))
		XCTAssertEqual(text(figure), "$19,800.00")
		app.descendants(matching: .any)["wrapped.hideAmounts"].click()
		XCTAssertTrue(waitUntil { self.text(figure) == "36.1%" }, text(figure))
		attachScreenshot(of: app, named: "wrapped-hidden")
	}

	@MainActor
	func testTheRedactedImageExportOpensASavePanel() {
		let app = openWrapped()
		export("Image without amounts…", in: app)
		assertSavePanel(named: "Wrapped 2026 without amounts.png", in: app)
	}

	@MainActor
	func testTheImageExportOpensASavePanel() {
		let app = openWrapped()
		export("Image…", in: app)
		assertSavePanel(named: "Wrapped 2026.png", in: app)
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
		XCTAssertTrue(app.descendants(matching: .any)["wrapped.card.totals"].waitForExistence(timeout: 5))
		return app
	}

	@MainActor
	private func export(_ item: String, in app: XCUIApplication) {
		let menu = app.descendants(matching: .any)["wrapped.export"]
		XCTAssertTrue(menu.waitForExistence(timeout: 5))
		XCTAssertTrue(waitUntil { menu.isEnabled })
		menu.click()
		let choice = app.menuItems[item]
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
