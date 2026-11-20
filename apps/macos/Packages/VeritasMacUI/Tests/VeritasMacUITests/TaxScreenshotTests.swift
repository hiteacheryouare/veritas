import AppKit
import SwiftUI
import Testing
import VeritasCore
import VeritasDesign
import VeritasKit
@testable import VeritasMacUI

/// The Tax summary on the stub budget, light and dark, into target/screenshots/tax.
@MainActor
struct TaxScreenshotTests {
	static let output = URL(filePath: #filePath)
		.deletingLastPathComponent()
		.appending(path: "../../../../../../target/screenshots/tax")
		.standardizedFileURL

	private func model() async -> (WorkspaceModel, TaxSummaryModel) {
		let workspace = WorkspaceModel(library: StubLibrary(readOnly: nil, importError: nil), localSettings: InMemoryBudgetSettings(), format: MoneyFormat())
		await workspace.reopen(directory: StubLaunch.budgetDirectory)
		let model = TaxSummaryModel(workspace: workspace, thisYear: 2026)
		await model.load()
		return (workspace, model)
	}

	@Test func bucketsAndAConflict() async throws {
		let (workspace, model) = await model()
		#expect(model.conflicts.map(\.transactionId) == ["tax-glasses"])
		try render(TaxScreen(workspace: workspace, model: model), named: "tax", size: CGSize(width: 900, height: 600))
	}

	@Test func expandedBuckets() async throws {
		let (workspace, model) = await model()
		model.toggle(.charitable)
		model.toggle(.medical)
		try render(TaxScreen(workspace: workspace, model: model), named: "tax-expanded", size: CGSize(width: 900, height: 820))
	}

	@Test func aYearWithoutConflicts() async throws {
		let (workspace, model) = await model()
		await model.previous()
		#expect(model.summary?.year == 2025)
		try render(TaxScreen(workspace: workspace, model: model), named: "tax-2025", size: CGSize(width: 900, height: 420))
	}

	@Test func anEmptyYear() async throws {
		let (workspace, model) = await model()
		await model.choose(2023)
		try render(TaxScreen(workspace: workspace, model: model), named: "tax-empty", size: CGSize(width: 900, height: 360))
	}

	func render(_ view: some View, named name: String, size: CGSize, settle: TimeInterval = 0.4) throws {
		try FileManager.default.createDirectory(at: Self.output, withIntermediateDirectories: true)
		for (appearance, suffix) in [(NSAppearance.Name.aqua, "light"), (.darkAqua, "dark")] {
			let host = NSHostingView(rootView: view.tint(Color.accent).background(.surfaceWindow))
			host.frame.size = size
			let window = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
			window.appearance = NSAppearance(named: appearance)
			window.contentView = host
			host.layoutSubtreeIfNeeded()
			RunLoop.main.run(until: .now.addingTimeInterval(settle))
			host.layoutSubtreeIfNeeded()
			let bitmap = try #require(host.bitmapImageRepForCachingDisplay(in: host.bounds))
			host.cacheDisplay(in: host.bounds, to: bitmap)
			let png = try #require(bitmap.representation(using: .png, properties: [:]))
			try png.write(to: Self.output.appending(path: "\(name)-\(suffix).png"))
		}
	}
}
