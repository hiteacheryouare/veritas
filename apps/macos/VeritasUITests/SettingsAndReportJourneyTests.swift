import XCTest

/// Open Budget, backups (R-REL-4), snapshot restore (R-REL-3), the Integrity
/// report (R-REL-2), and the Actual and Appearance settings.
final class SettingsAndReportJourneyTests: XCTestCase {
	override func setUp() {
		continueAfterFailure = false
	}

	@MainActor
	func testOpenBudgetListsBudgetsAndOpensOne() {
		let app = XCUIApplication.veritas(stub: "empty")
		XCTAssertTrue(app.buttons["newBudget"].waitForExistence(timeout: 10))
		app.typeKey("o", modifierFlags: .command)
		let row = app.descendants(matching: .any)["openBudget.household"]
		XCTAssertTrue(row.waitForExistence(timeout: 5))
		XCTAssertTrue(app.staticTexts["Untitled budget"].exists)
		attachScreenshot(of: app, named: "open-budget")
		app.buttons["openBudget.open"].click()
		XCTAssertTrue(app.screen("budget").waitForExistence(timeout: 10))
	}

	@MainActor
	func testChoosingABackupFolderAndBackingUpNow() {
		let app = openSettings()
		let lastBackup = app.staticTexts["settings.lastBackup"]
		XCTAssertTrue(lastBackup.waitForExistence(timeout: 5))
		XCTAssertEqual(lastBackup.value as? String, "Never")
		XCTAssertFalse(app.buttons["settings.backUpNow"].isEnabled)
		app.buttons["settings.chooseBackupFolder"].click()
		XCTAssertTrue(app.staticTexts["/stub/Backups"].waitForExistence(timeout: 5))
		app.buttons["settings.backUpNow"].click()
		XCTAssertTrue(waitUntil { (lastBackup.value as? String) != "Never" })
		attachScreenshot(of: app, named: "settings-backups")
	}

	@MainActor
	func testAFailedBackupRaisesTheBannerWithoutBlocking() {
		let app = openSettings(["-VeritasStubBackupProblem", "YES"])
		XCTAssertTrue(app.buttons["settings.chooseBackupFolder"].waitForExistence(timeout: 5))
		app.buttons["settings.chooseBackupFolder"].click()
		app.buttons["settings.backUpNow"].click()
		let banner = app.descendants(matching: .any)["backupBanner"]
		XCTAssertTrue(banner.waitForExistence(timeout: 5))
		attachScreenshot(of: app, named: "backup-failed")
		app.typeKey("3", modifierFlags: .command)
		XCTAssertTrue(app.screen("bills").waitForExistence(timeout: 5))
		XCTAssertTrue(banner.exists)
		banner.buttons["Show settings"].click()
		XCTAssertTrue(app.screen("settings").waitForExistence(timeout: 5))
	}

	@MainActor
	func testRestoringASnapshotAsksFirst() {
		let app = openSettings()
		let restore = app.buttons["settings.restore.hourly"]
		XCTAssertTrue(restore.waitForExistence(timeout: 5))
		restore.click()
		let confirm = app.buttons["Restore"]
		XCTAssertTrue(confirm.waitForExistence(timeout: 5))
		attachScreenshot(of: app, named: "restore-snapshot")
		confirm.click()
		XCTAssertTrue(app.buttons["settings.restore.restore"].waitForExistence(timeout: 5), "the restore's own snapshot is listed")
	}

	@MainActor
	func testTheIntegrityReportListsTheFindings() {
		let app = XCUIApplication.veritas(stub: "open", ["-VeritasStubReadOnly", "auditFailed", "-VeritasStubFindings", "YES"])
		let show = app.buttons["Show report"]
		XCTAssertTrue(show.waitForExistence(timeout: 10))
		show.click()
		let report = app.descendants(matching: .any)["sheet.integrityReport"]
		XCTAssertTrue(report.waitForExistence(timeout: 5))
		// Each finding reads as one element: its title, then where it is.
		let finding = report.staticTexts.matching(NSPredicate(format: "label BEGINSWITH %@ OR value BEGINSWITH %@", "Transfer with no other side", "Transfer with no other side")).firstMatch
		XCTAssertTrue(finding.waitForExistence(timeout: 5))
		attachScreenshot(of: app, named: "integrity-report")
		app.typeKey(.escape, modifierFlags: [])
		XCTAssertTrue(app.descendants(matching: .any)["sheet.integrityReport"].waitForDisappearance())
	}

	@MainActor
	func testTheActualAndAppearanceSections() {
		let app = openSettings()
		segment(app, "Actual").click()
		XCTAssertTrue(app.buttons["Import from Actual…"].waitForExistence(timeout: 5))
		XCTAssertTrue(app.buttons["Export to Actual…"].isEnabled)
		segment(app, "Appearance").click()
		let appearance = app.descendants(matching: .any)["settings.appearance"]
		XCTAssertTrue(appearance.waitForExistence(timeout: 5))
		appearance.radioButtons["Dark"].click()
		attachScreenshot(of: app, named: "settings-appearance-dark")
		appearance.radioButtons["System"].click()
	}

	@MainActor
	private func openSettings(_ extra: [String] = []) -> XCUIApplication {
		let app = XCUIApplication.veritas(stub: "open", extra)
		XCTAssertTrue(app.screen("budget").waitForExistence(timeout: 10))
		app.screen("budget").click()
		app.typeKey(",", modifierFlags: .command)
		XCTAssertTrue(app.screen("settings").waitForExistence(timeout: 5))
		return app
	}

	@MainActor
	private func segment(_ app: XCUIApplication, _ title: String) -> XCUIElement {
		app.toolbars.descendants(matching: .radioButton).matching(NSPredicate(format: "title == %@ OR label == %@", title, title)).firstMatch
	}

	private func waitUntil(_ condition: @escaping () -> Bool) -> Bool {
		let expectation = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: nil)
		return XCTWaiter().wait(for: [expectation], timeout: 5) == .completed
	}
}
