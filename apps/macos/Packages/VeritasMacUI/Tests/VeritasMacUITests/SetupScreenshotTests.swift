import AppKit
import SwiftUI
import Testing
import VeritasKit
@testable import VeritasMacUI

/// Renders first launch and every new-budget step, light and dark, into
/// target/screenshots for design review (§5.3). Offscreen, so it needs no UI
/// automation permission.
@MainActor
struct SetupScreenshotTests {
	static let output = URL(filePath: #filePath)
		.deletingLastPathComponent()
		.appending(path: "../../../../../../target/screenshots/new-budget")
		.standardizedFileURL

	@Test func firstLaunch() throws {
		try render(EmptyStateView(isWorking: false, newBudget: {}, importActualZip: {}).frame(width: 1028, height: 640).background(.surfaceWindow), named: "1-first-launch")
	}

	@Test func basics() async throws {
		let setup = await setupAt(.basics)
		try render(NewBudgetSheet(setup: setup) { _ in }, named: "2-basics")
	}

	@Test func accounts() async throws {
		let setup = await setupAt(.accounts)
		let rows: [(String, String, AccountType)] = [
			("Checking", "2,140.18", .checking),
			("Visa", "612.33", .creditCard),
			("Brokerage", "24,310.55", .investment),
			("Car loan", "9,800.00", .loan)
		]
		for (index, row) in rows.enumerated() {
			if index > 0 { setup.addAccount() }
			setup.accounts[index].name = row.0
			setup.accounts[index].balance = row.1
			setup.accounts[index].type = row.2
		}
		try render(NewBudgetSheet(setup: setup) { _ in }, named: "3-accounts")
	}

	@Test func accountsWithAProblem() async throws {
		let setup = await setupAt(.accounts)
		setup.accounts[0].name = "Checking"
		setup.accounts[0].balance = "2,140.18+"
		_ = await setup.advance()
		try render(NewBudgetSheet(setup: setup) { _ in }, named: "4-accounts-problem")
	}

	@Test func paycheckSavings() async throws {
		let setup = await setupAt(.paycheckSavings)
		setup.hasRetirementPlan = true
		try render(NewBudgetSheet(setup: setup) { _ in }, named: "5-401k-and-hsa")
	}

	private func setupAt(_ step: NewBudgetSetup.Step) async -> NewBudgetSetup {
		let setup = NewBudgetSetup(library: StubLibrary(readOnly: nil, importError: nil), preferredCurrencyCode: "USD")
		await setup.loadCurrencies()
		setup.name = "Household"
		while setup.step != step {
			_ = await setup.advance()
		}
		return setup
	}

	private func render(_ view: some View, named name: String) throws {
		try FileManager.default.createDirectory(at: Self.output, withIntermediateDirectories: true)
		for (appearance, suffix) in [(NSAppearance.Name.aqua, "light"), (.darkAqua, "dark")] {
			let host = NSHostingView(rootView: view.tint(Color.accent).background(.surfaceRaised))
			host.frame.size = host.fittingSize
			let window = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
			window.appearance = NSAppearance(named: appearance)
			window.contentView = host
			host.layoutSubtreeIfNeeded()
			let bitmap = try #require(host.bitmapImageRepForCachingDisplay(in: host.bounds))
			host.cacheDisplay(in: host.bounds, to: bitmap)
			let png = try #require(bitmap.representation(using: .png, properties: [:]))
			try png.write(to: Self.output.appending(path: "\(name)-\(suffix).png"))
		}
	}
}
