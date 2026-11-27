import AppKit
import SwiftUI
import Testing
import VeritasCore
import VeritasDesign
import VeritasKit
@testable import VeritasMacUI

/// "Upgrade to Veritas features" on the stub budget, every step, light and
/// dark, into target/screenshots/upgrade.
@MainActor
struct UpgradeScreenshotTests {
	static let output = URL(filePath: #filePath)
		.deletingLastPathComponent()
		.appending(path: "../../../../../../target/screenshots/upgrade")
		.standardizedFileURL

	private func loaded(offering: Bool = false) async -> (WorkspaceModel, UpgradeModel) {
		let workspace = WorkspaceModel(library: StubLibrary(readOnly: nil, importError: nil), localSettings: InMemoryBudgetSettings(), format: MoneyFormat())
		await workspace.reopen(directory: StubLaunch.budgetDirectory)
		let model = UpgradeModel(workspace: workspace, offering: offering, today: "2026-11-22")
		await model.load()
		return (workspace, model)
	}

	@Test func everyStep() async throws {
		let (workspace, model) = await loaded(offering: true)
		try render(UpgradeSheet(model: model, format: workspace.format), named: "offer")
		model.step = .overview
		let overview = try DrawnText.texts(in: UpgradeSheet(model: model, format: workspace.format), size: CGSize(width: 560, height: 320))
		#expect(overview.contains("1 envelope made a goal"))
		#expect(overview.contains("1 category marked as debt or savings"))
		#expect(overview.contains(UpgradeModel.nothingChanges))
		try render(UpgradeSheet(model: model, format: workspace.format), named: "overview")
		model.step = .review
		#expect(model.ticked == ["goal:vacation"], "the debt kind needs the user's judgment, so it starts unticked")
		try render(UpgradeSheet(model: model, format: workspace.format), named: "review", size: CGSize(width: 560, height: 640))
		model.toggle(try #require(model.visible.first { $0.key == "kind:carloan" }))
		model.step = .confirm
		try render(UpgradeSheet(model: model, format: workspace.format), named: "confirm", size: CGSize(width: 560, height: 520))
	}

	@Test func applyThenRevertTheKindAsksFirst() async throws {
		let (_, model) = await loaded()
		model.toggle(try #require(model.visible.first { $0.key == "kind:carloan" }))
		#expect(await model.apply())
		#expect(model.applied.map(\.key).sorted() == ["goal:vacation", "kind:carloan"])
		await model.revert("kind:carloan")
		#expect(model.question?.id == "kind:carloan")
		await model.revert("kind:carloan", confirmed: true)
		#expect(model.applied.map(\.key) == ["goal:vacation"])
	}

	func render(_ view: some View, named name: String, size: CGSize? = nil, settle: TimeInterval = 0.4) throws {
		try FileManager.default.createDirectory(at: Self.output, withIntermediateDirectories: true)
		for (appearance, suffix) in [(NSAppearance.Name.aqua, "light"), (.darkAqua, "dark")] {
			let host = NSHostingView(rootView: view.tint(Color.accent).background(.surfaceWindow))
			host.frame.size = size ?? host.fittingSize
			let window = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
			window.appearance = NSAppearance(named: appearance)
			window.contentView = host
			host.layoutSubtreeIfNeeded()
			RunLoop.main.run(until: .now.addingTimeInterval(settle))
			if size == nil { host.frame.size = host.fittingSize }
			host.layoutSubtreeIfNeeded()
			let bitmap = try #require(host.bitmapImageRepForCachingDisplay(in: host.bounds))
			host.cacheDisplay(in: host.bounds, to: bitmap)
			let png = try #require(bitmap.representation(using: .png, properties: [:]))
			try png.write(to: Self.output.appending(path: "\(name)-\(suffix).png"))
		}
	}
}
