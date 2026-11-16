import XCTest

/// Bills (R-BILL-1 to R-BILL-4) against the stub's bills, after design/mockups/bills.html.
final class BillsJourneyTests: XCTestCase {
	override func setUp() {
		continueAfterFailure = false
	}

	@MainActor
	func testOpeningBillsFromTheSidebar() {
		let app = XCUIApplication.veritas(stub: "open")
		XCTAssertTrue(app.screen("budget").waitForExistence(timeout: 10))
		app.sidebar("bills").click()
		XCTAssertTrue(app.screen("bills").waitForExistence(timeout: 5))
		XCTAssertTrue(element(app, "upcoming.car").waitForExistence(timeout: 5), "the missed car loan is in Upcoming")
		XCTAssertTrue(element(app, "bill.rent").exists)
		XCTAssertTrue(element(app, "bill.gym").exists, "paused bills are listed too")
		attachScreenshot(of: app, named: "bills")
	}

	@MainActor
	func testPaidWithAnEditedAmount() {
		let app = openBills()
		app.buttons["upcoming.electric.paid"].click()
		let markPaid = app.buttons["upcoming.electric.markPaid"]
		XCTAssertTrue(markPaid.waitForExistence(timeout: 5))
		let field = element(app, "upcoming.electric").textFields["amountField"]
		XCTAssertTrue(field.waitForExistence(timeout: 5))
		XCTAssertEqual(field.value as? String, "90.00", "opens on the scheduled amount")
		field.typeKey("a", modifierFlags: .command)
		field.typeText("93.41")
		attachScreenshot(of: app, named: "paying")
		markPaid.click()
		XCTAssertTrue(element(app, "upcoming.electric").waitForDisappearance(), "paid, so it leaves the tray")
		XCTAssertTrue(waitUntil { self.element(app, "bill.electric").label.contains("next Dec 15") }, element(app, "bill.electric").label)
	}

	@MainActor
	func testSkip() {
		let app = openBills()
		let music = element(app, "upcoming.music")
		XCTAssertTrue(music.waitForExistence(timeout: 5))
		app.buttons["upcoming.music.more"].click()
		app.menuItems["Skip"].click()
		XCTAssertTrue(music.waitForDisappearance(), "skipped to its next date, past the tray's week")
	}

	@MainActor
	func testCreatingABill() {
		let app = openBills()
		app.buttons["bills.new"].click()
		let sheet = element(app, "sheet.billEditor")
		XCTAssertTrue(sheet.waitForExistence(timeout: 5))
		let name = sheet.textFields["billEditor.name"]
		name.click()
		name.typeText("Water")
		let amount = sheet.textFields.matching(NSPredicate(format: "identifier IN %@", ["billEditor.amount", "amountField"])).firstMatch
		amount.click()
		amount.typeKey("a", modifierFlags: .command)
		amount.typeText("30")
		attachScreenshot(of: app, named: "new-bill")
		sheet.buttons["billEditor.save"].click()
		XCTAssertTrue(sheet.waitForDisappearance())
		let water = element(app, "bill.new-bill-1")
		XCTAssertTrue(water.waitForExistence(timeout: 5))
		XCTAssertTrue(water.label.hasPrefix("Water, 30.00"), water.label)
	}

	@MainActor
	func testPausingThenResuming() {
		let app = openBills()
		let rent = element(app, "bill.rent")
		XCTAssertTrue(rent.waitForExistence(timeout: 5))
		rent.rightClick()
		contextItem(app, "Pause")?.click()
		let resume = app.buttons["bill.rent.resume"]
		XCTAssertTrue(resume.waitForExistence(timeout: 5), "paused bills move to their own section with Resume")
		XCTAssertTrue(rent.label.contains("Paused"), rent.label)
		resume.click()
		XCTAssertTrue(resume.waitForDisappearance())
		XCTAssertTrue(waitUntil { rent.label.contains("next Dec 15") }, rent.label)
	}

	@MainActor
	func testAReadOnlyBillShowsEditInActual() {
		let app = openBills()
		let insurance = element(app, "bill.insurance")
		XCTAssertTrue(insurance.waitForExistence(timeout: 5))
		XCTAssertTrue(insurance.label.contains("Edit in Actual"), insurance.label)
		insurance.click()
		XCTAssertFalse(element(app, "sheet.billEditor").waitForExistence(timeout: 2), "a bill Veritas can't show doesn't open the editor")
		insurance.rightClick()
		XCTAssertNil(contextItem(app, "Edit…"))
		app.typeKey(.escape, modifierFlags: [])
	}

	@MainActor
	private func openBills() -> XCUIApplication {
		let app = XCUIApplication.veritas(stub: "open")
		XCTAssertTrue(app.screen("budget").waitForExistence(timeout: 10))
		app.typeKey("3", modifierFlags: .command)
		XCTAssertTrue(app.screen("bills").waitForExistence(timeout: 5))
		XCTAssertTrue(element(app, "upcoming.car").waitForExistence(timeout: 5))
		return app
	}

	private func element(_ app: XCUIApplication, _ identifier: String) -> XCUIElement {
		app.descendants(matching: .any)[identifier]
	}

	@MainActor
	private func contextItem(_ app: XCUIApplication, _ title: String) -> XCUIElement? {
		app.menuItems.matching(NSPredicate(format: "title == %@", title)).allElementsBoundByIndex.first { $0.isHittable }
	}

	private func waitUntil(_ condition: @escaping () -> Bool) -> Bool {
		let expectation = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: nil)
		return XCTWaiter().wait(for: [expectation], timeout: 5) == .completed
	}
}
