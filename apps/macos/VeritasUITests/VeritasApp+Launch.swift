import XCTest

extension XCUIApplication {
	/// Launches against the in-app stub instead of the core, with no restored windows.
	static func veritas(stub: String, _ extra: [String] = []) -> XCUIApplication {
		let app = XCUIApplication()
		app.launchArguments = ["-ApplePersistenceIgnoreState", "YES", "-VeritasStub", stub] + extra
		app.launch()
		return app
	}

	func screen(_ id: String) -> XCUIElement {
		descendants(matching: .any)["screen.\(id)"]
	}

	func sidebar(_ id: String) -> XCUIElement {
		descendants(matching: .any)["sidebar.\(id)"]
	}
}

extension XCUIElement {
	func waitForDisappearance(timeout: TimeInterval = 5) -> Bool {
		let gone = XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"), object: self)
		return XCTWaiter().wait(for: [gone], timeout: timeout) == .completed
	}
}

extension XCTestCase {
	@MainActor
	func attachScreenshot(of app: XCUIApplication, named name: String) {
		let attachment = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
		attachment.name = name
		attachment.lifetime = .keepAlways
		add(attachment)
	}
}
