import XCTest

/// The Rules page against the stub (R-RULE-2, R-RULE-3): the sentence list,
/// the editor, Apply to existing, and the rules Veritas only shows.
final class RulesJourneyTests: XCTestCase {
	override func setUp() {
		continueAfterFailure = false
	}

	@MainActor
	func testRulesOpenFromTheSidebarUnderBills() {
		let app = openRules()
		XCTAssertTrue(app.staticTexts["Before"].exists)
		XCTAssertTrue(app.staticTexts["Default"].exists)
		XCTAssertTrue(app.staticTexts["After"].exists)
		let amazon = app.descendants(matching: .any)["rule.amazon"]
		XCTAssertTrue(amazon.staticTexts["When imported payee contains \u{201C}AMZN\u{201D}"].exists)
		XCTAssertTrue(amazon.staticTexts["set payee to Amazon"].exists)
		attachScreenshot(of: app, named: "rules")
	}

	@MainActor
	func testCreatingARule() {
		let app = openRules()
		app.buttons["rules.new"].click()
		let sheet = app.descendants(matching: .any)["sheet.ruleEditor"]
		XCTAssertTrue(sheet.waitForExistence(timeout: 5))
		let text = sheet.textFields["ruleEditor.condition.0.value"]
		text.click()
		text.typeText("NETFLIX")
		sheet.popUpButtons["ruleEditor.action.0.value"].click()
		menuItem(app, "Hobbies")?.click()
		attachScreenshot(of: app, named: "new-rule")
		sheet.buttons["ruleEditor.save"].click()
		XCTAssertTrue(sheet.waitForDisappearance())
		let created = app.descendants(matching: .any)["rule.new-rule-1"]
		XCTAssertTrue(created.waitForExistence(timeout: 5))
		XCTAssertTrue(created.staticTexts["When imported payee contains \u{201C}NETFLIX\u{201D}"].exists)
		XCTAssertTrue(created.staticTexts["set category to Hobbies"].exists)
	}

	@MainActor
	func testAnEmptyConditionIsRefusedBesideTheSentence() {
		let app = openRules()
		app.buttons["rules.new"].click()
		let sheet = app.descendants(matching: .any)["sheet.ruleEditor"]
		XCTAssertTrue(sheet.waitForExistence(timeout: 5))
		sheet.popUpButtons["ruleEditor.action.0.value"].click()
		menuItem(app, "Hobbies")?.click()
		sheet.buttons["ruleEditor.save"].click()
		XCTAssertTrue(sheet.descendants(matching: .any)["ruleEditor.problem"].waitForExistence(timeout: 5))
		XCTAssertTrue(sheet.exists, "the editor stays open")
	}

	@MainActor
	func testEditingARule() {
		let app = openRules()
		openMenu(app, rule: "amazon", item: "Edit…")
		let sheet = app.descendants(matching: .any)["sheet.ruleEditor"]
		XCTAssertTrue(sheet.waitForExistence(timeout: 5))
		let text = sheet.textFields["ruleEditor.condition.0.value"]
		XCTAssertEqual(text.value as? String, "AMZN")
		text.click()
		text.typeKey("a", modifierFlags: .command)
		text.typeText("AMAZON")
		sheet.buttons["ruleEditor.addAction"].click()
		XCTAssertTrue(sheet.popUpButtons["ruleEditor.action.2.kind"].waitForExistence(timeout: 5))
		attachScreenshot(of: app, named: "edit-rule")
		sheet.buttons["ruleEditor.action.2.remove"].click()
		sheet.buttons["ruleEditor.save"].click()
		XCTAssertTrue(sheet.waitForDisappearance())
		let amazon = app.descendants(matching: .any)["rule.amazon"]
		XCTAssertTrue(amazon.staticTexts["When imported payee contains \u{201C}AMAZON\u{201D}"].waitForExistence(timeout: 5))
	}

	@MainActor
	func testApplyToExistingLeavesTheLockedRowUnticked() {
		let app = openRules()
		openMenu(app, rule: "amazon", item: "Apply to Existing…")
		let sheet = app.descendants(matching: .any)["sheet.applyRule"]
		XCTAssertTrue(sheet.waitForExistence(timeout: 5))
		let locked = sheet.checkBoxes["applyRule.row.tx-reconciled.tick"]
		XCTAssertEqual(locked.value as? Int, 0, "a reconciled row starts unticked")
		XCTAssertEqual(sheet.checkBoxes["applyRule.row.tx-1.tick"].value as? Int, 1)
		XCTAssertTrue(sheet.descendants(matching: .any)["applyRule.row.tx-reconciled.locked"].exists)
		XCTAssertTrue(sheet.staticTexts["applyRule.row.tx-reconciled.reconciledWarning"].exists)
		XCTAssertTrue(sheet.staticTexts["No category"].exists, "an uncategorized row reads No category, not nothing")
		let apply = sheet.buttons["applyRule.apply"]
		XCTAssertEqual(apply.label, "Apply to 2")
		attachScreenshot(of: app, named: "apply-to-existing")
		locked.click()
		XCTAssertEqual(locked.value as? Int, 1, "it stays pickable")
		XCTAssertEqual(apply.label, "Apply to 3")
		apply.click()
		XCTAssertTrue(sheet.waitForDisappearance())
	}

	@MainActor
	func testApplyToExistingFromTheEditorPreviewsTheDraft() {
		let app = openRules()
		openMenu(app, rule: "amazon", item: "Edit…")
		let editor = app.descendants(matching: .any)["sheet.ruleEditor"]
		XCTAssertTrue(editor.waitForExistence(timeout: 5))
		editor.buttons["ruleEditor.applyToExisting"].click()
		let preview = app.descendants(matching: .any)["sheet.applyRule"]
		XCTAssertTrue(preview.waitForExistence(timeout: 5))
		preview.buttons["applyRule.cancel"].click()
		XCTAssertTrue(preview.waitForDisappearance())
		XCTAssertTrue(editor.exists, "the editor is still open with its draft")
	}

	@MainActor
	func testTheEditInActualRowIsReadOnly() {
		let app = openRules()
		let row = app.descendants(matching: .any)["rule.templated"]
		XCTAssertTrue(row.waitForExistence(timeout: 5))
		XCTAssertFalse(app.menuButtons["rule.templated.menu"].exists)
		XCTAssertTrue(row.staticTexts["rule.templated.actualOnly"].exists)
		row.doubleClick()
		XCTAssertFalse(app.descendants(matching: .any)["sheet.ruleEditor"].waitForExistence(timeout: 2))
		app.buttons["rule.templated.editInActual"].click()
		let reasons = app.staticTexts["rule.templated.reasons"]
		XCTAssertTrue(reasons.waitForExistence(timeout: 5))
		XCTAssertTrue((reasons.value as? String ?? reasons.label).contains("a template action"))
		attachScreenshot(of: app, named: "edit-in-actual")
	}

	@MainActor
	func testABillsRuleOpensItsBill() {
		let app = openRules()
		XCTAssertFalse(app.menuButtons["rule.rent-rule.menu"].exists)
		app.buttons["rule.rent-rule.openBill"].click()
		XCTAssertTrue(app.screen("bills").waitForExistence(timeout: 5))
	}

	@MainActor
	private func openRules() -> XCUIApplication {
		let app = XCUIApplication.veritas(stub: "open")
		XCTAssertTrue(app.buttons["envelope.groceries.budgeted"].waitForExistence(timeout: 10))
		app.sidebar("rules").click()
		XCTAssertTrue(app.screen("rules").waitForExistence(timeout: 5))
		XCTAssertTrue(app.descendants(matching: .any)["rule.amazon"].waitForExistence(timeout: 5))
		return app
	}

	@MainActor
	private func openMenu(_ app: XCUIApplication, rule: String, item: String) {
		let menu = app.menuButtons["rule.\(rule).menu"]
		XCTAssertTrue(menu.waitForExistence(timeout: 5))
		menu.click()
		menuItem(app, item)?.click()
	}

	/// An open menu's item is the hittable one; the menu bar can hold the same title.
	@MainActor
	private func menuItem(_ app: XCUIApplication, _ title: String) -> XCUIElement? {
		let items = app.menuItems.matching(NSPredicate(format: "title == %@", title))
		_ = items.firstMatch.waitForExistence(timeout: 5)
		return items.allElementsBoundByIndex.first { $0.isHittable }
	}
}
