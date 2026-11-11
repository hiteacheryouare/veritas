import XCTest

/// R-IO-7, R-BUD-2, R-REL-2, R-REL-6 banners, and the alerts from contracts/errors.md.
final class ReadOnlyAndErrorTests: XCTestCase {
	override func setUp() {
		continueAfterFailure = false
	}

	@MainActor
	func testEveryReadOnlyReasonShowsItsBanner() {
		let expected = [
			"newerActualVersion": "A newer version of Actual saved this budget",
			"trackingBudget": "This is a tracking budget",
			"openElsewhere": "This budget is open in another window",
			"auditFailed": "The integrity check found problems",
			"newerVeritasSchema": "A newer version of Veritas changed this budget"
		]
		for (kind, opening) in expected {
			let app = XCUIApplication.veritas(stub: "open", ["-VeritasStubReadOnly", kind])
			let banner = app.descendants(matching: .any)["readOnlyBanner"]
			XCTAssertTrue(banner.waitForExistence(timeout: 10), kind)
			let message = banner.staticTexts.element(matching: NSPredicate(format: "value BEGINSWITH %@", opening))
			XCTAssertTrue(message.exists, kind)
			app.terminate()
		}
	}

	@MainActor
	func testTheIntegrityBannerOpensTheReport() {
		let app = XCUIApplication.veritas(stub: "open", ["-VeritasStubReadOnly", "auditFailed"])
		let show = app.buttons["Show report"]
		XCTAssertTrue(show.waitForExistence(timeout: 10))
		attachScreenshot(of: app, named: "read-only-banner")
		show.click()
		XCTAssertTrue(app.descendants(matching: .any)["sheet.integrityReport"].waitForExistence(timeout: 5))
	}

	@MainActor
	func testAnOlderActualFileSaysToUpgradeInActualFirst() {
		let app = XCUIApplication.veritas(stub: "empty", ["-VeritasStubImportError", "upgradeInActualFirst"])
		XCTAssertTrue(app.buttons["importFromActual"].waitForExistence(timeout: 10))
		app.buttons["importFromActual"].click()
		XCTAssertTrue(app.staticTexts["Upgrade this budget in Actual first"].waitForExistence(timeout: 5))
		XCTAssertTrue(app.staticTexts["Open this budget in Actual 26.11.0 or newer, then export it again."].exists)
		app.buttons["Close"].click()
		XCTAssertTrue(app.staticTexts["No budget open"].waitForExistence(timeout: 5))
	}

	@MainActor
	func testAFailedAuditDuringImportOpensTheIntegrityReport() {
		let app = XCUIApplication.veritas(stub: "empty", ["-VeritasStubImportError", "auditFailed"])
		XCTAssertTrue(app.buttons["importFromActual"].waitForExistence(timeout: 10))
		app.buttons["importFromActual"].click()
		XCTAssertTrue(app.descendants(matching: .any)["sheet.integrityReport"].waitForExistence(timeout: 5))
		XCTAssertTrue(app.staticTexts["No budget open"].exists)
	}

	@MainActor
	func testAStorageFailureOffersToTryAgain() {
		let app = XCUIApplication.veritas(stub: "empty", ["-VeritasStubImportError", "storage"])
		XCTAssertTrue(app.buttons["importFromActual"].waitForExistence(timeout: 10))
		app.buttons["importFromActual"].click()
		XCTAssertTrue(app.staticTexts["Couldn't import that budget"].waitForExistence(timeout: 5))
		XCTAssertTrue(app.buttons["Try again"].exists)
	}

	@MainActor
	func testABugOffersToCopyItsDetails() {
		let app = XCUIApplication.veritas(stub: "empty", ["-VeritasStubImportError", "internal"])
		XCTAssertTrue(app.buttons["importFromActual"].waitForExistence(timeout: 10))
		app.buttons["importFromActual"].click()
		XCTAssertTrue(app.staticTexts["Veritas hit a bug"].waitForExistence(timeout: 5))
		XCTAssertTrue(app.buttons["Copy details"].exists)
	}
}
