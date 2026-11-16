import AppKit
import SwiftUI
import Testing
import VeritasCore
import VeritasDesign
import VeritasKit
@testable import VeritasMacUI

/// The Bills screen on the stub budget, light and dark, into target/screenshots/bills.
@MainActor
struct BillsScreenshotTests {
	static let output = URL(filePath: #filePath)
		.deletingLastPathComponent()
		.appending(path: "../../../../../../target/screenshots/bills")
		.standardizedFileURL
	static let size = CGSize(width: 1240, height: 900)

	private func loaded() async -> (WorkspaceModel, BillsModel) {
		let workspace = WorkspaceModel(library: StubLibrary(readOnly: nil, importError: nil), localSettings: InMemoryBudgetSettings(), format: MoneyFormat())
		await workspace.reopen(directory: StubLaunch.budgetDirectory)
		let model = BillsModel(workspace: workspace)
		await model.load()
		return (workspace, model)
	}

	@Test func screen() async throws {
		let (workspace, model) = await loaded()
		#expect(model.tray.map(\.billId) == ["car", "electric", "music", "paycheck"])
		try render(BillsScreen(workspace: workspace, model: model), named: "bills", size: Self.size)
	}

	@Test func payForm() async throws {
		let (workspace, model) = await loaded()
		try render(BillsScreen(workspace: workspace, model: model, paying: "electric"), named: "bills-paying", size: Self.size)
	}

	@Test func timeline() async throws {
		let (workspace, model) = await loaded()
		#expect(model.timelineMonths.map(\.month) == ["2026-11", "2026-12"])
		try render(BillsScreen(workspace: workspace, model: model, showsTimeline: true), named: "bills-timeline", size: Self.size)
	}

	@Test func monthlyEditor() async throws {
		let (_, model) = await loaded()
		await model.edit(try #require(model.bill("internet")))
		let editing = try #require(model.editing)
		#expect(editing.rule?.patterns.count == 2)
		try render(BillEditor(model: model, editing: editing), named: "editor-monthly", size: CGSize(width: 580, height: 760))
	}

	@Test func estimateEditor() async throws {
		let (_, model) = await loaded()
		await model.edit(try #require(model.bill("electric")))
		var editing = try #require(model.editing)
		editing.draft.autoPost = true
		try render(BillEditor(model: model, editing: editing), named: "editor-estimate", size: CGSize(width: 580, height: 760))
	}

	@Test func customRuleEditor() async throws {
		let (_, model) = await loaded()
		await model.edit(try #require(model.bill("rent")))
		let editing = try #require(model.editing)
		try render(BillEditor(model: model, editing: editing), named: "editor-custom-rule", size: CGSize(width: 580, height: 760))
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
