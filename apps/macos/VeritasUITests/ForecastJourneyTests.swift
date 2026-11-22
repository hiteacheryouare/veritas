import XCTest

/// Forecast against the stub (R-FC-1 to R-FC-4): the four horizons with their
/// bands, a what-if joining the compare, and applying next month's suggestions.
/// The stub's baseline grows from whatever today is, so the figures hold on any day.
final class ForecastJourneyTests: XCTestCase {
	override func setUp() {
		continueAfterFailure = false
	}

	@MainActor
	func testForecastShowsTheFourHorizonsWithTheirBands() {
		let app = openForecast()
		let expected: [(horizon: String, netWorth: String, band: String)] = [
			("3 months", "$26,240", "$25,330 to $26,720"),
			("6 months", "$28,160", "$26,630 to $28,970"),
			("10 months", "$30,720", "$28,470 to $31,910"),
			("12 months", "$32,000", "$29,430 to $33,360")
		]
		for (horizon, netWorth, band) in expected {
			horizonButton(horizon, in: app).click()
			XCTAssertTrue(element(app, "forecast.hero").waitForText(netWorth), "\(horizon): \(element(app, "forecast.hero").debugText)")
			let bandLine = element(app, "forecast.band")
			XCTAssertTrue(bandLine.waitForText(band), "\(horizon): \(bandLine.debugText)")
			XCTAssertTrue(bandLine.waitForText("80% band from 14 past months"), "FC-4's label shows with the range: \(bandLine.debugText)")
			XCTAssertTrue(element(app, "forecast.compare.baseline").waitForText(netWorth))
		}
		XCTAssertTrue(element(app, "forecast.chart").exists)
		attachScreenshot(of: app, named: "forecast")

		app.buttons["forecast.method"].click()
		let notes = element(app, "forecast.method.notes")
		XCTAssertTrue(notes.waitForExistence(timeout: 5), "FC-4's method opens on request")
		XCTAssertTrue(app.staticTexts["Revaluations, adjustments and starting balances are taken as zero."].exists)
		XCTAssertTrue(app.staticTexts["Past months are the Net worth report's month-end figures, revaluations and adjustments included."].exists, "FC-3: the chart's past says what it includes")
		attachScreenshot(of: app, named: "forecast-method", wholeScreen: true)
	}

	@MainActor
	func testAddingAWhatIfChangesTheCompare() {
		let app = openForecast()
		XCTAssertTrue(element(app, "forecast.compare.baseline").waitForText("$32,000"))
		app.buttons["forecast.addWhatIf"].click()
		let sheet = element(app, "sheet.newWhatIf")
		XCTAssertTrue(sheet.waitForExistence(timeout: 5))
		let name = sheet.textFields["whatIf.name"]
		name.click()
		name.typeText("Vacation")
		let amount = sheet.textFields["amountField"]
		amount.click()
		amount.typeKey("a", modifierFlags: .command)
		amount.typeText("2000")
		attachScreenshot(of: app, named: "new-what-if")
		sheet.buttons["whatIf.create"].click()
		XCTAssertTrue(sheet.waitForDisappearance())

		let whatIf = element(app, "forecast.compare.what-if-1")
		XCTAssertTrue(whatIf.waitForExistence(timeout: 5), "a new what-if joins the compare")
		XCTAssertTrue(whatIf.waitForText("Vacation"), whatIf.debugText)
		XCTAssertTrue(whatIf.waitForText("$30,000"), "the $2,000 lowers net worth at 12 months: \(whatIf.debugText)")
		XCTAssertTrue(whatIf.waitForText("\u{2212}$2,000"), "v18: the core's difference from the baseline: \(whatIf.debugText)")
		let accounts = element(app, "forecast.accounts")
		XCTAssertTrue(accounts.staticTexts["Vacation"].waitForExistence(timeout: 5), "the what-if gets a column in Accounts")
		XCTAssertTrue(accounts.staticTexts["\u{2212}$2,000"].exists, "Checking carries the difference")
		XCTAssertTrue(element(app, "forecast.compare.baseline").waitForText("$32,000"), "the baseline stays put")
		horizonButton("3 months", in: app).click()
		XCTAssertTrue(whatIf.waitForText("$24,240"), whatIf.debugText)
		attachScreenshot(of: app, named: "forecast-compare")

		app.buttons["forecast.compare.remove.what-if-1"].click()
		XCTAssertTrue(whatIf.waitForDisappearance())
		XCTAssertTrue(app.buttons["forecast.compare.add.what-if-1"].waitForExistence(timeout: 5), "it stays a what-if, out of the compare")
	}

	@MainActor
	func testApplyingSuggestionsBudgetsNextMonth() {
		let app = openForecast()
		let groceries = element(app, "forecast.suggestion.groceries")
		XCTAssertTrue(groceries.waitForExistence(timeout: 5))
		XCTAssertTrue(groceries.waitForText("Groceries $430, 6-month median"), groceries.debugText)
		XCTAssertTrue(element(app, "forecast.suggestion.gifts").waitForText("keeping $40"))
		let apply = app.buttons["forecast.applySuggestions"]
		apply.click()
		XCTAssertTrue(element(app, "forecast.suggestions.applied").waitForExistence(timeout: 5))
		attachScreenshot(of: app, named: "suggestions-applied")

		app.typeKey("1", modifierFlags: .command)
		let cell = app.buttons["envelope.groceries.budgeted"]
		XCTAssertTrue(cell.waitForExistence(timeout: 10))
		XCTAssertTrue(cell.waitForText("Budgeted 430.00"), "the stub shows one month's cells for every month: \(cell.label)")
		XCTAssertTrue(app.buttons["envelope.gas.budgeted"].waitForText("Budgeted 123.00"))
		XCTAssertTrue(app.buttons["envelope.gifts.budgeted"].waitForText("Budgeted 40.00"), "a keeping line writes nothing")
	}

	@MainActor
	private func openForecast() -> XCUIApplication {
		let app = XCUIApplication.veritas(stub: "open")
		XCTAssertTrue(app.buttons["envelope.groceries.budgeted"].waitForExistence(timeout: 10))
		app.typeKey("6", modifierFlags: .command)
		XCTAssertTrue(app.sidebar("forecast").waitForExistence(timeout: 5))
		app.sidebar("forecast").click()
		XCTAssertTrue(app.screen("forecast").waitForExistence(timeout: 5))
		return app
	}

	@MainActor
	private func horizonButton(_ title: String, in app: XCUIApplication) -> XCUIElement {
		let button = element(app, "forecast.horizon").radioButtons[title]
		XCTAssertTrue(button.waitForExistence(timeout: 5), title)
		return button
	}

	@MainActor
	private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
		app.descendants(matching: .any)[id]
	}
}

private extension XCUIElement {
	/// A combined element keeps its text in `label` or `value`, depending on the element.
	var debugText: String {
		"\(label) \(value as? String ?? "")"
	}

	func waitForText(_ text: String, timeout: TimeInterval = 5) -> Bool {
		let predicate = NSPredicate { object, _ in
			guard let element = object as? XCUIElement, element.exists else { return false }
			return element.label.contains(text) || (element.value as? String)?.contains(text) == true
		}
		return XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: predicate, object: self)], timeout: timeout) == .completed
	}
}
